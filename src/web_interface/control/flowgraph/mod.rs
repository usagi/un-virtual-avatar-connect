//! Phase δ-6: Flowgraph 編集 GUI のための Control API。
//!
//! spec §9.5 に相当する read / write エンドポイント群。共通処理は [`util`]、ランタイム再構築は [`reload`]、
//! トリガーは [`trigger`]、フラグメント/ZIP は [`fragment_zip`]。
//!
//! ## パス表現
//!
//! URL 上は `/flowgraph/file/{fq:.*}` の形を採用し、`fq` は拡張子を除いた root 相対パス（`/` 区切り）。
//! 例: `chat-echo/main` ↔ `<flowgraph_dir>/chat-echo/main.flowgraph.toml`。
//! fq は `parse_port_ref` と同じ命名体系なので、ノード参照 (`chat-echo/main::node_id`) と共通化される。
//!
//! ## 安全設計
//!
//! [`util::fq_to_file_path`] で `<flowgraph_dir>/<fq>.flowgraph.toml` を組み立てた後、
//! 親ディレクトリを canonicalize して `<flowgraph_dir>` の配下であることを確認する（path traversal 防御）。
//! `..` や絶対パスは fq パース段階で拒否。

mod fragment_zip;
#[doc(hidden)]
pub mod purposes;
#[doc(hidden)]
pub use fragment_zip::{share_plan_snapshot, export_snapshot};
mod file_create;
mod write_lock;
mod file_revision;
mod file_backup;
mod file_replace;
pub mod file_recovery;
mod reload;
mod trigger;
mod util;

pub(crate) use reload::reload_runtime_with_state_snapshot_file;

const GRAPH_BODY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

fn parse_graph_body<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, actix_web::HttpResponse> {
	if body.len() > GRAPH_BODY_LIMIT_BYTES {
		return Err(util::err_json(actix_web::http::StatusCode::PAYLOAD_TOO_LARGE, "graph_request_too_large", "要求データが64 MiBを超えています"));
	}
	serde_json::from_slice(body).map_err(|error| util::err_json(actix_web::http::StatusCode::BAD_REQUEST, "invalid_graph_request", error))
}

use actix_web::web::{self, Data, Json};
use actix_web::{delete, get, post, put, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::flowgraph::loader::{parse_flowgraph_file, Diagnostic, FlowgraphFile};
use crate::flowgraph::quantity::{parse_unit, Quantity};
use crate::flowgraph::registry::registry;
use crate::flowgraph::{FlowgraphRuntime, PackageLockFile, PackageLockFileError, ProgramCommandError, SocketType, StateSnapshotFileError};
use crate::SharedState;

use util::{
	enrich_node_catalog_spec_json, ensure_within_root, err_json, flowgraph_dir, fq_to_file_path, make_backup, root_relative,
	spawn_os_opener,
};

// ============================================================================
// GET /flowgraph/node-catalog
// ============================================================================

#[derive(Debug, Serialize)]
pub struct NodeCatalogResponse {
	/// 登録されている全 feature の `NodeSpec`。feature 名でソート済み。
	pub specs: Vec<serde_json::Value>,
	/// 総件数（GUI のページング / 表示ヘルパ用）。
	pub count: usize,
}

#[get("/flowgraph/node-catalog")]
pub async fn get_node_catalog() -> impl Responder {
	// Phase φ-6: Trigger 表示用 `control_triggerable` を JSON に注入。
	// Phase ξ-5: Quantity ポートに `quantity_dim` / `quantity_unit_*` を付与（default から復元できる場合のみ）。
	let reg = registry();
	let specs = reg.all_specs();
	let values: Vec<serde_json::Value> = specs
		.iter()
		.map(|s| {
			let mut v = serde_json::to_value(s).unwrap();
			enrich_node_catalog_spec_json(reg, &mut v);
			v
		})
		.collect();
	HttpResponse::Ok().json(NodeCatalogResponse {
		count: values.len(),
		specs: values,
	})
}

#[derive(Debug, Deserialize)]
struct ParseUnitQuery {
	/// URL クエリで渡す単位文字列（例 `m%2Fs%5E2`）。空は dimensionless として valid。
	text: String,
}

#[derive(Debug, Deserialize)]
struct ParseSocketTypeQuery {
	/// URL クエリで渡す socket type 文字列（例 `list%3Cmap%3Cjson%3E%3E`）。
	text: String,
}

#[derive(Debug, Deserialize)]
struct SocketTypeCompatibilityQuery {
	/// 上流 port の socket type 文字列。
	from: String,
	/// 下流 port の socket type 文字列。
	to: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ParseSocketTypeResponse {
	pub valid: bool,
	pub canonical_type: Option<String>,
	pub type_expr: Option<crate::flowgraph::SocketTypeExpr>,
	pub error: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SocketTypeCompatibilityResponse {
	pub valid: bool,
	pub compatible: Option<bool>,
	pub from_canonical_type: Option<String>,
	pub to_canonical_type: Option<String>,
	pub from_type_expr: Option<crate::flowgraph::SocketTypeExpr>,
	pub to_type_expr: Option<crate::flowgraph::SocketTypeExpr>,
	pub error: Option<String>,
}

fn parse_socket_type_response(text: &str) -> ParseSocketTypeResponse {
	match SocketType::parse(text) {
		Ok(ty) => ParseSocketTypeResponse {
			valid: true,
			canonical_type: Some(ty.to_string()),
			type_expr: Some(ty.type_expr()),
			error: None,
		},
		Err(error) => ParseSocketTypeResponse {
			valid: false,
			canonical_type: None,
			type_expr: None,
			error: Some(error.to_string()),
		},
	}
}

fn socket_type_compatibility_response(from: &str, to: &str) -> SocketTypeCompatibilityResponse {
	let from_parsed = SocketType::parse(from);
	let to_parsed = SocketType::parse(to);
	match (from_parsed, to_parsed) {
		(Ok(from_ty), Ok(to_ty)) => SocketTypeCompatibilityResponse {
			valid: true,
			compatible: Some(from_ty.compatible_with(&to_ty)),
			from_canonical_type: Some(from_ty.to_string()),
			to_canonical_type: Some(to_ty.to_string()),
			from_type_expr: Some(from_ty.type_expr()),
			to_type_expr: Some(to_ty.type_expr()),
			error: None,
		},
		(from_result, to_result) => {
			let mut errors = Vec::new();
			if let Err(error) = from_result {
				errors.push(format!("from: {error}"));
			}
			if let Err(error) = to_result {
				errors.push(format!("to: {error}"));
			}
			SocketTypeCompatibilityResponse {
				valid: false,
				compatible: None,
				from_canonical_type: None,
				to_canonical_type: None,
				from_type_expr: None,
				to_type_expr: None,
				error: Some(errors.join("; ")),
			}
		}
	}
}

/// Phase ξ-5: プロパティエディタが単位文字列をサーバと同じ `parse_unit` で検証するための軽量 API。
#[get("/flowgraph/parse-unit")]
pub async fn get_parse_unit(q: web::Query<ParseUnitQuery>) -> impl Responder {
	let t = q.text.trim();
	if t.is_empty() {
		return HttpResponse::Ok().json(serde_json::json!({
			"valid": true,
			"dimension": serde_json::Value::Null,
			"canonical_unit": "",
			"error": serde_json::Value::Null,
		}));
	}
	match parse_unit(t) {
		Ok(u) => {
			let q = Quantity::of(1.0, u);
			let dim = q.dimension();
			HttpResponse::Ok().json(serde_json::json!({
				"valid": true,
				"dimension": if dim.is_dimensionless() {
					serde_json::Value::Null
				} else {
					serde_json::json!(dim.canonical())
				},
				"canonical_unit": q.unit.canonical(),
				"error": serde_json::Value::Null,
			}))
		}
		Err(e) => HttpResponse::Ok().json(serde_json::json!({
			"valid": false,
			"dimension": serde_json::Value::Null,
			"canonical_unit": serde_json::Value::Null,
			"error": e.to_string(),
		})),
	}
}

/// LF-5: GUI / tooling がサーバと同じ `SocketType::parse` で generic 型文字列を検証するための軽量 API。
#[get("/flowgraph/parse-socket-type")]
pub async fn get_parse_socket_type(q: web::Query<ParseSocketTypeQuery>) -> impl Responder {
	HttpResponse::Ok().json(parse_socket_type_response(&q.text))
}

/// LF-5: GUI / tooling がサーバと同じ socket type compatibility rule を参照するための軽量 API。
#[get("/flowgraph/socket-type-compatibility")]
pub async fn get_socket_type_compatibility(q: web::Query<SocketTypeCompatibilityQuery>) -> impl Responder {
	HttpResponse::Ok().json(socket_type_compatibility_response(&q.from, &q.to))
}

// ============================================================================
// GET /flowgraph/tree
// ============================================================================

#[derive(Debug, Serialize)]
pub struct TreeFileEntry {
	/// `<root>/<fq>.flowgraph.toml` の fq 部分。拡張子なし、`/` 区切り。
	pub fq: String,
	/// root 相対の実パス（Windows でも `/` 区切りに正規化）。
	pub path: String,
	/// ファイルの `[meta] title`（解析できれば）。
	pub title: Option<String>,
	/// ノード件数（解析できれば）。
	pub node_count: Option<usize>,
	/// エッジ件数。
	pub edge_count: Option<usize>,
	/// パース失敗時の短いエラー文字列。成功時 None。
	pub parse_error: Option<String>,
	pub source_digest: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TreeResponse {
	pub root_dir: String,
	pub exists: bool,
	pub files: Vec<TreeFileEntry>,
}

#[get("/flowgraph/tree")]
pub async fn get_tree(state: Data<SharedState>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	let exists = root.exists() && root.is_dir();
	let root_disp = graph_root_identity(&root);

	if !exists {
		return HttpResponse::Ok().json(TreeResponse {
			root_dir: root_disp,
			exists: false,
			files: vec![],
		});
	}

	let files = match crate::flowgraph::loader::walk_flowgraph_dir(&root) {
		Ok(v) => v,
		Err(e) => {
			return err_json(
				actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
				"walk_failed",
				format!("ディレクトリ走査に失敗: {e}"),
			)
		}
	};

	let mut entries = Vec::with_capacity(files.len());
	for file_path in files {
		let fq = crate::flowgraph::loader::fq_path_of_file(&root, &file_path).unwrap_or_default();
		let rel = file_path
			.strip_prefix(&root)
			.map(|p| p.to_string_lossy().replace('\\', "/"))
			.unwrap_or_default();
		match std::fs::read_to_string(&file_path) {
			Ok(src) => match parse_flowgraph_file(&src, Some(&file_path)) {
				Ok(file) => entries.push(TreeFileEntry {
					fq,
					path: rel,
					title: file.meta.as_ref().and_then(|m| m.title.clone()),
					node_count: Some(file.nodes.len()),
					edge_count: Some(file.edges.len()),
					parse_error: None,
					source_digest: Some(crate::flowgraph::loader::file::package_source_digest(&src)),
				}),
				Err(e) => entries.push(TreeFileEntry {
					fq,
					path: rel,
					title: None,
					node_count: None,
					edge_count: None,
					parse_error: Some(format!("{}", e)),
					source_digest: Some(crate::flowgraph::loader::file::package_source_digest(&src)),
				}),
			},
			Err(e) => entries.push(TreeFileEntry {
				fq,
				path: rel,
				title: None,
				node_count: None,
				edge_count: None,
				parse_error: Some(format!("read failed: {e}")),
				source_digest: None,
			}),
		}
	}

	HttpResponse::Ok().json(TreeResponse {
		root_dir: root_disp,
		exists: true,
		files: entries,
	})
}

// ============================================================================
// GET /flowgraph/file/{fq:.*}
// ============================================================================

#[derive(Debug, Serialize)]
pub struct FileResponse {
	pub root_dir: String,
	pub fq: String,
	/// root 相対のファイルパス（`/` 区切り）。
	pub path: String,
	/// ファイルそのものの TOML ソース。GUI がテキストエディタビューに流す用。
	pub raw_toml: String,
	/// パース済み構造化データ（meta/nodes/edges）。parse_error が None のときだけ有効。
	pub parsed: Option<FlowgraphFile>,
	/// パース失敗時のメッセージ（Diagnostic リスト）。
	pub parse_diagnostics: Vec<Diagnostic>,
}

#[get("/flowgraph/file/{fq:.*}")]
pub async fn get_file(state: Data<SharedState>, fq_path: web::Path<String>) -> impl Responder {
	let fq = fq_path.into_inner();
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	let file_path = match fq_to_file_path(&root, &fq) {
		Ok(v) => v,
		Err(r) => return r,
	};
	if let Err(r) = ensure_within_root(&root, &file_path) {
		return r;
	}

	if !file_path.is_file() {
		return err_json(
			actix_web::http::StatusCode::NOT_FOUND,
			"not_found",
			format!("ファイルが存在しません: {}", file_path.display()),
		);
	}
	let raw = match std::fs::read_to_string(&file_path) {
		Ok(v) => v,
		Err(e) => {
			return err_json(
				actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
				"read_failed",
				format!("読み込み失敗: {e}"),
			)
		}
	};
	let rel = file_path
		.strip_prefix(&root)
		.map(|p| p.to_string_lossy().replace('\\', "/"))
		.unwrap_or_default();

	let (parsed, parse_diagnostics) = match parse_flowgraph_file(&raw, Some(&file_path)) {
		Ok(f) => (Some(f), vec![]),
		Err(e) => (None, e.diagnostics),
	};

	HttpResponse::Ok().json(FileResponse {
		root_dir: graph_root_identity(&root),
		fq: fq.trim_matches('/').to_string(),
		path: rel,
		raw_toml: raw,
		parsed,
		parse_diagnostics,
	})
}

// ============================================================================
// GET /flowgraph/diagnostics
// ============================================================================

#[derive(Debug, Serialize)]
pub struct DiagnosticsResponse {
	pub root_dir: String,
	pub ok: bool,
	pub diagnostics: Vec<Diagnostic>,
	/// bridge など、ロード後の実行段階で判明した問題。
	pub runtime_issues: Vec<crate::runtime_health::RuntimeIssue>,
	pub node_meta: std::collections::HashMap<String, crate::flowgraph::loader::LoadedNodeMeta>,
	pub speech_routes: Vec<crate::flowgraph::speech_routes::SpeechRoute>,
	/// LF-2: graph 全体が要求する capability summary。
	pub capability_summary: crate::flowgraph::loader::GraphCapabilitySummary,
	/// LF-1: graph-as-node / library signature の入口となる read-only graph signature。
	pub graph_signature: crate::flowgraph::loader::GraphSignature,
	/// LF-4: `[package]` manifest catalog。
	pub package_manifests: Vec<crate::flowgraph::loader::PackageManifestSummary>,
	pub source_digests: std::collections::HashMap<String, String>,
	/// LF-4: local package dependency DAG の dependency-first order。
	pub package_dependency_order: Vec<String>,
	/// LF-4: lockfile 生成前に観測できる read-only preview。
	pub package_lock_preview: Vec<crate::flowgraph::loader::PackageLockEntry>,
	/// LF-5: `[[types]]` named schema metadata catalog。
	pub type_schemas: Vec<crate::flowgraph::loader::TypeSchemaSummary>,
	/// LF-5: record schema dependency graph の dependency-first order。
	pub type_schema_dependency_order: Vec<String>,
	/// LF-5: record schema dependency graph の集計。
	pub type_schema_graph_summary: crate::flowgraph::loader::TypeSchemaGraphSummary,
	/// LF-4: package lock preview 全体の stable digest。
	pub package_lock_preview_digest: Option<String>,
	/// LF-7: ロード直後の stateful node summary。worker 実行後の live state ではない。
	pub loaded_state_summary: crate::flowgraph::ProgramStateSummary,
	/// LF-7: ロード直後に snapshot export できる state payload。worker 実行後の live state ではない。
	pub loaded_state_snapshot: crate::flowgraph::ProgramStateSnapshot,
	/// LF-7: 明示 restore 付き load の場合に、実際に restore された node の report。
	pub loaded_state_restore_report: Option<crate::flowgraph::ProgramStateRestoreReport>,
	/// LF-7: profile-local state snapshot file の予定保存先。自動 read/write はまだ行わない。
	pub state_snapshot_file_path: Option<String>,
	/// LF-7: `state_snapshot_file_path` が指す snapshot file envelope が存在するか。
	pub state_snapshot_file_exists: Option<bool>,
	/// RM-3: 各 flowgraph ファイルの mode 用メタ（`[meta].mode_groups` / `default_enabled`）。
	pub file_activation: std::collections::HashMap<String, crate::flowgraph::FlowgraphFileActivationMeta>,
	/// v2 composition: graph定義、instance、cross-file portal。
	pub composition: crate::flowgraph::loader::GraphCompositionSummary,
	/// RM-3: exec が抑止されているノード ID。
	pub inactive_exec_nodes: Vec<String>,
	/// bounded runtime activity trace。値本体ではなく型・phaseだけを返す。
	pub runtime_trace: Vec<crate::flowgraph::RuntimeTraceEvent>,
	/// UNAA/Z: U.N. series app discovery snapshot for GUI / diagnostics.
	pub unaa_z: crate::un_app_api::UnAppApiDiscoverySnapshot,
	/// UNAA/Z: compact health summary for dashboard / resources UI.
	pub unaa_z_health: crate::un_app_api::UnAppApiDiscoveryHealthSummary,
	/// UNAA/Z: flattened UNMF/Z endpoint metadata candidates consumed by Flowgraph Studio.
	pub unaa_z_endpoint_candidates: Vec<serde_json::Value>,
	/// UNAA/Z: validation diagnostics for endpoint candidates.
	pub unaa_z_endpoint_candidate_diagnostics: Vec<crate::un_app_api::UnAppApiEndpointCandidateDiagnostic>,
	/// UNMF/Z data planeの接続経路。UNAA/Z endpoint metadataとは別のsession設定。
	pub unmfz_transport: crate::unmfz_transport::UnmfzTransportSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StateSnapshotFileStatus {
	path: String,
	exists: bool,
}

fn state_snapshot_file_status(rt: &FlowgraphRuntime) -> Option<StateSnapshotFileStatus> {
	rt.state_snapshot_file_path.as_ref().map(|path| StateSnapshotFileStatus {
		path: path.display().to_string().replace('\\', "/"),
		exists: path.is_file(),
	})
}

fn endpoint_runtime_node_id(raw: &str, fq: &str) -> Option<String> {
	let (node_ref, _) = raw.rsplit_once(':')?;
	if node_ref.contains("::") {
		Some(node_ref.to_string())
	} else {
		Some(format!("{fq}::{node_ref}"))
	}
}

fn apply_composition_activity(
	composition: &mut crate::flowgraph::loader::GraphCompositionSummary,
	node_meta: &std::collections::HashMap<String, crate::flowgraph::loader::LoadedNodeMeta>,
	inactive_nodes: &std::collections::HashSet<String>,
) -> Vec<crate::flowgraph::Diagnostic> {
	for instance in &mut composition.instances {
		let prefix = format!("{}::", instance.runtime_fq);
		let mut nodes = node_meta.keys().filter(|node| node.starts_with(&prefix)).peekable();
		instance.active = nodes.peek().is_none() || nodes.any(|node| !inactive_nodes.contains(node));
	}
	let mut diagnostics = Vec::new();
	for portal in &mut composition.portals {
		portal.from_active = endpoint_runtime_node_id(&portal.from, &portal.from_fq).is_none_or(|node| !inactive_nodes.contains(&node));
		portal.to_active = endpoint_runtime_node_id(&portal.to, &portal.to_fq).is_none_or(|node| !inactive_nodes.contains(&node));
		portal.active = portal.from_active && portal.to_active;
		if portal.from_active != portal.to_active {
			diagnostics.push(
				crate::flowgraph::Diagnostic::warning(
					crate::flowgraph::DiagnosticCode::ActivationBoundary,
					format!(
						"cross-file portalのactivation境界が不一致: {} ({}) -> {} ({})",
						portal.from,
						if portal.from_active { "active" } else { "inactive" },
						portal.to,
						if portal.to_active { "active" } else { "inactive" }
					),
				)
				.with_hint(format!("{} -> {}", portal.from, portal.to)),
			);
		}
	}
	diagnostics
}

#[get("/flowgraph/diagnostics")]
pub async fn get_diagnostics(state: Data<SharedState>) -> impl Responder {
	// External discovery may wait for replies. Do it before taking the graph
	// read guard so diagnostics cannot delay graph reload/save for that window.
	let unaa_z = crate::un_app_api::discovery_snapshot_from_state(&state).await;
	let unaa_z_health = unaa_z.health_summary();
	let unaa_z_endpoint_candidates = unaa_z.endpoint_candidates();
	let unaa_z_endpoint_candidate_diagnostics = unaa_z.endpoint_candidate_diagnostics();
	let (runtime_health, unmfz_transport) = {
		let state = state.read().await;
		(state.runtime_health.clone(), state.unmfz_transport.snapshot())
	};
	let runtime_issues = runtime_health.snapshot().await;
	let fg = state.read().await.flowgraph.clone();
	let rt = fg.read().await;
	let Some(rt) = rt.as_ref() else {
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"flowgraph_dir_unset",
			"conf.flowgraph_dir が未設定です",
		);
	};
	let inactive_exec_nodes = rt.trigger_gate.as_ref().map(|g| g.inactive_node_ids()).unwrap_or_default();
	let inactive_set = inactive_exec_nodes.iter().cloned().collect::<std::collections::HashSet<_>>();
	let mut composition = rt.composition.clone();
	let mut diagnostics = rt.diagnostics.clone();
	diagnostics.extend(apply_composition_activity(&mut composition, &rt.node_meta, &inactive_set));
	let runtime_trace = rt
		.handle
		.as_ref()
		.and_then(|handle| handle.runtime_trace.read().ok().map(|events| events.iter().cloned().collect()))
		.unwrap_or_default();
	let state_snapshot_file = state_snapshot_file_status(rt);
	HttpResponse::Ok().json(DiagnosticsResponse {
		root_dir: graph_root_identity(&rt.root_dir),
		ok: rt.ok,
		diagnostics,
		runtime_issues,
		node_meta: rt.node_meta.clone(),
		speech_routes: rt.speech_routes.clone(),
		capability_summary: rt.capability_summary.clone(),
		graph_signature: rt.graph_signature.clone(),
		package_manifests: rt.package_manifests.clone(),
		source_digests: rt.source_digests.clone(),
		package_dependency_order: rt.package_dependency_order.clone(),
		package_lock_preview: rt.package_lock_preview.clone(),
		type_schemas: rt.type_schemas.clone(),
		type_schema_dependency_order: rt.type_schema_dependency_order.clone(),
		type_schema_graph_summary: rt.type_schema_graph_summary.clone(),
		package_lock_preview_digest: rt.package_lock_preview_digest.clone(),
		loaded_state_summary: rt.loaded_state_summary.clone(),
		loaded_state_snapshot: rt.loaded_state_snapshot.clone(),
		loaded_state_restore_report: rt.loaded_state_restore_report.clone(),
		state_snapshot_file_path: state_snapshot_file.as_ref().map(|status| status.path.clone()),
		state_snapshot_file_exists: state_snapshot_file.as_ref().map(|status| status.exists),
		file_activation: rt.file_activation.clone(),
		composition,
		inactive_exec_nodes,
		runtime_trace,
		unaa_z,
		unaa_z_health,
		unaa_z_endpoint_candidates,
		unaa_z_endpoint_candidate_diagnostics,
		unmfz_transport,
	})
}

#[get("/flowgraph/signature")]
pub async fn get_signature(state: Data<SharedState>) -> impl Responder {
	let fg = state.read().await.flowgraph.clone();
	let rt = fg.read().await;
	let Some(rt) = rt.as_ref() else {
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"flowgraph_dir_unset",
			"conf.flowgraph_dir が未設定です",
		);
	};
	HttpResponse::Ok().json(rt.graph_signature.clone())
}

// ============================================================================
// GET /flowgraph/package-lock-preview
// ============================================================================

#[derive(Debug, Serialize)]
pub struct PackageLockPreviewResponse {
	pub digest: Option<String>,
	pub entries: Vec<crate::flowgraph::loader::PackageLockEntry>,
	pub entry_count: usize,
}

fn package_lock_preview_response(rt: &FlowgraphRuntime) -> PackageLockPreviewResponse {
	let entries = rt.package_lock_preview.clone();
	PackageLockPreviewResponse {
		digest: rt.package_lock_preview_digest.clone(),
		entry_count: entries.len(),
		entries,
	}
}

#[get("/flowgraph/package-lock-preview")]
pub async fn get_package_lock_preview(state: Data<SharedState>) -> impl Responder {
	let fg = state.read().await.flowgraph.clone();
	let rt = fg.read().await;
	let Some(rt) = rt.as_ref() else {
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"flowgraph_dir_unset",
			"conf.flowgraph_dir が未設定です",
		);
	};
	HttpResponse::Ok().json(package_lock_preview_response(rt))
}

// ============================================================================
// POST /flowgraph/package-lock-preview/save
// ============================================================================

#[derive(Debug, Serialize)]
pub struct SavePackageLockPreviewResponse {
	pub path: String,
	pub digest: Option<String>,
	pub entry_count: usize,
	pub written: bool,
}

#[derive(Debug)]
enum SavePackageLockPreviewError {
	BadIfMatch {
		raw: String,
	},
	OptimisticLockFailed {
		current_digest: Option<String>,
	},
	ReadCurrentFailed {
		path: std::path::PathBuf,
		error: PackageLockFileError,
	},
	WriteFailed {
		path: std::path::PathBuf,
		error: PackageLockFileError,
	},
}

fn package_lock_if_match_hex(raw: &str) -> Option<String> {
	let hex_part = raw.trim().trim_matches('"').trim_start_matches("b3:");
	if hex_part.len() == 64 && hex_part.chars().all(|ch| ch.is_ascii_hexdigit()) {
		Some(hex_part.to_ascii_lowercase())
	} else {
		None
	}
}

fn package_lock_digest_hex(digest: &str) -> Option<String> {
	let hex_part = digest.trim().trim_start_matches("b3:");
	if hex_part.len() == 64 && hex_part.chars().all(|ch| ch.is_ascii_hexdigit()) {
		Some(hex_part.to_ascii_lowercase())
	} else {
		None
	}
}

fn check_package_lock_if_match(path: &std::path::Path, if_match: Option<&str>) -> Result<(), SavePackageLockPreviewError> {
	let Some(raw) = if_match else {
		return Ok(());
	};
	let Some(expected_hex) = package_lock_if_match_hex(raw) else {
		return Err(SavePackageLockPreviewError::BadIfMatch { raw: raw.to_string() });
	};
	match crate::flowgraph::read_package_lock_file(path) {
		Ok(file) => {
			let current_digest = file.digest.clone();
			if current_digest
				.as_deref()
				.and_then(package_lock_digest_hex)
				.is_some_and(|current_hex| current_hex.eq_ignore_ascii_case(&expected_hex))
			{
				Ok(())
			} else {
				Err(SavePackageLockPreviewError::OptimisticLockFailed { current_digest })
			}
		}
		Err(PackageLockFileError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
			Err(SavePackageLockPreviewError::OptimisticLockFailed { current_digest: None })
		}
		Err(error) => Err(SavePackageLockPreviewError::ReadCurrentFailed {
			path: path.to_path_buf(),
			error,
		}),
	}
}

fn save_package_lock_preview_response(
	rt: &FlowgraphRuntime,
	if_match: Option<&str>,
) -> Result<SavePackageLockPreviewResponse, SavePackageLockPreviewError> {
	let path = rt.root_dir.join(crate::flowgraph::FLOWGRAPH_PACKAGE_LOCK_FILE_NAME);
	check_package_lock_if_match(&path, if_match)?;
	let file = PackageLockFile::new(rt.package_lock_preview_digest.clone(), rt.package_lock_preview.clone());
	let entry_count = file.entry_count;
	let digest = file.digest.clone();
	match crate::flowgraph::write_package_lock_file(&path, &file) {
		Ok(()) => Ok(SavePackageLockPreviewResponse {
			path: path.display().to_string().replace('\\', "/"),
			digest,
			entry_count,
			written: true,
		}),
		Err(error) => Err(SavePackageLockPreviewError::WriteFailed { path, error }),
	}
}

#[post("/flowgraph/package-lock-preview/save")]
pub async fn post_save_package_lock_preview(state: Data<SharedState>, req: HttpRequest) -> impl Responder {
	let rt = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await.clone();
		rt
	};
	let Some(rt) = rt else {
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"flowgraph_dir_unset",
			"conf.flowgraph_dir が未設定です",
		);
	};
	let if_match = match req.headers().get("If-Match") {
		Some(value) => match value.to_str() {
			Ok(raw) => Some(raw),
			Err(_) => {
				return err_json(
					actix_web::http::StatusCode::BAD_REQUEST,
					"bad_if_match",
					"If-Match ヘッダが ASCII ではありません",
				)
			}
		},
		None => None,
	};
	match save_package_lock_preview_response(&rt, if_match) {
		Ok(response) => HttpResponse::Ok().json(response),
		Err(SavePackageLockPreviewError::BadIfMatch { raw }) => err_json(
			actix_web::http::StatusCode::BAD_REQUEST,
			"bad_if_match",
			format!("If-Match は `b3:<64 hex>` 形式です。got='{raw}'"),
		),
		Err(SavePackageLockPreviewError::OptimisticLockFailed { current_digest }) => err_json(
			actix_web::http::StatusCode::CONFLICT,
			"optimistic_lock_failed",
			match current_digest {
				Some(digest) => format!("If-Match 不一致: 現在の package lock digest は {digest}"),
				None => "If-Match 不一致: package lockfile は未作成です".to_string(),
			},
		),
		Err(SavePackageLockPreviewError::ReadCurrentFailed { path, error }) => err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"package_lock_read_failed",
			format!("package lockfile の読み込みに失敗しました ({}): {error}", path.display()),
		),
		Err(SavePackageLockPreviewError::WriteFailed { path, error }) => err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"package_lock_write_failed",
			format!("package lockfile の保存に失敗しました ({}): {error}", path.display()),
		),
	}
}

// ============================================================================
// GET /flowgraph/package-lock
// ============================================================================

#[derive(Debug, Serialize)]
pub struct PackageLockStatusResponse {
	pub path: String,
	pub exists: bool,
	pub preview_digest: Option<String>,
	pub lock_digest: Option<String>,
	pub matches_preview: Option<bool>,
	pub entry_count: Option<usize>,
	pub diff: Option<PackageLockStatusDiff>,
	pub file: Option<PackageLockFile>,
	pub error: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PackageLockStatusDiff {
	pub added_ids: Vec<String>,
	pub removed_ids: Vec<String>,
	pub changed_ids: Vec<String>,
	pub unchanged_ids: Vec<String>,
}

fn package_lock_status_diff(
	saved: &[crate::flowgraph::loader::PackageLockEntry],
	preview: &[crate::flowgraph::loader::PackageLockEntry],
) -> PackageLockStatusDiff {
	let saved_by_id: std::collections::BTreeMap<&str, &crate::flowgraph::loader::PackageLockEntry> =
		saved.iter().map(|entry| (entry.id.as_str(), entry)).collect();
	let preview_by_id: std::collections::BTreeMap<&str, &crate::flowgraph::loader::PackageLockEntry> =
		preview.iter().map(|entry| (entry.id.as_str(), entry)).collect();
	let mut added_ids: Vec<String> = Vec::new();
	let mut removed_ids: Vec<String> = Vec::new();
	let mut changed_ids: Vec<String> = Vec::new();
	let mut unchanged_ids: Vec<String> = Vec::new();
	for (id, preview_entry) in &preview_by_id {
		match saved_by_id.get(id) {
			Some(saved_entry) if saved_entry.digest == preview_entry.digest => unchanged_ids.push((*id).to_string()),
			Some(_) => changed_ids.push((*id).to_string()),
			None => added_ids.push((*id).to_string()),
		}
	}
	for id in saved_by_id.keys() {
		if !preview_by_id.contains_key(id) {
			removed_ids.push((*id).to_string());
		}
	}
	PackageLockStatusDiff {
		added_ids,
		removed_ids,
		changed_ids,
		unchanged_ids,
	}
}

fn package_lock_status_response(rt: &FlowgraphRuntime) -> PackageLockStatusResponse {
	let path = rt.root_dir.join(crate::flowgraph::FLOWGRAPH_PACKAGE_LOCK_FILE_NAME);
	let path_text = path.display().to_string().replace('\\', "/");
	let preview_digest = rt.package_lock_preview_digest.clone();
	match crate::flowgraph::read_package_lock_file(&path) {
		Ok(file) => {
			let lock_digest = file.digest.clone();
			let matches_preview = Some(lock_digest == preview_digest);
			let diff = package_lock_status_diff(&file.entries, &rt.package_lock_preview);
			PackageLockStatusResponse {
				path: path_text,
				exists: true,
				preview_digest,
				lock_digest,
				matches_preview,
				entry_count: Some(file.entry_count),
				diff: Some(diff),
				file: Some(file),
				error: None,
			}
		}
		Err(PackageLockFileError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => PackageLockStatusResponse {
			path: path_text,
			exists: false,
			preview_digest,
			lock_digest: None,
			matches_preview: None,
			entry_count: None,
			diff: None,
			file: None,
			error: None,
		},
		Err(error) => PackageLockStatusResponse {
			path: path_text,
			exists: true,
			preview_digest,
			lock_digest: None,
			matches_preview: None,
			entry_count: None,
			diff: None,
			file: None,
			error: Some(error.to_string()),
		},
	}
}

#[get("/flowgraph/package-lock")]
pub async fn get_package_lock(state: Data<SharedState>) -> impl Responder {
	let fg = state.read().await.flowgraph.clone();
	let rt = fg.read().await;
	let Some(rt) = rt.as_ref() else {
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"flowgraph_dir_unset",
			"conf.flowgraph_dir が未設定です",
		);
	};
	HttpResponse::Ok().json(package_lock_status_response(rt))
}

// ============================================================================
// POST /flowgraph/state-snapshot/loaded/save
// ============================================================================

#[derive(Debug, Serialize)]
pub struct SaveLoadedStateSnapshotResponse {
	pub path: String,
	pub snapshot_node_count: usize,
	pub written: bool,
}

#[derive(Debug)]
enum SaveLoadedStateSnapshotError {
	PathUnset,
	WriteFailed {
		path: std::path::PathBuf,
		error: StateSnapshotFileError,
	},
}

fn save_loaded_state_snapshot_response(rt: &FlowgraphRuntime) -> Result<SaveLoadedStateSnapshotResponse, SaveLoadedStateSnapshotError> {
	let Some(path) = rt.state_snapshot_file_path.clone() else {
		return Err(SaveLoadedStateSnapshotError::PathUnset);
	};
	let snapshot_node_count = rt.loaded_state_snapshot.snapshot_node_count;
	match rt.write_loaded_state_snapshot_file() {
		Ok(Some(written_path)) => Ok(SaveLoadedStateSnapshotResponse {
			path: written_path.display().to_string().replace('\\', "/"),
			snapshot_node_count,
			written: true,
		}),
		Ok(None) => Err(SaveLoadedStateSnapshotError::PathUnset),
		Err(error) => Err(SaveLoadedStateSnapshotError::WriteFailed { path, error }),
	}
}

#[post("/flowgraph/state-snapshot/loaded/save")]
pub async fn post_save_loaded_state_snapshot(state: Data<SharedState>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
	let _write_guard = match util::lock_graph_writes(&root) { Ok(guard) => guard, Err(error) => return error };
	let rt = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await.clone();
		rt
	};
	let Some(rt) = rt else {
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"flowgraph_dir_unset",
			"conf.flowgraph_dir が未設定です",
		);
	};
	match save_loaded_state_snapshot_response(&rt) {
		Ok(response) => HttpResponse::Ok().json(response),
		Err(SaveLoadedStateSnapshotError::PathUnset) => err_json(
			actix_web::http::StatusCode::CONFLICT,
			"state_snapshot_file_path_unset",
			"state snapshot file path が未設定です。profile-local snapshot path metadata を持つ runtime が必要です。",
		),
		Err(SaveLoadedStateSnapshotError::WriteFailed { path, error }) => err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"state_snapshot_write_failed",
			format!("state snapshot file の保存に失敗しました ({}): {error}", path.display()),
		),
	}
}

// ============================================================================
// POST /flowgraph/state-snapshot/live/save
// ============================================================================

#[derive(Debug, Serialize)]
pub struct SaveLiveStateSnapshotResponse {
	pub path: String,
	pub snapshot_node_count: usize,
	pub written: bool,
}

#[derive(Debug)]
enum SaveLiveStateSnapshotError {
	PathUnset,
	WorkerUnavailable,
	CommandFailed(ProgramCommandError),
	WriteFailed {
		path: std::path::PathBuf,
		error: StateSnapshotFileError,
	},
}

async fn save_live_state_snapshot_response(rt: &FlowgraphRuntime) -> Result<SaveLiveStateSnapshotResponse, SaveLiveStateSnapshotError> {
	let Some(path) = rt.state_snapshot_file_path.clone() else {
		return Err(SaveLiveStateSnapshotError::PathUnset);
	};
	let Some(snapshot) = rt
		.export_live_state_snapshot()
		.await
		.map_err(SaveLiveStateSnapshotError::CommandFailed)?
	else {
		return Err(SaveLiveStateSnapshotError::WorkerUnavailable);
	};
	let snapshot_node_count = snapshot.snapshot_node_count;
	let file = crate::flowgraph::ProgramStateSnapshotFile::new(snapshot);
	crate::flowgraph::write_state_snapshot_file(&path, &file)
		.map_err(|error| SaveLiveStateSnapshotError::WriteFailed { path: path.clone(), error })?;
	Ok(SaveLiveStateSnapshotResponse {
		path: path.display().to_string().replace('\\', "/"),
		snapshot_node_count,
		written: true,
	})
}

#[post("/flowgraph/state-snapshot/live/save")]
pub async fn post_save_live_state_snapshot(state: Data<SharedState>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
	let _write_guard = match util::lock_graph_writes(&root) { Ok(guard) => guard, Err(error) => return error };
	let rt = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await.clone();
		rt
	};
	let Some(rt) = rt else {
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"flowgraph_dir_unset",
			"conf.flowgraph_dir が未設定です",
		);
	};
	match save_live_state_snapshot_response(&rt).await {
		Ok(response) => HttpResponse::Ok().json(response),
		Err(SaveLiveStateSnapshotError::PathUnset) => err_json(
			actix_web::http::StatusCode::CONFLICT,
			"state_snapshot_file_path_unset",
			"state snapshot file path が未設定です。profile-local snapshot path metadata を持つ runtime が必要です。",
		),
		Err(SaveLiveStateSnapshotError::WorkerUnavailable) => err_json(
			actix_web::http::StatusCode::CONFLICT,
			"flowgraph_worker_unavailable",
			"live state snapshot を取得するには起動中の Flowgraph worker が必要です。",
		),
		Err(SaveLiveStateSnapshotError::CommandFailed(error)) => err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"state_snapshot_live_export_failed",
			format!("live state snapshot の取得に失敗しました: {error}"),
		),
		Err(SaveLiveStateSnapshotError::WriteFailed { path, error }) => err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"state_snapshot_write_failed",
			format!("state snapshot file の保存に失敗しました ({}): {error}", path.display()),
		),
	}
}

// ============================================================================
// POST /flowgraph/file  （新規作成）
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct CreateFileRequest {
	/// 新規作成ファイルの fq。例: `chat-echo/main`。
	pub fq: String,
	/// 省略時は空ヘッダテンプレート（spec §14.1 δ-0 合意）。
	#[serde(default)]
	pub initial_toml: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WriteFileResponse {
	pub restart_required: bool,
	pub fq: String,
	pub path: String,
	/// Write 操作後に再ロードした結果のダイジェスト。
	pub diagnostics: Vec<Diagnostic>,
	/// ロードが成功したか（error 診断がなかったか）。
	pub ok: bool,
	/// 書き込み直前の `.bak` ファイル名（新規作成時は None）。
	pub backup: Option<String>,
}

fn default_initial_toml(fq: &str) -> String {
	format!(
		"# {fq}\n\n[meta]\ntitle = \"{fq}\"\n\n# --- nodes ---\n# [[nodes]]\n# id = \"example\"\n# feature = \"flowgraph.literal.string\"\n# properties.value = \"hello\"\n\n# --- edges ---\n# [[edges]]\n# from = \"example:value\"\n# to = \"logger:value\"\n"
	)
}

/// 書き込み前の妥当性検証。`raw_toml` が Flowgraph schema としてパースできることを確認する。
fn validate_toml_for_write(raw: &str) -> Result<(), HttpResponse> {
	match parse_flowgraph_file(raw, None) {
		Ok(_) => Ok(()),
		Err(e) => Err(err_json(
			actix_web::http::StatusCode::BAD_REQUEST,
			"invalid_toml",
			format!("書き込み内容が Flowgraph TOML として不正: {:#?}", e.diagnostics),
		)),
	}
}

#[post("/flowgraph/file")]
pub async fn post_file(state: Data<SharedState>, body: web::Bytes) -> impl Responder {
	let body: CreateFileRequest = match parse_graph_body(&body) { Ok(value) => value, Err(error) => return error };
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	let file_path = match fq_to_file_path(&root, &body.fq) {
		Ok(v) => v,
		Err(r) => return r,
	};
	if let Err(r) = ensure_within_root(&root, &file_path) {
		return r;
	}
	if let Err(error) = std::fs::create_dir_all(&root) {
		return err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "create_dir_failed", error);
	}
	let _write_guard = match util::lock_graph_writes(&root) { Ok(guard) => guard, Err(error) => return error };
	if file_path.exists() {
		return err_json(
			actix_web::http::StatusCode::CONFLICT,
			"already_exists",
			format!("既に存在します: {}", file_path.display()),
		);
	}
	let content = body.initial_toml.clone().unwrap_or_else(|| default_initial_toml(&body.fq));
	if let Err(r) = validate_toml_for_write(&content) {
		return r;
	}

	if let Some(parent) = file_path.parent() {
		if let Err(e) = std::fs::create_dir_all(parent) {
			return err_json(
				actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
				"create_dir_failed",
				format!("親ディレクトリ作成に失敗: {e}"),
			);
		}
	}
	if let Err(e) = file_create::write_new_file(&file_path, content.as_bytes()) {
		if e.kind() == std::io::ErrorKind::AlreadyExists {
			return err_json(
				actix_web::http::StatusCode::CONFLICT,
				"already_exists",
				format!("既に存在します。上書きしていません: {}", file_path.display()),
			);
		}
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"write_failed",
			format!("書き込み失敗: {e}"),
		);
	}

	let (ok, diagnostics, restart_required) = reload::reload_runtime_with_restart_status(&state, &root).await;
	log::info!("《Flowgraph》 作成: {}", file_path.display());
	HttpResponse::Created().json(WriteFileResponse {
		restart_required,
		fq: body.fq.trim_matches('/').to_string(),
		path: root_relative(&root, &file_path),
		diagnostics,
		ok,
		backup: None,
	})
}

// ============================================================================
// PUT /flowgraph/file/{fq:.*}  （丸ごと更新）
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct PutFileRequest {
	/// Root returned with the original file, preventing cross-root writes.
	#[serde(default)]
	pub expected_root: Option<String>,
	/// ファイル全体を置き換える TOML テキスト。GUI 側が `toml_edit` 等でフォーマットを保ったまま
	/// 編集した結果を送ることを想定。backend では schema 検証のみ行う。
	pub content: String,
	/// Last read source. Omitted only by legacy API clients.
	#[serde(default)]
	pub expected_content: Option<String>,
}

fn graph_root_identity(root: &std::path::Path) -> String {
	root.canonicalize().unwrap_or_else(|_| root.to_path_buf()).to_string_lossy().replace('\\', "/")
}

fn write_file_checked(root: &std::path::Path, fq: &str, body: &PutFileRequest) -> Result<(std::fs::File, std::path::PathBuf, Option<String>), HttpResponse> {
	if body.expected_root.as_ref().is_some_and(|expected| expected != &graph_root_identity(root)) {
		return Err(err_json(actix_web::http::StatusCode::CONFLICT, "stale_root", "保存先の構成が変わりました。別の構成には保存していません。編集中の内容は保持しています。"));
	}
	let file_path = match fq_to_file_path(&root, &fq) {
		Ok(v) => v,
		Err(r) => return Err(r),
	};
	if let Err(r) = ensure_within_root(&root, &file_path) {
		return Err(r);
	}
	let _write_guard = match util::lock_graph_writes(&root) { Ok(guard) => guard, Err(error) => return Err(error) };
	if !file_path.is_file() {
		return Err(err_json(
			actix_web::http::StatusCode::NOT_FOUND,
			"not_found",
			format!("ファイルが存在しません: {}", file_path.display()),
		));
	}
	if let Err(r) = validate_toml_for_write(&body.content) {
		return Err(r);
	}
	match file_revision::check(&file_path, body.expected_content.as_deref()) {
		Ok(()) => {}
		Err(file_revision::RevisionError::Stale) => return Err(err_json(actix_web::http::StatusCode::CONFLICT, "stale_content", "別の操作がこのファイルを変更しました。上書きしていません。編集中の内容を控えて、保存済みの内容を再確認してください。")),
		Err(file_revision::RevisionError::Read(error)) => return Err(err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "read_failed", error)),
	}

	let backup = match make_backup(&file_path) {
		Ok(name) => Some(name),
		Err(e) => {
			return Err(err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
				"backup_failed", format!("バックアップ作成に失敗しました。元ファイルは変更していません: {e}")));
		}
	};
	if let Err(e) = file_replace::replace(&file_path, body.content.as_bytes()) {
		return Err(err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"write_failed",
			format!("書き込み失敗: {e}"),
		));
	}
	Ok((_write_guard, file_path, backup))
}

#[put("/flowgraph/file/{fq:.*}")]
pub async fn put_file(state: Data<SharedState>, fq_path: web::Path<String>, body: web::Bytes) -> impl Responder {
	let body: PutFileRequest = match parse_graph_body(&body) { Ok(value) => value, Err(error) => return error };
	let fq = fq_path.into_inner();
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	let (_write_guard, file_path, backup) = match write_file_checked(&root, &fq, &body) { Ok(result) => result, Err(error) => return error };
	let (ok, diagnostics, restart_required) = reload::reload_runtime_with_restart_status(&state, &root).await;
	log::info!("《Flowgraph》 更新: {} (backup={:?})", file_path.display(), backup);
	HttpResponse::Ok().json(WriteFileResponse {
		restart_required,
		fq: fq.trim_matches('/').to_string(),
		path: root_relative(&root, &file_path),
		diagnostics,
		ok,
		backup,
	})
}

// ============================================================================
// DELETE /flowgraph/file/{fq:.*}
// ============================================================================

fn removal_diagnostics(root: &std::path::Path, fq: &str) -> (bool, Vec<Diagnostic>) {
	let excluded = std::collections::HashSet::from([fq.to_string()]);
	match crate::flowgraph::fragment::composition_share::retained_package_owners(root, &excluded) {
		Ok(uses) if !uses.is_empty() => return (false, uses.into_iter().map(|reference|
			Diagnostic::error(crate::flowgraph::loader::DiagnosticCode::InvalidPackageManifest,
				format!("{} が所属定義 {} を使用しています。所属するファイルが残るため削除できません。", reference.from_file, reference.target.unwrap_or_default()))
		).collect()),
		Err(error) => return (false, vec![Diagnostic::error(crate::flowgraph::loader::DiagnosticCode::Io,
			format!("削除前の所属関係を確認できません: {error}"))]),
		Ok(_) => {}
	}
	match crate::flowgraph::loader::dir::load_flowgraph_dir_excluding(root, Some(fq)) {
		Ok(report) => (!report.diagnostics.iter().any(|item| item.severity == crate::flowgraph::loader::Severity::Error), report.diagnostics),
		Err(error) => (false, error.diagnostics),
	}
}

#[get("/flowgraph/removal-plan/{fq:.*}")]
async fn get_removal_plan(state: Data<SharedState>, fq: web::Path<String>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
	match removal_plan_snapshot(&root, &fq) { Ok(plan) => HttpResponse::Ok().json(plan), Err(error) => error }
}

pub fn removal_plan_snapshot(root: &std::path::Path, fq: &str) -> Result<serde_json::Value, HttpResponse> {
	let file = fq_to_file_path(root, fq)?;
	ensure_within_root(root, &file)?;
	let _write_guard = util::lock_graph_writes(root)?;
	if !file.is_file() { return Err(err_json(actix_web::http::StatusCode::NOT_FOUND, "not_found", "ファイルがありません")); }
	let expected_content = std::fs::read_to_string(&file).map_err(|error| err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "read_failed", error))?;
	let (can_delete, diagnostics) = removal_diagnostics(root, fq.trim_matches('/'));
	Ok(serde_json::json!({"fq":fq,"root_dir":graph_root_identity(root),"expected_content":expected_content,"can_delete":can_delete,"diagnostics":diagnostics}))
}

/// File operation only; callers must separately report runtime application.
pub fn remove_file_without_reload(root: &std::path::Path, fq: &str, expected_content: Option<&str>, expected_root: Option<&str>) -> Result<String, HttpResponse> {
	remove_file_checked(root, fq, expected_content, expected_root).map(|(_, _, backup)| backup)
}

fn remove_file_checked(root: &std::path::Path, fq: &str, expected_content: Option<&str>, expected_root: Option<&str>) -> Result<(std::fs::File, std::path::PathBuf, String), HttpResponse> {
	if expected_root.is_some_and(|expected| expected != graph_root_identity(root)) {
		return Err(err_json(actix_web::http::StatusCode::CONFLICT, "stale_root", "削除対象の保存先が変わりました。別の構成は削除していません。"));
	}
	let file = fq_to_file_path(root, fq)?;
	ensure_within_root(root, &file)?;
	let write_guard = util::lock_graph_writes(root)?;
	if !file.is_file() { return Err(err_json(actix_web::http::StatusCode::NOT_FOUND, "not_found", "ファイルがありません")); }
	let backup = remove_checked_file(root, fq, &file, expected_content)?;
	Ok((write_guard, file, backup))
}

fn remove_checked_file(root: &std::path::Path, fq: &str, file_path: &std::path::Path, expected_content: Option<&str>) -> Result<String, HttpResponse> {
	match file_revision::check(file_path, expected_content) {
		Ok(()) => {}
		Err(file_revision::RevisionError::Stale) => return Err(err_json(actix_web::http::StatusCode::CONFLICT, "stale_content", "削除確認後にファイルが変更されました。削除していません。削除内容を再確認してください。")),
		Err(file_revision::RevisionError::Read(error)) => return Err(err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "read_failed", error)),
	}

	// Re-evaluate on DELETE as the graph may have changed since preview.
	let (can_delete, diagnostics) = removal_diagnostics(&root, fq.trim_matches('/'));
	if !can_delete {
		return Err(HttpResponse::Conflict().json(serde_json::json!({
			"error":"dependency_conflict", "detail":"削除後の構成にエラーがあります。参照元・診断を確認してください。ファイルは変更していません。", "diagnostics":diagnostics
		})));
	}
	// 削除はまず .bak 保存してから本体削除 → 誤操作復元が容易。
	let backup = match make_backup(&file_path) {
		Ok(n) => n,
		Err(e) => {
			return Err(err_json(
				actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
				"backup_failed",
				format!("バックアップ作成に失敗: {e}"),
			))
		}
	};
	if let Err(e) = std::fs::remove_file(&file_path) {
		return Err(err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"delete_failed",
			format!("削除失敗: {e}"),
		));
	}

	Ok(backup)
}

#[derive(Default, Deserialize)]
struct DeleteFileRequest {
	expected_content: Option<String>,
	expected_root: Option<String>,
}

#[delete("/flowgraph/file/{fq:.*}")]
pub async fn delete_file(state: Data<SharedState>, fq_path: web::Path<String>, body: web::Bytes) -> impl Responder {
	let body: DeleteFileRequest = if body.is_empty() { DeleteFileRequest::default() } else {
		match serde_json::from_slice(&body) { Ok(body) => body, Err(error) => return err_json(actix_web::http::StatusCode::BAD_REQUEST, "invalid_request", error) }
	};
	let fq = fq_path.into_inner();
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	let (_write_guard, file_path, backup) = match remove_file_checked(&root, &fq, body.expected_content.as_deref(), body.expected_root.as_deref()) { Ok(result) => result, Err(error) => return error };

	let (ok, diagnostics, restart_required) = reload::reload_runtime_with_restart_status(&state, &root).await;
	log::info!("《Flowgraph》 削除: {} (backup={})", file_path.display(), backup);
	HttpResponse::Ok().json(WriteFileResponse {
		restart_required,
		fq: fq.trim_matches('/').to_string(),
		path: root_relative(&root, &file_path),
		diagnostics,
		ok,
		backup: Some(backup),
	})
}

// ============================================================================
// POST /flowgraph/file/{fq:.*}/open-external
// ============================================================================

#[derive(Debug, Serialize)]
pub struct OpenExternalResponse {
	pub fq: String,
	pub path: String,
	pub command: String,
	pub spawned: bool,
}

#[post("/flowgraph/file/{fq:.*}/open-external")]
pub async fn post_open_external(state: Data<SharedState>, fq_path: web::Path<String>) -> impl Responder {
	let fq = fq_path.into_inner();
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	let file_path = match fq_to_file_path(&root, &fq) {
		Ok(v) => v,
		Err(r) => return r,
	};
	if let Err(r) = ensure_within_root(&root, &file_path) {
		return r;
	}
	if !file_path.is_file() {
		return err_json(
			actix_web::http::StatusCode::NOT_FOUND,
			"not_found",
			format!("ファイルが存在しません: {}", file_path.display()),
		);
	}
	let (command, spawned) = spawn_os_opener(&file_path);
	log::info!(
		"《Flowgraph》 open-external: {} (cmd={command}, spawned={spawned})",
		file_path.display()
	);
	HttpResponse::Ok().json(OpenExternalResponse {
		fq: fq.trim_matches('/').to_string(),
		path: root_relative(&root, &file_path),
		command,
		spawned,
	})
}

// ============================================================================
// POST /flowgraph/reload
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ReloadResponse {
	pub root_dir: String,
	pub ok: bool,
	pub diagnostics: Vec<Diagnostic>,
	pub node_count: usize,
	pub restart_required: bool,
}

#[derive(Debug, Serialize)]
pub struct ReloadPreservingStateResponse {
	pub restart_required: bool,
	pub root_dir: String,
	pub path: String,
	pub ok: bool,
	pub diagnostics: Vec<Diagnostic>,
	pub node_count: usize,
	pub saved_node_count: usize,
	pub restored_node_count: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct RestoreStateSnapshotResponse {
	pub restart_required: bool,
	pub root_dir: String,
	pub path: String,
	pub ok: bool,
	pub diagnostics: Vec<Diagnostic>,
	pub node_count: usize,
	pub restored_node_count: Option<usize>,
}

/// ディスクから Flowgraph を再ロードし、結果を保存 + `FlowgraphReloaded` イベント配信。
///
/// 外部エディタで `.flowgraph.toml` を直接編集した後に GUI から叩く用途を想定。
/// `notify` crate 経由の自動ウォッチャーは δ-7 以降で検討（spec §10.3）。
#[post("/flowgraph/reload")]
pub async fn post_reload(state: Data<SharedState>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	// Share the writer gate before snapshotting or stopping runtime workers.
	let _write_guard = match util::lock_graph_writes(&root) { Ok(guard) => guard, Err(error) => return error };
	let (ok, diagnostics, restart_required) = reload::reload_runtime_with_restart_status(&state, &root).await;
	let node_count = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await;
		rt.as_ref().map(|r| r.node_meta.len()).unwrap_or(0)
	};
	HttpResponse::Ok().json(ReloadResponse {
		root_dir: graph_root_identity(&root),
		ok,
		diagnostics,
		node_count,
		restart_required,
	})
}

/// 起動中 worker の live state を profile-local snapshot file へ保存してから、同じ snapshot file で reload / restore する。
/// 通常 reload の挙動は変えず、ユーザー操作または外部ツールが明示的に選ぶ state-preserving reload。
#[post("/flowgraph/reload/preserve-state")]
pub async fn post_reload_preserving_state(state: Data<SharedState>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	// Share the writer gate before snapshotting or stopping runtime workers.
	let _write_guard = match util::lock_graph_writes(&root) { Ok(guard) => guard, Err(error) => return error };
	let rt = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await.clone();
		rt
	};
	let Some(rt) = rt else {
		return err_json(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"flowgraph_dir_unset",
			"conf.flowgraph_dir が未設定です",
		);
	};
	let snapshot_path = match rt.state_snapshot_file_path.clone() {
		Some(path) => path,
		None => {
			return err_json(
				actix_web::http::StatusCode::CONFLICT,
				"state_snapshot_file_path_unset",
				"state snapshot file path が未設定です。profile-local snapshot path metadata を持つ runtime が必要です。",
			)
		}
	};
	let save_response = match save_live_state_snapshot_response(&rt).await {
		Ok(response) => response,
		Err(SaveLiveStateSnapshotError::PathUnset) => {
			return err_json(
				actix_web::http::StatusCode::CONFLICT,
				"state_snapshot_file_path_unset",
				"state snapshot file path が未設定です。profile-local snapshot path metadata を持つ runtime が必要です。",
			)
		}
		Err(SaveLiveStateSnapshotError::WorkerUnavailable) => {
			return err_json(
				actix_web::http::StatusCode::CONFLICT,
				"flowgraph_worker_unavailable",
				"state-preserving reload には起動中の Flowgraph worker が必要です。",
			)
		}
		Err(SaveLiveStateSnapshotError::CommandFailed(error)) => {
			return err_json(
				actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
				"state_snapshot_live_export_failed",
				format!("live state snapshot の取得に失敗しました: {error}"),
			)
		}
		Err(SaveLiveStateSnapshotError::WriteFailed { path, error }) => {
			return err_json(
				actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
				"state_snapshot_write_failed",
				format!("state snapshot file の保存に失敗しました ({}): {error}", path.display()),
			)
		}
	};
	let (ok, diagnostics, restart_required) = reload_runtime_with_state_snapshot_file(&state, &root, &snapshot_path).await;
	let (node_count, restored_node_count) = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await;
		let node_count = rt.as_ref().map(|r| r.node_meta.len()).unwrap_or(0);
		let restored_node_count = rt
			.as_ref()
			.and_then(|r| r.loaded_state_restore_report.as_ref())
			.map(|report| report.restored_node_count);
		(node_count, restored_node_count)
	};
	HttpResponse::Ok().json(ReloadPreservingStateResponse {
		restart_required,
		root_dir: graph_root_identity(&root),
		path: save_response.path,
		ok,
		diagnostics,
		node_count,
		saved_node_count: save_response.snapshot_node_count,
		restored_node_count: if ok { restored_node_count } else { None },
	})
}

// ============================================================================
// POST /flowgraph/state-snapshot/profile-local/restore
// ============================================================================

/// 現在 profile / flowgraph root に対応する snapshot file envelope を明示 restore して reload する。
/// 自動 restore ではなく、ユーザー操作または外部ツールから叩く手動 API。
#[post("/flowgraph/state-snapshot/profile-local/restore")]
pub async fn post_restore_profile_local_state_snapshot(state: Data<SharedState>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	// Share the writer gate before snapshotting or stopping runtime workers.
	let _write_guard = match util::lock_graph_writes(&root) { Ok(guard) => guard, Err(error) => return error };
	let snapshot_path = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await;
		let Some(rt) = rt.as_ref() else {
			return err_json(
				actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
				"flowgraph_dir_unset",
				"conf.flowgraph_dir が未設定です",
			);
		};
		let Some(path) = rt.state_snapshot_file_path.clone() else {
			return err_json(
				actix_web::http::StatusCode::CONFLICT,
				"state_snapshot_file_path_unset",
				"state snapshot file path が未設定です。profile-local snapshot path metadata を持つ runtime が必要です。",
			);
		};
		path
	};
	let (ok, diagnostics, restart_required) = reload_runtime_with_state_snapshot_file(&state, &root, &snapshot_path).await;
	let node_count = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await;
		rt.as_ref().map(|r| r.node_meta.len()).unwrap_or(0)
	};
	let restored_node_count = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await;
		rt.as_ref()
			.and_then(|r| r.loaded_state_restore_report.as_ref())
			.map(|report| report.restored_node_count)
	};
	HttpResponse::Ok().json(RestoreStateSnapshotResponse {
		restart_required,
		root_dir: graph_root_identity(&root),
		path: snapshot_path.display().to_string().replace('\\', "/"),
		ok,
		diagnostics,
		node_count,
		restored_node_count: if ok { restored_node_count } else { None },
	})
}

// ============================================================================
// Route registration
// ============================================================================

/// Control API から Flowgraph routes を登録する。
///
/// `/flowgraph/import/zip` が raw ZIP body（`web::Bytes`）を受けるため、actix-web 既定の
/// `PayloadConfig`（256 KiB）では不足。spec §14.4 に従い **64 MiB** に拡張する。
/// `app_data` は scope 内 route 全体に効くが、他の control route は通常 JSON 数 KiB で
/// 上限を跨がないため影響しない。
pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.configure(purposes::api::configure);
	cfg.app_data(web::PayloadConfig::new(GRAPH_BODY_LIMIT_BYTES))
		.service(get_node_catalog)
		.service(get_parse_unit)
		.service(get_parse_socket_type)
		.service(get_socket_type_compatibility)
		.service(get_tree)
		.service(get_diagnostics)
		.service(get_signature)
		.service(get_package_lock_preview)
		.service(get_package_lock)
		.service(post_save_package_lock_preview)
		.service(post_save_loaded_state_snapshot)
		.service(post_save_live_state_snapshot)
		.service(post_reload_preserving_state)
		.service(post_restore_profile_local_state_snapshot)
		.service(post_reload)
		.service(fragment_zip::post_fragment_copy)
		.service(fragment_zip::post_share_plan)
		.service(fragment_zip::get_companions)
		.service(fragment_zip::post_fragment_paste)
		.service(fragment_zip::post_export_zip)
		.service(fragment_zip::post_import_zip)
		.service(trigger::post_trigger_node)
		// open-external を file GET より先に登録しないと `{fq:.*}` が open-external も食うため。
		.service(post_open_external)
		.service(post_file)
		.service(put_file)
		.service(delete_file)
		.service(get_removal_plan)
		.service(file_recovery::list_backups)
		.service(file_recovery::recovery_history)
		.service(file_recovery::preview_restore)
		.service(file_recovery::restore_file)
		.service(get_file);
}

#[cfg(test)]
mod browser_review;

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::loader::{GraphCapabilitySummary, GraphSignature, PackageLockEntry};
	use crate::flowgraph::{ProgramStateSnapshot, ProgramStateSnapshotNode, ProgramStateSummary, StateSnapshotFormat};
	use std::collections::{BTreeMap, HashMap};
	use std::path::PathBuf;
	use std::time::{SystemTime, UNIX_EPOCH};

	fn snapshot() -> ProgramStateSnapshot {
		ProgramStateSnapshot {
			snapshot_node_count: 1,
			nodes: vec![ProgramStateSnapshotNode {
				node: "main::counter".into(),
				feature: "flowgraph.state.int_counter".into(),
				version: 1,
				format: StateSnapshotFormat::Json,
				value: serde_json::json!({ "value": 7 }),
			}],
		}
	}

	fn temp_dir(label: &str) -> PathBuf {
		let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_millis();
		let dir = std::env::temp_dir().join(format!("vac-flowgraph-control-save-{label}-{}-{now_ms}", std::process::id()));
		std::fs::create_dir_all(&dir).expect("create temp dir");
		dir
	}

	#[actix_web::test]
	async fn diagnostics_speech_routes_follow_loaded_program_not_saved_files() {
		use actix_web::{test as http, App};
		let root = temp_dir("speech-routes-http");
		let graph_root = root.join("graphs");
		std::fs::create_dir(&graph_root).unwrap();
		let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("flowgraph.example/chat-echo");
		for name in ["main.flowgraph.toml", "tts.flowgraph.toml"] {
			std::fs::copy(examples.join(name), graph_root.join(name)).unwrap();
		}
		let mut conf: crate::Conf = toml::from_str("").unwrap();
		conf.runtime_dir = Some(root.join("runtime"));
		let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		let shared = state.read().await.flowgraph.clone();
		// Load metadata only: no graph worker, device, listener or speech execution.
		*shared.write().await = Some(FlowgraphRuntime::load_program(&graph_root).0);
		let app = http::init_service(App::new().app_data(Data::new(state.clone())).service(get_diagnostics)).await;
		let read = || http::TestRequest::get().uri("/flowgraph/diagnostics").to_request();
		let loaded: serde_json::Value = http::call_and_read_body_json(&app, read()).await;
		assert_eq!(loaded["ok"], true);
		let routes = loaded["speech_routes"].as_array().unwrap();
		assert_eq!(routes.len(), 1);
		assert_eq!(routes[0]["runtime_node"], "tts::speaker");
		assert_eq!(routes[0]["definition_instances"], 1);
		assert!(routes[0]["incoming"].as_array().unwrap().iter().any(|edge|
			edge["from_node"] == "main::in" && edge["to_port"] == "text" && edge["exec"] == false));
		assert!(!routes[0].to_string().contains("voicevox"));
		for name in ["main.flowgraph.toml", "tts.flowgraph.toml"] {
			std::fs::write(graph_root.join(name), "# saved empty graph, not yet applied\n").unwrap();
		}
		let retained: serde_json::Value = http::call_and_read_body_json(&app, read()).await;
		assert_eq!(retained["speech_routes"], loaded["speech_routes"]);
		assert_eq!(retained["source_digests"], loaded["source_digests"]);
		*shared.write().await = Some(FlowgraphRuntime::load_program(&graph_root).0);
		let reloaded: serde_json::Value = http::call_and_read_body_json(&app, read()).await;
		assert_eq!(reloaded["ok"], true);
		assert_eq!(reloaded["speech_routes"], serde_json::json!([]));
		assert_ne!(reloaded["source_digests"], loaded["source_digests"]);
		drop(app);
		drop(state);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[actix_web::test]
	async fn checked_write_http_rejects_stale_source_and_busy_root() {
		// Exercise production disk transaction/HTTP errors without audio, services or reload.
		async fn disk_endpoint(root: Data<PathBuf>, body: Json<PutFileRequest>) -> HttpResponse {
			match write_file_checked(root.get_ref(), "main", &body) {
				Ok((_guard, _, backup)) => HttpResponse::Ok().json(serde_json::json!({"backup": backup})),
				Err(error) => error,
			}
		}
		let root = temp_dir("write-http");
		let initial = "[meta]\ntitle = 'initial'\n";
		let saved = "[meta]\ntitle = 'saved A'\n";
		std::fs::write(root.join("main.flowgraph.toml"), initial).unwrap();
		let app = actix_web::test::init_service(actix_web::App::new()
			.app_data(Data::new(root.clone())).route("/write", web::put().to(disk_endpoint))).await;
		let request = |content: &str| actix_web::test::TestRequest::put().uri("/write")
			.set_json(serde_json::json!({"content":content,"expected_content":initial,"expected_root":graph_root_identity(&root)})).to_request();
		let wrong_root_request = actix_web::test::TestRequest::put().uri("/write")
			.set_json(serde_json::json!({"content":saved,"expected_content":initial,"expected_root":"different-root"})).to_request();
		let response = actix_web::test::call_service(&app, wrong_root_request).await;
		assert_eq!(response.status(), actix_web::http::StatusCode::CONFLICT);
		let body: serde_json::Value = actix_web::test::read_body_json(response).await;
		assert_eq!(body["error"], "stale_root");
		assert_eq!(std::fs::read_to_string(root.join("main.flowgraph.toml")).unwrap(), initial);
		let response = actix_web::test::call_service(&app, request(saved)).await;
		assert_eq!(response.status(), actix_web::http::StatusCode::OK);
		let body: serde_json::Value = actix_web::test::read_body_json(response).await;
		let backup = root.join(body["backup"].as_str().unwrap());
		assert_eq!(std::fs::read_to_string(&backup).unwrap(), initial);
		let response = actix_web::test::call_service(&app, request("[meta]\ntitle = 'stale B'\n")).await;
		assert_eq!(response.status(), actix_web::http::StatusCode::CONFLICT);
		let body: serde_json::Value = actix_web::test::read_body_json(response).await;
		assert_eq!(body["error"], "stale_content");
		assert_eq!(std::fs::read_to_string(root.join("main.flowgraph.toml")).unwrap(), saved);
		assert_eq!(std::fs::read_to_string(&backup).unwrap(), initial);
		let guard = write_lock::acquire(&root).unwrap();
		let response = actix_web::test::call_service(&app, request(saved)).await;
		assert_eq!(response.status(), actix_web::http::StatusCode::CONFLICT);
		let body: serde_json::Value = actix_web::test::read_body_json(response).await;
		assert_eq!(body["error"], "flowgraph_busy");
		drop(guard);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[actix_web::test]
	async fn large_graph_save_preserves_conflict_check_and_backup() {
		async fn disk_endpoint(root: Data<PathBuf>, bytes: web::Bytes) -> HttpResponse {
			let body: PutFileRequest = match parse_graph_body(&bytes) { Ok(v)=>v,Err(e)=>return e };
			match write_file_checked(&root,"main",&body) {
				Ok((_guard,_,backup))=>HttpResponse::Ok().json(serde_json::json!({"backup":backup})),
				Err(error)=>error,
			}
		}
		async fn legacy(_body: Json<PutFileRequest>) -> HttpResponse {HttpResponse::Ok().finish()}
		async fn create_parse(bytes: web::Bytes) -> HttpResponse {
			match parse_graph_body::<CreateFileRequest>(&bytes) {Ok(body)=>HttpResponse::Ok().json(serde_json::json!({"length":body.initial_toml.unwrap().len()})),Err(e)=>e}
		}
		let root=temp_dir("large-write");
		let initial=format!("# {}\n[meta]\ntitle='initial'\n","x".repeat(1_100_000));
		let saved=initial.replace("initial","saved");
		std::fs::write(root.join("main.flowgraph.toml"),&initial).unwrap();
		let app=actix_web::test::init_service(actix_web::App::new()
			.app_data(Data::new(root.clone())).app_data(web::PayloadConfig::new(GRAPH_BODY_LIMIT_BYTES))
			.route("/old",web::put().to(legacy)).route("/save",web::put().to(disk_endpoint)).route("/create-parse",web::post().to(create_parse))).await;
		let body=serde_json::json!({"content":saved,"expected_content":initial,"expected_root":graph_root_identity(&root)});
		let old=actix_web::test::call_service(&app,actix_web::test::TestRequest::put().uri("/old").set_json(&body).to_request()).await;
		assert_eq!(old.status(),actix_web::http::StatusCode::PAYLOAD_TOO_LARGE);
		let response=actix_web::test::call_service(&app,actix_web::test::TestRequest::put().uri("/save").set_json(&body).to_request()).await;
		assert_eq!(response.status(),actix_web::http::StatusCode::OK);
		let result:serde_json::Value=actix_web::test::read_body_json(response).await;
		assert_eq!(std::fs::read_to_string(root.join("main.flowgraph.toml")).unwrap(),saved);
		assert_eq!(std::fs::read_to_string(root.join(result["backup"].as_str().unwrap())).unwrap(),initial);
		let stale=actix_web::test::call_service(&app,actix_web::test::TestRequest::put().uri("/save").set_json(&body).to_request()).await;
		assert_eq!(stale.status(),actix_web::http::StatusCode::CONFLICT);
		let invalid=actix_web::test::call_service(&app,actix_web::test::TestRequest::put().uri("/save").set_payload(b"{".as_slice()).to_request()).await;
		assert_eq!(invalid.status(),actix_web::http::StatusCode::BAD_REQUEST);
		assert_eq!(std::fs::read_to_string(root.join("main.flowgraph.toml")).unwrap(),saved);
		let large=initial.repeat(2);
		let response=actix_web::test::call_service(&app,actix_web::test::TestRequest::post().uri("/create-parse").set_json(serde_json::json!({"fq":"new","initial_toml":large})).to_request()).await;
		assert_eq!(response.status(),actix_web::http::StatusCode::OK);
		let result:serde_json::Value=actix_web::test::read_body_json(response).await;
		assert_eq!(result["length"],large.len());
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn checked_removal_blocks_dependents_and_preserves_independent_backup() {
		let root = temp_dir("removal");
		let module = root.join("module.flowgraph.toml");
		let dependent = root.join("main.flowgraph.toml");
		let source = "[graph]\nkind='module'\n[[nodes]]\nid='count'\nfeature='flowgraph.state.int_counter'\n";
		std::fs::write(&module, source).unwrap();
		std::fs::write(&dependent, "[[instances]]\nid='used'\ngraph='module'\n").unwrap();
		assert!(!removal_diagnostics(&root, "module").0);
		let rejected = remove_checked_file(&root, "module", &module, None).unwrap_err();
		assert_eq!(rejected.status(), actix_web::http::StatusCode::CONFLICT);
		assert_eq!(std::fs::read_to_string(&module).unwrap(), source);
		assert!(remove_checked_file(&root, "main", &dependent, None).is_ok());
		assert!(removal_diagnostics(&root, "module").0);
		let backup = remove_checked_file(&root, "module", &module, None).unwrap();
		assert!(!module.exists());
		assert_eq!(std::fs::read_to_string(root.join(backup)).unwrap(), source);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn checked_removal_rejects_changed_source_then_backs_up_current_source() {
		let root = temp_dir("removal-revision");
		let path = root.join("main.flowgraph.toml");
		let original = "[meta]\ntitle='original'\n";
		let current = "[meta]\ntitle='changed'\n";
		std::fs::write(&path, current).unwrap();
		let guard = write_lock::acquire(&root).unwrap();
		let rejected = remove_checked_file(&root, "main", &path, Some(original)).unwrap_err();
		assert_eq!(rejected.status(), actix_web::http::StatusCode::CONFLICT);
		assert_eq!(std::fs::read_to_string(&path).unwrap(), current);
		assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2, "rejection must not create a backup");
		let backup = remove_checked_file(&root, "main", &path, Some(current)).unwrap();
		assert!(!path.exists());
		assert_eq!(std::fs::read_to_string(root.join(backup)).unwrap(), current);
		drop(guard);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[actix_web::test]
	#[ignore = "opens an audio output device; run only with timed isolated-test authorization"]
	async fn reload_routes_reject_busy_root_before_runtime_changes() {
		use actix_web::{test as http, App};
		let root = temp_dir("reload-busy-root");
		let graph = root.join("graphs");
		std::fs::create_dir(&graph).unwrap();
		std::fs::write(graph.join("main.flowgraph.toml"), "[meta]\ntitle='retained'\n").unwrap();
		let mut conf: crate::Conf = toml::from_str("").unwrap();
		conf.runtime_dir = Some(root.join("runtime"));
		let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		let shared = state.read().await.flowgraph.clone();
		*shared.write().await = Some(FlowgraphRuntime::load(&graph));
		let app = http::init_service(App::new().app_data(Data::new(state.clone()))
			.service(post_reload).service(post_reload_preserving_state)
			.service(post_restore_profile_local_state_snapshot)
			.service(post_save_loaded_state_snapshot).service(post_save_live_state_snapshot)).await;
		let guard = write_lock::acquire(&graph).unwrap();
		for path in ["/flowgraph/reload", "/flowgraph/reload/preserve-state", "/flowgraph/state-snapshot/profile-local/restore", "/flowgraph/state-snapshot/loaded/save", "/flowgraph/state-snapshot/live/save"] {
			let response = http::call_service(&app, http::TestRequest::post().uri(path).to_request()).await;
			assert_eq!(response.status(), actix_web::http::StatusCode::CONFLICT, "{path}");
			let body: serde_json::Value = http::read_body_json(response).await;
			assert_eq!(body["error"], "flowgraph_busy", "{path}");
			assert!(shared.read().await.as_ref().unwrap().handle.is_none(), "rejection must not spawn workers");
			assert_eq!(std::fs::read_to_string(graph.join("main.flowgraph.toml")).unwrap(), "[meta]\ntitle='retained'\n");
		}
		drop(guard);
		drop(app);
		drop(state);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[actix_web::test]
	async fn busy_reload_keeps_live_worker_and_retry_replaces_it() {
		use actix_web::{test as http, App};
		let root = temp_dir("reload-live-contention");
		let graph = root.join("graphs");
		std::fs::create_dir(&graph).unwrap();
		std::fs::copy(state_counter_root().join("main.flowgraph.toml"), graph.join("main.flowgraph.toml")).unwrap();
		let mut conf: crate::Conf = toml::from_str("").unwrap();
		conf.runtime_dir = Some(root.join("runtime"));
		let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		let shared = state.read().await.flowgraph.clone();
		let original = FlowgraphRuntime::load_and_spawn(&graph, std::sync::Weak::new(), None, None, None, None);
		assert!(original.ok);
		let old_handle = original.handle.clone().unwrap();
		*shared.write().await = Some(original);
		let app = http::init_service(App::new().app_data(Data::new(state.clone()))
			.service(post_reload).service(post_reload_preserving_state)
			.service(post_restore_profile_local_state_snapshot)
			.service(post_save_loaded_state_snapshot).service(post_save_live_state_snapshot)).await;
		let guard = write_lock::acquire(&graph).unwrap();
		for (index, path) in ["/flowgraph/reload", "/flowgraph/reload/preserve-state", "/flowgraph/state-snapshot/profile-local/restore", "/flowgraph/state-snapshot/loaded/save", "/flowgraph/state-snapshot/live/save"].iter().enumerate() {
			let response = http::call_service(&app, http::TestRequest::post().uri(path).to_request()).await;
			assert_eq!(response.status(), 409, "{path}");
			let body: serde_json::Value = http::read_body_json(response).await;
			assert_eq!(body["error"], "flowgraph_busy");
			let runtime = shared.read().await;
			let runtime = runtime.as_ref().unwrap();
			runtime.trigger().unwrap().send_exec("main::in", "__trigger__").unwrap();
			let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), runtime.export_live_state_snapshot()).await.unwrap().unwrap().unwrap();
			assert_eq!(snapshot.nodes[0].value, serde_json::json!({"value":index + 1}), "worker must continue after {path}");
		}
		drop(guard);
		// Saving must succeed before state-preserving reload stops the live worker.
		let blocked_parent = root.join("not-a-directory");
		std::fs::write(&blocked_parent, b"retained").unwrap();
		shared.write().await.as_mut().unwrap().state_snapshot_file_path = Some(blocked_parent.join("saved.json"));
		let failed = http::call_service(&app, http::TestRequest::post().uri("/flowgraph/reload/preserve-state").to_request()).await;
		assert_eq!(failed.status(), 500);
		let body: serde_json::Value = http::read_body_json(failed).await;
		assert_eq!(body["error"], "state_snapshot_write_failed");
		assert_eq!(std::fs::read(&blocked_parent).unwrap(), b"retained");
		{
			let runtime = shared.read().await;
			let runtime = runtime.as_ref().unwrap();
			assert!(std::sync::Arc::ptr_eq(runtime.handle.as_ref().unwrap(), &old_handle));
			runtime.trigger().unwrap().send_exec("main::in", "__trigger__").unwrap();
			let snapshot = runtime.export_live_state_snapshot().await.unwrap().unwrap();
			assert_eq!(snapshot.nodes[0].value, serde_json::json!({"value":6}));
		}
		let saved = root.join("saved.json");
		shared.write().await.as_mut().unwrap().state_snapshot_file_path = Some(saved.clone());
		let preserved = http::call_service(&app, http::TestRequest::post().uri("/flowgraph/reload/preserve-state").to_request()).await;
		assert_eq!(preserved.status(), 200);
		let body: serde_json::Value = http::read_body_json(preserved).await;
		assert_eq!(body["ok"], true, "{body}");
		assert_eq!(body["saved_node_count"], 1);
		assert_eq!(body["restored_node_count"], 1);
		assert_eq!(crate::flowgraph::read_state_snapshot_file(&saved).unwrap().snapshot.nodes[0].value, serde_json::json!({"value":6}));
		let preserved_handle = {
			let runtime = shared.read().await;
			let runtime = runtime.as_ref().unwrap();
			assert!(!std::sync::Arc::ptr_eq(runtime.handle.as_ref().unwrap(), &old_handle));
			runtime.trigger().unwrap().send_exec("main::in", "__trigger__").unwrap();
			let snapshot = runtime.export_live_state_snapshot().await.unwrap().unwrap();
			assert_eq!(snapshot.nodes[0].value, serde_json::json!({"value":7}));
			runtime.handle.clone().unwrap()
		};
		let response = http::call_service(&app, http::TestRequest::post().uri("/flowgraph/reload").to_request()).await;
		assert_eq!(response.status(), 200);
		let body: serde_json::Value = http::read_body_json(response).await;
		assert_eq!(body["ok"], true, "{body}");
		assert!(old_handle.command.export_state_snapshot().await.is_err(), "old worker must shut down after successful reload");
		assert!(preserved_handle.command.export_state_snapshot().await.is_err(), "preserved worker must stop at later normal reload");
		let handle = {
			let runtime = shared.read().await;
			let runtime = runtime.as_ref().unwrap();
			runtime.trigger().unwrap().send_exec("main::in", "__trigger__").unwrap();
			let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), runtime.export_live_state_snapshot()).await.unwrap().unwrap().unwrap();
			assert_eq!(snapshot.nodes[0].value, serde_json::json!({"value":1}), "normal reload starts a fresh worker");
			runtime.handle.clone().unwrap()
		};
		let bridges = state.read().await.bridge_handles.clone();
		let owned = std::mem::replace(&mut *bridges.lock().await, crate::bridges::BridgeHandles::empty());
		owned.finish_all().await;
		handle.shutdown().await;
		drop(app);
		drop(state);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[actix_web::test]
	async fn failed_normal_reload_preserves_live_worker() {
		let root=temp_dir("reload-pending-live");
		let graph=root.join("graphs");std::fs::create_dir(&graph).unwrap();
		let file=graph.join("main.flowgraph.toml");
		let source=std::fs::read_to_string(state_counter_root().join("main.flowgraph.toml")).unwrap();
		std::fs::write(&file,&source).unwrap();
		let mut conf:crate::Conf=toml::from_str("").unwrap();conf.runtime_dir=Some(root.join("runtime"));
		let audio=std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test()));
		let state=crate::State::new(&conf,audio,crate::shutdown::ShutdownBroker::new()).await.unwrap();
		let shared=state.read().await.flowgraph.clone();
		let original=FlowgraphRuntime::load_and_spawn(&graph,std::sync::Weak::new(),None,None,None,None);
		let handle=original.handle.clone().unwrap();*shared.write().await=Some(original);
		for (index,pending) in [true,false].into_iter().enumerate() {
			if pending { std::fs::write(graph.join(crate::flowgraph::source_transaction::PENDING),"pending").unwrap(); }
			else {std::fs::remove_file(graph.join(crate::flowgraph::source_transaction::PENDING)).unwrap();std::fs::write(&file,"[ invalid").unwrap();}
			let (ok,_,_)=reload::reload_runtime_with_restart_status(&state,&graph).await;assert!(!ok);
			let runtime=shared.read().await;let runtime=runtime.as_ref().unwrap();
			assert!(std::sync::Arc::ptr_eq(runtime.handle.as_ref().unwrap(),&handle));
			runtime.trigger().unwrap().send_exec("main::in","__trigger__").unwrap();
			let snapshot=tokio::time::timeout(std::time::Duration::from_secs(3),runtime.export_live_state_snapshot()).await.unwrap().unwrap().unwrap();
			assert_eq!(snapshot.nodes[0].value,serde_json::json!({"value":index+1}));
		}
		std::fs::write(&file,&source).unwrap();
		let (ok,_,_)=reload::reload_runtime_with_restart_status(&state,&graph).await;assert!(ok);
		let replacement={
			let runtime=shared.read().await;let runtime=runtime.as_ref().unwrap();
			assert!(!std::sync::Arc::ptr_eq(runtime.handle.as_ref().unwrap(),&handle));
			runtime.trigger().unwrap().send_exec("main::in","__trigger__").unwrap();
			let snapshot=tokio::time::timeout(std::time::Duration::from_secs(3),runtime.export_live_state_snapshot()).await.unwrap().unwrap().unwrap();
			assert_eq!(snapshot.nodes[0].value,serde_json::json!({"value":1}));
			runtime.handle.clone().unwrap()
		};
		let bridges=state.read().await.bridge_handles.clone();
		let owned=std::mem::replace(&mut *bridges.lock().await,crate::bridges::BridgeHandles::empty());
		owned.finish_all().await;replacement.shutdown().await;drop(state);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[actix_web::test]
	async fn failed_snapshot_restore_preserves_worker_then_valid_restore_succeeds() {
		use actix_web::{test as http, App};
		let root = temp_dir("restore-live-failure");
		let graph = root.join("graphs");
		std::fs::create_dir(&graph).unwrap();
		std::fs::copy(state_counter_root().join("main.flowgraph.toml"), graph.join("main.flowgraph.toml")).unwrap();
		let snapshot_path = root.join("saved.json");
		let mut conf: crate::Conf = toml::from_str("").unwrap();
		conf.runtime_dir = Some(root.join("runtime"));
		let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		let shared = state.read().await.flowgraph.clone();
		let mut original = FlowgraphRuntime::load_and_spawn(&graph, std::sync::Weak::new(), None, None, None, None);
		assert!(original.ok);
		original.state_snapshot_file_path = Some(snapshot_path.clone());
		let old_handle = original.handle.clone().unwrap();
		*shared.write().await = Some(original);
		let app = http::init_service(App::new().app_data(Data::new(state.clone()))
			.service(post_restore_profile_local_state_snapshot)).await;
		for (index, corrupt) in [false, true].into_iter().enumerate() {
			if corrupt { std::fs::write(&snapshot_path, "{ invalid snapshot").unwrap(); }
			let response = http::call_service(&app, http::TestRequest::post().uri("/flowgraph/state-snapshot/profile-local/restore").to_request()).await;
			let body: serde_json::Value = http::read_body_json(response).await;
			assert_eq!(body["ok"], false);
			assert!(body["restored_node_count"].is_null());
			let runtime = shared.read().await;
			let runtime = runtime.as_ref().unwrap();
			assert!(std::sync::Arc::ptr_eq(runtime.handle.as_ref().unwrap(), &old_handle));
			runtime.trigger().unwrap().send_exec("main::in", "__trigger__").unwrap();
			let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), runtime.export_live_state_snapshot()).await.unwrap().unwrap().unwrap();
			assert_eq!(snapshot.nodes[0].value, serde_json::json!({"value":index + 1}));
		}
		{
			let runtime = shared.read().await;
			save_live_state_snapshot_response(runtime.as_ref().unwrap()).await.unwrap();
		}
		let response = http::call_service(&app, http::TestRequest::post().uri("/flowgraph/state-snapshot/profile-local/restore").to_request()).await;
		let body: serde_json::Value = http::read_body_json(response).await;
		assert_eq!(body["ok"], true, "{body}");
		assert_eq!(body["restored_node_count"], 1);
		let handle = {
			let runtime = shared.read().await;
			let runtime = runtime.as_ref().unwrap();
			assert!(!std::sync::Arc::ptr_eq(runtime.handle.as_ref().unwrap(), &old_handle));
			runtime.trigger().unwrap().send_exec("main::in", "__trigger__").unwrap();
			let snapshot = runtime.export_live_state_snapshot().await.unwrap().unwrap();
			assert_eq!(snapshot.nodes[0].value, serde_json::json!({"value":3}));
			runtime.handle.clone().unwrap()
		};
		let bridges = state.read().await.bridge_handles.clone();
		let owned = std::mem::replace(&mut *bridges.lock().await, crate::bridges::BridgeHandles::empty());
		owned.finish_all().await;
		handle.shutdown().await;
		drop(app);
		drop(state);
		std::fs::remove_dir_all(root).unwrap();
	}

	fn state_counter_root() -> PathBuf {
		std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("flowgraph.example")
			.join("state-counter")
	}

	#[actix_web::test]
	async fn http_tree_and_runtime_revisions_track_save_then_reload() {
		use actix_web::{test as http, App};
		let root = temp_dir("http-revision");
		let graph_root = root.join("graphs");
		std::fs::create_dir_all(&graph_root).unwrap();
		let path = graph_root.join("main.flowgraph.toml");
		let original = "[[nodes]]\nid='value'\nfeature='flowgraph.literal.string'\n[nodes.properties]\nvalue='original'\n";
		std::fs::write(&path, original).unwrap();
		// No configured bridges, accounts, avatar links or executing graph workers.
		let mut conf: crate::Conf = toml::from_str("").unwrap();
		conf.runtime_dir = Some(root.join("runtime"));
		let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		let shared = state.read().await.flowgraph.clone();
		*shared.write().await = Some(FlowgraphRuntime::load(&graph_root));
		let app = http::init_service(App::new().app_data(Data::new(state.clone())).service(get_tree).service(get_diagnostics).service(get_file).service(fragment_zip::get_companions)).await;
		let tree: serde_json::Value = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/flowgraph/tree").to_request()).await;
		let loaded: serde_json::Value = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/flowgraph/diagnostics").to_request()).await;
		assert_eq!(loaded["ok"], true);
		let original_hash = tree["files"][0]["source_digest"].as_str().expect("saved digest").to_owned();
		assert_eq!(loaded["source_digests"]["main"], original_hash);
		assert_eq!(tree["root_dir"], loaded["root_dir"]);
		let opened:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::get().uri("/flowgraph/file/main").to_request()).await;
		assert_eq!(opened["root_dir"],tree["root_dir"]);
		let companions:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::get().uri("/flowgraph/fragment/companions").to_request()).await;
		assert_eq!(companions["root_dir"],tree["root_dir"]);
		std::fs::write(&path, original.replace("original", "edited")).unwrap();
		let changed: serde_json::Value = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/flowgraph/tree").to_request()).await;
		let retained: serde_json::Value = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/flowgraph/diagnostics").to_request()).await;
		assert_ne!(changed["files"][0]["source_digest"], original_hash);
		assert_eq!(retained["source_digests"]["main"], original_hash);
		*shared.write().await = Some(FlowgraphRuntime::load(&graph_root));
		let reloaded: serde_json::Value = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/flowgraph/diagnostics").to_request()).await;
		assert_eq!(reloaded["source_digests"]["main"], changed["files"][0]["source_digest"]);
		drop(app);
		drop(state);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[actix_web::test]
	async fn native_http_delete_restore_reloads_and_survives_runtime_reconstruction() {
		use actix_web::{test as http, App};
		let root=temp_dir("http-delete-restore");
		let graphs=root.join("graphs");std::fs::create_dir(&graphs).unwrap();
		let path=graphs.join("main.flowgraph.toml");
		let content="[[nodes]]\nid='value'\nfeature='flowgraph.literal.string'\n[nodes.properties]\nvalue='retained value'\n";
		std::fs::write(&path,content).unwrap();
		let mut conf:crate::Conf=toml::from_str("").unwrap();conf.runtime_dir=Some(root.join("runtime"));
		let audio=std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
		let state=crate::State::new(&conf,audio,crate::shutdown::ShutdownBroker::new()).await.unwrap();
		let shared=state.read().await.flowgraph.clone();
		*shared.write().await=Some(FlowgraphRuntime::load(&graphs));
		let app=http::init_service(App::new().app_data(Data::new(state.clone()))
			.service(get_removal_plan).service(delete_file).service(get_tree).service(get_diagnostics)
			.service(file_recovery::preview_restore).service(file_recovery::restore_file).service(file_recovery::recovery_history)
			.service(fragment_zip::post_import_zip).service(fragment_zip::post_export_zip)).await;
		for dry in ["true", "false"] {
			let refused=http::call_service(&app,http::TestRequest::post().uri(&format!("/flowgraph/import/zip?dry_run={dry}&expected_root=other-profile")).set_payload("invalid archive must not be inspected before root rejection").to_request()).await;
			assert_eq!(refused.status(),409);
			let body:serde_json::Value=http::read_body_json(refused).await;
			assert_eq!(body["error"],"import_root_changed");
			assert_eq!(std::fs::read_to_string(&path).unwrap(),content);
		}
		let plan:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::get().uri("/flowgraph/removal-plan/main").to_request()).await;
		assert_eq!(plan["can_delete"],true);
		let wrong_root=http::call_service(&app,http::TestRequest::delete().uri("/flowgraph/file/main").set_json(serde_json::json!({"expected_content":content,"expected_root":"other-profile"})).to_request()).await;
		assert_eq!(wrong_root.status(),409);assert_eq!(std::fs::read_to_string(&path).unwrap(),content);
		let rejected=http::call_service(&app,http::TestRequest::delete().uri("/flowgraph/file/main").set_json(serde_json::json!({"expected_content":"stale"})).to_request()).await;
		assert_eq!(rejected.status(),409);assert_eq!(std::fs::read_to_string(&path).unwrap(),content);
		let deleted:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::delete().uri("/flowgraph/file/main").set_json(serde_json::json!({"expected_content":content,"expected_root":plan["root_dir"]})).to_request()).await;
		assert!(!path.exists());
		let backup=deleted["backup"].as_str().unwrap();
		assert_eq!(std::fs::read_to_string(graphs.join(backup)).unwrap(),content);
		let recovery:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::post().uri("/flowgraph/recovery-plan/main").set_json(serde_json::json!({"backup":backup})).to_request()).await;
		assert_eq!(recovery["current"]["state"],"missing");
		let restored:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::post().uri("/flowgraph/restore-file/main").set_json(&recovery).to_request()).await;
		assert_eq!(restored["ok"],true,"{restored}");
		assert_eq!(std::fs::read_to_string(&path).unwrap(),content);
		let tree:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::get().uri("/flowgraph/tree").to_request()).await;
		let loaded:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::get().uri("/flowgraph/diagnostics").to_request()).await;
		assert_eq!(tree["files"][0]["source_digest"],loaded["source_digests"]["main"]);
		assert!(shared.read().await.as_ref().unwrap().handle.is_some());
		let history:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::get().uri("/flowgraph/recovery-history").to_request()).await;
		assert_eq!(history["entries"][0]["current_present"],true);
		// Exercise the positive ZIP HTTP path with the same root identity used by
		// the GUI. Preview must not write; repeated skip imports must preserve edits.
		let copy=serde_json::from_value(serde_json::json!({"scope":"file","targets":[{"kind":"file","fq":"main"}]})).unwrap();
		let archive=crate::flowgraph::fragment::zip_codec::export_zip(&graphs,&copy).unwrap();
		let inventory:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::get().uri("/flowgraph/tree").to_request()).await;
		assert_eq!(inventory["root_dir"],graph_root_identity(&graphs));
		let exported=http::call_service(&app,http::TestRequest::post().uri("/flowgraph/export/zip").set_json(serde_json::json!({"scope":"file","targets":[{"kind":"file","fq":"main"}],"expected_root":inventory["root_dir"]})).to_request()).await;
		assert_eq!(exported.status(),200);
		let encoded_root=inventory["root_dir"].as_str().unwrap().bytes().map(|byte|format!("%{byte:02X}")).collect::<String>();
		let import_uri=format!("/flowgraph/import/zip?expected_root={encoded_root}&target_prefix=imported&on_conflict_file=skip");
		let preview:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::post().uri(&format!("{import_uri}&dry_run=true")).set_payload(archive.clone()).to_request()).await;
		assert_eq!(preview["kind"],"preview","{preview}");
		let imported_path=graphs.join("imported/main.flowgraph.toml");
		assert!(!imported_path.exists());
		let imported:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::post().uri(&import_uri).set_payload(archive.clone()).to_request()).await;
		assert_eq!(imported["reload_ok"],true,"{imported}");
		assert!(imported_path.exists());
		let edited=format!("# User-edited imported graph\n{}",std::fs::read_to_string(&imported_path).unwrap());
		std::fs::write(&imported_path,&edited).unwrap();
		let skipped:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::post().uri(&import_uri).set_payload(archive).to_request()).await;
		assert_eq!(skipped["reload_ok"],true,"{skipped}");
		assert_eq!(skipped["written_files"].as_array().unwrap().len(),0);
		assert_eq!(std::fs::read_to_string(&imported_path).unwrap(),edited);
		assert_eq!(std::fs::read_to_string(&path).unwrap(),content);
		let tree:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::get().uri("/flowgraph/tree").to_request()).await;
		let loaded:serde_json::Value=http::call_and_read_body_json(&app,http::TestRequest::get().uri("/flowgraph/diagnostics").to_request()).await;
		assert_eq!(tree["files"].as_array().unwrap().len(),2);
		for file in tree["files"].as_array().unwrap() {
			assert_eq!(file["source_digest"],loaded["source_digests"][file["fq"].as_str().unwrap()]);
		}
		// Dispose only this test's worker/bridges, then reconstruct from disk.
		let bridges=state.read().await.bridge_handles.clone();
		std::mem::replace(&mut *bridges.lock().await,crate::bridges::BridgeHandles::empty()).finish_all().await;
		shared.read().await.as_ref().unwrap().handle.clone().unwrap().shutdown().await;
		let fresh=FlowgraphRuntime::load(&graphs);
		assert!(fresh.ok);
		assert_eq!(serde_json::to_value(&fresh.source_digests).unwrap(),loaded["source_digests"]);
		drop(app);drop(state);drop(shared);drop(fresh);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[actix_web::test]
	#[ignore = "manual isolated API integration with browser-saved counter artifact"]
	async fn browser_counter_save_reloads_and_rejects_stale_target() {
		use actix_web::{test as http, App};
		let artifact = std::env::var("UNVAC_BROWSER_COUNTER_ARTIFACT").unwrap();
		let content = std::fs::read_to_string(artifact).unwrap();
		let parsed: toml::Value = toml::from_str(&content).unwrap();
		let nodes = parsed["nodes"].as_array().unwrap();
		assert_eq!(nodes.len(), 5);
		for node in nodes {
			assert!(matches!(node["feature"].as_str().unwrap(),
				"flowgraph.ingress.web_input" | "flowgraph.state.int_counter" |
				"flowgraph.convert.int_to_string" | "flowgraph.util.log"),
				"Only the isolated counter features may execute in this test");
		}
		let root = temp_dir("browser-counter-api");
		let graph_root = root.join("graphs");
		std::fs::create_dir_all(&graph_root).unwrap();
		let path = graph_root.join("main.flowgraph.toml");
		let baseline = format!("# Before browser save\n{content}");
		std::fs::write(&path, &baseline).unwrap();
		let mut conf: crate::Conf = toml::from_str("").unwrap();
		conf.runtime_dir = Some(root.join("runtime"));
		let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		let shared = state.read().await.flowgraph.clone();
		*shared.write().await = Some(FlowgraphRuntime::load(&graph_root));
		let app = http::init_service(App::new().app_data(Data::new(state.clone()))
			.service(put_file).service(get_tree).service(get_diagnostics)).await;
		let body = serde_json::json!({"content":content,"expected_content":baseline,
			"expected_root":graph_root_identity(&graph_root)});
		let save = http::call_service(&app, http::TestRequest::put().uri("/flowgraph/file/main").set_json(&body).to_request()).await;
		assert_eq!(save.status(), 200);
		let saved: serde_json::Value = http::read_body_json(save).await;
		assert_eq!(saved["ok"], true, "{saved}");
		assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
		assert!(shared.read().await.as_ref().unwrap().handle.is_some(), "save must spawn the actual runtime worker");
		let tree: serde_json::Value = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/flowgraph/tree").to_request()).await;
		let loaded: serde_json::Value = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/flowgraph/diagnostics").to_request()).await;
		assert_eq!(loaded["ok"], true);
		assert_eq!(tree["files"][0]["source_digest"], loaded["source_digests"]["main"]);
		for wrong_root in [false, true] {
			let mut rejected = body.clone();
			if wrong_root {
				rejected["expected_root"] = serde_json::json!(root.join("other").to_string_lossy());
				rejected["expected_content"] = serde_json::json!(content);
			}
			let response = http::call_service(&app, http::TestRequest::put().uri("/flowgraph/file/main").set_json(&rejected).to_request()).await;
			assert_eq!(response.status(), 409);
			assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
		}
		// Tear down only this test's bridges and worker before removing its files.
		let bridges = state.read().await.bridge_handles.clone();
		let owned = std::mem::replace(&mut *bridges.lock().await, crate::bridges::BridgeHandles::empty());
		owned.finish_all().await;
		let handle = shared.read().await.as_ref().unwrap().handle.clone().unwrap();
		handle.shutdown().await;
		drop(app);
		drop(state);
		std::fs::remove_dir_all(root).unwrap();
	}

	fn runtime_with_snapshot_path(path: Option<PathBuf>) -> FlowgraphRuntime {
		FlowgraphRuntime {
			root_dir: PathBuf::from("flowgraph.example"),
			ok: true,
			diagnostics: vec![],
			node_meta: HashMap::new(),
			speech_routes: Vec::new(),
			graph_signature: GraphSignature::default(),
			package_manifests: Vec::new(),
			source_digests: Default::default(),
			package_dependency_order: Vec::new(),
			package_lock_preview: Vec::new(),
			type_schemas: Vec::new(),
			type_schema_dependency_order: Vec::new(),
			type_schema_graph_summary: crate::flowgraph::loader::TypeSchemaGraphSummary::default(),
			package_lock_preview_digest: None,
			capability_summary: GraphCapabilitySummary::default(),
			loaded_state_summary: ProgramStateSummary::default(),
			loaded_state_snapshot: snapshot(),
			loaded_state_restore_report: None,
			state_snapshot_file_path: path,
			file_activation: HashMap::new(),
			composition: Default::default(),
			trigger_gate: None,
			handle: None,
		}
	}

	#[test]
	fn parse_socket_type_response_reports_canonical_expr() {
		let response = parse_socket_type_response(" map<string, list<json>> ");
		assert!(response.valid);
		assert_eq!(response.canonical_type.as_deref(), Some("map<list<json>>"));
		let expr = response.type_expr.expect("type expr");
		assert_eq!(expr.name, "map");
		assert_eq!(expr.args[0].name, "string");
		assert_eq!(expr.args[1].name, "list");
		assert_eq!(expr.args[1].args[0].name, "json");
		assert!(response.error.is_none());

		let option_response = parse_socket_type_response("option<list<string>>");
		assert!(option_response.valid);
		assert_eq!(option_response.canonical_type.as_deref(), Some("option<list<string>>"));
		let option_expr = option_response.type_expr.expect("option expr");
		assert_eq!(option_expr.name, "option");
		assert_eq!(option_expr.args[0].name, "list");

		let dictionary_response = parse_socket_type_response("dictionary<string, list<json>>");
		assert!(dictionary_response.valid);
		assert_eq!(dictionary_response.canonical_type.as_deref(), Some("map<list<json>>"));
		let dictionary_expr = dictionary_response.type_expr.expect("dictionary expr");
		assert_eq!(dictionary_expr.name, "map");
		assert_eq!(dictionary_expr.args[0].name, "string");
		assert_eq!(dictionary_expr.args[1].name, "list");

		let collection_response = parse_socket_type_response("collection<option<string>>");
		assert!(collection_response.valid);
		assert_eq!(collection_response.canonical_type.as_deref(), Some("list<option<string>>"));
		let collection_expr = collection_response.type_expr.expect("collection expr");
		assert_eq!(collection_expr.name, "list");
		assert_eq!(collection_expr.args[0].name, "option");

		let record_response = parse_socket_type_response("record<twitch.event>");
		assert!(record_response.valid);
		assert_eq!(record_response.canonical_type.as_deref(), Some("record<twitch.event>"));
		let record_expr = record_response.type_expr.expect("record expr");
		assert_eq!(record_expr.name, "record");
		assert_eq!(record_expr.display, "record<twitch.event>");
	}

	#[test]
	fn parse_socket_type_response_reports_errors() {
		let response = parse_socket_type_response("map<int, string>");
		assert!(!response.valid);
		assert!(response.canonical_type.is_none());
		assert!(response.type_expr.is_none());
		assert!(response.error.as_deref().unwrap_or_default().contains("map の key 型は string"));
	}

	#[test]
	fn socket_type_compatibility_response_reports_engine_rule() {
		let response = socket_type_compatibility_response("list<float>", "list<quantity>");
		assert!(response.valid);
		assert_eq!(response.compatible, Some(true));
		assert_eq!(response.from_canonical_type.as_deref(), Some("list<float>"));
		assert_eq!(response.to_canonical_type.as_deref(), Some("list<quantity>"));
		assert_eq!(response.from_type_expr.as_ref().expect("from expr").name, "list");
		assert_eq!(response.to_type_expr.as_ref().expect("to expr").args[0].name, "quantity");
		assert!(response.error.is_none());

		let rejected = socket_type_compatibility_response("string", "quantity");
		assert!(rejected.valid);
		assert_eq!(rejected.compatible, Some(false));

		let option = socket_type_compatibility_response("option<list<float>>", "option<list<quantity>>");
		assert!(option.valid);
		assert_eq!(option.compatible, Some(true));
		assert_eq!(option.from_type_expr.as_ref().expect("from expr").name, "option");
		assert_eq!(option.to_type_expr.as_ref().expect("to expr").args[0].name, "list");

		let record = socket_type_compatibility_response("record<twitch.event>", "record<twitch.event>");
		assert!(record.valid);
		assert_eq!(record.compatible, Some(true));
		let different_record = socket_type_compatibility_response("record<twitch.event>", "record<obs.event>");
		assert!(different_record.valid);
		assert_eq!(different_record.compatible, Some(false));
		let json_to_record = socket_type_compatibility_response("json", "record<twitch.event>");
		assert!(json_to_record.valid);
		assert_eq!(json_to_record.compatible, Some(true));
	}

	#[test]
	fn socket_type_compatibility_response_reports_parse_errors() {
		let response = socket_type_compatibility_response("map<int, string>", "list<>");
		assert!(!response.valid);
		assert!(response.compatible.is_none());
		let error = response.error.as_deref().unwrap_or_default();
		assert!(error.contains("from:"), "{error}");
		assert!(error.contains("to:"), "{error}");
	}

	#[test]
	fn save_loaded_state_snapshot_response_writes_snapshot_file() {
		let path = temp_dir("ok").join("profile").join("state.snapshot.json");
		let rt = runtime_with_snapshot_path(Some(path.clone()));

		let response = save_loaded_state_snapshot_response(&rt).expect("save snapshot");

		assert!(response.path.ends_with("/profile/state.snapshot.json"));
		assert_eq!(response.snapshot_node_count, 1);
		assert!(response.written);
		let file = crate::flowgraph::read_state_snapshot_file(&path).expect("read snapshot file");
		assert_eq!(file.snapshot, rt.loaded_state_snapshot);
	}

	#[test]
	fn save_loaded_state_snapshot_response_reports_missing_path() {
		let rt = runtime_with_snapshot_path(None);
		let error = save_loaded_state_snapshot_response(&rt).expect_err("missing path");
		assert!(matches!(error, SaveLoadedStateSnapshotError::PathUnset));
	}

	#[test]
	fn save_loaded_state_snapshot_response_reports_write_failure() {
		let parent_file = temp_dir("write-failed").join("not-a-directory");
		std::fs::write(&parent_file, "occupied").expect("create parent file");
		let path = parent_file.join("state.snapshot.json");
		let rt = runtime_with_snapshot_path(Some(path.clone()));

		let error = save_loaded_state_snapshot_response(&rt).expect_err("write failure");

		match error {
			SaveLoadedStateSnapshotError::WriteFailed {
				path: actual_path,
				error: _,
			} => {
				assert_eq!(actual_path, path);
			}
			SaveLoadedStateSnapshotError::PathUnset => panic!("expected write failure"),
		}
	}

	#[test]
	fn package_lock_preview_response_reports_digest_and_count() {
		let mut rt = runtime_with_snapshot_path(None);
		rt.package_lock_preview_digest = Some("b3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into());
		rt.package_lock_preview = vec![PackageLockEntry {
			id: "example.pkg".into(),
			version: Some("1.0.0".into()),
			source_fq: "main".into(),
			source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
			digest: "b3:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".into(),
			dependencies: BTreeMap::new(),
		}];

		let response = package_lock_preview_response(&rt);

		assert_eq!(response.digest, rt.package_lock_preview_digest);
		assert_eq!(response.entry_count, 1);
		assert_eq!(response.entries[0].id, "example.pkg");
	}

	#[test]
	fn save_package_lock_preview_response_writes_lockfile() {
		let root = temp_dir("package-lock-save");
		let mut rt = runtime_with_snapshot_path(None);
		rt.root_dir = root.clone();
		rt.package_lock_preview_digest = Some("b3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into());
		rt.package_lock_preview = vec![PackageLockEntry {
			id: "example.pkg".into(),
			version: Some("1.0.0".into()),
			source_fq: "main".into(),
			source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
			digest: "b3:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".into(),
			dependencies: BTreeMap::new(),
		}];

		let response = save_package_lock_preview_response(&rt, None).expect("save package lock");

		assert!(response.path.ends_with("/flowgraph.lock.json"));
		assert_eq!(response.entry_count, 1);
		assert!(response.written);
		let decoded = crate::flowgraph::read_package_lock_file(root.join(crate::flowgraph::FLOWGRAPH_PACKAGE_LOCK_FILE_NAME))
			.expect("read package lock");
		assert_eq!(decoded.entries[0].id, "example.pkg");
		let _ = std::fs::remove_dir_all(root);
	}

	#[test]
	fn package_lock_status_response_reports_missing_file() {
		let root = temp_dir("package-lock-missing");
		let mut rt = runtime_with_snapshot_path(None);
		rt.root_dir = root.clone();
		rt.package_lock_preview_digest = Some("b3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into());

		let response = package_lock_status_response(&rt);

		assert!(response.path.ends_with("/flowgraph.lock.json"));
		assert!(!response.exists);
		assert_eq!(response.preview_digest, rt.package_lock_preview_digest);
		assert_eq!(response.matches_preview, None);
		assert!(response.file.is_none());
		assert!(response.error.is_none());
		let _ = std::fs::remove_dir_all(root);
	}

	#[test]
	fn package_lock_status_response_compares_saved_digest() {
		let root = temp_dir("package-lock-status");
		let mut rt = runtime_with_snapshot_path(None);
		rt.root_dir = root.clone();
		rt.package_lock_preview_digest = Some("b3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into());
		rt.package_lock_preview = vec![PackageLockEntry {
			id: "example.pkg".into(),
			version: Some("1.0.0".into()),
			source_fq: "main".into(),
			source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
			digest: "b3:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".into(),
			dependencies: BTreeMap::new(),
		}];
		save_package_lock_preview_response(&rt, None).expect("save package lock");

		let response = package_lock_status_response(&rt);

		assert!(response.exists);
		assert_eq!(response.lock_digest, rt.package_lock_preview_digest);
		assert_eq!(response.matches_preview, Some(true));
		assert_eq!(response.entry_count, Some(1));
		assert_eq!(response.file.as_ref().expect("file").entries[0].id, "example.pkg");

		rt.package_lock_preview_digest = Some("b3:1111111111111111111111111111111111111111111111111111111111111111".into());
		let stale = package_lock_status_response(&rt);
		assert_eq!(stale.matches_preview, Some(false));
		let _ = std::fs::remove_dir_all(root);
	}

	#[test]
	fn package_lock_status_response_reports_diff_payload() {
		let root = temp_dir("package-lock-diff");
		let mut rt = runtime_with_snapshot_path(None);
		rt.root_dir = root.clone();
		rt.package_lock_preview_digest = Some("b3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into());
		rt.package_lock_preview = vec![
			PackageLockEntry {
				id: "example.changed".into(),
				version: Some("1.0.0".into()),
				source_fq: "changed".into(),
				source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
				digest: "b3:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
				dependencies: BTreeMap::new(),
			},
			PackageLockEntry {
				id: "example.removed".into(),
				version: Some("1.0.0".into()),
				source_fq: "removed".into(),
				source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
				digest: "b3:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
				dependencies: BTreeMap::new(),
			},
			PackageLockEntry {
				id: "example.same".into(),
				version: Some("1.0.0".into()),
				source_fq: "same".into(),
				source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
				digest: "b3:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
				dependencies: BTreeMap::new(),
			},
		];
		save_package_lock_preview_response(&rt, None).expect("save package lock");

		rt.package_lock_preview_digest = Some("b3:1111111111111111111111111111111111111111111111111111111111111111".into());
		rt.package_lock_preview = vec![
			PackageLockEntry {
				id: "example.added".into(),
				version: Some("1.0.0".into()),
				source_fq: "added".into(),
				source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
				digest: "b3:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".into(),
				dependencies: BTreeMap::new(),
			},
			PackageLockEntry {
				id: "example.changed".into(),
				version: Some("1.0.0".into()),
				source_fq: "changed".into(),
				source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
				digest: "b3:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".into(),
				dependencies: BTreeMap::new(),
			},
			PackageLockEntry {
				id: "example.same".into(),
				version: Some("1.0.0".into()),
				source_fq: "same".into(),
				source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
				digest: "b3:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
				dependencies: BTreeMap::new(),
			},
		];

		let response = package_lock_status_response(&rt);
		let diff = response.diff.expect("diff");

		assert_eq!(diff.added_ids, vec!["example.added".to_string()]);
		assert_eq!(diff.removed_ids, vec!["example.removed".to_string()]);
		assert_eq!(diff.changed_ids, vec!["example.changed".to_string()]);
		assert_eq!(diff.unchanged_ids, vec!["example.same".to_string()]);
		let _ = std::fs::remove_dir_all(root);
	}

	#[test]
	fn save_package_lock_preview_response_honors_if_match() {
		let root = temp_dir("package-lock-if-match");
		let mut rt = runtime_with_snapshot_path(None);
		rt.root_dir = root.clone();
		rt.package_lock_preview_digest = Some("b3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into());
		rt.package_lock_preview = vec![PackageLockEntry {
			id: "example.pkg".into(),
			version: Some("1.0.0".into()),
			source_fq: "main".into(),
			source_digest: "b3:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789".into(),
			digest: "b3:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".into(),
			dependencies: BTreeMap::new(),
		}];

		let missing = save_package_lock_preview_response(&rt, Some("b3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"))
			.expect_err("missing lock should conflict");
		assert!(matches!(
			missing,
			SavePackageLockPreviewError::OptimisticLockFailed { current_digest: None }
		));

		save_package_lock_preview_response(&rt, None).expect("initial save");
		save_package_lock_preview_response(&rt, Some("\"b3:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\""))
			.expect("matching if-match");

		let stale = save_package_lock_preview_response(&rt, Some("b3:1111111111111111111111111111111111111111111111111111111111111111"))
			.expect_err("stale if-match");
		assert!(matches!(
			stale,
			SavePackageLockPreviewError::OptimisticLockFailed { current_digest: Some(_) }
		));
		let bad = save_package_lock_preview_response(&rt, Some("not-a-digest")).expect_err("bad if-match");
		assert!(matches!(bad, SavePackageLockPreviewError::BadIfMatch { .. }));
		let _ = std::fs::remove_dir_all(root);
	}

	#[tokio::test]
	async fn save_live_state_snapshot_response_reports_missing_path() {
		let rt = runtime_with_snapshot_path(None);
		let error = save_live_state_snapshot_response(&rt).await.expect_err("missing path");
		assert!(matches!(error, SaveLiveStateSnapshotError::PathUnset));
	}

	#[tokio::test]
	async fn save_live_state_snapshot_response_reports_missing_worker() {
		let path = temp_dir("live-no-worker").join("profile").join("state.snapshot.json");
		let rt = runtime_with_snapshot_path(Some(path));
		let error = save_live_state_snapshot_response(&rt).await.expect_err("missing worker");
		assert!(matches!(error, SaveLiveStateSnapshotError::WorkerUnavailable));
	}

	#[tokio::test]
	async fn save_live_state_snapshot_response_writes_worker_snapshot_file() {
		let path = temp_dir("live-ok").join("profile").join("state.snapshot.json");
		let mut rt = FlowgraphRuntime::load_and_spawn(&state_counter_root(), std::sync::Weak::new(), None, None, None, None);
		rt.state_snapshot_file_path = Some(path.clone());
		rt.trigger()
			.expect("trigger handle")
			.send_exec("main::in", "__trigger__")
			.expect("send trigger");

		let response = save_live_state_snapshot_response(&rt).await.expect("save live snapshot");

		assert!(response.path.ends_with("/profile/state.snapshot.json"));
		assert_eq!(response.snapshot_node_count, 1);
		assert!(response.written);
		let file = crate::flowgraph::read_state_snapshot_file(&path).expect("read snapshot file");
		assert_eq!(file.snapshot.nodes[0].version, 1);
		assert_eq!(file.snapshot.nodes[0].value, serde_json::json!({ "value": 1 }));
		if let Some(handle) = rt.handle.as_ref() {
			let trace = handle.runtime_trace.read().expect("runtime trace");
			assert!(trace.iter().any(|event| {
				event.runtime_node_id == "main::in" && event.source_fq == "main" && event.instance_path.is_empty() && event.active
			}));
			drop(trace);
			handle.shutdown().await;
		}
	}

	#[test]
	fn state_snapshot_file_status_reports_none_without_path() {
		let rt = runtime_with_snapshot_path(None);
		assert_eq!(state_snapshot_file_status(&rt), None);
	}

	#[test]
	fn state_snapshot_file_status_reports_missing_file() {
		let path = temp_dir("status-missing").join("profile").join("state.snapshot.json");
		let rt = runtime_with_snapshot_path(Some(path));

		let status = state_snapshot_file_status(&rt).expect("status");

		assert!(status.path.ends_with("/profile/state.snapshot.json"));
		assert!(!status.exists);
	}

	#[test]
	fn state_snapshot_file_status_reports_existing_file() {
		let path = temp_dir("status-existing").join("profile").join("state.snapshot.json");
		let rt = runtime_with_snapshot_path(Some(path.clone()));
		rt.write_loaded_state_snapshot_file().expect("write snapshot");

		let status = state_snapshot_file_status(&rt).expect("status");

		assert!(status.path.ends_with("/profile/state.snapshot.json"));
		assert!(status.exists);
	}

	#[test]
	fn composition_activity_marks_inactive_portal_and_boundary() {
		let mut composition = crate::flowgraph::loader::GraphCompositionSummary {
			graphs: Vec::new(),
			instances: vec![crate::flowgraph::loader::GraphInstanceSummary {
				owner_fq: "main".into(),
				id: "child".into(),
				graph_fq: "module".into(),
				runtime_fq: "main/@child".into(),
				position: None,
				active: true,
			}],
			portals: vec![crate::flowgraph::loader::CrossFilePortalSummary {
				owner_fq: "main".into(),
				from: "source:value".into(),
				to: "target::sink:value".into(),
				from_fq: "main".into(),
				to_fq: "target".into(),
				from_active: true,
				to_active: true,
				active: true,
			}],
		};
		let meta = |file: &str| crate::flowgraph::loader::LoadedNodeMeta {
			feature: "flowgraph.literal.string".into(),
			file: std::path::PathBuf::from(file),
			position: None,
			activation_chain: Vec::new(),
			properties: Default::default(),
		};
		let node_meta = std::collections::HashMap::from([
			("main::source".into(), meta("main")),
			("target::sink".into(), meta("target")),
			("main/@child::node".into(), meta("module")),
		]);
		let inactive = std::collections::HashSet::from(["target::sink".to_string(), "main/@child::node".to_string()]);

		let diagnostics = apply_composition_activity(&mut composition, &node_meta, &inactive);

		assert!(!composition.instances[0].active);
		assert!(composition.portals[0].from_active);
		assert!(!composition.portals[0].to_active);
		assert!(!composition.portals[0].active);
		assert_eq!(diagnostics[0].code, crate::flowgraph::DiagnosticCode::ActivationBoundary);
	}
}

#[cfg(test)]
mod package_removal_guard_tests {
	#[test]
	fn ordinary_file_removal_retains_package_boundary() {
		let root=std::env::temp_dir().join(format!("vac-file-package-{}",rand::random::<u64>()));
		std::fs::create_dir_all(root.join("feature")).unwrap();
		let manifest="[package]\nid='test.feature'\n";
		std::fs::write(root.join("feature/package.flowgraph.toml"),manifest).unwrap();
		std::fs::write(root.join("feature/module.flowgraph.toml"),"[graph]\nkind='module'\n").unwrap();
		let plan=super::removal_plan_snapshot(&root,"feature/package").unwrap();
		assert_eq!(plan["can_delete"],false);
		assert!(plan["diagnostics"][0]["message"].as_str().unwrap().contains("feature/module"));
		let rejected=super::remove_file_without_reload(&root,"feature/package",Some(manifest),None).unwrap_err();
		assert_eq!(rejected.status(),409);
		assert_eq!(std::fs::read_to_string(root.join("feature/package.flowgraph.toml")).unwrap(),manifest);
		// Removing the module while retaining its declaration remains allowed.
		super::remove_file_without_reload(&root,"feature/module",None,None).unwrap();
		assert_eq!(super::removal_plan_snapshot(&root,"feature/package").unwrap()["can_delete"],true);
		super::remove_file_without_reload(&root,"feature/package",Some(manifest),None).unwrap();
		assert!(!root.join("feature/package.flowgraph.toml").exists());
		// Broken files remain removable as a recovery action.
		std::fs::write(root.join("broken.flowgraph.toml"),"[invalid").unwrap();
		assert_eq!(super::removal_plan_snapshot(&root,"broken").unwrap()["can_delete"],true);
		super::remove_file_without_reload(&root,"broken",Some("[invalid"),None).unwrap();
		std::fs::remove_dir_all(root).unwrap();
	}
}

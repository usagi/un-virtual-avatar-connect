//! Fragment copy/paste と ZIP import/export（Phase δ-7c / δ-7d）。

use actix_web::web::{self, Data, Json};
use actix_web::{get, post, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::flowgraph::fragment::paste::{paste_fragment, PasteError, PasteReport, PasteRequest};
use crate::flowgraph::fragment::zip_codec::{export_zip_with_companions, import_zip, ZipExportRequest, ZipExportError, ZipImportError, ZipImportOptions, ZipImportOutcome};
use crate::flowgraph::fragment::{copy_targets, serialize_fragment, CopyError, CopyRequest, Fragment};
use crate::SharedState;

use super::reload::reload_runtime_with_restart_status;
use super::util::{err_json, flowgraph_dir};

/// copy レスポンス。spec §9.4 に従い `fragment_toml` に生 TOML を詰める。
///
/// `fragment` フィールドは GUI が TOML を再パースしなくても済むように **構造化コピー**
/// を重ねて返す（TOML 側が真の持ち物、structured 側は優先度低い参考情報）。
#[derive(Debug, Serialize)]
pub struct CopyResponse {
	pub fragment_toml: String,
	pub fragment: Fragment,
	pub file_count: usize,
	pub dangling_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct SharePlanRequest {
	pub files: Vec<String>,
}

#[get("/flowgraph/fragment/companions")]
pub async fn get_companions(state: Data<SharedState>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(value) => value,
		Err(response) => return response,
	};
	let _guard = match super::util::lock_graph_writes(&root) {
		Ok(guard) => guard,
		Err(response) => return response,
	};
	match crate::flowgraph::fragment::zip_codec::list_companion_candidates(&root) {
		Ok(mut inventory) => {
			inventory.root_dir = super::graph_root_identity(&root);
			HttpResponse::Ok().json(inventory)
		},
		Err(error) => err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "companion_inventory_failed", error),
	}
}

/// Read saved graph dependencies without exporting or changing runtime state.
#[post("/flowgraph/fragment/share-plan")]
pub async fn post_share_plan(state: Data<SharedState>, req: Json<SharePlanRequest>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(value) => value,
		Err(response) => return response,
	};
	match share_plan_snapshot(&root, &req.files) {
        Ok(value) => HttpResponse::Ok().json(value),
        Err(response) => response,
    }
}

/// The same revision-bound read is used by HTTP and isolated UI verification.
pub fn share_plan_snapshot(root: &std::path::Path, files: &[String]) -> Result<serde_json::Value, HttpResponse> {
    let _guard = super::util::lock_graph_writes(root)?;
    let sources = graph_sources(root)?;
    let plan = crate::flowgraph::fragment::composition_share::plan_composition_share(root, files)
        .map_err(copy_error_to_response)?;
    check_graph_sources(root, Some(&sources))?;
    let mut value = serde_json::to_value(plan).expect("serializable share plan");
    value["root_dir"] = serde_json::json!(super::graph_root_identity(root));
    value["source_digests"] = serde_json::json!(sources);
    Ok(value)
}

/// paste レスポンス。δ-7b の [`PasteReport`] に reload サマリを添えたもの。
#[derive(Debug, Serialize)]
pub struct PasteResponse {
	pub restart_required: bool,
	pub report: PasteReport,
	pub reload_ok: bool,
	pub diagnostics_count: usize,
	pub node_count: usize,
}

/// 指定した範囲を fragment TOML に切り出す。spec §9.4 `POST /fragment/copy`。
#[post("/flowgraph/fragment/copy")]
pub async fn post_fragment_copy(state: Data<SharedState>, req: Json<CopyRequest>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	let fragment = match copy_targets(&root, &req) {
		Ok(f) => f,
		Err(e) => return copy_error_to_response(e),
	};
	let fragment_toml = match serialize_fragment(&fragment) {
		Ok(s) => s,
		Err(e) => {
			return err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "serialize_failed", e);
		}
	};
	let file_count = fragment.files.len();
	let dangling_count = fragment.danglings.len();
	HttpResponse::Ok().json(CopyResponse {
		fragment_toml,
		fragment,
		file_count,
		dangling_count,
	})
}

fn copy_error_to_response(e: CopyError) -> HttpResponse {
	use actix_web::http::StatusCode;
	match e {
		CopyError::RootMissing(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "flowgraph_dir_missing", e),
		CopyError::NoTargets => err_json(StatusCode::BAD_REQUEST, "no_targets", e),
		CopyError::FileNotFound(_) => err_json(StatusCode::NOT_FOUND, "file_not_found", e),
		CopyError::NodeNotFound { .. } => err_json(StatusCode::NOT_FOUND, "node_not_found", e),
		CopyError::InvalidFq(_) => err_json(StatusCode::BAD_REQUEST, "invalid_fq", e),
		CopyError::Io { .. } => err_json(StatusCode::INTERNAL_SERVER_ERROR, "io", e),
		CopyError::Parse(_) => err_json(StatusCode::UNPROCESSABLE_ENTITY, "toml_parse", e),
	}
}

/// fragment を対象ディレクトリ配下に書き戻す。spec §9.4 `POST /fragment/paste`。
///
/// 書き込み後は `reload_runtime` で runtime を再構築 + `FlowgraphReloaded` WS push。
#[post("/flowgraph/fragment/paste")]
pub async fn post_fragment_paste(state: Data<SharedState>, req: Json<PasteRequest>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	let _write_guard = match super::util::lock_graph_writes(&root) { Ok(guard) => guard, Err(error) => return error };
	let report = match paste_fragment(&root, &req) {
		Ok(r) => r,
		Err(e) => return paste_error_to_response(e),
	};
	// 書き込みが発生した場合のみ reload + push。write_files が空でも念のため reload する
	// （不整合な状態を避ける）。
	let (reload_ok, diagnostics, restart_required) = reload_runtime_with_restart_status(&state, &root).await;
	let node_count = {
		let fg = state.read().await.flowgraph.clone();
		let rt = fg.read().await;
		rt.as_ref().map(|r| r.node_meta.len()).unwrap_or(0)
	};
	HttpResponse::Ok().json(PasteResponse {
		restart_required,
		report,
		reload_ok,
		diagnostics_count: diagnostics.len(),
		node_count,
	})
}

fn paste_error_to_response(e: PasteError) -> HttpResponse {
	use actix_web::http::StatusCode;
	match e {
		PasteError::PartialWrite { source, report } => HttpResponse::InternalServerError().json(serde_json::json!({
			"error": "partial_write",
			"detail": format!("保存処理の途中で失敗しました。自動再試行せず、保存済みファイルとバックアップを確認してください: {source}"),
			"written_files": report.written_files,
			"backups": report.backups,
			"reload_attempted": false
		})),
		PasteError::RootMissing(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "flowgraph_dir_missing", e),
		PasteError::Parse(_) => err_json(StatusCode::UNPROCESSABLE_ENTITY, "fragment_parse", e),
		PasteError::InvalidTarget(_) => err_json(StatusCode::BAD_REQUEST, "invalid_target", e),
		PasteError::InvalidFragmentPath(_) => err_json(StatusCode::BAD_REQUEST, "invalid_fragment_path", e),
		PasteError::Io { .. } => err_json(StatusCode::INTERNAL_SERVER_ERROR, "io", e),
		PasteError::DestinationChanged(_) => err_json(StatusCode::CONFLICT, "destination_changed", e),
		PasteError::ExistingParse { .. } => err_json(StatusCode::CONFLICT, "existing_parse_failed", e),
	}
}

/// export 用 query. `GET` でバイナリを返すが、payload が CopyRequest なので POST + JSON body
/// で受ける（spec は GET を想定するが、GUI が targets を body で送れる方が現実的なため **POST** 採用）。
#[post("/flowgraph/export/zip")]
pub async fn post_export_zip(state: Data<SharedState>, req: Json<ZipExportRequest>) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};
	if let Err(response) = check_export_root(&root, req.expected_root.as_deref()) {
		return response;
	}
	let _guard = match super::util::lock_graph_writes(&root) {
		Ok(guard) => guard,
		Err(response) => return response,
	};
	if let Err(response) = check_graph_sources(&root, req.expected_graph_sources.as_ref()) { return response; }
	match export_zip_with_companions(&root, &req.copy, &req.companions) {
		Ok(bytes) => {
			if let Err(response) = check_graph_sources(&root, req.expected_graph_sources.as_ref()) { return response; }
			HttpResponse::Ok()
			.content_type("application/zip")
			.append_header((
				actix_web::http::header::CONTENT_DISPOSITION,
				"attachment; filename=\"fragment.flowgraph.zip\"",
			))
			.body(bytes)
		},
		Err(e) => export_zip_error_to_response(e),
	}
}

/// Checked export for disposable adapters; normal HTTP uses the same checks.
pub fn export_snapshot(root: &std::path::Path, req: &ZipExportRequest) -> Result<Vec<u8>, HttpResponse> {
    check_export_root(root, req.expected_root.as_deref())?;
    let _guard = super::util::lock_graph_writes(root)?;
    check_graph_sources(root, req.expected_graph_sources.as_ref())?;
    let bytes = export_zip_with_companions(root, &req.copy, &req.companions).map_err(export_zip_error_to_response)?;
    check_graph_sources(root, req.expected_graph_sources.as_ref())?;
    Ok(bytes)
}

fn graph_sources(root: &std::path::Path) -> Result<std::collections::BTreeMap<String, String>, HttpResponse> {
	let read_error = |error: String| err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "export_inventory_failed", error);
	let files = crate::flowgraph::loader::walk_flowgraph_dir(root).map_err(|e| read_error(e.to_string()))?;
	let mut sources = std::collections::BTreeMap::new();
	for file in files {
		let fq = crate::flowgraph::loader::fq_path_of_file(root, &file).map_err(&read_error)?;
		let content = std::fs::read_to_string(file).map_err(|e| read_error(e.to_string()))?;
		sources.insert(fq, crate::flowgraph::loader::file::package_source_digest(&content));
	}
	Ok(sources)
}

fn check_graph_sources(root: &std::path::Path, expected: Option<&std::collections::BTreeMap<String, String>>) -> Result<(), HttpResponse> {
	if let Some(expected) = expected {
		if &graph_sources(root)? != expected {
			return Err(err_json(actix_web::http::StatusCode::CONFLICT, "export_graphs_changed", "構成が確認時から変わりました。共有する内容を再確認してください"));
		}
	}
	Ok(())
}

fn check_export_root(root: &std::path::Path, expected: Option<&str>) -> Result<(), HttpResponse> {
	let Some(expected) = expected else { return Ok(()); };
	let actual = root.canonicalize().map_err(|error| err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "io", error))?;
	if actual.to_string_lossy() != expected && super::graph_root_identity(&actual) != expected {
		return Err(err_json(actix_web::http::StatusCode::CONFLICT, "export_root_changed", "保存先が変わりました。書出し対象を確認し直してください"));
	}
	Ok(())
}

#[cfg(test)]
mod export_root_tests {
	#[test]
	fn reviewed_inventory_rejects_edits_additions_and_removals() {
		let root = std::env::temp_dir().join(format!("unvac-export-review-{}-{}", std::process::id(), rand::random::<u64>()));
		std::fs::create_dir(&root).unwrap();
		let file = root.join("main.flowgraph.toml");
		std::fs::write(&file, "# first\n").unwrap();
		let reviewed = super::graph_sources(&root).unwrap();
		assert!(super::check_graph_sources(&root, Some(&reviewed)).is_ok());
		std::fs::write(&file, "# changed\n").unwrap();
		assert_eq!(super::check_graph_sources(&root, Some(&reviewed)).unwrap_err().status(), 409);
		std::fs::write(&file, "# first\n").unwrap();
		let added = root.join("dependency.flowgraph.toml");
		std::fs::write(&added, "# new package definition\n").unwrap();
		assert_eq!(super::check_graph_sources(&root, Some(&reviewed)).unwrap_err().status(), 409);
		std::fs::remove_file(&added).unwrap();
		std::fs::remove_file(&file).unwrap();
		assert_eq!(super::check_graph_sources(&root, Some(&reviewed)).unwrap_err().status(), 409);
		assert!(super::check_graph_sources(&root, None).is_ok());
		std::fs::remove_dir(root).unwrap();
	}

	#[test]
	fn checks_inventory_identity_without_resolving_client_supplied_paths() {
		let root = std::env::temp_dir().canonicalize().unwrap();
		assert!(super::check_export_root(&root, Some(&root.to_string_lossy())).is_ok());
		assert!(super::check_export_root(&root, Some(&super::super::graph_root_identity(&root))).is_ok());
		assert!(super::check_export_root(&root, None).is_ok());
		let response = super::check_export_root(&root, Some("a different root")).unwrap_err();
		assert_eq!(response.status(), actix_web::http::StatusCode::CONFLICT);
	}
}

fn export_zip_error_to_response(e: ZipExportError) -> HttpResponse {
	use actix_web::http::StatusCode;
	match e {
		ZipExportError::InvalidCompanion(_) => err_json(StatusCode::BAD_REQUEST, "invalid_companion", e),
		ZipExportError::Copy(c) => copy_error_to_response(c),
		ZipExportError::Serialize(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "serialize", e),
		ZipExportError::Zip(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "zip", e),
		ZipExportError::Io(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "io", e),
	}
}

/// `POST /flowgraph/import/zip`
///
/// Body: **raw ZIP バイナリ**（`Content-Type: application/zip`）。
/// Query string で `dry_run`（bool）と `target_prefix`（`imported` デフォルト）を受け取る。
///
/// `paste` オプション（衝突戦略・remap 等）は spec §9.4 では body 内で JSON を期待するが、
/// actix multipart 依存を避けるため **query で絞った簡易版**（`on_conflict_node=suffix|skip|overwrite`）のみサポート。
/// 詳細 remap を指定したい場合は、ZIP を copy API で取り出した fragment に対して paste API を直接叩く
/// （ZIP はそのバルク輸送用途）。
#[derive(Debug, Deserialize)]
pub struct ImportZipQuery {
	#[serde(default)]
	pub expected_root: Option<String>,
	#[serde(default)]
	pub dry_run: bool,
	#[serde(default)]
	pub target_prefix: Option<String>,
	#[serde(default)]
	pub on_conflict_node: Option<String>,
	#[serde(default)]
	pub on_conflict_file: Option<String>,
}

#[post("/flowgraph/import/zip")]
pub async fn post_import_zip(state: Data<SharedState>, query: web::Query<ImportZipQuery>, body: web::Bytes) -> impl Responder {
	let (root, _) = match flowgraph_dir(&state).await {
		Ok(v) => v,
		Err(r) => return r,
	};

	if query.expected_root.as_deref().is_some_and(|expected| expected != super::graph_root_identity(&root)) {
		return err_json(actix_web::http::StatusCode::CONFLICT, "import_root_changed", "取込先が変わりました。別の構成には追加していません。内容を確認し直してください。");
	}

	let mut opts = ZipImportOptions {
		dry_run: query.dry_run,
		target_prefix: query.target_prefix.clone().unwrap_or_else(|| "imported".into()),
		..Default::default()
	};
	if let Some(s) = &query.on_conflict_node {
		opts.paste.on_conflict_node = match s.as_str() {
			"suffix" => crate::flowgraph::fragment::paste::OnConflict::Suffix,
			"skip" => crate::flowgraph::fragment::paste::OnConflict::Skip,
			"overwrite" => crate::flowgraph::fragment::paste::OnConflict::Overwrite,
			other => {
				return err_json(
					actix_web::http::StatusCode::BAD_REQUEST,
					"invalid_on_conflict_node",
					format!("不明: '{other}'"),
				);
			}
		};
	}
	if let Some(s) = &query.on_conflict_file {
		opts.paste.on_conflict_file = match s.as_str() {
			"overwrite" => crate::flowgraph::fragment::paste::OnConflictFile::Overwrite,
			"skip" => crate::flowgraph::fragment::paste::OnConflictFile::Skip,
			"rename" => crate::flowgraph::fragment::paste::OnConflictFile::Rename,
			"merge" => crate::flowgraph::fragment::paste::OnConflictFile::Merge,
			other => {
				return err_json(
					actix_web::http::StatusCode::BAD_REQUEST,
					"invalid_on_conflict_file",
					format!("不明: '{other}'"),
				);
			}
		};
	}

	let _write_guard = if opts.dry_run { None } else {
		match super::util::lock_graph_writes(&root) { Ok(guard) => Some(guard), Err(error) => return error }
	};
	match import_zip(&root, &body, &opts) {
		Ok(ZipImportOutcome::Preview(p)) => HttpResponse::Ok().json(ZipImportOutcome::Preview(p)),
		Ok(ZipImportOutcome::Report(mut r)) => {
			// 本番書き込みが起きたので reload + WS push。
			let (ok, diags, restart_required) = reload_runtime_with_restart_status(&state, &root).await;
			r.reload_ok = Some(ok);
			r.restart_required = Some(restart_required);
			r.diagnostics_count = Some(diags.len());
			HttpResponse::Ok().json(ZipImportOutcome::Report(r))
		}
		Err(e) => import_zip_error_to_response(e),
	}
}

pub(super) fn zip_partial_response(error: &ZipImportError) -> Option<HttpResponse> {
    zip_partial_details(error).map(|body| HttpResponse::InternalServerError().json(body))
}
pub(super) fn zip_partial_details(error: &ZipImportError) -> Option<serde_json::Value> {
    let body = match error {
        ZipImportError::CompanionWrite { source, failed_path, written_files, file_mappings, written_companions, companion_mappings, backups } => serde_json::json!({
            "error":"partial_write", "detail":format!("素材の保存中に失敗しました。対象: {failed_path}: {source}"),
            "written_files":written_files, "written_companions":written_companions, "backups":backups,
            "file_mappings":file_mappings, "companion_mappings":companion_mappings,
            "reload_attempted":false
        }),
        ZipImportError::Paste(PasteError::PartialWrite { source, report }) => serde_json::json!({
            "error":"partial_write", "detail":format!("構成の保存中に失敗しました: {source}"),
            "written_files":report.written_files, "written_companions":[], "backups":report.backups,
            "reload_attempted":false
        }),
        _=>return None,
    };
    Some(body)
}

fn import_zip_error_to_response(e: ZipImportError) -> HttpResponse {
	use actix_web::http::StatusCode;
	if let Some(response)=zip_partial_response(&e){return response;}
	match e {
		ZipImportError::CompanionWrite { source, failed_path, written_files, written_companions, backups, .. } => HttpResponse::InternalServerError().json(serde_json::json!({
			"error": "partial_write", "detail": format!("素材の保存中に失敗しました。保存済みファイルを確認してください。対象: {failed_path}: {source}"),
			"written_files": written_files, "written_companions": written_companions, "backups": backups, "reload_attempted": false
		})),
		ZipImportError::CompanionConflict(_) => err_json(StatusCode::CONFLICT, "companion_conflict", e),
		ZipImportError::UnsafeEntry(_) => err_json(StatusCode::BAD_REQUEST, "unsafe_entry", e),
		ZipImportError::ManifestMissing => err_json(StatusCode::UNPROCESSABLE_ENTITY, "manifest_missing", e),
		ZipImportError::ManifestParse(_) => err_json(StatusCode::UNPROCESSABLE_ENTITY, "manifest_parse", e),
		ZipImportError::Zip(_) => err_json(StatusCode::UNPROCESSABLE_ENTITY, "zip_parse", e),
		ZipImportError::Io(_) => err_json(StatusCode::INTERNAL_SERVER_ERROR, "io", e),
		ZipImportError::Paste(p) => paste_error_to_response(p),
	}
}

#[cfg(test)]
mod partial_write_response_tests {
    use super::*;
    #[actix_web::test]
    async fn partial_write_response_keeps_completed_paths_without_reloading() {
        use crate::flowgraph::fragment::paste::{PasteReport, WrittenFile};
        let error = PasteError::PartialWrite {
            source: Box::new(PasteError::DestinationChanged("second.flowgraph.toml".into())),
            report: Box::new(PasteReport {
                written_files: vec![WrittenFile {content_digest:"test-digest".into(),original_fq:None,fq:"first".into(),path:"first.flowgraph.toml".into()}],
                backups:vec!["first.flowgraph.toml.bak".into()], imported_nodes:vec![],skipped_nodes:vec![],skipped_files:vec![],unresolved_danglings:vec![],
            }),
        };
        let response = paste_error_to_response(error);
        assert_eq!(response.status(), actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);
        let bytes = actix_web::body::to_bytes(response.into_body()).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"], "partial_write");
        assert_eq!(value["written_files"][0]["path"], "first.flowgraph.toml");
        assert_eq!(value["backups"][0], "first.flowgraph.toml.bak");
        assert_eq!(value["reload_attempted"], false);
    }
}

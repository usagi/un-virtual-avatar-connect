//! Phase δ-7b: Fragment paste 処理（spec §9.2「paste 時の処理」/ §9.4）。
//!
//! `Fragment` を受け取り、ターゲットディレクトリ配下のファイルに書き出す。
//! 処理順は以下の通り:
//!   1. `PasteTarget` からターゲットファイル群の **絶対 path 表** を組み立てる。
//!   2. 既存ファイルとの衝突を `on_conflict_file` に従って解消。
//!   3. 各ファイルで **ノード ID 衝突** を `on_conflict_node` に従って解消。
//!      ID rewrite はテーブルに溜めておき、エッジ側の `node_id:port` も rewrite する。
//!   4. エッジをマージ（重複エッジは skip）。
//!   5. dangling は `RemapTable` で再マッピング or drop。
//!   6. `.bak` を作ってから atomic 書き込み。
//!
//! 位置オフセットは GUI から `position_offset` で受け取り、各ノード position に加算する。

use super::{parse_fragment, Fragment, FragmentDangling, FragmentFile, SCRATCH_PATH};
use crate::flowgraph::loader::file::{parse_flowgraph_file, EdgeEntry, FlowgraphFile, FlowgraphGroupDef};
use crate::flowgraph::loader::reference::parse_port_ref;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// ノード ID 衝突の解消戦略。spec §9.4 の `on_conflict` と同値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OnConflict {
	/// 連番 suffix（`tts_jp` → `tts_jp_1` → `tts_jp_2` ...）で衝突を解消する（既定）。
	#[default]
	Suffix,
	/// 衝突したノードは paste に含めない。
	Skip,
	/// 既存を置き換える。
	Overwrite,
}

/// ファイル単位の衝突解消戦略。フォルダ paste 時、同名 `.flowgraph.toml` が既に存在するケース用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OnConflictFile {
	/// `.bak` に退避してから上書き（ノードは完全置換）。
	#[default]
	Overwrite,
	/// そのファイルに対する paste をスキップ。
	Skip,
	/// `foo.flowgraph.toml` → `foo_1.flowgraph.toml` → ... と連番で回避。
	Rename,
	/// **既存ファイルのノード列にマージする**。衝突ノードは `on_conflict_node` で解消。
	/// scope=nodes + PasteTarget::File で最頻の挙動（= デフォルト相当）。scope=file/folder では
	/// 他ファイルを汚染しないため明示選択されたときのみ使用する想定。
	Merge,
}

/// paste 先の指定。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PasteTarget {
	/// scope=nodes 用。fragment 内の scratch ファイルを、この既存 or 新規ファイルに **merge** する。
	/// `on_conflict_file` は無視して常に `Merge` 扱い。
	File { fq: String },
	/// scope=file/folder/mixed 用。fragment の `path` をこのフォルダ prefix の下に配置。
	/// 空文字列はルート（`flowgraph_dir` 直下）。
	Folder { path: String },
}

/// paste 動作オプション。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PasteOptions {
	#[serde(default)]
	pub on_conflict_node: OnConflict,
	#[serde(default)]
	pub on_conflict_file: OnConflictFile,
	/// ノード position に一律加算する `[dx, dy]`。省略時は無加算。
	#[serde(default)]
	pub position_offset: Option<[f64; 2]>,
	/// dangling 再マッピング表。key = fragment.danglings[].original、value は:
	///   - `Some("fragment 内の新ターゲット")` → そのノード/ポートに繋ぎ直す
	///   - `None` → 明示的に破棄
	///
	/// map に無い dangling は `unresolved_danglings` に残す（= 何もしない）。
	#[serde(default)]
	pub remap: HashMap<String, Option<String>>,
}

/// paste 要求。API body もこれに合わせる。
#[derive(Debug, Clone, Deserialize)]
pub struct PasteRequest {
	/// fragment TOML 文字列。クライアントが任意編集したものも受け入れる。
	pub fragment_toml: String,
	pub target: PasteTarget,
	#[serde(default)]
	pub options: PasteOptions,
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

/// paste 結果レポート。GUI は `imported_nodes` / `unresolved_danglings` を表示する。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasteReport {
	pub written_files: Vec<WrittenFile>,
	pub imported_nodes: Vec<ImportedNode>,
	pub skipped_nodes: Vec<SkippedNode>,
	pub skipped_files: Vec<String>,
	pub unresolved_danglings: Vec<FragmentDangling>,
	pub backups: Vec<String>,
}

/// 書き出したファイルの fq (root 相対、拡張子なし) と 実 path。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrittenFile {
	/// Original fragment file FQ for folder imports; absent for node merges.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub original_fq: Option<String>,
	/// BLAKE3 of the serialized bytes successfully published by this write.
	pub content_digest: String,
	pub fq: String,
	pub path: String,
}

/// 取り込んだノードの ID 変換結果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedNode {
	pub fq_file: String,
	pub original_id: String,
	pub new_id: String,
}

/// スキップされたノード（on_conflict_node = skip で衝突したケース）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedNode {
	pub fq_file: String,
	pub original_id: String,
	pub reason: String,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum PasteError {
	#[error("flowgraph root が存在しない: {0}")]
	RootMissing(PathBuf),
	#[error("fragment TOML パース失敗: {0}")]
	Parse(String),
	#[error("不正な target 指定: {0}")]
	InvalidTarget(String),
	#[error("不正な fragment.file.path: {0}")]
	InvalidFragmentPath(String),
	#[error("I/O エラー ({path}): {err}")]
	Io { path: PathBuf, err: std::io::Error },
	#[error("既存ファイル TOML パース失敗 ({path}): {err}")]
	ExistingParse { path: PathBuf, err: String },
	#[error("取込計画後に保存先が変更されました。上書きしていません: {0}")]
	DestinationChanged(PathBuf),
	#[error("保存処理の途中で失敗しました。保存済みファイルとバックアップを確認してください: {source}")]
	PartialWrite { source: Box<PasteError>, report: Box<PasteReport> },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Called before publication; an error prevents this file from being written.
pub(crate) trait WriteObserver {
    fn purpose_id(&self) -> Option<String> { None }
    fn graphs_planned(&self, _files: &[PlannedGraph], _report: &PasteReport) -> std::io::Result<()> { Ok(()) }
    fn before_write(&self, path: &Path, before: Option<&[u8]>, after: &[u8], source: Option<&str>) -> std::io::Result<()>;
    fn graphs_completed(&self, _report: &PasteReport) -> std::io::Result<()> { Ok(()) }
}

/// Exact post-remapping content, retained before the first folder publication.
#[derive(Debug, Serialize)]
pub(crate) struct PlannedGraph {
    pub original_fq: String,
    pub fq: String,
    pub content: String,
    pub before_digest: Option<String>,
}

/// fragment TOML + ターゲットを入力に、ディスクへ反映する。
///
/// 成功時でも `unresolved_danglings` が残るケースがある（GUI が remap を指定しなかった場合）。
/// `.bak` はファイル上書き時のみ作成する。
pub fn paste_fragment(root: &Path, req: &PasteRequest) -> Result<PasteReport, PasteError> {
    paste_fragment_observed(root, req, None)
}
pub(crate) fn paste_fragment_observed(root: &Path, req: &PasteRequest, observer: Option<&dyn WriteObserver>) -> Result<PasteReport, PasteError> {
	if !root.exists() {
		return Err(PasteError::RootMissing(root.to_path_buf()));
	}
	let fragment = parse_fragment(&req.fragment_toml).map_err(PasteError::Parse)?;

	let mut report = PasteReport {
		written_files: Vec::new(),
		imported_nodes: Vec::new(),
		skipped_nodes: Vec::new(),
		skipped_files: Vec::new(),
		unresolved_danglings: Vec::new(),
		backups: Vec::new(),
	};

	// (1) target の fq prefix / 1 ファイル fq を確定。
	let result = match &req.target {
		PasteTarget::File { fq } => {
			let fq = normalize_fq(fq)?;
			paste_into_single_file(root, &fq, &fragment, &req.options, &mut report, observer)
		}
		PasteTarget::Folder { path } => {
			let prefix = normalize_folder_prefix(path)?;
			paste_into_folder(root, &prefix, &fragment, &req.options, &mut report, observer)
		}
	};
	if let Err(error) = result {
		return Err(with_write_progress(error, report));
	}
	Ok(report)
}

fn with_write_progress(error: PasteError, report: PasteReport) -> PasteError {
	if report.written_files.is_empty() && report.backups.is_empty() { return error; }
	PasteError::PartialWrite { source: Box::new(error), report: Box::new(report) }
}

// ---------------------------------------------------------------------------
// Single-file merge path（scope=nodes 用）
// ---------------------------------------------------------------------------

fn paste_into_single_file(
	root: &Path,
	target_fq: &str,
	fragment: &Fragment,
	options: &PasteOptions,
	report: &mut PasteReport,
    observer: Option<&dyn WriteObserver>,
) -> Result<(), PasteError> {
	// fragment は複数ファイルを含む可能性がある（scope=file/folder/mixed）。
	// PasteTarget::File は scope=nodes が主用途で、scratch 1 本を前提にするが、
	// ここは「すべての fragment.files のノード/エッジを 1 ファイルに合流する」挙動に定義する。
	// （GUI が間違って scope=file 用の fragment を File target に投げても fail-open する。）
	let dest_path = fq_to_file_path(root, target_fq);
	let expected = read_destination(&dest_path)?;
	let existing = expected.as_deref().map(|bytes| parse_destination(&dest_path, bytes)).transpose()?;

	let is_new = existing.is_none();
	let mut merged_doc = existing.unwrap_or_default();
	if is_new {
		if let Some(first) = fragment.files.first() {
			merged_doc.graph.kind = first.graph.kind;
		}
	}
	let mut rewrite_map: HashMap<(String, String), String> = HashMap::new(); // (source_fq, src_id) → new_id
	let mut existing_ids: HashSet<String> = merged_doc
		.nodes
		.iter()
		.map(|n| n.id.clone())
		.chain(merged_doc.instances.iter().map(|i| i.id.clone()))
		.collect();

	for ff in &fragment.files {
		merge_instances_into(ff, target_fq, &mut merged_doc, &mut rewrite_map, &mut existing_ids, options, report);
		merge_nodes_into(ff, target_fq, &mut merged_doc, &mut rewrite_map, &mut existing_ids, options, report);
		merge_declarations(ff, &mut merged_doc, &rewrite_map)?;
	}
	merge_edges_into(fragment, target_fq, &mut merged_doc, &rewrite_map);
	for ff in &fragment.files {
		merge_groups_into(ff, &mut merged_doc, &rewrite_map);
	}
	apply_remaps_into(fragment, target_fq, &mut merged_doc, &rewrite_map, options, report);

	let content_digest = write_file_atomic_observed(&dest_path, &merged_doc, expected.as_deref(), report, observer, None)?;
	report.written_files.push(WrittenFile {
		original_fq: None,
		content_digest,
		fq: target_fq.to_string(),
		path: dest_path.display().to_string().replace('\\', "/"),
	});
	Ok(())
}

// ---------------------------------------------------------------------------
// Folder path（scope=file/folder/mixed 用）
// ---------------------------------------------------------------------------

fn paste_into_folder(
	root: &Path,
	prefix_fq: &str,
	fragment: &Fragment,
	options: &PasteOptions,
	report: &mut PasteReport,
    observer: Option<&dyn WriteObserver>,
) -> Result<(), PasteError> {
	// Reserve all destinations before writing: a renamed file must not steal
	// another incoming file's name, and references need the complete mapping.
	let sources = fragment
		.files
		.iter()
		.map(|f| sanitize_fragment_path(&f.path))
		.collect::<Result<Vec<_>, _>>()?;
	let destinations = sources
		.iter()
		.map(|fq| {
			if prefix_fq.is_empty() {
				fq.clone()
			} else {
				format!("{prefix_fq}/{fq}")
			}
		})
		.collect::<Vec<_>>();
	let mut reserved: HashSet<String> = destinations.iter().cloned().collect();
	let mut mapping = HashMap::new();
	let mut planned_replacements = HashMap::new();
	let mut skipped = HashSet::new();
	for (source, requested) in sources.iter().zip(&destinations) {
		let mut dest = requested.clone();
		let mut expected = read_destination(&fq_to_file_path(root, &dest))?;
		if expected.is_some() {
			match options.on_conflict_file {
				OnConflictFile::Skip => {
					skipped.insert(source.clone());
					report.skipped_files.push(dest.clone());
				}
				OnConflictFile::Rename => {
					expected = None;
					for n in 1..u64::MAX {
						let candidate = format!("{requested}_{n}");
						if !reserved.contains(&candidate) && !fq_to_file_path(root, &candidate).exists() {
							dest = candidate;
							break;
						}
					}
				}
				_ => {}
			}
		}
		reserved.insert(dest.clone());
		planned_replacements.insert(source.clone(), expected);
		mapping.insert(source.clone(), dest);
	}
	let known = sources.iter().cloned().collect::<HashSet<_>>();
	let mut planned = Vec::new();
	for (ff, source) in fragment.files.iter().zip(&sources) {
		if skipped.contains(source) {
			continue;
		}
		let final_fq = &mapping[source];
		let dest_path = fq_to_file_path(root, final_fq);
		let mut moved = ff.clone();
		let rewrite_path = |raw: &str| -> String {
			use crate::flowgraph::loader::reference::{resolve_fq_ref, PortRefStr, ResolveContext};
			let reference = PortRefStr {
				fq_path: Some(raw.to_owned()),
				node_id: "_".into(),
				port: "_".into(),
			};
			let ctx = ResolveContext {
				current_file_fq: source.clone(),
				known_file_fqs: known.clone(),
			};
			resolve_fq_ref(&reference, &ctx)
				.ok()
				.and_then(|fq| mapping.get(&fq).cloned())
				.unwrap_or_else(|| raw.to_owned())
		};
		let rewrite_port = |raw: &str| -> String {
			match parse_port_ref(raw) {
				Ok(port) => match port.fq_path {
					Some(path) => format!("{}::{}:{}", rewrite_path(&path), port.node_id, port.port),
					None => raw.to_owned(),
				},
				Err(_) => raw.to_owned(),
			}
		};
		for e in &mut moved.edges {
			e.from = rewrite_port(&e.from);
			e.to = rewrite_port(&e.to);
		}
		for instance in &mut moved.instances {
			instance.graph = rewrite_path(&instance.graph);
		}
		for port in &mut moved.graph.inputs {
			port.to = rewrite_port(&port.to);
		}
		for port in &mut moved.graph.outputs {
			port.from = rewrite_port(&port.from);
		}
		if let Some(package) = &mut moved.package {
			for export in &mut package.exports {
				*export = rewrite_path(export);
			}
		}
		// Apply each unresolved connection only to its owning file.
		let mut local = fragment.clone();
		local.danglings.retain(|d| d.in_file.as_ref().is_none_or(|fq| fq == source));
		for d in &mut local.danglings {
			if let Some(from) = &mut d.edge_from {
				*from = rewrite_port(from);
			}
			if let Some(to) = &mut d.edge_to {
				*to = rewrite_port(to);
			}
		}
		// Cross-file endpoints need ID allocations from every destination first.
		let incoming_edges = std::mem::take(&mut moved.edges);
		let expected = planned_replacements[source].clone();
		let doc = if let Some(bytes) = expected.as_deref().filter(|_| options.on_conflict_file == OnConflictFile::Merge) {
			prepare_fragment_file_merge(&moved, final_fq, parse_destination(&dest_path, bytes)?, &local, options, report)?
		} else {
			prepare_fragment_file_fresh(&moved, final_fq, &local, options, report)?
		};
		planned.push((final_fq.clone(), dest_path, doc, incoming_edges, expected, source.clone()));
	}
	let global_rewrite = report
		.imported_nodes
		.iter()
		.map(|node| {
			(
				(format!("{}.flowgraph.toml", node.fq_file), node.original_id.clone()),
				node.new_id.clone(),
			)
		})
		.collect::<HashMap<_, _>>();
	for (fq, _, doc, incoming_edges, _, _) in &mut planned {
		for edge in incoming_edges.iter() {
			let rewritten = rewrite_edge_endpoints(edge, &format!("{fq}.flowgraph.toml"), &global_rewrite);
			if !edge_already_present(&doc.edges, &rewritten) {
				doc.edges.push(rewritten);
			}
		}
		serialize_flowgraph_file(doc)?;
	}
	for (_, path, _, _, expected, _) in &planned {
		check_destination(path, expected.as_deref())?;
	}
    if let Some(observer) = observer {
        let files = planned.iter().map(|(fq, _, doc, _, expected, source)| {
            Ok(PlannedGraph {original_fq:source.clone(),fq:fq.clone(),
                content:serialize_flowgraph_file(doc)?,
                before_digest:expected.as_deref().map(|bytes|blake3::hash(bytes).to_hex().to_string())})
        }).collect::<Result<Vec<_>, PasteError>>()?;
        observer.graphs_planned(&files, report).map_err(|err|PasteError::Io {path:root.into(),err})?;
    }
	for (fq, path, doc, _, expected, source) in planned {
		// A file appearing after planning is a conflict, never new overwrite permission.
		let content_digest = write_file_atomic_observed(&path, &doc, expected.as_deref(), report, observer, Some(&source))?;
		report.written_files.push(WrittenFile {
			original_fq: Some(source),
			content_digest,
			fq,
			path: path.display().to_string().replace('\\', "/"),
		});
	}
	Ok(())
}

fn prepare_fragment_file_fresh(
	ff: &FragmentFile,
	final_fq: &str,
	fragment: &Fragment,
	options: &PasteOptions,
	report: &mut PasteReport,
) -> Result<FlowgraphFile, PasteError> {
	let mut doc = FlowgraphFile {
		activation_chain: Vec::new(),
		meta: ff.meta.clone(),
		graph: ff.graph.clone(),
		package: ff.package.clone(),
		nodes: Vec::new(),
		edges: Vec::new(),
		enums: ff.enums.clone(),
		types: ff.types.clone(),
		groups: Vec::new(),
		instances: ff.instances.clone(),
	};
	let mut rewrite: HashMap<(String, String), String> = HashMap::new();
	let mut existing_ids: HashSet<String> = HashSet::new();
	merge_nodes_into(ff, final_fq, &mut doc, &mut rewrite, &mut existing_ids, options, report);

	// edges: ff 内部のものだけ対象（scope 越えは paste_single_fragment_file 系では特殊化不要）。
	for e in &ff.edges {
		let rewritten = rewrite_edge_endpoints(e, &ff.path, &rewrite);
		if !edge_already_present(&doc.edges, &rewritten) {
			doc.edges.push(rewritten);
		}
	}
	merge_groups_into(ff, &mut doc, &rewrite);

	// dangling remap（この fragment 全体に対して適用、in_file が final_fq 相当のみ拾う）。
	apply_remaps_into(fragment, final_fq, &mut doc, &rewrite, options, report);

	Ok(doc)
}

fn prepare_fragment_file_merge(
	ff: &FragmentFile,
	final_fq: &str,
	existing: FlowgraphFile,
	fragment: &Fragment,
	options: &PasteOptions,
	report: &mut PasteReport,
) -> Result<FlowgraphFile, PasteError> {
	let mut doc = existing;
	let mut rewrite: HashMap<(String, String), String> = HashMap::new();
	let mut existing_ids: HashSet<String> = doc
		.nodes
		.iter()
		.map(|n| n.id.clone())
		.chain(doc.instances.iter().map(|i| i.id.clone()))
		.collect();
	merge_instances_into(ff, final_fq, &mut doc, &mut rewrite, &mut existing_ids, options, report);
	merge_nodes_into(ff, final_fq, &mut doc, &mut rewrite, &mut existing_ids, options, report);
	merge_declarations(ff, &mut doc, &rewrite)?;
	for e in &ff.edges {
		let rewritten = rewrite_edge_endpoints(e, &ff.path, &rewrite);
		if !edge_already_present(&doc.edges, &rewritten) {
			doc.edges.push(rewritten);
		}
	}
	merge_groups_into(ff, &mut doc, &rewrite);
	apply_remaps_into(fragment, final_fq, &mut doc, &rewrite, options, report);
	Ok(doc)
}

// ---------------------------------------------------------------------------
// Node merge
// ---------------------------------------------------------------------------

fn merge_declarations(ff: &FragmentFile, doc: &mut FlowgraphFile, rewrite: &HashMap<(String, String), String>) -> Result<(), PasteError> {
	fn equal<T: Serialize>(a: &T, b: &T) -> Result<bool, PasteError> {
		Ok(serde_json::to_value(a).map_err(|e| PasteError::Parse(e.to_string()))?
			== serde_json::to_value(b).map_err(|e| PasteError::Parse(e.to_string()))?)
	}
	macro_rules! merge_named {
		($target:expr_2021, $incoming:expr_2021, $key:ident, $label:expr_2021) => {
			for item in $incoming {
				if let Some(existing) = $target.iter().find(|existing| existing.$key == item.$key) {
					if !equal(existing, item)? {
						return Err(PasteError::Parse(format!(
							"{} '{}' の定義が競合する。別ファイルへの追加または名前の変更が必要。",
							$label, item.$key
						)));
					}
				} else {
					$target.push(item.clone());
				}
			}
		};
	}
	merge_named!(doc.enums, &ff.enums, id, "enum");
	merge_named!(doc.types, &ff.types, id, "type");
	if let Some(package) = &ff.package {
		if let Some(existing) = &doc.package {
			if !equal(existing, package)? {
				return Err(PasteError::Parse("package定義が競合する。別ファイルへの追加が必要。".into()));
			}
		} else {
			doc.package = Some(package.clone());
		}
	}
	let mut graph = ff.graph.clone();
	// Only this source file's local IDs may rewrite its public interface.
	let local = rewrite
		.iter()
		.filter(|((file, _), _)| file == &ff.path)
		.map(|(k, v)| (k.clone(), v.clone()))
		.collect();
	for port in &mut graph.inputs {
		port.to = rewrite_single_endpoint(&port.to, &ff.path, &local);
	}
	for port in &mut graph.outputs {
		port.from = rewrite_single_endpoint(&port.from, &ff.path, &local);
	}
	if graph.kind != crate::flowgraph::loader::file::GraphKind::Entry || !graph.inputs.is_empty() || !graph.outputs.is_empty() {
		if doc.graph.kind != graph.kind {
			return Err(PasteError::Parse(
				"entryとmoduleは同じファイルへ統合できない。別ファイルへの追加が必要。".into(),
			));
		}
		merge_named!(doc.graph.inputs, &graph.inputs, name, "graph input");
		merge_named!(doc.graph.outputs, &graph.outputs, name, "graph output");
	}
	Ok(())
}

fn merge_instances_into(
	ff: &FragmentFile,
	target_fq: &str,
	doc: &mut FlowgraphFile,
	rewrite: &mut HashMap<(String, String), String>,
	existing_ids: &mut HashSet<String>,
	options: &PasteOptions,
	report: &mut PasteReport,
) {
	for instance in &ff.instances {
		let id = if existing_ids.contains(&instance.id) {
			match options.on_conflict_node {
				OnConflict::Suffix => allocate_suffixed_id(&instance.id, existing_ids),
				OnConflict::Skip => {
					report.skipped_nodes.push(SkippedNode {
						fq_file: target_fq.into(),
						original_id: instance.id.clone(),
						reason: "id-conflict".into(),
					});
					continue;
				}
				OnConflict::Overwrite => {
					doc.nodes.retain(|node| node.id != instance.id);
					doc.instances.retain(|old| old.id != instance.id);
					instance.id.clone()
				}
			}
		} else {
			instance.id.clone()
		};
		existing_ids.insert(id.clone());
		let mut copied = instance.clone();
		copied.id = id.clone();
		copied.position = apply_position_offset(copied.position, options.position_offset);
		rewrite.insert((ff.path.clone(), instance.id.clone()), id.clone());
		doc.instances.push(copied);
		report.imported_nodes.push(ImportedNode {
			fq_file: target_fq.into(),
			original_id: instance.id.clone(),
			new_id: id,
		});
	}
}

fn merge_nodes_into(
	ff: &FragmentFile,
	target_fq: &str,
	doc: &mut FlowgraphFile,
	rewrite: &mut HashMap<(String, String), String>,
	existing_ids: &mut HashSet<String>,
	options: &PasteOptions,
	report: &mut PasteReport,
) {
	for node in &ff.nodes {
		let new_id = if existing_ids.contains(&node.id) {
			match options.on_conflict_node {
				OnConflict::Suffix => {
					let nid = allocate_suffixed_id(&node.id, existing_ids);
					existing_ids.insert(nid.clone());
					nid
				}
				OnConflict::Skip => {
					report.skipped_nodes.push(SkippedNode {
						fq_file: target_fq.to_string(),
						original_id: node.id.clone(),
						reason: "id-conflict".to_string(),
					});
					continue;
				}
				OnConflict::Overwrite => {
					// 既存の同 id ノードを除去してから新規 push。
					doc.nodes.retain(|n| n.id != node.id);
					doc.instances.retain(|i| i.id != node.id);
					existing_ids.insert(node.id.clone());
					node.id.clone()
				}
			}
		} else {
			existing_ids.insert(node.id.clone());
			node.id.clone()
		};

		let mut copied = node.clone();
		copied.id = new_id.clone();
		copied.position = apply_position_offset(copied.position, options.position_offset);
		rewrite.insert((ff.path.clone(), node.id.clone()), new_id.clone());
		doc.nodes.push(copied);
		report.imported_nodes.push(ImportedNode {
			fq_file: target_fq.to_string(),
			original_id: node.id.clone(),
			new_id,
		});
	}
}

fn apply_position_offset(p: Option<[f64; 2]>, offset: Option<[f64; 2]>) -> Option<[f64; 2]> {
	match (p, offset) {
		(Some([x, y]), Some([dx, dy])) => Some([x + dx, y + dy]),
		(Some(v), None) => Some(v),
		(None, Some(v)) => Some(v),
		(None, None) => None,
	}
}

fn allocate_suffixed_id(base: &str, existing: &HashSet<String>) -> String {
	for n in 1..u64::MAX {
		let candidate = format!("{base}_{n}");
		if !existing.contains(&candidate) {
			return candidate;
		}
	}
	unreachable!("u64 空間を使い切ることはない")
}

// ---------------------------------------------------------------------------
// Group merge
// ---------------------------------------------------------------------------

fn merge_groups_into(ff: &FragmentFile, doc: &mut FlowgraphFile, rewrite: &HashMap<(String, String), String>) {
	if ff.groups.is_empty() {
		return;
	}
	let existing_nodes: HashSet<String> = doc.nodes.iter().map(|n| n.id.clone()).collect();
	let mut existing_group_ids: HashSet<String> = doc.groups.iter().map(|g| g.id.clone()).collect();
	for group in &ff.groups {
		let node_ids = group
			.node_ids
			.iter()
			.map(|node_id| rewrite_node_id(node_id, &ff.path, rewrite))
			.filter(|node_id| existing_nodes.contains(node_id))
			.collect::<Vec<_>>();
		if node_ids.is_empty() {
			continue;
		}
		let id = if existing_group_ids.contains(&group.id) {
			allocate_suffixed_group_id(&group.id, &existing_group_ids)
		} else {
			group.id.clone()
		};
		existing_group_ids.insert(id.clone());
		doc.groups.push(FlowgraphGroupDef {
			id,
			label: group.label.clone(),
			node_ids,
			color: group.color.clone(),
		});
	}
}

fn rewrite_node_id(node_id: &str, source_path: &str, rewrite: &HashMap<(String, String), String>) -> String {
	rewrite
		.get(&(source_path.to_owned(), node_id.to_owned()))
		.cloned()
		.unwrap_or_else(|| node_id.to_owned())
}

fn allocate_suffixed_group_id(base: &str, existing: &HashSet<String>) -> String {
	for n in 1..u64::MAX {
		let candidate = format!("{base}_{n}");
		if !existing.contains(&candidate) {
			return candidate;
		}
	}
	unreachable!("u64 空間を使い切ることはない")
}

// ---------------------------------------------------------------------------
// Edge merge
// ---------------------------------------------------------------------------

fn merge_edges_into(fragment: &Fragment, target_fq: &str, doc: &mut FlowgraphFile, rewrite: &HashMap<(String, String), String>) {
	for ff in &fragment.files {
		for e in &ff.edges {
			// 元 fragment では fragment 内 edge だけがここに来る（dangling は別）。
			// rewrite 辞書で node_id を書き換えつつ doc に追加。
			let rewritten = EdgeEntry {
				from: rewrite_merged_endpoint(&e.from, &ff.path, rewrite),
				to: rewrite_merged_endpoint(&e.to, &ff.path, rewrite),
			};
			if !edge_already_present(&doc.edges, &rewritten) {
				doc.edges.push(rewritten);
			}
		}
	}
	// rewrite 未使用変数警告を抑制するため明示的に参照。
	let _ = rewrite;
	let _ = target_fq;
}

fn rewrite_edge_endpoints(e: &EdgeEntry, source_path: &str, rewrite: &HashMap<(String, String), String>) -> EdgeEntry {
	EdgeEntry {
		from: rewrite_single_endpoint(&e.from, source_path, rewrite),
		to: rewrite_single_endpoint(&e.to, source_path, rewrite),
	}
}

/// Resolve by source file and ID; identical IDs in other files are unrelated.
fn rewritten_endpoint(s: &str, source_path: &str, rewrite: &HashMap<(String, String), String>, flatten: bool) -> String {
	use crate::flowgraph::loader::reference::{resolve_fq_ref, ResolveContext};
	let Ok(port) = parse_port_ref(s) else {
		return s.to_owned();
	};
	let ctx = ResolveContext {
		current_file_fq: source_path.trim_end_matches(".flowgraph.toml").to_owned(),
		known_file_fqs: rewrite
			.keys()
			.map(|(file, _)| file.trim_end_matches(".flowgraph.toml").to_owned())
			.collect(),
	};
	let Ok(fq) = resolve_fq_ref(&port, &ctx) else {
		return s.to_owned();
	};
	let Some(id) = rewrite.get(&(format!("{fq}.flowgraph.toml"), port.node_id.clone())) else {
		return s.to_owned();
	};
	if !flatten {
		if let Some(path) = port.fq_path {
			return format!("{path}::{id}:{}", port.port);
		}
	}
	format!("{id}:{}", port.port)
}
fn rewrite_single_endpoint(s: &str, source_path: &str, rewrite: &HashMap<(String, String), String>) -> String {
	rewritten_endpoint(s, source_path, rewrite, false)
}
fn rewrite_merged_endpoint(s: &str, source_path: &str, rewrite: &HashMap<(String, String), String>) -> String {
	rewritten_endpoint(s, source_path, rewrite, true)
}

fn edge_already_present(edges: &[EdgeEntry], e: &EdgeEntry) -> bool {
	edges.iter().any(|ex| ex.from == e.from && ex.to == e.to)
}

// ---------------------------------------------------------------------------
// Remap table application
// ---------------------------------------------------------------------------

fn apply_remaps_into(
	fragment: &Fragment,
	target_fq: &str,
	doc: &mut FlowgraphFile,
	rewrite: &HashMap<(String, String), String>,
	options: &PasteOptions,
	report: &mut PasteReport,
) {
	for d in &fragment.danglings {
		// remap table に lookup。
		match options.remap.get(&d.original) {
			Some(None) => {
				// 明示的 drop。
				continue;
			}
			Some(Some(new_ref)) => {
				// 新しい参照文字列でエッジを再構築。
				// edge_from/edge_to と external_side が必要（schema v1 拡張フィールド）。
				let (Some(ef), Some(et), Some(es)) = (&d.edge_from, &d.edge_to, &d.external_side) else {
					// 情報不足。unresolved に残す。
					report.unresolved_danglings.push(d.clone());
					continue;
				};
				let (from, to) = match es.as_str() {
					"from" => (new_ref.clone(), et.clone()),
					"to" => (ef.clone(), new_ref.clone()),
					_ => {
						report.unresolved_danglings.push(d.clone());
						continue;
					}
				};
				let rewritten = rewrite_edge_endpoints(
					&EdgeEntry { from, to },
					&format!("{}.flowgraph.toml", d.in_file.as_deref().unwrap_or(target_fq)),
					rewrite,
				);
				if !edge_already_present(&doc.edges, &rewritten) {
					doc.edges.push(rewritten);
				}
			}
			None => {
				// 未指定 → unresolved に残す。
				report.unresolved_danglings.push(d.clone());
			}
		}
	}
}

// ---------------------------------------------------------------------------
// File helpers
// ---------------------------------------------------------------------------

fn fq_to_file_path(root: &Path, fq: &str) -> PathBuf {
	let mut p = root.to_path_buf();
	for seg in fq.split('/') {
		p.push(seg);
	}
	// 拡張子を付与。
	let mut s = p.as_os_str().to_os_string();
	s.push(".flowgraph.toml");
	PathBuf::from(s)
}

fn read_destination(path: &Path) -> Result<Option<Vec<u8>>, PasteError> {
	match std::fs::read(path) {
		Ok(bytes) => Ok(Some(bytes)),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(err) => Err(PasteError::Io { path: path.into(), err }),
	}
}
fn parse_destination(path: &Path, bytes: &[u8]) -> Result<FlowgraphFile, PasteError> {
	let text = std::str::from_utf8(bytes).map_err(|error| PasteError::ExistingParse { path: path.into(), err: error.to_string() })?;
	parse_flowgraph_file(text, Some(path)).map_err(|error| PasteError::ExistingParse { path: path.into(), err: error.to_string() })
}
#[cfg(test)]
fn load_existing_file(path: &Path) -> Result<Option<FlowgraphFile>, PasteError> {
	read_destination(path)?.as_deref().map(|bytes| parse_destination(path, bytes)).transpose()
}
fn check_destination(path: &Path, expected: Option<&[u8]>) -> Result<(), PasteError> {
	if read_destination(path)?.as_deref() != expected {
		return Err(PasteError::DestinationChanged(path.into()));
	}
	Ok(())
}

#[cfg(test)]
fn write_file_atomic(path: &Path, doc: &FlowgraphFile, expected: Option<&[u8]>, report: &mut PasteReport) -> Result<String, PasteError> {
    write_file_atomic_observed(path, doc, expected, report, None, None)
}
fn write_file_atomic_observed(path: &Path, doc: &FlowgraphFile, expected: Option<&[u8]>, report: &mut PasteReport, observer: Option<&dyn WriteObserver>, source: Option<&str>) -> Result<String, PasteError> {
    use std::io::Write;
    let text = serialize_flowgraph_file(doc)?;
    let digest = blake3::hash(text.as_bytes()).to_hex().to_string();
    if let Some(observer) = observer {
        observer.before_write(path, expected, text.as_bytes(), source).map_err(|err| PasteError::Io {path:path.into(),err})?;
    }
    write_file_atomic_with(path, expected, report, |file| file.write_all(text.as_bytes()))?;
    Ok(digest)
}

fn write_file_atomic_with(
    path: &Path, expected: Option<&[u8]>, report: &mut PasteReport,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> Result<(), PasteError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or_else(|| PasteError::InvalidTarget(path.display().to_string()))?;
    std::fs::create_dir_all(parent).map_err(|err| PasteError::Io { path: parent.into(), err })?;
    let temporary = parent.join(format!(".vac-import-{}-{}-{}.tmp", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    let io_error = |path: &Path, err| PasteError::Io { path: path.into(), err };
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|err| io_error(&temporary, err))?;
    struct Temporary(std::path::PathBuf);
    impl Drop for Temporary { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
    let _cleanup = Temporary(temporary.clone());
    let staged = write(&mut file).and_then(|_| file.sync_all());
    // Close before propagating an error: cleanup must not depend on whether the
    // filesystem permits removing a file while its writer handle remains open.
    drop(file);
    staged.map_err(|err| io_error(&temporary, err))?;
    check_destination(path, expected)?;
    if expected.is_some() {
        let bak = path.with_extension("toml.bak");
        std::fs::copy(path, &bak).map_err(|err| io_error(&bak, err))?;
        report.backups.push(bak.display().to_string().replace('\\', "/"));
    }
    if expected.is_some() {
        std::fs::rename(&temporary, path).map_err(|err| io_error(path, err))?;
    } else {
        // Publish without truncating a destination created since the caller checked.
        std::fs::hard_link(&temporary, path).map_err(|err| io_error(path, err))?;
    }
    Ok(())
}

/// `FlowgraphFile` を TOML 文字列化。spec §7.4 の format-preserving は対象外（本処理は paste/import で
/// 新規 or 完全上書きされるケース専用）。
fn serialize_flowgraph_file(doc: &FlowgraphFile) -> Result<String, PasteError> {
	toml::to_string_pretty(doc).map_err(|e| PasteError::Parse(format!("serialize: {e}")))
}

fn normalize_fq(s: &str) -> Result<String, PasteError> {
	let s = s.trim();
	if s.is_empty() {
		return Err(PasteError::InvalidTarget("(空)".into()));
	}
	if s.starts_with('/') || s.starts_with('\\') {
		return Err(PasteError::InvalidTarget(format!("絶対パス不可: '{s}'")));
	}
	for seg in s.split('/') {
		if seg == ".." || seg == "." || seg.is_empty() {
			return Err(PasteError::InvalidTarget(format!("'..' / '.' / 空セグ不可: '{s}'")));
		}
	}
	Ok(s.replace('\\', "/"))
}

fn normalize_folder_prefix(s: &str) -> Result<String, PasteError> {
	let s = s.trim().trim_end_matches('/');
	if s.is_empty() {
		return Ok(String::new());
	}
	normalize_fq(s)
}

/// fragment 内の相対 path を検証し、拡張子を取り除いた fq 部分を返す。
/// 例: `"chat/main.flowgraph.toml"` → `"chat/main"`。SCRATCH_PATH のときは `"__scratch__"`。
fn sanitize_fragment_path(path: &str) -> Result<String, PasteError> {
	let trimmed = path.trim().replace('\\', "/");
	if trimmed.starts_with('/') {
		return Err(PasteError::InvalidFragmentPath(format!("絶対パス不可: '{trimmed}'")));
	}
	for seg in trimmed.split('/') {
		if seg == ".." || seg == "." || seg.is_empty() {
			return Err(PasteError::InvalidFragmentPath(format!("'..' / '.' / 空セグ不可: '{trimmed}'")));
		}
	}
	let fq = if trimmed == SCRATCH_PATH {
		"__scratch__".to_string()
	} else {
		trimmed
			.strip_suffix(".flowgraph.toml")
			.ok_or_else(|| PasteError::InvalidFragmentPath(format!("'.flowgraph.toml' で終わっていない: '{trimmed}'")))?
			.to_string()
	};
	Ok(fq)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::fragment::{serialize_fragment, Fragment, FragmentHeader, FragmentScope, FRAGMENT_SCHEMA_V1};
	use crate::flowgraph::loader::file::NodeEntry;
	use std::fs;
	use std::io::Write;
	use std::sync::atomic::{AtomicU64, Ordering};
	use std::time::{SystemTime, UNIX_EPOCH};

	struct TempDir(PathBuf);
	impl TempDir {
		fn path(&self) -> &Path {
			&self.0
		}
	}
	impl Drop for TempDir {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.0);
		}
	}
	fn mk_dir() -> TempDir {
		static SEQ: AtomicU64 = AtomicU64::new(0);
		let ns = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		let seq = SEQ.fetch_add(1, Ordering::Relaxed);
		let dir = std::env::temp_dir().join(format!("vac-fg-paste-{}-{ns}-{seq}", std::process::id()));
		fs::create_dir_all(&dir).unwrap();
		TempDir(dir)
	}
	fn write_file(p: &Path, s: &str) {
		if let Some(parent) = p.parent() {
			fs::create_dir_all(parent).unwrap();
		}
		let mut f = fs::File::create(p).unwrap();
		f.write_all(s.as_bytes()).unwrap();
	}

	fn make_scratch_fragment(node_id: &str, position: Option<[f64; 2]>) -> String {
		let mut t = toml::Table::new();
		t.insert("value".into(), toml::Value::String("hi".into()));
		let frag = Fragment {
			header: FragmentHeader {
				schema: FRAGMENT_SCHEMA_V1.into(),
				exported_at: "2026-04-17T00:00:00Z".into(),
				origin: None,
				scope: FragmentScope::Nodes,
			},
			files: vec![FragmentFile {
				path: SCRATCH_PATH.into(),
				meta: None,
				nodes: vec![NodeEntry {
					id: node_id.into(),
					feature: "flowgraph.literal.string".into(),
					position,
					properties: t,
				}],
				edges: vec![],
				groups: vec![],
				..Default::default()
			}],
			danglings: vec![],
		};
		serialize_fragment(&frag).unwrap()
	}

	#[test]
	fn folder_merge_cross_file_edges_follow_imported_node_ids() {
		let dir = mk_dir();
		write_file(
			&dir.path().join("b.flowgraph.toml"),
			"[[nodes]]\nid = 'sink'\nfeature = 'flowgraph.util.log'\n",
		);
		let mut fragment = parse_fragment(&make_scratch_fragment("value", None)).unwrap();
		fragment.files[0].path = "a.flowgraph.toml".into();
		fragment.files[0].edges.push(EdgeEntry {
			from: "value:value".into(),
			to: "b::sink:value".into(),
		});
		let mut second = fragment.files[0].clone();
		second.path = "b.flowgraph.toml".into();
		second.nodes[0].id = "sink".into();
		second.nodes[0].feature = "flowgraph.util.log".into();
		second.nodes[0].properties.clear();
		second.edges.clear();
		fragment.files.push(second);
		paste_fragment(
			dir.path(),
			&PasteRequest {
				fragment_toml: serialize_fragment(&fragment).unwrap(),
				target: PasteTarget::Folder { path: String::new() },
				options: PasteOptions {
					on_conflict_file: OnConflictFile::Merge,
					..Default::default()
				},
			},
		)
		.unwrap();
		let first = load_existing_file(&dir.path().join("a.flowgraph.toml")).unwrap().unwrap();
		let second = load_existing_file(&dir.path().join("b.flowgraph.toml")).unwrap().unwrap();
		assert_eq!(first.edges[0].to, "b::sink_1:value");
		assert_eq!(
			second.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
			vec!["sink", "sink_1"]
		);
		crate::flowgraph::loader::dir::load_flowgraph_dir(dir.path()).expect("merged graph loads");
	}

	#[test]
	fn late_folder_merge_conflict_leaves_all_destinations_unchanged() {
		let dir = mk_dir();
		let first_path = dir.path().join("a.flowgraph.toml");
		let last_path = dir.path().join("z.flowgraph.toml");
		let original_first = "[[nodes]]\nid = 'original'\nfeature = 'flowgraph.util.log'\n";
		let original_last = "[[enums]]\nid = 'Mood'\nvariants = ['calm']\n";
		write_file(&first_path, original_first);
		write_file(&last_path, original_last);
		let mut fragment = parse_fragment(&make_scratch_fragment("added", None)).unwrap();
		fragment.files[0].path = "a.flowgraph.toml".into();
		let mut middle = fragment.files[0].clone();
		middle.path = "new.flowgraph.toml".into();
		fragment.files.push(middle);
		let mut last = fragment.files[0].clone();
		last.path = "z.flowgraph.toml".into();
		last.enums.push(crate::flowgraph::loader::file::FlowgraphEnumDef {
			id: "Mood".into(),
			variants: vec!["angry".into()],
			..Default::default()
		});
		fragment.files.push(last);
		let result = paste_fragment(
			dir.path(),
			&PasteRequest {
				fragment_toml: serialize_fragment(&fragment).unwrap(),
				target: PasteTarget::Folder { path: String::new() },
				options: PasteOptions {
					on_conflict_file: OnConflictFile::Merge,
					..Default::default()
				},
			},
		);
		assert!(result.is_err());
		assert_eq!(fs::read_to_string(&first_path).unwrap(), original_first);
		assert_eq!(fs::read_to_string(&last_path).unwrap(), original_last);
		assert!(!dir.path().join("new.flowgraph.toml").exists());
		assert!(!first_path.with_extension("toml.bak").exists());
	}

	#[test]
	fn multi_file_merge_scopes_duplicate_ids_and_cross_file_edges() {
		let dir = mk_dir();
		let mut fragment = parse_fragment(&make_scratch_fragment("value", None)).unwrap();
		let first = &mut fragment.files[0];
		first.path = "a.flowgraph.toml".into();
		let mut sink = first.nodes[0].clone();
		sink.id = "sink".into();
		sink.feature = "flowgraph.util.log".into();
		sink.properties.clear();
		first.nodes.push(sink);
		first.edges.push(EdgeEntry {
			from: "value:value".into(),
			to: "sink:value".into(),
		});
		first.groups.push(FlowgraphGroupDef {
			id: "group".into(),
			node_ids: vec!["value".into(), "sink".into()],
			..Default::default()
		});
		let mut second = first.clone();
		second.path = "b.flowgraph.toml".into();
		second.edges.push(EdgeEntry {
			from: "a::value:value".into(),
			to: "sink:value".into(),
		});
		fragment.files.push(second);
		paste_fragment(
			dir.path(),
			&PasteRequest {
				fragment_toml: serialize_fragment(&fragment).unwrap(),
				target: PasteTarget::File { fq: "combined".into() },
				options: Default::default(),
			},
		)
		.unwrap();
		let doc = load_existing_file(&dir.path().join("combined.flowgraph.toml")).unwrap().unwrap();
		assert!(doc.edges.iter().any(|e| e.from == "value:value" && e.to == "sink:value"));
		assert!(doc.edges.iter().any(|e| e.from == "value_1:value" && e.to == "sink_1:value"));
		assert!(doc.edges.iter().any(|e| e.from == "value:value" && e.to == "sink_1:value"));
		assert_eq!(doc.groups[0].node_ids, vec!["value", "sink"]);
		assert_eq!(doc.groups[1].node_ids, vec!["value_1", "sink_1"]);
	}

	#[test]
	fn merge_preserves_instances_and_rewrites_their_connections() {
		let dir = mk_dir();
		let path = dir.path().join("main.flowgraph.toml");
		write_file(&path, "[[nodes]]\nid = 'child'\nfeature = 'flowgraph.util.log'\n");
		let text = r#"
[fragment]
schema = "vac-flowgraph-fragment/v2"
exported_at = "2026-10-16T00:00:00Z"
scope = "file"
[[fragment.files]]
path = "source.flowgraph.toml"
[[fragment.files.nodes]]
id = "input"
feature = "flowgraph.ingress.web_input"
[[fragment.files.instances]]
id = "child"
graph = "module"
[[fragment.files.edges]]
from = "input:exec_out"
to = "child:exec_in"
[[fragment.files.enums]]
id = "Mood"
variants = ["happy", "calm"]
"#;
		paste_fragment(
			dir.path(),
			&PasteRequest {
				fragment_toml: text.into(),
				target: PasteTarget::File { fq: "main".into() },
				options: Default::default(),
			},
		)
		.unwrap();
		let doc = load_existing_file(&path).unwrap().unwrap();
		assert!(doc.nodes.iter().any(|n| n.id == "child"));
		assert_eq!(doc.instances[0].id, "child_1");
		assert_eq!(doc.edges[0].to, "child_1:exec_in");
		assert_eq!(doc.enums[0].variants, vec!["happy", "calm"]);
	}

	#[test]
	fn module_paste_into_new_file_retains_public_interface() {
		let dir = mk_dir();
		let mut fragment = parse_fragment(&make_scratch_fragment("value", None)).unwrap();
		fragment.files[0].graph.kind = crate::flowgraph::loader::file::GraphKind::Module;
		fragment.files[0]
			.graph
			.outputs
			.push(crate::flowgraph::loader::file::GraphOutputPort {
				name: "result".into(),
				ty: "string".into(),
				from: "value:value".into(),
			});
		paste_fragment(
			dir.path(),
			&PasteRequest {
				fragment_toml: serialize_fragment(&fragment).unwrap(),
				target: PasteTarget::File { fq: "module".into() },
				options: Default::default(),
			},
		)
		.unwrap();
		let doc = load_existing_file(&dir.path().join("module.flowgraph.toml")).unwrap().unwrap();
		assert_eq!(doc.graph.kind, crate::flowgraph::loader::file::GraphKind::Module);
		assert_eq!(doc.graph.outputs[0].from, "value:value");
	}

	#[test]
	fn conflicting_type_merge_does_not_modify_destination() {
		let dir = mk_dir();
		let path = dir.path().join("main.flowgraph.toml");
		let original = "[[enums]]\nid = 'Mood'\nvariants = ['calm']\n";
		write_file(&path, original);
		let mut fragment = parse_fragment(&make_scratch_fragment("added", None)).unwrap();
		fragment.files[0].enums.push(crate::flowgraph::loader::file::FlowgraphEnumDef {
			id: "Mood".into(),
			variants: vec!["angry".into()],
			..Default::default()
		});
		let result = paste_fragment(
			dir.path(),
			&PasteRequest {
				fragment_toml: serialize_fragment(&fragment).unwrap(),
				target: PasteTarget::File { fq: "main".into() },
				options: Default::default(),
			},
		);
		assert!(result.is_err());
		assert_eq!(fs::read_to_string(path).unwrap(), original);
	}

	#[test]
	fn paste_nodes_into_new_file() {
		let td = mk_dir();
		let toml = make_scratch_fragment("lit", Some([1.0, 2.0]));
		let req = PasteRequest {
			fragment_toml: toml,
			target: PasteTarget::File { fq: "mynew".into() },
			options: PasteOptions::default(),
		};
		let report = paste_fragment(td.path(), &req).unwrap();
		assert_eq!(report.written_files.len(), 1);
		assert_eq!(report.imported_nodes.len(), 1);
		assert_eq!(report.imported_nodes[0].new_id, "lit");

		let p = td.path().join("mynew.flowgraph.toml");
		let text = fs::read_to_string(&p).unwrap();
		assert!(text.contains("id = \"lit\""));
	}

	#[test]
	fn paste_group_metadata_rewrites_conflicting_node_ids() {
		let td = mk_dir();
		write_file(
			&td.path().join("dest.flowgraph.toml"),
			r#"
[[nodes]]
id = "a"
feature = "flowgraph.literal.string"
"#,
		);
		let frag = Fragment {
			header: FragmentHeader {
				schema: FRAGMENT_SCHEMA_V1.into(),
				exported_at: "2026-04-17T00:00:00Z".into(),
				origin: None,
				scope: FragmentScope::Nodes,
			},
			files: vec![FragmentFile {
				path: SCRATCH_PATH.into(),
				meta: None,
				nodes: vec![
					NodeEntry {
						id: "a".into(),
						feature: "flowgraph.literal.string".into(),
						position: None,
						properties: toml::Table::new(),
					},
					NodeEntry {
						id: "b".into(),
						feature: "flowgraph.log.info".into(),
						position: None,
						properties: toml::Table::new(),
					},
				],
				edges: vec![],
				groups: vec![FlowgraphGroupDef {
					id: "g1".into(),
					label: Some("Pair".into()),
					node_ids: vec!["a".into(), "b".into()],
					color: Some("#38bdf8".into()),
				}],
				..Default::default()
			}],
			danglings: vec![],
		};
		let req = PasteRequest {
			fragment_toml: serialize_fragment(&frag).unwrap(),
			target: PasteTarget::File { fq: "dest".into() },
			options: PasteOptions::default(),
		};
		let _ = paste_fragment(td.path(), &req).unwrap();
		let text = fs::read_to_string(td.path().join("dest.flowgraph.toml")).unwrap();
		let parsed = parse_flowgraph_file(&text, None).unwrap();
		assert!(parsed.nodes.iter().any(|n| n.id == "a_1"), "{text}");
		assert_eq!(parsed.groups.len(), 1, "{text}");
		assert_eq!(parsed.groups[0].id, "g1");
		assert_eq!(parsed.groups[0].node_ids, vec!["a_1".to_string(), "b".to_string()]);
	}

	#[test]
	fn paste_nodes_into_existing_file_with_conflict_suffix() {
		let td = mk_dir();
		write_file(
			&td.path().join("main.flowgraph.toml"),
			r#"
[[nodes]]
id = "lit"
feature = "flowgraph.literal.string"
[nodes.properties]
value = "existing"
"#,
		);

		let toml = make_scratch_fragment("lit", None);
		let req = PasteRequest {
			fragment_toml: toml,
			target: PasteTarget::File { fq: "main".into() },
			options: PasteOptions::default(),
		};
		let report = paste_fragment(td.path(), &req).unwrap();
		assert_eq!(report.imported_nodes.len(), 1);
		assert_eq!(report.imported_nodes[0].original_id, "lit");
		assert_eq!(report.imported_nodes[0].new_id, "lit_1");
		// .bak ができる
		assert_eq!(report.backups.len(), 1);

		let text = fs::read_to_string(td.path().join("main.flowgraph.toml")).unwrap();
		assert!(text.contains("id = \"lit\""));
		assert!(text.contains("id = \"lit_1\""));
	}

	#[test]
	fn paste_nodes_skip_conflict() {
		let td = mk_dir();
		write_file(
			&td.path().join("main.flowgraph.toml"),
			r#"
[[nodes]]
id = "lit"
feature = "flowgraph.literal.string"
[nodes.properties]
value = "existing"
"#,
		);

		let toml = make_scratch_fragment("lit", None);
		let req = PasteRequest {
			fragment_toml: toml,
			target: PasteTarget::File { fq: "main".into() },
			options: PasteOptions {
				on_conflict_node: OnConflict::Skip,
				..Default::default()
			},
		};
		let report = paste_fragment(td.path(), &req).unwrap();
		assert_eq!(report.imported_nodes.len(), 0);
		assert_eq!(report.skipped_nodes.len(), 1);
		assert_eq!(report.skipped_nodes[0].original_id, "lit");
	}

	#[test]
	fn paste_nodes_overwrite_conflict() {
		let td = mk_dir();
		write_file(
			&td.path().join("main.flowgraph.toml"),
			r#"
[[nodes]]
id = "lit"
feature = "flowgraph.literal.string"
[nodes.properties]
value = "OLD"
"#,
		);

		let toml = make_scratch_fragment("lit", None);
		let req = PasteRequest {
			fragment_toml: toml,
			target: PasteTarget::File { fq: "main".into() },
			options: PasteOptions {
				on_conflict_node: OnConflict::Overwrite,
				..Default::default()
			},
		};
		let _ = paste_fragment(td.path(), &req).unwrap();
		let text = fs::read_to_string(td.path().join("main.flowgraph.toml")).unwrap();
		assert!(text.contains("value = \"hi\""), "overwrite: {}", text);
		assert!(!text.contains("OLD"));
	}

	#[test]
	fn paste_file_into_folder_new() {
		let td = mk_dir();
		// fragment with scope=file, 1 file entry "chat/bot.flowgraph.toml".
		let frag = Fragment {
			header: FragmentHeader {
				schema: FRAGMENT_SCHEMA_V1.into(),
				exported_at: "2026-04-17T00:00:00Z".into(),
				origin: None,
				scope: FragmentScope::File,
			},
			files: vec![FragmentFile {
				path: "chat/bot.flowgraph.toml".into(),
				meta: None,
				nodes: vec![NodeEntry {
					id: "a".into(),
					feature: "flowgraph.literal.string".into(),
					position: None,
					properties: {
						let mut t = toml::Table::new();
						t.insert("value".into(), "x".into());
						t
					},
				}],
				edges: vec![],
				groups: vec![],
				..Default::default()
			}],
			danglings: vec![],
		};
		let req = PasteRequest {
			fragment_toml: serialize_fragment(&frag).unwrap(),
			target: PasteTarget::Folder { path: "imported".into() },
			options: PasteOptions::default(),
		};
        struct RejectPlan;
        impl WriteObserver for RejectPlan {
            fn graphs_planned(&self,files:&[PlannedGraph],report:&PasteReport)->std::io::Result<()> {
                assert_eq!(files.len(),1);
                assert_eq!(files[0].fq,"imported/chat/bot");
                assert!(files[0].content.contains("flowgraph.literal.string"));
                assert!(report.written_files.is_empty());
                Err(std::io::Error::other("journal unavailable"))
            }
            fn before_write(&self,_:&Path,_:Option<&[u8]>,_:&[u8],_:Option<&str>)->std::io::Result<()> {panic!("must not publish without complete plan")}
        }
        assert!(paste_fragment_observed(td.path(),&req,Some(&RejectPlan)).is_err());
        assert!(!td.path().join("imported/chat/bot.flowgraph.toml").exists());
		let report = paste_fragment(td.path(), &req).unwrap();
		assert_eq!(report.written_files.len(), 1);
		assert_eq!(report.written_files[0].fq, "imported/chat/bot");
		assert_eq!(report.written_files[0].original_fq.as_deref(), Some("chat/bot"));
		assert!(td.path().join("imported/chat/bot.flowgraph.toml").exists());
	}

	#[test]
	fn paste_file_folder_conflict_rename() {
		let td = mk_dir();
		write_file(
			&td.path().join("chat/bot.flowgraph.toml"),
			r#"
[[nodes]]
id = "preexisting"
feature = "flowgraph.literal.string"
[nodes.properties]
value = "x"
"#,
		);
		let frag = Fragment {
			header: FragmentHeader {
				schema: FRAGMENT_SCHEMA_V1.into(),
				exported_at: "2026-04-17T00:00:00Z".into(),
				origin: None,
				scope: FragmentScope::File,
			},
			files: vec![FragmentFile {
				path: "chat/bot.flowgraph.toml".into(),
				meta: None,
				nodes: vec![NodeEntry {
					id: "new".into(),
					feature: "flowgraph.literal.string".into(),
					position: None,
					properties: {
						let mut t = toml::Table::new();
						t.insert("value".into(), "y".into());
						t
					},
				}],
				edges: vec![],
				groups: vec![],
				..Default::default()
			}],
			danglings: vec![],
		};
		let req = PasteRequest {
			fragment_toml: serialize_fragment(&frag).unwrap(),
			target: PasteTarget::Folder { path: "".into() },
			options: PasteOptions {
				on_conflict_file: OnConflictFile::Rename,
				..Default::default()
			},
		};
		let report = paste_fragment(td.path(), &req).unwrap();
		assert_eq!(report.written_files.len(), 1);
		assert_eq!(report.written_files[0].fq, "chat/bot_1");
		assert_eq!(report.written_files[0].original_fq.as_deref(), Some("chat/bot"));
		let mut skipped = req.clone();
		skipped.options.on_conflict_file = OnConflictFile::Skip;
		let skipped_report = paste_fragment(td.path(), &skipped).unwrap();
		assert!(skipped_report.written_files.is_empty());
	}

	#[test]
	fn paste_dangling_remap_reconstructs_edge() {
		let td = mk_dir();
		// 既存ファイルに "keeper" ノード。
		write_file(
			&td.path().join("dest.flowgraph.toml"),
			r#"
[[nodes]]
id = "keeper"
feature = "flowgraph.log.info"
"#,
		);

		let frag = Fragment {
			header: FragmentHeader {
				schema: FRAGMENT_SCHEMA_V1.into(),
				exported_at: "2026-04-17T00:00:00Z".into(),
				origin: None,
				scope: FragmentScope::Nodes,
			},
			files: vec![FragmentFile {
				path: SCRATCH_PATH.into(),
				meta: None,
				nodes: vec![NodeEntry {
					id: "src".into(),
					feature: "flowgraph.literal.string".into(),
					position: None,
					properties: {
						let mut t = toml::Table::new();
						t.insert("value".into(), "x".into());
						t
					},
				}],
				edges: vec![],
				groups: vec![],
				..Default::default()
			}],
			danglings: vec![FragmentDangling {
				kind: "edge_endpoint".into(),
				original: "external::x:in".into(),
				reason: "out_of_scope".into(),
				in_file: Some("main".into()),
				edge_from: Some("src:value".into()),
				edge_to: Some("external::x:in".into()),
				external_side: Some("to".into()),
			}],
		};

		let mut remap: HashMap<String, Option<String>> = HashMap::new();
		remap.insert("external::x:in".into(), Some("keeper:message".into()));

		let req = PasteRequest {
			fragment_toml: serialize_fragment(&frag).unwrap(),
			target: PasteTarget::File { fq: "dest".into() },
			options: PasteOptions {
				remap,
				..Default::default()
			},
		};
		let _ = paste_fragment(td.path(), &req).unwrap();
		let text = fs::read_to_string(td.path().join("dest.flowgraph.toml")).unwrap();
		assert!(text.contains("from = \"src:value\""));
		assert!(text.contains("to = \"keeper:message\""), "{text}");
	}

	#[test]
	fn paste_dangling_drop_and_unresolved() {
		let td = mk_dir();
		let frag = Fragment {
			header: FragmentHeader {
				schema: FRAGMENT_SCHEMA_V1.into(),
				exported_at: "2026-04-17T00:00:00Z".into(),
				origin: None,
				scope: FragmentScope::Nodes,
			},
			files: vec![FragmentFile {
				path: SCRATCH_PATH.into(),
				meta: None,
				nodes: vec![],
				edges: vec![],
				groups: vec![],
				..Default::default()
			}],
			danglings: vec![
				FragmentDangling {
					kind: "edge_endpoint".into(),
					original: "dropped::n:p".into(),
					reason: "out_of_scope".into(),
					in_file: Some("main".into()),
					edge_from: Some("a:b".into()),
					edge_to: Some("dropped::n:p".into()),
					external_side: Some("to".into()),
				},
				FragmentDangling {
					kind: "edge_endpoint".into(),
					original: "unresolved::n:p".into(),
					reason: "out_of_scope".into(),
					in_file: None,
					edge_from: None,
					edge_to: None,
					external_side: None,
				},
			],
		};
		let mut remap: HashMap<String, Option<String>> = HashMap::new();
		remap.insert("dropped::n:p".into(), None);

		let req = PasteRequest {
			fragment_toml: serialize_fragment(&frag).unwrap(),
			target: PasteTarget::File { fq: "dest".into() },
			options: PasteOptions {
				remap,
				..Default::default()
			},
		};
		let report = paste_fragment(td.path(), &req).unwrap();
		assert_eq!(report.unresolved_danglings.len(), 1);
		assert_eq!(report.unresolved_danglings[0].original, "unresolved::n:p");
	}

	#[test]
	fn paste_position_offset_is_applied() {
		let td = mk_dir();
		let toml = make_scratch_fragment("lit", Some([10.0, 20.0]));
		let req = PasteRequest {
			fragment_toml: toml,
			target: PasteTarget::File { fq: "dst".into() },
			options: PasteOptions {
				position_offset: Some([100.0, 200.0]),
				..Default::default()
			},
		};
		let _ = paste_fragment(td.path(), &req).unwrap();
		let text = fs::read_to_string(td.path().join("dst.flowgraph.toml")).unwrap();
		// toml::to_string_pretty は配列を改行込みで書くため部分文字列で確認。
		assert!(text.contains("110.0"), "{text}");
		assert!(text.contains("220.0"), "{text}");
	}

	#[test]
	fn sanitize_fragment_path_rejects_traversal() {
		assert!(sanitize_fragment_path("../evil.flowgraph.toml").is_err());
		assert!(sanitize_fragment_path("/abs.flowgraph.toml").is_err());
		assert!(sanitize_fragment_path("ok.txt").is_err()); // 拡張子不一致
		assert_eq!(sanitize_fragment_path("chat/bot.flowgraph.toml").unwrap(), "chat/bot");
		assert_eq!(sanitize_fragment_path(SCRATCH_PATH).unwrap(), "__scratch__");
	}
    #[test]
    fn failed_staging_preserves_destination_and_removes_temporary_file() {
        for existing in [false, true] {
            let root = mk_dir();
            let path = root.path().join("main.flowgraph.toml");
            if existing { fs::write(&path, b"original").unwrap(); }
            let expected = read_destination(&path).unwrap();
            let mut report = PasteReport {written_files:vec![],imported_nodes:vec![],skipped_nodes:vec![],skipped_files:vec![],unresolved_danglings:vec![],backups:vec![]};
            let result = write_file_atomic_with(&path, expected.as_deref(), &mut report, |file| {
                file.write_all(b"incomplete")?;
                Err(std::io::Error::other("injected staging failure"))
            });
            assert!(matches!(result, Err(PasteError::Io { .. })));
            assert_eq!(read_destination(&path).unwrap(), expected);
            assert!(report.backups.is_empty());
            assert!(report.written_files.is_empty());
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), usize::from(existing));
        }
    }

    #[test]
    fn atomic_publish_preserves_existing_destination_and_backup() {
        let root = mk_dir();
        let path = root.path().join("main.flowgraph.toml");
        let replacement_planned = read_destination(&path).unwrap();
        assert!(replacement_planned.is_none());
        // Another writer creates the formerly empty destination after planning.
        std::fs::write(&path, b"# original exact bytes\r\n").unwrap();
        let doc: FlowgraphFile = toml::from_str("[meta]\ntitle='replacement'\n").unwrap();
        let mut report=PasteReport {written_files:vec![],imported_nodes:vec![],skipped_nodes:vec![],skipped_files:vec![],unresolved_danglings:vec![],backups:vec![]};
        assert!(write_file_atomic(&path,&doc,replacement_planned.as_deref(),&mut report).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),b"# original exact bytes\r\n");
        let original = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"# edited after planning").unwrap();
        assert!(matches!(write_file_atomic(&path,&doc,Some(&original),&mut report), Err(PasteError::DestinationChanged(_))));
        assert_eq!(std::fs::read(&path).unwrap(), b"# edited after planning");
        assert!(report.backups.is_empty());
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(write_file_atomic(&path,&doc,Some(&original),&mut report), Err(PasteError::DestinationChanged(_))));
        assert!(!path.exists());
        std::fs::write(&path, &original).unwrap();
        let digest = write_file_atomic(&path,&doc,Some(&original),&mut report).unwrap();
        assert_eq!(digest, blake3::hash(&std::fs::read(&path).unwrap()).to_hex().to_string());
        assert_eq!(std::fs::read(path.with_extension("toml.bak")).unwrap(),b"# original exact bytes\r\n");
        assert!(std::fs::read_to_string(&path).unwrap().contains("replacement"));
        assert!(std::fs::read_dir(root.path()).unwrap().all(|e|!e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
    }

}

/// dangling remap 表の alias（δ-7a の暫定定義と同義）。
pub type RemapTable = HashMap<String, Option<String>>;

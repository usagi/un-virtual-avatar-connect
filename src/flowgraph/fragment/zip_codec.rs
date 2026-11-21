//! Phase δ-7d: Fragment ZIP codec（spec §9.3）。
//!
//! 複数ファイル（＋補助ドキュメント等のコンパニオン）をまとめて 1 つの ZIP に export / import する。
//!
//! ## 構成
//! ```text
//! fragment.flowgraph.zip
//! ├── manifest.flowgraph.toml  // [fragment] header + [[fragment.danglings]] のみ
//! └── flowgraph/               // 元のフォルダ構造をそのまま内包
//!     ├── main.flowgraph.toml
//!     └── .../**/*.flowgraph.toml
//! ```
//! `[[fragment.files]]` は ZIP 内の実ファイルに委譲する（spec §9.3）。scope=nodes のときは
//! `flowgraph/__scratch__.flowgraph.toml` 1 本。
//!
//! ## 安全ルール（import 時、spec §9.3）
//! - `..` / 絶対パス / Windows ドライブ / シンボリックリンク → 拒否
//! - 実行ファイル系拡張子（`.exe`, `.dll`, `.bat`, `.ps1`, `.sh`, `.cmd`, `.com`, `.scr`, `.msi`）→ 拒否
//! - 上記以外は許可（`.md`, 画像, 音声等のコンパニオンを許容）
//!
//! ## dry_run プレビュー
//! import は `dry_run=true` でプレビューを返し、衝突一覧・dangling 一覧・進入予定の file 一覧を提示。
//! GUI はこれを見てから `dry_run=false` で本番 import を実行する。

use crate::flowgraph::fragment::paste::{PasteError, PasteOptions, PasteRequest, PasteTarget};
use crate::flowgraph::fragment::{
	copy_targets, parse_fragment, serialize_fragment, CopyError, CopyRequest, Fragment, FragmentDangling, FragmentFile, FragmentHeader,
	FragmentScope, SCRATCH_PATH,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

/// 拒否する危険拡張子（spec §9.3）。
/// 大文字小文字は区別せず小文字で比較する。
const DENY_EXTENSIONS: &[&str] = &["exe", "dll", "bat", "ps1", "sh", "cmd", "com", "scr", "msi", "app", "apk", "jar"];

/// ZIP 内で flowgraph ファイルを置くフォルダ（spec §9.3）。
const ZIP_FLOWGRAPH_PREFIX: &str = "flowgraph/";

/// ZIP 内の manifest file 名。
const MANIFEST_NAME: &str = "manifest.flowgraph.toml";
const PURPOSE_NAME: &str = "purpose.json";

/// Portable explicit selection. Local registration IDs and the local catalogue
/// are intentionally absent. This is not ownership or activation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortablePurpose {
    pub schema_version: u32,
    pub name: String,
    pub roots: Vec<String>,
}
impl PortablePurpose {
    fn validate(&self, files: impl Iterator<Item = String>) -> Result<(), String> {
        let available = files.collect::<std::collections::BTreeSet<_>>();
        let mut unique = std::collections::BTreeSet::new();
        if self.schema_version != 1 || self.name.trim().is_empty() || self.roots.is_empty()
            || serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > 65536 {
            return Err("invalid purpose schema, name or roots".into());
        }
        for root in &self.roots {
            if root.contains(['\\', ':']) || root.chars().any(char::is_control)
                || root.split('/').any(|p| matches!(p, "" | "." | ".."))
                || !unique.insert(root) || !available.contains(root) {
                return Err(format!("invalid or absent purpose root: {root}"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportedPurpose {
    pub name: String,
    /// Only roots actually written by this import; never guessed from prefix.
    pub roots: Vec<String>,
    /// Original roots skipped by the import. Registration must not silently
    /// substitute a pre-existing destination for these missing members.
    pub missing_roots: Vec<String>,
}


// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ZipExportError {
	#[error("補助ファイルを同梱できません: {0}")]
	InvalidCompanion(String),
	#[error("copy に失敗: {0}")]
	Copy(#[from] CopyError),
	#[error("serialize に失敗: {0}")]
	Serialize(String),
	#[error("zip 書き込みに失敗: {0}")]
	Zip(#[from] zip::result::ZipError),
	#[error("I/O: {0}")]
	Io(#[from] std::io::Error),
}

#[derive(Debug, thiserror::Error)]
pub enum ZipImportError {
	#[error("素材の保存中に失敗しました ({failed_path}): {source}")]
	CompanionWrite {
		source: std::io::Error,
		failed_path: String,
		written_files: Vec<String>,
		file_mappings: Vec<crate::flowgraph::fragment::paste::WrittenFile>,
		written_companions: Vec<String>,
        companion_mappings: Vec<WrittenCompanion>,
		backups: Vec<String>,
	},
	#[error("既存の素材は上書きできません: {0}")]
	CompanionConflict(String),
	#[error("zip 読み出しに失敗: {0}")]
	Zip(#[from] zip::result::ZipError),
	#[error("I/O: {0}")]
	Io(#[from] std::io::Error),
	#[error("manifest が見つからない: '{MANIFEST_NAME}' が zip 内にありません")]
	ManifestMissing,
	#[error("manifest パース失敗: {0}")]
	ManifestParse(String),
	#[error("危険なエントリを検出: {0}")]
	UnsafeEntry(String),
	#[error("paste 段階で失敗: {0}")]
	Paste(#[from] PasteError),
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// copy と同じ targets 指定で、ZIP バイナリを生成する。
///
/// 内部では `copy_targets` で fragment を作り、`[[fragment.files]]` を ZIP 内の
/// 実ファイルに展開する。`manifest.flowgraph.toml` は fragment から files セクションを
/// 抜いた薄いメタだけ残す（spec §9.3）。
pub fn export_zip(root: &Path, req: &CopyRequest) -> Result<Vec<u8>, ZipExportError> {
	export_zip_with_companions(root, req, &[])
}

/// Existing request fields remain valid. Companion paths are explicitly selected,
/// root-relative files, never inferred from nearby folders or node properties.
#[derive(Debug, Deserialize)]
pub struct ZipExportRequest {
	#[serde(flatten)]
	pub copy: CopyRequest,
	#[serde(default)]
	pub companions: Vec<String>,
	/// Optional for legacy callers; interactive selection binds to its inventory root.
	#[serde(default)]
	pub expected_root: Option<String>,
	/// Reviewed graph inventory, including dependencies and potential new definitions.
	#[serde(default)]
	pub expected_graph_sources: Option<BTreeMap<String, String>>,
}

fn local_purpose_metadata(name: &str) -> bool {
    name.split('/').any(|part| {
        let part = part.to_ascii_lowercase();
        part == ".flowgraph-write.lock" || part == ".flowgraph-purposes.json" || part.starts_with(".flowgraph-purposes.json.bak-") || part.starts_with(".flowgraph-purpose-import-") || part.starts_with(".flowgraph-purpose-removal-") || part.starts_with(".flowgraph-purpose-transfer-")
    })
}

fn companion_name_allowed(name: &str) -> bool {
	!local_purpose_metadata(name)
        && !is_unsafe_entry_path(name) && !name.contains(['\\', ':'])
		&& !name.split('/').any(|part| part.is_empty() || part == ".")
		&& !has_deny_extension(name) && !name.to_ascii_lowercase().ends_with(".flowgraph.toml")
}

#[derive(Debug, Serialize)]
pub struct CompanionCandidate {
	pub path: String,
	pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct CompanionInventory {
	pub root_dir: String,
	pub files: Vec<CompanionCandidate>,
	pub truncated: bool,
}

/// Metadata only. No selection is implied and no file contents are read.
pub fn list_companion_candidates(root: &Path) -> std::io::Result<CompanionInventory> {
	let root = root.canonicalize()?;
	let mut directories = vec![root.clone()];
	let mut files = Vec::new();
	let mut visited = 0;
	let mut truncated = false;
	'walk: while let Some(directory) = directories.pop() {
		for entry in std::fs::read_dir(directory)? {
			if visited >= 10_000 { truncated = true; break 'walk; }
			visited += 1;
			let entry = entry?;
			let metadata = std::fs::symlink_metadata(entry.path())?;
			if metadata.file_type().is_symlink() { continue; }
			let resolved = entry.path().canonicalize()?;
			if !resolved.starts_with(&root) { continue; }
			if metadata.is_dir() { directories.push(resolved); continue; }
			if !metadata.is_file() { continue; }
			let relative = resolved.strip_prefix(&root).expect("checked root");
			let Some(name) = relative.to_str() else { continue; };
			let name = name.replace('\\', "/");
			if companion_name_allowed(&name) { files.push(CompanionCandidate { path: name, bytes: metadata.len() }); }
		}
	}
	files.sort_by(|a, b| a.path.cmp(&b.path));
	Ok(CompanionInventory { root_dir: root.to_string_lossy().into_owned(), files, truncated })
}

pub fn export_zip_with_companions(root: &Path, req: &CopyRequest, selected: &[String]) -> Result<Vec<u8>, ZipExportError> {
    export_zip_with_purpose(root, req, selected, None)
}

pub fn export_zip_with_purpose(root: &Path, req: &CopyRequest, selected: &[String], purpose: Option<&PortablePurpose>) -> Result<Vec<u8>, ZipExportError> {
	let fragment = copy_targets(root, req)?;
    if let Some(purpose) = purpose {
        purpose.validate(fragment.files.iter().map(|f| f.path.trim_end_matches(".flowgraph.toml").to_owned())).map_err(ZipExportError::Serialize)?;
    }
	let root = root.canonicalize()?;
	let mut companions = BTreeMap::new();
	for name in selected {
		if !companion_name_allowed(name) {
			return Err(ZipExportError::InvalidCompanion(name.clone()));
		}
		let mut path = root.clone();
		for part in name.split('/') {
			path.push(part);
			if std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
				return Err(ZipExportError::InvalidCompanion(name.clone()));
			}
		}
		let resolved = path.canonicalize()?;
		if !resolved.starts_with(&root) || !resolved.is_file() {
			return Err(ZipExportError::InvalidCompanion(name.clone()));
		}
		// Reading all selections before producing a ZIP prevents partial success.
		companions.insert(name.clone(), std::fs::read(resolved)?);
	}

	let mut buf: Vec<u8> = Vec::new();
	{
		let cursor = Cursor::new(&mut buf);
		let mut zw = zip::ZipWriter::new(cursor);
		let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
			.compression_method(zip::CompressionMethod::Deflated)
			.unix_permissions(0o644);

		// 1) manifest（fragment の header + danglings のみ）
		let manifest_str = build_manifest_toml(&fragment)?;
		zw.start_file(MANIFEST_NAME, opts)?;
		zw.write_all(manifest_str.as_bytes())?;

		if let Some(purpose) = purpose {
            zw.start_file(PURPOSE_NAME, opts)?;
            zw.write_all(&serde_json::to_vec(purpose).map_err(|e| ZipExportError::Serialize(e.to_string()))?)?;
        }

        // 2) flowgraph/ 配下の各ファイル
		for ff in &fragment.files {
			let zip_path = format!("{ZIP_FLOWGRAPH_PREFIX}{}", ff.path);
			zw.start_file(&zip_path, opts)?;
			let file_toml = serialize_fragment_file_as_flowgraph_toml(ff).map_err(ZipExportError::Serialize)?;
			zw.write_all(file_toml.as_bytes())?;
		}

		for (name, bytes) in companions {
			zw.start_file(format!("{ZIP_FLOWGRAPH_PREFIX}{name}"), opts)?;
			zw.write_all(&bytes)?;
		}
		zw.finish()?;
	}
	Ok(buf)
}

/// manifest = fragment から `[[fragment.files]]` を外した TOML。
fn build_manifest_toml(fragment: &Fragment) -> Result<String, ZipExportError> {
	let manifest = Fragment {
		header: fragment.header.clone(),
		files: Vec::new(),
		danglings: fragment.danglings.clone(),
	};
	serialize_fragment(&manifest).map_err(ZipExportError::Serialize)
}

/// FragmentFile を `*.flowgraph.toml` の生形式に書き出す（meta + nodes + edges + groups）。
fn serialize_fragment_file_as_flowgraph_toml(ff: &FragmentFile) -> Result<String, String> {
	let doc = crate::flowgraph::loader::file::FlowgraphFile {
		meta: ff.meta.clone(),
		graph: ff.graph.clone(),
		package: ff.package.clone(),
		nodes: ff.nodes.clone(),
		edges: ff.edges.clone(),
		groups: ff.groups.clone(),
		enums: ff.enums.clone(),
		types: ff.types.clone(),
		instances: ff.instances.clone(),
		..Default::default()
	};
	toml::to_string_pretty(&doc).map_err(|e| format!("serialize: {e}"))
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// import オプション。HTTP 層 (`POST /flowgraph/import/zip`) から渡す。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ZipImportOptions {
	/// true なら実ファイル書き込みは行わず [`ZipImportPreview`] を返す。
	#[serde(default)]
	pub dry_run: bool,
	/// 既定 `"imported"`。prefix 以下に fragment の flowgraph/ を展開する。
	/// 空文字列で root 直下展開（上書きリスクが上がるので非推奨）。
	#[serde(default = "default_target_prefix")]
	pub target_prefix: String,
	/// paste オプション（衝突戦略・remap 等）。
	#[serde(default)]
	pub paste: PasteOptions,
}

fn default_target_prefix() -> String {
	"imported".to_string()
}

/// dry_run 応答。衝突と dangling の件数・リストを一覧化する。
#[derive(Debug, Clone, Serialize)]
pub struct ZipImportPreview {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<PortablePurpose>,
	pub manifest: FragmentHeader,
	pub file_count: usize,
	/// 展開予定のエントリ（flowgraph/ 以下のみ / 安全チェック通過済）。
	pub entries: Vec<ZipImportEntry>,
	pub danglings: Vec<FragmentDangling>,
	/// target_prefix（実際に適用される）。
	pub target_prefix: String,
	/// 既存と衝突する fq（拡張子なしの root 相対）。
	pub conflicts: Vec<String>,
	/// companion files（`.flowgraph.toml` 以外、展開先のみ提示）。
	pub companions: Vec<ZipImportEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WrittenCompanion {
    pub source_path: String,
    pub path: String,
    pub content_digest: String,
}

/// 本番 import の結果。内部で `paste_fragment` を呼んでいるため [`PasteReport`] 互換のサマリ。
#[derive(Debug, Clone, Serialize)]
pub struct ZipImportReport {
    /// Import result only; no local purpose registration has been saved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<ImportedPurpose>,
	/// Successfully written source-to-destination mappings; skipped files are absent.
	pub file_mappings: Vec<crate::flowgraph::fragment::paste::WrittenFile>,
	pub manifest: FragmentHeader,
	pub written_files: Vec<String>,
	pub written_companions: Vec<String>,
    pub companion_mappings: Vec<WrittenCompanion>,
	pub danglings_unresolved: Vec<FragmentDangling>,
	pub target_prefix: String,
	/// Set by the control API after applying the saved files to the runtime.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub reload_ok: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub restart_required: Option<bool>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub diagnostics_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZipImportEntry {
	pub zip_path: String,
	pub dest_path: String,
	pub size: u64,
}

/// ZIP バイナリを import（dry_run or 本番）。
pub fn import_zip(root: &Path, bytes: &[u8], opts: &ZipImportOptions) -> Result<ZipImportOutcome, ZipImportError> {
    import_zip_observed(root, bytes, opts, None)
}
pub(crate) fn import_zip_observed(root: &Path, bytes: &[u8], opts: &ZipImportOptions, observer: Option<&dyn super::paste::WriteObserver>) -> Result<ZipImportOutcome, ZipImportError> {
	let reader = Cursor::new(bytes);
	let mut archive = zip::ZipArchive::new(reader)?;

	// (1) manifest + safe paths を舐めてメモリに載せる。
	let mut manifest_text: Option<String> = None;
    let mut purpose: Option<PortablePurpose> = None;
	let mut fg_files: BTreeMap<String, String> = BTreeMap::new(); // zip_rel_path → content
	let mut companions: Vec<ZipImportEntry> = Vec::new();
	let mut companion_bytes: BTreeMap<String, Vec<u8>> = BTreeMap::new();

	for i in 0..archive.len() {
		let mut entry = archive.by_index(i)?;
		let raw_name = entry.name().to_string();
		if entry.is_dir() {
			continue;
		}
		if is_unsafe_entry_path(&raw_name) {
			return Err(ZipImportError::UnsafeEntry(raw_name));
		}
		// unix mode 確認: symlink 検出。
		if let Some(mode) = entry.unix_mode() {
			// S_IFLNK = 0o120000
			if mode & 0o170000 == 0o120000 {
				return Err(ZipImportError::UnsafeEntry(format!("symlink を拒否: {raw_name}")));
			}
		}
		if has_deny_extension(&raw_name) {
			return Err(ZipImportError::UnsafeEntry(format!("危険な拡張子を拒否: {raw_name}")));
		}

		if raw_name == MANIFEST_NAME {
			let mut s = String::new();
			entry.read_to_string(&mut s)?;
			manifest_text = Some(s);
		} else if raw_name == PURPOSE_NAME {
            if purpose.is_some() || entry.size() > 65536 { return Err(ZipImportError::ManifestParse("duplicate or oversized purpose metadata".into())); }
            let mut bytes = Vec::new();
            entry.by_ref().take(65537).read_to_end(&mut bytes)?;
            if bytes.len() > 65536 { return Err(ZipImportError::ManifestParse("oversized purpose metadata".into())); }
            purpose = Some(serde_json::from_slice(&bytes).map_err(|e| ZipImportError::ManifestParse(e.to_string()))?);
        } else if let Some(rel) = raw_name.strip_prefix(ZIP_FLOWGRAPH_PREFIX) {
            if local_purpose_metadata(rel) { return Err(ZipImportError::UnsafeEntry(raw_name)); }
			// flowgraph/ 配下
			if rel.ends_with(".flowgraph.toml") {
				let mut s = String::new();
				entry.read_to_string(&mut s)?;
				fg_files.insert(rel.to_string(), s);
			} else {
				// companion file（GUI 側で活用される前提で保存）
				let size = entry.size();
				let mut b = Vec::with_capacity(size as usize);
				entry.read_to_end(&mut b)?;
				companions.push(ZipImportEntry {
					zip_path: raw_name.clone(),
					dest_path: companion_dest(rel, &opts.target_prefix),
					size,
				});
				companion_bytes.insert(rel.to_string(), b);
			}
		}
		// それ以外（zip 直下の `.md` 等）は無視する。
	}

	if let Some(purpose) = &purpose {
        purpose.validate(fg_files.keys().map(|f| f.trim_end_matches(".flowgraph.toml").to_owned())).map_err(ZipImportError::ManifestParse)?;
    }
    let manifest_text = manifest_text.ok_or(ZipImportError::ManifestMissing)?;
	// manifest は Fragment の軽量版（files 空 + danglings のみ + header）。
	let manifest_frag = parse_fragment(&manifest_text).map_err(ZipImportError::ManifestParse)?;

	// (2) fragment を再構築: manifest.header + danglings + fg_files からの nodes/edges。
	let mut rebuilt = Fragment {
		header: manifest_frag.header.clone(),
		files: Vec::new(),
		danglings: manifest_frag.danglings.clone(),
	};
	for (rel, content) in &fg_files {
		let parsed = crate::flowgraph::loader::file::parse_flowgraph_file(content, None)
			.map_err(|e| ZipImportError::ManifestParse(format!("{rel}: {e}")))?;
		rebuilt.files.push(FragmentFile {
			path: rel.clone(),
			meta: parsed.meta,
			graph: parsed.graph,
			package: parsed.package,
			enums: parsed.enums,
			types: parsed.types,
			instances: parsed.instances,
			nodes: parsed.nodes,
			edges: parsed.edges,
			groups: parsed.groups,
		});
	}

	// (3) dry_run なら preview を作って return。
	let target_prefix = sanitize_target_prefix(&opts.target_prefix)?;
    for entry in &mut companions {
        entry.dest_path = companion_dest(entry.zip_path.strip_prefix(ZIP_FLOWGRAPH_PREFIX).unwrap(), &target_prefix);
    }
    let companion_conflicts = companions.iter().filter(|entry| root.join(&entry.dest_path).exists()).map(|entry| entry.dest_path.clone()).collect::<Vec<_>>();
	if opts.dry_run {
		let entries = fg_files
			.iter()
			.map(|(rel, content)| ZipImportEntry {
				zip_path: format!("{ZIP_FLOWGRAPH_PREFIX}{rel}"),
				dest_path: flowgraph_dest(rel, &target_prefix),
				size: content.len() as u64,
			})
			.collect::<Vec<_>>();
		let conflicts = entries
			.iter()
			.filter(|e| root.join(&e.dest_path).exists())
			.map(|e| e.dest_path.clone())
			.chain(companion_conflicts.iter().cloned())
			.collect::<Vec<_>>();
		return Ok(ZipImportOutcome::Preview(ZipImportPreview {
            purpose,
			manifest: rebuilt.header,
			file_count: entries.len(),
			entries,
			danglings: rebuilt.danglings,
			target_prefix,
			conflicts,
			companions,
		}));
	}

	// Reject before any graph is written. Companion paths cannot be safely
    // renamed independently of references inside the graph.
    if let Some(conflict) = companion_conflicts.first() {
        return Err(ZipImportError::CompanionConflict(conflict.clone()));
    }
    // (4) 本番書き込み: fragment の TOML を作って paste_fragment に委譲。
	let fragment_toml = serialize_fragment(&rebuilt).map_err(ZipImportError::ManifestParse)?;
	let paste_req = PasteRequest {
		fragment_toml,
		target: PasteTarget::Folder {
			path: target_prefix.clone(),
		},
		options: opts.paste.clone(),
	};
	let report = super::paste::paste_fragment_observed(root, &paste_req, observer)?;
    if let Some(observer) = observer {
        if let Err(err) = observer.graphs_completed(&report) {
            return Err(ZipImportError::Paste(PasteError::PartialWrite {
                source: Box::new(PasteError::Io {path:root.into(),err}), report:Box::new(report),
            }));
        }
    }


	// (5) companion files を flowgraph_dir/<target_prefix>/ 以下にそのまま展開。
	let companion_mappings = write_companions_observed(root, &target_prefix, &companion_bytes, &report, observer)?;

	let purpose = purpose.map(|purpose| {
        let mut mapped = ImportedPurpose { name: purpose.name, roots: vec![], missing_roots: vec![] };
        for root in purpose.roots {
            if let Some(file) = report.written_files.iter().find(|f| f.original_fq.as_ref() == Some(&root)) {
                mapped.roots.push(file.fq.clone());
            } else { mapped.missing_roots.push(root); }
        }
        mapped
    });
    Ok(ZipImportOutcome::Report(ZipImportReport {
        purpose,
		manifest: rebuilt.header,
		written_files: report.written_files.iter().map(|w| w.path.clone()).collect(),
		written_companions: companion_mappings.iter().map(|file| file.path.clone()).collect(),
        companion_mappings,
		file_mappings: report.written_files,
		danglings_unresolved: report.unresolved_danglings,
		target_prefix,
		reload_ok: None,
		restart_required: None,
		diagnostics_count: None,
	}))
}

// Publish a complete companion without exposing a truncated destination.
fn publish_companion(path: &Path, write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or_else(|| std::io::Error::other("missing companion parent"))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(".vac-companion-{}-{}-{}.tmp", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
    let _cleanup = Cleanup(temp.clone());
    // Close before cleanup, including the error path (Windows does not permit unlinking an open file).
    let result = write(&mut file).and_then(|_| file.sync_all());
    drop(file);
    result?;
    std::fs::hard_link(&temp, path)
}

#[cfg(test)]
fn write_companions(root: &Path, prefix: &str, companions: &BTreeMap<String, Vec<u8>>, report: &crate::flowgraph::fragment::paste::PasteReport) -> Result<Vec<WrittenCompanion>, ZipImportError> {
    write_companions_observed(root, prefix, companions, report, None)
}
fn write_companions_observed(root: &Path, prefix: &str, companions: &BTreeMap<String, Vec<u8>>, report: &crate::flowgraph::fragment::paste::PasteReport, observer: Option<&dyn super::paste::WriteObserver>) -> Result<Vec<WrittenCompanion>, ZipImportError> {
    let mut written: Vec<WrittenCompanion> = Vec::new();
    for (rel, bytes) in companions {
        let dest = compose_companion_path(root, prefix, Path::new(rel));
        let publication = observer.map_or(Ok(()), |observer| observer.before_write(&dest, None, bytes, Some(rel)))
            .and_then(|_| publish_companion(&dest, |file| file.write_all(bytes)));
        if let Err(source) = publication {
            return Err(ZipImportError::CompanionWrite {
                source, failed_path: dest.display().to_string(),
                written_files: report.written_files.iter().map(|file| file.path.clone()).collect(),
                file_mappings: report.written_files.clone(),
                written_companions: written.iter().map(|file| file.path.clone()).collect(), companion_mappings: written, backups: report.backups.clone(),
            });
        }
        written.push(WrittenCompanion { source_path: rel.clone(), path: dest.display().to_string().replace('\\', "/"), content_digest: blake3::hash(bytes).to_hex().to_string() });
    }
    Ok(written)
}

/// `import_zip` の戻り値を dry_run / 本番で分岐。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ZipImportOutcome {
	Preview(ZipImportPreview),
	Report(ZipImportReport),
}

// ---------------------------------------------------------------------------
// Safety helpers
// ---------------------------------------------------------------------------

fn is_unsafe_entry_path(name: &str) -> bool {
	if name.is_empty() {
		return true;
	}
	if name.contains('\0') {
		return true;
	}
	// Windows ドライブ / UNC
	if name.starts_with('/') || name.starts_with('\\') {
		return true;
	}
	// `C:\...` 系
	let bytes = name.as_bytes();
	if bytes.len() >= 2 && bytes[1] == b':' {
		return true;
	}
	for seg in name.split('/') {
		if seg == ".." {
			return true;
		}
	}
	false
}

fn has_deny_extension(name: &str) -> bool {
	let lower = name.to_ascii_lowercase();
	let Some(dot) = lower.rfind('.') else {
		return false;
	};
	let ext = &lower[dot + 1..];
	DENY_EXTENSIONS.contains(&ext)
}

fn sanitize_target_prefix(s: &str) -> Result<String, ZipImportError> {
	let t = s.trim().trim_matches('/').replace('\\', "/");
	if t.is_empty() {
		return Ok(String::new());
	}
	for seg in t.split('/') {
		if seg == ".." || seg == "." || seg.is_empty() {
			return Err(ZipImportError::UnsafeEntry(format!("target_prefix 不正: '{s}'")));
		}
	}
	Ok(t)
}

fn flowgraph_dest(rel: &str, target_prefix: &str) -> String {
	if target_prefix.is_empty() {
		rel.to_string()
	} else {
		format!("{target_prefix}/{rel}")
	}
}

fn companion_dest(rel: &str, target_prefix: &str) -> String {
	if target_prefix.is_empty() {
		rel.to_string()
	} else {
		format!("{target_prefix}/{rel}")
	}
}

fn compose_companion_path(root: &Path, target_prefix: &str, rel: &Path) -> PathBuf {
	let mut p = root.to_path_buf();
	if !target_prefix.is_empty() {
		for seg in target_prefix.split('/') {
			p.push(seg);
		}
	}
	for c in rel.components() {
		if let std::path::Component::Normal(os) = c {
			p.push(os);
		}
	}
	p
}

// 未使用警告避け
#[allow(dead_code)]
fn _unused_scope_touch(_: FragmentScope) {}
#[allow(dead_code)]
const _SCRATCH_TOUCH: &str = SCRATCH_PATH;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::fragment::{CopyRequest, CopyTarget, FragmentScope};
	use crate::flowgraph::loader::file::parse_flowgraph_file;
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
		let p = std::env::temp_dir().join(format!("vac-fg-zip-{}-{ns}-{seq}", std::process::id()));
		fs::create_dir_all(&p).unwrap();
		TempDir(p)
	}
	fn write_file(p: &Path, s: &str) {
		if let Some(parent) = p.parent() {
			fs::create_dir_all(parent).unwrap();
		}
		let mut f = fs::File::create(p).unwrap();
		f.write_all(s.as_bytes()).unwrap();
	}

    #[test]
    fn portable_purpose_roundtrip_relocation_rename_skip_and_preview() {
        let source = mk_dir();
        let dest = mk_dir();
        write_file(&source.path().join("main.flowgraph.toml"), "[meta]\ntitle='main'\n");
        write_file(&source.path().join("dependency.flowgraph.toml"), "[meta]\ntitle='dependency'\n");
        write_file(&source.path().join(".flowgraph-purposes.json"), "private catalogue");
        write_file(&dest.path().join(".flowgraph-purposes.json"), "existing local registrations");
        let request = CopyRequest {scope:FragmentScope::Folder,targets:vec![CopyTarget::Folder{path:String::new()}],origin:None};
        let purpose = PortablePurpose {schema_version:1,name:"配信支援".into(),roots:vec!["main".into()]};
        let bytes = export_zip_with_purpose(source.path(), &request, &[], Some(&purpose)).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(archive.len(), 4); // manifest, selected purpose, two graphs
        assert!(archive.by_name("flowgraph/.flowgraph-purposes.json").is_err());
        assert!(export_zip_with_purpose(source.path(), &request, &[".flowgraph-purposes.json".into()], Some(&purpose)).is_err());
        assert!(!list_companion_candidates(source.path()).unwrap().files.iter().any(|file| file.path.contains("flowgraph-purposes")));
        let mut options = ZipImportOptions {dry_run:true,target_prefix:"received".into(),..Default::default()};
        let ZipImportOutcome::Preview(preview) = import_zip(dest.path(), &bytes, &options).unwrap() else {panic!("preview")};
        assert_eq!(preview.purpose, Some(purpose.clone()));
        assert!(!dest.path().join("received").exists());
        options.dry_run = false;
        options.paste.on_conflict_file = crate::flowgraph::fragment::paste::OnConflictFile::Rename;
        for suffix in ["", "_1"] {
            let ZipImportOutcome::Report(report) = import_zip(dest.path(), &bytes, &options).unwrap() else {panic!("report")};
            let imported = report.purpose.unwrap();
            assert_eq!(imported.name, purpose.name);
            assert_eq!(imported.roots, vec![format!("received/main{suffix}")]);
            assert!(imported.missing_roots.is_empty());
        }
        options.paste.on_conflict_file = crate::flowgraph::fragment::paste::OnConflictFile::Skip;
        let ZipImportOutcome::Report(report) = import_zip(dest.path(), &bytes, &options).unwrap() else {panic!("report")};
        let imported = report.purpose.unwrap();
        assert!(imported.roots.is_empty());
        assert_eq!(imported.missing_roots, vec!["main"]);
        assert_eq!(std::fs::read_to_string(dest.path().join(".flowgraph-purposes.json")).unwrap(), "existing local registrations");
        let mut invalid = purpose.clone();
        invalid.roots = vec!["missing".into()];
        assert!(export_zip_with_purpose(source.path(), &request, &[], Some(&invalid)).is_err());
    }

    #[test]
    fn local_purpose_catalogue_cannot_be_imported_as_a_companion() {
        let source = mk_dir();
        write_file(&source.path().join("main.flowgraph.toml"), "[meta]\ntitle='main'\n");
        let request = CopyRequest {scope:FragmentScope::File,targets:vec![CopyTarget::File{fq:"main".into()}],origin:None};
        let base = export_zip(source.path(), &request).unwrap();
        for name in [".flowgraph-write.lock", "nested/.FLOWGRAPH-WRITE.LOCK", ".flowgraph-purposes.json", "nested/.FLOWGRAPH-PURPOSES.JSON", ".flowgraph-purposes.json.bak-injected", ".flowgraph-purpose-import-injected.json", "nested/.FLOWGRAPH-PURPOSE-REMOVAL-injected.json", ".flowgraph-purpose-transfer-injected.json", "nested/.FLOWGRAPH-PURPOSE-TRANSFER-injected.result.json"] {
            let mut original = zip::ZipArchive::new(Cursor::new(&base)).unwrap();
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for index in 0..original.len() { writer.raw_copy_file(original.by_index(index).unwrap()).unwrap(); }
            writer.start_file(format!("flowgraph/{name}"), zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(b"injected local catalogue").unwrap();
            let bytes = writer.finish().unwrap().into_inner();
            let dest = mk_dir();
            assert!(matches!(import_zip(dest.path(), &bytes, &ZipImportOptions::default()), Err(ZipImportError::UnsafeEntry(_))));
            assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
            assert!(!companion_name_allowed(name));
            write_file(&source.path().join(name), "local-only");
            let inventory = list_companion_candidates(source.path()).unwrap();
            assert!(!inventory.files.iter().any(|file| file.path == name));
            assert!(export_zip_with_companions(source.path(), &request, &[name.into()]).is_err());
        }
    }

    #[test]
    fn invalid_portable_purpose_rejected_before_graph_writes() {
        let source = mk_dir();
        write_file(&source.path().join("main.flowgraph.toml"), "[meta]\ntitle='main'\n");
        let request = CopyRequest {scope:FragmentScope::File,targets:vec![CopyTarget::File{fq:"main".into()}],origin:None};
        let base = export_zip(source.path(), &request).unwrap();
        for invalid in [
            r#"{"schema_version":2,"name":"new","roots":["main"]}"#,
            r#"{"schema_version":1,"name":"new","roots":["missing"]}"#,
            r#"{"schema_version":1,"name":"new","roots":["main","main"]}"#,
            r#"{"schema_version":1,"name":"new","roots":["../main"]}"#,
            r#"{"schema_version":1,"name":"new","roots":["main"],"id":"overwrite"}"#,
        ] {
            let mut original = zip::ZipArchive::new(Cursor::new(&base)).unwrap();
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for index in 0..original.len() { writer.raw_copy_file(original.by_index(index).unwrap()).unwrap(); }
            writer.start_file(PURPOSE_NAME, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(invalid.as_bytes()).unwrap();
            let bytes = writer.finish().unwrap().into_inner();
            let dest = mk_dir();
            assert!(matches!(import_zip(dest.path(), &bytes, &ZipImportOptions::default()), Err(ZipImportError::ManifestParse(_))));
            assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 0);
        }
    }

	#[test]
	fn companion_inventory_lists_metadata_in_selected_root_only() {
		let first = mk_dir();
		let other = mk_dir();
		write_file(&first.path().join("assets/hello.txt"), "hello");
		write_file(&first.path().join("main.flowgraph.toml"), "graph");
		write_file(&first.path().join("run.exe"), "executable");
		write_file(&other.path().join("private.txt"), "other root");
		let inventory = list_companion_candidates(first.path()).unwrap();
		assert!(!inventory.truncated);
		assert_eq!(inventory.files.len(), 1);
		assert_eq!(inventory.files[0].path, "assets/hello.txt");
		assert_eq!(inventory.files[0].bytes, 5);
		assert_eq!(inventory.root_dir, first.path().canonicalize().unwrap().to_string_lossy());
		assert_eq!(fs::read(first.path().join("assets/hello.txt")).unwrap(), b"hello");
		assert!(list_companion_candidates(&first.path().join("missing")).is_err());
	}

	#[test]
	fn selected_companions_only_and_invalid_selection_is_all_or_nothing() {
		let dir = mk_dir();
		write_file(&dir.path().join("main.flowgraph.toml"), "[meta]\ntitle='Graph'\n");
		write_file(&dir.path().join("assets/readme.md"), "Selected");
		write_file(&dir.path().join("unselected.txt"), "Do not export");
		let req: ZipExportRequest = serde_json::from_value(serde_json::json!({"scope":"file","targets":[{"kind":"file","fq":"main"}]})).unwrap();
		assert!(req.companions.is_empty());
		let selected = vec!["assets/readme.md".to_string(), "assets/readme.md".to_string()];
		let bytes = export_zip_with_companions(dir.path(), &req.copy, &selected).unwrap();
		let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
		assert_eq!(zip.len(), 3); // manifest, graph, one deduplicated selection
		let mut text = String::new();
		zip.by_name("flowgraph/assets/readme.md").unwrap().read_to_string(&mut text).unwrap();
		assert_eq!(text, "Selected");
		assert!(zip.by_name("flowgraph/unselected.txt").is_err());
		for invalid in ["../unselected.txt", "assets/../unselected.txt", "/unselected.txt", "C:/secret", "main.flowgraph.toml", "assets/run.ps1", "assets", "missing.txt"] {
			assert!(export_zip_with_companions(dir.path(), &req.copy, &["assets/readme.md".into(), invalid.into()]).is_err(), "{invalid}");
		}
		assert_eq!(fs::read_to_string(dir.path().join("assets/readme.md")).unwrap(), "Selected");
	}

	#[test]
	fn companion_failure_leaves_no_partial_destination_and_preserves_progress() {
        let dir = mk_dir();
        let path = dir.path().join("partial.txt");
        let result = publish_companion(&path, |file| { file.write_all(b"half")?; Err(std::io::Error::other("injected write failure")) });
        assert!(result.is_err());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        publish_companion(&path, |file| file.write_all(b"winner")).unwrap();
        assert!(publish_companion(&path, |file| file.write_all(b"loser")).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"winner");
        let mut companions = BTreeMap::new();
        companions.insert("a.txt".into(), b"first".to_vec());
        companions.insert("z/second.txt".into(), b"second".to_vec());
        fs::create_dir(dir.path().join("imported")).unwrap();
        fs::write(dir.path().join("imported/z"), b"blocked parent").unwrap();
        let report = crate::flowgraph::fragment::paste::PasteReport {
            written_files:vec![crate::flowgraph::fragment::paste::WrittenFile{content_digest:"test-digest".into(),original_fq:None,fq:"main".into(),path:"main.flowgraph.toml".into()}],
            backups:vec!["main.bak".into()], imported_nodes:vec![],skipped_nodes:vec![],skipped_files:vec![],unresolved_danglings:vec![],
        };
        let error = write_companions(dir.path(), "imported", &companions, &report).unwrap_err();
        match error {
            ZipImportError::CompanionWrite { written_files, written_companions, backups, .. } => {
                assert_eq!(written_files, vec!["main.flowgraph.toml"]);
                assert_eq!(written_companions.len(),1);
                assert!(written_companions[0].ends_with("imported/a.txt"));
                assert_eq!(backups, vec!["main.bak"]);
            }
            other => panic!("unexpected: {other}"),
        }
        assert_eq!(fs::read(dir.path().join("imported/a.txt")).unwrap(), b"first");
        assert_eq!(fs::read(dir.path().join("imported/z")).unwrap(), b"blocked parent");
    }

	#[test]
	fn composition_folder_zip_preserves_executable_structure() {
		let src = mk_dir();
		let dest = mk_dir();
		let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("gui/tests/e2e/fixtures/flowgraph");
		let names = ["composition-module", "composition-parent", "composition-target"];
		for name in names {
			let file = format!("{name}.flowgraph.toml");
			fs::copy(fixtures.join(&file), src.path().join(&file)).unwrap();
		}
		let bytes = export_zip(
			src.path(),
			&CopyRequest {
				scope: FragmentScope::Folder,
				targets: vec![CopyTarget::Folder { path: String::new() }],
				origin: None,
			},
		)
		.unwrap();
		let outcome = import_zip(
			dest.path(),
			&bytes,
			&ZipImportOptions {
				target_prefix: String::new(),
				..Default::default()
			},
		)
		.unwrap();
		let ZipImportOutcome::Report(report) = outcome else {
			panic!("expected import report")
		};
		assert!(
			report.danglings_unresolved.is_empty(),
			"whole composition must not acquire unresolved connections"
		);
		for name in names {
			let file = format!("{name}.flowgraph.toml");
			let original = parse_flowgraph_file(&fs::read_to_string(src.path().join(&file)).unwrap(), None).unwrap();
			let restored = parse_flowgraph_file(&fs::read_to_string(dest.path().join(&file)).unwrap(), None).unwrap();
			assert_eq!(
				serde_json::to_value(original).unwrap(),
				serde_json::to_value(restored).unwrap(),
				"structure changed for {file}"
			);
		}
	}

	#[test]
	fn composition_zip_relocates_and_renames_internal_references() {
		let src = mk_dir();
		let dest = mk_dir();
		let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("gui/tests/e2e/fixtures/flowgraph");
		for name in ["composition-module", "composition-parent", "composition-target"] {
			let file = format!("{name}.flowgraph.toml");
			fs::copy(fixtures.join(&file), src.path().join(&file)).unwrap();
		}
		let bytes = export_zip(
			src.path(),
			&CopyRequest {
				scope: FragmentScope::Folder,
				targets: vec![CopyTarget::Folder { path: String::new() }],
				origin: None,
			},
		)
		.unwrap();
		let mut opts = ZipImportOptions {
			target_prefix: "added".into(),
			..Default::default()
		};
		opts.paste.on_conflict_file = crate::flowgraph::fragment::paste::OnConflictFile::Rename;
		for suffix in ["", "_1"] {
			let ZipImportOutcome::Report(report) = import_zip(dest.path(), &bytes, &opts).unwrap() else {
				panic!("expected report")
			};
			assert!(report.danglings_unresolved.is_empty());
			assert_eq!(report.file_mappings.len(), 3);
			for name in ["composition-module", "composition-parent", "composition-target"] {
				let mapping = report.file_mappings.iter().find(|entry| entry.original_fq.as_deref() == Some(name)).unwrap();
				assert_eq!(mapping.fq, format!("added/{name}{suffix}"));
				assert!(report.written_files.contains(&mapping.path));
			}
			let parent_path = dest.path().join(format!("added/composition-parent{suffix}.flowgraph.toml"));
			let parent = parse_flowgraph_file(&fs::read_to_string(parent_path).unwrap(), None).unwrap();
			assert_eq!(parent.instances[0].graph, format!("added/composition-module{suffix}"));
			assert!(parent
				.edges
				.iter()
				.any(|edge| edge.to == format!("added/composition-target{suffix}::sink:exec_in")));
			assert!(parent.edges.iter().any(|edge| edge.to == "child:exec_in"));
			let loaded = crate::flowgraph::loader::dir::load_flowgraph_dir(dest.path())
				.unwrap_or_else(|error| panic!("imported composition must load independently: {:?}", error.diagnostics));
			assert_eq!(loaded.composition.instances.len(), if suffix.is_empty() { 1 } else { 2 });
			let ids = loaded.program.node_ids().cloned().collect::<std::collections::HashSet<_>>();
			assert!(ids.contains(&format!("added/composition-parent{suffix}/@child::log")));
		}
		// The second import must not mutate the first import's references.
		let first = fs::read_to_string(dest.path().join("added/composition-parent.flowgraph.toml")).unwrap();
		assert!(!first.contains("_1"));
	}

	#[test]
	fn export_import_roundtrip() {
		let src = mk_dir();
		write_file(
			&src.path().join("tts/main.flowgraph.toml"),
			r#"
[meta]
title = "Main"

[[nodes]]
id = "a"
feature = "flowgraph.literal.string"
[nodes.properties]
value = "x"

[[nodes]]
id = "b"
feature = "flowgraph.log.info"

[[edges]]
from = "a:value"
to   = "b:message"
"#,
		);
		write_file(
			&src.path().join("tts/jp.flowgraph.toml"),
			r#"
[[nodes]]
id = "c"
feature = "flowgraph.literal.string"
[nodes.properties]
value = "y"
"#,
		);

		let req = CopyRequest {
			scope: FragmentScope::Folder,
			targets: vec![CopyTarget::Folder { path: "tts".into() }],
			origin: Some("tests".into()),
		};
		let bytes = export_zip(src.path(), &req).unwrap();
		assert!(!bytes.is_empty());

		// zip 内に manifest と 2 ファイルがある。
		let mut ar = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
		let mut names: Vec<String> = (0..ar.len()).map(|i| ar.by_index(i).unwrap().name().to_string()).collect();
		names.sort();
		assert!(names.iter().any(|n| n == MANIFEST_NAME));
		assert!(names.iter().any(|n| n == "flowgraph/tts/main.flowgraph.toml"));
		assert!(names.iter().any(|n| n == "flowgraph/tts/jp.flowgraph.toml"));

		// dry_run preview
		let dest = mk_dir();
		let opts = ZipImportOptions {
			dry_run: true,
			target_prefix: "imported".into(),
			..Default::default()
		};
		let outcome = import_zip(dest.path(), &bytes, &opts).unwrap();
		let preview = match outcome {
			ZipImportOutcome::Preview(p) => p,
			_ => panic!("dry_run should return preview"),
		};
		assert_eq!(preview.file_count, 2);
		assert_eq!(preview.target_prefix, "imported");
		assert!(preview.conflicts.is_empty());
		assert_eq!(preview.manifest.scope, FragmentScope::Folder);

		// 本番 import
		let opts2 = ZipImportOptions {
			dry_run: false,
			target_prefix: "imported".into(),
			..Default::default()
		};
		let outcome = import_zip(dest.path(), &bytes, &opts2).unwrap();
		let rep = match outcome {
			ZipImportOutcome::Report(r) => r,
			_ => panic!("non-dry_run should return report"),
		};
		assert_eq!(rep.written_files.len(), 2);
		assert!(dest.path().join("imported/tts/main.flowgraph.toml").exists());
		assert!(dest.path().join("imported/tts/jp.flowgraph.toml").exists());
	}

	#[test]
	fn import_rejects_deny_extension() {
		// 人工的に `.exe` を含む ZIP を組み立てる。
		let mut buf: Vec<u8> = Vec::new();
		{
			let cursor = Cursor::new(&mut buf);
			let mut zw = zip::ZipWriter::new(cursor);
			let opts = zip::write::SimpleFileOptions::default();
			// manifest は最低限
			let frag = Fragment {
				header: FragmentHeader {
					schema: crate::flowgraph::fragment::FRAGMENT_SCHEMA_V1.into(),
					exported_at: "2026-04-17T00:00:00Z".into(),
					origin: None,
					scope: FragmentScope::File,
				},
				files: vec![],
				danglings: vec![],
			};
			zw.start_file(MANIFEST_NAME, opts).unwrap();
			zw.write_all(serialize_fragment(&frag).unwrap().as_bytes()).unwrap();
			zw.start_file("flowgraph/evil.exe", opts).unwrap();
			zw.write_all(b"MZ...").unwrap();
			zw.finish().unwrap();
		}

		let dest = mk_dir();
		let opts = ZipImportOptions {
			dry_run: true,
			..Default::default()
		};
		let err = import_zip(dest.path(), &buf, &opts).unwrap_err();
		assert!(matches!(err, ZipImportError::UnsafeEntry(_)));
	}

	#[test]
	fn import_rejects_parent_traversal() {
		let mut buf: Vec<u8> = Vec::new();
		{
			let cursor = Cursor::new(&mut buf);
			let mut zw = zip::ZipWriter::new(cursor);
			let opts = zip::write::SimpleFileOptions::default();
			let frag = Fragment {
				header: FragmentHeader {
					schema: crate::flowgraph::fragment::FRAGMENT_SCHEMA_V1.into(),
					exported_at: "2026-04-17T00:00:00Z".into(),
					origin: None,
					scope: FragmentScope::File,
				},
				files: vec![],
				danglings: vec![],
			};
			zw.start_file(MANIFEST_NAME, opts).unwrap();
			zw.write_all(serialize_fragment(&frag).unwrap().as_bytes()).unwrap();
			zw.start_file("flowgraph/../evil.flowgraph.toml", opts).unwrap();
			zw.write_all(b"").unwrap();
			zw.finish().unwrap();
		}
		let dest = mk_dir();
		let opts = ZipImportOptions::default();
		let err = import_zip(dest.path(), &buf, &opts).unwrap_err();
		assert!(matches!(err, ZipImportError::UnsafeEntry(_)));
	}

	#[test]
	fn import_missing_manifest_errors() {
		let mut buf: Vec<u8> = Vec::new();
		{
			let cursor = Cursor::new(&mut buf);
			let mut zw = zip::ZipWriter::new(cursor);
			zw.start_file("flowgraph/nope.flowgraph.toml", zip::write::SimpleFileOptions::default())
				.unwrap();
			zw.write_all(b"").unwrap();
			zw.finish().unwrap();
		}
		let dest = mk_dir();
		let err = import_zip(dest.path(), &buf, &ZipImportOptions::default()).unwrap_err();
		assert!(matches!(err, ZipImportError::ManifestMissing));
	}

	#[test]
	fn companion_file_roundtrips() {
		let src = mk_dir();
		write_file(
			&src.path().join("main.flowgraph.toml"),
			r#"
[[nodes]]
id = "a"
feature = "flowgraph.literal.string"
[nodes.properties]
value = "x"
"#,
		);
		let req = CopyRequest {
			scope: FragmentScope::File,
			targets: vec![CopyTarget::File { fq: "main".into() }],
			origin: None,
		};
		let mut bytes = export_zip(src.path(), &req).unwrap();
		// 手動で companion として README.md を追加する（VAC export は current 実装では付けない）。
		{
			// 既存 zip を読み直して書き足すのが面倒なので新規作成。
			let mut buf: Vec<u8> = Vec::new();
			{
				let cursor = Cursor::new(&mut buf);
				let mut zw = zip::ZipWriter::new(cursor);
				// 既存エントリを写経
				let mut ar = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
				for i in 0..ar.len() {
					let mut e = ar.by_index(i).unwrap();
					let name = e.name().to_string();
					zw.start_file(&name, zip::write::SimpleFileOptions::default()).unwrap();
					let mut s = Vec::new();
					e.read_to_end(&mut s).unwrap();
					zw.write_all(&s).unwrap();
				}
				zw.start_file("flowgraph/README.md", zip::write::SimpleFileOptions::default())
					.unwrap();
				zw.write_all(b"# hi").unwrap();
				zw.finish().unwrap();
			}
			bytes = buf;
		}

		let dest = mk_dir();
		let outcome = import_zip(
			dest.path(),
			&bytes,
			&ZipImportOptions {
				dry_run: false,
				target_prefix: "imp".into(),
				..Default::default()
			},
		)
		.unwrap();
		let rep = match outcome {
			ZipImportOutcome::Report(r) => r,
			_ => panic!(),
		};
        struct RejectCheckpoint;
        impl super::super::paste::WriteObserver for RejectCheckpoint {
            fn before_write(&self,_:&Path,_:Option<&[u8]>,_:&[u8],_:Option<&str>)->std::io::Result<()>{Ok(())}
            fn graphs_completed(&self,_:&super::super::paste::PasteReport)->std::io::Result<()>{Err(std::io::Error::other("checkpoint unavailable"))}
        }
        let checkpoint_dest=mk_dir();
        let failed=import_zip_observed(checkpoint_dest.path(),&bytes,&ZipImportOptions{dry_run:false,target_prefix:"imp".into(),..Default::default()},Some(&RejectCheckpoint));
        assert!(matches!(failed,Err(ZipImportError::Paste(PasteError::PartialWrite{..}))));
        assert!(checkpoint_dest.path().join("imp/main.flowgraph.toml").exists());
        assert!(!checkpoint_dest.path().join("imp/README.md").exists());
		assert!(rep.written_companions.iter().any(|p| p.ends_with("README.md")));
		assert!(dest.path().join("imp/README.md").exists());
        let conflict_dest = mk_dir();
        write_file(&conflict_dest.path().join("imp/README.md"), "existing user content");
        let options=ZipImportOptions {dry_run:true,target_prefix:"imp".into(),..Default::default()};
        let preview=import_zip(conflict_dest.path(),&bytes,&options).unwrap();
        let ZipImportOutcome::Preview(preview)=preview else {panic!("expected preview")};
        assert!(preview.conflicts.iter().any(|path|path=="imp/README.md"));
        let outcome=import_zip(conflict_dest.path(),&bytes,&ZipImportOptions{dry_run:false,..options});
        assert!(matches!(outcome,Err(ZipImportError::CompanionConflict(_))));
        assert_eq!(fs::read_to_string(conflict_dest.path().join("imp/README.md")).unwrap(),"existing user content");
        assert!(!conflict_dest.path().join("imp/main.flowgraph.toml").exists());

	}
}

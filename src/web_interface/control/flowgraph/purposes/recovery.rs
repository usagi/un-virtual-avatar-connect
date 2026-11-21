//! Durable registration intent after successful graph import; not a graph rollback journal.
use super::{invalid, Document, Purpose, Snapshot};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::Path};
const PREFIX: &str = ".flowgraph-purpose-import-";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    schema_version: u32,
    root: String,
    pub purpose: Purpose,
    graphs: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    companions: Option<BTreeMap<String, String>>,
}
#[derive(Debug, Clone, Serialize)]
pub struct RecoverySnapshot {
    pub id: String,
    pub revision: String,
    pub receipt: Receipt,
}
#[derive(Debug, Serialize)]
pub struct RecoveryEntry {
    pub id: String,
    pub snapshot: Option<RecoverySnapshot>,
    pub error: Option<String>,
}
fn path(root: &Path, id: &str) -> io::Result<std::path::PathBuf> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') { return Err(invalid("invalid recovery id")); }
    Ok(root.join(format!("{PREFIX}{id}.json")))
}
fn graph_digest(root: &Path, fq: &str) -> io::Result<String> {
    relative_digest(root, &format!("{fq}.flowgraph.toml"))
}
fn relative_digest(root: &Path, relative: &str) -> io::Result<String> {
    let mut file = root.to_path_buf();
    for part in relative.split('/') {
        if matches!(part, "" | "." | "..") || part.contains(['\\', ':']) { return Err(invalid("invalid recovery graph path")); }
        file.push(part);
        if std::fs::symlink_metadata(&file)?.file_type().is_symlink() { return Err(invalid("recovery graph must not use links")); }
    }
    if !file.canonicalize()?.starts_with(root.canonicalize()?) { return Err(invalid("recovery graph outside root")); }
    Ok(blake3::hash(&std::fs::read(file)?).to_hex().to_string())
}
pub(super) fn prepare(root: &Path, purpose: &Purpose, imported: &crate::flowgraph::fragment::zip_codec::ZipImportReport) -> io::Result<RecoverySnapshot> {
    prepare_inner(root,purpose,imported,false)
}
pub(super) fn prepare_idempotent(root:&Path,purpose:&Purpose,imported:&crate::flowgraph::fragment::zip_codec::ZipImportReport)->io::Result<RecoverySnapshot>{
    prepare_inner(root,purpose,imported,true)
}
fn prepare_inner(root:&Path,purpose:&Purpose,imported:&crate::flowgraph::fragment::zip_codec::ZipImportReport,allow_identical:bool)->io::Result<RecoverySnapshot>{
    let graphs = imported.file_mappings.iter().map(|file| {
        if graph_digest(root, &file.fq)? != file.content_digest {
            return Err(invalid("imported graph changed before recovery receipt; review before registering"));
        }
        Ok((file.fq.clone(), file.content_digest.clone()))
    }).collect::<io::Result<BTreeMap<_,_>>>()?;
    let companions = imported.companion_mappings.iter().map(|file| {
        // Import reports use forward slashes, including Windows verbatim prefixes.
        // Compare the same representation; then validate each relative component.
        let prefix = format!("{}/", root.to_string_lossy().replace('\\', "/").trim_end_matches('/'));
        let relative = file.path.strip_prefix(&prefix).ok_or_else(|| invalid("recovery material outside root"))?.to_string();
        if relative_digest(root, &relative)? != file.content_digest {
            return Err(invalid("imported material changed before recovery receipt; review before registering"));
        }
        Ok((relative, file.content_digest.clone()))
    }).collect::<io::Result<BTreeMap<_,_>>>()?;
    let receipt = Receipt {schema_version:2, root:super::super::graph_root_identity(root), purpose:purpose.clone(), graphs, companions:Some(companions)};
    let bytes = serde_json::to_vec_pretty(&receipt).map_err(|e| invalid(&e.to_string()))?;
    let id = purpose.id.clone();
    match super::super::file_replace::create(&path(root,&id)?, &bytes) {
        Ok(())=>(),
        Err(error) if allow_identical && error.kind()==io::ErrorKind::AlreadyExists=>{
            let existing=read_unlocked(root,&id)?;
            if serde_json::to_vec_pretty(&existing.receipt)?!=bytes {return Err(invalid("registration receipt differs from resumed import"));}
        },
        Err(error)=>return Err(error),
    }
    read_unlocked(root,&id)
}
fn read_unlocked(root: &Path, id: &str) -> io::Result<RecoverySnapshot> {
    let path = path(root,id)?;
    let meta = std::fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.file_type().is_symlink() { return Err(invalid("invalid recovery receipt file")); }
    let bytes = std::fs::read(path)?;
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|e| invalid(&e.to_string()))?;
    if !matches!(receipt.schema_version, 1 | 2) || (receipt.schema_version == 2 && receipt.companions.is_none()) || receipt.root != super::super::graph_root_identity(root) || receipt.purpose.id != id { return Err(invalid("recovery receipt identity changed")); }
    Document {schema_version:1,purposes:vec![receipt.purpose.clone()]}.validate()?;
    if !receipt.purpose.roots.iter().all(|root|receipt.graphs.contains_key(root)) { return Err(invalid("incomplete recovery graph inventory")); }
    Ok(RecoverySnapshot {id:id.into(),revision:blake3::hash(&bytes).to_hex().to_string(),receipt})
}
pub fn registration_recovery(root: &Path) -> io::Result<Vec<RecoveryEntry>> {
    let _guard = super::super::write_lock::acquire(root)?;
    let mut entries = vec![];
    for entry in std::fs::read_dir(root)? {
        let entry = entry?; let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_prefix(PREFIX).and_then(|s|s.strip_suffix(".json")) else {continue};
        entries.push(match read_unlocked(root,id) {
            Ok(snapshot) => RecoveryEntry {id:id.into(),snapshot:Some(snapshot),error:None},
            Err(error) => RecoveryEntry {id:id.into(),snapshot:None,error:Some(error.to_string())},
        });
    }
    entries.sort_by(|a,b|a.id.cmp(&b.id)); Ok(entries)
}
pub fn retry_registration(root: &Path, expected: &Snapshot, id: &str, revision: &str) -> io::Result<Snapshot> {
    if super::super::graph_root_identity(root) != expected.root { return Err(invalid("purpose root changed")); }
    let _guard = super::super::write_lock::acquire(root)?;
    let current = super::read_unlocked(root)?;
    if current.revision != expected.revision { return Err(invalid("purpose revision changed")); }
    let recovery = read_unlocked(root,id)?;
    if recovery.revision != revision { return Err(invalid("recovery receipt changed")); }
    if let Some(existing) = current.document.purposes.iter().find(|p|p.id == id) {
        if existing == &recovery.receipt.purpose { return Ok(current); }
        return Err(invalid("recovery registration identity is already in use"));
    }
    for (fq,digest) in &recovery.receipt.graphs {
        if graph_digest(root,fq)? != *digest { return Err(invalid("imported graphs changed; review before registering")); }
    }
    if let Some(companions) = &recovery.receipt.companions {
        for (relative, digest) in companions {
            if relative_digest(root, relative)? != *digest { return Err(invalid("imported materials changed; review before registering")); }
        }
    }
    let mut document = current.document.clone(); document.purposes.push(recovery.receipt.purpose);
    super::save_unlocked(root,&current,document)
}

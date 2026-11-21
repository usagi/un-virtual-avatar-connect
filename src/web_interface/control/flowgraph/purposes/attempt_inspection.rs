//! Read-only comparison of prepublication intents with current files.
use std::{io, path::Path};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema_version: u32,
    id: String,
    root: String,
    sequence: u64,
    path: String,
    source: Option<String>,
    before_digest: Option<String>,
    #[serde(default)]
    preimage: Option<Preimage>,
    content_digest: String,
    state: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preimage {
    path: String,
    content_digest: String,
    size: u64,
}
fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
fn digest_valid(value: &str) -> bool { value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()) }
pub(super) fn current_digest(root: &Path, relative: &str) -> io::Result<Option<String>> {
    let mut path = root.to_path_buf();
    let components:Vec<_> = relative.split('/').collect();
    for (index,component) in components.iter().enumerate() {
        path.push(component);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(invalid("linked destination cannot be inspected")),
            Ok(meta) if index + 1 < components.len() && !meta.is_dir() => return Err(invalid("destination parent is not a directory")),
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        }
    }
    if !path.canonicalize()?.starts_with(root.canonicalize()?) { return Err(invalid("destination outside root")); }
    let file = std::fs::File::open(&path)?;
    if !file.metadata()?.is_file() { return Err(invalid("destination is not a file")); }
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(file)?;
    Ok(Some(hasher.finalize().to_hex().to_string()))
}
fn inspect_one(root: &Path, path: &Path, id: &str, sequence: u64) -> io::Result<Value> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 1024 * 1024 { return Err(invalid("invalid write intent file")); }
    let bytes = std::fs::read(path)?;
    let encoded: Value = serde_json::from_slice(&bytes)?;
    if encoded.get("before_digest").is_none() || encoded.get("source").is_none() { return Err(invalid("incomplete write intent")); }
    let intent: Intent = serde_json::from_value(encoded)?;
    if intent.schema_version != 1 || intent.id != id || intent.sequence != sequence || intent.state != "planned"
        || intent.root != super::super::graph_root_identity(root)
        || !digest_valid(&intent.content_digest) || intent.before_digest.as_deref().is_some_and(|digest| !digest_valid(digest))
        || intent.path.split('/').any(|part| matches!(part, ""|"."|"..") || part.contains(['\\', ':', '\0'])) {
        return Err(invalid("invalid write intent identity or content"));
    }
    let preimage = if let Some(saved) = &intent.preimage {
        if saved.path != format!(".flowgraph-purpose-transfer-{id}.before-{sequence:08}.bin")
            || Some(saved.content_digest.as_str()) != intent.before_digest.as_deref() {
            return Err(invalid("invalid preimage identity"));
        }
        let observation = match current_digest(root,&saved.path) {
            Ok(None) => "missing".to_owned(),
            Ok(Some(digest)) if digest == saved.content_digest
                && std::fs::metadata(root.join(&saved.path))?.len() == saved.size => "verified".to_owned(),
            Ok(Some(_)) => "changed".to_owned(),
            Err(_) => "unreadable".to_owned(),
        };
        json!({"path":saved.path,"content_digest":saved.content_digest,"size":saved.size,"observation":observation})
    } else { Value::Null };
    let current = match current_digest(root, &intent.path) {
        Ok(current) => current,
        Err(error) => return Ok(json!({"sequence":sequence,"path":intent.path,"source":intent.source,
            "observation":"unreadable","error":error.to_string(),"intent_revision":blake3::hash(&bytes).to_hex().to_string()})),
    };
    let state = match current.as_deref() {
        Some(digest) if digest == intent.content_digest => "matches_intended",
        Some(digest) if Some(digest) == intent.before_digest.as_deref() => "matches_previous",
        Some(_) => "changed",
        None if intent.before_digest.is_none() => "not_created",
        None => "missing",
    };
    Ok(json!({"sequence":sequence,"path":intent.path,"source":intent.source,"observation":state,
        "before_digest":intent.before_digest,"content_digest":intent.content_digest,
        "intent_revision":blake3::hash(&bytes).to_hex().to_string(),"current_digest":current,"preimage":preimage}))
}
pub(super) fn inspect(root: &Path, id: &str) -> io::Result<Vec<Value>> {
    let prefix = format!(".flowgraph-purpose-transfer-{id}.write-");
    let mut records = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(number) = name.strip_prefix(&prefix).and_then(|s| s.strip_suffix(".json")) else { continue };
        if records.len() >= 8192 { return Err(invalid("too many write intents")); }
        let row = match number.parse::<u64>() {
            Ok(sequence) if number == format!("{sequence:08}") => inspect_one(root, &entry.path(), id, sequence)
                .unwrap_or_else(|error| json!({"sequence":sequence,"observation":"unreadable","error":error.to_string()})),
            _ => json!({"observation":"unreadable","error":"invalid write intent sequence"}),
        };
        records.push(row);
    }
    records.sort_by_key(|row| row["sequence"].as_u64().unwrap_or(u64::MAX));
    Ok(records)
}

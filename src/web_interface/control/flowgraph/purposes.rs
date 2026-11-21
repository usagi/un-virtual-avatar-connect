//! Experimental named selections, not graph ownership or activation state.
//! Metadata and journaled removal/recovery routes are available; normal UI adoption remains separate.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, io, path::Path};

mod recovery;
pub mod api;
mod transfer;
mod attempt;
mod attempt_inspection;
mod graph_plan;
mod transfer_wire;
mod resume;
mod removal;
mod removal_recovery;
pub use removal_recovery::{prepare_removal_recovery, remove_purpose_files, restore_removal, removal_recovery_history, RemovalRecovery, RemovalOutcome};
pub use removal::removal_plan;
#[cfg(test)]
mod browser_review;
pub use recovery::{registration_recovery, retry_registration, RecoverySnapshot};

const FILE: &str = ".flowgraph-purposes.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Purpose {
    pub id: String,
    pub name: String,
    /// Explicitly selected graph FQs. Dependencies are resolved afresh, never owned.
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub purposes: Vec<Purpose>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub root: String,
    pub revision: Option<String>,
    pub document: Document,
}

fn invalid(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }

impl Document {
    fn validate(&self) -> io::Result<()> {
        if self.schema_version != 1 { return Err(invalid("unsupported purpose schema")); }
        let mut ids = BTreeSet::new();
        for purpose in &self.purposes {
            if purpose.id.is_empty() || !purpose.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') || !ids.insert(&purpose.id) {
                return Err(invalid("invalid or duplicate purpose id"));
            }
            if purpose.name.trim().is_empty() || purpose.roots.is_empty() { return Err(invalid("purpose needs name and roots")); }
            let mut roots = BTreeSet::new();
            for fq in &purpose.roots {
                if fq.contains(['\\', ':']) || fq.chars().any(char::is_control) || fq.split('/').any(|part| matches!(part, "" | "." | "..")) || !roots.insert(fq) {
                    return Err(invalid("invalid or duplicate graph reference"));
                }
            }
        }
        Ok(())
    }
}

fn read_unlocked(root: &Path) -> io::Result<Snapshot> {
    let path = root.join(FILE);
    // This metadata file must belong to this graph root; do not follow a link
    // to another profile, including a dangling link that looks like absence.
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => return Err(invalid("purpose metadata must not be a symbolic link")),
        Ok(meta) if !meta.is_file() => return Err(invalid("purpose metadata is not a file")),
        Ok(_) => {},
        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
        Err(error) => return Err(error),
    }
    let raw = match std::fs::read(&path) {
        Ok(raw) => Some(raw),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let document: Document = match &raw {
        Some(raw) => serde_json::from_slice(raw).map_err(|e| invalid(&e.to_string()))?,
        None => Document { schema_version: 1, purposes: vec![] },
    };
    document.validate()?;
    Ok(Snapshot { root: super::graph_root_identity(root), revision: raw.map(|r| blake3::hash(&r).to_hex().to_string()), document })
}

pub fn read(root: &Path) -> io::Result<Snapshot> {
    let _guard = super::write_lock::acquire(root)?;
    read_unlocked(root)
}

#[derive(Debug, Serialize)]
pub struct HistoryEntry {
    pub backup: String,
    pub document: Option<Document>,
    pub error: Option<String>,
}

/// Read only this metadata's local backups, never arbitrary client paths.
pub fn history(root: &Path) -> io::Result<Vec<HistoryEntry>> {
    let _guard = super::write_lock::acquire(root)?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&format!("{FILE}.bak-")) { continue; }
        let result = (|| -> io::Result<Document> {
            if !entry.file_type()?.is_file() { return Err(invalid("backup is not a regular file")); }
            let document: Document = serde_json::from_slice(&std::fs::read(entry.path())?).map_err(|e| invalid(&e.to_string()))?;
            document.validate()?;
            Ok(document)
        })();
        entries.push(match result {
            Ok(document) => HistoryEntry { backup: name, document: Some(document), error: None },
            Err(error) => HistoryEntry { backup: name, document: None, error: Some(error.to_string()) },
        });
    }
    entries.sort_by(|a,b| b.backup.cmp(&a.backup));
    Ok(entries)
}

/// Save only the metadata. Removing a purpose never removes its graphs.
pub fn save(root: &Path, expected: &Snapshot, document: Document) -> io::Result<Snapshot> {
    document.validate()?;
    if super::graph_root_identity(root) != expected.root { return Err(invalid("purpose root changed")); }
    let _guard = super::write_lock::acquire(root)?;
    save_unlocked(root, expected, document)
}

fn save_unlocked(root: &Path, expected: &Snapshot, document: Document) -> io::Result<Snapshot> {
    document.validate()?;
    let current = read_unlocked(root)?;
    if current.revision != expected.revision { return Err(invalid("purpose revision changed")); }
    if current.document == document { return Ok(current); }
    let bytes = serde_json::to_vec_pretty(&document).map_err(|e| invalid(&e.to_string()))?;
    let path = root.join(FILE);
    if current.revision.is_some() {
        super::util::make_backup(&path)?;
        super::file_replace::replace(&path, &bytes)?;
    } else { super::file_replace::create(&path, &bytes)?; }
    read_unlocked(root)
}

/// Native isolated integration only. No runtime reload or normal HTTP route.
#[derive(Debug, Serialize)]
pub struct RegisteredImport {
    pub recovery: Option<RecoverySnapshot>,
    pub imported: crate::flowgraph::fragment::zip_codec::ZipImportReport,
    pub registration: Option<Purpose>,
    pub registration_error: Option<String>,
}

pub fn import_registered(
    root: &Path, expected: &Snapshot, archive: &[u8],
    options: &crate::flowgraph::fragment::zip_codec::ZipImportOptions,
) -> io::Result<RegisteredImport> {
    import_registered_with_save(root, expected, archive, options, save_unlocked)
}

fn import_registered_with_save(
    root: &Path, expected: &Snapshot, archive: &[u8],
    options: &crate::flowgraph::fragment::zip_codec::ZipImportOptions,
    publish: impl FnOnce(&Path, &Snapshot, Document) -> io::Result<Snapshot>,
) -> io::Result<RegisteredImport> {
    import_registered_observed_with_save(root, expected, archive, options, None, publish)
}
fn import_registered_observed(
    root: &Path, expected: &Snapshot, archive: &[u8],
    options: &crate::flowgraph::fragment::zip_codec::ZipImportOptions,
    observer: &dyn crate::flowgraph::fragment::paste::WriteObserver,
) -> io::Result<RegisteredImport> {
    import_registered_observed_with_save(root, expected, archive, options, Some(observer), save_unlocked)
}
fn import_registered_observed_with_save(
    root: &Path, expected: &Snapshot, archive: &[u8],
    options: &crate::flowgraph::fragment::zip_codec::ZipImportOptions,
    observer: Option<&dyn crate::flowgraph::fragment::paste::WriteObserver>,
    publish: impl FnOnce(&Path, &Snapshot, Document) -> io::Result<Snapshot>,
) -> io::Result<RegisteredImport> {
    use crate::flowgraph::fragment::zip_codec::{import_zip, ZipImportOutcome};
    if options.dry_run { return Err(invalid("registration requires a confirmed import")); }
    if super::graph_root_identity(root) != expected.root { return Err(invalid("purpose root changed")); }
    let _guard = super::write_lock::acquire(root)?;
    let current = read_unlocked(root)?;
    if current.revision != expected.revision { return Err(invalid("purpose revision changed")); }
    let mut preview_options = options.clone(); preview_options.dry_run = true;
    let ZipImportOutcome::Preview(preview) = import_zip(root, archive, &preview_options).map_err(|e| invalid(&e.to_string()))? else { unreachable!() };
    if preview.purpose.is_none() { return Err(invalid("archive has no portable purpose")); }
    let ZipImportOutcome::Report(imported) = crate::flowgraph::fragment::zip_codec::import_zip_observed(root, archive, options, observer).map_err(|e| io::Error::other(e))? else { unreachable!() };
    let mapped = imported.purpose.as_ref().expect("validated portable purpose");
    if !mapped.missing_roots.is_empty() || mapped.roots.is_empty() {
        return Ok(RegisteredImport { recovery: None, imported, registration: None, registration_error: Some("Some purpose roots were not imported; no registration was created".into()) });
    }
    let fixed_id=observer.and_then(|observer|observer.purpose_id());
    let mut id = fixed_id.clone().unwrap_or_else(||format!("imported-{:032x}", rand::random::<u128>()));
    if fixed_id.is_some() && current.document.purposes.iter().any(|p|p.id==id) { return Err(invalid("import registration id already exists")); }
    while current.document.purposes.iter().any(|p| p.id == id) { id = format!("imported-{:032x}", rand::random::<u128>()); }
    let purpose = Purpose { id, name: mapped.name.clone(), roots: mapped.roots.clone() };
    let recovery = match recovery::prepare(root, &purpose, &imported) {
        Ok(receipt) => receipt,
        Err(error) => return Ok(RegisteredImport {recovery:None,imported,registration:None,registration_error:Some(format!("Could not preserve registration recovery: {error}"))}),
    };
    let mut document = current.document.clone(); document.purposes.push(purpose.clone());
    match publish(root, &current, document) {
        Ok(_) => Ok(RegisteredImport { recovery: Some(recovery), imported, registration: Some(purpose), registration_error: None }),
        Err(error) => Ok(RegisteredImport { recovery: Some(recovery), imported, registration: None, registration_error: Some(error.to_string()) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_registration_retry() {
        let Some(root) = std::env::var_os("UNVAC_TEST_PURPOSE_RECOVERY_ROOT") else { return; };
        let root = std::path::PathBuf::from(root);
        let expected: Snapshot = serde_json::from_str(&std::env::var("UNVAC_TEST_PURPOSE_RECOVERY_EXPECTED").unwrap()).unwrap();
        let id = std::env::var("UNVAC_TEST_PURPOSE_RECOVERY_ID").unwrap();
        let revision = std::env::var("UNVAC_TEST_PURPOSE_RECOVERY_REVISION").unwrap();
        let records = registration_recovery(&root).unwrap();
        let record = records.iter().find(|record|record.id == id).unwrap().snapshot.as_ref().unwrap();
        assert_eq!(record.revision, revision);
        let saved = retry_registration(&root, &expected, &id, &revision).unwrap();
        assert!(saved.document.purposes.iter().any(|purpose|purpose.id == id));
    }

    fn retry_in_fresh_process(root: &Path, expected: &Snapshot, receipt: &RecoverySnapshot) {
        let name = format!("{}::child_registration_retry", module_path!().split_once("::").unwrap().1);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &name])
            .env("UNVAC_TEST_PURPOSE_RECOVERY_ROOT",root)
            .env("UNVAC_TEST_PURPOSE_RECOVERY_EXPECTED",serde_json::to_string(expected).unwrap())
            .env("UNVAC_TEST_PURPOSE_RECOVERY_ID",&receipt.id)
            .env("UNVAC_TEST_PURPOSE_RECOVERY_REVISION",&receipt.revision)
            .output().unwrap();
        assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    }

    #[test]
    fn imported_registration_preserves_existing_and_reports_partial_success() {
        use crate::flowgraph::fragment::{CopyRequest, CopyTarget, FragmentScope};
        use crate::flowgraph::fragment::zip_codec::{export_zip_with_purpose, PortablePurpose, ZipImportOptions};
        let root = std::env::temp_dir().join(format!("vac-purpose-import-{}-{}", std::process::id(), rand::random::<u64>()));
        let source = root.join("source"); let dest = root.join("dest");
        std::fs::create_dir_all(&source).unwrap(); std::fs::create_dir(&dest).unwrap();
        std::fs::write(source.join("main.flowgraph.toml"), "[meta]\ntitle='test'\n").unwrap();
        let request = CopyRequest {scope:FragmentScope::File,targets:vec![CopyTarget::File{fq:"main".into()}],origin:None};
        std::fs::write(source.join("README.md"), b"imported material").unwrap();
        let archive = export_zip_with_purpose(&source, &request, &["README.md".into()], Some(&PortablePurpose {schema_version:1,name:"同じ用途名".into(),roots:vec!["main".into()]})).unwrap();
        let empty = read(&dest).unwrap();
        let original = Purpose {id:"original".into(),name:"同じ用途名".into(),roots:vec!["existing".into()]};
        let current = save(&dest, &empty, Document {schema_version:1,purposes:vec![original.clone()]}).unwrap();
        let options = ZipImportOptions {target_prefix:"received".into(),..Default::default()};
        assert!(import_registered(&dest, &empty, &archive, &options).is_err());
        assert!(!dest.join("received").exists());
        let report = import_registered(&dest, &current, &archive, &options).unwrap();
        let created = report.registration.unwrap();
        assert_ne!(created.id, original.id); assert_eq!(created.roots, vec!["received/main"]);
        assert!(report.registration_error.is_none());
        // A receipt must retain the import's bytes, not bless an intervening edit.
        let receipt_candidate = Purpose { id:"changed-before-receipt".into(), ..created.clone() };
        let imported_path = dest.join("received/main.flowgraph.toml");
        let imported_bytes = std::fs::read(&imported_path).unwrap();
        std::fs::write(&imported_path, b"# edited before receipt").unwrap();
        let rejected = recovery::prepare(&dest, &receipt_candidate, &report.imported).unwrap_err();
        assert!(rejected.to_string().contains("changed before recovery receipt"));
        assert!(!dest.join(".flowgraph-purpose-import-changed-before-receipt.json").exists());
        assert_eq!(std::fs::read(&imported_path).unwrap(), b"# edited before receipt");
        std::fs::write(&imported_path, &imported_bytes).unwrap();

        let saved = read(&dest).unwrap(); assert_eq!(saved.document.purposes, vec![original.clone(),created]);
        let mut failure_options = options.clone(); failure_options.target_prefix = "second".into();
        let failed = import_registered_with_save(&dest, &saved, &archive, &failure_options, |_,_,_| Err(io::Error::other("injected publication failure"))).unwrap();
        assert!(failed.registration.is_none()); assert!(failed.registration_error.unwrap().contains("injected"));
        assert_eq!(failed.imported.written_files.len(), 1);
        assert!(dest.join("second/main.flowgraph.toml").exists());
        assert_eq!(read(&dest).unwrap(), saved);
        let mut skip = options.clone(); skip.paste.on_conflict_file = crate::flowgraph::fragment::paste::OnConflictFile::Skip;
        let graph_only = export_zip_with_purpose(&source, &request, &[], Some(&PortablePurpose {schema_version:1,name:"同じ用途名".into(),roots:vec!["main".into()]})).unwrap();
        let skipped = import_registered(&dest, &saved, &graph_only, &skip).unwrap();
        assert!(skipped.registration.is_none()); assert!(skipped.registration_error.is_some());
        assert_eq!(read(&dest).unwrap(), saved);
        let receipt = failed.recovery.unwrap();
        let reopened = registration_recovery(&dest).unwrap().into_iter().find(|entry|entry.id == receipt.id).unwrap().snapshot.unwrap();
        assert_eq!(reopened.revision, receipt.revision);
        assert!(retry_registration(&dest, &saved, &receipt.id, "stale receipt").is_err());
        let imported_file = dest.join("second/main.flowgraph.toml");
        let original_bytes = std::fs::read(&imported_file).unwrap();
        std::fs::write(&imported_file, "# edited after import").unwrap();
        assert!(retry_registration(&dest, &saved, &receipt.id, &receipt.revision).is_err());
        assert_eq!(read(&dest).unwrap(), saved);
        std::fs::write(&imported_file, &original_bytes).unwrap();
        let material_path = dest.join("second/README.md");
        std::fs::write(&material_path, b"edited material").unwrap();
        assert!(retry_registration(&dest, &saved, &receipt.id, &receipt.revision).unwrap_err().to_string().contains("materials changed"));
        assert_eq!(read(&dest).unwrap(), saved);
        assert_eq!(std::fs::read(&material_path).unwrap(), b"edited material");
        std::fs::write(&material_path, b"imported material").unwrap();
        retry_in_fresh_process(&dest, &saved, &receipt);
        let recovered = read(&dest).unwrap();
        assert_eq!(recovered.document.purposes.len(), 3);
        assert_eq!(std::fs::read(&imported_file).unwrap(), original_bytes);
        assert!(retry_registration(&dest, &saved, &receipt.id, &receipt.revision).is_err());
        retry_in_fresh_process(&dest, &recovered, &receipt);
        assert_eq!(read(&dest).unwrap(), recovered);
        assert!(retry_registration(&dest, &recovered, "../escape", &receipt.revision).is_err());
        std::fs::write(dest.join(".flowgraph-purpose-import-corrupt.json"), "broken").unwrap();
        assert!(registration_recovery(&dest).unwrap().iter().any(|entry|entry.id == "corrupt" && entry.error.is_some() && entry.snapshot.is_none()));
        let other = root.join("other"); std::fs::create_dir(&other).unwrap();
        let receipt_name = format!(".flowgraph-purpose-import-{}.json", receipt.id);
        std::fs::copy(dest.join(&receipt_name), other.join(&receipt_name)).unwrap();
        assert!(registration_recovery(&other).unwrap()[0].error.is_some());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_or_newer_metadata_is_not_replaced_by_an_empty_document() {
        let root = std::env::temp_dir().join(format!("vac-purpose-invalid-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let empty = read(&root).unwrap();
        for raw in ["broken JSON", r#"{"schema_version":2,"purposes":[]}"#, r#"{"schema_version":1,"purposes":[],"future_field":true}"#] {
            std::fs::write(root.join(FILE), raw).unwrap();
            assert!(read(&root).is_err());
            assert!(save(&root, &empty, empty.document.clone()).is_err());
            assert_eq!(std::fs::read_to_string(root.join(FILE)).unwrap(), raw);
            assert!(!std::fs::read_dir(&root).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains(".bak-")));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn named_selection_survives_reopen_conflicts_and_metadata_removal() {
        let root = std::env::temp_dir().join(format!("vac-purpose-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("main.flowgraph.toml"), "[meta]\ntitle='original'\n").unwrap();
        let empty = read(&root).unwrap();
        let document = Document { schema_version: 1, purposes: vec![Purpose { id: "stream-tools".into(), name: "配信支援".into(), roots: vec!["main".into(), "missing-but-retained/main".into()] }] };
        let saved = save(&root, &empty, document.clone()).unwrap();
        assert_eq!(read(&root).unwrap(), saved);
        assert!(save(&root, &empty, document.clone()).is_err());
        let mut other = saved.clone(); other.root.push_str("/other");
        assert!(save(&root, &other, document).is_err());
        let removed = save(&root, &saved, empty.document).unwrap();
        assert!(removed.document.purposes.is_empty());
        assert_eq!(std::fs::read_to_string(root.join("main.flowgraph.toml")).unwrap(), "[meta]\ntitle='original'\n");
        let backup = std::fs::read_dir(&root).unwrap().map(|e| e.unwrap().path()).find(|p| p.file_name().unwrap().to_string_lossy().contains(".bak-")).unwrap();
        let historical: Document = serde_json::from_slice(&std::fs::read(backup).unwrap()).unwrap();
        let listed = history(&root).unwrap();
        assert!(listed.iter().any(|entry| entry.document.as_ref() == Some(&saved.document)));
        std::fs::write(root.join(format!("{FILE}.bak-corrupt")), "broken").unwrap();
        assert!(history(&root).unwrap().iter().any(|entry| entry.backup.ends_with("corrupt") && entry.error.is_some() && entry.document.is_none()));
        assert_eq!(save(&root, &removed, historical).unwrap().document, saved.document);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_ambiguous_or_unsafe_references_and_future_schema() {
        let mut doc = Document { schema_version: 1, purposes: vec![Purpose { id: "one".into(), name: "用途".into(), roots: vec!["main".into()] }] };
        for fq in ["../main", "/main", "C:/main", "a\\b", "a//b", "a/./b"] {
            doc.purposes[0].roots = vec![fq.into()]; assert!(doc.validate().is_err());
        }
        doc.purposes[0].roots = vec!["main".into(), "main".into()]; assert!(doc.validate().is_err());
        doc.purposes[0].roots = vec!["main".into()]; doc.schema_version = 2; assert!(doc.validate().is_err());
    }
}

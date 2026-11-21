//! File integrity of build outputs, not a product release approval.
use crate::{sha256_file, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::{Component, Path}};

pub const FILE: &str = "unvac-release-files.json";
#[derive(Serialize, Deserialize)]
struct Entry { bytes: u64, sha256: String }
#[derive(Serialize, Deserialize)]
struct Inventory { schema_version: u32, variant: String, files: BTreeMap<String, Entry> }

fn inspect(root: &Path, name: &str) -> Result<Entry> {
    let relative = Path::new(name);
    if relative.components().any(|c| !matches!(c, Component::Normal(_))) || name.is_empty() {
        return Err(format!("invalid release path: {name}").into());
    }
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(format!("release symlink is not supported: {name}").into());
        }
    }
    let metadata = fs::metadata(&path)?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!("release file missing or empty: {name}").into());
    }
    Ok(Entry { bytes: metadata.len(), sha256: sha256_file(&path)? })
}

pub fn write(root: &Path, variant: &str, names: &[String]) -> Result<()> {
    let mut files = BTreeMap::new();
    for name in names { files.insert(name.clone(), inspect(root, name)?); }
    let inventory = Inventory { schema_version: 1, variant: variant.into(), files };
    fs::write(root.join(FILE), serde_json::to_vec_pretty(&inventory)?)?;
    Ok(())
}

pub fn verify(root: &Path, expected: impl FnOnce(&str) -> Result<Vec<String>>) -> Result<usize> {
    let inventory: Inventory = serde_json::from_slice(&fs::read(root.join(FILE))?)?;
    if inventory.schema_version != 1 { return Err("unsupported release inventory schema".into()); }
    let expected: std::collections::BTreeSet<_> = expected(&inventory.variant)?.into_iter().collect();
    if inventory.files.keys().cloned().collect::<std::collections::BTreeSet<_>>() != expected {
        return Err("release inventory does not contain the required file set".into());
    }
    for (name, recorded) in &inventory.files {
        let actual = inspect(root, name)?;
        if actual.bytes != recorded.bytes || actual.sha256 != recorded.sha256 {
            return Err(format!("release file changed: {name}").into());
        }
    }
    Ok(inventory.files.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_changed_missing_empty_and_omitted_files() {
        let root = std::env::temp_dir().join(format!("vac-release-inventory-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let names = vec!["app.exe".to_owned(), "worker.exe".to_owned()];
        for name in &names { fs::write(root.join(name), b"original").unwrap(); }
        write(&root, "portable", &names).unwrap();
        let check = || verify(&root, |_| Ok(names.clone()));
        assert_eq!(check().unwrap(), 2);
        fs::write(root.join("worker.exe"), b"modified").unwrap();
        assert!(check().is_err());
        fs::write(root.join("worker.exe"), []).unwrap();
        assert!(check().is_err());
        fs::remove_file(root.join("worker.exe")).unwrap();
        assert!(check().is_err());
        write(&root, "portable", &names[..1]).unwrap();
        assert!(check().is_err());
        assert!(inspect(&root, "../outside").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

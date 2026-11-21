//! Source comparison performed while the caller holds the root write lock.
use std::{io, path::Path};

#[derive(Debug)]
pub(super) enum RevisionError {
    Stale,
    Read(io::Error),
}

pub(super) fn check(path: &Path, expected: Option<&str>) -> Result<(), RevisionError> {
    let Some(expected) = expected else { return Ok(()); };
    let current = std::fs::read(path).map_err(RevisionError::Read)?;
    if current == expected.as_bytes() { Ok(()) } else { Err(RevisionError::Stale) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_source_is_rejected_without_touching_saved_bytes() {
        let root = std::env::temp_dir().join(format!("unvac-revision-test-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("main.flowgraph.toml");
        let original = "[meta]\r\ntitle = '初期'\r\n";
        let first = "[meta]\r\ntitle = '保存A'\r\n";
        std::fs::write(&path, original).unwrap();
        {
            let _guard = super::super::write_lock::acquire(&root).unwrap();
            check(&path, Some(original)).unwrap();
            std::fs::write(&path, first).unwrap();
        }
        {
            let _guard = super::super::write_lock::acquire(&root).unwrap();
            assert!(matches!(check(&path, Some(original)), Err(RevisionError::Stale)));
            assert_eq!(std::fs::read(&path).unwrap(), first.as_bytes());
            check(&path, Some(first)).unwrap();
            assert!(matches!(check(&path, Some(&first.replace("\r\n", "\n"))), Err(RevisionError::Stale)));
            check(&path, None).unwrap(); // legacy API contract remains explicit
        }
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(check(&path, Some(first)), Err(RevisionError::Read(error)) if error.kind() == io::ErrorKind::NotFound));
        std::fs::remove_file(root.join(".flowgraph-write.lock")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

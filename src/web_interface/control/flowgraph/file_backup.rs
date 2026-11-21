//! Preserve every backup, including repeated saves within the same clock tick.
use std::{io, path::Path};

pub(super) fn create(file: &Path, timestamp: &str) -> io::Result<String> {
    let name = file.file_name().and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid backup source name"))?;
    let source = std::fs::read(file)?;
    for suffix in 0..10_000 {
        let backup_name = if suffix == 0 { format!("{name}.bak-{timestamp}") }
            else { format!("{name}.bak-{timestamp}-{suffix}") };
        let path = file.with_file_name(&backup_name);
        match super::file_replace::create(&path, &source) {
            Ok(()) => {
                return Ok(backup_name);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "backup name collision limit reached"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_timestamp_keeps_each_previous_version() {
        let root = std::env::temp_dir().join(format!("unvac-backup-test-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("main.flowgraph.toml");
        let mut names = Vec::new();
        for version in ["first", "second", "third"] {
            std::fs::write(&path, version).unwrap();
            names.push(create(&path, "20261017-120000").unwrap());
        }
        assert_eq!(names.iter().collect::<std::collections::HashSet<_>>().len(), 3);
        for (name, version) in names.iter().zip(["first", "second", "third"]) {
            assert_eq!(std::fs::read_to_string(root.join(name)).unwrap(), version);
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "third");
        assert_eq!(create(&root.join("missing"), "20261017-120000").unwrap_err().kind(), io::ErrorKind::NotFound);
        for name in names { std::fs::remove_file(root.join(name)).unwrap(); }
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

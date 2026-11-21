//! Publish a complete sibling file without truncating the current document.
//! Callers own operation serialization and revision/backup checks.
use std::{fs::{File, OpenOptions}, io::{self, Write}, path::Path};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) fn replace(path: &Path, content: &[u8]) -> io::Result<()> {
    replace_with(path, |file| file.write_all(content))
}

pub(crate) async fn replace_async(path: std::path::PathBuf, content: Vec<u8>) -> io::Result<()> {
    replace_checked_async(path, content, None).await.map_err(io::Error::other)
}

pub(crate) fn revision(content: &[u8]) -> String {
    blake3::hash(content).to_hex().to_string()
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CheckedWriteError {
    #[error("file changed since it was read; reload before saving")]
    Conflict,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Cooperative exclusion for Flowgraph data writers. Keep the lock file: removing
/// it can split lock identity between waiting/open handles in separate processes.
pub(crate) fn data_write_lock(path: &Path) -> io::Result<File> {
    let parent = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    let parent = if parent.as_os_str().is_empty() { Path::new(".") } else { parent };
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(parent.join(".vac-data-write.lock"))?;
    lock.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => io::Error::from(io::ErrorKind::WouldBlock),
        std::fs::TryLockError::Error(error) => error,
    })?;
    Ok(lock)
}

fn replace_checked(path: &Path, content: &[u8], expected: Option<&str>) -> Result<(), CheckedWriteError> {
    let _lock = data_write_lock(path)?;
    if let Some(expected) = expected {
        let current = match std::fs::read(path) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(CheckedWriteError::Conflict),
            Err(error) => return Err(error.into()),
        };
        if revision(&current) != expected { return Err(CheckedWriteError::Conflict); }
    }
    replace(path, content)?;
    Ok(())
}

pub(crate) async fn replace_checked_async(
    path: std::path::PathBuf, content: Vec<u8>, expected: Option<String>,
) -> Result<(), CheckedWriteError> {
    tokio::task::spawn_blocking(move || replace_checked(&path, &content, expected.as_deref()))
        .await.map_err(io::Error::other)?
}

pub(crate) fn create(path: &Path, content: &[u8]) -> io::Result<()> {
    publish_with(path, false, |file| file.write_all(content))
}

fn replace_with(path: &Path, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    publish_with(path, true, write)
}

fn publish_with(path: &Path, overwrite: bool, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing parent"))?;
    let (temporary, mut file) = loop {
        let candidate = parent.join(format!(".vac-save-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => break (candidate, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = write(&mut file).and_then(|()| file.sync_all());
    drop(file); // Windows requires releasing the staging handle before cleanup.
    let result = result.and_then(|()| if overwrite {
        std::fs::rename(&temporary, path)
    } else {
        // Linking publishes the fully written inode without replacing a winner.
        // No copy fallback: it would expose a partially written destination.
        std::fs::hard_link(&temporary, path)
    });
    if !overwrite || result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_writes_reject_stale_and_deleted_inputs_and_serialize_competitors() {
        let root = std::env::temp_dir().join(format!("vac-cas-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("dictionary.tsv");
        std::fs::write(&path, b"initial").unwrap();
        let expected = revision(b"initial");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let jobs: Vec<_> = [b"first".to_vec(), b"second".to_vec()].into_iter().map(|content| {
            let (path, expected, barrier) = (path.clone(), expected.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                replace_checked(&path, &content, Some(&expected))
            })
        }).collect();
        let results: Vec<_> = jobs.into_iter().map(|job| job.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(results.iter().filter_map(|r| r.as_ref().err()).all(|e|
            matches!(e, CheckedWriteError::Conflict) ||
            matches!(e, CheckedWriteError::Io(error) if error.kind() == io::ErrorKind::WouldBlock)));
        let winner = std::fs::read(&path).unwrap();
        assert!(winner == b"first" || winner == b"second");
        assert!(matches!(replace_checked(&path, b"stale", Some(&expected)), Err(CheckedWriteError::Conflict)));
        assert_eq!(std::fs::read(&path).unwrap(), winner);
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(replace_checked(&path, b"resurrected", Some(&expected)), Err(CheckedWriteError::Conflict)));
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1); // persistent coordination lock only
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn failed_staging_keeps_original_and_success_publishes_complete_content() {
        let root = std::env::temp_dir().join(format!("vac-save-test-{}-{}",std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("main.flowgraph.toml");
        std::fs::write(&path, b"original").unwrap();
        let error = replace_with(&path, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected write failure"))
        }).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(),1);
        let new_path = root.join("new.flowgraph.toml");
        assert!(publish_with(&new_path, false, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected creation failure"))
        }).is_err());
        assert!(!new_path.exists());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(),1);
        create(&new_path, b"new complete").unwrap();
        assert_eq!(create(&new_path,b"other").unwrap_err().kind(),io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&new_path).unwrap(),b"new complete");
        std::fs::remove_file(new_path).unwrap();
        replace(&path, b"complete replacement").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(),b"complete replacement");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(),1);
        // Publication failure also discards only the staging file.
        let directory = root.join("directory");
        std::fs::create_dir(&directory).unwrap();
        assert!(replace(&directory,b"cannot replace directory").is_err());
        assert!(directory.is_dir());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(),2);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

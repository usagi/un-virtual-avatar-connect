//! Cooperative, nonblocking lock for writers sharing one Flowgraph directory.
//! Keep the lock file in place: unlinking it would split the lock identity.
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub(crate) fn acquire(root: &Path) -> io::Result<File> {
    let root = root.canonicalize()?;
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(root.join(".flowgraph-write.lock"))?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => io::Error::from(io::ErrorKind::WouldBlock),
        std::fs::TryLockError::Error(error) => error,
    })?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_lock_probe() {
        let Some(root) = std::env::var_os("UNVAC_TEST_LOCK_ROOT") else { return; };
        let result = acquire(Path::new(&root));
        if std::env::var_os("UNVAC_TEST_LOCK_BUSY").is_some() {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WouldBlock);
        } else {
            assert!(result.is_ok());
        }
    }

    #[test]
    fn shared_root_excludes_another_process_and_releases_on_drop() {
        let root = std::env::temp_dir().join(format!("unvac-lock-test-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let lock = acquire(&root).unwrap();
        let child = |busy: bool| {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            let test_path = format!("{}::child_lock_probe", module_path!().split_once("::").unwrap().1);
            command.args(["--exact", &test_path])
                .env("UNVAC_TEST_LOCK_ROOT", &root).env_remove("UNVAC_TEST_LOCK_BUSY");
            if busy { command.env("UNVAC_TEST_LOCK_BUSY", "1"); }
            let output = command.output().unwrap();
            assert!(output.status.success(), "{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        };
        child(true);
        drop(lock);
        child(false);
        std::fs::remove_file(root.join(".flowgraph-write.lock")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}

//! Exclusive creation: an existence check alone must never authorize truncation.
use std::io;
use std::path::Path;

pub(super) fn write_new_file(path: &Path, content: &[u8]) -> io::Result<()> {
    super::file_replace::create(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn competing_creators_do_not_overwrite_the_winner() {
        let root = std::env::temp_dir().join(format!(
            "unvac-create-race-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("main.flowgraph.toml");
        let barrier = Arc::new(Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = [b"first".as_slice(), b"second".as_slice()].into_iter().map(|bytes| {
                let barrier = barrier.clone();
                let path = &path;
                scope.spawn(move || {
                    barrier.wait();
                    (bytes, write_new_file(path, bytes))
                })
            }).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|(_, result)| result.is_ok()).count(), 1);
        let winner = results.iter().find(|(_, result)| result.is_ok()).unwrap().0;
        let loser = results.iter().find(|(_, result)| result.is_err()).unwrap();
        assert_eq!(loser.1.as_ref().unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).unwrap(), winner);
        assert_eq!(write_new_file(&path, b"third").unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).unwrap(), winner);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }
}

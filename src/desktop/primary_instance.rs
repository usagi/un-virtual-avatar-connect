//! Desktop launch admission, before settings, devices, or linked apps are started.
use std::{fs::{File, OpenOptions}, io, path::{Path, PathBuf}};

pub enum Admission { Primary(Primary), Activated }
pub struct Primary { _lock: File, request: PathBuf }
impl Primary {
    pub fn take_activation(&self) -> io::Result<bool> {
        match std::fs::remove_file(&self.request) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }
}
pub fn admit(root: &Path) -> io::Result<Admission> {
    std::fs::create_dir_all(root)?;
    let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(root.join(".desktop-primary.lock"))?;
    let request = root.join(".desktop-activate.request");
    match lock.try_lock() {
        Ok(()) => Ok(Admission::Primary(Primary { _lock: lock, request })),
        Err(std::fs::TryLockError::WouldBlock) => {
            // Existence is the entire message; no commands, URLs, or credentials.
            // A request during boot remains pending until the window is ready.
            OpenOptions::new().create(true).truncate(false).write(true).open(request)?;
            Ok(Admission::Activated)
        }
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
 use super::*;
 #[test]
 fn primary_is_unique_requests_survive_boot_and_lock_is_released() {
  let root=std::env::temp_dir().join(format!("vac-primary-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
  let Admission::Primary(first)=admit(&root).unwrap() else { panic!("first launch") };
  assert!(matches!(admit(&root).unwrap(),Admission::Activated));
  assert!(matches!(admit(&root).unwrap(),Admission::Activated));
  assert!(first.take_activation().unwrap()); assert!(!first.take_activation().unwrap());
  let other=root.join("other-store");
  let Admission::Primary(independent)=admit(&other).unwrap() else { panic!("separate store") };
  assert!(!independent.take_activation().unwrap()); drop(independent);
  drop(first);
  let Admission::Primary(restarted)=admit(&root).unwrap() else { panic!("lock released") };
  drop(restarted); std::fs::remove_dir_all(root).unwrap();
 }
}

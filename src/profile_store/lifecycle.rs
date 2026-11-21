use super::*;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};

#[derive(Debug, thiserror::Error)]
#[error("設定が別の操作で変更されています。編集内容を控えて再読込してください")]
pub struct StaleRevision;

#[derive(Serialize)]
pub struct ProfileContent {
	pub content: String,
	pub revision: String,
}
#[derive(Serialize)]
pub struct ArchivedProfile {
	pub archive_id: String,
	pub profile_id: String,
	pub label: String,
}

fn lock(path: &Path) -> Result<File> {
	let file = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(path)?;
	file.try_lock().map_err(|_| anyhow!("別の操作が進行中、または使用中です"))?;
	Ok(file)
}
pub(super) fn operation_lock(root: &Path) -> Result<File> {
	std::fs::create_dir_all(root)?;
	lock(&root.join(".profiles-operation.lock"))
}
// Startup may briefly overlap a profile save. Bound the wait and do not retry
// permission/path failures; read settings only after this lock is acquired.
pub(super) fn startup_operation_lock(root: &Path, timeout: std::time::Duration) -> Result<File> {
    std::fs::create_dir_all(root)?;
    let file = OpenOptions::new().create(true).truncate(false).read(true).write(true)
        .open(root.join(".profiles-operation.lock"))?;
    let started = std::time::Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
            Err(std::fs::TryLockError::WouldBlock) => {
                let Some(remaining) = timeout.checked_sub(started.elapsed()).filter(|d| !d.is_zero()) else {
                    return Err(anyhow!("別の設定保存が続いているため起動を待機できませんでした。保存完了後に起動し直してください"));
                };
                std::thread::sleep(remaining.min(std::time::Duration::from_millis(20)));
            }
        }
    }
}

pub fn lock_config_write(conf: &Path) -> Result<File> {
	let root = user_config_root()?;
	if super::is_managed_conf_path(&root, conf) {
		operation_lock(&root)
	} else {
		external_config_lock(conf)
	}
}

// Lock a stable sidecar, not the config inode replaced by transactional writes.
// Directory-level locking also serializes creation of new configuration files.
fn external_config_lock(conf: &Path) -> Result<File> {
	let resolved = match std::fs::canonicalize(conf) {
		Ok(path) => path,
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
			let parent = conf.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
			std::fs::canonicalize(parent)?.join(conf.file_name().context("config filename missing")?)
		},
		Err(error) => return Err(error.into()),
	};
	let parent = resolved.parent().context("config directory missing")?;
	lock(&parent.join(".unvac-config-write.lock"))
}
fn profile_dir(root: &Path, id: &str) -> Result<PathBuf> {
	validate_profile_id(id)?;
	let parent = std::fs::canonicalize(root.join(PROFILES_DIR))?;
	if parent.parent() != Some(std::fs::canonicalize(root)?.as_path()) {
		return Err(anyhow!("profile保存先が設定保管先の外です"));
	}
	let path = parent.join(id);
	let resolved = std::fs::canonicalize(&path)?;
	if resolved.parent() != Some(parent.as_path()) || std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
		return Err(anyhow!("profile保存先の境界が不正です"));
	}
	let conf = resolved.join(PROFILE_CONF_FILE);
	if std::fs::canonicalize(&conf)?.parent() != Some(resolved.as_path()) {
		return Err(anyhow!("conf保存先の境界が不正です"));
	}
	Ok(resolved)
}
fn revision(source: &str) -> String {
	Sha256::digest(source.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Resolve only a regular managed profile's conf, using the same storage boundary as editing.
pub(crate) fn managed_conf_at(root: &Path, path: &Path) -> Option<PathBuf> {
	let canonical = std::fs::canonicalize(path).ok()?;
	if canonical.file_name()? != PROFILE_CONF_FILE || !canonical.is_file() {
		return None;
	}
	let id = canonical.parent()?.file_name()?.to_str()?;
	let expected = profile_dir(root, id).ok()?.join(PROFILE_CONF_FILE);
	(std::fs::canonicalize(expected).ok()? == canonical).then_some(canonical)
}
fn read_at(root: &Path, id: &str) -> Result<ProfileContent> {
	let content = std::fs::read_to_string(profile_dir(root, id)?.join(PROFILE_CONF_FILE))?;
	Ok(ProfileContent {
		revision: revision(&content),
		content,
	})
}
pub fn read_content(id: &str) -> Result<ProfileContent> {
	read_at(&user_config_root()?, id)
}
/// Resolve and edit under the same store lock as archive/restore, so the target
/// cannot be moved between path validation and publication.
pub(super) fn edit_managed_config<T>(root: &Path, id: &str, edit: impl FnOnce(&str) -> Result<(String, T)>) -> Result<T> {
	let _operation = operation_lock(root)?;
	let path = profile_dir(root, id)?.join(PROFILE_CONF_FILE);
	let source = std::fs::read_to_string(&path)?;
	let (prepared, result) = edit(&source)?;
	toml::from_str::<crate::Conf>(&prepared).context("設定の構文が不正です。保存していません")?;
	if source != prepared { super::write_config_transaction(&path,prepared.as_bytes())?; }
	Ok(result)
}
pub fn save_content(id: &str, content: &str, expected: &str) -> Result<ProfileContent> {
	save_at(&user_config_root()?, id, content, expected)
}
fn save_at(root: &Path, id: &str, content: &str, expected: &str) -> Result<ProfileContent> {
	let _operation = operation_lock(root)?;
	let old = read_at(root, id)?;
	if old.revision != expected {
		return Err(StaleRevision.into());
	}
	toml::from_str::<crate::Conf>(content).context("設定の構文が不正です。保存していません")?;
	let dir = profile_dir(root, id)?;
	let temp = staging_profile_dir(&dir, id, "save");
	let backup = staging_profile_dir(&dir, id, "backup");
	let conf = dir.join(PROFILE_CONF_FILE);
	std::fs::write(&temp, content)?;
	if let Err(error) = std::fs::rename(&conf, &backup) {
		let _ = std::fs::remove_file(&temp);
		return Err(error.into());
	}
	if let Err(error) = std::fs::rename(&temp, &conf) {
		std::fs::rename(&backup, &conf).context("保存失敗後の元設定復元に失敗")?;
		let _ = std::fs::remove_file(&temp);
		return Err(error.into());
	}
	read_at(root, id)
}

/// Held until process exit. Shared leases allow two VACs to use the same profile.
pub fn acquire_lease(conf: &Path) -> Result<Option<File>> {
	let root = user_config_root()?;
	if !super::is_managed_conf_path(&root, conf) {
		return Ok(None);
	}
	let _operation = startup_operation_lock(&root, std::time::Duration::from_secs(2))?;
	let resolved = std::fs::canonicalize(conf)?;
	let dir = resolved.parent().context("profile directory missing")?;
	let id = dir.file_name().and_then(|s| s.to_str()).context("profile id missing")?;
	if profile_dir(&root, id)?.join(PROFILE_CONF_FILE) != resolved {
		return Ok(None);
	}
	let file = OpenOptions::new()
		.create(true)
		.truncate(false)
		.read(true)
		.write(true)
		.open(dir.join(".in-use.lock"))?;
	file.try_lock_shared()
		.map_err(|_| anyhow!("プロファイルを管理操作中です。起動を中止しました"))?;
	Ok(Some(file))
}

pub fn archive(id: &str) -> Result<()> {
	archive_at(&user_config_root()?, id)
}
fn archive_at(root: &Path, id: &str) -> Result<()> {
	let _operation = operation_lock(root)?;
	let settings = read_settings(&root.join(SETTINGS_FILE))?.unwrap_or_else(DesktopSettings::starter);
	if settings.active_profile_id == id {
		return Err(anyhow!("次回起動用のプロファイルは削除できません。先に別の起動先を選んでください"));
	}
	let dir = profile_dir(root, id)?;
	let _lease = lock(&dir.join(".in-use.lock"))?;
	// Windows cannot move a directory with this handle open. The store operation
	// lock remains held, so a new managed-profile boot cannot acquire a lease here.
	drop(_lease);
	let trash = root.join("deleted-profiles");
	std::fs::create_dir_all(&trash)?;
	let trash = std::fs::canonicalize(trash)?;
	if trash.parent() != Some(std::fs::canonicalize(root)?.as_path()) {
		return Err(anyhow!("削除済み保管先が設定保管先の外です"));
	}
	let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
	std::fs::rename(dir, trash.join(format!("{id}-{nonce}")))?;
	Ok(())
}
pub fn archived() -> Result<Vec<ArchivedProfile>> {
	archived_at(&user_config_root()?)
}
fn archived_at(root: &Path) -> Result<Vec<ArchivedProfile>> {
	let trash = root.join("deleted-profiles");
	if !trash.exists() {
		return Ok(Vec::new());
	}
	let mut result = Vec::new();
	for entry in std::fs::read_dir(trash)? {
		let entry = entry?;
		if !entry.file_type()?.is_dir() {
			continue;
		}
		if let Ok(manifest) = read_profile_manifest(&entry.path()) {
			result.push(ArchivedProfile {
				archive_id: entry.file_name().to_string_lossy().into_owned(),
				profile_id: manifest.id,
				label: manifest.label,
			});
		}
	}
	result.sort_by(|a, b| a.archive_id.cmp(&b.archive_id));
	Ok(result)
}
pub fn restore(archive_id: &str) -> Result<()> {
	restore_at(&user_config_root()?, archive_id)
}
fn restore_at(root: &Path, archive_id: &str) -> Result<()> {
	let _operation = operation_lock(root)?;
	if archive_id.is_empty() || !archive_id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
		return Err(anyhow!("復元対象IDが不正です"));
	}
	let trash = std::fs::canonicalize(root.join("deleted-profiles"))?;
	let root_canonical = std::fs::canonicalize(root)?;
	if trash.parent() != Some(root_canonical.as_path()) {
		return Err(anyhow!("復元元が設定保管先の外です"));
	}
	let source = std::fs::canonicalize(trash.join(archive_id))?;
	if source.parent() != Some(trash.as_path()) {
		return Err(anyhow!("復元元の境界が不正です"));
	}
	let manifest = read_profile_manifest(&source)?;
	validate_profile_id(&manifest.id)?;
	let profiles = std::fs::canonicalize(root.join(PROFILES_DIR))?;
	if profiles.parent() != Some(root_canonical.as_path()) {
		return Err(anyhow!("復元先が設定保管先の外です"));
	}
	let target = profiles.join(&manifest.id);
	if target.exists() {
		return Err(anyhow!(
			"同じIDのプロファイルが存在するため復元できません。既存設定は変更していません"
		));
	}
	std::fs::rename(source, target)?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	#[ignore = "child of external_config_guard_survives_replacement; no devices"]
	fn external_config_guard_child() {
		let path = std::env::var_os("UNVAC_TEST_EXTERNAL_CONFIG_LOCK").expect("parent test config path");
		assert!(external_config_lock(Path::new(&path)).is_err());
	}

	#[test]
	fn external_config_guard_survives_replacement() {
		let root = setup();
		let external = root.join("external");
		std::fs::create_dir(&external).unwrap();
		let conf = external.join("conf.toml");
		std::fs::write(&conf, "workers = 1\n").unwrap();
		let guard = external_config_lock(&conf).unwrap();
		assert!(external_config_lock(&external.join("new.toml")).is_err());
		let replacement = external.join("replacement.toml");
		std::fs::rename(&conf, &replacement).unwrap();
		std::fs::write(&conf, "workers = 2\n").unwrap();
		assert!(external_config_lock(&conf).is_err(), "replacement must not bypass sidecar lock");
		let result = std::process::Command::new(std::env::current_exe().unwrap())
			.args(["profile_store::lifecycle::tests::external_config_guard_child", "--exact", "--ignored", "--test-threads=1"])
			.env("UNVAC_TEST_EXTERNAL_CONFIG_LOCK", &conf).output().unwrap();
		assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stdout));
		drop(guard);
		let retry = external_config_lock(&conf).unwrap();
		drop(retry);
		assert_eq!(std::fs::read_to_string(&conf).unwrap(), "workers = 2\n");
		std::fs::remove_dir_all(root).unwrap();
	}
	fn setup() -> PathBuf {
		let root = std::env::temp_dir().join(format!(
			"vac-lifecycle-{}",
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_nanos()
		));
		super::super::ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		create_profile_at(root.clone(), "other", "Other", "workers = 2\n", None, &[]).unwrap();
		root
	}
	#[test]
	fn invalid_and_stale_edits_preserve_contents() {
		let root = setup();
		let old = read_at(&root, "other").unwrap();
		assert!(save_at(&root, "other", "not valid [", &old.revision).is_err());
		assert_eq!(read_at(&root, "other").unwrap().content, old.content);
		let saved = save_at(&root, "other", "workers = 3\n", &old.revision).unwrap();
		assert!(save_at(&root, "other", "workers = 4\n", &old.revision).is_err());
		assert_eq!(read_at(&root, "other").unwrap().content, saved.content);
		std::fs::remove_dir_all(root).unwrap();
	}
	#[test]
	fn saved_profile_roundtrip_preserves_workspace_backups_and_other_profile() {
		let root = setup();
		let dir = profile_dir(&root, "other").unwrap();
		let settings = std::fs::read(root.join(SETTINGS_FILE)).unwrap();
		let default = read_at(&root, "default").unwrap();
		let manifest = std::fs::read(dir.join("profile.toml")).unwrap();
		let original = read_at(&root, "other").unwrap();
		std::fs::create_dir_all(dir.join("flowgraph/custom")).unwrap();
		std::fs::write(dir.join("flowgraph/custom/main.flowgraph.toml"), b"# user graph\r\n").unwrap();
		std::fs::write(dir.join("user-asset.bin"), [0, 255, 42, 128]).unwrap();
		let purpose_bytes = include_bytes!("../../profile.template/streaming-tools/purposes.json");
		std::fs::write(dir.join("flowgraph/.flowgraph-purposes.json"), purpose_bytes).unwrap();
		let first = "# 個別設定を保持\r\nworkers = 3\r\n";
		let saved = save_at(&root, "other", first, &original.revision).unwrap();
		let final_text = "# 次の版\nworkers = 4\n";
		let final_saved = save_at(&root, "other", final_text, &saved.revision).unwrap();
		assert_eq!(final_saved.content, final_text);
		let backups: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|entry| entry.unwrap().path())
			.filter(|path| path.file_name().unwrap().to_string_lossy().starts_with(".other.backup-")).collect();
		assert_eq!(backups.len(), 2);
		let backup_bytes: Vec<_> = backups.iter().map(|path| std::fs::read(path).unwrap()).collect();
		assert!(backup_bytes.contains(&original.content.into_bytes()));
		assert!(backup_bytes.contains(&first.as_bytes().to_vec()));
		archive_at(&root, "other").unwrap();
		assert!(!dir.exists());
		let archived = archived_at(&root).unwrap();
		assert_eq!(archived.len(), 1);
		restore_at(&root, &archived[0].archive_id).unwrap();
		let restored = read_at(&root, "other").unwrap();
		assert_eq!(restored.content, final_text);
		assert_eq!(restored.revision, final_saved.revision);
		assert_eq!(std::fs::read(dir.join("profile.toml")).unwrap(), manifest);
		assert_eq!(std::fs::read(dir.join("flowgraph/custom/main.flowgraph.toml")).unwrap(), b"# user graph\r\n");
		assert_eq!(std::fs::read(dir.join("user-asset.bin")).unwrap(), [0, 255, 42, 128]);
		assert_eq!(std::fs::read(dir.join("flowgraph/.flowgraph-purposes.json")).unwrap(), purpose_bytes);
		for (path, bytes) in backups.iter().zip(backup_bytes) {
			assert_eq!(std::fs::read(path).unwrap(), bytes);
		}
		assert_eq!(std::fs::read(root.join(SETTINGS_FILE)).unwrap(), settings);
		let default_after = read_at(&root, "default").unwrap();
		assert_eq!(default_after.content, default.content);
		assert_eq!(default_after.revision, default.revision);
		std::fs::remove_dir_all(root).unwrap();
	}
	#[test]
	fn archive_guards_in_use_default_and_restore_collision() {
		let root = setup();
		assert!(archive_at(&root, "default").is_err());
		let dir = profile_dir(&root, "other").unwrap();
		std::fs::create_dir(dir.join("credentials")).unwrap();
		std::fs::write(dir.join("credentials/key.txt"), "fixture-private").unwrap();
		let lease = OpenOptions::new()
			.create(true)
			.truncate(false)
			.read(true)
			.write(true)
			.open(dir.join(".in-use.lock"))
			.unwrap();
		lease.lock_shared().unwrap();
		assert!(archive_at(&root, "other").is_err());
		drop(lease);
		archive_at(&root, "other").unwrap();
		let deleted = archived_at(&root).unwrap();
		assert_eq!(deleted.len(), 1);
		create_profile_at(root.clone(), "other", "Replacement", "workers = 9\n", None, &[]).unwrap();
		let replacement = read_at(&root, "other").unwrap().content;
		assert!(restore_at(&root, &deleted[0].archive_id).is_err());
		assert_eq!(read_at(&root, "other").unwrap().content, replacement);
		archive_at(&root, "other").unwrap();
		restore_at(&root, &deleted[0].archive_id).unwrap();
		assert_eq!(
			std::fs::read_to_string(profile_dir(&root, "other").unwrap().join("credentials/key.txt")).unwrap(),
			"fixture-private"
		);
		assert!(restore_at(&root, "../other").is_err());
		std::fs::remove_dir_all(root).unwrap();
	}
}

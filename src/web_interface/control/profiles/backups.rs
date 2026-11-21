//! Configuration backups are restored as a new file, never over a live configuration.
use super::util::{err_response, profiles_dir, safe_basename};
use actix_web::{
	get, post,
	web::{Data, Json},
	HttpResponse,
};
use serde::{Deserialize, Serialize};
use std::{
	io::Write,
	path::{Path, PathBuf},
};

#[derive(Serialize)]
struct Entry {
	filename: String,
	original: String,
	size: u64,
}

fn source(dir: &Path, name: &str) -> anyhow::Result<(PathBuf, String)> {
	let (original, stamp) = name
		.rsplit_once(".bak-")
		.ok_or_else(|| anyhow::anyhow!("バックアップ名が不正です"))?;
	if stamp.is_empty() || !stamp.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
		anyhow::bail!("バックアップ名が不正です");
	}
	safe_basename(dir, original).map_err(|_| anyhow::anyhow!("復元元の設定ファイル名が不正です"))?;
	let path = dir.join(name);
	let meta = std::fs::symlink_metadata(&path)?;
	if !meta.is_file()
		|| meta.file_type().is_symlink()
		|| std::fs::canonicalize(&path)?.parent() != Some(std::fs::canonicalize(dir)?.as_path())
	{
		anyhow::bail!("このバックアップは復元できません");
	}
	Ok((path, original.to_owned()))
}

fn restore_file(dir: &Path, backup: &str, filename: &str) -> anyhow::Result<PathBuf> {
	let (source, _) = source(dir, backup)?;
	let target = safe_basename(dir, filename).map_err(|_| anyhow::anyhow!("復元先の設定ファイル名が不正です"))?;
	let _operation = crate::profile_store::lifecycle::lock_config_write(&target)?;
	let content = std::fs::read(&source)?;
	toml::from_str::<crate::conf::Conf>(std::str::from_utf8(&content)?)?;
	// create_new prevents both accidental replacement and a check/write race.
	let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&target)?;
	if let Err(error) = output.write_all(&content).and_then(|_| output.sync_all()) {
		drop(output);
		let _ = std::fs::remove_file(&target);
		return Err(error.into());
	}
	Ok(target)
}

#[get("/profile-backups")]
pub async fn list(state: Data<crate::SharedState>) -> HttpResponse {
	let (dir, _) = match profiles_dir(&state).await {
		Ok(value) => value,
		Err(response) => return response,
	};
	let files = match std::fs::read_dir(&dir) {
		Ok(files) => files,
		Err(error) => return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "read_failed", error),
	};
	let mut entries = Vec::new();
	for file in files.flatten() {
		let name = file.file_name().to_string_lossy().to_string();
		if let Ok((path, original)) = source(&dir, &name) {
			if let Ok(meta) = path.metadata() {
				entries.push(Entry {
					filename: name,
					original,
					size: meta.len(),
				});
			}
		}
	}
	entries.sort_by(|a, b| b.filename.cmp(&a.filename));
	HttpResponse::Ok().json(serde_json::json!({"entries": entries, "directory": dir}))
}

#[derive(Deserialize)]
pub struct Restore {
	backup: String,
	filename: String,
}

#[post("/profile-backups/restore")]
pub async fn restore(state: Data<crate::SharedState>, body: Json<Restore>) -> HttpResponse {
	let (dir, _) = match profiles_dir(&state).await {
		Ok(value) => value,
		Err(response) => return response,
	};
	match restore_file(&dir, &body.backup, &body.filename) {
		Ok(path) => HttpResponse::Ok().json(super::ProfileOpResponse {
			filename: body.filename.trim().to_owned(),
			path: path.display().to_string(),
			backup: None,
			warning: Some("新しい設定ファイルへ復元しました。実行中の設定は変更していません。内容を確認してから適用してください。".into()),
		}),
		Err(error) => err_response(
			actix_web::http::StatusCode::CONFLICT,
			"restore_failed",
			format!("復元できません。別の操作が進行中なら完了を待ち、それ以外は復元先名とバックアップを確認してください: {error}"),
		),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	struct TestDir(PathBuf);
	impl TestDir {
		fn new() -> Self {
			let path = std::env::temp_dir().join(format!(
				"vac-restore-{}-{}",
				std::process::id(),
				std::time::SystemTime::now()
					.duration_since(std::time::UNIX_EPOCH)
					.unwrap()
					.as_nanos()
			));
			std::fs::create_dir(&path).unwrap();
			Self(path)
		}
		fn path(&self) -> &Path {
			&self.0
		}
	}
	impl Drop for TestDir {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.0);
		}
	}
	#[test]
	fn restores_exact_contents_without_overwriting_or_starting_anything() {
		let dir = TestDir::new();
		let content = "workers = 2\n# retained comment\n";
		std::fs::write(dir.path().join("conf.toml.bak-20261017-123456"), content).unwrap();
		let restored = restore_file(dir.path(), "conf.toml.bak-20261017-123456", "restored.toml").unwrap();
		assert_eq!(std::fs::read_to_string(&restored).unwrap(), content);
		std::fs::write(&restored, "workers = 3\n").unwrap();
		assert!(restore_file(dir.path(), "conf.toml.bak-20261017-123456", "restored.toml").is_err());
		assert_eq!(std::fs::read_to_string(&restored).unwrap(), "workers = 3\n");
		assert_eq!(
			std::fs::read_to_string(dir.path().join("conf.toml.bak-20261017-123456")).unwrap(),
			content
		);
	}
	#[test]
	fn rejects_invalid_content_and_paths_without_creating_output() {
		let dir = TestDir::new();
		std::fs::write(dir.path().join("conf.toml.bak-1"), "[invalid").unwrap();
		assert!(restore_file(dir.path(), "conf.toml.bak-1", "restored.toml").is_err());
		assert!(!dir.path().join("restored.toml").exists());
		assert!(source(dir.path(), "../conf.toml.bak-1").is_err());
		assert!(source(dir.path(), "conf.toml.bak-1/other").is_err());
		assert!(restore_file(dir.path(), "conf.toml.bak-1", "../outside.toml").is_err());
	}
	#[test]
	fn restoration_waits_for_config_writer_without_creating_target() {
		let dir = TestDir::new();
		let backup = "conf.toml.bak-20261019-1";
		let content = "workers = 2\n# recovery content\n";
		std::fs::write(dir.path().join(backup), content).unwrap();
		let current = dir.path().join("conf.toml");
		std::fs::write(&current, "workers = 1\n").unwrap();
		let guard = crate::profile_store::lifecycle::lock_config_write(&current).unwrap();
		assert!(restore_file(dir.path(), backup, "restored.toml").is_err());
		assert!(!dir.path().join("restored.toml").exists());
		assert_eq!(std::fs::read_to_string(&current).unwrap(), "workers = 1\n");
		drop(guard);
		let restored = restore_file(dir.path(), backup, "restored.toml").unwrap();
		assert_eq!(std::fs::read_to_string(restored).unwrap(), content);
		assert_eq!(std::fs::read_to_string(dir.path().join(backup)).unwrap(), content);
	}
}

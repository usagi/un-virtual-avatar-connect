//! Edit only the connection identity; preserve advanced settings and credentials.
use crate::{conf::Conf, SharedState};
use actix_web::{get, put, web, HttpResponse};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct Input {
	#[serde(default)]
	pub expected_source: Option<String>,
	pub revision: String,
	pub broadcaster: String,
	pub moderator: String,
}

#[derive(Serialize)]
struct View {
	source_path: String,
	revision: String,
	broadcaster: String,
	moderator: String,
	restart_required: bool,
}

fn identity(conf: &Conf) -> (String, String) {
	let Some(t) = &conf.twitch else {
		return (String::new(), String::new());
	};
	(
		t.eventsub
			.as_ref()
			.and_then(|e| e.broadcaster_login.clone())
			.unwrap_or_else(|| t.username.clone()),
		t.moderator.as_ref().and_then(|m| m.login.clone()).unwrap_or_default(),
	)
}

fn login(value: &str) -> Result<String, &'static str> {
	let value = value.trim().to_ascii_lowercase();
	if value.is_empty() || value.len() > 25 || !value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
		return Err("ログイン名は半角英数字と _ で入力してください。表示名やURLではありません。");
	}
	Ok(value)
}

fn edit(original: &str, input: &Input) -> Result<String, &'static str> {
	if blake3::hash(original.as_bytes()).to_hex().as_str() != input.revision {
		return Err("別の画面で設定が変更されました。入力内容を控えてから読み直してください。");
	}
	let broadcaster = login(&input.broadcaster)?;
	let moderator = login(&input.moderator)?;
	let mut doc = original
		.parse::<toml_edit::DocumentMut>()
		.map_err(|_| "設定ファイルを読み取れません。")?;
	let fresh = doc.get("twitch").is_none();
	let previous: Conf = toml::from_str(original).map_err(|_| "設定ファイルを読み取れません。")?;
	let previous_broadcaster = identity(&previous).0;
	// Existing username may be an independent legacy ingress identity. Do not replace it.
	if fresh || previous.twitch.as_ref().is_some_and(|t| t.username == previous_broadcaster) {
		doc["twitch"]["username"] = toml_edit::value(&broadcaster);
	}
	for section in ["eventsub", "moderator"] {
		if doc["twitch"].get(section).is_none() {
			doc["twitch"][section]["oauth_auto"] = toml_edit::value(false);
			// Flowgraph owns reception; configuring OAuth must not start a second legacy loop.
			doc["twitch"][section]["enabled"] = toml_edit::value(section == "moderator");
		}
	}
	doc["twitch"]["eventsub"]["broadcaster_login"] = toml_edit::value(&broadcaster);
	doc["twitch"]["moderator"]["login"] = toml_edit::value(&moderator);
	// Preserve explicit token-key choices, but make both newly configured accounts reachable.
	if let Some(keys) = doc["twitch"].get_mut("token_keys").and_then(|item| item.as_array_mut()) {
		for key in ["broadcaster", "moderator"] {
			if !keys.iter().any(|v| v.as_str() == Some(key)) {
				keys.push(key);
			}
		}
	}
	let result = doc.to_string();
	toml::from_str::<Conf>(&result).map_err(|_| "変更後の設定が不正です。保存しませんでした。")?;
	Ok(result)
}

fn failure(code: actix_web::http::StatusCode, reason: &str) -> HttpResponse {
	HttpResponse::build(code).json(serde_json::json!({"error":"twitch_setup_failed", "reason":reason}))
}

fn read_or_save(path: &std::path::Path, input: Option<Input>) -> Result<String, HttpResponse> {
	use actix_web::http::StatusCode as S;
	// Bind a reviewed GUI draft to its source even when another profile has identical bytes.
    if input.as_ref().and_then(|value| value.expected_source.as_deref())
        .is_some_and(|expected| expected != path.to_string_lossy()) {
        return Err(failure(S::CONFLICT, "保存先が変わりました。入力内容は残しています。対象を確認してから読み直してください。"));
    }
	let _lock = match crate::profile_store::lifecycle::lock_config_write(&path) {
		Ok(lock) => lock,
		Err(_) => return Err(failure(S::CONFLICT, "このプロファイルは別の操作中です。")),
	};
	let original = match std::fs::read_to_string(&path) {
		Ok(v) => v,
		Err(_) => return Err(failure(S::INTERNAL_SERVER_ERROR, "設定ファイルを読み取れません。")),
	};
	let text = if let Some(input) = input {
		if input.revision != blake3::hash(original.as_bytes()).to_hex().as_str() {
			return Err(failure(
				S::CONFLICT,
				"別の画面で設定が変更されました。入力内容を控えてから読み直してください。",
			));
		}
		let updated = match edit(&original, &input) {
			Ok(v) => v,
			Err(e) => return Err(failure(S::BAD_REQUEST, e)),
		};
		if updated != original {
			let backup = path.with_extension(format!(
				"toml.bak-{}",
				std::time::SystemTime::now()
					.duration_since(std::time::UNIX_EPOCH)
					.unwrap_or_default()
					.as_nanos()
			));
			if std::fs::copy(&path, backup).is_err() {
				return Err(failure(S::INTERNAL_SERVER_ERROR, "バックアップを作れないため保存しませんでした。"));
			}
			if crate::profile_store::write_config_transaction(&path, updated.as_bytes()).is_err() {
				return Err(failure(S::INTERNAL_SERVER_ERROR, "設定の保存に失敗しました。"));
			}
		}
		updated
	} else {
		original
	};
	Ok(text)
}

async fn handle(state: web::Data<SharedState>, input: Option<Input>) -> HttpResponse {
	use actix_web::http::StatusCode as S;
	let (path, runtime) = {
		let s = state.read().await;
		(s.conf_source_path.clone(), s.twitch.clone())
	};
	let Some(path) = path else {
		return failure(S::CONFLICT, "保存先を特定できません。");
	};
	let text = match read_or_save(&path, input) {
		Ok(text) => text,
		Err(response) => return response,
	};
	let conf = match toml::from_str::<Conf>(&text) {
		Ok(v) => v,
		Err(_) => return failure(S::INTERNAL_SERVER_ERROR, "設定を読み取れません。"),
	};
	let (broadcaster, moderator) = identity(&conf);
	let restart_required = serde_json::to_value(&conf.twitch).ok() != serde_json::to_value(&runtime).ok();
	HttpResponse::Ok().json(View {
		source_path: path.display().to_string(),
		revision: blake3::hash(text.as_bytes()).to_hex().to_string(),
		broadcaster,
		moderator,
		restart_required,
	})
}

#[get("/twitch/setup")]
async fn get_setup(state: web::Data<SharedState>) -> HttpResponse {
	handle(state, None).await
}
#[put("/twitch/setup")]
async fn put_setup(state: web::Data<SharedState>, input: web::Json<Input>) -> HttpResponse {
	handle(state, Some(input.into_inner())).await
}
pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(get_setup).service(put_setup);
}

/// Development comparison adapter. Operates only on supplied strings: no files,
/// credentials, network, runtime initialization or authorization side effects.
#[doc(hidden)]
pub fn isolated_preview(text: &str, runtime_text: &str, input: Option<Input>) -> Result<serde_json::Value, String> {
    let updated = match input { Some(input) => edit(text, &input).map_err(str::to_owned)?, None => text.to_owned() };
    let saved: Conf = toml::from_str(&updated).map_err(|e| e.to_string())?;
    let runtime: Conf = toml::from_str(runtime_text).map_err(|e| e.to_string())?;
    let (broadcaster, moderator) = identity(&saved);
    let view = View { source_path: "isolated-memory".into(), revision: blake3::hash(updated.as_bytes()).to_hex().to_string(),
        broadcaster, moderator, restart_required: serde_json::to_value(&saved.twitch).ok() != serde_json::to_value(&runtime.twitch).ok() };
    Ok(serde_json::json!({"content": updated, "view": view}))
}

#[cfg(test)]
mod tests {
	use super::*;
	fn input(text: &str) -> Input {
		Input {
			expected_source: None,
			revision: blake3::hash(text.as_bytes()).to_hex().to_string(),
			broadcaster: "Example_User".into(),
			moderator: "example_bot".into(),
		}
	}
	#[test]
	fn initial_setup_requires_no_ai_and_no_auto_authorization() {
		let result = edit("workers = 1\n", &input("workers = 1\n")).unwrap();
		let c: Conf = toml::from_str(&result).unwrap();
		assert_eq!(identity(&c), ("example_user".into(), "example_bot".into()));
		let t = c.twitch.unwrap();
		assert!(!t.eventsub.unwrap().oauth_auto);
		assert!(!result.contains("ai_personas"));
	}
	#[test]
	fn preserves_advanced_settings_and_rejects_stale_input() {
		let text = "workers=1\n[twitch]\nusername='legacy'\nreads=['other']\ntoken_keys=['custom']\n[twitch.eventsub]\nbroadcaster_login='old_channel'\nchannel_points_reward_id='keep'\nuser_access_token='private'\n";
		let result = edit(text, &input(text)).unwrap();
		let c: Conf = toml::from_str(&result).unwrap();
		let t = c.twitch.unwrap();
		assert_eq!(t.username, "legacy");
		assert_eq!(t.resolved_token_keys(), vec!["custom", "broadcaster", "moderator"]);
		let es = t.eventsub.unwrap();
		assert_eq!(es.user_access_token.as_deref(), Some("private"));
		assert_eq!(es.channel_points_reward_id.as_deref(), Some("keep"));
		assert!(edit(&result, &input(text)).is_err());
	}
	#[test]
	fn rejects_urls_and_blank_accounts() {
		for value in ["", "https://twitch.tv/example", "表示名", "a b"] {
			assert!(login(value).is_err());
		}
	}
	#[test]
	fn disk_save_backup_conflict_and_other_profile_isolation() {
		let root = std::env::temp_dir().join(format!("vac-twitch-setup-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
		std::fs::create_dir_all(&root).unwrap();
		let a = root.join("a.toml");
		let b = root.join("b.toml");
		let original = "workers=1\n";
		std::fs::write(&a, original).unwrap();
		std::fs::write(&b, original).unwrap();
        let mut reviewed = input(original);
        reviewed.expected_source = Some(a.display().to_string());
        assert_eq!(read_or_save(&b, Some(reviewed)).unwrap_err().status(), actix_web::http::StatusCode::CONFLICT);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), original);
        assert_eq!(std::fs::read_to_string(&b).unwrap(), original);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2, "Rejected source must create no backup or staging files");
        let mut reviewed = input(original);
        reviewed.expected_source = Some(a.display().to_string());
        let saved = read_or_save(&a, Some(reviewed)).unwrap();
		assert_eq!(std::fs::read_to_string(&a).unwrap(), saved);
		assert_eq!(std::fs::read_to_string(&b).unwrap(), original);
		let backup = std::fs::read_dir(&root).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("a.toml.bak-")).unwrap();
		assert_eq!(std::fs::read_to_string(backup.path()).unwrap(), original);
		assert_eq!(read_or_save(&a, Some(input(original))).unwrap_err().status(), actix_web::http::StatusCode::CONFLICT);
		assert_eq!(read_or_save(&a, None).unwrap(), saved);
		let loaded: Conf = toml::from_str(&std::fs::read_to_string(&a).unwrap()).unwrap();
		assert_eq!(identity(&loaded).0, "example_user");
		std::fs::remove_dir_all(root).unwrap();
	}
    #[test]
    fn saved_setup_reopens_in_fresh_process() {
        const CHILD: &str = "UNVAC_TWITCH_REOPEN_FIXTURE";
        if let Some(path) = std::env::var_os(CHILD) {
            let path = std::path::PathBuf::from(path);
            // This loads from disk through the application loader. It does not
            // initialize State, audio, OAuth or Twitch workers.
            let reopened = Conf::new_noop_probe(path.clone()).unwrap();
            let text = read_or_save(&path, None).unwrap();
            let stored: Conf = toml::from_str(&text).unwrap();
            assert_eq!(identity(&reopened), ("example_user".into(), "example_bot".into()));
            assert_eq!(serde_json::to_value(&reopened.twitch).unwrap(), serde_json::to_value(&stored.twitch).unwrap());
            assert!(!reopened.twitch.as_ref().unwrap().eventsub.as_ref().unwrap().oauth_auto);
            std::fs::write(path.with_extension("reopened.json"), serde_json::to_vec(&serde_json::json!({
                "pid":std::process::id(), "broadcaster":identity(&reopened).0,
                "moderator":identity(&reopened).1, "digest":blake3::hash(text.as_bytes()).to_hex().to_string()
            })).unwrap()).unwrap();
            return;
        }
        let root = std::env::temp_dir().join(format!("vac-twitch-reopen-{}-{}", std::process::id(), rand::random::<u64>()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("conf.toml");
        let original = "workers=1\n";
        std::fs::write(&path, original).unwrap();
        let mut reviewed = input(original);
        reviewed.expected_source = Some(path.display().to_string());
        let saved = read_or_save(&path, Some(reviewed)).unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "web_interface::control::twitch_setup::tests::saved_setup_reopens_in_fresh_process", "--nocapture"])
            .env(CHILD, &path).output().unwrap();
        assert!(child.status.success(), "{}\n{}", String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
        let report: serde_json::Value = serde_json::from_slice(&std::fs::read(path.with_extension("reopened.json")).unwrap()).unwrap();
        assert_ne!(report["pid"].as_u64().unwrap(), u64::from(std::process::id()));
        assert_eq!(report["digest"], blake3::hash(saved.as_bytes()).to_hex().to_string());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
        std::fs::remove_dir_all(root).unwrap();
    }

	#[test]
	#[cfg(windows)]
	fn locked_destination_retains_config_and_can_retry_same_revision() {
		use std::os::windows::fs::OpenOptionsExt;
		let root = std::env::temp_dir().join(format!("vac-twitch-locked-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
		std::fs::create_dir_all(&root).unwrap();
		let path = root.join("conf.toml");
		let other = root.join("other.toml");
		let original = "# Keep this comment\nworkers=1\n[twitch]\nusername='legacy'\nreads=['untouched']\n";
		std::fs::write(&path, original).unwrap();
		std::fs::write(&other, original).unwrap();
		// Allow the backup read but deny replacement, like an external file holder.
		let handle = std::fs::OpenOptions::new().read(true).share_mode(1).open(&path).unwrap();
		let error = read_or_save(&path, Some(input(original))).unwrap_err();
		assert_eq!(error.status(), actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);
		assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
		assert_eq!(std::fs::read_to_string(&other).unwrap(), original);
		drop(handle);
		let saved = read_or_save(&path, Some(input(original))).unwrap();
		assert_eq!(read_or_save(&path, None).unwrap(), saved);
		let loaded: Conf = toml::from_str(&saved).unwrap();
		assert_eq!(identity(&loaded), ("example_user".into(), "example_bot".into()));
		assert!(saved.contains("# Keep this comment"));
		assert!(saved.contains("untouched"));
		assert_eq!(std::fs::read_to_string(&other).unwrap(), original);
		assert_eq!(read_or_save(&path, Some(input(original))).unwrap_err().status(), actix_web::http::StatusCode::CONFLICT);
		assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
		std::fs::remove_dir_all(root).unwrap();
	}

}

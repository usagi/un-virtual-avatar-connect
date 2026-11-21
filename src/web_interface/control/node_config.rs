//! Processor / AI persona の conf エントリ単位 read/write API。
//!
//! GUI から runtime reload とディスク上の conf 更新を分けて扱えるよう、現
//! `conf_source_path` の `[[ai.personas]]` を index 指定で読み書きする。

use std::path::{Path, PathBuf};

use actix_web::web::{self, Data, Json, Path as ActixPath};
use actix_web::{get, put, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

use crate::ai::config::AiPersonaConf;
use crate::conf::Conf;
use crate::SharedState;

#[derive(Debug, Serialize)]
pub struct AiPersonaConfigResponse {
	pub index: usize,
	pub id: Option<String>,
	pub config: JsonValue,
	pub source_path: Option<String>,
	pub revision: String,
}

#[derive(Debug, Deserialize)]
pub struct NodePutConfigRequest {
	pub config: JsonValue,
	#[serde(default)]
	pub expected_revision: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NodePutConfigResponse {
	pub index: usize,
	pub source_path: String,
	pub backup: String,
	pub warning: String,
}

#[get("/ai_personas/{index}/config")]
pub async fn get_ai_persona_config(state: Data<SharedState>, index: ActixPath<usize>) -> impl Responder {
	let index = index.into_inner();
	let Some(source_path) = conf_source_path(&state).await else {
		return err_response(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"conf_source_path_unset",
			"current conf path is unknown (source_path is None)",
		);
	};
	let _operation = match crate::profile_store::lifecycle::lock_config_write(&source_path) {
		Ok(lock) => lock,
		Err(error) => return err_response(actix_web::http::StatusCode::CONFLICT, "profile_busy", error),
	};
	let original = match std::fs::read_to_string(&source_path) {
		Ok(value) => value,
		Err(error) => return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "read_failed", error),
	};
	let conf = match Conf::new_noop_probe(source_path.clone()) {
		Ok(conf) => conf,
		Err(e) => return err_response(actix_web::http::StatusCode::BAD_REQUEST, "conf_load_failed", e),
	};
	if std::fs::read_to_string(&source_path).ok().as_deref() != Some(original.as_str()) {
		return err_response(actix_web::http::StatusCode::CONFLICT, "config_changed", "設定が読込中に変更されました。再読込してください。");
	}
	let Some(persona) = conf.ai.personas.get(index) else {
		return err_response(
			actix_web::http::StatusCode::NOT_FOUND,
			"ai_persona_not_found",
			format!("ai.personas[{index}] が存在しません。"),
		);
	};
	let config = match serde_json::to_value(persona) {
		Ok(v) => v,
		Err(e) => return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "serialize_failed", e),
	};
	HttpResponse::Ok().json(AiPersonaConfigResponse {
		index,
		id: persona.id.clone(),
		config,
		source_path: Some(source_path.display().to_string()),
		revision: config_revision(&source_path, &original),
	})
}

#[put("/ai_personas/{index}/config")]
pub async fn put_ai_persona_config(state: Data<SharedState>, index: ActixPath<usize>, body: Json<NodePutConfigRequest>) -> impl Responder {
	let index = index.into_inner();
	let Some(source_path) = conf_source_path(&state).await else {
		return err_response(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"conf_source_path_unset",
			"current conf path is unknown (source_path is None)",
		);
	};

	let persona: AiPersonaConf = match serde_json::from_value(body.config.clone()) {
		Ok(v) => v,
		Err(e) => return err_response(actix_web::http::StatusCode::BAD_REQUEST, "invalid_ai_persona_config", e),
	};

	let _operation = match crate::profile_store::lifecycle::lock_config_write(&source_path) {
		Ok(lock) => lock,
		Err(error) => return err_response(actix_web::http::StatusCode::CONFLICT, "profile_busy", error),
	};
	let original = match std::fs::read_to_string(&source_path) {
		Ok(v) => v,
		Err(e) => return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "read_failed", e),
	};
	if !revision_matches(&source_path, &original, body.expected_revision.as_deref()) {
		return err_response(actix_web::http::StatusCode::CONFLICT, "config_changed", "設定または保存先が変更されています。入力を確認してから再試行してください。");
	}
	let mut doc = match original.parse::<DocumentMut>() {
		Ok(v) => v,
		Err(e) => return err_response(actix_web::http::StatusCode::BAD_REQUEST, "invalid_toml", e),
	};
	let table = match persona_to_table(&persona) {
		Ok(v) => v,
		Err(e) => return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "serialize_failed", e),
	};
	let personas = match ai_personas_array_mut(&mut doc) {
		Ok(v) => v,
		Err(r) => return r,
	};
	if index >= personas.len() {
		return err_response(
			actix_web::http::StatusCode::NOT_FOUND,
			"ai_persona_not_found",
			format!("ai.personas[{index}] が存在しません。"),
		);
	}
	personas.replace(index, table);

	let updated_text = doc.to_string();
	if let Err(e) = toml::from_str::<Conf>(&updated_text) {
		return err_response(
			actix_web::http::StatusCode::BAD_REQUEST,
			"invalid_conf_after_update",
			format!("conf として不正です: {e}"),
		);
	}

	let backup = match make_backup(&source_path) {
		Ok(v) => v,
		Err(e) => return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "backup_failed", e),
	};
	if let Err(e) = crate::profile_store::write_config_transaction(&source_path, updated_text.as_bytes()) {
		return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "write_failed", e);
	}

	log::info!(
		"《ControlAPI/config》 ai.personas[{index}] を更新しました: {}",
		source_path.display()
	);
	HttpResponse::Ok().json(NodePutConfigResponse {
		index,
		source_path: source_path.display().to_string(),
		backup,
		warning: "このファイルは現在読み込まれているプロファイルです。再起動しないと完全には反映されません。".to_string(),
	})
}

async fn conf_source_path(state: &SharedState) -> Option<PathBuf> {
	state.read().await.conf_source_path.clone()
}

fn config_revision(path: &Path, content: &str) -> String {
	let mut hash = Sha256::new();
	hash.update(path.as_os_str().as_encoded_bytes());
	hash.update([0]);
	hash.update(content.as_bytes());
	hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn revision_matches(path: &Path, content: &str, expected: Option<&str>) -> bool {
	// Legacy API clients may omit the precondition. The GUI always supplies it.
	expected.is_none_or(|value| value == config_revision(path, content))
}

#[cfg(test)]
mod revision_tests {
	use super::*;
	#[test]
	fn rejects_changed_content_and_different_source() {
		let path = Path::new("profile-a/conf.toml");
		let original = "[[ai.personas]]\nid = 'a'\n";
		let revision = config_revision(path, original);
		assert!(revision_matches(path, original, Some(&revision)));
		assert!(!revision_matches(path, "[[ai.personas]]\nid = 'b'\n", Some(&revision)));
		assert!(!revision_matches(Path::new("profile-b/conf.toml"), original, Some(&revision)));
		assert!(!revision_matches(path, original, Some("")));
		assert!(revision_matches(path, original, None));
	}

	#[actix_web::test]
	async fn http_save_rejects_stale_revision_and_preserves_other_persona() {
		use actix_web::{test as http, App};
		use std::sync::Arc;
		let root = std::env::temp_dir().join(format!("vac-persona-http-{}", rand::random::<u64>()));
		std::fs::create_dir_all(&root).unwrap();
		let path = root.join("conf.toml");
		let original = "workers = 2\n[[ai.personas]]\nid = 'first'\nis_enabled = false\n[[ai.personas]]\nid = 'second'\nis_enabled = false\nsystem_instructions_extra = 'retain second'\n";
		std::fs::write(&path, original).unwrap();
		// Empty runtime: no AI, Twitch, avatar links or Flowgraph workers are configured.
		let mut conf: Conf = toml::from_str("").unwrap();
		conf.source_path = Some(path.clone());
		conf.runtime_dir = Some(root.join("runtime"));
		let audio = Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
		let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
		let app = http::init_service(App::new().app_data(Data::new(state.clone())).configure(configure).configure(super::super::profiles::configure)).await;
		let read = http::call_service(&app, http::TestRequest::get().uri("/ai_personas/0/config").to_request()).await;
		assert!(read.status().is_success());
		let first: JsonValue = http::read_body_json(read).await;
		let mut edited = first["config"].clone();
		edited["system_instructions_extra"] = JsonValue::String("saved first".into());
		let request = serde_json::json!({"config":edited,"expected_revision":first["revision"]});
		let external = original.replace("workers = 2", "workers = 3");
		std::fs::write(&path, &external).unwrap();
		let rejected = http::call_service(&app, http::TestRequest::put().uri("/ai_personas/0/config").set_json(&request).to_request()).await;
		assert_eq!(rejected.status(), actix_web::http::StatusCode::CONFLICT);
		assert_eq!(std::fs::read_to_string(&path).unwrap(), external);
		assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains(".bak-")));
		let latest: JsonValue = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/ai_personas/0/config").to_request()).await;
		#[cfg(windows)]
		{
			use std::os::windows::fs::OpenOptionsExt;
			// Permit reads/writes but deny replacement: the staged writer must fail
			// without falling back to truncating the live configuration.
			let held = std::fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2).open(&path).unwrap();
			let failed = http::call_service(&app, http::TestRequest::put().uri("/ai_personas/0/config")
				.set_json(serde_json::json!({"config":edited,"expected_revision":latest["revision"]})).to_request()).await;
			assert_eq!(failed.status(), actix_web::http::StatusCode::INTERNAL_SERVER_ERROR);
			let failure: JsonValue = http::read_body_json(failed).await;
			assert_eq!(failure["error"], "write_failed");
			assert_eq!(std::fs::read_to_string(&path).unwrap(), external);
			assert!(!std::fs::read_dir(&root).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains(".unvac-tmp-")));
			drop(held);
		}
		let saved = http::call_service(&app, http::TestRequest::put().uri("/ai_personas/0/config")
			.set_json(serde_json::json!({"config":edited,"expected_revision":latest["revision"]})).to_request()).await;
		assert!(saved.status().is_success());
		let saved: JsonValue = http::read_body_json(saved).await;
		assert_eq!(std::fs::read_to_string(root.join(saved["backup"].as_str().unwrap())).unwrap(), external);
		let stored: Conf = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
		assert_eq!(serde_json::to_value(&stored.ai.personas[0]).unwrap()["system_instructions_extra"], "saved first");
		assert_eq!(serde_json::to_value(&stored.ai.personas[1]).unwrap()["system_instructions_extra"], "retain second");
		assert!(std::fs::read_to_string(&path).unwrap().contains("workers = 3"));
		// Reopen through the production loader and HTTP read path, rather than
		// treating a successful write response as evidence of readable settings.
		let reopened = Conf::new_noop_probe(path.clone()).unwrap();
		assert_eq!(serde_json::to_value(&reopened.ai.personas).unwrap(), serde_json::to_value(&stored.ai.personas).unwrap());
		let reread: JsonValue = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/ai_personas/0/config").to_request()).await;
		assert_eq!(reread["config"]["system_instructions_extra"], "saved first");
		assert_ne!(reread["revision"], latest["revision"]);
		assert!(saved["warning"].as_str().unwrap().contains("再起動"));
		// Force a same-second collision and exercise the public recovery API for
		// both snapshots, including a pre-existing backup that must not change.
		let first_backup = make_backup_at(&path, "20261018000000").unwrap();
		let first_content = std::fs::read_to_string(&path).unwrap();
		std::fs::write(&path, &external).unwrap();
		let second_backup = make_backup_at(&path, "20261018000000").unwrap();
		assert_ne!(first_backup, second_backup);
		assert_eq!(std::fs::read_to_string(root.join(&first_backup)).unwrap(), first_content);
		let listed: JsonValue = http::call_and_read_body_json(&app, http::TestRequest::get().uri("/profile-backups").to_request()).await;
		for (backup, filename, content) in [(&first_backup, "restored-first.toml", &first_content), (&second_backup, "restored-second.toml", &external)] {
			assert!(listed["entries"].as_array().unwrap().iter().any(|entry| entry["filename"] == *backup));
			let response = http::call_service(&app, http::TestRequest::post().uri("/profile-backups/restore")
				.set_json(serde_json::json!({"backup":backup,"filename":filename})).to_request()).await;
			assert!(response.status().is_success());
			assert_eq!(std::fs::read_to_string(root.join(filename)).unwrap(), *content);
		}
		assert_eq!(std::fs::read_to_string(&path).unwrap(), external);
		let other = root.join("other.toml");
		std::fs::write(&other, &external).unwrap();
		state.write().await.conf_source_path = Some(other.clone());
		let changed_source = http::call_service(&app, http::TestRequest::put().uri("/ai_personas/0/config")
			.set_json(serde_json::json!({"config":edited,"expected_revision":latest["revision"]})).to_request()).await;
		assert_eq!(changed_source.status(), actix_web::http::StatusCode::CONFLICT);
		assert_eq!(std::fs::read_to_string(other).unwrap(), external);
	}
}

fn ai_personas_array_mut(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables, HttpResponse> {
	match &mut doc["ai"]["personas"] {
		Item::ArrayOfTables(aot) => Ok(aot),
		Item::None => Err(err_response(
			actix_web::http::StatusCode::NOT_FOUND,
			"ai_personas_not_found",
			"conf.toml に [[ai.personas]] が存在しません。",
		)),
		_ => Err(err_response(
			actix_web::http::StatusCode::BAD_REQUEST,
			"invalid_ai_personas_shape",
			"conf.toml の ai.personas が array-of-tables ではありません。",
		)),
	}
}

fn persona_to_table(persona: &AiPersonaConf) -> Result<Table, String> {
	let text = toml::to_string(persona).map_err(|e| e.to_string())?;
	let doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
	Ok(doc.as_table().clone())
}

fn make_backup(path: &Path) -> std::io::Result<String> {
	let ts = jiff::Timestamp::now().strftime("%Y%m%d%H%M%S").to_string();
	make_backup_at(path, &ts)
}

fn make_backup_at(path: &Path, stamp: &str) -> std::io::Result<String> {
	let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("conf.toml");
	let mut input = std::fs::File::open(path)?;
	for suffix in 0..10000 {
		let backup_name = if suffix == 0 { format!("{file_name}.bak-{stamp}") } else { format!("{file_name}.bak-{stamp}-{suffix:04}") };
		let backup_path = path.with_file_name(&backup_name);
		let mut output = match std::fs::OpenOptions::new().write(true).create_new(true).open(&backup_path) {
			Ok(file) => file,
			Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
			Err(error) => return Err(error),
		};
		if let Err(error) = std::io::copy(&mut input, &mut output).and_then(|_| output.sync_all()) {
			drop(output);
			let _ = std::fs::remove_file(&backup_path);
			return Err(error);
		}
		return Ok(backup_name);
	}
	Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "この時刻のバックアップ名を確保できませんでした"))
}

fn err_response(status: actix_web::http::StatusCode, error: impl ToString, message: impl ToString) -> HttpResponse {
	HttpResponse::build(status).json(serde_json::json!({
		"error": error.to_string(),
		"message": message.to_string(),
	}))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(get_ai_persona_config).service(put_ai_persona_config);
}

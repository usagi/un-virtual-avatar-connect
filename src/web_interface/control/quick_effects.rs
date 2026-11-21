use actix_web::web::{self, Data, Json, Path, Query};
use actix_web::{get, post, put, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::path::{Path as FsPath, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{sleep, Duration};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table};

use crate::conf::{Conf, QuickEffectPresetConf};
use crate::quick_effects::{QuickEffectItem, QuickEffectStatus, QUICK_EFFECT_QUEUE_CAPACITY};
use crate::state::DataSource;
use crate::{ChannelDatum, SharedState};

#[derive(Debug, Deserialize)]
pub struct QuickEffectsQuery {
	#[serde(default = "default_limit")]
	pub limit: usize,
}

#[derive(Debug, Deserialize)]
pub struct QuickEffectEnqueueRequest {
	pub label: String,
	pub channel: String,
	pub content: String,
	#[serde(default)]
	pub flags: Vec<String>,
	#[serde(default)]
	pub duration_ms: Option<u64>,
	#[serde(default)]
	pub fire_at_ms: Option<u128>,
	#[serde(default)]
	pub meta: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
pub struct QuickEffectMoveRequest {
	pub direction: String,
	#[serde(default)]
	pub to_index: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct QuickEffectsResponse {
	pub effects: Vec<QuickEffectItem>,
	pub limit: usize,
	pub capacity: usize,
}

#[derive(Debug, Deserialize)]
pub struct QuickEffectPresetsPutRequest {
	#[serde(default)]
	pub expected_revision: Option<String>,
	pub presets: Vec<QuickEffectPresetConf>,
}

#[derive(Debug, Serialize)]
pub struct QuickEffectPresetsResponse {
	pub revision: Option<String>,
	pub presets: Vec<QuickEffectPresetConf>,
	pub source_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub backup: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub warning: Option<String>,
}

fn preset_config_revision(path: &FsPath, content: &str) -> String {
	let mut hash = Sha256::new();
	hash.update(path.as_os_str().as_encoded_bytes());
	hash.update([0]);
	hash.update(content.as_bytes());
	hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

#[get("/quick_effects")]
pub async fn get_quick_effects(state: Data<SharedState>, query: Query<QuickEffectsQuery>) -> impl Responder {
	let limit = query.limit.clamp(1, QUICK_EFFECT_QUEUE_CAPACITY);
	let queue = {
		let s = state.read().await;
		s.quick_effect_queue.clone()
	};
	let now_ms = now_epoch_ms();
	let q = queue.read().await;
	let mut effects: Vec<_> = q
		.iter()
		.rev()
		.take(limit)
		.cloned()
		.map(|item| item.with_effective_status(now_ms))
		.collect();
	effects.reverse();
	HttpResponse::Ok().json(QuickEffectsResponse {
		effects,
		limit,
		capacity: QUICK_EFFECT_QUEUE_CAPACITY,
	})
}

#[get("/quick_effects/presets")]
pub async fn get_quick_effect_presets(state: Data<SharedState>) -> impl Responder {
	let source_path = conf_source_path(&state).await;
	let mut revision = None;
	let presets = if let Some(path) = source_path.as_ref() {
		let read = std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|text| {
			let conf = toml::from_str::<Conf>(&text).map_err(|e| e.to_string())?;
			revision = Some(preset_config_revision(path, &text));
			Ok(conf)
		});
		match read {
			Ok(conf) => effective_presets(conf.control_api.as_ref()),
			Err(e) => {
				return HttpResponse::BadRequest().json(serde_json::json!({
					"error": "conf_load_failed",
					"message": e.to_string(),
				}));
			}
		}
	} else {
		default_presets()
	};
	HttpResponse::Ok().json(QuickEffectPresetsResponse {
		revision,
		presets,
		source_path: source_path.map(|p| p.display().to_string()),
		backup: None,
		warning: None,
	})
}

#[put("/quick_effects/presets")]
pub async fn put_quick_effect_presets(state: Data<SharedState>, body: Json<QuickEffectPresetsPutRequest>) -> impl Responder {
	let Some(source_path) = conf_source_path(&state).await else {
		return HttpResponse::InternalServerError().json(serde_json::json!({
			"error": "conf_source_path_unset",
			"message": "current conf path is unknown (source_path is None)",
		}));
	};
	put_quick_effect_presets_at(source_path, body.into_inner())
}

fn put_quick_effect_presets_at(source_path: PathBuf, body: QuickEffectPresetsPutRequest) -> HttpResponse {
	let presets = normalize_presets(&body.presets);
	if presets.is_empty() {
		return HttpResponse::BadRequest().json(serde_json::json!({
			"error": "bad_request",
			"message": "at least one valid preset is required",
		}));
	}

	let _operation = match crate::profile_store::lifecycle::lock_config_write(&source_path) {
		Ok(lock) => lock,
		Err(e) => return HttpResponse::Conflict().json(serde_json::json!({"error":"profile_busy","message":e.to_string()})),
	};
	let original = match std::fs::read_to_string(&source_path) {
		Ok(v) => v,
		Err(e) => return HttpResponse::InternalServerError().json(serde_json::json!({ "error": "read_failed", "message": e.to_string() })),
	};
	if body.expected_revision.as_deref().is_some_and(|expected| expected != preset_config_revision(&source_path, &original)) {
		return HttpResponse::Conflict().json(serde_json::json!({"error":"config_changed","message":"設定または保存先が変更されています。再読み込みして内容を確認してください。"}));
	}
	let mut doc = match original.parse::<DocumentMut>() {
		Ok(v) => v,
		Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": "invalid_toml", "message": e.to_string() })),
	};
	let aot = match presets_to_array_of_tables(&presets) {
		Ok(v) => v,
		Err(e) => return HttpResponse::InternalServerError().json(serde_json::json!({ "error": "serialize_failed", "message": e })),
	};
	let control_api = match ensure_control_api_table(&mut doc) {
		Ok(table) => table,
		Err(message) => return HttpResponse::BadRequest().json(serde_json::json!({"error":"invalid_control_api","message":message})),
	};
	control_api["quick_effect_presets"] = Item::ArrayOfTables(aot);
	let updated_text = doc.to_string();
	if let Err(e) = toml::from_str::<Conf>(&updated_text) {
		return HttpResponse::BadRequest().json(serde_json::json!({
			"error": "invalid_conf_after_update",
			"message": format!("conf として不正です: {e}"),
		}));
	}
	let backup = match make_backup(&source_path) {
		Ok(v) => v,
		Err(e) => {
			return HttpResponse::InternalServerError().json(serde_json::json!({ "error": "backup_failed", "message": e.to_string() }))
		}
	};
	if let Err(e) = crate::profile_store::write_config_transaction(&source_path, updated_text.as_bytes()) {
		return HttpResponse::InternalServerError().json(serde_json::json!({ "error": "write_failed", "message": e.to_string() }));
	}
	HttpResponse::Ok().json(QuickEffectPresetsResponse {
		revision: Some(preset_config_revision(&source_path, &updated_text)),
		presets,
		source_path: Some(source_path.display().to_string()),
		backup: Some(backup),
		warning: Some("このファイルは現在読み込まれているプロファイルです。再起動後も同じ preset が使われます。".to_string()),
	})
}

#[post("/quick_effects")]
pub async fn post_quick_effect(state: Data<SharedState>, body: Json<QuickEffectEnqueueRequest>) -> impl Responder {
	let req = body.into_inner();
	let label = req.label.trim();
	let channel = req.channel.trim();
	if label.is_empty() || channel.is_empty() {
		return HttpResponse::BadRequest().json(serde_json::json!({
			"error": "bad_request",
			"reason": "label and channel are required",
		}));
	}
	let duration_ms = req.duration_ms.filter(|duration| *duration > 0);
	let now_ms = now_epoch_ms();
	let fire_at_ms = req.fire_at_ms.filter(|fire_at| *fire_at > now_ms);
	let cd = build_quick_effect_datum(label, channel, &req.content, duration_ms, &req.flags, req.meta.clone());
	let datum_id = cd.get_id();
	let item = QuickEffectItem {
		id: format!("qe-{datum_id}"),
		label: label.to_string(),
		channel: channel.to_string(),
		content: req.content,
		flags: normalized_flags(&req.flags),
		created_at: crate::datetime::DateTime::now().to_rfc3339(),
		created_at_ms: now_ms,
		fire_at_ms,
		fired_at_ms: if fire_at_ms.is_some() { None } else { Some(now_ms) },
		duration_ms,
		expires_at_ms: fire_at_ms
			.or(Some(now_ms))
			.and_then(|base| duration_ms.map(|duration| base + duration as u128)),
		status: if fire_at_ms.is_some() {
			QuickEffectStatus::Scheduled
		} else {
			QuickEffectStatus::Active
		},
		datum_id,
		meta: req.meta.unwrap_or_else(|| JsonValue::Object(Default::default())),
	};
	let queue = {
		let s = state.read().await;
		if fire_at_ms.is_none() {
			s.push_channel_datum(cd.clone()).await;
		}
		s.quick_effect_queue.clone()
	};
	{
		let mut q = queue.write().await;
		if q.len() >= QUICK_EFFECT_QUEUE_CAPACITY {
			q.pop_front();
		}
		q.push_back(item.clone());
	}
	if let Some(fire_at_ms) = fire_at_ms {
		spawn_scheduled_fire(state.get_ref().clone(), queue, item.id.clone(), fire_at_ms, duration_ms, cd);
	}
	HttpResponse::Ok().json(item)
}

#[post("/quick_effects/{id}/cancel")]
pub async fn post_quick_effect_cancel(state: Data<SharedState>, id: Path<String>) -> impl Responder {
	let id = id.into_inner();
	let queue = {
		let s = state.read().await;
		s.quick_effect_queue.clone()
	};
	let mut q = queue.write().await;
	let Some(item) = q.iter_mut().find(|item| item.id == id) else {
		return HttpResponse::NotFound().json(serde_json::json!({
			"error": "not_found",
			"id": id,
		}));
	};
	item.status = QuickEffectStatus::Cancelled;
	HttpResponse::Ok().json(item.clone().with_effective_status(now_epoch_ms()))
}

#[post("/quick_effects/{id}/move")]
pub async fn post_quick_effect_move(state: Data<SharedState>, id: Path<String>, body: Json<QuickEffectMoveRequest>) -> impl Responder {
	let id = id.into_inner();
	let req = body.into_inner();
	let queue = {
		let s = state.read().await;
		s.quick_effect_queue.clone()
	};
	let mut q = queue.write().await;
	let Some(index) = q.iter().position(|item| item.id == id) else {
		return HttpResponse::NotFound().json(serde_json::json!({
			"error": "not_found",
			"id": id,
		}));
	};
	let len = q.len();
	let target = match req.direction.as_str() {
		"up" => index.saturating_sub(1),
		"down" => (index + 1).min(len.saturating_sub(1)),
		"top" => 0,
		"bottom" => len.saturating_sub(1),
		"index" => req.to_index.unwrap_or(index).min(len.saturating_sub(1)),
		_ => {
			return HttpResponse::BadRequest().json(serde_json::json!({
				"error": "bad_request",
				"reason": "direction must be up, down, top, bottom, or index",
			}));
		}
	};
	if target != index {
		let Some(item) = q.remove(index) else {
			return HttpResponse::InternalServerError().finish();
		};
		q.insert(target, item);
	}
	let effects = q
		.iter()
		.cloned()
		.map(|item| item.with_effective_status(now_epoch_ms()))
		.collect::<Vec<_>>();
	HttpResponse::Ok().json(QuickEffectsResponse {
		effects,
		limit: len,
		capacity: QUICK_EFFECT_QUEUE_CAPACITY,
	})
}

fn build_quick_effect_datum(
	label: &str,
	channel: &str,
	content: &str,
	duration_ms: Option<u64>,
	flags: &[String],
	meta: Option<JsonValue>,
) -> ChannelDatum {
	let mut cd = ChannelDatum::new(channel.to_string(), content.to_string())
		.with_flag(ChannelDatum::FLAG_IS_FINAL)
		.with_meta("quick_effect_label", label)
		.with_meta("quick_effect_duration_ms", duration_ms.unwrap_or(0));
	for flag in flags.iter().map(|flag| flag.trim()).filter(|flag| !flag.is_empty()) {
		cd.flags.insert(flag.to_string());
	}
	cd.source = Some(DataSource::new("control.quick_effect").with_actor("gui"));
	if let Some(JsonValue::Object(meta)) = meta {
		for (key, value) in meta {
			cd.meta.insert(key, value);
		}
	}
	cd
}

fn normalized_flags(flags: &[String]) -> Vec<String> {
	flags
		.iter()
		.map(|flag| flag.trim())
		.filter(|flag| !flag.is_empty())
		.map(str::to_string)
		.collect()
}

fn spawn_scheduled_fire(
	state: SharedState,
	queue: std::sync::Arc<tokio::sync::RwLock<std::collections::VecDeque<QuickEffectItem>>>,
	id: String,
	fire_at_ms: u128,
	duration_ms: Option<u64>,
	cd: ChannelDatum,
) {
	tokio::spawn(async move {
		let now = now_epoch_ms();
		if fire_at_ms > now {
			sleep(Duration::from_millis((fire_at_ms - now).min(u128::from(u64::MAX)) as u64)).await;
		}
		{
			let q = queue.read().await;
			if q.iter().any(|item| item.id == id && item.status == QuickEffectStatus::Cancelled) {
				return;
			}
		}
		state.read().await.push_channel_datum(cd).await;
		let fired_at_ms = now_epoch_ms();
		let mut q = queue.write().await;
		if let Some(item) = q.iter_mut().find(|item| item.id == id) {
			if item.status != QuickEffectStatus::Cancelled {
				item.status = QuickEffectStatus::Active;
				item.fired_at_ms = Some(fired_at_ms);
				item.expires_at_ms = duration_ms.map(|duration| fired_at_ms + duration as u128);
			}
		}
	});
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(get_quick_effect_presets)
		.service(put_quick_effect_presets)
		.service(get_quick_effects)
		.service(post_quick_effect)
		.service(post_quick_effect_cancel)
		.service(post_quick_effect_move);
}

fn default_limit() -> usize {
	50
}

fn now_epoch_ms() -> u128 {
	SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

async fn conf_source_path(state: &SharedState) -> Option<PathBuf> {
	state.read().await.conf_source_path.clone()
}

fn effective_presets(control_api: Option<&crate::conf::ControlApiConf>) -> Vec<QuickEffectPresetConf> {
	control_api
		.map(|conf| normalize_presets(&conf.quick_effect_presets))
		.filter(|presets| !presets.is_empty())
		.unwrap_or_else(default_presets)
}

fn default_presets() -> Vec<QuickEffectPresetConf> {
	vec![
		QuickEffectPresetConf {
			id: "raid".into(),
			label: "RAID".into(),
			channel: "effect".into(),
			content: "RAID incoming".into(),
			flags: vec!["obs.raid".into()],
			duration_ms: Some(8000),
		},
		QuickEffectPresetConf {
			id: "chat_highlight".into(),
			label: "チャット強調".into(),
			channel: "chat".into(),
			content: "highlight".into(),
			flags: vec!["obs.chat.highlight".into()],
			duration_ms: Some(5000),
		},
		QuickEffectPresetConf {
			id: "scene_flash".into(),
			label: "画面効果".into(),
			channel: "effect".into(),
			content: "scene flash".into(),
			flags: vec!["obs.effect.flash".into()],
			duration_ms: Some(2500),
		},
		QuickEffectPresetConf {
			id: "bgm_fade".into(),
			label: "BGM fade".into(),
			channel: "bgm".into(),
			content: "fade".into(),
			flags: vec!["audio.bgm.fade".into()],
			duration_ms: Some(12000),
		},
	]
}

fn normalize_presets(presets: &[QuickEffectPresetConf]) -> Vec<QuickEffectPresetConf> {
	let mut out = Vec::new();
	for preset in presets {
		let id = preset.id.trim();
		let label = preset.label.trim();
		let channel = preset.channel.trim();
		if id.is_empty() || label.is_empty() || channel.is_empty() {
			continue;
		}
		out.push(QuickEffectPresetConf {
			id: id.to_string(),
			label: label.to_string(),
			channel: channel.to_string(),
			content: preset.content.clone(),
			flags: normalized_flags(&preset.flags),
			duration_ms: preset.duration_ms.filter(|duration| *duration > 0),
		});
	}
	out
}

fn presets_to_array_of_tables(presets: &[QuickEffectPresetConf]) -> Result<ArrayOfTables, String> {
	let mut aot = ArrayOfTables::new();
	for preset in presets {
		aot.push(preset_to_table(preset)?);
	}
	Ok(aot)
}

fn preset_to_table(preset: &QuickEffectPresetConf) -> Result<Table, String> {
	let text = toml::to_string(preset).map_err(|e| e.to_string())?;
	let doc = text.parse::<DocumentMut>().map_err(|e| e.to_string())?;
	Ok(doc.as_table().clone())
}

fn ensure_control_api_table(doc: &mut DocumentMut) -> Result<&mut Table, &'static str> {
	if !doc.contains_key("control_api") {
		doc["control_api"] = Item::Table(Table::new());
	}
	if let Some(inline) = doc["control_api"].as_inline_table() {
		doc["control_api"] = Item::Table(inline.clone().into_table());
	}
	doc["control_api"].as_table_mut().ok_or("control_api must be a table")
}

fn make_backup(path: &FsPath) -> std::io::Result<String> {
	let ts = jiff::Timestamp::now().strftime("%Y%m%d%H%M%S").to_string();
	make_backup_at(path, &ts)
}

fn make_backup_at(path: &FsPath, ts: &str) -> std::io::Result<String> {
	let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("conf.toml");
	let mut input = std::fs::File::open(path)?;
	for suffix in 0..10000 {
		let backup_name = if suffix == 0 { format!("{file_name}.bak-{ts}") } else { format!("{file_name}.bak-{ts}-{suffix}") };
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
	Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "backup filename limit reached"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn preset_save_preserves_inline_settings_and_rejects_wrong_table_type() {
		let root = std::env::temp_dir().join(format!("vac-preset-inline-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
		std::fs::create_dir_all(&root).unwrap();
		let path = root.join("inline.toml");
		let original = "workers = 2\ncontrol_api = { require_token_for_loopback = true }\n";
		let _: Conf = toml::from_str(original).unwrap();
		std::fs::write(&path, original).unwrap();
		let response = put_quick_effect_presets_at(path.clone(), QuickEffectPresetsPutRequest { expected_revision: Some(preset_config_revision(&path, original)), presets: default_presets() });
		assert_eq!(response.status(), actix_web::http::StatusCode::OK);
		let conf: Conf = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
		assert_eq!(conf.workers, Some(2));
		let api = conf.control_api.unwrap();
		assert!(api.require_token_for_loopback);
		assert_eq!(api.quick_effect_presets.len(), 4);
		let invalid = root.join("invalid.toml");
		let original = "control_api = 42\n";
		std::fs::write(&invalid, original).unwrap();
		let count = std::fs::read_dir(&root).unwrap().count();
		let response = put_quick_effect_presets_at(invalid.clone(), QuickEffectPresetsPutRequest { expected_revision: Some(preset_config_revision(&invalid, original)), presets: default_presets() });
		assert_eq!(response.status(), actix_web::http::StatusCode::BAD_REQUEST);
		assert_eq!(std::fs::read_to_string(invalid).unwrap(), original);
		assert_eq!(std::fs::read_dir(&root).unwrap().count(), count);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn successive_backups_preserve_each_saved_state() {
		let root = std::env::temp_dir().join(format!("vac-backup-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
		std::fs::create_dir_all(&root).unwrap();
		let path = root.join("conf.toml");
		std::fs::write(&path, "first state").unwrap();
		let first = make_backup_at(&path, "20261019000000").unwrap();
		std::fs::write(&path, "second state").unwrap();
		let second = make_backup_at(&path, "20261019000000").unwrap();
		assert_ne!(first, second);
		assert_eq!(std::fs::read_to_string(root.join(first)).unwrap(), "first state");
		assert_eq!(std::fs::read_to_string(root.join(second)).unwrap(), "second state");
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn preset_file_save_rejects_stale_revision_and_wrong_destination() {
		let root = std::env::temp_dir().join(format!("vac-preset-conflict-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
		std::fs::create_dir_all(&root).unwrap();
		let path = root.join("conf.toml");
		let original = "# unrelated setting must survive\n[control_api]\nbind = '127.0.0.1:46000'\n";
		std::fs::write(&path, original).unwrap();
		let revision = preset_config_revision(&path, original);
		let save = |destination: PathBuf, revision: String, label: &str| {
			let mut presets = default_presets();
			presets[0].label = label.into();
			put_quick_effect_presets_at(destination, QuickEffectPresetsPutRequest { expected_revision: Some(revision), presets })
		};
		assert_eq!(save(path.clone(), revision.clone(), "First").status(), actix_web::http::StatusCode::OK);
		let saved = std::fs::read_to_string(&path).unwrap();
		let snapshot = || {
			let mut files: Vec<_> = std::fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().path())
				.filter(|path| path.file_name().unwrap() != ".unvac-config-write.lock").map(|path| {
				(path.file_name().unwrap().to_owned(), std::fs::read(path).unwrap())
			}).collect();
			files.sort();
			files
		};
		let before = snapshot();
		let guard = crate::profile_store::lifecycle::lock_config_write(&path).unwrap();
		assert_eq!(save(path.clone(), preset_config_revision(&path, &saved), "Busy").status(), actix_web::http::StatusCode::CONFLICT);
		assert_eq!(snapshot(), before, "busy save must not alter files or create backups");
		drop(guard);
		assert_eq!(save(path.clone(), revision, "Stale").status(), actix_web::http::StatusCode::CONFLICT);
		assert_eq!(snapshot(), before, "rejected save must not alter files or backups");
		let other = root.join("other.toml");
		std::fs::write(&other, &saved).unwrap();
		let before = snapshot();
		assert_eq!(save(other, preset_config_revision(&path, &saved), "Wrong target").status(), actix_web::http::StatusCode::CONFLICT);
		assert_eq!(snapshot(), before);
		assert_eq!(save(path.clone(), preset_config_revision(&path, &saved), "After reload").status(), actix_web::http::StatusCode::OK);
		let final_text = std::fs::read_to_string(&path).unwrap();
		assert!(final_text.contains("# unrelated setting must survive"));
		assert!(final_text.contains("127.0.0.1:46000"));
		let conf: Conf = toml::from_str(&final_text).unwrap();
		assert_eq!(effective_presets(conf.control_api.as_ref())[0].label, "After reload");
		std::fs::remove_dir_all(&root).unwrap();
	}

	#[test]
	fn preset_revision_tracks_both_content_and_destination() {
		let a = FsPath::new("profile-a/conf.toml");
		let b = FsPath::new("profile-b/conf.toml");
		assert_eq!(preset_config_revision(a, "name='one'"), preset_config_revision(a, "name='one'"));
		assert_ne!(preset_config_revision(a, "name='one'"), preset_config_revision(a, "name='two'"));
		assert_ne!(preset_config_revision(a, "name='one'"), preset_config_revision(b, "name='one'"));
	}

	#[test]
	fn normalize_presets_trims_and_drops_invalid_entries() {
		let presets = normalize_presets(&[
			QuickEffectPresetConf {
				id: " raid ".into(),
				label: " RAID ".into(),
				channel: " effect ".into(),
				content: "incoming".into(),
				flags: vec![" obs.raid ".into(), "".into(), "audio.hit".into()],
				duration_ms: Some(8000),
			},
			QuickEffectPresetConf {
				id: "".into(),
				label: "bad".into(),
				channel: "effect".into(),
				content: "bad".into(),
				flags: vec![],
				duration_ms: Some(0),
			},
		]);

		assert_eq!(presets.len(), 1);
		assert_eq!(presets[0].id, "raid");
		assert_eq!(presets[0].label, "RAID");
		assert_eq!(presets[0].channel, "effect");
		assert_eq!(presets[0].flags, vec!["obs.raid", "audio.hit"]);
		assert_eq!(presets[0].duration_ms, Some(8000));
	}

	#[test]
	fn presets_to_array_of_tables_renders_conf_shape() {
		let presets = vec![QuickEffectPresetConf {
			id: "stinger".into(),
			label: "Stinger".into(),
			channel: "effect".into(),
			content: "sting".into(),
			flags: vec!["obs.stinger".into()],
			duration_ms: Some(5000),
		}];

		let mut doc = DocumentMut::new();
		doc["control_api"] = Item::Table(Table::new());
		doc["control_api"]["quick_effect_presets"] = Item::ArrayOfTables(presets_to_array_of_tables(&presets).expect("serialize"));
		let rendered = doc.to_string();

		assert!(rendered.contains("[[control_api.quick_effect_presets]]"));
		assert!(rendered.contains("id = \"stinger\""));
		assert!(rendered.contains("duration_ms = 5000"));
	}
}

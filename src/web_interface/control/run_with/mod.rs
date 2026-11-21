//! Phase VI-γ-5a: `run_with` 配列のエントリを GUI から追加・削除する API。
//!
//! 主眼: **既存 `conf.toml` のコメント・書式をできる限り温存したまま**、
//! `run_with = [ ... ]` のインライン配列末尾に追加 / 指定 index を削除する。
//! そのために全体の `Conf` 往復（→ serde serialize）は避け、[`toml_edit`] の
//! `DocumentMut` で該当部分だけをピンポイント操作する。
//!
//! ## 設計
//!
//! - `GET /run_with` — 現 **メモリ上** の `Conf::run_with` を GUI 向け DTO に変換して返す。
//!   effective_id / display_label / supports_status などの派生情報を付ける。
//! - `POST /run_with` — 末尾に 1 entry 追加。書き込み前にサーバ側で `toml::from_str::<Conf>()` で
//!   全体妥当性を検証し、失敗時はファイルを一切変更しない。
//! - `DELETE /run_with/{index}` — 指定 index を削除。
//!
//! ## 後処理
//!
//! 書き込み成功後は:
//!  1. `.bak-YYYYMMDD-HHmmSS` バックアップを生成（既存 conf.toml を退避コピー）
//!  2. 新ファイルをパースし直して `ManagedAppRegistry::from_conf` で registry を丸ごと再構築
//!  3. `probe_all` で初期 statuses を埋める
//!  4. `state.managed_apps` を置換
//!
//! `state.conf` の in-memory 更新は行わない（再起動まで他の設定と混ざるとバグりやすい）。
//! `run_with` は起動時に一度だけ使われる設計なので、disk と registry の整合が取れていれば十分。
//!
//! TOML 配列操作は [`toml_ops`]、ビュー構築は [`util`]。

mod batch;
mod toml_ops;
mod mode_actions;
mod util;

use actix_web::web::{self, Data, Json, Path};
use actix_web::{delete, get, post, put, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

use crate::conf::{Conf, RunWith};
use crate::managed_app::{self, ManagedAppRegistry};
use crate::SharedState;

use toml_ops::{dto_to_value, run_with_array};
use util::{build_views, err_response};

// ---- DTO ---------------------------------------------------------------------

/// GUI との JSON やり取りに使う `RunWith` の DTO。内部 `RunWith` とほぼ同義だが、
/// フィールドをフラットに出すため `untagged` 版を独立に用意。
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum RunWithDto {
	/// 単純な文字列形式（`"notepad"` や `"http://..."`）
	Simple(String),
	/// `{ command = "...", if_not_running = "...", ... }` 形式
	Table(RunWithTableDto),
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct RunWithTableDto {
	/// GUI-only patch, persisted under modes rather than run_with.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub mode_actions: Option<std::collections::BTreeMap<String, String>>,
	pub command: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub if_not_running: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub start_on_launch: Option<bool>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub run_as_admin: Option<bool>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub working_dir: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub minimized: Option<bool>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub label: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub unaa_z_app_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub unaa_z_instance_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub shutdown: Option<crate::conf::RunWithShutdownSpec>,
}

impl From<&RunWith> for RunWithDto {
	fn from(r: &RunWith) -> Self {
		match r {
			RunWith::Command(s) => RunWithDto::Simple(s.clone()),
			RunWith::CommandIfProcessIsNotRunning {
				command,
				if_not_running,
				start_on_launch,
				run_as_admin,
				working_dir,
				minimized,
				id,
				label,
				unaa_z_app_id,
				unaa_z_instance_id,
				shutdown,
			} => RunWithDto::Table(RunWithTableDto {
				mode_actions: None,
				command: command.clone(),
				if_not_running: if_not_running.clone(),
				start_on_launch: *start_on_launch,
				run_as_admin: *run_as_admin,
				working_dir: working_dir.clone(),
				minimized: *minimized,
				id: id.clone(),
				label: label.clone(),
				unaa_z_app_id: unaa_z_app_id.clone(),
				unaa_z_instance_id: unaa_z_instance_id.clone(),
				shutdown: shutdown.clone(),
			}),
		}
	}
}

/// GUI が扱いやすいよう派生情報を追加した 1 件の表示形。
#[derive(Debug, Serialize, Clone)]
pub struct RunWithView {
	pub modes: Vec<mode_actions::ModeActionView>,
	pub index: usize,
	/// 明示 `id` または `run-with-<index>` の自動値。
	pub effective_id: String,
	/// GUI 向け表示ラベル。
	pub display_label: String,
	/// Managed App として状態監視可能か（`if_not_running` が埋まっているか）。
	pub supports_status: bool,
	pub entry: RunWithDto,
}

#[derive(Debug, Serialize)]
pub struct RunWithListResponse {
	pub modes: Vec<mode_actions::ModeActionView>,
	pub revision: Option<String>,
	pub entries: Vec<RunWithView>,
	/// 書き込み先 conf ファイルのパス。GUI の UX（「このファイルに追記します」）のため。
	pub source_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RunWithMutationResponse {
	pub modes: Vec<mode_actions::ModeActionView>,
	pub revision: Option<String>,
	pub entries: Vec<RunWithView>,
	pub source_path: Option<String>,
	pub backup: Option<String>,
	pub warning: Option<String>,
}

// ---- GET ---------------------------------------------------------------------

#[get("/run_with")]
pub async fn get_run_with(state: Data<SharedState>) -> impl Responder {
	let path_opt = {
		let s = state.read().await;
		s.conf_source_path.clone()
	};
	let Some(path) = path_opt else {
		return HttpResponse::Ok().json(RunWithListResponse {
			modes: Vec::new(),
			entries: Vec::new(),
			revision: None,
			source_path: None,
		});
	};
	let original = match std::fs::read_to_string(&path) {
		Ok(text) => text,
		Err(e) => return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "read_failed", e),
	};
	match toml::from_str::<Conf>(&original) {
		Ok(c) => HttpResponse::Ok().json(RunWithListResponse {
			modes: mode_actions::definitions(&c),
			entries: mode_actions::views(&c),
			revision: Some(blake3::hash(original.as_bytes()).to_hex().to_string()),
			source_path: Some(path.display().to_string()),
		}),
		Err(e) => err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "conf_load_failed", e),
	}
}

// ---- 書き込み共通 ------------------------------------------------------------

/// `conf.toml` を load して `toml_edit` でいじり、書き戻し + managed_apps registry を更新する。
/// クロージャ側は `&mut DocumentMut` を受け取って `run_with` 配列を編集する。
async fn mutate_conf<F>(state: &SharedState, request: &HttpRequest, mutate: F) -> Result<RunWithMutationResponse, HttpResponse>
where
	F: FnOnce(&mut DocumentMut) -> Result<(), HttpResponse>,
{
	let source_path = {
		let s = state.read().await;
		s.conf_source_path.clone()
	};
	let Some(source_path) = source_path else {
		return Err(err_response(
			actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
			"conf_source_path_unset",
			"conf ファイルの出自が記録されていないので、run_with の書き換えはできません。",
		));
	};

	let _operation = crate::profile_store::lifecycle::lock_config_write(&source_path)
		.map_err(|error| err_response(actix_web::http::StatusCode::CONFLICT, "profile_busy", error))?;
	let original = std::fs::read_to_string(&source_path)
		.map_err(|e| err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "read_failed", e))?;

	check_revision(request, &original)?;
	let mut doc = original
		.parse::<DocumentMut>()
		.map_err(|e| err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "conf_toml_parse_failed", e))?;

	mutate(&mut doc)?;

	let updated_text = doc.to_string();

	// 書き込み前に完全な Conf としてパース検証。
	if let Err(e) = toml::from_str::<Conf>(&updated_text) {
		return Err(err_response(
			actix_web::http::StatusCode::BAD_REQUEST,
			"invalid_conf_after_edit",
			format!("編集後の conf が `Conf` としてパースできません: {e}"),
		));
	}

	let checked: Conf = toml::from_str(&updated_text).map_err(|e| err_response(actix_web::http::StatusCode::BAD_REQUEST,"invalid_conf_after_edit",e))?;
	crate::conf::runtime_mode::validate_runtime_modes(&checked).map_err(|e| err_response(actix_web::http::StatusCode::BAD_REQUEST,"invalid_mode_after_edit",e))?;

	// バックアップ。
	let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
	let bak_name = format!(
		"{}.bak-{}",
		source_path.file_name().and_then(|s| s.to_str()).unwrap_or("conf.toml"),
		ts
	);
	let bak_path = source_path.with_file_name(&bak_name);
	if let Err(e) = std::fs::copy(&source_path, &bak_path) {
		return Err(err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "backup_failed", format!("バックアップを作れないため保存しませんでした: {e}")));
	}

	if let Err(e) = crate::profile_store::write_config_transaction(&source_path, updated_text.as_bytes()) {
		return Err(err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "write_failed", e));
	}

	// 新 conf をパースし直して registry を再構築。
	let new_conf = match Conf::new_noop_probe(source_path.clone()) {
		Ok(c) => c,
		Err(e) => {
			return Err(err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "reload_failed", e));
		}
	};
	let mut new_registry = ManagedAppRegistry::from_conf(&new_conf);
	// 即時 probe で初期ステータスを埋める（monitor タスクの次 tick を待たない）。
	new_registry.statuses = managed_app::probe_all(&new_registry.specs);

	let entries = mode_actions::views(&new_conf);

	// state.managed_apps を置換。
	{
		let s = state.read().await;
		let reg = s.managed_apps.clone();
		drop(s);
		let mut w = reg.write().await;
		*w = new_registry;
	}

	log::info!(
		"《RunWithOps》 {} を更新し、managed_apps registry を再構築しました。",
		source_path.display()
	);

	Ok(RunWithMutationResponse {
		modes: mode_actions::definitions(&new_conf),
		revision: Some(blake3::hash(updated_text.as_bytes()).to_hex().to_string()),
		entries,
		source_path: Some(source_path.display().to_string()),
		backup: Some(bak_name),
		warning: Some(
			"run_with の変更を conf.toml に書き込みました。既に起動済みのプロセスは影響を受けません。\
     必要に応じて Managed Apps 画面から起動し直してください。"
				.to_string(),
		),
	})
}

// Optional for compatibility with existing API clients; the GUI always supplies this.
fn check_revision(request: &HttpRequest, original: &str) -> Result<(), HttpResponse> {
	if let Some(expected) = request.headers().get("If-Match") {
		let current = format!("b3:{}", blake3::hash(original.as_bytes()).to_hex());
		if expected.to_str().ok() != Some(current.as_str()) {
			return Err(err_response(actix_web::http::StatusCode::CONFLICT, "revision_conflict",
				"別の画面で設定が変更されています。入力内容を控え、一覧を更新して対象を選び直してください。"));
		}
	}
	Ok(())
}

// ---- POST / PUT / DELETE -----------------------------------------------------

#[put("/run_with/batch")]
pub async fn put_batch(request: HttpRequest, state: Data<SharedState>, body: Json<batch::Batch>) -> impl Responder {
	if !request.headers().contains_key("If-Match") {
		return err_response(actix_web::http::StatusCode::PRECONDITION_REQUIRED,"revision_required","保存前に現在の設定を読み込んでください。");
	}
	match mutate_conf(&state, &request, |doc| batch::apply(doc, &body)).await {
		Ok(response) => HttpResponse::Ok().json(response),
		Err(error) => error,
	}
}

#[post("/run_with")]
pub async fn post_add(request: HttpRequest, state: Data<SharedState>, body: Json<RunWithDto>) -> impl Responder {
	// 妥当性の軽い事前チェック: command が空だと後続の Conf パースが実用上通ってしまう恐れがある。
	if let RunWithDto::Table(t) = &*body {
		if t.command.trim().is_empty() {
			return err_response(actix_web::http::StatusCode::BAD_REQUEST, "empty_command", "command が空です。");
		}
	} else if let RunWithDto::Simple(s) = &*body {
		if s.trim().is_empty() {
			return err_response(actix_web::http::StatusCode::BAD_REQUEST, "empty_command", "command が空です。");
		}
	}

	let new_entry = body.into_inner();
	match mutate_conf(&state, &request, |doc| {
		let index = run_with_array(doc)?.len();
		run_with_array(doc)?.push(dto_to_value(&new_entry));
		mode_actions::patch(doc, index, &new_entry)?;
		Ok(())
	})
	.await
	{
		Ok(resp) => HttpResponse::Ok().json(resp),
		Err(r) => r,
	}
}

#[put("/run_with/{index}")]
pub async fn put_entry(request: HttpRequest, state: Data<SharedState>, index: Path<usize>, body: Json<RunWithDto>) -> impl Responder {
	let target_index = index.into_inner();
	let updated_entry = body.into_inner();
	let command = match &updated_entry {
		RunWithDto::Table(t) => t.command.as_str(),
		RunWithDto::Simple(s) => s.as_str(),
	};
	if command.trim().is_empty() {
		return err_response(actix_web::http::StatusCode::BAD_REQUEST, "empty_command", "command が空です。");
	}

	match mutate_conf(&state, &request, move |doc| {
		let arr = run_with_array(doc)?;
		if target_index >= arr.len() {
			return Err(err_response(
				actix_web::http::StatusCode::NOT_FOUND,
				"index_out_of_range",
				format!("index {target_index} は run_with[{}] の範囲外です。", arr.len()),
			));
		}
		let old_conf: Conf = toml::from_str(&doc.to_string()).map_err(|e| err_response(actix_web::http::StatusCode::BAD_REQUEST, "invalid_conf", e))?;
		let old_id = build_views(&old_conf.run_with)[target_index].effective_id.clone();
		run_with_array(doc)?.replace(target_index, dto_to_value(&updated_entry));
		mode_actions::preserve_id(doc, target_index, &old_id)?;
		mode_actions::patch(doc, target_index, &updated_entry)?;
		Ok(())
	})
	.await
	{
		Ok(resp) => HttpResponse::Ok().json(resp),
		Err(r) => r,
	}
}

#[delete("/run_with/{index}")]
pub async fn delete_entry(request: HttpRequest, state: Data<SharedState>, index: Path<usize>) -> impl Responder {
	let target_index = index.into_inner();
	match mutate_conf(&state, &request, move |doc| {
		let arr = run_with_array(doc)?;
		if target_index >= arr.len() {
			return Err(err_response(
				actix_web::http::StatusCode::NOT_FOUND,
				"index_out_of_range",
				format!("index {target_index} は run_with[{}] の範囲外です。", arr.len()),
			));
		}
		mode_actions::remove(doc, target_index)?;
		Ok(())
	})
	.await
	{
		Ok(resp) => HttpResponse::Ok().json(resp),
		Err(r) => r,
	}
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(get_run_with).service(post_add).service(put_batch).service(put_entry).service(delete_entry);
}

#[cfg(test)]
mod revision_tests {
 use super::*;
 #[test]
 fn stale_edit_is_rejected_before_index_mutation() {
  let original = "run_with = [\"first\", \"second\"]";
  let header = format!("b3:{}", blake3::hash(original.as_bytes()).to_hex());
  let request = actix_web::test::TestRequest::default().insert_header(("If-Match", header)).to_http_request();
  assert!(check_revision(&request, original).is_ok());
  let result = check_revision(&request, "run_with = [\"second\"]").unwrap_err();
  assert_eq!(result.status(), actix_web::http::StatusCode::CONFLICT);
 }
}

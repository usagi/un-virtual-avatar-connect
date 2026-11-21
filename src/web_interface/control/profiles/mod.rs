//! Phase VI-γ-4a: プロファイル（conf ファイル）操作 API。
//!
//! 既存の [`restart`](super::restart) の `GET /profiles` に対し、「複製」「リネーム」「削除」「内容取得」「内容更新」を追加する。
//!
//! ## 安全設計
//!
//! 全操作は **現 `conf_source_path` の親ディレクトリ内に限定**（chroot 的な意味で）。外部ディレクトリに
//! 出られないよう、入力はファイル名（basename）のみ受け付け、canonicalize 後に親ディレクトリの
//! 部分でなければ 400。拡張子は `.toml` に限定。
//!
//! 破壊的操作（rename/delete/put_content）は **`.bak-YYYYMMDDHHmmSS` バックアップ**を自動生成する。
//! 削除 → バックアップ、上書き → 旧を .bak に移してから書き込み、リネーム → 旧名を .bak にして新名に書き込み。
//!
//! `is_current` なファイルの **削除・リネームは拒否**（現プロセスが参照しているファイルの扱いを単純化するため）。
//! 内容更新は `is_current` でも許すが、クライアントに「再起動しないと反映されない」注意を返す責務がある。
//! 書き込み前に `toml::from_str::<Conf>()` で**妥当性検証**を行い、パース失敗時は 400（ファイルは触らない）。
//!
//! パス検証・バックアップ名は [`util`]。

mod util;
mod backups;

use actix_web::web::{self, Data, Json};
use actix_web::{delete, get, post, put, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::conf::Conf;
use crate::SharedState;

use util::{backup_path, err_response, is_current, profiles_dir, safe_basename};

// ---- DTO ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CloneRequest {
	/// 複製元のファイル名（basename、拡張子込み）。
	pub source: String,
	/// 新ファイル名（basename、拡張子込み）。既存と衝突する場合は 409。
	pub new_filename: String,
}

#[derive(Debug, Deserialize)]
pub struct RenameRequest {
	pub new_filename: String,
}

#[derive(Debug, Deserialize)]
pub struct PutContentRequest {
	/// 新しいファイル全体の内容（UTF-8 TOML）。サーバ側で `Conf` としてパースして通らなければ 400。
	pub content: String,
	#[serde(default)]
	pub expected_revision: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ProfileContentResponse {
	pub revision: String,
	pub filename: String,
	pub path: String,
	pub size: u64,
	pub content: String,
	pub is_current: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileOpResponse {
	pub filename: String,
	pub path: String,
	/// 生成されたバックアップファイル名（あれば）。UI が「バックアップを作成しました」を出せるよう返す。
	pub backup: Option<String>,
	/// 付随する警告（例: 「内容を更新しましたが再起動しないと反映されません」）。
	#[serde(skip_serializing_if = "Option::is_none")]
	pub warning: Option<String>,
}

// ---- エンドポイント実装 ------------------------------------------------------

#[get("/profiles/{filename}/content")]
pub async fn get_profile_content(state: Data<SharedState>, filename: web::Path<String>) -> impl Responder {
	let filename = filename.into_inner();
	let (dir, current) = match profiles_dir(&state).await {
		Ok(x) => x,
		Err(r) => return r,
	};
	let path = match safe_basename(&dir, &filename) {
		Ok(p) => p,
		Err(r) => return r,
	};
	if !path.exists() {
		return err_response(
			actix_web::http::StatusCode::NOT_FOUND,
			"not_found",
			format!("{} が存在しません。", path.display()),
		);
	}
	let content = match std::fs::read_to_string(&path) {
		Ok(c) => c,
		Err(e) => return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "read_failed", e),
	};
	let size = std::fs::metadata(&path).ok().map(|m| m.len()).unwrap_or(0);
	HttpResponse::Ok().json(ProfileContentResponse {
		revision: blake3::hash(content.as_bytes()).to_hex().to_string(),
		filename,
		path: path.display().to_string(),
		size,
		content,
		is_current: is_current(&path, &current),
	})
}

#[put("/profiles/{filename}/content")]
pub async fn put_profile_content(state: Data<SharedState>, filename: web::Path<String>, body: Json<PutContentRequest>) -> impl Responder {
	let filename = filename.into_inner();
	let (dir, current) = match profiles_dir(&state).await {
		Ok(x) => x,
		Err(r) => return r,
	};
	let path = match safe_basename(&dir, &filename) {
		Ok(p) => p,
		Err(r) => return r,
	};

	// パース検証（副作用なし）。失敗時は書き込まない。
	if let Err(e) = toml::from_str::<Conf>(&body.content) {
		return err_response(
			actix_web::http::StatusCode::BAD_REQUEST,
			"invalid_toml",
			format!("conf として不正です: {e}"),
		);
	}

	let _operation = match crate::profile_store::lifecycle::lock_config_write(&path) {
		Ok(lock) => lock,
		Err(error) => return err_response(actix_web::http::StatusCode::CONFLICT, "profile_busy", error),
	};
	if let Some(expected) = &body.expected_revision {
		let original = match std::fs::read(&path) {
			Ok(bytes) => bytes,
			Err(e) => return err_response(actix_web::http::StatusCode::CONFLICT, "revision_unavailable", e),
		};
		if blake3::hash(&original).to_hex().as_str() != expected {
			return err_response(actix_web::http::StatusCode::CONFLICT, "revision_conflict",
				"別の画面でファイルが変更されています。入力内容を控え、最新の内容を読み直してください。");
		}
	}
	// 既存ファイルのバックアップ（新規作成でなければ）
	let backup = if path.exists() {
		let bak = backup_path(&path);
		if let Err(e) = std::fs::copy(&path, &bak) {
			return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "backup_failed", e);
		}
		Some(bak)
	} else {
		None
	};

	if let Err(e) = crate::profile_store::write_config_transaction(&path, body.content.as_bytes()) {
		return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "write_failed", e);
	}

	let warning = if is_current(&path, &current) {
		Some("このファイルは現在読み込まれているプロファイルです。再起動しないと反映されません。".to_string())
	} else {
		None
	};

	log::info!("《ProfileOps》 {} を更新しました。", path.display());
	HttpResponse::Ok().json(ProfileOpResponse {
		filename,
		path: path.display().to_string(),
		backup: backup.map(|b| b.file_name().unwrap_or_default().to_string_lossy().to_string()),
		warning,
	})
}

#[post("/profiles/clone")]
pub async fn post_clone(state: Data<SharedState>, body: Json<CloneRequest>) -> impl Responder {
	let (dir, _current) = match profiles_dir(&state).await {
		Ok(x) => x,
		Err(r) => return r,
	};
	let src = match safe_basename(&dir, &body.source) {
		Ok(p) => p,
		Err(r) => return r,
	};
	let dst = match safe_basename(&dir, &body.new_filename) {
		Ok(p) => p,
		Err(r) => return r,
	};
	let _operation = match crate::profile_store::lifecycle::lock_config_write(&src) {
		Ok(lock) => lock,
		Err(error) => return err_response(actix_web::http::StatusCode::CONFLICT, "profile_busy", error),
	};
	if !src.exists() {
		return err_response(
			actix_web::http::StatusCode::NOT_FOUND,
			"source_not_found",
			format!("複製元が存在しません: {}", src.display()),
		);
	}
	if dst.exists() {
		return err_response(
			actix_web::http::StatusCode::CONFLICT,
			"destination_exists",
			format!("複製先が既に存在します: {}", dst.display()),
		);
	}
	if let Err(e) = std::fs::copy(&src, &dst) {
		return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "copy_failed", e);
	}
	log::info!("《ProfileOps》 {} → {} を複製しました。", src.display(), dst.display());
	HttpResponse::Ok().json(ProfileOpResponse {
		filename: body.new_filename.clone(),
		path: dst.display().to_string(),
		backup: None,
		warning: None,
	})
}

#[post("/profiles/{filename}/rename")]
pub async fn post_rename(state: Data<SharedState>, filename: web::Path<String>, body: Json<RenameRequest>) -> impl Responder {
	let filename = filename.into_inner();
	let (dir, current) = match profiles_dir(&state).await {
		Ok(x) => x,
		Err(r) => return r,
	};
	let src = match safe_basename(&dir, &filename) {
		Ok(p) => p,
		Err(r) => return r,
	};
	let dst = match safe_basename(&dir, &body.new_filename) {
		Ok(p) => p,
		Err(r) => return r,
	};
	let _operation = match crate::profile_store::lifecycle::lock_config_write(&src) {
		Ok(lock) => lock,
		Err(error) => return err_response(actix_web::http::StatusCode::CONFLICT, "profile_busy", error),
	};
	if !src.exists() {
		return err_response(
			actix_web::http::StatusCode::NOT_FOUND,
			"not_found",
			format!("{} が存在しません。", src.display()),
		);
	}
	if is_current(&src, &current) {
		return err_response(
			actix_web::http::StatusCode::CONFLICT,
			"cannot_rename_current",
			"現在読み込み中のプロファイルはリネームできません。先に別プロファイルへ切り替えてください。",
		);
	}
	if dst.exists() {
		return err_response(
			actix_web::http::StatusCode::CONFLICT,
			"destination_exists",
			format!("リネーム先が既に存在します: {}", dst.display()),
		);
	}
	if let Err(e) = std::fs::rename(&src, &dst) {
		return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "rename_failed", e);
	}
	log::info!("《ProfileOps》 {} → {} にリネームしました。", src.display(), dst.display());
	HttpResponse::Ok().json(ProfileOpResponse {
		filename: body.new_filename.clone(),
		path: dst.display().to_string(),
		backup: None,
		warning: None,
	})
}

#[delete("/profiles/{filename}")]
pub async fn delete_profile(state: Data<SharedState>, filename: web::Path<String>) -> impl Responder {
	let filename = filename.into_inner();
	let (dir, current) = match profiles_dir(&state).await {
		Ok(x) => x,
		Err(r) => return r,
	};
	let path = match safe_basename(&dir, &filename) {
		Ok(p) => p,
		Err(r) => return r,
	};
	let _operation = match crate::profile_store::lifecycle::lock_config_write(&path) {
		Ok(lock) => lock,
		Err(error) => return err_response(actix_web::http::StatusCode::CONFLICT, "profile_busy", error),
	};
	if !path.exists() {
		return err_response(
			actix_web::http::StatusCode::NOT_FOUND,
			"not_found",
			format!("{} が存在しません。", path.display()),
		);
	}
	if is_current(&path, &current) {
		return err_response(
			actix_web::http::StatusCode::CONFLICT,
			"cannot_delete_current",
			"現在読み込み中のプロファイルは削除できません。先に別プロファイルへ切り替えてください。",
		);
	}
	// 削除前にバックアップ（削除したいとは言えユーザが間違えるケースはありうる）。
	let bak = backup_path(&path);
	if let Err(e) = std::fs::rename(&path, &bak) {
		return err_response(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "delete_failed", e);
	}
	log::info!(
		"《ProfileOps》 {} を {} にバックアップしました（削除扱い）。",
		path.display(),
		bak.display()
	);
	HttpResponse::Ok().json(ProfileOpResponse {
		filename,
		path: path.display().to_string(),
		backup: Some(bak.file_name().unwrap_or_default().to_string_lossy().to_string()),
		warning: None,
	})
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(backups::list)
		.service(backups::restore)
		.service(get_profile_content)
		.service(put_profile_content)
		.service(post_clone)
		.service(post_rename)
		.service(delete_profile);
}

#[cfg(test)]
mod management_tests {
    use super::*;

    #[actix_web::test]
    #[ignore = "opens an idle audio output device; timed user approval required"]
    async fn file_management_conflict_then_recovery() {
        use actix_web::{test as http, App, http::{Method, StatusCode}};
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("vac-file-management-{}-{nonce}",std::process::id()));
        std::fs::create_dir(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let _cleanup = Cleanup(root.clone());
        let current = root.join("conf.toml");
        let original = "workers = 1\n# active retained\n";
        let alternate = "workers = 2\n# alternate retained\n";
        std::fs::write(&current,original).unwrap();
        std::fs::write(root.join("other.toml"),alternate).unwrap();
        let mut conf: crate::Conf = toml::from_str("").unwrap();
        conf.source_path=Some(current.clone());
        conf.runtime_dir=Some(root.join("runtime"));
        let audio=std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
        let state=crate::State::new(&conf,audio,crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let app=http::init_service(App::new().app_data(Data::new(state.clone())).configure(configure)).await;
        let guard=crate::profile_store::lifecycle::lock_config_write(&current).unwrap();
        for (method,path,body) in [
            (Method::POST,"/profiles/clone",serde_json::json!({"source":"other.toml","new_filename":"copy.toml"})),
            (Method::POST,"/profiles/other.toml/rename",serde_json::json!({"new_filename":"renamed.toml"})),
            (Method::DELETE,"/profiles/other.toml",serde_json::json!({})),
        ] {
            let response=http::call_service(&app,http::TestRequest::default().method(method).uri(path).set_json(body).to_request()).await;
            assert_eq!(response.status(),StatusCode::CONFLICT,"{path}");
            let body:serde_json::Value=http::read_body_json(response).await;
            assert_eq!(body["error"],"profile_busy");
            assert_eq!(std::fs::read_to_string(root.join("other.toml")).unwrap(),alternate);
            assert!(!root.join("copy.toml").exists());
            assert!(!root.join("renamed.toml").exists());
        }
        drop(guard);
        for (path,body) in [
            ("/profiles/clone",serde_json::json!({"source":"other.toml","new_filename":"copy.toml"})),
            ("/profiles/copy.toml/rename",serde_json::json!({"new_filename":"renamed.toml"})),
        ] {
            let response=http::call_service(&app,http::TestRequest::post().uri(path).set_json(body).to_request()).await;
            assert_eq!(response.status(),StatusCode::OK,"{path}");
        }
        let response=http::call_service(&app,http::TestRequest::delete().uri("/profiles/renamed.toml").to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        let deleted:serde_json::Value=http::read_body_json(response).await;
        assert!(!root.join("renamed.toml").exists());
        let response=http::call_service(&app,http::TestRequest::post().uri("/profile-backups/restore").set_json(serde_json::json!({"backup":deleted["backup"],"filename":"restored.toml"})).to_request()).await;
        assert_eq!(response.status(),StatusCode::OK);
        assert_eq!(std::fs::read_to_string(root.join("restored.toml")).unwrap(),alternate);
        for (method,path,body) in [
            (Method::DELETE,"/profiles/conf.toml",serde_json::json!({})),
            (Method::POST,"/profiles/conf.toml/rename",serde_json::json!({"new_filename":"forbidden.toml"})),
        ] {
            let response=http::call_service(&app,http::TestRequest::default().method(method).uri(path).set_json(body).to_request()).await;
            assert_eq!(response.status(),StatusCode::CONFLICT);
        }
        assert_eq!(std::fs::read_to_string(&current).unwrap(),original);
        assert_eq!(std::fs::read_to_string(root.join("other.toml")).unwrap(),alternate);
        drop(app);drop(state);
    }
}

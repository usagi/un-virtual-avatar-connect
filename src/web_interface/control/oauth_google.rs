//! Google OAuth helpers for Google Sheets read-only access.
//!
//! This first slice keeps the flow stateless on the server side:
//! - `/start` creates a Google Device Authorization session and returns the
//!   device code + user-facing code.
//! - `/poll` performs one token polling attempt and saves tokens on success.
//! GUI can poll until the user authorizes or the device code expires.

use actix_web::web::{self, Data, Json};
use actix_web::{delete, get, post, HttpResponse, Responder};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::SharedState;

const GOOGLE_DEVICE_CODE_URL: &str = "https://oauth2.googleapis.com/device/code";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const DEFAULT_SCOPE: &str = "https://www.googleapis.com/auth/spreadsheets.readonly";
const DEFAULT_CLIENT_ID_ENV: &str = "UNVAC_GOOGLE_OAUTH_CLIENT_ID";

#[derive(Debug, Clone, Deserialize)]
pub struct GoogleOAuthStartRequest {
	pub client_id: Option<String>,
	pub scope: Option<String>,
	pub token_file: Option<String>,
	pub open_browser: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleDeviceStartResponse {
	pub outcome: &'static str,
	pub client_id_source: &'static str,
	pub device_code: String,
	pub user_code: String,
	pub verification_url: String,
	pub verification_url_complete: Option<String>,
	pub expires_in: u64,
	pub interval: u64,
	pub scope: String,
	pub token_file: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoogleOAuthPollRequest {
	pub client_id: Option<String>,
	pub device_code: String,
	pub token_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleOAuthPollResponse {
	pub status: &'static str,
	pub token_file: String,
	pub interval: Option<u64>,
	pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleOAuthStatusResponse {
	pub configured: bool,
	pub client_id_source: Option<&'static str>,
	pub token_file: String,
	pub token_file_exists: bool,
	pub has_access_token: bool,
	pub has_refresh_token: bool,
	pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredGoogleTokens {
	pub access_token: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub refresh_token: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub scope: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub token_type: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub expires_in: Option<u64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub obtained_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleDeviceCodeResponse {
	device_code: String,
	user_code: String,
	#[serde(alias = "verification_uri")]
	verification_url: String,
	#[serde(default)]
	verification_url_complete: Option<String>,
	expires_in: u64,
	#[serde(default = "default_poll_interval")]
	interval: u64,
}

#[derive(Debug, Deserialize)]
struct GoogleTokenResponse {
	access_token: Option<String>,
	refresh_token: Option<String>,
	scope: Option<String>,
	token_type: Option<String>,
	expires_in: Option<u64>,
	error: Option<String>,
	error_description: Option<String>,
	interval: Option<u64>,
}

#[post("/oauth/google/sheets/start")]
pub async fn post_google_sheets_start(_state: Data<SharedState>, body: Json<GoogleOAuthStartRequest>) -> impl Responder {
	match start_google_sheets_device_flow(body.into_inner()).await {
		Ok(response) => HttpResponse::Ok().json(response),
		Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
			"error": "google_oauth_start_failed",
			"reason": e.to_string(),
		})),
	}
}

#[post("/oauth/google/sheets/poll")]
pub async fn post_google_sheets_poll(_state: Data<SharedState>, body: Json<GoogleOAuthPollRequest>) -> impl Responder {
	match poll_google_sheets_device_flow(body.into_inner()).await {
		Ok((status, response)) => HttpResponse::build(status).json(response),
		Err(e) => HttpResponse::BadRequest().json(serde_json::json!({
			"error": "google_oauth_poll_failed",
			"reason": e.to_string(),
		})),
	}
}

#[get("/oauth/google/sheets/status")]
pub async fn get_google_sheets_status() -> impl Responder {
	HttpResponse::Ok().json(google_sheets_status(None))
}

#[delete("/oauth/google/sheets/tokens")]
pub async fn delete_google_sheets_tokens() -> impl Responder {
	let token_file = default_google_sheets_token_file();
	let deleted = match std::fs::remove_file(&token_file) {
		Ok(()) => true,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
		Err(e) => {
			return HttpResponse::InternalServerError().json(serde_json::json!({
				"error": "delete_failed",
				"reason": e.to_string(),
			}));
		}
	};
	HttpResponse::Ok().json(serde_json::json!({
		"deleted": deleted,
		"path": token_file.display().to_string(),
	}))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(post_google_sheets_start);
	cfg.service(post_google_sheets_poll);
	cfg.service(get_google_sheets_status);
	cfg.service(delete_google_sheets_tokens);
}

async fn start_google_sheets_device_flow(req: GoogleOAuthStartRequest) -> Result<GoogleDeviceStartResponse> {
	let (client_id, client_id_source) = resolve_google_client_id(req.client_id.as_deref())?;
	let scope = req
		.scope
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.unwrap_or(DEFAULT_SCOPE)
		.to_string();
	let token_file = resolve_token_file(req.token_file.as_deref());

	let response = reqwest::Client::new()
		.post(GOOGLE_DEVICE_CODE_URL)
		.header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
		.body(
			serde_urlencoded::to_string(&[("client_id", client_id.as_str()), ("scope", scope.as_str())])
				.context("device code body encode failed")?,
		)
		.send()
		.await
		.context("device code request failed")?;
	let status = response.status();
	let text = response.text().await.context("device code response read failed")?;
	if !status.is_success() {
		return Err(anyhow!("device code HTTP {}: {}", status.as_u16(), text));
	}
	let device: GoogleDeviceCodeResponse = serde_json::from_str(&text).with_context(|| format!("device code JSON parse failed: {text}"))?;

	if req.open_browser.unwrap_or(true) {
		let url = device
			.verification_url_complete
			.as_deref()
			.unwrap_or(device.verification_url.as_str());
		let _ = webbrowser::open(url);
	}

	Ok(GoogleDeviceStartResponse {
		outcome: "started",
		client_id_source,
		device_code: device.device_code,
		user_code: device.user_code,
		verification_url: device.verification_url,
		verification_url_complete: device.verification_url_complete,
		expires_in: device.expires_in,
		interval: device.interval,
		scope,
		token_file: token_file.display().to_string(),
	})
}

async fn poll_google_sheets_device_flow(req: GoogleOAuthPollRequest) -> Result<(actix_web::http::StatusCode, GoogleOAuthPollResponse)> {
	let (client_id, _) = resolve_google_client_id(req.client_id.as_deref())?;
	let device_code = req.device_code.trim();
	if device_code.is_empty() {
		return Err(anyhow!("device_code is empty"));
	}
	let token_file = resolve_token_file(req.token_file.as_deref());
	let response = reqwest::Client::new()
		.post(GOOGLE_TOKEN_URL)
		.header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
		.body(
			serde_urlencoded::to_string(&[
				("client_id", client_id.as_str()),
				("device_code", device_code),
				("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
			])
			.context("token body encode failed")?,
		)
		.send()
		.await
		.context("token request failed")?;
	let status = response.status();
	let text = response.text().await.context("token response read failed")?;
	let token: GoogleTokenResponse = serde_json::from_str(&text).with_context(|| format!("token JSON parse failed: {text}"))?;

	if status.is_success() {
		let access_token = token.access_token.ok_or_else(|| anyhow!("token response missing access_token"))?;
		let stored = StoredGoogleTokens {
			access_token,
			refresh_token: token.refresh_token,
			scope: token.scope,
			token_type: token.token_type,
			expires_in: token.expires_in,
			obtained_at: Some(crate::datetime::DateTime::now().to_rfc3339()),
		};
		save_google_tokens(&token_file, &stored)?;
		return Ok((
			actix_web::http::StatusCode::OK,
			GoogleOAuthPollResponse {
				status: "authorized",
				token_file: token_file.display().to_string(),
				interval: None,
				error: None,
			},
		));
	}

	let error = token.error.unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
	let pending = error == "authorization_pending" || error == "slow_down";
	let http_status = if pending {
		actix_web::http::StatusCode::ACCEPTED
	} else {
		actix_web::http::StatusCode::BAD_REQUEST
	};
	Ok((
		http_status,
		GoogleOAuthPollResponse {
			status: if pending { "pending" } else { "failed" },
			token_file: token_file.display().to_string(),
			interval: token.interval.or_else(|| (error == "slow_down").then_some(10)),
			error: Some(token.error_description.unwrap_or(error)),
		},
	))
}

fn google_sheets_status(token_file_override: Option<&str>) -> GoogleOAuthStatusResponse {
	let client = resolve_google_client_id(None).ok();
	let token_file = resolve_token_file(token_file_override);
	let stored = load_google_tokens(&token_file).ok().flatten();
	GoogleOAuthStatusResponse {
		configured: client.is_some(),
		client_id_source: client.map(|(_, source)| source),
		token_file: token_file.display().to_string(),
		token_file_exists: token_file.exists(),
		has_access_token: stored
			.as_ref()
			.map(|tokens| !tokens.access_token.trim().is_empty())
			.unwrap_or(false),
		has_refresh_token: stored
			.as_ref()
			.and_then(|tokens| tokens.refresh_token.as_deref())
			.map(|value| !value.trim().is_empty())
			.unwrap_or(false),
		scope: stored.and_then(|tokens| tokens.scope),
	}
}

fn resolve_google_client_id(explicit: Option<&str>) -> Result<(String, &'static str)> {
	if let Some(value) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
		return Ok((value.to_string(), "request"));
	}
	if let Ok(value) = std::env::var(DEFAULT_CLIENT_ID_ENV) {
		let value = value.trim().to_string();
		if !value.is_empty() {
			return Ok((value, "env"));
		}
	}
	Err(anyhow!(
		"Google OAuth client_id is empty. Set request.client_id or env {DEFAULT_CLIENT_ID_ENV}."
	))
}

fn resolve_token_file(value: Option<&str>) -> PathBuf {
	value
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.map(PathBuf::from)
		.unwrap_or_else(default_google_sheets_token_file)
}

fn default_google_sheets_token_file() -> PathBuf {
	dirs::config_dir()
		.unwrap_or_else(std::env::temp_dir)
		.join("virtual-avatar-connect")
		.join("google-sheets-token.json")
}

fn save_google_tokens(path: &PathBuf, tokens: &StoredGoogleTokens) -> Result<()> {
	if let Some(parent) = path.parent() {
		std::fs::create_dir_all(parent).with_context(|| format!("create token dir {}", parent.display()))?;
	}
	let bytes = serde_json::to_vec_pretty(tokens).context("serialize token JSON failed")?;
	std::fs::write(path, bytes).with_context(|| format!("write token file {}", path.display()))?;
	Ok(())
}

fn load_google_tokens(path: &PathBuf) -> Result<Option<StoredGoogleTokens>> {
	let text = match std::fs::read_to_string(path) {
		Ok(text) => text,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
		Err(e) => return Err(e).with_context(|| format!("read token file {}", path.display())),
	};
	Ok(Some(
		serde_json::from_str(&text).with_context(|| format!("parse token file {}", path.display()))?,
	))
}

fn default_poll_interval() -> u64 {
	5
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_google_sheets_token_file_is_under_vac_config_dir() {
		let path = default_google_sheets_token_file();
		assert!(path.ends_with("google-sheets-token.json"));
		assert!(path.to_string_lossy().contains("virtual-avatar-connect"));
	}

	#[test]
	fn resolve_token_file_prefers_request_path() {
		let path = resolve_token_file(Some("C:/tmp/google-token.json"));
		assert_eq!(path, PathBuf::from("C:/tmp/google-token.json"));
	}

	#[test]
	fn stored_google_tokens_roundtrip() {
		let dir = std::env::temp_dir().join(format!(
			"unvac-google-oauth-test-{}",
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_nanos()
		));
		let path = dir.join("token.json");
		let tokens = StoredGoogleTokens {
			access_token: "access".into(),
			refresh_token: Some("refresh".into()),
			scope: Some(DEFAULT_SCOPE.into()),
			token_type: Some("Bearer".into()),
			expires_in: Some(3600),
			obtained_at: Some("2026-05-31T00:00:00Z".into()),
		};
		save_google_tokens(&path, &tokens).unwrap();
		let loaded = load_google_tokens(&path).unwrap().unwrap();
		assert_eq!(loaded.access_token, "access");
		assert_eq!(loaded.refresh_token.as_deref(), Some("refresh"));
		let _ = std::fs::remove_dir_all(dir);
	}
}

//! Phase VI-alpha-5: Control API endpoint group that triggers the Twitch
//! Device Code Flow (DCF) from the GUI.
//!
//! Endpoints (Bearer auth is handled by outer `control_api_auth`):
//!   - `POST   /api/v1/control/oauth/twitch/{account}/start`   - begin DCF (idempotent if pending)
//!   - `GET    /api/v1/control/oauth/twitch/{account}/status`  - current session status
//!   - `POST   /api/v1/control/oauth/twitch/{account}/cancel`  - cancel in-flight session
//!   - `DELETE /api/v1/control/oauth/twitch/{account}/tokens`  - purge stored tokens (force re-auth)
//!
//! `account` must be `broadcaster` or `moderator`. If the corresponding config
//! (`[twitch.eventsub]` / `[twitch.moderator]`) is missing we return 400.
//!
//! The WS `/events` stream emits `ControlEvent::OAuthStatus` so GUIs can
//! follow progress without polling `status`.
//!
//! DCF の開始〜ポーリング登録は [`start_flow`]。

use actix_web::web::{self, Data};
use actix_web::{delete, get, post, HttpResponse, Responder};
use serde::Serialize;

use crate::twitch::oauth::{self, OAuthIdent};
use crate::SharedState;

use crate::web_interface::control::events::ControlEvent;

pub use crate::twitch_oauth_sessions::{OAuthAccount, OAuthSessionStatus, OAuthSessionView, OAuthSessions};

// ---------------------------------------------------------------------------
// DTO（`start_flow` から参照されるためサブモジュールより先に置く）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct OAuthStartResponse {
	/// One of: "started" (new), "already_pending", "already_authorized".
	pub outcome: &'static str,
	pub session: Option<OAuthSessionView>,
}

#[derive(Debug, Serialize)]
struct TwitchRewardsResponse {
 available: bool,
 reason: Option<String>,
	broadcaster_login: String,
	broadcaster_id: String,
	rewards: Vec<crate::twitch::eventsub::CustomRewardSummary>,
}

#[derive(Debug, Serialize)]
struct TwitchAuthorizationResponse {
	account: &'static str,
	configured: bool,
	authorized: bool,
}

mod start_flow;

use start_flow::start_oauth;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[post("/oauth/twitch/{account}/start")]
pub async fn post_oauth_start(state: Data<SharedState>, path: web::Path<String>) -> impl Responder {
	let Some(account) = OAuthAccount::from_path(&path.into_inner()) else {
		return bad_request("account must be `broadcaster` or `moderator`");
	};

	match start_oauth(state.get_ref(), account).await {
		Ok(resp) => HttpResponse::Ok().json(resp),
		Err(e) => {
			log::error!("[ControlAPI/oauth] start {} failed: {:#}", account.as_tag(), e);
			HttpResponse::BadRequest().json(serde_json::json!({
			 "error": "oauth_start_failed",
			 "reason": e.to_string(),
			}))
		}
	}
}

/// Explicit user action. Keeps the configured account-specific browser command.
#[post("/oauth/twitch/{account}/open")]
async fn post_oauth_open(state: Data<SharedState>, path: web::Path<String>) -> impl Responder {
	let Some(account) = OAuthAccount::from_path(&path.into_inner()) else { return bad_request("unknown account"); };
	let (ident, sessions) = {
		let s = state.read().await;
		let Some(twitch) = s.twitch.as_ref() else { return bad_request("Twitch設定がありません。"); };
		let Some(es) = twitch.eventsub.as_ref() else { return bad_request("Twitch設定がありません。"); };
		let ident = match account {
			OAuthAccount::Broadcaster => OAuthIdent::for_broadcaster(es),
			OAuthAccount::Moderator => {
				let Some(mc) = twitch.moderator.as_ref() else { return bad_request("投稿用アカウントの設定がありません。"); };
				OAuthIdent::for_moderator(es, mc)
			}
		};
		(ident, s.twitch_oauth.clone())
	};
	let Some(session) = sessions.active_pending_snapshot(account, tokio::time::Instant::now()).await else {
		return bad_request("有効な認証コードがありません。認証を開始してください。");
	};
	if !oauth::launch_browser(&ident, &session.verification_uri) {
		return bad_request("ブラウザーを開けませんでした。認証URLをコピーして開いてください。");
	}
	HttpResponse::Ok().json(serde_json::json!({"opened":true}))
}

#[get("/oauth/twitch/{account}/status")]
pub async fn get_oauth_status(state: Data<SharedState>, path: web::Path<String>) -> impl Responder {
	let Some(account) = OAuthAccount::from_path(&path.into_inner()) else {
		return bad_request("account must be `broadcaster` or `moderator`");
	};

	let sessions = {
		let s = state.read().await;
		s.twitch_oauth.clone()
	};
	match sessions.snapshot(account).await {
		Some(view) => HttpResponse::Ok().json(view),
		None => HttpResponse::NotFound().json(serde_json::json!({
		 "error": "no_session",
		 "account": account.as_tag(),
		})),
	}
}

/// 保存済みトークンを明示的に検証する。通常のruntime snapshotは外部通信を行わない。
#[get("/oauth/twitch/{account}/authorization")]
pub async fn get_oauth_authorization(state: Data<SharedState>, path: web::Path<String>) -> impl Responder {
	let Some(account) = OAuthAccount::from_path(&path.into_inner()) else {
		return bad_request("account must be `broadcaster` or `moderator`");
	};
	let ident = {
		let s = state.read().await;
		let Some(twitch) = s.twitch.as_ref() else {
			return HttpResponse::Ok().json(TwitchAuthorizationResponse {
				account: account.as_tag(),
				configured: false,
				authorized: false,
			});
		};
		let Some(eventsub) = twitch.eventsub.as_ref() else {
			return HttpResponse::Ok().json(TwitchAuthorizationResponse {
				account: account.as_tag(),
				configured: false,
				authorized: false,
			});
		};
		match account {
			OAuthAccount::Broadcaster => OAuthIdent::for_broadcaster(eventsub),
			OAuthAccount::Moderator => {
				let Some(moderator) = twitch.moderator.as_ref() else {
					return HttpResponse::Ok().json(TwitchAuthorizationResponse {
						account: account.as_tag(),
						configured: false,
						authorized: false,
					});
				};
				OAuthIdent::for_moderator(eventsub, moderator)
			}
		}
	};
	let authorized = !ident.client_id.trim().is_empty() && oauth::try_load_valid_token_for(&ident).await.is_some();
	HttpResponse::Ok().json(TwitchAuthorizationResponse {
		account: account.as_tag(),
		configured: true,
		authorized,
	})
}

#[get("/oauth/twitch/broadcaster/rewards")]
pub async fn get_broadcaster_rewards(state: Data<SharedState>) -> impl Responder {
	let twitch = {
		let s = state.read().await;
		let Some(twitch) = s.twitch.as_ref().cloned() else {
			return bad_request("[twitch] section is not configured");
		};
		twitch
	};
	let Some(es) = twitch.eventsub.as_ref() else {
		return bad_request("[twitch.eventsub] section is not configured");
	};

	let client_id = oauth::resolve_twitch_client_id(es);
	if client_id.trim().is_empty() {
		return bad_request("Twitch Client ID is not configured");
	}
	let ident = OAuthIdent::for_broadcaster(es);
	let explicit_token = std::env::var("VAC_TWITCH_USER_ACCESS_TOKEN")
		.ok()
		.filter(|value| !value.trim().is_empty())
		.or_else(|| es.user_access_token.clone().filter(|value| !value.trim().is_empty()));
	let access_token = match explicit_token {
		Some(token) if oauth::validate_user_access_token(&token).await.is_ok() => token,
		_ => match oauth::try_load_valid_token_for(&ident).await {
			Some(token) => token,
			None => {
				return HttpResponse::Unauthorized().json(serde_json::json!({
					"error": "twitch_not_authorized",
					"reason": "Twitch broadcaster authorization is required",
				}));
			}
		},
	};
	let broadcaster_login = es
		.broadcaster_login
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.unwrap_or(twitch.username.trim())
		.trim_start_matches('#')
		.to_lowercase();
	if broadcaster_login.is_empty() {
		return bad_request("Twitch broadcaster login is not configured");
	}

	let broadcaster = match crate::twitch::eventsub::helix_broadcaster(&client_id, &access_token, &broadcaster_login).await {
		Ok(id) => id,
		Err(error) => {
			log::warn!("[ControlAPI/oauth] failed to resolve Twitch broadcaster: {error:#}");
			return HttpResponse::BadGateway().json(serde_json::json!({
				"error": "twitch_user_lookup_failed",
				"reason": error.to_string(),
			}));
		}
	};
 let available = match broadcaster.has_channel_points() {
  Ok(value) => value,
  Err(error) => return HttpResponse::BadGateway().json(serde_json::json!({"error":"twitch_capability_unknown","reason":error.to_string()})),
 };
 let broadcaster_id = broadcaster.id;
 if !available {
  return HttpResponse::Ok().json(TwitchRewardsResponse { broadcaster_login, broadcaster_id, rewards: Vec::new(), available: false,
   reason: Some("このチャンネルではチャンネルポイントを利用しません。Twitchのアフィリエイトまたはパートナー資格が必要な機能です。フォロー・Raidなどは引き続き利用できます。".into()) });
 }
	match crate::twitch::eventsub::helix_list_custom_rewards(&client_id, &access_token, &broadcaster_id).await {
		Ok(rewards) => HttpResponse::Ok().json(TwitchRewardsResponse {
 available: true,
 reason: None,
			broadcaster_login,
			broadcaster_id,
			rewards,
		}),
		Err(error) => {
			log::warn!("[ControlAPI/oauth] failed to list Twitch custom rewards: {error:#}");
			HttpResponse::BadGateway().json(serde_json::json!({
				"error": "twitch_rewards_failed",
				"reason": error.to_string(),
			}))
		}
	}
}

#[post("/oauth/twitch/{account}/cancel")]
pub async fn post_oauth_cancel(state: Data<SharedState>, path: web::Path<String>) -> impl Responder {
	let Some(account) = OAuthAccount::from_path(&path.into_inner()) else {
		return bad_request("account must be `broadcaster` or `moderator`");
	};

	let (sessions, tx) = {
		let s = state.read().await;
		(s.twitch_oauth.clone(), s.control_event_tx.clone())
	};

	let Some((canceled, view)) = sessions.cancel_pending(account).await else {
		return HttpResponse::NotFound().json(serde_json::json!({
		 "error": "no_session",
		 "account": account.as_tag(),
		}));
	};

	if canceled {
		log::info!("[ControlAPI/oauth] {} session canceled", account.as_tag());
		let _ = tx.send(ControlEvent::OAuthStatus {
			account: account.as_tag(),
			status: OAuthSessionStatus::Canceled,
			view: view.clone(),
		});
	}
	HttpResponse::Ok().json(serde_json::json!({
	 "canceled": canceled,
	 "session": view,
	}))
}

/// Purges stored tokens for the given account.
///
/// Use case: the user notices "I authorized the wrong account" or a stale
/// token is misbehaving, and wants a forced reset.
///
/// Behaviour:
///   - Removes the token file (no-op if absent).
///   - Removes any in-flight / completed OAuth session from the map
///     (aborts the polling task if it is still Pending).
///   - A subsequent `GET /authorization` will report `authorized=false`.
///
/// Note: currently running EventSub WebSocket connections keep their
/// in-memory access tokens; restart VAC for a clean reset.
#[delete("/oauth/twitch/{account}/tokens")]
pub async fn delete_oauth_tokens(state: Data<SharedState>, path: web::Path<String>) -> impl Responder {
	let Some(account) = OAuthAccount::from_path(&path.into_inner()) else {
		return bad_request("account must be `broadcaster` or `moderator`");
	};

	// Build ident first (delete is a no-op if config is missing).
	let ident = {
		let s = state.read().await;
		let Some(twitch) = s.twitch.as_ref().cloned() else {
			return HttpResponse::BadRequest().json(serde_json::json!({
			 "error": "no_twitch_config",
			 "reason": "[twitch] section is not configured",
			}));
		};
		let Some(es) = twitch.eventsub.as_ref() else {
			return HttpResponse::BadRequest().json(serde_json::json!({
			 "error": "no_eventsub_config",
			 "reason": "[twitch.eventsub] section is not configured",
			}));
		};
		match account {
			OAuthAccount::Broadcaster => OAuthIdent::for_broadcaster(es),
			OAuthAccount::Moderator => {
				let Some(mc) = twitch.moderator.as_ref() else {
					return HttpResponse::BadRequest().json(serde_json::json!({
					 "error": "no_moderator_config",
					 "reason": "[twitch.moderator] section is not configured",
					}));
				};
				OAuthIdent::for_moderator(es, mc)
			}
		}
	};

	let deleted_path = match oauth::delete_stored_tokens(&ident) {
		Ok(opt) => opt,
		Err(e) => {
			log::error!("[ControlAPI/oauth] tokens delete {} failed: {:#}", account.as_tag(), e);
			return HttpResponse::InternalServerError().json(serde_json::json!({
			 "error": "delete_failed",
			 "reason": e.to_string(),
			}));
		}
	};

	// Remove any existing session (aborting Pending polls).
	let session_removed = {
		let sessions = {
			let s = state.read().await;
			s.twitch_oauth.clone()
		};
		sessions.remove_and_abort(account).await
	};

	log::info!(
		"[ControlAPI/oauth] {}: tokens delete done (deleted_file={} session_removed={})",
		account.as_tag(),
		deleted_path.is_some(),
		session_removed,
	);

	HttpResponse::Ok().json(serde_json::json!({
	 "account": account.as_tag(),
	 "deleted": deleted_path.is_some(),
	 "path": deleted_path.map(|p| p.display().to_string()),
	 "session_removed": session_removed,
	}))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(post_oauth_start)
		.service(post_oauth_open);
	cfg.service(get_oauth_status);
	cfg.service(get_oauth_authorization);
	cfg.service(get_broadcaster_rewards);
	cfg.service(post_oauth_cancel);
	cfg.service(delete_oauth_tokens);
}

fn bad_request(reason: &str) -> HttpResponse {
	HttpResponse::BadRequest()
		.content_type("application/json")
		.body(format!(r#"{{"error":"bad_request","reason":"{}"}}"#, reason.replace('"', "\\\"")))
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use tokio::time::Instant as TokioInstant;

	use super::*;
	use crate::twitch_oauth_sessions::OAuthSessionInternal;

	#[test]
	fn account_from_path() {
		assert_eq!(OAuthAccount::from_path("broadcaster"), Some(OAuthAccount::Broadcaster));
		assert_eq!(OAuthAccount::from_path("moderator"), Some(OAuthAccount::Moderator));
		assert_eq!(OAuthAccount::from_path("viewer"), None);
		assert_eq!(OAuthAccount::from_path(""), None);
	}

	#[test]
	fn account_tag_roundtrip() {
		for a in [OAuthAccount::Broadcaster, OAuthAccount::Moderator] {
			let t = a.as_tag();
			assert_eq!(OAuthAccount::from_path(t), Some(a));
		}
	}

	#[test]
	fn session_view_serializes() {
		let internal = OAuthSessionInternal {
			account: OAuthAccount::Broadcaster,
			user_code: "AB12CD".to_string(),
			verification_uri: "https://www.twitch.tv/activate?public=true&device-code=AB12CD".to_string(),
			interval_secs: 5,
			expires_at_system: std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
			started_at_system: std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_699_999_400),
			expires_at_instant: TokioInstant::now(),
			status: OAuthSessionStatus::Pending,
			last_error: None,
			cancel_handle: None,
		};
		let view = internal.to_view();
		let json = serde_json::to_string(&view).unwrap();
		assert!(json.contains("\"account\":\"broadcaster\""));
		assert!(json.contains("\"status\":\"pending\""));
		assert!(json.contains("\"user_code\":\"AB12CD\""));
	}
}

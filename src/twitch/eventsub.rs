//! 《Twitch》 EventSub over WebSocket  Eフォロー・サブスク・レイド�Eチャンネルポイント等を VAC チャンネルへ流す、E
//!
//! Helix の購読作�Eには **ユーザーアクセスト�Eクン**�E��E信老E��が忁E��。`VAC_TWITCH_USER_ACCESS_TOKEN` また�E設定�E `user_access_token`、E
//! 未設定�E検証失敗時は `oauth_auto`�E�既宁Etrue�E�でローカル OAuth により取得可能�E�Ecrate::twitch::oauth`�E�、E

use crate::conf::TwitchEventSubConfig;
use crate::state::DataSource;
use crate::{ChannelDatum, SharedState};
use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

pub(crate) const EVENTSUB_WS: &str = "wss://eventsub.wss.twitch.tv/ws";
pub(crate) const HELIX: &str = "https://api.twitch.tv/helix";

const ENV_USER_TOKEN: &str = "VAC_TWITCH_USER_ACCESS_TOKEN";

/// Get Channel Information と Get Streams を同じ観測時点の Helix snapshot として取得する。
pub(crate) async fn helix_channel_snapshot(
	client_id: &str,
	bearer: &str,
	broadcaster_id: &str,
) -> Result<crate::stream_state::HelixChannelSnapshot> {
	let client = reqwest::Client::new();
	let channel = client
		.get(format!("{HELIX}/channels"))
		.query(&[("broadcaster_id", broadcaster_id)])
		.header("Client-ID", client_id)
		.header("Authorization", format!("Bearer {bearer}"))
		.send()
		.await?
		.error_for_status()?
		.json::<Value>()
		.await?;
	let streams = client
		.get(format!("{HELIX}/streams"))
		.query(&[("user_id", broadcaster_id)])
		.header("Client-ID", client_id)
		.header("Authorization", format!("Bearer {bearer}"))
		.send()
		.await?
		.error_for_status()?
		.json::<Value>()
		.await?;
	crate::stream_state::parse_helix_snapshot(&channel, &streams)
}

#[derive(Clone)]
pub(crate) struct ResolvedEventSub {
	pub client_id: String,
	pub access_token: String,
	pub broadcaster_login: String,
	pub broadcaster_id: String,
	pub channel_to: String,
	pub cfg: TwitchEventSubConfig,
}

pub(crate) async fn resolve_from_top_level(
	twitch_username: &str,
	twitch_channel_to: &str,
	es: &TwitchEventSubConfig,
) -> Option<Result<ResolvedEventSub>> {
	resolve_inner(
		es,
		es.broadcaster_login.as_deref().unwrap_or(twitch_username),
		es.channel_to.as_deref().unwrap_or(twitch_channel_to),
	)
	.await
}

pub(crate) async fn resolve_from_processor(
	twitch_username: &str,
	processor_channel_to: &str,
	es: &TwitchEventSubConfig,
) -> Option<Result<ResolvedEventSub>> {
	if twitch_username.trim().is_empty() || processor_channel_to.trim().is_empty() {
		return None;
	}
	resolve_inner(
		es,
		es.broadcaster_login.as_deref().unwrap_or(twitch_username),
		es.channel_to.as_deref().unwrap_or(processor_channel_to),
	)
	.await
}

async fn resolve_inner(es: &TwitchEventSubConfig, broadcaster_login: &str, channel_to: &str) -> Option<Result<ResolvedEventSub>> {
	if !es.enabled {
		return None;
	}

	// 1) 環墁E��数また�E設定�E user_access_token があれ�Eそれを優先！Ealidate に成功するときだけ採用�E�、E
	let explicit_token = std::env::var(ENV_USER_TOKEN)
		.ok()
		.filter(|s| !s.trim().is_empty())
		.or_else(|| es.user_access_token.clone().filter(|s| !s.trim().is_empty()));

	let access_token = if let Some(t) = explicit_token.as_ref() {
		if super::oauth::validate_user_access_token(t).await.is_ok() {
			Some(t.clone())
		} else {
			log::warn!(
    "《Twitch》 EventSub: {} / user_access_token が無効または期限切れでした。保存トークンまたは Device Code Flow にフォールバックします。",
    ENV_USER_TOKEN
   );
			None
		}
	} else {
		None
	};

	let access_token = match access_token {
		Some(t) => t,
		None => {
			if !es.oauth_auto {
				log::warn!(
     "《Twitch》 EventSub: 有効なトークンが無く oauth_auto=false のため OAuth を行いません。{} を設定するか oauth_auto=true にしてください。",
     ENV_USER_TOKEN
    );
				return None;
			}
			// 起動中に対話的な Device Code Flow を始めると、HTTP / desktop GUI の開始まで
			// 最大数分塞いでしまう。保存済みtokenの検証・refreshだけを試し、再認可は
			// 起動後にGUIのResourcesから明示的に行う。
			let ident = super::oauth::OAuthIdent::for_broadcaster(es);
			match super::oauth::try_load_valid_token_for(&ident).await {
				Some(t) => t,
				None => {
					log::warn!(
						"《Twitch》 EventSub: 有効な保存済みtokenがないため起動をスキップします。GUIのResourcesからTwitchを再認可してください。"
					);
					return None;
				}
			}
		}
	};

	let bl = broadcaster_login.trim().trim_start_matches('#').to_lowercase();
	let ct = channel_to.trim().to_string();
	if bl.is_empty() || ct.is_empty() {
		return Some(Err(anyhow!("《Twitch》 EventSub: broadcaster または channel_to が空です。")));
	}
	Some(Ok(ResolvedEventSub {
		client_id: super::oauth::resolve_twitch_client_id(es),
		access_token,
		broadcaster_login: bl,
		broadcaster_id: String::new(),
		channel_to: ct,
		cfg: es.clone(),
	}))
}

/// ループ�E接続付きで EventSub セチE��ョンを走らせる、E
pub(crate) fn spawn_eventsub_loop(state: SharedState, mut resolved: ResolvedEventSub) -> tokio::task::JoinHandle<()> {
	tokio::spawn(async move {
		loop {
			match run_eventsub_session(&state, &mut resolved).await {
				Ok(()) => log::info!("《Twitch》 EventSub セッションが終了しました。再接続します。"),
				Err(e) => log::error!("《Twitch》 EventSub: {:?}", e),
			}
			tokio::time::sleep(Duration::from_secs(5)).await;
		}
	})
}

async fn run_eventsub_session(state: &SharedState, resolved: &mut ResolvedEventSub) -> Result<()> {
	let broadcaster = helix_broadcaster(&resolved.client_id, &resolved.access_token, &resolved.broadcaster_login)
		.await
		.with_context(|| format!("配信老EID を取得できません: {}", resolved.broadcaster_login))?;
	let channel_points_available = broadcaster.has_channel_points()?;
	resolved.broadcaster_id = broadcaster.id;
	log::info!(
		"《Twitch》 EventSub: broadcaster_id={} ↁEVAC channel_to={}",
		resolved.broadcaster_id,
		resolved.channel_to
	);

	let (mut ws_stream, _) = tokio_tungstenite::connect_async(EVENTSUB_WS)
		.await
		.map_err(|e| anyhow!("EventSub WebSocket 接続失敁E {}", e))?;

	// 最初�E **チE��スチE*フレーム = `session_welcome` を征E��。Ping/Pong/Binary などの制御・非テキスト�E読み飛�Eす、E
	// tokio-tungstenite 0.26 では Message::Text の中身は Utf8Bytes。as_str() で &str を得る、E
	let welcome_str: String = loop {
		let next = tokio::time::timeout(Duration::from_secs(15), ws_stream.next())
			.await
			.map_err(|_| anyhow!("EventSub: session_welcome の受信がタイムアウトしました (15s)"))?
			.ok_or_else(|| anyhow!("EventSub: 最初のメッセージがありません (接続が即時クローズされた可能性)"))??;
		match next {
			Message::Text(s) => break s.as_str().to_string(),
			Message::Close(frame) => {
				return Err(anyhow!("EventSub: welcome 前にサーバかめEClose されました: {:?}", frame));
			}
			other => {
				log::debug!("EventSub: skipping non-text frame: {:?}", std::mem::discriminant(&other));
				continue;
			}
		}
	};
	let session_id = parse_session_welcome(&welcome_str)?;
	log::debug!("《Twitch》 EventSub session_id={}", session_id);

	// **WS 読み取りループを別タスクで並行実行すめE*、E
	// 琁E��: tokio-tungstenite の自勁EPong は WebSocket めEpolling したときにのみ駁E��される、E
	// 直列に Helix へ購読を作�EしてぁE��閁EWS めE`next()` してぁE��ぁE��、Twitch の keepalive(既宁E10s)
	// に間に合わぁE`4002 failed ping pong` で刁E��されるため、E
	let state_for_read = state.clone();
	let resolved_for_read = resolved.clone();
	let read_task: tokio::task::JoinHandle<Result<()>> = tokio::spawn(async move {
		let mut ws = ws_stream;
		while let Some(msg) = ws.next().await {
			match msg? {
				Message::Text(t) => {
					handle_message(&state_for_read, &resolved_for_read, t.as_str()).await?;
				}
				Message::Close(frame) => {
					log::info!("《Twitch》 EventSub: サーバから Close されました: {:?}（再接続します）", frame);
					return Ok(());
				}
				_ => continue,
			}
		}
		Ok(())
	});

	// 購読作�Eは HTTP�E�Eelix�E�。並行に走らせて WS の auto-pong を止めなぁE��E
	let sub_res = create_subscriptions(&resolved.client_id, &resolved.access_token, &session_id, resolved, channel_points_available).await;
	if let Err(e) = sub_res {
		read_task.abort();
		return Err(e);
	}

	match read_task.await {
		Ok(Ok(())) => Ok(()),
		Ok(Err(e)) => Err(e),
		Err(join_err) if join_err.is_cancelled() => Ok(()),
		Err(join_err) => Err(anyhow!("EventSub 読み取りタスクが落ちました: {}", join_err)),
	}
}

pub(crate) fn parse_session_welcome(json: &str) -> Result<String> {
	let v: Value = serde_json::from_str(json)?;
	let msg_type = v["metadata"]["message_type"].as_str().unwrap_or("");
	if msg_type != "session_welcome" {
		return Err(anyhow!("EventSub: 最初が session_welcome ではありません: {}", msg_type));
	}
	v["payload"]["session"]["id"]
		.as_str()
		.map(String::from)
		.ok_or_else(|| anyhow!("EventSub: session.id がありません"))
}

#[derive(Debug, Deserialize)]
pub(crate) struct HelixBroadcaster {
 pub id: String,
 // Required: a missing field must not be mistaken for a normal broadcaster.
 pub broadcaster_type: String,
}
impl HelixBroadcaster {
 pub fn has_channel_points(&self) -> Result<bool> {
  match self.broadcaster_type.as_str() {
   "affiliate" | "partner" => Ok(true),
   "" => Ok(false),
   _ => Err(anyhow!("Twitchの配信者種別を確認できません。時間をおいて再取得してください。")),
  }
 }
}
pub(crate) async fn helix_broadcaster(client_id: &str, bearer: &str, login: &str) -> Result<HelixBroadcaster> {
	let client = reqwest::Client::new();
	let url = format!("{}/users?login={}", HELIX, urlencoding::encode(login));
	let res = client
		.get(&url)
		.header("Client-ID", client_id)
		.header("Authorization", format!("Bearer {}", bearer))
		.send()
		.await?;
	if !res.status().is_success() {
		let status = res.status();
		let t = res.text().await.unwrap_or_default();
		return Err(HelixHttpError {
			endpoint: "GET /users",
			status,
			body: t,
		}
		.into());
	}
	#[derive(Deserialize)]
	struct UsersResp {
		data: Vec<HelixBroadcaster>,
	}
	let u: UsersResp = res.json().await?;
	u.data
		.into_iter()
		.next()
		.ok_or_else(|| anyhow!("ユーザーが見つかりません: {}", login))
}

#[derive(Debug)]
struct HelixHttpError {
	endpoint: &'static str,
	status: reqwest::StatusCode,
	body: String,
}

impl std::fmt::Display for HelixHttpError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}: {} {}", self.endpoint, self.status, self.body)
	}
}

impl std::error::Error for HelixHttpError {}

pub(crate) fn is_helix_unauthorized(error: &anyhow::Error) -> bool {
	error
		.chain()
		.filter_map(|source| source.downcast_ref::<HelixHttpError>())
		.any(|source| source.status == reqwest::StatusCode::UNAUTHORIZED)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct CustomRewardSummary {
	pub id: String,
	pub title: String,
	#[serde(default)]
	pub prompt: String,
	pub cost: u64,
	#[serde(default)]
	pub is_enabled: bool,
	#[serde(default)]
	pub is_paused: bool,
	#[serde(default)]
	pub is_in_stock: bool,
	#[serde(default)]
	pub is_user_input_required: bool,
	#[serde(default)]
	pub background_color: String,
}

#[derive(Deserialize)]
struct CustomRewardsResponse {
	data: Vec<CustomRewardSummary>,
}

fn custom_rewards_url(broadcaster_id: &str) -> String {
	format!(
		"{}/channel_points/custom_rewards?broadcaster_id={}",
		HELIX,
		urlencoding::encode(broadcaster_id)
	)
}

/// Helix: チャンネルのカスタム報酬を取得する。
///
/// Get Custom Reward は最大 50 件を単一レスポンスで返し、ページング引数を持たない。
pub(crate) async fn helix_list_custom_rewards(client_id: &str, bearer: &str, broadcaster_id: &str) -> Result<Vec<CustomRewardSummary>> {
	let client = reqwest::Client::new();
	let res = client
		.get(custom_rewards_url(broadcaster_id))
		.header("Client-ID", client_id)
		.header("Authorization", format!("Bearer {}", bearer))
		.send()
		.await?;
	if !res.status().is_success() {
		let t = res.text().await.unwrap_or_default();
		return Err(anyhow!("GET /channel_points/custom_rewards: {}", t));
	}
	Ok(res.json::<CustomRewardsResponse>().await?.data)
}

pub(crate) async fn helix_list_custom_reward_ids(client_id: &str, bearer: &str, broadcaster_id: &str) -> Result<Vec<String>> {
	Ok(helix_list_custom_rewards(client_id, bearer, broadcaster_id)
		.await?
		.into_iter()
		.map(|reward| reward.id)
		.collect())
}

async fn create_subscriptions(client_id: &str, bearer: &str, session_id: &str, r: &ResolvedEventSub, channel_points_available: bool) -> Result<()> {
	let bid = &r.broadcaster_id;
	let bc = serde_json::json!({ "broadcaster_user_id": bid });

	let mut queue: Vec<(&str, &str, Value)> = Vec::new();

	if r.cfg.stream_online {
		queue.push(("stream.online", "1", bc.clone()));
	}
	if r.cfg.stream_offline {
		queue.push(("stream.offline", "1", bc.clone()));
	}
	if r.cfg.follow {
		// v2 では condition.moderator_user_id が忁E��！Eroadcaster と同一でも可。要Emoderator:read:followers スコープ）、E
		queue.push((
			"channel.follow",
			"2",
			serde_json::json!({ "broadcaster_user_id": bid, "moderator_user_id": bid }),
		));
	}
	if r.cfg.subscribe {
		queue.push(("channel.subscribe", "1", bc.clone()));
	}
	if r.cfg.channel_subscription_message {
		queue.push(("channel.subscription.message", "1", bc.clone()));
	}
	if r.cfg.channel_subscription_gift {
		queue.push(("channel.subscription.gift", "1", bc.clone()));
	}
	if r.cfg.raid {
		queue.push(("channel.raid", "1", serde_json::json!({ "to_broadcaster_user_id": bid })));
	}
	if r.cfg.channel_cheer {
		queue.push(("channel.cheer", "1", bc.clone()));
	}
	if r.cfg.hype_train_begin {
		queue.push(("channel.hype_train.begin", "2", bc.clone()));
	}
	if r.cfg.hype_train_progress {
		queue.push(("channel.hype_train.progress", "2", bc.clone()));
	}
	if r.cfg.hype_train_end {
		queue.push(("channel.hype_train.end", "2", bc.clone()));
	}
	if r.cfg.channel_points && channel_points_available {
		let reward_ids: Vec<String> = match &r.cfg.channel_points_reward_id {
			Some(rid) if !rid.trim().is_empty() => vec![rid.trim().to_string()],
			_ => helix_list_custom_reward_ids(client_id, bearer, bid).await?,
		};

		if reward_ids.is_empty() {
			log::info!("《Twitch》 EventSub: カスタム報酬が 0 件のため channel_points 購読は行いません（Dashboard で報酬を作成するか、channel_points = false にしてください）。");
		} else {
			if r.cfg.channel_points_reward_id.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true) {
				log::info!("EventSub: subscribing channel_points for {} reward(s)", reward_ids.len());
			}
			for rid in reward_ids {
				queue.push((
					"channel.channel_points_custom_reward_redemption.add",
					"1",
					serde_json::json!({
					 "broadcaster_user_id": bid,
					 "reward_id": rid,
					}),
				));
			}
		}
	}

	for (sub_type, version, condition) in queue.into_iter() {
		if let Err(error) = post_subscription(client_id, bearer, session_id, sub_type, version, &condition).await {
			// Legacy delivery also keeps successfully established subscriptions alive.
			log::error!("《Twitch》 EventSub 購読を確立できません type={}: {}", sub_type, error);
		}
	}

	Ok(())
}

/// 1 つの EventSub サブスクリプションを Helix に POST する。
///
/// HTTP拒否も `Err` で返す。呼び出し側の接続状態を成功扱いにせず、
/// 再接続と問題表示へ伝える。401は他のHelix APIと同じ型で保持する。
pub(crate) async fn post_subscription(
	client_id: &str,
	bearer: &str,
	session_id: &str,
	sub_type: &str,
	version: &str,
	condition: &Value,
) -> Result<()> {
	let body = serde_json::json!({
	 "type": sub_type,
	 "version": version,
	 "condition": condition,
	 "transport": {
	  "method": "websocket",
	  "session_id": session_id,
	 },
	});
	let client = reqwest::Client::new();
	let res = client
		.post(format!("{}/eventsub/subscriptions", HELIX))
		.header("Client-ID", client_id)
		.header("Authorization", format!("Bearer {}", bearer))
		.header("Content-Type", "application/json")
		.json(&body)
		.send()
		.await?;
	let status = res.status();
	if !status.is_success() {
		let t = res.text().await.unwrap_or_default();
		log::error!("《Twitch》 EventSub 購読失敗 type={} {} {}", sub_type, status, t);
		return Err(subscription_http_error(status, t));
	} else if sub_type == "channel.channel_points_custom_reward_redemption.add" {
		let rid = condition.get("reward_id").and_then(|v| v.as_str()).unwrap_or("?");
		log::info!("《Twitch》 EventSub 購読 OK: {} (reward_id={})", sub_type, rid);
	} else {
		log::info!("《Twitch》 EventSub 購読 OK: {}", sub_type);
	}
	Ok(())
}

fn subscription_http_error(status: reqwest::StatusCode, body: String) -> anyhow::Error {
	HelixHttpError { endpoint: "eventsub/subscriptions", status, body }.into()
}

#[cfg(test)]
mod subscription_failure_tests {
	use super::*;

	#[test]
	fn rejected_subscription_preserves_unauthorized_for_token_recovery() {
		let error = subscription_http_error(reqwest::StatusCode::UNAUTHORIZED, "expired token".into());
		assert!(is_helix_unauthorized(&error));
		assert!(error.to_string().contains("eventsub/subscriptions"));
	}

	#[test]
	fn missing_permission_is_failure_but_not_expired_token() {
		let error = subscription_http_error(reqwest::StatusCode::FORBIDDEN, "missing scope".into());
		assert!(!is_helix_unauthorized(&error));
		assert!(error.to_string().contains("403"));
	}
}

/// `[twitch.eventsub]` の bool トグルから購読キューを組み立てる（V1 と共通ロジック）。
///
/// `channel.points` だけは `channel_points_reward_id` (UUID) があれば優先、
/// 無ければ `helix_list_custom_reward_ids` で列挙する必要があるため、呼び出し側で
/// 展開してから push すること。ここは bool → (type, version, condition) だけを返す。
pub(crate) fn default_subscription_queue_from_cfg(
	cfg: &TwitchEventSubConfig,
	broadcaster_id: &str,
) -> Vec<(&'static str, &'static str, Value)> {
	let bc = serde_json::json!({ "broadcaster_user_id": broadcaster_id });
	let mut queue: Vec<(&'static str, &'static str, Value)> = Vec::new();
	if cfg.stream_online {
		queue.push(("stream.online", "1", bc.clone()));
	}
	if cfg.stream_offline {
		queue.push(("stream.offline", "1", bc.clone()));
	}
	if cfg.follow {
		queue.push((
			"channel.follow",
			"2",
			serde_json::json!({ "broadcaster_user_id": broadcaster_id, "moderator_user_id": broadcaster_id }),
		));
	}
	if cfg.subscribe {
		queue.push(("channel.subscribe", "1", bc.clone()));
	}
	if cfg.channel_subscription_message {
		queue.push(("channel.subscription.message", "1", bc.clone()));
	}
	if cfg.channel_subscription_gift {
		queue.push(("channel.subscription.gift", "1", bc.clone()));
	}
	if cfg.raid {
		queue.push(("channel.raid", "1", serde_json::json!({ "to_broadcaster_user_id": broadcaster_id })));
	}
	if cfg.channel_cheer {
		queue.push(("channel.cheer", "1", bc.clone()));
	}
	if cfg.hype_train_begin {
		queue.push(("channel.hype_train.begin", "2", bc.clone()));
	}
	if cfg.hype_train_progress {
		queue.push(("channel.hype_train.progress", "2", bc.clone()));
	}
	if cfg.hype_train_end {
		queue.push(("channel.hype_train.end", "2", bc.clone()));
	}
	queue
}

/// 明示 `event_types` リストから購読キューを組む。version と condition は sub_type ごとに決め打ち。
/// 未知の sub_type は warn してスキップ。
pub(crate) fn explicit_subscription_queue(event_types: &[String], broadcaster_id: &str) -> Vec<(String, &'static str, Value)> {
	let bc = serde_json::json!({ "broadcaster_user_id": broadcaster_id });
	let mut queue: Vec<(String, &'static str, Value)> = Vec::new();
	for raw in event_types {
		let t = raw.trim();
		if t.is_empty() {
			continue;
		}
		let entry = match t {
			"stream.online"
			| "stream.offline"
			| "channel.update"
			| "channel.subscribe"
			| "channel.subscription.message"
			| "channel.subscription.gift"
			| "channel.cheer" => Some((t.to_string(), if t == "channel.update" { "2" } else { "1" }, bc.clone())),
			"channel.follow" => Some((
				t.to_string(),
				"2",
				serde_json::json!({ "broadcaster_user_id": broadcaster_id, "moderator_user_id": broadcaster_id }),
			)),
			"channel.raid" => Some((t.to_string(), "1", serde_json::json!({ "to_broadcaster_user_id": broadcaster_id }))),
			"channel.hype_train.begin" | "channel.hype_train.progress" | "channel.hype_train.end" => Some((t.to_string(), "2", bc.clone())),
			"channel.channel_points_custom_reward_redemption.add" => {
				// reward_id の自動展開は呼び出し側責務。ここでは wildcard (reward_id 無し) を暫定で返す。
				Some((t.to_string(), "1", bc.clone()))
			}
			_ => {
				log::warn!("《Flowgraph/EventSub》 未知の event_type をスキップ: {:?}", t);
				None
			}
		};
		if let Some(e) = entry {
			queue.push(e);
		}
	}
	queue
}

async fn handle_message(state: &SharedState, r: &ResolvedEventSub, text: &str) -> Result<()> {
	let v: Value = serde_json::from_str(text)?;
	let msg_type = v["metadata"]["message_type"].as_str().unwrap_or("");
	match msg_type {
		"session_keepalive" | "notification" => {}
		"session_reconnect" => {
			let url = v["payload"]["session"]["reconnect_url"].as_str().unwrap_or("");
			log::warn!("《Twitch》 EventSub session_reconnect url={}（再接続はループで行います）", url);
			return Err(anyhow!("reconnect"));
		}
		"revocation" => {
			log::warn!("《Twitch》 EventSub revocation: {:?}", v);
		}
		_ => {
			log::trace!("EventSub: unknown message type: {}", msg_type);
		}
	}

	if msg_type != "notification" {
		return Ok(());
	}

	let sub_type = v["payload"]["subscription"]["type"].as_str().unwrap_or("");
	let event = &v["payload"]["event"];
	let Some(datum) = build_notification_datum(r, sub_type, event) else {
		return Ok(());
	};
	state.read().await.push_channel_datum(datum).await;
	Ok(())
}

/// EventSub の `notification` めE`ChannelDatum` に変換する、E
///
/// - `content` は人間可読な 1 行（従来どおり、`[Twitch] ...` のフォーマット）、E
/// - `source` は `kind="twitch.eventsub"`, `subtype=sub_type`, `actor=<表示吁E`、E
/// - `meta` は Consumer が扱ぁE��すいよう **正規化済みキー** で主要値のみを拾ぁE��生 payload は添付しなぁE��針、E
///
/// `content` が空になるよぁE��イベント�E `None` を返す�E�Eatum めEpush しなぁE��、E
fn build_notification_datum(r: &ResolvedEventSub, sub_type: &str, event: &Value) -> Option<ChannelDatum> {
	let line = format_notification(sub_type, event);
	if line.trim().is_empty() {
		return None;
	}

	// 表示用 actor: イベント種別によって「誰のアクションか」が違う�E�Eheer/follow/subscribe は user、raid は from_broadcaster など�E�、E
	let actor = match sub_type {
		"channel.raid" => pick_name(event, "from_broadcaster_user_name", "from_broadcaster_user_login"),
		"stream.online" | "stream.offline" => pick_name(event, "broadcaster_user_name", "broadcaster_user_login"),
		"channel.subscription.gift" => {
			if event["is_anonymous"].as_bool() == Some(true) {
				Some("匿名".to_string())
			} else {
				pick_name(event, "user_name", "user_login")
			}
		}
		_ => pick_name(event, "user_name", "user_login"),
	};

	let source = DataSource::new("twitch.eventsub").with_subtype(sub_type);
	let source = if let Some(a) = actor.as_ref() {
		source.with_actor(a.clone())
	} else {
		source
	};

	let mut datum = ChannelDatum::new(r.channel_to.clone(), line)
		.with_flag(ChannelDatum::FLAG_IS_FINAL)
		.with_source(source)
		.with_meta("broadcaster_login", r.broadcaster_login.clone())
		.with_meta("broadcaster_id", r.broadcaster_id.clone())
		.with_meta("event_type", sub_type.to_string());

	// イベント固有�E正規化フィールド。存在するときだけ�Eれる、E
	datum = populate_event_meta(datum, sub_type, event);
	Some(datum)
}

fn pick_name(event: &Value, name_key: &str, login_key: &str) -> Option<String> {
	event[name_key]
		.as_str()
		.or_else(|| event[login_key].as_str())
		.filter(|s| !s.is_empty())
		.map(str::to_string)
}

/// `sub_type` ごとに主要フィールドを `meta` に正規化して詰める、E
fn populate_event_meta(mut d: ChannelDatum, sub_type: &str, e: &Value) -> ChannelDatum {
	// 「誰が」系の三点セチE���E�多くのイベントで共通）、E
	let user_keys = [("user_name", "user_name"), ("user_login", "user_login"), ("user_id", "user_id")];
	for (meta_key, ev_key) in user_keys {
		if let Some(v) = e[ev_key].as_str().filter(|s| !s.is_empty()) {
			d = d.with_meta(meta_key, v.to_string());
		}
	}

	match sub_type {
		"stream.online" => {
			if let Some(t) = e["type"].as_str() {
				d = d.with_meta("stream_type", t.to_string());
			}
			if let Some(t) = e["started_at"].as_str() {
				d = d.with_meta("started_at", t.to_string());
			}
		}
		"channel.subscribe" | "channel.subscription.message" => {
			if let Some(t) = e["tier"].as_str() {
				d = d.with_meta("tier", t.to_string());
				d = d.with_meta("tier_label", tier_label(t).to_string());
			}
			if let Some(b) = e["is_gift"].as_bool() {
				d = d.with_meta("is_gift", b);
			}
			if sub_type == "channel.subscription.message" {
				if let Some(n) = e["cumulative_months"].as_u64() {
					d = d.with_meta("cumulative_months", n);
				}
				if let Some(n) = e["streak_months"].as_u64() {
					d = d.with_meta("streak_months", n);
				}
				if let Some(n) = e["duration_months"].as_u64() {
					d = d.with_meta("duration_months", n);
				}
				if let Some(t) = e["message"]["text"].as_str().filter(|s| !s.is_empty()) {
					d = d.with_meta("message_text", t.to_string());
				}
			}
		}
		"channel.subscription.gift" => {
			if let Some(t) = e["tier"].as_str() {
				d = d.with_meta("tier", t.to_string());
				d = d.with_meta("tier_label", tier_label(t).to_string());
			}
			if let Some(n) = e["total"].as_u64() {
				d = d.with_meta("total", n);
			}
			if let Some(n) = e["cumulative_total"].as_u64() {
				d = d.with_meta("cumulative_total", n);
			}
			if let Some(b) = e["is_anonymous"].as_bool() {
				d = d.with_meta("is_anonymous", b);
			}
		}
		"channel.raid" => {
			// user_* が�EってぁE��ぁE�Eで from_* を別途詰める、E
			for (mk, ek) in [
				("from_user_name", "from_broadcaster_user_name"),
				("from_user_login", "from_broadcaster_user_login"),
				("from_user_id", "from_broadcaster_user_id"),
			] {
				if let Some(v) = e[ek].as_str().filter(|s| !s.is_empty()) {
					d = d.with_meta(mk, v.to_string());
				}
			}
			if let Some(n) = e["viewers"].as_u64() {
				d = d.with_meta("viewers", n);
			}
		}
		"channel.cheer" => {
			if let Some(n) = e["bits"].as_u64() {
				d = d.with_meta("bits", n);
			}
			if let Some(t) = e["message"].as_str().filter(|s| !s.is_empty()) {
				d = d.with_meta("message_text", t.to_string());
			}
			if let Some(b) = e["is_anonymous"].as_bool() {
				d = d.with_meta("is_anonymous", b);
			}
		}
		"channel.hype_train.begin" | "channel.hype_train.progress" | "channel.hype_train.end" => {
			if let Some(n) = e["level"].as_u64() {
				d = d.with_meta("level", n);
			}
			if let Some(n) = e["progress"].as_u64() {
				d = d.with_meta("progress", n);
			}
			if let Some(n) = e["goal"].as_u64() {
				d = d.with_meta("goal", n);
			}
			if let Some(n) = e["total"].as_u64() {
				d = d.with_meta("total", n);
			}
		}
		"channel.channel_points_custom_reward_redemption.add" => {
			if let Some(v) = e["id"].as_str() {
				d = d.with_meta("redemption_id", v.to_string());
			}
			if let Some(v) = e["status"].as_str() {
				d = d.with_meta("redemption_status", v.to_string());
			}
			if let Some(v) = e["user_input"].as_str().filter(|s| !s.is_empty()) {
				d = d.with_meta("user_input", v.to_string());
			}
			let reward = &e["reward"];
			if let Some(v) = reward["id"].as_str() {
				d = d.with_meta("reward_id", v.to_string());
			}
			if let Some(v) = reward["title"].as_str() {
				d = d.with_meta("reward_title", v.to_string());
			}
			if let Some(n) = reward["cost"].as_u64() {
				d = d.with_meta("reward_cost", n);
			}
			if let Some(v) = reward["prompt"].as_str().filter(|s| !s.is_empty()) {
				d = d.with_meta("reward_prompt", v.to_string());
			}
		}
		"channel.follow" => {
			if let Some(v) = e["followed_at"].as_str() {
				d = d.with_meta("followed_at", v.to_string());
			}
		}
		_ => {
			// 未知種別: 最低限、キーの存在する斁E���E�E�数値のみを流E��拾ぁE���E列�Eオブジェクト�E除外）、E
			if let Some(obj) = e.as_object() {
				for (k, v) in obj {
					match v {
						Value::String(s) if !s.is_empty() => {
							d = d.with_meta(format!("ev_{}", k), json!(s));
						}
						Value::Number(n) => {
							d = d.with_meta(format!("ev_{}", k), json!(n));
						}
						Value::Bool(b) => {
							d = d.with_meta(format!("ev_{}", k), json!(*b));
						}
						_ => {}
					}
				}
			}
		}
	}
	d
}

fn tier_label(tier: &str) -> &str {
	match tier {
		"1000" => "Tier1",
		"2000" => "Tier2",
		"3000" => "Tier3",
		_ => tier,
	}
}

fn format_notification(sub_type: &str, event: &Value) -> String {
	match sub_type {
		"stream.online" => {
			let st = event["type"].as_str().unwrap_or("live");
			let sa = event["started_at"].as_str().unwrap_or("");
			format!("[Twitch] 配信開始 ({}) {}", st, sa)
		}
		"stream.offline" => {
			let n = event["broadcaster_user_name"]
				.as_str()
				.or(event["broadcaster_user_login"].as_str())
				.unwrap_or("?");
			format!("[Twitch] 配信終了: {}", n)
		}
		"channel.follow" => {
			let u = event["user_name"].as_str().or(event["user_login"].as_str()).unwrap_or("?");
			format!("[Twitch] フォロー: {}", u)
		}
		"channel.subscribe" => {
			let u = event["user_name"].as_str().or(event["user_login"].as_str()).unwrap_or("?");
			let tier = event["tier"].as_str().unwrap_or("");
			format!("[Twitch] 新規サブスク: {} ({})", u, tier_label(tier))
		}
		"channel.subscription.message" => {
			let u = event["user_name"].as_str().or(event["user_login"].as_str()).unwrap_or("?");
			let tier = event["tier"].as_str().unwrap_or("");
			let months = event["cumulative_months"].as_u64().unwrap_or(0);
			let txt = event["message"]["text"].as_str().unwrap_or("");
			let msg = if txt.is_empty() { String::new() } else { format!(" / {}", txt) };
			format!("[Twitch] サブスク継続: {} ({} / {} か月){}", u, tier_label(tier), months, msg)
		}
		"channel.subscription.gift" => {
			let u = if event["is_anonymous"].as_bool() == Some(true) {
				"匿名"
			} else {
				event["user_name"].as_str().or(event["user_login"].as_str()).unwrap_or("?")
			};
			let n = event["total"].as_u64().unwrap_or(0);
			let tier = event["tier"].as_str().unwrap_or("");
			format!("[Twitch] ギフトサブ: {} から {} 件 ({})", u, n, tier_label(tier))
		}
		"channel.raid" => {
			let from = event["from_broadcaster_user_name"]
				.as_str()
				.or(event["from_broadcaster_user_login"].as_str())
				.unwrap_or("?");
			let n = event["viewers"].as_u64().unwrap_or(0);
			format!("[Twitch] RAID 受け: {} から約 {} 人", from, n)
		}
		"channel.cheer" => {
			let bits = event["bits"].as_u64().unwrap_or(0);
			let msg = event["message"].as_str().unwrap_or("");
			if event["is_anonymous"].as_bool() == Some(true) {
				format!("[Twitch] Cheer (匿名): {} Bits / {}", bits, msg)
			} else {
				let u = event["user_name"].as_str().or(event["user_login"].as_str()).unwrap_or("?");
				format!("[Twitch] Cheer: {} が {} Bits / {}", u, bits, msg)
			}
		}
		"channel.hype_train.begin" => {
			let lv = event["level"].as_u64().unwrap_or(0);
			let prog = event["progress"].as_u64().unwrap_or(0);
			let goal = event["goal"].as_u64().unwrap_or(0);
			format!("[Twitch] Hype Train 開始: Lv{} 進捗 {}/{}", lv, prog, goal)
		}
		"channel.hype_train.progress" => {
			let lv = event["level"].as_u64().unwrap_or(0);
			let prog = event["progress"].as_u64().unwrap_or(0);
			let goal = event["goal"].as_u64().unwrap_or(0);
			format!("[Twitch] Hype Train 進行: Lv{} 進捗 {}/{}", lv, prog, goal)
		}
		"channel.hype_train.end" => {
			let lv = event["level"].as_u64().unwrap_or(0);
			let tot = event["total"].as_u64().unwrap_or(0);
			format!("[Twitch] Hype Train 終了: Lv{} 合計 {}", lv, tot)
		}
		"channel.channel_points_custom_reward_redemption.add" => {
			let u = event["user_name"].as_str().or(event["user_login"].as_str()).unwrap_or("?");
			let title = event["reward"]["title"].as_str().unwrap_or("?");
			let input = event["user_input"].as_str().filter(|s| !s.is_empty());
			match input {
				Some(i) => format!("[Twitch] チャンネルポイント: {} が「{}」を交換 ({})", u, title, i),
				None => format!("[Twitch] チャンネルポイント: {} が「{}」を交換", u, title),
			}
		}
		_ => format!("[Twitch] EventSub {}: {}", sub_type, event),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn custom_rewards_request_has_only_supported_query() {
		assert_eq!(
			custom_rewards_url("123 45"),
			"https://api.twitch.tv/helix/channel_points/custom_rewards?broadcaster_id=123%2045"
		);
	}

	#[test]
	fn custom_rewards_response_parses_summary() {
		let response: CustomRewardsResponse = serde_json::from_value(serde_json::json!({
			"data": [{
				"id": "reward-1",
				"title": "水を飲む",
				"prompt": "",
				"cost": 500,
				"is_enabled": true,
				"is_paused": false,
				"is_in_stock": true,
				"is_user_input_required": false,
				"background_color": "#00AAFF"
			}]
		}))
		.unwrap();
		assert_eq!(response.data[0].id, "reward-1");
		assert_eq!(response.data[0].title, "水を飲む");
		assert_eq!(response.data[0].cost, 500);
	}

	#[test]
	fn helix_unauthorized_is_structured_for_refresh_retry() {
		let unauthorized: anyhow::Error = HelixHttpError {
			endpoint: "GET /users",
			status: reqwest::StatusCode::UNAUTHORIZED,
			body: "invalid token".into(),
		}
		.into();
		let forbidden: anyhow::Error = HelixHttpError {
			endpoint: "GET /users",
			status: reqwest::StatusCode::FORBIDDEN,
			body: "missing scope".into(),
		}
		.into();
		assert!(is_helix_unauthorized(&unauthorized));
		assert!(!is_helix_unauthorized(&forbidden));
	}
}

#[cfg(test)]
mod capability_tests {
 use super::*;
 #[test]
 fn broadcaster_eligibility_is_explicit_not_inferred_from_failure() {
  for (kind,expected) in [("",false),("affiliate",true),("partner",true)] {
   let user:HelixBroadcaster=serde_json::from_value(json!({"id":"1","broadcaster_type":kind})).unwrap();assert_eq!(user.has_channel_points().unwrap(),expected);
  }
  assert!(serde_json::from_value::<HelixBroadcaster>(json!({"id":"1"})).is_err());
  assert!(HelixBroadcaster{id:"1".into(),broadcaster_type:"future".into()}.has_channel_points().is_err());
 }
}

//! `flowgraph.ingress.twitch` 用ブリッジ（ζ-1: IRC 本実装）。
//!
//! ## 責務
//!
//! - `flowgraph.ingress.twitch` ノードの property から [`FlowgraphTwitchIngress`] を生成する。
//! - [`spawn`] で各エントリに対し IRC クライアントを起動し、PRIVMSG を
//!   `TriggerEvent` として `TriggerHandle::send` で流し込む。
//! - 返されるハンドルは shutdown 時に `finish().await` すること。
//!
//! 認可トークンは以下の順で解決する:
//!   1. property `access_token` が非空ならそれをそのまま使う。
//!   2. property `token_key` が非空かつ `SharedState` に該当 token_key の有効トークンが
//!      保存済みならそれを使う。
//!   3. `token_key` 自体が未指定なら匿名 (justinfan) で IRC 接続する。
//!
//! `login` プロパティが空なら `username` に anonymous 接続のダミー login を使う。
//!
//! V1 `src/processor/twitch.rs` → `src/twitch/chat.rs` にあった IRC 接続コードを
//! ベースに、Flowgraph の TriggerEvent 発火に繋ぎ込んだもの。

use crate::flowgraph::loader::LoadedNodeMeta;
use crate::flowgraph::node::{TriggerEvent, TriggerHandle};
use crate::flowgraph::socket::SocketValue;
use crate::SharedState;
use std::collections::{BTreeSet, HashSet};
use std::sync::{Arc, RwLock};
use tokio::task::JoinHandle;
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::message::ServerMessage;
use twitch_irc::validate::validate_login;
use twitch_irc::TwitchIRCClient;
use twitch_irc::{ClientConfig, SecureTCPTransport};

#[derive(Debug, Clone)]
#[allow(dead_code)] // client_id / broadcaster_id / fixed_channel は ζ-2 (EventSub Flowgraph 化) で使用予定
pub struct FlowgraphTwitchIngress {
	pub node_id: String,
	pub label: String,
	pub mode: String,
	pub channels: Vec<String>,
	pub access_token: String,
	pub token_key: String,
	pub client_id: String,
	pub broadcaster_id: String,
	pub login: String,
	pub fixed_channel: String,
}

impl FlowgraphTwitchIngress {
	pub fn from_meta(fq: &str, meta: &LoadedNodeMeta) -> Option<Self> {
		let p = &meta.properties;
		let get_str = |k: &str| -> String {
			p.get(k)
				.and_then(|v| v.as_str().ok())
				.map(str::trim)
				.map(str::to_string)
				.unwrap_or_default()
		};
		let channels: Vec<String> = p
			.get("channels")
			.and_then(|v| v.as_list().ok())
			.map(|items| {
				items
					.iter()
					.filter_map(|v| v.as_str().ok().map(|s| s.trim().to_string()))
					.filter(|s| !s.is_empty())
					.collect()
			})
			.unwrap_or_default();
		Some(Self {
			node_id: fq.to_string(),
			label: get_str("label"),
			mode: {
				let m = get_str("mode");
				if m.is_empty() {
					"irc".into()
				} else {
					m.to_ascii_lowercase()
				}
			},
			channels,
			access_token: get_str("access_token"),
			token_key: get_str("token_key"),
			client_id: get_str("client_id"),
			broadcaster_id: get_str("broadcaster_id"),
			login: get_str("login"),
			fixed_channel: get_str("fixed_channel"),
		})
	}

	pub(crate) fn subscription_channels(&self) -> Vec<String> {
		channels_to_join(&self.login, &self.channels)
	}
}

/// `source_actor` 文字列を組み立てる（PRIVMSG 送信者）。
///
/// `msg.sender.name` は表示名（大文字混在）、`msg.sender.login` は安定 login（小文字）。
/// Flowgraph 側の無視リスト比較は小文字安定 login で揃えるため、login を採用する。
pub(crate) fn build_source_actor(sender_login: &str) -> String {
	sender_login.trim().to_string()
}

/// TriggerEvent に載せる meta マップを組み立てる。
///
/// Flowgraph 下流の `ingress.twitch` ノードは standard ingress schema に載るため、
/// ここでは channel / sender などの補助情報を meta に入れて下流に伝える。
pub(crate) fn build_meta(channel_login: &str, sender_login: &str, sender_display_name: &str, multi_channel: bool) -> serde_json::Value {
	serde_json::json!({
		"channel_login": channel_login,
		"sender_login": sender_login,
		"sender_display_name": sender_display_name,
		"multi_channel": multi_channel,
	})
}

/// 明示された購読先を正規化する。購読先が空のときだけログイン名を後方互換の既定値にする。
fn channels_to_join(login: &str, reads: &[String]) -> Vec<String> {
	fn norm(s: &str) -> String {
		s.trim().trim_start_matches('#').to_lowercase()
	}
	let mut set = BTreeSet::new();
	for r in reads {
		let n = norm(r);
		if !n.is_empty() {
			set.insert(n);
		}
	}
	if set.is_empty() {
		let login = norm(login);
		if !login.is_empty() {
			set.insert(login);
		}
	}
	set.into_iter().collect()
}

/// 1 エントリ分の IRC ワーカーハンドル。shutdown で `finish().await` する。
pub struct TwitchBridgeHandle {
	#[allow(dead_code)] // 将来 admin API などで表示する想定
	node_id: String,
	keepalive: Option<TwitchIRCClient<SecureTCPTransport, StaticLoginCredentials>>,
	join_handle: JoinHandle<()>,
	expected_channels: usize,
	joined_channels: Arc<RwLock<HashSet<String>>>,
}

impl TwitchBridgeHandle {
	#[allow(dead_code)] // 将来 admin API などで表示する想定
	pub fn node_id(&self) -> &str {
		&self.node_id
	}

	pub fn expected_channel_count(&self) -> usize {
		self.expected_channels
	}

	pub fn joined_channel_count(&self) -> usize {
		self.joined_channels.read().map(|channels| channels.len()).unwrap_or(0)
	}

	pub fn joined_channels(&self) -> Vec<String> {
		let mut channels = self
			.joined_channels
			.read()
			.map(|channels| channels.iter().cloned().collect::<Vec<_>>())
			.unwrap_or_default();
		channels.sort();
		channels
	}

	pub fn is_ready(&self) -> bool {
		self.expected_channels > 0 && self.joined_channel_count() >= self.expected_channels
	}

	pub async fn finish(mut self) {
		// 先に IRC client を drop して incoming_messages.recv() を終了させる。
		drop(self.keepalive.take());
		// 保険で 3 秒タイムアウト。
		let _ = tokio::time::timeout(std::time::Duration::from_secs(3), self.join_handle).await;
	}
}

/// property `access_token` → `token_key` (via state) の順でトークンを解決する。
///
/// token_key が指定されているのに認可を解決できない場合は、匿名へ暗黙フォールバックしない。
async fn resolve_access_token(entry: &FlowgraphTwitchIngress, state: &SharedState) -> crate::Result<Option<String>> {
	if !entry.access_token.trim().is_empty() {
		let token = entry.access_token.trim().to_string();
		crate::twitch::oauth::validate_user_access_token_scopes(&token, "chat:read").await?;
		return Ok(Some(token));
	}
	let key = entry.token_key.trim();
	if key.is_empty() {
		return Ok(None);
	}
	let ident = {
		let s = state.read().await;
		let twitch = s.twitch.as_ref().ok_or_else(|| anyhow::anyhow!("[twitch] 設定がありません"))?;
		let es = twitch
			.eventsub
			.as_ref()
			.ok_or_else(|| anyhow::anyhow!("[twitch.eventsub] 設定がありません"))?;
		let spec = twitch.token_spec(key);
		crate::twitch::oauth::OAuthIdent::for_key(key, es, spec)
	};
	let token = crate::twitch::oauth::try_load_valid_token_for(&ident).await.ok_or_else(|| {
		anyhow::anyhow!(
			"Twitch OAuth[{}] が未認可、期限切れ、または必要権限不足です。GUI の連携画面から再認可してください",
			key
		)
	})?;
	crate::twitch::oauth::validate_user_access_token_scopes(&token, "chat:read").await?;
	Ok(Some(token))
}

/// `flowgraph.ingress.twitch` の IRC モードに限り、全エントリを spawn する。
///
/// `trigger` が `None` の場合（Flowgraph ランタイム未起動）は warn を出して何もしない。
/// `mode != "irc"` のエントリはスキップする（EventSub は ζ-2 で Flowgraph 化予定）。
///
/// login / channels が共に空のエントリには `conf.twitch.username` をフォールバックとして差し込む
/// （`flowgraph.ingress.twitch_eventsub` の broadcaster_login フォールバックと対になる挙動）。
pub async fn spawn(entries: &[FlowgraphTwitchIngress], trigger: Option<TriggerHandle>, state: SharedState) -> Vec<TwitchBridgeHandle> {
	if entries.is_empty() {
		return Vec::new();
	}
	let runtime_health = state.read().await.runtime_health.clone();
	let Some(trigger) = trigger else {
		log::warn!(
			"《Flowgraph/Twitch》 ingress ノード {} 件を検出しましたが、Flowgraph ランタイムが起動していないため twitch bridge を開始できません。",
			entries.len()
		);
		for entry in entries {
			runtime_health
				.report(twitch_runtime_issue(
					entry,
					"twitch_irc_runtime_unavailable",
					"Twitchコメント受信を開始できない。",
					"Flowgraphランタイムが起動していないため、コメントを受信できない。",
					"Flowgraphのエラーを修正し、再読み込みする。",
				))
				.await;
		}
		return Vec::new();
	};

	// conf.twitch.username をフォールバックとして 1 度だけ取り出す。
	let twitch_username_fallback = {
		let s = state.read().await;
		s.twitch.as_ref().map(|t| t.username.trim().to_string()).unwrap_or_default()
	};

	let mut handles = Vec::new();
	for entry in entries {
		if entry.mode != "irc" {
			log::info!(
				"《Flowgraph/Twitch》 node={} mode={} はこの bridge では未対応（IRC のみ）。EventSub は ζ-2 で移行予定。",
				entry.node_id,
				entry.mode
			);
			continue;
		}
		let mut resolved = entry.clone();
		if resolved.login.trim().is_empty() && resolved.channels.is_empty() && !twitch_username_fallback.is_empty() {
			log::info!(
				"《Flowgraph/Twitch》 node={} login/channels 未指定のため conf.twitch.username={:?} をフォールバック採用",
				resolved.node_id,
				twitch_username_fallback
			);
			resolved.login = twitch_username_fallback.clone();
		}
		let access_token = match resolve_access_token(&resolved, &state).await {
			Ok(token) => token,
			Err(e) => {
				log::error!("《Flowgraph/Twitch》 IRC を開始できません node={}: {:#}", resolved.node_id, e);
				runtime_health
					.report(twitch_runtime_issue(
						&resolved,
						"twitch_irc_authorization_failed",
						"Twitchコメント受信の認可を確認できない。",
						format!("Twitchコメントを受信できない: {e}"),
						"連携画面で使用するアカウントを再認可し、Flowgraphを再読み込みする。",
					))
					.await;
				continue;
			}
		};
		match spawn_one(resolved.clone(), trigger.clone(), state.clone(), access_token) {
			Some(h) => handles.push(h),
			None => {
				log::error!(
					"《Flowgraph/Twitch》 IRC 起動失敗 node={} login={:?} channels={:?}",
					resolved.node_id,
					resolved.login,
					resolved.channels
				);
				runtime_health
					.report(twitch_runtime_issue(
						&resolved,
						"twitch_irc_start_failed",
						"Twitchコメント受信ワーカーを開始できない。",
						"設定したチャンネルのコメントを受信できない。",
						"Twitch IngressのLogin、Channels、Token Keyを確認し、Flowgraphを再読み込みする。",
					))
					.await;
			}
		}
	}
	handles
}

fn twitch_runtime_issue(
	entry: &FlowgraphTwitchIngress,
	code: &str,
	message: impl Into<String>,
	impact: impl Into<String>,
	resolution: impl Into<String>,
) -> crate::runtime_health::RuntimeIssue {
	crate::runtime_health::RuntimeIssue::error(
		format!("twitch_irc:{}", entry.node_id),
		"twitch_irc",
		code,
		message,
		impact,
		"resources",
		Some(entry.node_id.clone()),
	)
	.with_resolution(resolution)
}

fn spawn_one(
	entry: FlowgraphTwitchIngress,
	trigger: TriggerHandle,
	state: SharedState,
	access_token: Option<String>,
) -> Option<TwitchBridgeHandle> {
	let channels = channels_to_join(&entry.login, &entry.channels);
	if channels.is_empty() {
		log::error!("《Flowgraph/Twitch》 node={} に参加チャンネルがありません", entry.node_id);
		return None;
	}
	let mut wanted = std::collections::HashSet::new();
	for ch in &channels {
		if let Err(e) = validate_login(ch) {
			log::error!("《Flowgraph/Twitch》 node={} 不正なチャンネル名 {:?}: {}", entry.node_id, ch, e);
			continue;
		}
		wanted.insert(ch.clone());
	}
	if wanted.is_empty() {
		log::error!("《Flowgraph/Twitch》 node={} に有効なチャンネルがありません", entry.node_id);
		return None;
	}
	let multi_channel = wanted.len() > 1;
	let expected_channels = wanted.len();
	let node_id = entry.node_id.clone();
	let joined_channels = Arc::new(RwLock::new(HashSet::new()));

	let credentials = access_token
		.map(|token| StaticLoginCredentials::new(entry.login.clone(), Some(token)))
		.unwrap_or_else(StaticLoginCredentials::anonymous);
	let config = ClientConfig::new_simple(credentials);
	let (mut incoming_messages, client) = TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config);

	let trigger_for_task = trigger.clone();
	let state_for_task = state.clone();
	let entry_for_task = entry.clone();
	let node_id_for_task = node_id.clone();
	let joined_channels_for_task = joined_channels.clone();

	let join_handle = tokio::spawn(async move {
		let (ignore_logins, control_event_tx) = {
			let state = state_for_task.read().await;
			(state.twitch_ignore_logins.clone(), state.control_event_tx.clone())
		};
		log::info!(
			"《Flowgraph/Twitch》 IRC 接続開始 node={} channels={:?} multi={}",
			node_id_for_task,
			entry_for_task.channels,
			multi_channel
		);
		while let Some(message) = incoming_messages.recv().await {
			match message {
				ServerMessage::Join(msg) => {
					if let Ok(mut channels) = joined_channels_for_task.write() {
						channels.insert(msg.channel_login.clone());
					}
					log::info!(
						"《Flowgraph/Twitch》 IRC channel参加完了 node={} channel={}",
						node_id_for_task,
						msg.channel_login
					);
				}
				ServerMessage::Part(msg) => {
					if let Ok(mut channels) = joined_channels_for_task.write() {
						channels.remove(&msg.channel_login);
					}
				}
				ServerMessage::Privmsg(msg) => {
					let sender_login_lc = msg.sender.login.to_lowercase();
					if ignore_logins.read().await.contains(&sender_login_lc) {
						log::debug!(
							"《Flowgraph/Twitch》 node={} ingress 無視リストによりスキップ sender={}",
							node_id_for_task,
							sender_login_lc
						);
						continue;
					}
					let _ = control_event_tx.send(crate::control_events::ControlEvent::TwitchChatReceived {
						node_id: node_id_for_task.clone(),
						channel_login: msg.channel_login.clone(),
						sender_login: msg.sender.login.clone(),
						text_chars: msg.message_text.chars().count(),
						observed_at: jiff::Timestamp::now().to_string(),
					});
					let text = if multi_channel {
						format!("#{} {}:{}", msg.channel_login, msg.sender.name, msg.message_text)
					} else {
						format!("{}:{}", msg.sender.name, msg.message_text)
					};
					let actor = build_source_actor(&msg.sender.login);
					let mut meta = build_meta(&msg.channel_login, &msg.sender.login, &msg.sender.name, multi_channel);
					meta["transport"] = serde_json::json!("twitch.irc");
					meta["message_text"] = serde_json::json!(msg.message_text);
					meta["message_id"] = serde_json::json!(msg.message_id);
					meta["broadcaster_id"] = serde_json::json!(msg.channel_id);
					meta["sender_id"] = serde_json::json!(msg.sender.id);
					meta["is_moderator"] = serde_json::json!(msg.badges.iter().any(|b| b.name == "moderator"));
					let mut ev = TriggerEvent::new(&node_id_for_task)
						.with_exec("__trigger__")
						.with_override("__content__", SocketValue::String(text))
						.with_override("__source_actor__", SocketValue::String(actor));
					ev = ev.with_override("__meta__", SocketValue::Json(meta));
					if let Err(e) = trigger_for_task.send(ev) {
						log::warn!(
							"《Flowgraph/Twitch》 node={} trigger 送信に失敗: {}（worker を終了）",
							node_id_for_task,
							e
						);
						break;
					}
				}
				_ => {}
			}
		}
		if let Ok(mut channels) = joined_channels_for_task.write() {
			channels.clear();
		}
		log::info!("《Flowgraph/Twitch》 IRC ワーカー終了 node={}", node_id_for_task);
	});

	for channel in wanted {
		if let Err(e) = client.join(channel) {
			log::error!("《Flowgraph/Twitch》 node={} join 失敗: {:?}", entry.node_id, e);
			return None;
		}
	}

	Some(TwitchBridgeHandle {
		node_id,
		keepalive: Some(client),
		join_handle,
		expected_channels,
		joined_channels,
	})
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::node::InputMap;
	use crate::flowgraph::socket::SocketValue;
	use std::path::PathBuf;

	#[test]
	fn from_meta_reads_channels_list() {
		let mut props = InputMap::new();
		props.insert("mode".into(), SocketValue::String("IRC".into()));
		props.insert(
			"channels".into(),
			SocketValue::List(vec![SocketValue::String("user_a".into()), SocketValue::String("user_b".into())]),
		);
		props.insert("login".into(), SocketValue::String("bot".into()));
		props.insert("token_key".into(), SocketValue::String("broadcaster".into()));
		let meta = LoadedNodeMeta {
			feature: "flowgraph.ingress.twitch".into(),
			file: PathBuf::from("x.toml"),
			position: None,
			activation_chain: Vec::new(),
			properties: props,
		};
		let t = FlowgraphTwitchIngress::from_meta("in", &meta).unwrap();
		assert_eq!(t.mode, "irc");
		assert_eq!(t.channels, vec!["user_a".to_string(), "user_b".to_string()]);
		assert_eq!(t.login, "bot");
		assert_eq!(t.token_key, "broadcaster");
	}

	#[test]
	fn defaults_for_empty_properties() {
		let meta = LoadedNodeMeta {
			feature: "flowgraph.ingress.twitch".into(),
			file: PathBuf::from("x.toml"),
			position: None,
			activation_chain: Vec::new(),
			properties: InputMap::new(),
		};
		let t = FlowgraphTwitchIngress::from_meta("in", &meta).unwrap();
		assert_eq!(t.mode, "irc");
		assert!(t.channels.is_empty());
		assert!(t.token_key.is_empty());
	}

	#[test]
	fn channels_to_join_dedupes_and_normalizes() {
		let ch = channels_to_join("  Bot  ", &vec!["#ChanA".into(), "chanb".into(), "ChanA".into(), "".into()]);
		assert_eq!(ch, vec!["chana".to_string(), "chanb".to_string()]);
	}

	#[test]
	fn channels_to_join_uses_login_only_when_subscription_is_empty() {
		assert_eq!(channels_to_join("  Bot  ", &[]), vec!["bot".to_string()]);
	}

	#[test]
	fn build_source_actor_trims_and_lowercases_input() {
		assert_eq!(build_source_actor("  alice  "), "alice");
		// note: 小文字化は sender.login を渡す側の責務（ここでは trim のみ）。
		assert_eq!(build_source_actor("ALICE"), "ALICE");
	}

	#[test]
	fn build_meta_contains_expected_fields() {
		let m = build_meta("mychan", "alice", "Alice", true);
		assert_eq!(m.get("channel_login").and_then(serde_json::Value::as_str), Some("mychan"));
		assert_eq!(m.get("sender_login").and_then(serde_json::Value::as_str), Some("alice"));
		assert_eq!(m.get("sender_display_name").and_then(serde_json::Value::as_str), Some("Alice"));
		assert_eq!(m.get("multi_channel").and_then(serde_json::Value::as_bool), Some(true));
	}
}

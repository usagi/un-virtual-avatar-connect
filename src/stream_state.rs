//! 配信宣言値の観測状態。
//!
//! Twitch の宣言値（配信中、タイトル、カテゴリー等）を、画像から推定した実内容とは
//! 混ぜずに保持する。値ごとに観測時刻と情報源を残し、参照時に鮮度を計算する。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

pub const DEFAULT_FRESHNESS_SECS: u64 = 300;
pub const TWITCH_STREAM_STATE_CHANNEL: &str = "twitch:stream_state";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TwitchObservationSource {
	HelixStartup,
	HelixReconnect,
	HelixStreamOnline,
	EventsubChannelUpdate,
	EventsubStreamOnline,
	EventsubStreamOffline,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservedField<T> {
	pub value: T,
	pub observed_at: String,
	pub observed_at_unix_ms: u64,
	pub source: TwitchObservationSource,
}

impl<T: Clone> ObservedField<T> {
	fn view(&self, now_ms: u64, freshness_secs: u64) -> ObservedFieldView<T> {
		let age_ms = now_ms.saturating_sub(self.observed_at_unix_ms);
		ObservedFieldView {
			value: self.value.clone(),
			observed_at: self.observed_at.clone(),
			source: self.source.clone(),
			age_ms,
			fresh: age_ms <= freshness_secs.saturating_mul(1000),
		}
	}
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ObservedFieldView<T> {
	pub value: T,
	pub observed_at: String,
	pub source: TwitchObservationSource,
	pub age_ms: u64,
	pub fresh: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TwitchStreamState {
	pub broadcaster_id: String,
	pub broadcaster_login: String,
	pub online: Option<ObservedField<bool>>,
	pub started_at: Option<ObservedField<Option<String>>>,
	pub title: Option<ObservedField<String>>,
	pub category_id: Option<ObservedField<String>>,
	pub category_name: Option<ObservedField<String>>,
	pub language: Option<ObservedField<String>>,
	pub classification_labels: Option<ObservedField<Vec<String>>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TwitchStreamStateView {
	pub broadcaster_id: String,
	pub broadcaster_login: String,
	pub freshness_secs: u64,
	pub online: Option<ObservedFieldView<bool>>,
	pub started_at: Option<ObservedFieldView<Option<String>>>,
	pub title: Option<ObservedFieldView<String>>,
	pub category_id: Option<ObservedFieldView<String>>,
	pub category_name: Option<ObservedFieldView<String>>,
	pub language: Option<ObservedFieldView<String>>,
	pub classification_labels: Option<ObservedFieldView<Vec<String>>>,
}

#[derive(Debug, Default)]
pub struct TwitchStreamStateRegistry {
	inner: RwLock<TwitchStreamState>,
}

impl TwitchStreamStateRegistry {
	pub async fn snapshot(&self, freshness_secs: u64) -> TwitchStreamStateView {
		let state = self.inner.read().await;
		let now_ms = unix_ms();
		TwitchStreamStateView {
			broadcaster_id: state.broadcaster_id.clone(),
			broadcaster_login: state.broadcaster_login.clone(),
			freshness_secs,
			online: state.online.as_ref().map(|v| v.view(now_ms, freshness_secs)),
			started_at: state.started_at.as_ref().map(|v| v.view(now_ms, freshness_secs)),
			title: state.title.as_ref().map(|v| v.view(now_ms, freshness_secs)),
			category_id: state.category_id.as_ref().map(|v| v.view(now_ms, freshness_secs)),
			category_name: state.category_name.as_ref().map(|v| v.view(now_ms, freshness_secs)),
			language: state.language.as_ref().map(|v| v.view(now_ms, freshness_secs)),
			classification_labels: state.classification_labels.as_ref().map(|v| v.view(now_ms, freshness_secs)),
		}
	}

	pub async fn apply_helix(&self, update: HelixChannelSnapshot, source: TwitchObservationSource) -> TwitchStreamStateView {
		let observed_at = jiff::Timestamp::now().to_string();
		let observed_at_unix_ms = unix_ms();
		let mut state = self.inner.write().await;
		state.broadcaster_id = update.broadcaster_id;
		state.broadcaster_login = update.broadcaster_login;
		state.online = Some(field(update.online, &observed_at, observed_at_unix_ms, &source));
		state.started_at = Some(field(update.started_at, &observed_at, observed_at_unix_ms, &source));
		state.title = Some(field(update.title, &observed_at, observed_at_unix_ms, &source));
		state.category_id = Some(field(update.category_id, &observed_at, observed_at_unix_ms, &source));
		state.category_name = Some(field(update.category_name, &observed_at, observed_at_unix_ms, &source));
		state.language = Some(field(update.language, &observed_at, observed_at_unix_ms, &source));
		state.classification_labels = Some(field(update.classification_labels, &observed_at, observed_at_unix_ms, &source));
		drop(state);
		self.snapshot(DEFAULT_FRESHNESS_SECS).await
	}

	pub async fn apply_eventsub(&self, sub_type: &str, event: &Value) -> Option<TwitchStreamStateView> {
		let source = match sub_type {
			"channel.update" => TwitchObservationSource::EventsubChannelUpdate,
			"stream.online" => TwitchObservationSource::EventsubStreamOnline,
			"stream.offline" => TwitchObservationSource::EventsubStreamOffline,
			_ => return None,
		};
		let observed_at = jiff::Timestamp::now().to_string();
		let observed_at_unix_ms = unix_ms();
		let mut state = self.inner.write().await;
		if let Some(v) = event.get("broadcaster_user_id").and_then(Value::as_str) {
			state.broadcaster_id = v.to_string();
		}
		if let Some(v) = event.get("broadcaster_user_login").and_then(Value::as_str) {
			state.broadcaster_login = v.to_string();
		}
		match sub_type {
			"channel.update" => {
				set_string(&mut state.title, event, "title", &observed_at, observed_at_unix_ms, &source);
				set_string(
					&mut state.category_id,
					event,
					"category_id",
					&observed_at,
					observed_at_unix_ms,
					&source,
				);
				set_string(
					&mut state.category_name,
					event,
					"category_name",
					&observed_at,
					observed_at_unix_ms,
					&source,
				);
				set_string(&mut state.language, event, "language", &observed_at, observed_at_unix_ms, &source);
				if let Some(labels) = string_array(event.get("content_classification_labels")) {
					state.classification_labels = Some(field(labels, &observed_at, observed_at_unix_ms, &source));
				}
			}
			"stream.online" => {
				state.online = Some(field(true, &observed_at, observed_at_unix_ms, &source));
				let started_at = event.get("started_at").and_then(Value::as_str).map(str::to_string);
				state.started_at = Some(field(started_at, &observed_at, observed_at_unix_ms, &source));
			}
			"stream.offline" => {
				state.online = Some(field(false, &observed_at, observed_at_unix_ms, &source));
				state.started_at = Some(field(None, &observed_at, observed_at_unix_ms, &source));
			}
			_ => unreachable!(),
		}
		drop(state);
		Some(self.snapshot(DEFAULT_FRESHNESS_SECS).await)
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelixChannelSnapshot {
	pub broadcaster_id: String,
	pub broadcaster_login: String,
	pub online: bool,
	pub started_at: Option<String>,
	pub title: String,
	pub category_id: String,
	pub category_name: String,
	pub language: String,
	pub classification_labels: Vec<String>,
}

pub fn parse_helix_snapshot(channel_response: &Value, streams_response: &Value) -> anyhow::Result<HelixChannelSnapshot> {
	let channel = channel_response
		.get("data")
		.and_then(Value::as_array)
		.and_then(|v| v.first())
		.ok_or_else(|| anyhow::anyhow!("Helix Get Channel Information returned no channel"))?;
	let stream = streams_response.get("data").and_then(Value::as_array).and_then(|v| v.first());
	Ok(HelixChannelSnapshot {
		broadcaster_id: required_string(channel, "broadcaster_id")?,
		broadcaster_login: required_string(channel, "broadcaster_login")?,
		online: stream.is_some(),
		started_at: stream.and_then(|v| v.get("started_at")).and_then(Value::as_str).map(str::to_string),
		title: required_string(channel, "title")?,
		category_id: required_string(channel, "game_id")?,
		category_name: required_string(channel, "game_name")?,
		language: required_string(channel, "broadcaster_language")?,
		classification_labels: string_array(channel.get("content_classification_labels")).unwrap_or_default(),
	})
}

pub fn state_observation_content(view: &TwitchStreamStateView) -> String {
	let online = view
		.online
		.as_ref()
		.map(|v| if v.value { "配信中" } else { "配信外" })
		.unwrap_or("不明");
	let title = view.title.as_ref().map(|v| v.value.as_str()).unwrap_or("");
	let category = view.category_name.as_ref().map(|v| v.value.as_str()).unwrap_or("");
	format!("[Twitch配信状態] {online} / カテゴリー: {category} / タイトル: {title}")
}

pub(crate) fn state_observation_datum(view: &crate::stream_state::TwitchStreamStateView) -> crate::ChannelDatum {
	let observation = crate::stream_state::state_observation_meta(view);
	crate::ChannelDatum::new(
		crate::stream_state::TWITCH_STREAM_STATE_CHANNEL.to_string(),
		crate::stream_state::state_observation_content(view),
	)
	.with_flag(crate::ChannelDatum::FLAG_IS_FINAL)
	.with_source(crate::DataSource::new("twitch.stream_state").with_subtype("declared"))
	.with_meta(crate::ChannelDatum::META_STATE_KEY, "twitch_declared_stream_state")
	.with_meta("observation_kind", "twitch_declared_stream_state")
	.with_meta("trust", "external_declared_value")
	.with_meta("state", observation["state"].clone())
	.with_attachment(crate::Attachment::json_with_label(observation, "twitch.declared_stream_state"))
}

pub fn state_observation_meta(view: &TwitchStreamStateView) -> Value {
	json!({
		"observation_kind": "twitch_declared_stream_state",
		"state": view,
		"trust": "external_declared_value",
	})
}

fn field<T>(value: T, observed_at: &str, observed_at_unix_ms: u64, source: &TwitchObservationSource) -> ObservedField<T> {
	ObservedField {
		value,
		observed_at: observed_at.to_string(),
		observed_at_unix_ms,
		source: source.clone(),
	}
}

fn set_string(
	target: &mut Option<ObservedField<String>>,
	event: &Value,
	key: &str,
	observed_at: &str,
	observed_at_unix_ms: u64,
	source: &TwitchObservationSource,
) {
	if let Some(value) = event.get(key).and_then(Value::as_str) {
		*target = Some(field(value.to_string(), observed_at, observed_at_unix_ms, source));
	}
}

fn required_string(value: &Value, key: &str) -> anyhow::Result<String> {
	value
		.get(key)
		.and_then(Value::as_str)
		.map(str::to_string)
		.ok_or_else(|| anyhow::anyhow!("Helix channel field `{key}` is missing"))
}

fn string_array(value: Option<&Value>) -> Option<Vec<String>> {
	value?
		.as_array()
		.map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

pub(crate) fn unix_ms() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.unwrap_or_default()
		.as_millis()
		.try_into()
		.unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn helix_snapshot_keeps_declared_values_and_live_status_separate() {
		let snapshot = parse_helix_snapshot(
			&json!({"data": [{
				"broadcaster_id": "1", "broadcaster_login": "usagi", "broadcaster_language": "ja",
				"game_id": "42", "game_name": "Science & Technology", "title": "研究配信",
				"content_classification_labels": ["MatureGame"]
			}]}),
			&json!({"data": [{"started_at": "2026-08-02T00:00:00Z"}]}),
		)
		.unwrap();
		assert!(snapshot.online);
		assert_eq!(snapshot.category_id, "42");
		assert_eq!(snapshot.classification_labels, vec!["MatureGame"]);
	}

	#[tokio::test]
	async fn eventsub_channel_update_changes_only_declared_fields() {
		let registry = TwitchStreamStateRegistry::default();
		registry
			.apply_eventsub(
				"stream.online",
				&json!({"broadcaster_user_id":"1", "broadcaster_user_login":"usagi", "started_at":"now"}),
			)
			.await;
		let view = registry
			.apply_eventsub(
				"channel.update",
				&json!({"title":"new", "category_id":"2", "category_name":"Game", "language":"ja", "content_classification_labels":[]}),
			)
			.await
			.unwrap();
		assert_eq!(view.online.unwrap().value, true);
		assert_eq!(view.title.unwrap().source, TwitchObservationSource::EventsubChannelUpdate);
	}

	#[tokio::test]
	async fn eventsub_stream_offline_clears_live_state_with_its_own_source() {
		let registry = TwitchStreamStateRegistry::default();
		registry
			.apply_eventsub(
				"stream.online",
				&json!({
					"broadcaster_user_id": "1",
					"broadcaster_user_login": "usagi",
					"started_at": "2026-08-07T10:03:43Z"
				}),
			)
			.await;

		let view = registry
			.apply_eventsub(
				"stream.offline",
				&json!({"broadcaster_user_id": "1", "broadcaster_user_login": "usagi"}),
			)
			.await
			.unwrap();

		let online = view.online.unwrap();
		assert!(!online.value);
		assert_eq!(online.source, TwitchObservationSource::EventsubStreamOffline);
		assert!(online.fresh);

		let started_at = view.started_at.unwrap();
		assert_eq!(started_at.value, None);
		assert_eq!(started_at.source, TwitchObservationSource::EventsubStreamOffline);
		assert!(started_at.fresh);
	}

	#[tokio::test]
	async fn declared_state_exposes_trust_source_and_freshness_without_image_inference() {
		let registry = TwitchStreamStateRegistry::default();
		let view = registry
			.apply_helix(
				HelixChannelSnapshot {
					broadcaster_id: "1".into(),
					broadcaster_login: "usagi".into(),
					online: false,
					started_at: None,
					title: "研究配信".into(),
					category_id: "42".into(),
					category_name: "Science & Technology".into(),
					language: "ja".into(),
					classification_labels: vec!["MatureGame".into()],
				},
				TwitchObservationSource::HelixReconnect,
			)
			.await;

		let title = view.title.as_ref().unwrap();
		assert_eq!(title.source, TwitchObservationSource::HelixReconnect);
		assert!(title.fresh);
		assert!(title.observed_at.parse::<jiff::Timestamp>().is_ok());

		let meta = state_observation_meta(&view);
		assert_eq!(meta["trust"], "external_declared_value");
		assert_eq!(meta["observation_kind"], "twitch_declared_stream_state");
		assert_eq!(meta["state"]["category_name"]["value"], "Science & Technology");

		{
			let mut state = registry.inner.write().await;
			state.title.as_mut().unwrap().observed_at_unix_ms = unix_ms().saturating_sub(600_000);
		}
		let stale = registry.snapshot(DEFAULT_FRESHNESS_SECS).await;
		assert!(!stale.title.as_ref().unwrap().fresh);
		let datum = state_observation_datum(&stale);
		assert_eq!(datum.meta["state"]["title"]["observed_at"], view.title.as_ref().unwrap().observed_at);
		assert_eq!(datum.meta["state"]["title"]["fresh"], false);
		assert!(datum.meta["state"]["title"]["age_ms"].as_u64().unwrap() >= 600_000);
		assert_eq!(datum.meta["state"]["category_name"]["fresh"], true);
	}
}

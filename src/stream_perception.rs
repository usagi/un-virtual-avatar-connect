//! 配信中の外部画面を、AI会話時に参照できる最新状態として保持する。
//!
//! 画像取得自体はローカルOBSだけで完結し、取得周期ごとにAI APIを呼ばない。
//! `ChannelDatum`はsource単位で最新1件へ置換され、personaが次に通常評価を行う際だけ
//! `include_additional`経由でmultipart入力へ入る。

use crate::shutdown::ShutdownBroker;
use crate::state::SharedState;
use crate::web_interface::control::obs_observation::{
	capture_and_publish, publish_no_visual_content, CaptureObservationError, ObsSourceObservationSpec,
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct StreamPerceptionConf {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub image_archive: Option<crate::diagnostic_images::ImageArchiveConf>,
	#[serde(default)]
	pub obs_sources: Vec<ObsPerceptionSourceConf>,
	/// Retained for explicit restore. Never passed to the capture runtime.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub archived_obs_sources: Vec<ObsPerceptionSourceConf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ObsPerceptionSourceConf {
	pub id: String,
	#[serde(default)]
	pub enabled: bool,
	/// Opt in to a bounded refresh immediately before an AI request.
	#[serde(default)]
	pub refresh_before_reply: bool,
	#[serde(default = "default_obs_url")]
	pub url: String,
	#[serde(default)]
	pub password: String,
	#[serde(default)]
	pub password_env: Option<String>,
	/// 同一ユーザーのOBS WebSocket設定から認証情報を読む。秘密をVAC profileへ複製しないためのローカル専用経路。
	#[serde(default)]
	pub use_local_obs_config_password: bool,
	pub scene_name: String,
	pub source_name: String,
	#[serde(default = "default_image_format")]
	pub image_format: String,
	#[serde(default = "default_width")]
	pub width: i64,
	#[serde(default = "default_height")]
	pub height: i64,
	#[serde(default = "default_timeout_ms")]
	pub timeout_ms: i64,
	#[serde(default = "default_interval_secs")]
	pub interval_secs: u64,
	#[serde(default = "default_channel")]
	pub channel: String,
	/// 空なら全mode。配信専用なら `["streaming"]` を指定する。
	#[serde(default)]
	pub active_modes: Vec<String>,
}

impl StreamPerceptionConf {
	pub fn validate(&self) -> Result<()> {
		if let Some(archive) = &self.image_archive { archive.validate()?; }
		let mut ids = HashSet::new();
		for source in self.obs_sources.iter().chain(&self.archived_obs_sources) {
			let id = source.id.trim();
			if id.is_empty() {
				bail!("[[perception.obs_sources]].id は空にできません");
			}
			if !ids.insert(id) {
				bail!("[[perception.obs_sources]] の id が重複しています: {id}");
			}
			if source.scene_name.trim().is_empty() || source.source_name.trim().is_empty() {
				bail!("perception source `{id}` は scene_name と source_name の明示が必要です");
			}
			if !(1..=3600).contains(&source.interval_secs) {
				bail!("perception source `{id}` の interval_secs は 1..=3600 が必要です");
			}
			if !(64..=4096).contains(&source.width) || !(64..=4096).contains(&source.height) {
				bail!("perception source `{id}` の width/height は 64..=4096 が必要です");
			}
			if !(100..=30_000).contains(&source.timeout_ms) {
				bail!("perception source `{id}` の timeout_ms は 100..=30000 が必要です");
			}
			if !matches!(source.image_format.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg") {
				bail!("perception source `{id}` の image_format は png/jpeg のみです");
			}
			if source.password_env.as_ref().is_some_and(|value| value.trim().is_empty()) {
				bail!("perception source `{id}` の password_env は空にできません");
			}
		}
		Ok(())
	}
}

#[derive(Clone)]
pub(crate) struct StreamPerceptionRuntime {
	sources: Vec<Arc<ObsCaptureSource>>,
	// Restored state may still contain the last image from a disabled or
	// archived source. Capture suppression alone cannot remove that input.
	suppressed_sources: Vec<(String, String)>,
}

impl std::fmt::Debug for StreamPerceptionRuntime {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("StreamPerceptionRuntime")
			.field("source_count", &self.sources.len())
			.finish()
	}
}

struct ObsCaptureSource {
	conf: ObsPerceptionSourceConf,
	// Both periodic and reply-time acquisition use this lock. Its boolean
	// tracks blank publication, so the two paths cannot leave stale images.
	blank: tokio::sync::Mutex<bool>,
}

#[derive(Default)]
pub(crate) struct ReplyRefreshResult {
	pub attempted: bool,
	pub failed: bool,
	unrefreshed: Vec<(String, String)>,
}

impl ReplyRefreshResult {
	pub(crate) fn exclude_unrefreshed(&self, observations: &mut Vec<crate::ChannelDatum>) {
		observations.retain(|datum| {
			!self.unrefreshed.iter().any(|(channel, key)| {
				&datum.channel == channel
					&& datum
						.meta_get(crate::ChannelDatum::META_STATE_KEY)
						.and_then(serde_json::Value::as_str)
						== Some(key.as_str())
			})
		});
	}
}

impl StreamPerceptionRuntime {
	pub fn new(conf: &StreamPerceptionConf) -> Self {
		Self {
			suppressed_sources: conf.obs_sources.iter().filter(|source| !source.enabled)
				.chain(conf.archived_obs_sources.iter())
				.map(|source| (source.channel.clone(), format!("obs_source:{}", source.id.trim())))
				.collect(),
			sources: conf
				.obs_sources
				.iter()
				.filter(|source| source.enabled)
				.cloned()
				.map(|conf| {
					Arc::new(ObsCaptureSource {
						conf,
						blank: tokio::sync::Mutex::new(false),
					})
				})
				.collect(),
		}
	}

	pub async fn refresh_for_reply(&self, state: &SharedState, observe: &crate::ai::observe::ObserveSet) -> ReplyRefreshResult {
		let mode = state.read().await.runtime_mode_id.read().ok().and_then(|value| value.clone());
		self.refresh_with(mode.as_deref(), observe, Duration::from_secs(2), |source| async move {
			source.capture(state, true).await
		})
		.await
	}

	async fn refresh_with<F, Fut>(
		&self,
		mode: Option<&str>,
		observe: &crate::ai::observe::ObserveSet,
		budget: Duration,
		mut capture: F,
	) -> ReplyRefreshResult
	where
		F: FnMut(Arc<ObsCaptureSource>) -> Fut,
		Fut: std::future::Future<Output = Result<bool, CaptureObservationError>>,
	{
		let selected: Vec<_> = self
			.sources
			.iter()
			.filter(|source| {
				source.conf.refresh_before_reply && observe.is_observed(&source.conf.channel) && source_active_in_mode(&source.conf, mode)
			})
			.collect();
		let mut result = ReplyRefreshResult {
			attempted: !selected.is_empty(),
			failed: false,
			unrefreshed: selected
				.iter()
				.map(|source| (source.conf.channel.clone(), format!("obs_source:{}", source.conf.id.trim())))
				.collect(),
		};
		// One shared budget includes lock waits. Mark all selected sources first:
		// timeout can also leave later sources unattempted with an old image.
		let work = async {
			for source in selected {
				if matches!(capture(source.clone()).await, Ok(true)) {
					let key = format!("obs_source:{}", source.conf.id.trim());
					result
						.unrefreshed
						.retain(|(channel, retained)| channel != &source.conf.channel || retained != &key);
				}
			}
		};
		let _ = tokio::time::timeout(budget, work).await;
		result.failed = !result.unrefreshed.is_empty();
		// Switching modes stops acquisition, but the state channel still holds
		// its last frame. Exclude it from this reply without reporting an
		// intentional mode restriction as a capture failure. This also covers
		// periodic-only sources; no additional capture is requested.
		self.restrict_reply_to_mode(&mut result, mode);
		result
	}

	/// Recheck after asynchronous acquisition, at the input assembly boundary.
	/// A mode restriction is intentional, not a capture failure.
	pub(crate) fn restrict_reply_to_mode(&self, result: &mut ReplyRefreshResult, mode: Option<&str>) {
		for key in &self.suppressed_sources {
			if !result.unrefreshed.contains(key) { result.unrefreshed.push(key.clone()); }
		}
		for source in self.sources.iter().filter(|source| !source_active_in_mode(&source.conf, mode)) {
			let key = (source.conf.channel.clone(), format!("obs_source:{}", source.conf.id.trim()));
			if !result.unrefreshed.contains(&key) { result.unrefreshed.push(key); }
		}
	}

	async fn capture_periodic(&self, id: &str, state: &SharedState) -> Result<bool, CaptureObservationError> {
		let source = self
			.sources
			.iter()
			.find(|source| source.conf.id == id)
			.ok_or_else(|| CaptureObservationError::Upstream("configured capture source is unavailable".into()))?;
		source.capture(state, false).await
	}
}

impl ObsCaptureSource {
	async fn capture(&self, state: &SharedState, refresh: bool) -> Result<bool, CaptureObservationError> {
		let mut blank = self.blank.lock().await;
		if !source_active_in_current_mode(&self.conf, state).await {
			return Ok(false);
		}
		let spec = self.conf.to_observation_spec();
		let key = format!("obs_source:{}", self.conf.id.trim());
		match capture_and_publish(state, &spec, Some(&key), true).await {
			Ok(_) => {
				*blank = false;
				Ok(true)
			}
			Err(CaptureObservationError::NoVisualContent) => {
				if !*blank || refresh {
					publish_no_visual_content(state, &spec, &key).await;
				}
				*blank = true;
				Ok(true)
			}
			Err(error) => Err(error),
		}
	}
}

pub fn spawn_all(conf: &StreamPerceptionConf, state: SharedState, shutdown: Arc<ShutdownBroker>) -> Vec<tokio::task::JoinHandle<()>> {
	conf.obs_sources
		.iter()
		.filter(|source| source.enabled)
		.cloned()
		.map(|source| {
			let state = state.clone();
			let shutdown = shutdown.clone();
			tokio::spawn(async move { run_obs_source(source, state, shutdown).await })
		})
		.collect()
}

async fn run_obs_source(source: ObsPerceptionSourceConf, state: SharedState, shutdown: Arc<ShutdownBroker>) {
	let issue_key = format!("stream_perception:{}", source.id.trim());
	let runtime_health = state.read().await.runtime_health.clone();
	let runtime = state.read().await.stream_perception.clone();
	let mut last_error: Option<String> = None;
	loop {
		if shutdown.is_triggered() {
			break;
		}
		if source_active_in_current_mode(&source, &state).await {
			match runtime.capture_periodic(&source.id, &state).await {
				Ok(_) => {
					runtime_health.clear(&issue_key).await;
					if last_error.take().is_some() {
						log::info!("《配信知覚》 OBS観測を再開しました: id={}", source.id);
					}
				}
				Err(error) => {
					let message = error.to_string();
					if last_error.as_deref() != Some(message.as_str()) {
						log::warn!("《配信知覚》 OBS観測に失敗: id={} error={message}", source.id);
						runtime_health
							.report(
								crate::runtime_health::RuntimeIssue::error(
									issue_key.clone(),
									"stream_perception",
									"obs_source_capture_failed",
									format!("OBSのゲーム画面を観測できない: {message}"),
									"AI共演者が現在のゲーム画面を参照できない。会話とTwitch宣言値は継続する。",
									"live",
									None,
								)
								.with_resolution("OBS WebSocketと、設定したscene/source名を確認する。"),
							)
							.await;
						last_error = Some(message);
					}
				}
			}
		}
		tokio::select! {
			_ = shutdown.wait() => break,
			_ = tokio::time::sleep(Duration::from_secs(source.interval_secs)) => {}
		}
	}
}

async fn source_active_in_current_mode(source: &ObsPerceptionSourceConf, state: &SharedState) -> bool {
	let mode = state.read().await.runtime_mode_id.read().ok().and_then(|value| value.clone());
	source_active_in_mode(source, mode.as_deref())
}

fn source_active_in_mode(source: &ObsPerceptionSourceConf, mode: Option<&str>) -> bool {
	if source.active_modes.is_empty() {
		return true;
	}
	mode.is_some_and(|current| source.active_modes.iter().any(|candidate| candidate == current))
}

impl ObsPerceptionSourceConf {
	fn to_observation_spec(&self) -> ObsSourceObservationSpec {
		let password = self
			.password_env
			.as_deref()
			.and_then(|name| std::env::var(name).ok())
			.filter(|value| !value.is_empty())
			.or_else(|| (!self.password.is_empty()).then(|| self.password.clone()))
			.or_else(|| self.use_local_obs_config_password.then(load_local_obs_websocket_password).flatten())
			.unwrap_or_default();
		ObsSourceObservationSpec {
			url: self.url.clone(),
			password,
			scene_name: self.scene_name.clone(),
			source_name: self.source_name.clone(),
			image_format: self.image_format.clone(),
			width: self.width,
			height: self.height,
			timeout_ms: self.timeout_ms,
			channel: self.channel.clone(),
		}
	}
}

fn load_local_obs_websocket_password() -> Option<String> {
	let path = dirs::config_dir()?.join("obs-studio/plugin_config/obs-websocket/config.json");
	let bytes = std::fs::read(path).ok()?;
	parse_local_obs_websocket_password(&bytes)
}

fn parse_local_obs_websocket_password(bytes: &[u8]) -> Option<String> {
	let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
	if value.get("auth_required").and_then(serde_json::Value::as_bool) == Some(false) {
		return Some(String::new());
	}
	value
		.get("server_password")
		.and_then(serde_json::Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string)
}

fn default_obs_url() -> String {
	"ws://127.0.0.1:4455".into()
}
fn default_image_format() -> String {
	"jpeg".into()
}
fn default_width() -> i64 {
	960
}
fn default_height() -> i64 {
	540
}
fn default_timeout_ms() -> i64 {
	3000
}
fn default_interval_secs() -> u64 {
	5
}
fn default_channel() -> String {
	"obs:game_frame".into()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn archived_source_does_not_enter_capture_runtime() {
		let conf:StreamPerceptionConf=toml::from_str("[[archived_obs_sources]]\nid='old'\nenabled=true\nrefresh_before_reply=true\nscene_name='Scene'\nsource_name='Capture'\n").unwrap();
		conf.validate().unwrap();
		assert!(StreamPerceptionRuntime::new(&conf).sources.is_empty());
		let serialized=toml::to_string(&conf).unwrap();
		let reopened:StreamPerceptionConf=toml::from_str(&serialized).unwrap();
		assert_eq!(reopened.archived_obs_sources[0].id,"old");
		assert!(reopened.obs_sources.is_empty());
	}

	fn refresh_fixture() -> (StreamPerceptionConf, crate::ai::observe::ObserveSet) {
		let conf: StreamPerceptionConf = toml::from_str(
			r#"
[[obs_sources]]
id = "game"
enabled = true
refresh_before_reply = true
scene_name = "Game"
source_name = "Game Capture"
active_modes = ["streaming"]
"#,
		)
		.unwrap();
		let observe = crate::ai::observe::ObserveSet {
			triggers: vec!["user".into()],
			channel_utterance: "ai".into(),
			include_all: false,
			include_additional: vec!["obs:game_frame".into()],
			exclude: vec![],
		};
		(conf, observe)
	}

	#[tokio::test]
	async fn reply_refresh_respects_opt_in_channels_and_runtime_mode() {
		let (mut conf, mut observe) = refresh_fixture();
		let mut disabled = conf.obs_sources[0].clone();
		disabled.id = "disabled".into();
		disabled.enabled = false;
		let mut excluded = conf.obs_sources[0].clone();
		excluded.id = "excluded".into();
		excluded.channel = "other-screen".into();
		let mut periodic_only = conf.obs_sources[0].clone();
		periodic_only.id = "periodic".into();
		periodic_only.refresh_before_reply = false;
		conf.obs_sources.extend([disabled, excluded, periodic_only]);
		let runtime = StreamPerceptionRuntime::new(&conf);
		for mode in [None, Some("maintenance"), Some("streaming")] {
			let mut captured = Vec::new();
			let result = runtime
				.refresh_with(mode, &observe, Duration::from_secs(1), |source| {
					captured.push(source.conf.id.clone());
					async { Ok(true) }
				})
				.await;
			assert_eq!(captured, if mode == Some("streaming") { vec!["game"] } else { vec![] });
			assert!(!result.failed);
		}
		observe.exclude.push("obs:game_frame".into());
		let result = runtime
			.refresh_with(Some("streaming"), &observe, Duration::from_secs(1), |_| async {
				panic!("excluded channel must never be captured")
			})
			.await;
		assert!(!result.attempted);
	}

	#[tokio::test]
	async fn restored_disabled_and_archived_images_are_excluded_without_touching_live_peer() {
		let (mut conf,observe)=refresh_fixture();
		let mut disabled=conf.obs_sources[0].clone();disabled.id="disabled".into();disabled.enabled=false;
		let mut archived=conf.obs_sources[0].clone();archived.id="archived".into();
		conf.obs_sources.push(disabled);conf.archived_obs_sources.push(archived);
		// Reopen serialized settings as a new runtime, as startup does.
		let reopened:StreamPerceptionConf=toml::from_str(&toml::to_string(&conf).unwrap()).unwrap();
		let runtime=StreamPerceptionRuntime::new(&reopened);
		let mut captured=Vec::new();
		let mut result=runtime.refresh_with(Some("streaming"),&observe,Duration::from_secs(1),|source| {
			captured.push((source.conf.id.clone(),source.conf.scene_name.clone(),source.conf.source_name.clone()));
			async {Ok(true)}
		}).await;
		assert_eq!(captured,vec![(conf.obs_sources[0].id.clone(),conf.obs_sources[0].scene_name.clone(),conf.obs_sources[0].source_name.clone())]);
		assert!(result.attempted && !result.failed,"intentional suppression is not a capture failure");
		runtime.restrict_reply_to_mode(&mut result,Some("streaming"));
		let datum=|channel:&str,key:&str|crate::ChannelDatum::new(channel.into(),key.into()).with_meta(crate::ChannelDatum::META_STATE_KEY,key);
		let mut observations=vec![datum("obs:game_frame","obs_source:disabled"),datum("obs:game_frame","obs_source:archived"),datum("obs:game_frame","obs_source:game"),datum("obs:game_frame","twitch:declared"),datum("other","obs_source:archived")];
		result.exclude_unrefreshed(&mut observations);
		assert_eq!(observations.iter().map(|d|d.content.as_str()).collect::<Vec<_>>(),vec!["obs_source:game","twitch:declared","obs_source:archived"]);
		assert_eq!(observations[2].channel,"other");
	}

	#[tokio::test]
	async fn mode_changed_after_successful_refresh_excludes_frame_at_assembly() {
		let (conf, observe) = refresh_fixture();
		let runtime = StreamPerceptionRuntime::new(&conf);
		let mut result = runtime.refresh_with(Some("streaming"), &observe, Duration::from_secs(1), |_| async { Ok(true) }).await;
		assert!(result.attempted && !result.failed);
		assert!(result.unrefreshed.is_empty());
		runtime.restrict_reply_to_mode(&mut result, Some("maintenance"));
		runtime.restrict_reply_to_mode(&mut result, Some("maintenance"));
		assert_eq!(result.unrefreshed.len(), 1);
		assert!(!result.failed);
		let mut observations = vec![
			crate::ChannelDatum::new("obs:game_frame".into(), "old frame".into()).with_meta(crate::ChannelDatum::META_STATE_KEY, "obs_source:game"),
			crate::ChannelDatum::new("user".into(), "keep conversation".into()),
		];
		result.exclude_unrefreshed(&mut observations);
		assert_eq!(observations.len(), 1);
		assert_eq!(observations[0].channel, "user");
	}

	#[tokio::test]
	async fn inactive_mode_excludes_retained_screen_without_capture_failure() {
		let (conf, observe) = refresh_fixture();
		let runtime = StreamPerceptionRuntime::new(&conf);
		for mode in [None, Some("maintenance")] {
			let result = runtime.refresh_with(mode, &observe, Duration::from_secs(1), |_| async {
				panic!("inactive source must not be captured")
			}).await;
			assert!(!result.attempted && !result.failed);
			let datum = |channel: &str, key: &str| crate::ChannelDatum::new(channel.into(), key.into())
				.with_meta(crate::ChannelDatum::META_STATE_KEY, key);
			let mut observations = vec![
				datum("obs:game_frame", "obs_source:game"),
				datum("other", "obs_source:game"),
				datum("obs:game_frame", "twitch:declared"),
			];
			result.exclude_unrefreshed(&mut observations);
			assert_eq!(observations.len(), 2, "inactive mode must not reuse the last streaming frame");
			assert_eq!(observations[0].channel, "other");
			assert_eq!(observations[1].content, "twitch:declared");
		}
	}

	#[tokio::test]
	async fn reply_refresh_budget_includes_periodic_lock_wait() {
		let (conf, observe) = refresh_fixture();
		let runtime = StreamPerceptionRuntime::new(&conf);
		let held = runtime.sources[0].blank.lock().await;
		let reached = Arc::new(std::sync::atomic::AtomicBool::new(false));
		let reached_capture = reached.clone();
		let result = runtime
			.refresh_with(Some("streaming"), &observe, Duration::from_millis(20), move |source| {
				let reached = reached_capture.clone();
				async move {
					let _lock = source.blank.lock().await;
					reached.store(true, std::sync::atomic::Ordering::SeqCst);
					Ok(true)
				}
			})
			.await;
		assert!(result.attempted && result.failed);
		drop(held);
		tokio::task::yield_now().await;
		assert!(
			!reached.load(std::sync::atomic::Ordering::SeqCst),
			"timed-out capture must not continue later"
		);
		let failure = runtime
			.refresh_with(Some("streaming"), &observe, Duration::from_secs(1), |_| async {
				Err(CaptureObservationError::Upstream("fixture failure".into()))
			})
			.await;
		assert!(failure.attempted && failure.failed);
	}

	#[tokio::test]
	async fn reply_timeout_excludes_failed_and_unattempted_sources_only() {
		let (mut conf, observe) = refresh_fixture();
		for id in ["stalled", "unattempted"] {
			let mut source = conf.obs_sources[0].clone();
			source.id = id.into();
			conf.obs_sources.push(source);
		}
		let runtime = StreamPerceptionRuntime::new(&conf);
		let result = runtime
			.refresh_with(Some("streaming"), &observe, Duration::from_millis(20), |source| async move {
				match source.conf.id.as_str() {
					"game" => Ok(true),
					"stalled" => std::future::pending().await,
					_ => panic!("budget must expire before the last source"),
				}
			})
			.await;
		assert!(result.failed);
		let datum = |key: &str, text: &str| {
			crate::ChannelDatum::new("obs:game_frame".into(), text.into())
				.with_flag(crate::ChannelDatum::FLAG_IS_FINAL)
				.with_meta(crate::ChannelDatum::META_STATE_KEY, key)
		};
		let mut observations = vec![
			datum("obs_source:game", "fresh"),
			datum("obs_source:stalled", "stale failed screen"),
			datum("obs_source:unattempted", "stale unattempted screen"),
			datum("twitch:declared", "declared game"),
		];
		result.exclude_unrefreshed(&mut observations);
		assert_eq!(
			observations.iter().map(|d| d.content.as_str()).collect::<Vec<_>>(),
			["fresh", "declared game"]
		);
	}

	#[tokio::test]
	async fn source_deactivated_during_refresh_cannot_reuse_old_frame() {
		let (conf, observe) = refresh_fixture();
		let runtime = StreamPerceptionRuntime::new(&conf);
		let result = runtime
			.refresh_with(Some("streaming"), &observe, Duration::from_secs(1), |_| async { Ok(false) })
			.await;
		assert!(result.attempted && result.failed);
		assert_eq!(result.unrefreshed, [("obs:game_frame".into(), "obs_source:game".into())]);
	}

	#[test]
	fn config_defaults_are_bounded_and_opt_in() {
		let conf: StreamPerceptionConf = toml::from_str(
			r#"
[[obs_sources]]
id = "game"
scene_name = "Game"
source_name = "Game Capture"
"#,
		)
		.unwrap();
		conf.validate().unwrap();
		let source = &conf.obs_sources[0];
		assert!(!source.enabled);
		assert!(!source.refresh_before_reply);
		assert_eq!(source.interval_secs, 5);
		assert_eq!(source.channel, "obs:game_frame");
	}

	#[test]
	fn invalid_or_ambiguous_source_is_rejected() {
		let conf: StreamPerceptionConf = toml::from_str(
			r#"
[[obs_sources]]
id = "game"
scene_name = ""
source_name = "Game Capture"
"#,
		)
		.unwrap();
		assert!(conf.validate().is_err());
	}

	#[test]
	fn local_obs_password_parser_does_not_log_or_require_profile_secret() {
		assert_eq!(
			parse_local_obs_websocket_password(br#"{"auth_required":true,"server_password":"secret"}"#).as_deref(),
			Some("secret")
		);
		assert_eq!(
			parse_local_obs_websocket_password(br#"{"auth_required":false,"server_password":"ignored"}"#).as_deref(),
			Some("")
		);
	}
}

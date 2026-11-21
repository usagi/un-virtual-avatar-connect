//! ランタイム共有状態。`flowgraph` / `ai` / `bridges` 等と接続する。
//!
//! ## レイヤ（Step 4 メモ）
//!
//! Control イベント型は [`crate::control_events`] にあり、本モジュールは `web_interface` に依存しない。

mod channel_attach;
mod channel_datum;
mod managed_apps_mode_apply;
mod runtime_mode_apply;
mod runtime_mode_transition;
mod speech_floor;
mod voice_input;

pub use channel_attach::{Attachment, DataSource};
pub use channel_datum::{ChannelData, ChannelDatum, SharedChannelData};
pub use runtime_mode_apply::{apply_runtime_mode_change, ApplyRuntimeModeError};
pub use runtime_mode_transition::{apply_runtime_mode_transition_full, try_begin_runtime_mode_transition, RuntimeModeTransitionStatus};
pub use speech_floor::SpeechFloorManager;
pub use voice_input::VoiceInputControl;

use crate::ai::Observation;
use crate::conf::Twitch;
use crate::control_events::{ChannelDatumPhase, ControlEvent};
use crate::flowgraph::{shared_flowgraph_new, SharedFlowgraph};
use crate::runtime::{init_runtime_paths, RuntimePaths};
use crate::shutdown::ShutdownBroker;
use crate::twitch_oauth_sessions::OAuthSessions;
use crate::{Arc, Conf, RwLock, SharedAudioSink};
use anyhow::Result;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::broadcast;

fn control_event_from_channel_datum(phase: ChannelDatumPhase, cd: &ChannelDatum) -> ControlEvent {
	ControlEvent::ChannelDatum {
		phase,
		id: cd.get_id(),
		channel: cd.channel.clone(),
		content: cd.content.clone(),
		flags: cd.flags.iter().cloned().collect(),
		datetime: cd.get_datetime().to_string(),
		diagnostic_meta: channel_datum_diagnostic_meta(cd),
	}
}

const CHANNEL_DATUM_DIAGNOSTIC_META_KEYS: [&str; 6] = [
	"event_type",
	"reward_id",
	"reward_title",
	"route",
	"quick_effect_label",
	"quick_effect_duration_ms",
];

fn channel_datum_diagnostic_meta(cd: &ChannelDatum) -> BTreeMap<String, serde_json::Value> {
	CHANNEL_DATUM_DIAGNOSTIC_META_KEYS
		.into_iter()
		.filter_map(|key| cd.meta.get(key).cloned().map(|value| (key.to_string(), value)))
		.collect()
}

#[cfg(test)]
mod control_event_tests {
	use super::*;

	#[test]
	fn failure_diagnostic_is_retained_without_error_body() {
		let item = TimestampedControlEvent {
			at: "2026-11-05T00:00:00Z".into(),
			event: ControlEvent::AiReactionFailure {
				persona_id: "fixture".into(), observation_datum_id: 42,
				stage: "request_or_tool_loop".into(), category: "api".into(),
				http_status: Some(429), observed_at: "2026-11-05T00:00:00Z".into(),
			},
		};
		let value = session_diagnostic_value(&item).unwrap();
		assert_eq!(item.event.kind(), "ai_reaction_failure");
		assert_eq!(value["event"]["http_status"], 429);
		assert_eq!(value["event"]["observation_datum_id"], 42);
		assert!(value["event"].get("error").is_none());
		assert!(value["event"].get("body").is_none());
	}

	#[test]
	fn channel_datum_event_exposes_only_allowlisted_diagnostic_meta() {
		let datum = ChannelDatum::new("effect".into(), "private content".into())
			.with_meta("reward_id", "reward-123")
			.with_meta("route", "kaltsit_ai_request")
			.with_meta("access_token", "must-not-leak")
			.with_meta("viewer_input", "must-not-leak-either");
		let value = serde_json::to_value(control_event_from_channel_datum(ChannelDatumPhase::Pushed, &datum)).unwrap();
		assert_eq!(value["diagnostic_meta"]["reward_id"], "reward-123");
		assert_eq!(value["diagnostic_meta"]["route"], "kaltsit_ai_request");
		assert!(value["diagnostic_meta"].get("access_token").is_none());
		assert!(value["diagnostic_meta"].get("viewer_input").is_none());
	}

	#[test]
	fn persistent_diagnostic_journal_keeps_ai_input_correlation() {
		let item = TimestampedControlEvent {
			at: "2026-09-13T04:00:01Z".into(),
			event: ControlEvent::AiRequestContext {
                visual_history_json: None,
				persona_id: "kaltsit".into(),
				observation_datum_id: 42,
				model: "test-model".into(),
                input_datums: vec![crate::control_events::AiContextDatum { datum_id: 41, input_index: 3, role: "assistant".into(), source_kind: Some("ai".into()) }],
				request_class: None,
				context_collected_at: "2026-09-13T04:00:00Z".into(),
				prepared_at: "2026-09-13T04:00:01Z".into(),
				observations: vec![crate::control_events::AiContextObservation {
					datum_id: 43,
					channel: "obs:game_frame".into(),
					source_kind: "obs.scene_source".into(),
					source_subtype: Some("no_visual_content".into()),
					observed_at: Some("2026-09-13T03:59:59Z".into()),
					attachment_count: 0,
					input_image_count: 0,
				}],
			},
		};
		assert_eq!(item.event.kind(), "ai_request_context");
		let value = session_diagnostic_value(&item).unwrap();
		assert_eq!(value["event"]["observation_datum_id"], 42);
        assert_eq!(value["event"]["input_datums"][0]["role"], "assistant");
        assert_eq!(value["event"]["input_datums"][0]["input_index"], 3);
		assert_eq!(value["event"]["observations"][0]["datum_id"], 43);
		assert_eq!(value["event"]["observations"][0]["input_image_count"], 0);
		assert_eq!(value["event"]["observations"][0]["source_subtype"], "no_visual_content");
	}

	#[test]
	fn persistent_diagnostic_journal_keeps_avatar_parameters_without_dialogue() {
		let item = TimestampedControlEvent {
			at: "2026-09-26T00:00:00Z".into(),
			event: ControlEvent::AiTurnDecision {
				persona_id: "test".into(),
				observation_channel: "user".into(),
				observation_datum_id: 1,
				action: "react".into(),
				speak: Some("private spoken text".into()),
				episode_id: None,
				episode_phase: None,
				episode_memory: Some("private episode text".into()),
				episode_turn_count: None,
				episode_phase_remaining_secs: None,
				avatar_intent_requested_count: 1,
				avatar_intent_emitted_count: 0,
				avatar_intent_parameters: vec![serde_json::json!({"kind":"head_nod","count":1,"duration_ms":700})],
				avatar_intent_target_link_id: Some("test".into()),
				avatar_intent_error: Some("deadline".into()),
				avatar_pose_requested: false,
				avatar_pose_emitted: false,
				avatar_pose_preset: None,
				avatar_pose_target_link_id: None,
				avatar_pose_error: None,
				output_ready_at: Some("2026-09-25T23:59:55Z".into()),
				speech_utterance_id: Some("test:turn-1".into()),
				decided_at: "2026-09-26T00:00:00Z".into(),
			},
		};
		let value = session_diagnostic_value(&item).unwrap();
		assert!(value["event"].get("speak").is_none());
		assert_eq!(value["event"]["output_ready_at"], "2026-09-25T23:59:55Z");
		assert_eq!(value["event"]["speech_utterance_id"], "test:turn-1");
		assert!(value["event"].get("episode_memory").is_none());
		assert_eq!(value["event"]["avatar_intent_parameters"][0]["kind"], "head_nod");
		assert_eq!(value["event"]["avatar_intent_parameters"][0]["duration_ms"], 700);
		assert_eq!(
			value["event"]["avatar_intent_emitted_count"], 0,
			"requested parameters do not imply successful delivery"
		);
	}

	#[test]
	fn persistent_diagnostic_journal_redacts_observation_text() {
		let item = TimestampedControlEvent {
			at: "2026-08-09T00:00:00Z".into(),
			event: ControlEvent::VoiceRecognition {
				phase: crate::control_events::VoiceRecognitionPhase::Final,
				node_id: "voice".into(),
				utterance_id: 7,
				text: "do not persist me".into(),
				elapsed_ms: 12,
				observed_at: "2026-08-09T00:00:00Z".into(),
			},
		};
		let value = session_diagnostic_value(&item).unwrap();
		assert_eq!(value["event"]["kind"], "voice_recognition");
		assert!(value["event"].get("text").is_none());
		assert_eq!(value["event"]["utterance_id"], 7);
	}

	#[test]
	fn persistent_diagnostic_journal_keeps_chat_send_outcome_without_payload() {
		let item = TimestampedControlEvent { at: "2026-10-31T00:00:00Z".into(),
			event: ControlEvent::TwitchChatSendResult { node_id: "notices::send".into(), outcome: "sent".into() } };
		let value = session_diagnostic_value(&item).unwrap();
		assert_eq!(value["event"], serde_json::json!({"kind":"twitch_chat_send_result","node_id":"notices::send","outcome":"sent"}));
		assert_eq!(item.event.kind(), "twitch_chat_send_result");
	}

	#[test]
	fn persistent_diagnostic_journal_keeps_camera_effect_failure() {
		let item = TimestampedControlEvent {
			at: "2026-10-15T00:00:00Z".into(),
			event: ControlEvent::AvatarCameraEffect {
				node_id:"stream/channel-points::camera_zoom".into(), target_link_id:"dr_usagi".into(),
				phase:"target_rejected".into(), details:serde_json::json!({"error":"target unavailable","discovery_errors":["timeout"]}),
			},
		};
		let value = session_diagnostic_value(&item).expect("camera failures must survive process exit");
		assert_eq!(value["event"]["kind"], "avatar_camera_effect");
		assert_eq!(value["event"]["details"]["discovery_errors"][0], "timeout");
		assert_eq!(item.event.kind(), "avatar_camera_effect");
	}
}

pub type SharedState = Arc<RwLock<State>>;

const DEFAULT_STATE_DATA_CAPACITY: usize = 256;
/// AI サービスに通知する観測イベント（`ChannelDatum` の発行）のブロードキャストキャパシティ。
/// 観測を取りこぼしてもログに `Lagged` が出るだけで致命的ではないが、短時間に大量に発行される配信負荷を考慮し広めに取る。
const AI_OBSERVATION_CHANNEL_CAPACITY: usize = 1024;
/// Control API WebSocket (`/api/v1/control/events`) 用のイベント配信キャパシティ。
/// 接続されている GUI が遅い場合に `Lagged` 通知で補足するため、こちらも余裕をもって取る。
const CONTROL_EVENT_CHANNEL_CAPACITY: usize = 2048;
/// 10分程度の実配信でpartial認識やprocessor発火が続いても、開始時の縦断証拠を保持する。
pub(crate) const CONTROL_EVENT_HISTORY_CAPACITY: usize = 4096;
const UN_APP_API_COMMAND_HISTORY_CAPACITY: usize = 128;
const QUICK_EFFECT_QUEUE_CAPACITY: usize = crate::quick_effects::QUICK_EFFECT_QUEUE_CAPACITY;
/// δ-9 Part E: Flowgraph の `channel.subscribe` ingress が購読する `ChannelDatum` ブロードキャストの容量。
/// 複数 bridge が独立して受信するため、遅い購読者がいても後続を取りこぼさないよう広めに取る。
const CHANNEL_DATUM_BROADCAST_CAPACITY: usize = 2048;

#[derive(Debug, Clone, serde::Serialize)]
pub struct TimestampedControlEvent {
	pub at: String,
	pub event: ControlEvent,
}

fn spawn_control_event_history_recorder(
	mut rx: broadcast::Receiver<ControlEvent>,
	history: Arc<RwLock<VecDeque<TimestampedControlEvent>>>,
	diagnostic_path: PathBuf,
) {
	tokio::spawn(async move {
		let mut diagnostic_writer = match tokio::fs::OpenOptions::new()
			.create(true)
			.truncate(true)
			.write(true)
			.open(&diagnostic_path)
			.await
		{
			Ok(file) => Some(BufWriter::new(file)),
			Err(error) => {
				log::warn!("Control Event診断journalを作成できません: {}: {error}", diagnostic_path.display());
				None
			}
		};
		loop {
			let item = match rx.recv().await {
				Ok(event) => TimestampedControlEvent {
					at: crate::datetime::DateTime::now().to_rfc3339(),
					event,
				},
				Err(broadcast::error::RecvError::Lagged(dropped)) => TimestampedControlEvent {
					at: crate::datetime::DateTime::now().to_rfc3339(),
					event: ControlEvent::Lagged { dropped },
				},
				Err(broadcast::error::RecvError::Closed) => break,
			};
			{
				let mut h = history.write().await;
				if h.len() >= CONTROL_EVENT_HISTORY_CAPACITY {
					h.pop_front();
				}
				h.push_back(item.clone());
			}
			if let (Some(writer), Some(value)) = (diagnostic_writer.as_mut(), session_diagnostic_value(&item)) {
				let mut line = match serde_json::to_vec(&value) {
					Ok(line) => line,
					Err(error) => {
						log::warn!("Control Event診断journalのserializeに失敗しました: {error}");
						continue;
					}
				};
				line.push(b'\n');
				let write_result = match writer.write_all(&line).await {
					Ok(()) => writer.flush().await,
					Err(error) => Err(error),
				};
				if let Err(error) = write_result {
					log::warn!(
						"Control Event診断journalの書き込みを停止します: {}: {error}",
						diagnostic_path.display()
					);
					diagnostic_writer = None;
				}
			}
		}
	});
}

fn session_diagnostic_value(item: &TimestampedControlEvent) -> Option<serde_json::Value> {
	let keep = matches!(
		item.event,
		ControlEvent::ChannelDatum { .. }
			| ControlEvent::VoiceRecognition { .. }
			| ControlEvent::TwitchChatReceived { .. }
			| ControlEvent::TwitchChatSendResult { .. }
			| ControlEvent::TwitchEventSubReceived { .. }
			| ControlEvent::AvatarCameraEffect { .. }
			| ControlEvent::TtsLifecycle { .. }
			| ControlEvent::AiTurnDecision { .. }
			| ControlEvent::AiRequestContext { .. }
			| ControlEvent::AiReactionFailure { .. }
			| ControlEvent::AiActivationDecision { .. }
			| ControlEvent::AiInitiativeDecision { .. }
			| ControlEvent::Lagged { .. }
	);
	if !keep {
		return None;
	}
	let mut value = serde_json::to_value(item).ok()?;
	let event = value.get_mut("event")?.as_object_mut()?;
	for key in ["content", "text", "speak", "episode_memory"] {
		event.remove(key);
	}
	Some(value)
}

/// `respect_speech_floor` が設定されているとき、対応する speech floor が空くまで非同期待機する（スキップしない）。
pub async fn wait_respect_speech_floor_key(state: &SharedState, key: Option<&str>) {
	let key = key.map(str::trim).filter(|s| !s.is_empty());
	let Some(k) = key else {
		return;
	};
	state.read().await.speech_floor.wait_until_free(k).await;
}

/// 各 AI persona のランタイム状態。`AiService::run` / `run_heartbeat` は起動時にこの `paused` を共有し、
/// `true` の間は新規 Datum 処理・heartbeat 評価をスキップする（既に走っている応答は中断しない soft suspend）。
#[derive(Debug, Clone)]
pub struct AiRuntime {
	pub persona_id: Option<String>,
	pub display_name: Option<String>,
	pub paused: Arc<AtomicBool>,
	/// Phase VI-α-4: Control API からの hot-reload ハンドル。`spawn_all` が AiService 生成直後に登録する。
	/// cycle 回避のため、本ハンドルは AiService の一部フィールド (`persona` / `request_template` / `decision`)
	/// しか指さない（`state: SharedState` を参照しない）。
	pub reload_handle: Option<crate::ai::AiReloadHandle>,
	pub usage: Arc<RwLock<AiUsageTelemetry>>,
}

#[derive(Debug, Clone, Default)]
pub struct AiUsageTelemetry {
	/// AI処理が正常完了した回数。ターン管理が発話せず`observe`を選んだ処理も含む。
	pub completed_reactions: u64,
	pub failed_reactions: u64,
	pub api_requests: u64,
	pub tool_calls: u64,
	pub input_tokens: u64,
	pub cached_tokens: u64,
	pub cache_write_tokens: u64,
	pub output_tokens: u64,
	pub reasoning_tokens: u64,
	pub last_model: Option<String>,
	pub last_latency_ms: Option<u64>,
	pub last_updated_at: Option<String>,
}

impl AiRuntime {
	pub fn new(persona_id: Option<String>) -> Self {
		Self {
			persona_id,
			display_name: None,
			paused: Arc::new(AtomicBool::new(false)),
			reload_handle: None,
			usage: Arc::new(RwLock::new(AiUsageTelemetry::default())),
		}
	}

	pub fn is_paused(&self) -> bool {
		self.paused.load(Ordering::SeqCst)
	}

	pub fn set_paused(&self, v: bool) {
		self.paused.store(v, Ordering::SeqCst);
	}
}

#[derive(Debug, Clone)]
pub struct State {
	pub state_data_capacity: usize,
	pub state_data_path: Option<PathBuf>,
	pub state_data_auto_save: bool,
	pub state_data_pretty: bool,
	pub channel_data: SharedChannelData,
	/// 起動済み AI persona のランタイム情報。`ai::spawn_all` が登録する。
	pub ai_runtimes: Arc<RwLock<Vec<AiRuntime>>>,
	pub audio_sink: SharedAudioSink,
	/// AvatarLink単位でambient、articulation、semantic motionを単一UNMFへ合成する。
	pub(crate) avatar_motion: crate::avatar_motion::AvatarMotionController,
	/// AvatarLinkから単一RendererのUNAA/Z identityを解決する起動時設定。
	pub(crate) avatar_links: Arc<std::collections::BTreeMap<String, crate::conf::AvatarLinkSpec>>,
	/// 読み取り専用の定期確認と配信前診断で得たRendererの受信・適用証拠。
	pub(crate) avatar_renderer_probe: Arc<RwLock<Option<crate::avatar_renderer_probe::AvatarRendererProbeResult>>>,
	pub(crate) avatar_renderer_probe_refresh: Arc<tokio::sync::Mutex<Option<std::time::Instant>>>,
	/// AI・チャット・システム音声を同じ再生キューで直列化する。
	pub(crate) speech_scheduler: crate::speech_scheduler::SpeechScheduler,
	pub(crate) stream_perception: crate::stream_perception::StreamPerceptionRuntime,
	pub(crate) diagnostic_images: crate::diagnostic_images::ImageArchive,
	/// 《Voice》発話中などの speech floor（キーごとに `Notify` で待機解除）
	pub speech_floor: Arc<SpeechFloorManager>,
	/// 音声認識workerを再起動せず、Voice IngressのFlowgraph投入を止めるソフトミュート状態。
	pub voice_input: Arc<VoiceInputControl>,
	/// 《LibreTranslate》: 自動起動した子プロセスの停止用
	pub libretranslate: crate::libretranslate::SharedRuntime,
	/// 添付ファイル用のランタイム一時ディレクトリ情報（セッション単位）。
	pub runtime_paths: Arc<RuntimePaths>,
	/// AI サービスへ観測イベントを流す `broadcast` 送信側（Phase I で導入）。
	/// 受信側は `[[ai.personas]]` ごとに subscribe される常駐タスクのみ。
	pub ai_observation_tx: broadcast::Sender<Observation>,
	/// Control API WebSocket の配信チャネル（Phase VI-α-3）。
	/// 各 WS 接続が `subscribe()` で受信側を作り、切断で drop する使い方。受信者 0 でも `send()` は失敗しない設計にする。
	pub control_event_tx: broadcast::Sender<ControlEvent>,
	pub control_event_history: Arc<RwLock<VecDeque<TimestampedControlEvent>>>,
	pub un_app_api_command_history: Arc<RwLock<VecDeque<crate::un_app_api::UnAppApiCommandHistoryItem>>>,
	pub quick_effect_queue: Arc<RwLock<VecDeque<crate::quick_effects::QuickEffectItem>>>,
	/// δ-9 Part E: `push_channel_datum` / `push_channel_datum_quiet` / `finalize_channel_datum_and_dispatch`
	/// で `ChannelDatum` を配信するブロードキャスト送信側。
	///
	/// 外部出力や目的別の内部service bridgeが購読する。公開Flowgraph間配送には使わない。
	/// 受信者0でも`send()`は失敗しない。
	pub channel_datum_tx: broadcast::Sender<ChannelDatum>,
	/// Phase V: Twitch outbound / モデレーション系プロセッサーが参照する `[twitch]` 全体のスナップショット。
	/// `conf.twitch` が `Some` のときだけセットされる。Arc 共有で低コスト。
	pub twitch: Option<Arc<Twitch>>,
	/// Twitch宣言値の現在状態。画像観測による実内容とは別の情報源として保持する。
	pub twitch_stream_state: Arc<crate::stream_state::TwitchStreamStateRegistry>,
	/// Phase VI-α-5: Control API からの Twitch Device Code Flow セッション管理。
	/// broadcaster / moderator 各 1 本まで並行保持する（同一アカウントへの重複 start は既存セッションを冪等に返す）。
	pub twitch_oauth: Arc<OAuthSessions>,
	/// RM-3: 現在の Runtime Mode ID（Control API で更新可）。`None` のときは `conf.default_runtime_mode` を意味する。
	pub runtime_mode_id: std::sync::Arc<std::sync::RwLock<Option<String>>>,
	/// Distinguishes mode round trips even when no AI request runs during the gap.
	pub runtime_mode_generation: Arc<std::sync::atomic::AtomicU64>,
	/// Phase VI-γ-1: この State を組み立てた conf の読み込み元パス（絶対パス寄りに正規化済み）。
	///
	/// `/api/v1/control/restart` が「現在の conf」を識別する、`/api/v1/control/profiles` が
	/// 同ディレクトリを走査する起点になる、といった運用に使う。`Conf.source_path` を clone して置く。
	pub conf_source_path: Option<PathBuf>,
	/// Phase VI-γ-2a: `[browser_source] document_root` のスナップショット。
	///
	/// `/api/v1/control/bos` が走査するルート。GUI Live タブの iframe 埋め込み用 URL の根拠。
	/// `lib.rs` の actix-web 側とは別経由で参照したいので、ここに持たせておく。
	pub browser_source_document_root: Option<PathBuf>,
	/// Phase VI-γ-2b: Managed App（`run_with` の GUI 管理版）レジストリ。
	///
	/// `spec`（conf から組み立てる静的定義）と `statuses`（監視タスクが書き込む現在状態）を 1 つの
	/// `RwLock` に束ねている。`/api/v1/control/managed_apps` API と監視タスクが共有する。
	/// `Arc<RwLock<_>>` にしておくのは `State` 自体の `RwLock::read` を取らなくても独立に操作できるようにするため。
	pub managed_apps: std::sync::Arc<tokio::sync::RwLock<crate::managed_app::ManagedAppRegistry>>,
	/// Phase V: Twitch IRC ingress でスキップする送信者 login の集合（小文字・正規化済み）。
	///
	/// AI → `twitch-out` → 自 chat → ingress という **エコー**を防ぐ目的で、以下から充填される:
	///   1. `[twitch.ignore_logins]` の明示指定
	///   2. `[twitch.moderator].login`（設定されていれば）
	///   3. `TwitchOut` 初期化時に Helix `validate` で解決した自 bot の login
	/// ingress 起動後も動的に追加されうるため `Arc<RwLock<..>>`。
	pub twitch_ignore_logins: Arc<RwLock<HashSet<String>>>,

	/// Phase M3: VMC passthrough の Control API 表示用ランタイム状態。
	pub vmc_passthrough_status: std::sync::Arc<crate::motion::VmcPassthroughStatusRegistry>,
	/// 起動時に確定したUNMF/Z data planeの接続経路。key expressionとは分離して保持する。
	pub(crate) unmfz_transport: crate::unmfz_transport::UnmfzTransportRuntime,

	/// Phase δ-6: Flowgraph ランタイム（ロード済み program + 診断）。
	///
	/// `conf.flowgraph_dir` が指すディレクトリを起動時にロードした結果を保持する。
	/// 本フェーズでは **GUI / Control API の受け皿**として働き、実行は V1 processor 側が担う。
	/// δ-8 以降で `run_forever` を接続する。未設定 / ディレクトリ非存在時は `None` が入る。
	pub flowgraph: SharedFlowgraph,

	/// ζ-3: Flowgraph bridges (twitch / twitch_eventsub / voice / channel_subscribe) のライフサイクル束。
	///
	/// 初回起動は `app_core::boot` が [`crate::bridges::spawn_all_from_state`] で populate する。
	/// flowgraph reload 時は `web_interface::control::flowgraph::reload::reload_runtime_with_restart_status` が旧ハンドルを取り出して
	/// [`crate::bridges::BridgeHandles::finish_all`] で停止し、新 runtime 上で再 spawn して差し替える。
	///
	/// - `Arc<tokio::sync::Mutex<...>>` なのは、respawn 中に他のリクエストが触りに来ないよう
	///   独立ロックを取るため（`SharedState::write()` を長時間保持したくない）。
	/// - web_input は actix route 登録制で hot-swap 不可なので、`finish_all` 対象には含まない。
	///   差分は `web_input_snapshot` で検出し、`ControlEvent::RestartRecommended` で GUI に促す。
	pub bridge_handles: std::sync::Arc<tokio::sync::Mutex<crate::bridges::BridgeHandles>>,
	/// 外部 bridge など、起動後にだけ判明する問題を GUI と診断 API へ渡す。
	pub runtime_health: std::sync::Arc<crate::runtime_health::RuntimeHealthRegistry>,
	/// 現在の起動セッションでEventSub実通知をFlowgraphへ渡せたか。
	pub twitch_eventsub_event_received: Arc<AtomicBool>,
	/// EventSub WebSocketから最後にtext frame（keepaliveを含む）を受信したUNIX時刻。
	/// 宣言済みonline値を長時間配信中に安全に利用するため、session livenessとしてAI initiativeが参照する。
	pub twitch_eventsub_last_message_unix_ms: Arc<AtomicU64>,

	/// Flowgraph `tts.speak` + `engine=voicepeak` で `endpoint` が空のときに埋める CLI パス（`[voicepeak]` + OS 既定）。
	pub voicepeak_fallback_exe: String,

	/// Phase ε-1: 統合シャットダウンブローカー。
	///
	/// Ctrl+C / `POST /api/v1/control/shutdown` / 致命的エラー の全てをここに集約する。
	/// 登録タイミングは `app_core::boot` で `ShutdownBroker::new()` を生成し、
	/// Ctrl+C listener と `State::new()` に同じ broker を渡す。
	pub shutdown: Arc<ShutdownBroker>,

	/// RM-5: `apply_runtime_mode_transition_full` が Managed App I/O 等を実行している間 true。
	/// 再入の `try_begin_runtime_mode_transition` は失敗させる。
	pub runtime_mode_transition_busy: Arc<AtomicBool>,
	pub runtime_mode_transition_status: Arc<RwLock<RuntimeModeTransitionStatus>>,
}

impl State {
	pub async fn new(conf: &Conf, audio_sink: SharedAudioSink, shutdown: Arc<ShutdownBroker>) -> Result<SharedState> {
		let initial_voice_input_muted = crate::profile_store::voice_input_muted_for_conf(conf.source_path.as_deref())?;
		let state_data_auto_save = conf.state_data_auto_save.unwrap_or(false) && conf.state_data_path.is_some();
		if conf.state_data_auto_save.unwrap_or(false) && conf.state_data_path.is_none() {
			log::info!("state_data_auto_save は有効ですが state_data_path が未設定のため、永続化なしで起動します。");
		}
		let channel_data = match &conf.state_data_path {
			Some(path) => load_channel_data(path).await?,
			None => Arc::new(RwLock::new(VecDeque::new())),
		};
		log::trace!("ChannelData の初期化が完了しました。");

		let runtime_paths = Arc::new(init_runtime_paths(conf)?);
		let control_event_diagnostic_path = runtime_paths.session_path("control-events.diagnostic.jsonl");

		let (ai_observation_tx, _rx) = broadcast::channel::<Observation>(AI_OBSERVATION_CHANNEL_CAPACITY);
		let (control_event_tx, _rx) = broadcast::channel::<ControlEvent>(CONTROL_EVENT_CHANNEL_CAPACITY);
		let control_event_history = Arc::new(RwLock::new(VecDeque::with_capacity(CONTROL_EVENT_HISTORY_CAPACITY)));
		let un_app_api_command_history = Arc::new(RwLock::new(VecDeque::with_capacity(UN_APP_API_COMMAND_HISTORY_CAPACITY)));
		let quick_effect_queue = Arc::new(RwLock::new(VecDeque::with_capacity(QUICK_EFFECT_QUEUE_CAPACITY)));
		let (channel_datum_tx, _rx) = broadcast::channel::<ChannelDatum>(CHANNEL_DATUM_BROADCAST_CAPACITY);

		let twitch_snapshot = conf.twitch.as_ref().map(|t| Arc::new(t.clone()));

		// ingress で無視する login 集合を、`[twitch.ignore_logins]` と `[twitch.moderator].login` から先に充填しておく。
		// `TwitchOut` 初期化時に Helix validate 経由で解決した自 bot login もここに追加される。
		let mut twitch_ignore_set: HashSet<String> = HashSet::new();
		if let Some(t) = conf.twitch.as_ref() {
			if let Some(list) = t.ignore_logins.as_ref() {
				for s in list {
					let n = s.trim().trim_start_matches('#').to_lowercase();
					if !n.is_empty() {
						twitch_ignore_set.insert(n);
					}
				}
			}
			if let Some(ml) = t.moderator.as_ref().and_then(|m| m.login.as_ref()) {
				let n = ml.trim().trim_start_matches('#').to_lowercase();
				if !n.is_empty() {
					twitch_ignore_set.insert(n);
				}
			}
		}
		let twitch_ignore_logins = Arc::new(RwLock::new(twitch_ignore_set));

		let voicepeak_fallback_exe = crate::conf::resolve_voicepeak_fallback_executable(conf);
		let unmfz_transport = crate::unmfz_transport::UnmfzTransportRuntime::new(conf.unmfz.clone().unwrap_or_default());
		let blink_policy = conf.un_avatar.as_ref().map(|avatar| avatar.links.iter()
			.map(|(id, link)| (id.clone(), link.blink_suppressed_expressions.clone())).collect()).unwrap_or_default();
		let avatar_motion = crate::avatar_motion::AvatarMotionController::spawn_with_blink_policy(unmfz_transport.clone(), blink_policy);
		let avatar_links = Arc::new(conf.un_avatar.as_ref().map(|un_avatar| un_avatar.links.clone()).unwrap_or_default());
		let speech_scheduler = crate::speech_scheduler::SpeechScheduler::spawn(Some(audio_sink.clone()), avatar_motion.clone());
		if let Some(un_avatar) = conf.un_avatar.as_ref() {
			for (link_id, link) in &un_avatar.links {
				let Some(unmfz) = link.unmfz.as_ref().filter(|_| link.has_capability("ai_control")) else {
					continue;
				};
				let config = crate::avatar_motion::AvatarMotionConfig {
					base_key_expr: unmfz.base_key_expr.clone(),
					expression: "JawOpen".into(),
					target_link_id: link_id.clone(),
					gain: 1.0,
				};
				if let Err(error) = avatar_motion.prepare(config) {
					log::warn!("《Avatar Expression Mixer》 AvatarLink {link_id} の起動時登録に失敗しました: {error}");
				}
			}
		}

		let runtime_mode_id = std::sync::Arc::new(std::sync::RwLock::new(conf.default_runtime_mode.clone()));
		let runtime_mode_id_for_flowgraph = runtime_mode_id.clone();
		let vmc_passthrough_status = crate::motion::VmcPassthroughStatusRegistry::from_conf(conf);

		// δ-6: Flowgraph ランタイム共有ハンドル。opt-in なので `flowgraph_dir` が None / 非存在なら `None` 保持。
		// δ-9 Part A: ロード + 実行ワーカー spawn は `State` 生成後に遅延実行する（`Weak<RwLock<State>>` が必要なため）。
		let flowgraph = shared_flowgraph_new();

		let state = Arc::new(RwLock::new(Self {
			state_data_capacity: conf.state_data_capacity.unwrap_or(DEFAULT_STATE_DATA_CAPACITY),
			state_data_path: conf.state_data_path.clone(),
			state_data_auto_save,
			state_data_pretty: conf.state_data_pretty.unwrap_or(false),
			channel_data,
			ai_runtimes: Arc::new(RwLock::new(Vec::new())),
			audio_sink,
			avatar_motion,
			avatar_links,
			avatar_renderer_probe: Arc::new(RwLock::new(None)),
			avatar_renderer_probe_refresh: Arc::new(tokio::sync::Mutex::new(None)),
			speech_scheduler,
			speech_floor: Arc::new(SpeechFloorManager::new()),
			voice_input: Arc::new(VoiceInputControl::new(initial_voice_input_muted)),
			libretranslate: crate::libretranslate::runtime_new(),
			stream_perception: crate::stream_perception::StreamPerceptionRuntime::new(&conf.perception),
			diagnostic_images: crate::diagnostic_images::ImageArchive::new(conf.perception.image_archive.as_ref(), &conf.perception.obs_sources),
			runtime_paths,
			ai_observation_tx,
			control_event_tx: control_event_tx.clone(),
			control_event_history: control_event_history.clone(),
			un_app_api_command_history,
			quick_effect_queue,
			channel_datum_tx,
			twitch: twitch_snapshot,
			twitch_stream_state: Arc::new(crate::stream_state::TwitchStreamStateRegistry::default()),
			twitch_oauth: OAuthSessions::new(),
			runtime_mode_id,
			runtime_mode_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
			conf_source_path: conf.source_path.clone(),
			browser_source_document_root: conf.browser_source.as_ref().and_then(|b| b.document_root.clone()),
			managed_apps: std::sync::Arc::new(tokio::sync::RwLock::new(crate::managed_app::ManagedAppRegistry::from_conf(conf))),
			twitch_ignore_logins,
			vmc_passthrough_status,
			unmfz_transport,
			flowgraph,
			bridge_handles: std::sync::Arc::new(tokio::sync::Mutex::new(crate::bridges::BridgeHandles::empty())),
			runtime_health: std::sync::Arc::new(crate::runtime_health::RuntimeHealthRegistry::default()),
			twitch_eventsub_event_received: Arc::new(AtomicBool::new(false)),
			twitch_eventsub_last_message_unix_ms: Arc::new(AtomicU64::new(0)),
			voicepeak_fallback_exe,
			shutdown,
			runtime_mode_transition_busy: Arc::new(AtomicBool::new(false)),
			runtime_mode_transition_status: Arc::new(RwLock::new(RuntimeModeTransitionStatus::idle())),
		}));
		spawn_control_event_history_recorder(control_event_tx.subscribe(), control_event_history, control_event_diagnostic_path);
		log::trace!("State の生成が完了しました。");

		// Phase δ-9 D.2/D.3 (v1.0): V1 `[[processors]]` は全廃。`conf.processors` が残っていても warning を出すだけ。
		// Part E.2 (v1.0.1-): voice も Flowgraph に完全移行。残存は twitch (IRC/EventSub) のみ。
		if !conf.processors.is_empty() {
			log::warn!(
				"conf.processors ({} 件) が設定されていますが、V1 `[[processors]]` は v1.0 で廃止されました。\
				 transform 系 / voice は全て Flowgraph ノードに移行してください。\
				 twitch (IRC/EventSub) のみ当面 `[[processors]]` 経由でも起動します（Part E 次回で bridges に移管予定）。",
				conf.processors.len()
			);
		}

		// δ-9 Part A: Flowgraph ロード + ワーカー spawn。`State` 全体への `Weak` を ExecCtx に渡して
		// `channel.emit` 等が `State::push_channel_datum` を呼べるようにする。
		if let Some(root) = conf.resolved_flowgraph_dir() {
			let (audio_sink_clone, flowgraph_arc, mode_for_gate, runtime_root, profile_path) = {
				let s = state.read().await;
				let m = s.runtime_mode_id.read().ok().and_then(|g| g.clone());
				(
					s.audio_sink.clone(),
					s.flowgraph.clone(),
					m,
					s.runtime_paths.root.clone(),
					s.conf_source_path.clone(),
				)
			};
			let state_weak = Arc::downgrade(&state);
			let mut rt = crate::flowgraph::FlowgraphRuntime::load_and_spawn(
				&root,
				state_weak,
				Some(audio_sink_clone),
				Some(conf),
				mode_for_gate.as_deref(),
				Some(runtime_mode_id_for_flowgraph),
			);
			if let Some(profile_path) = profile_path.as_ref() {
				rt.set_profile_local_state_snapshot_path(&runtime_root, profile_path);
			}
			crate::flowgraph::runtime::log_load_outcome(&rt);
			*flowgraph_arc.write().await = Some(rt);
		}
		Ok(state)
	}

	pub async fn rfind_channel_datum(&self, id: u64) -> Option<ChannelDatum> {
		let channel_data = self.channel_data.read().await;
		let datum = channel_data.iter().rfind(|cd| cd.get_id() == id);
		datum.cloned()
	}

	pub async fn push_channel_data(&self, channel_data: ChannelData) {
		for cd in channel_data.into_iter() {
			self.push_channel_datum(cd).await;
		}
	}

	pub async fn push_channel_datum(&self, cd: ChannelDatum) {
		self.push_channel_datum_with_state_key(cd, None).await;
	}

	/// 同じchannel・state keyの過去datumを置換してから通常どおり配信する。
	///
	/// 画面観測や外部状態のように履歴全件ではなく「現在値」だけが必要なデータ向け。
	/// 返り値は置換されたdatumで、一時添付ファイルの後始末をproducer側で行える。
	pub async fn push_channel_datum_replacing_state(&self, mut cd: ChannelDatum, state_key: &str) -> Vec<ChannelDatum> {
		cd.meta.insert(ChannelDatum::META_STATE_KEY.to_string(), state_key.into());
		self.push_channel_datum_with_state_key(cd, Some(state_key)).await
	}

	async fn push_channel_datum_with_state_key(&self, cd: ChannelDatum, state_key: Option<&str>) -> Vec<ChannelDatum> {
		let id = cd.get_id();
		let channel_from = cd.channel.clone();
		let notify_ai = !cd.is_preflight_probe();

		log::trace!("ChannelDatum を追加します: {:?}", cd);
		let replaced = self.append_channel_datum_trim_capacity(cd, state_key).await;

		// Control API WS 購読者へ通知。δ-9 D.3 (v1.0) で V1 Processor dispatch は除去済み。
		self.emit_channel_datum_event(ChannelDatumPhase::Pushed, id).await;
		self.broadcast_channel_datum(id).await;
		if notify_ai {
			self.notify_ai_observation(id, &channel_from);
		}

		if self.state_data_auto_save {
			log::trace!("state_data_auto_save が有効になっているため、保存処理を行います。");
			self.save().await.unwrap();
		}
		log::trace!("push_channel_datum の処理が完了しました。");
		replaced
	}

	/// `channel_data` にだけ追加し **Processor は実行しない**（OpenAI ストリーミングの中間表示用）。
	pub async fn push_channel_datum_quiet(&self, cd: ChannelDatum) {
		let id = cd.get_id();
		log::trace!("ChannelDatum を quiet に追加します（Processor は未実行）: {:?}", cd);
		self.append_channel_datum_trim_capacity(cd, None).await;
		// GUI にはストリーム中間も見えた方が嬉しいので PushedQuiet で通知する。
		self.emit_channel_datum_event(ChannelDatumPhase::PushedQuiet, id).await;
		// δ-9 Part E: Flowgraph channel.subscribe にも quiet push を届ける（is_final=false なので
		// require_final フィルタのある購読者には届かない）。
		self.broadcast_channel_datum(id).await;
	}

	/// OBS / GUI 出力面だけを検査する `ChannelDatum` を追加する。
	///
	/// `push_channel_datum_quiet` と異なり、Flowgraph `channel.subscribe` とAI観測へは流さない。
	/// 配信前字幕probeが会話やTTSを誤発火させないための専用境界。
	pub async fn push_channel_datum_output_only(&self, cd: ChannelDatum) {
		let id = cd.get_id();
		log::trace!("ChannelDatum を output-only に追加します: {:?}", cd);
		self.append_channel_datum_trim_capacity(cd, None).await;
		self.emit_channel_datum_event(ChannelDatumPhase::PushedQuiet, id).await;
	}

	/// AI内部の判断材料を履歴とControl Eventへ残すが、通常のAI観測通知やFlowgraph ingressへ再配送しない。
	/// Initiative Schedulerが自分で生成した候補を二重処理しないための境界。
	pub async fn push_channel_datum_ai_internal(&self, cd: ChannelDatum) {
		let id = cd.get_id();
		log::trace!("ChannelDatum をAI内部観測として追加します: {:?}", cd);
		self.append_channel_datum_trim_capacity(cd, None).await;
		self.emit_channel_datum_event(ChannelDatumPhase::Pushed, id).await;
		if self.state_data_auto_save {
			if let Err(error) = self.save().await {
				log::warn!("AI内部観測のstate保存に失敗しました: {error:#}");
			}
		}
	}

	/// `id` に一致する `ChannelDatum` の `content` を置き換える（ストリーミング追記のたびに呼ぶ）。
	pub async fn update_channel_datum_content_by_id(&self, id: u64, content: String) -> bool {
		let len = content.chars().count();
		let updated = {
			let mut channel_data = self.channel_data.write().await;
			let mut hit = false;
			for cd in channel_data.iter_mut().rev() {
				if cd.get_id() == id {
					cd.content = content;
					log::trace!("ChannelDatum id={} の content を更新しました（len={}）。", id, len);
					hit = true;
					break;
				}
			}
			hit
		};
		if updated {
			self.emit_channel_datum_event(ChannelDatumPhase::Updated, id).await;
		} else {
			log::warn!("update_channel_datum_content_by_id: id={} が見つかりませんでした。", id);
		}
		updated
	}

	/// Expose only the delivered prefix to conversation history, without
	/// replaying Flowgraph effects on every audio segment. Final dispatch is
	/// performed once by the delivery watcher after the utterance settles.
	pub(crate) async fn update_delivered_channel_datum(&self, id: u64, content: String, marker: &str) -> bool {
		{
			let mut data = self.channel_data.write().await;
			let Some(datum) = data.iter_mut().find(|datum| datum.get_id() == id) else {
				return false;
			};
			datum.content = content;
			if !datum.content.is_empty() {
				datum.flags.insert(ChannelDatum::FLAG_IS_FINAL.to_string());
				datum.flags.insert(marker.to_owned());
			}
		}
		self.emit_channel_datum_event(ChannelDatumPhase::Updated, id).await;
		true
	}

	/// 内容を確定し `is_final` と追加フラグを付けたうえで、`push_channel_datum` と同等の Processor 連鎖と auto_save を行う。
	pub async fn finalize_channel_datum_and_dispatch(&self, id: u64, content: String, processor_marker_flag: &str) {
		let (channel_from, notify_ai) = {
			let mut channel_data = self.channel_data.write().await;
			let Some(cd) = channel_data.iter_mut().rev().find(|d| d.get_id() == id) else {
				log::error!("finalize_channel_datum_and_dispatch: id={} が channel_data にありません。", id);
				return;
			};
			cd.content = content;
			cd.flags.insert(ChannelDatum::FLAG_IS_FINAL.to_string());
			cd.flags.insert(processor_marker_flag.to_string());
			if !cd.is_preflight_probe() {
				self.speech_scheduler.playback.observe_final(&cd.channel, cd.get_id());
			}
			(cd.channel.clone(), !cd.is_preflight_probe())
		};
		log::trace!("ChannelDatum id={} を確定しました。", id);
		// δ-9 D.3 (v1.0): 旧 V1 Processor への dispatch は除去済み。Flowgraph 側は `ingress.*` 経由で
		// 別系統に流れているため、ここでは WS 購読者 / AI observation / Flowgraph channel 購読者の 3 系統に通知する。
		self.emit_channel_datum_event(ChannelDatumPhase::Finalized, id).await;
		self.broadcast_channel_datum(id).await;
		if notify_ai {
			self.notify_ai_observation(id, &channel_from);
		}
		let _ = &channel_from;
		if self.state_data_auto_save {
			log::trace!("state_data_auto_save が有効になっているため、保存処理を行います。");
			self.save().await.unwrap();
		}
	}

	async fn append_channel_datum_trim_capacity(&self, cd: ChannelDatum, state_key: Option<&str>) -> Vec<ChannelDatum> {
		let mut channel_data = self.channel_data.write().await;
		if cd.has_flag(ChannelDatum::FLAG_IS_FINAL) && !cd.is_preflight_probe() {
			self.speech_scheduler.playback.observe_final(&cd.channel, cd.get_id());
		}
		let mut replaced = Vec::new();
		if let Some(state_key) = state_key {
			let channel = cd.channel.as_str();
			let mut kept = VecDeque::with_capacity(channel_data.len());
			while let Some(existing) = channel_data.pop_front() {
				let same_state = existing.channel == channel
					&& existing.meta_get(ChannelDatum::META_STATE_KEY).and_then(serde_json::Value::as_str) == Some(state_key);
				if same_state {
					replaced.push(existing);
				} else {
					kept.push_back(existing);
				}
			}
			*channel_data = kept;
		}
		channel_data.push_back(cd);
		if channel_data.len() > self.state_data_capacity {
			log::trace!("channel_data の容量が上限を超えたため、先頭の要素を削除します。");
			channel_data.pop_front();
		}
		log::trace!("channel_data の容量: {}", channel_data.len());
		replaced
	}

	/// Control API WebSocket (`/api/v1/control/events`) に 1 件流す。受信者 0 件は正常系として飲み込む。
	///
	/// `cd` を `RwLock` 越しに読んで組み立てる。**呼び出し側が書き込みロックを保持したまま** 呼ぶとデッドロック
	/// もしくは無用なロック伸長を招くため、必ず write lock を解放してから呼ぶこと。
	async fn emit_channel_datum_event(&self, phase: ChannelDatumPhase, id: u64) {
		// 先に channel_data の read ロックで必要情報を取り、ロック外で send する。
		let payload = {
			let channel_data = self.channel_data.read().await;
			channel_data
				.iter()
				.rev()
				.find(|c| c.get_id() == id)
				.map(|cd| control_event_from_channel_datum(phase, cd))
		};
		if let Some(ev) = payload {
			if let Err(e) = self.control_event_tx.send(ev) {
				// 受信者 0 件（GUI 未接続）は想定内。
				log::trace!("control_event_tx.send に失敗（受信者 0 件の可能性）: {e}");
			}
		}
	}

	/// δ-9 Part E: `channel_datum_tx` に現在格納されている `ChannelDatum` のクローンを送る。
	/// Flowgraph `channel.subscribe` bridge が subscribe して `TriggerEvent` に変換する。
	/// 受信者 0 件は正常系として trace ログにとどめる。
	async fn broadcast_channel_datum(&self, id: u64) {
		let payload = {
			let channel_data = self.channel_data.read().await;
			channel_data.iter().rev().find(|c| c.get_id() == id).cloned()
		};
		if let Some(cd) = payload {
			if let Err(e) = self.channel_datum_tx.send(cd) {
				log::trace!("channel_datum_tx.send: 受信者 0 件の可能性 ({e})");
			}
		}
	}

	/// `ai_observation_tx` に観測通知を送る。受信者がいなくてもエラーにしない（`[[ai.personas]]` 未設定時を許容）。
	fn notify_ai_observation(&self, id: u64, channel: &str) {
		// 受信者が 0 件なら SendError になるが、AI サービスが居ないだけの正常系なので debug ログのみに留める。
		if let Err(e) = self.ai_observation_tx.send(Observation {
			datum_id: id,
			channel: channel.to_string(),
		}) {
			log::trace!("AI observation 送信先が 0 件でした（`[[ai.personas]]` 未設定の可能性）: {:?}", e);
		}
	}

	// δ-9 D.2/D.3 (v1.0): V1 `Processor` trait / `dispatch_processors_for_incoming` は全廃。
	// 旧 `[[processors]]` パイプラインに相当する処理は Flowgraph ノードチェーン側へ移管済み。
	// ingress → push_channel_datum で届いた Datum に対しては、Flowgraph `ingress.*` ノード / WS 購読者
	// のみが反応する。

	pub async fn save(&self) -> Result<()> {
		let Some(path) = self.state_data_path.as_ref() else {
			log::trace!("state_data_path が未設定のため、保存処理をスキップします。");
			return Ok(());
		};
		save_channel_data(path, &self.channel_data).await?;
		Ok(())
	}

	pub async fn load(&mut self) -> Result<()> {
		let channel_data = load_channel_data(self.state_data_path.as_ref().unwrap()).await?;
		self.channel_data = channel_data;
		Ok(())
	}
}

async fn load_channel_data<P: AsRef<Path>>(path: P) -> Result<SharedChannelData> {
	let path = path.as_ref();
	if !path.exists() {
		log::warn!(
			"指定されたファイルが存在しないため、読み込み処理を行えませんでした。 path = {:?}",
			path
		);
		return Ok(Arc::new(RwLock::new(VecDeque::new())));
	}
	let serialized_channel_data = tokio::fs::read_to_string(path).await?;
	let channel_data = ron::de::from_str(&serialized_channel_data)?;
	Ok(Arc::new(RwLock::new(channel_data)))
}

async fn save_channel_data<P: AsRef<Path>>(path: P, channel_data: &SharedChannelData) -> Result<()> {
	let channel_data = channel_data.read().await;
	let channel_data = channel_data.iter().collect::<VecDeque<_>>();
	let serialized_channel_data = ron::ser::to_string_pretty(&channel_data, ron::ser::PrettyConfig::new().indentor(" ".to_string()))?;
	tokio::fs::write(path, serialized_channel_data).await?;
	Ok(())
}

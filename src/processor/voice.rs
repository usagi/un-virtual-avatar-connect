//! 《Voice》: 既定の録音デバイスから PCM を取り込み、**Vosk** または **Whisper（whisper.cpp）**で文字起こしし、
//! `flowgraph.ingress.voice` ノードに `TriggerEvent` として流し込む ingress。
//!
//! **既定エンジン**: `vosk`（`engine` プロパティ省略時）。
//!
//! **Vosk**: `model_path`（展開済みディレクトリ、または alphacephei のモデル ID で初回自動取得）。
//!
//! **Whisper**（オプション）: `voice-whisper`で外部STT workerとのIPCを有効化する。
//! `engine = "whisper"` と `model_path`（既定ビルドは Vosk のみ）。
//!
//! **入力**: cpal の既定入力デバイス。
//!
//! ## 経路 (δ-9 Part E.2 以降)
//!
//! V1 `[[processors]]` 経路は v1.0 で完全除去済み。本モジュールは `flowgraph.ingress.voice`
//! から [`crate::bridges::voice::spawn`] 経由で呼ばれ、認識結果は [`VoiceSink`] を介して
//! `TriggerHandle::send` で Flowgraph に投入される。
//!
//! 部分認識は観測イベントと speech floor の保持に使い、Flowgraph には確定テキストだけを送る。

#[derive(Clone, Debug)]
pub(crate) struct VoiceWorkerStatus {
	pub(crate) node_id: String,
	pub(crate) engine: String,
	pub(crate) requested_backend: String,
	pub(crate) active_backend: String,
	pub(crate) partial_interval_ms: u64,
	pub(crate) partial_min_audio_ms: u64,
	pub(crate) partial_attempt_count: u64,
	pub(crate) partial_count: u64,
	pub(crate) partial_deadline_miss_count: u64,
	pub(crate) final_count: u64,
	pub(crate) error_count: u64,
	pub(crate) last_partial_inference_ms: Option<u64>,
	pub(crate) last_final_inference_ms: Option<u64>,
	pub(crate) last_activity_at: Option<String>,
	pub(crate) last_error: Option<String>,
}

pub(crate) type SharedVoiceWorkerStatus = std::sync::Arc<std::sync::RwLock<VoiceWorkerStatus>>;

pub(crate) fn voice_worker_status(ingress: &FlowgraphVoiceIngress) -> SharedVoiceWorkerStatus {
	std::sync::Arc::new(std::sync::RwLock::new(VoiceWorkerStatus {
		node_id: ingress.node_id.clone(),
		engine: ingress.engine.clone(),
		requested_backend: if ingress.engine == "whisper" {
			ingress.whisper_backend.clone()
		} else {
			ingress.engine.clone()
		},
		active_backend: "starting".into(),
		partial_interval_ms: ingress.partial_interval_ms,
		partial_min_audio_ms: ingress.partial_min_audio_ms,
		partial_attempt_count: 0,
		partial_count: 0,
		partial_deadline_miss_count: 0,
		final_count: 0,
		error_count: 0,
		last_partial_inference_ms: None,
		last_final_inference_ms: None,
		last_activity_at: None,
		last_error: None,
	}))
}

pub(crate) fn set_voice_worker_backend(status: &SharedVoiceWorkerStatus, backend: impl Into<String>) {
	if let Ok(mut status) = status.write() {
		status.active_backend = backend.into();
	}
}

pub(crate) fn record_voice_worker_partial(status: &SharedVoiceWorkerStatus, inference_ms: Option<u64>, emitted: bool) {
	if let Ok(mut status) = status.write() {
		status.partial_attempt_count = status.partial_attempt_count.saturating_add(1);
		if emitted {
			status.partial_count = status.partial_count.saturating_add(1);
		}
		if inference_ms.is_some_and(|elapsed| status.partial_interval_ms > 0 && elapsed > status.partial_interval_ms) {
			status.partial_deadline_miss_count = status.partial_deadline_miss_count.saturating_add(1);
		}
		status.last_partial_inference_ms = inference_ms;
		status.last_activity_at = Some(jiff::Timestamp::now().to_string());
	}
}

pub(crate) fn record_voice_worker_final(status: &SharedVoiceWorkerStatus, inference_ms: Option<u64>) {
	if let Ok(mut status) = status.write() {
		status.final_count = status.final_count.saturating_add(1);
		status.last_final_inference_ms = inference_ms;
		status.last_activity_at = Some(jiff::Timestamp::now().to_string());
	}
}

pub(crate) fn record_voice_worker_error(status: &SharedVoiceWorkerStatus, error: impl Into<String>) {
	if let Ok(mut status) = status.write() {
		status.error_count = status.error_count.saturating_add(1);
		status.last_activity_at = Some(jiff::Timestamp::now().to_string());
		status.last_error = Some(error.into());
	}
}

pub(crate) struct VoiceIngress {
	pub(crate) join_handle: std::thread::JoinHandle<()>,
	/// Worker ループに停止要求を伝えるフラグ。`finish` で立てる。
	pub(crate) stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
	pub(crate) speech_floor: std::sync::Arc<crate::state::SpeechFloorManager>,
	pub(crate) speech_floor_key: String,
	pub(crate) status: SharedVoiceWorkerStatus,
}

impl VoiceIngress {
	pub(crate) fn release_speech_floor(&self) {
		if !self.speech_floor_key.is_empty() {
			self.speech_floor.set_held(&self.speech_floor_key, false);
		}
	}

	pub(crate) fn status(&self) -> VoiceWorkerStatus {
		self.status
			.read()
			.map(|status| status.clone())
			.unwrap_or_else(|_| VoiceWorkerStatus {
				node_id: "unknown".into(),
				engine: "unknown".into(),
				requested_backend: "unknown".into(),
				active_backend: "unavailable".into(),
				partial_interval_ms: 0,
				partial_min_audio_ms: 0,
				partial_attempt_count: 0,
				partial_count: 0,
				partial_deadline_miss_count: 0,
				final_count: 0,
				error_count: 0,
				last_partial_inference_ms: None,
				last_final_inference_ms: None,
				last_activity_at: None,
				last_error: Some("voice worker status lock is poisoned".into()),
			})
	}

	pub(crate) async fn finish(self) {
		use std::sync::atomic::Ordering;
		let Self {
			join_handle,
			stop_flag,
			speech_floor,
			speech_floor_key,
			status: _,
		} = self;
		// 1. stop_flag を立てて worker ループが次のチェックで抜けるよう促す。
		stop_flag.store(true, Ordering::Relaxed);
		if !speech_floor_key.is_empty() {
			speech_floor.set_held(&speech_floor_key, false);
		}
		// 2. 実際のスレッド join は最大 3 秒だけ待つ。CPAL / Vosk / Whisper の
		//    リソース解放に時間がかかるケースもあるが、shutdown 時は OS が
		//    プロセス終了時にクリーンアップするのでここで無限待ちは避ける。
		let join = tokio::task::spawn_blocking(move || {
			let _ = join_handle.join();
		});
		if tokio::time::timeout(std::time::Duration::from_secs(3), join).await.is_err() {
			log::warn!("《Flowgraph/Voice》 worker スレッドの join が 3 秒で完了しませんでした。OS に任せて続行します。");
		}
	}
}

use crate::bridges::voice::FlowgraphVoiceIngress;
use crate::flowgraph::node::{TriggerEvent, TriggerHandle};
use crate::flowgraph::socket::SocketValue;
use crate::processor::voice_partial_preview::VoicePartialPreview;

// ---------------------------------------------------------------------
// VoiceSink: 認識結果を `flowgraph.ingress.voice` ノードに流すための薄いラッパ
// ---------------------------------------------------------------------

/// `flowgraph.ingress.voice` ノードへ TriggerEvent を発火するための sink。
///
/// `voice-vosk` / `voice-whisper` のいずれかでビルドされたときに voice_vosk.rs /
/// voice_whisper.rs から使われる。両方無効な `--no-default-features` ビルドでは未使用になるため
/// `#[allow(dead_code)]` で警告を抑える。
#[allow(dead_code)]
pub(crate) struct VoiceSink {
	pub trigger: TriggerHandle,
	pub node_id: String,
	/// `flowgraph.ingress.voice` の `fixed_channel` property。空なら `"voice"` を使う。
	pub fixed_channel: String,
	pub control_event_tx: tokio::sync::broadcast::Sender<crate::control_events::ControlEvent>,
	pub speech_floor: std::sync::Arc<crate::state::SpeechFloorManager>,
	pub voice_input: std::sync::Arc<crate::state::VoiceInputControl>,
	pub partial_preview: VoicePartialPreview,
	pub speech_floor_key: String,
	pub speech_floor_release_delay_ms: u64,
	floor_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
	next_utterance_id: u64,
	current_utterance: Option<VoiceUtterance>,
	suppress_current_utterance: bool,
}

#[allow(dead_code)]
struct VoiceUtterance {
	id: u64,
	started_at: std::time::Instant,
	last_partial: String,
}

#[allow(dead_code)]
impl VoiceSink {
	pub(crate) fn new(
		trigger: TriggerHandle,
		node_id: String,
		fixed_channel: String,
		control_event_tx: tokio::sync::broadcast::Sender<crate::control_events::ControlEvent>,
		speech_floor: std::sync::Arc<crate::state::SpeechFloorManager>,
		voice_input: std::sync::Arc<crate::state::VoiceInputControl>,
		partial_preview: VoicePartialPreview,
		speech_floor_key: String,
		speech_floor_release_delay_ms: u64,
	) -> Self {
		Self {
			trigger,
			node_id,
			fixed_channel,
			control_event_tx,
			speech_floor,
			voice_input,
			partial_preview,
			speech_floor_key,
			speech_floor_release_delay_ms,
			floor_generation: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
			next_utterance_id: 1,
			current_utterance: None,
			suppress_current_utterance: false,
		}
	}

	fn start_utterance(&mut self) -> &mut VoiceUtterance {
		if self.current_utterance.is_none() {
			let id = self.next_utterance_id;
			self.next_utterance_id = self.next_utterance_id.saturating_add(1);
			self.current_utterance = Some(VoiceUtterance {
				id,
				started_at: std::time::Instant::now(),
				last_partial: String::new(),
			});
		}
		self.current_utterance.as_mut().expect("voice utterance initialized")
	}

	fn emit_recognition(&self, phase: crate::control_events::VoiceRecognitionPhase, utterance_id: u64, text: String, elapsed_ms: u64) {
		let _ = self.control_event_tx.send(crate::control_events::ControlEvent::VoiceRecognition {
			phase,
			node_id: self.node_id.clone(),
			utterance_id,
			text,
			elapsed_ms,
			observed_at: jiff::Timestamp::now().to_string(),
		});
	}

	fn hold_speech_floor(&self) {
		use std::sync::atomic::Ordering;
		if self.speech_floor_key.is_empty() {
			return;
		}
		self.floor_generation.fetch_add(1, Ordering::SeqCst);
		self.speech_floor.set_held(&self.speech_floor_key, true);
	}

	fn release_speech_floor_after_delay(&self, rt: &tokio::runtime::Handle) {
		use std::sync::atomic::Ordering;
		if self.speech_floor_key.is_empty() {
			return;
		}
		let generation = self.floor_generation.fetch_add(1, Ordering::SeqCst) + 1;
		let current_generation = self.floor_generation.clone();
		let floor = self.speech_floor.clone();
		let key = self.speech_floor_key.clone();
		let delay_ms = self.speech_floor_release_delay_ms;
		rt.spawn(async move {
			tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
			if current_generation.load(Ordering::SeqCst) == generation {
				floor.set_held(&key, false);
			}
		});
	}

	/// 認識テキストが得られる前に発話開始だけを通知する。
	/// Whisper VADがspeech floorとutterance IDを先行確保するために使う。
	pub(crate) fn on_speech_started(&mut self) {
		if self.voice_input.is_muted() {
			self.suppress_current_utterance = true;
			self.discard_current_utterance();
			return;
		}
		self.suppress_current_utterance = false;
		self.hold_speech_floor();
		self.start_utterance();
	}

	/// 部分認識テキスト。AIを発火させず、観測イベントと任意の字幕previewだけを更新する。
	pub(crate) fn on_partial(&mut self, text: String) {
		if self.voice_input.is_muted() {
			self.suppress_current_utterance = true;
		}
		if self.suppress_current_utterance {
			self.discard_current_utterance();
			return;
		}
		self.hold_speech_floor();
		let (id, elapsed_ms) = {
			let utterance = self.start_utterance();
			utterance.last_partial = text.clone();
			(
				utterance.id,
				utterance.started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
			)
		};
		let preview = text.clone();
		self.emit_recognition(crate::control_events::VoiceRecognitionPhase::Partial, id, text, elapsed_ms);
		self.partial_preview.update(preview);
	}

	/// 確定テキストを `TriggerHandle::send` で投入する。
	pub(crate) fn on_final(&mut self, rt: &tokio::runtime::Handle, text: String) {
		if self.voice_input.is_muted() {
			self.suppress_current_utterance = true;
		}
		if self.suppress_current_utterance {
			self.suppress_current_utterance = false;
			self.discard_current_utterance();
			return;
		}
		if text.is_empty() {
			if let Some(utterance) = self.current_utterance.take() {
				self.emit_recognition(
					crate::control_events::VoiceRecognitionPhase::Discarded,
					utterance.id,
					utterance.last_partial,
					utterance.started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
				);
			}
			self.partial_preview.clear();
			self.release_speech_floor_after_delay(rt);
			return;
		}
		let utterance = self.current_utterance.take().unwrap_or_else(|| {
			let id = self.next_utterance_id;
			self.next_utterance_id = self.next_utterance_id.saturating_add(1);
			VoiceUtterance {
				id,
				started_at: std::time::Instant::now(),
				last_partial: String::new(),
			}
		});
		self.emit_recognition(
			crate::control_events::VoiceRecognitionPhase::Final,
			utterance.id,
			text.clone(),
			utterance.started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
		);
		self.partial_preview.finish(text.clone());
		self.release_speech_floor_after_delay(rt);
		let source_kind = if self.fixed_channel.is_empty() {
			"voice".to_string()
		} else {
			self.fixed_channel.clone()
		};
		let event = TriggerEvent::new(&self.node_id)
			.with_exec("__trigger__")
			.with_override("__content__", SocketValue::String(text))
			.with_override("__source_kind__", SocketValue::String(source_kind))
			.with_override("__meta__", SocketValue::Json(serde_json::json!({
				"speech_recognition": {
					"schema": "unvac.speech_recognition.v1",
					"node_id": self.node_id,
					"utterance_id": utterance.id,
				}
			})));
		if let Err(e) = self.trigger.send(event) {
			log::warn!("《Flowgraph/Voice》 trigger 送信失敗 node={}: {:?}", self.node_id, e);
		}
	}

	fn discard_current_utterance(&mut self) {
		self.current_utterance = None;
		self.partial_preview.clear();
		if !self.speech_floor_key.is_empty() {
			self.speech_floor.set_held(&self.speech_floor_key, false);
		}
	}
}

// ---------------------------------------------------------------------
// エントリ: flowgraph.ingress.voice から spawn
// ---------------------------------------------------------------------

/// `flowgraph.ingress.voice` ノードごとに Vosk / Whisper ワーカーを起動する。
///
/// 認識結果は `TriggerHandle::send` で該当ノードに流れ、graph の exec_out 下流へ伝播する。
/// 戻り値 `None` は engine / feature ビルド / モデルパス不足などで起動失敗を示す。
#[allow(unused_variables)]
pub(crate) fn spawn_from_flowgraph(
	ingress: &FlowgraphVoiceIngress,
	tokio_handle: tokio::runtime::Handle,
	trigger: TriggerHandle,
	control_event_tx: tokio::sync::broadcast::Sender<crate::control_events::ControlEvent>,
	speech_floor: std::sync::Arc<crate::state::SpeechFloorManager>,
	voice_input: std::sync::Arc<crate::state::VoiceInputControl>,
	state: std::sync::Weak<tokio::sync::RwLock<crate::state::State>>,
) -> Option<VoiceIngress> {
	match ingress.engine.as_str() {
		"vosk" => {
			#[cfg(feature = "voice-vosk")]
			{
				super::voice_vosk::spawn_from_flowgraph(ingress, tokio_handle, trigger, control_event_tx, speech_floor, voice_input, state)
			}
			#[cfg(not(feature = "voice-vosk"))]
			{
				log::error!(
					"《Flowgraph/Voice》: engine=vosk ですが voice-vosk 機能有効でビルドされていません。`cargo build` 既定 or `--features voice-vosk` でビルドしてください。node={}",
					ingress.node_id
				);
				None
			}
		}
		"whisper" | "whisper-rs" => {
			#[cfg(feature = "voice-whisper")]
			{
				super::voice_whisper::spawn_from_flowgraph(
					ingress,
					tokio_handle,
					trigger,
					control_event_tx,
					speech_floor,
					voice_input,
					state,
				)
			}
			#[cfg(not(feature = "voice-whisper"))]
			{
				log::error!(
					"《Flowgraph/Voice》: engine=whisper ですが外部Whisper worker対応が含まれていません。portable releaseを使用してください。node={}",
					ingress.node_id
				);
				None
			}
		}
		other => {
			log::error!(
				"《Flowgraph/Voice》: 不明な engine {:?}（voskまたはwhisperを指定してください）node={}",
				other,
				ingress.node_id
			);
			None
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::engine::create_trigger_bus;
	use crate::state::{SpeechFloorManager, VoiceInputControl};
	use std::sync::Arc;
	use tokio::sync::broadcast;

	fn sink(
		voice_input: Arc<VoiceInputControl>,
	) -> (
		VoiceSink,
		tokio::sync::mpsc::UnboundedReceiver<TriggerEvent>,
		broadcast::Receiver<crate::control_events::ControlEvent>,
	) {
		let (trigger, trigger_rx) = create_trigger_bus();
		let (event_tx, event_rx) = broadcast::channel(8);
		(
			VoiceSink::new(
				trigger,
				"voice".into(),
				"user".into(),
				event_tx,
				Arc::new(SpeechFloorManager::new()),
				voice_input,
				VoicePartialPreview::disabled(),
				"human_voice".into(),
				0,
			),
			trigger_rx,
			event_rx,
		)
	}

	#[tokio::test]
	async fn muted_sink_discards_partial_and_final() {
		let control = Arc::new(VoiceInputControl::default());
		control.set_muted(true);
		let (mut sink, mut trigger_rx, mut event_rx) = sink(control);
		let runtime = tokio::runtime::Handle::current();

		sink.on_speech_started();
		sink.on_partial("partial".into());
		sink.on_final(&runtime, "final".into());

		assert!(trigger_rx.try_recv().is_err());
		assert!(event_rx.try_recv().is_err());
	}

	#[tokio::test]
	async fn utterance_started_while_muted_stays_suppressed_after_unmute() {
		let control = Arc::new(VoiceInputControl::default());
		control.set_muted(true);
		let (mut sink, mut trigger_rx, mut event_rx) = sink(control.clone());
		let runtime = tokio::runtime::Handle::current();

		sink.on_speech_started();
		control.set_muted(false);
		sink.on_final(&runtime, "must not leak".into());

		assert!(trigger_rx.try_recv().is_err());
		assert!(event_rx.try_recv().is_err());

		sink.on_speech_started();
		sink.on_final(&runtime, "next utterance".into());
		assert!(trigger_rx.try_recv().is_ok());
		assert!(event_rx.try_recv().is_ok());
	}

	#[tokio::test]
	async fn unmuted_sink_emits_final_and_trigger() {
		let control = Arc::new(VoiceInputControl::default());
		let (mut sink, mut trigger_rx, mut event_rx) = sink(control);
		let runtime = tokio::runtime::Handle::current();

		sink.on_final(&runtime, "heard".into());

		let event = event_rx.try_recv().expect("voice recognition event");
		assert!(matches!(
			event,
			crate::control_events::ControlEvent::VoiceRecognition {
				phase: crate::control_events::VoiceRecognitionPhase::Final,
				text,
				..
			} if text == "heard"
		));
		let trigger = trigger_rx.try_recv().expect("flowgraph trigger");
		assert_eq!(trigger.node_id, "voice");
		assert_eq!(
			trigger.data_overrides.get("__meta__"),
			Some(&SocketValue::Json(serde_json::json!({
				"speech_recognition": {
					"schema": "unvac.speech_recognition.v1",
					"node_id": "voice",
					"utterance_id": 1,
				}
			})))
		);
		assert_eq!(
			trigger
				.data_overrides
				.get("__content__")
				.expect("content override")
				.as_str()
				.unwrap(),
			"heard"
		);
	}

	#[test]
	fn worker_status_records_inference_and_recovers_from_error() {
		let status = std::sync::Arc::new(std::sync::RwLock::new(VoiceWorkerStatus {
			node_id: "voice".into(),
			engine: "whisper".into(),
			requested_backend: "auto".into(),
			active_backend: "vulkan".into(),
			partial_interval_ms: 650,
			partial_min_audio_ms: 1_300,
			partial_attempt_count: 0,
			partial_count: 0,
			partial_deadline_miss_count: 0,
			final_count: 0,
			error_count: 0,
			last_partial_inference_ms: None,
			last_final_inference_ms: None,
			last_activity_at: None,
			last_error: None,
		}));

		record_voice_worker_error(&status, "temporary failure");
		record_voice_worker_partial(&status, Some(700), false);
		record_voice_worker_partial(&status, Some(42), true);
		record_voice_worker_final(&status, Some(84));

		let status = status.read().unwrap();
		assert_eq!(status.partial_attempt_count, 2);
		assert_eq!(status.partial_count, 1);
		assert_eq!(status.final_count, 1);
		assert_eq!(status.partial_deadline_miss_count, 1);
		assert_eq!(status.last_partial_inference_ms, Some(42));
		assert_eq!(status.last_final_inference_ms, Some(84));
		assert!(status.last_activity_at.is_some());
		assert_eq!(status.error_count, 1);
		assert_eq!(status.last_error.as_deref(), Some("temporary failure"));
	}
}

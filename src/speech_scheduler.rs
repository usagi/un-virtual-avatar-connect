//! AI共演者とチャット読み上げが共有する音声再生順序の制御。
//!
//! このモジュールはTTS合成やRodio再生から独立した公平制御コアを持つ。
//! 待機時間は診断にだけ使い、受理済み項目の暗黙破棄には使わない。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Semaphore};

use crate::avatar_motion::{AvatarMotionConfig, AvatarMotionController};
use crate::control_events::{ControlEvent, TtsLifecyclePhase};
use crate::flowgraph::tts::driver::{maybe_save_wav, AudioContext, TtsDriver, TtsError, TtsOutcome, TtsRequest, TtsSynthesis};

/// TTS要求ごとの処理時間をControl APIへ通知する観測情報。
#[derive(Clone)]
pub(crate) struct SpeechLifecycleObserver {
	tx: tokio::sync::broadcast::Sender<ControlEvent>,
	node_id: String,
	utterance_id: String,
	engine: String,
	voice: String,
	lane: SpeechLane,
	text_chars: usize,
	observation_origin: Option<(String, u64)>,
	started: Instant,
}

impl SpeechLifecycleObserver {
	pub fn new(
		tx: tokio::sync::broadcast::Sender<ControlEvent>,
		node_id: String,
		utterance_id: String,
		engine: String,
		voice: String,
		lane: SpeechLane,
		text_chars: usize,
	) -> Self {
		Self {
			tx,
			node_id,
			utterance_id,
			engine,
			voice,
			lane,
			text_chars,
			observation_origin: None,
			started: Instant::now(),
		}
	}

	pub(crate) fn with_observation_origin(mut self, origin: Option<(String, u64)>) -> Self {
		self.observation_origin = origin;
		self
	}

	pub fn emit(&self, phase: TtsLifecyclePhase, played: Option<bool>, error: Option<String>) {
		self.emit_queue(phase, played, error, None, None);
	}

	pub fn emit_queue(
		&self,
		phase: TtsLifecyclePhase,
		played: Option<bool>,
		error: Option<String>,
		queue_depth: Option<usize>,
		wait_ms: Option<u64>,
	) {
		let _ = self.tx.send(ControlEvent::TtsLifecycle {
			phase,
			node_id: self.node_id.clone(),
			utterance_id: self.utterance_id.clone(),
			observation_channel: self.observation_origin.as_ref().map(|(channel, _)| channel.clone()),
			observation_datum_id: self.observation_origin.as_ref().map(|(_, id)| *id),
			engine: self.engine.clone(),
			voice: self.voice.clone(),
			lane: self.lane.as_str().to_string(),
			text_chars: self.text_chars,
			elapsed_ms: self.started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
			queue_depth,
			wait_ms,
			played,
			error,
			observed_at: jiff::Timestamp::now().to_string(),
		});
	}
}

/// 発話元ごとのスケジューリング区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpeechLane {
	Assistant,
	Chat,
	System,
}

impl SpeechLane {
	pub fn parse(value: &str) -> Option<Self> {
		match value.trim().to_ascii_lowercase().as_str() {
			"assistant" => Some(Self::Assistant),
			"chat" => Some(Self::Chat),
			"system" => Some(Self::System),
			_ => None,
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Self::Assistant => "assistant",
			Self::Chat => "chat",
			Self::System => "system",
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SpeechQueuePolicy {
	max_wait: Option<Duration>,
	max_pending: Option<usize>,
}

impl SpeechQueuePolicy {
	const CHAT_DEFAULT_MAX_WAIT: Duration = Duration::from_secs(30);
	const CHAT_DEFAULT_MAX_PENDING: usize = 20;

	pub fn for_lane(lane: SpeechLane) -> Self {
		match lane {
			SpeechLane::Chat => Self {
				max_wait: Some(Self::CHAT_DEFAULT_MAX_WAIT),
				max_pending: Some(Self::CHAT_DEFAULT_MAX_PENDING),
			},
			SpeechLane::Assistant | SpeechLane::System => Self {
				max_wait: None,
				max_pending: None,
			},
		}
	}

	pub fn with_overrides(lane: SpeechLane, max_wait_ms: i64, max_pending: i64) -> Self {
		let defaults = Self::for_lane(lane);
		Self {
			max_wait: match max_wait_ms {
				value if value < 0 => None,
				0 => defaults.max_wait,
				value => Some(Duration::from_millis(value as u64)),
			},
			max_pending: match max_pending {
				value if value < 0 => None,
				0 => defaults.max_pending,
				value => Some(value as usize),
			},
		}
	}
}

/// Schedulerが受理した発話片。
#[derive(Debug)]
pub(crate) struct SpeechItem<T> {
	pub lane: SpeechLane,
	pub utterance_id: String,
	pub segment_index: u32,
	pub payload: T,
	enqueued_at: Instant,
	policy: SpeechQueuePolicy,
}

impl<T> SpeechItem<T> {
	pub fn new(lane: SpeechLane, utterance_id: impl Into<String>, segment_index: u32, payload: T) -> Self {
		Self {
			lane,
			utterance_id: utterance_id.into(),
			segment_index,
			payload,
			enqueued_at: Instant::now(),
			policy: SpeechQueuePolicy::for_lane(lane),
		}
	}

	pub fn with_policy(mut self, policy: SpeechQueuePolicy) -> Self {
		self.policy = policy;
		self
	}

	#[cfg(test)]
	fn with_enqueued_at(mut self, enqueued_at: Instant) -> Self {
		self.enqueued_at = enqueued_at;
		self
	}

	pub fn waiting_for(&self, now: Instant) -> Duration {
		now.saturating_duration_since(self.enqueued_at)
	}

	fn is_expired(&self, now: Instant) -> bool {
		self.policy.max_wait.is_some_and(|max_wait| self.waiting_for(now) >= max_wait)
	}
}

/// AIの連続発話を制限しつつ、AIとチャットの双方を飢餓状態にしないキュー。
#[derive(Debug)]
pub(crate) struct FairSpeechQueue<T> {
	system: VecDeque<SpeechItem<T>>,
	assistant: VecDeque<SpeechItem<T>>,
	chat: VecDeque<SpeechItem<T>>,
	assistant_burst: usize,
	assistant_since_chat: usize,
}

impl<T> FairSpeechQueue<T> {
	pub fn new(assistant_burst: usize) -> Self {
		Self {
			system: VecDeque::new(),
			assistant: VecDeque::new(),
			chat: VecDeque::new(),
			assistant_burst: assistant_burst.max(1),
			assistant_since_chat: 0,
		}
	}

	#[cfg(test)]
	pub fn push(&mut self, item: SpeechItem<T>) {
		match item.lane {
			SpeechLane::Assistant => self.assistant.push_back(item),
			SpeechLane::Chat => self.chat.push_back(item),
			SpeechLane::System => self.system.push_back(item),
		}
	}

	fn push_bounded(&mut self, item: SpeechItem<T>) -> Option<SpeechItem<T>> {
		let queue = match item.lane {
			SpeechLane::Assistant => &mut self.assistant,
			SpeechLane::Chat => &mut self.chat,
			SpeechLane::System => &mut self.system,
		};
		let dropped = item
			.policy
			.max_pending
			.filter(|max_pending| queue.len() >= *max_pending)
			.and_then(|_| queue.pop_front());
		queue.push_back(item);
		dropped
	}

	fn drain_expired(&mut self, now: Instant) -> Vec<SpeechItem<T>> {
		fn drain<T>(queue: &mut VecDeque<SpeechItem<T>>, now: Instant, expired: &mut Vec<SpeechItem<T>>) {
			let count = queue.len();
			for _ in 0..count {
				let item = queue.pop_front().expect("queue length was captured");
				if item.is_expired(now) {
					expired.push(item);
				} else {
					queue.push_back(item);
				}
			}
		}
		let mut expired = Vec::new();
		drain(&mut self.system, now, &mut expired);
		drain(&mut self.assistant, now, &mut expired);
		drain(&mut self.chat, now, &mut expired);
		expired
	}

	pub fn pop_next(&mut self) -> Option<SpeechItem<T>> {
		if let Some(item) = self.system.pop_front() {
			return Some(item);
		}

		match (self.assistant.is_empty(), self.chat.is_empty()) {
			(false, false) if self.assistant_since_chat >= self.assistant_burst => {
				self.assistant_since_chat = 0;
				self.chat.pop_front()
			}
			(false, false) => {
				self.assistant_since_chat += 1;
				self.assistant.pop_front()
			}
			(false, true) => {
				self.assistant_since_chat = 0;
				self.assistant.pop_front()
			}
			(true, false) => {
				self.assistant_since_chat = 0;
				self.chat.pop_front()
			}
			(true, true) => None,
		}
	}

	pub fn len(&self) -> usize {
		self.system.len() + self.assistant.len() + self.chat.len()
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	pub fn oldest_wait(&self, now: Instant) -> Option<Duration> {
		self.system
			.front()
			.into_iter()
			.chain(self.assistant.front())
			.chain(self.chat.front())
			.map(|item| item.waiting_for(now))
			.max()
	}
}

struct ScheduledPayload {
	text: String,
	freshness: Option<crate::speech_playback::SpeechFreshness>,
	speech_floor: Option<(Arc<crate::state::SpeechFloorManager>, String)>,
	playback_segment: crate::speech_playback::SpeechPlaybackSegment,
	synthesis: Option<oneshot::Receiver<Result<(Vec<u8>, String), TtsError>>>,
	synthesis_cancelled: Arc<AtomicBool>,
	completion: oneshot::Sender<Result<TtsOutcome, TtsError>>,
	observer: Option<SpeechLifecycleObserver>,
	lip_sync: Option<AvatarMotionConfig>,
}

/// TTS合成を最大2件先読みし、再生は公平キューから一片ずつ直列化する共有ハンドル。
#[derive(Debug, Clone)]
pub(crate) struct SpeechScheduler {
	pub(crate) playback: crate::speech_playback::SpeechPlayback,
	tx: mpsc::UnboundedSender<SpeechItem<ScheduledPayload>>,
	synthesis_slots: Arc<Semaphore>,
	next_utterance_id: Arc<AtomicU64>,
}

impl SpeechScheduler {
	pub fn spawn(audio_sink: Option<crate::SharedAudioSink>, avatar_motion: AvatarMotionController) -> Self {
		let (tx, rx) = mpsc::unbounded_channel();
		let playback = crate::speech_playback::SpeechPlayback::default();
		tokio::spawn(run_scheduler(rx, audio_sink, avatar_motion.clone()));
		Self {
			playback,
			tx,
			synthesis_slots: Arc::new(Semaphore::new(2)),
			next_utterance_id: Arc::new(AtomicU64::new(1)),
		}
	}

	pub async fn speak(
		&self,
		lane: SpeechLane,
		utterance_id: String,
		segment_index: u32,
		policy: SpeechQueuePolicy,
		driver: Arc<dyn TtsDriver>,
		request: TtsRequest,
		lip_sync: Option<AvatarMotionConfig>,
		observer: Option<SpeechLifecycleObserver>,
	) -> Result<TtsOutcome, TtsError> {
		self.speak_with_floor(lane, utterance_id, segment_index, policy, driver, request, lip_sync, observer, None)
			.await
	}

	pub async fn speak_with_floor(
		&self,
		lane: SpeechLane,
		utterance_id: String,
		segment_index: u32,
		policy: SpeechQueuePolicy,
		driver: Arc<dyn TtsDriver>,
		request: TtsRequest,
		lip_sync: Option<AvatarMotionConfig>,
		observer: Option<SpeechLifecycleObserver>,
		speech_floor: Option<(Arc<crate::state::SpeechFloorManager>, String)>,
	) -> Result<TtsOutcome, TtsError> {
		if !driver.supports_deferred_playback() {
			return Err(TtsError::Config(format!(
				"engine '{}' はSpeech Schedulerによる再生順序制御に対応していません",
				driver.name()
			)));
		}

		let utterance_id = if utterance_id.trim().is_empty() {
			format!("speech-{}", self.next_utterance_id.fetch_add(1, Ordering::Relaxed))
		} else {
			utterance_id
		};
		let (synthesis_tx, synthesis_rx) = oneshot::channel();
		let freshness = self.playback.freshness(&utterance_id);
		let playback_segment = self.playback.track_segment(&utterance_id);
		let (completion_tx, completion_rx) = oneshot::channel();
		let slots = self.synthesis_slots.clone();
		let synthesis_observer = observer.clone();
		let synthesis_cancelled = Arc::new(AtomicBool::new(false));
		if self
			.tx
			.send(
				SpeechItem::new(
					lane,
					utterance_id,
					segment_index,
					ScheduledPayload {
						text: request.text.clone(),
						freshness,
						speech_floor,
						playback_segment,
						synthesis: Some(synthesis_rx),
						synthesis_cancelled: synthesis_cancelled.clone(),
						completion: completion_tx,
						observer: observer.clone(),
						lip_sync,
					},
				)
				.with_policy(policy),
			)
			.is_err()
		{
			synthesis_cancelled.store(true, Ordering::Release);
			let error = TtsError::Playback("Speech Schedulerが終了しています".into());
			if let Some(observer) = observer.as_ref() {
				observer.emit(TtsLifecyclePhase::Failed, Some(false), Some(error.to_string()));
			}
			return Err(error);
		}
		tokio::spawn(async move {
			let result = async {
				let _permit = slots
					.acquire_owned()
					.await
					.map_err(|_| TtsError::Synthesis("Speech Schedulerの合成スロットが終了しました".into()))?;
				if synthesis_cancelled.load(Ordering::Acquire) {
					return Err(TtsError::Playback("Speech Schedulerが破棄したため合成を中止しました".into()));
				}
				match driver.synthesize(&request).await? {
					TtsSynthesis::Wav(wav) => {
						let audio_path = maybe_save_wav(&wav, &request.save_path).await?;
						Ok((wav, audio_path))
					}
					TtsSynthesis::SelfPlayed => Err(TtsError::Config(format!(
						"engine '{}' が遅延再生不能な合成結果を返しました",
						driver.name()
					))),
				}
			}
			.await;
			if result.is_ok() {
				if let Some(observer) = synthesis_observer.as_ref() {
					observer.emit(TtsLifecyclePhase::Synthesized, None, None);
				}
			}
			let _ = synthesis_tx.send(result);
		});

		let result = match completion_rx.await {
			Ok(result) => result,
			Err(_) => {
				let error = TtsError::Playback("Speech Schedulerが結果を返さず終了しました".into());
				if let Some(observer) = observer.as_ref() {
					observer.emit(TtsLifecyclePhase::Failed, Some(false), Some(error.to_string()));
				}
				Err(error)
			}
		};
		result
	}
}

async fn run_scheduler(
	mut rx: mpsc::UnboundedReceiver<SpeechItem<ScheduledPayload>>,
	audio_sink: Option<crate::SharedAudioSink>,
	avatar_motion: AvatarMotionController,
) {
	const DELAY_WARNING: Duration = Duration::from_secs(15);
	let mut queue = FairSpeechQueue::new(2);
	loop {
		if queue.is_empty() {
			let Some(item) = rx.recv().await else {
				return;
			};
			enqueue_scheduled_item(&mut queue, item);
		}
		while let Ok(item) = rx.try_recv() {
			enqueue_scheduled_item(&mut queue, item);
		}
		for item in queue.drain_expired(Instant::now()) {
			drop_scheduled_item(item, "max_queue_wait_exceeded", queue.len());
		}
		if let Some(wait) = queue.oldest_wait(Instant::now()).filter(|wait| *wait >= DELAY_WARNING) {
			log::warn!(
				"《Speech Scheduler》 最古の発話が{}ms待機しています（破棄せず再生を継続）。queue={}",
				wait.as_millis(),
				queue.len()
			);
		}

		let Some(mut item) = queue.pop_next() else {
			continue;
		};
		if item.is_expired(Instant::now()) {
			drop_scheduled_item(item, "max_queue_wait_exceeded_before_playback", queue.len());
			continue;
		}
		log::trace!(
			"《Speech Scheduler》 playback selected lane={:?} utterance={} segment={} wait_ms={}",
			item.lane,
			item.utterance_id,
			item.segment_index,
			item.waiting_for(Instant::now()).as_millis()
		);
		if audio_sink.is_some() {
			if let Some(config) = item.payload.lip_sync.as_ref() {
				if let Err(error) = avatar_motion.prepare(config.clone()) {
					log::warn!("《Speech LipSync》 prewarm failed: {error}");
				}
			}
		}
		let synthesis_rx = item.payload.synthesis.take().expect("scheduled synthesis receiver must exist");
		let synthesis = tokio::select! {
			biased;
			_ = wait_for_superseded(item.payload.freshness.as_ref()) => {
				drop_stopped_item(item, queue.len(), "synthesis");
				continue;
			}
			result = synthesis_rx => result,
		};
		// Synthesis can finish after the human starts a new utterance.
		// Check every segment at the playback boundary, not just before AI generation.
		if matches!(&synthesis, Ok(Ok(_))) {
			if let Some((floor, key)) = item.payload.speech_floor.as_ref() {
				let remaining = item.policy.max_wait.map(|max| max.saturating_sub(item.waiting_for(Instant::now())));
				let released = tokio::select! {
					biased;
					_ = wait_for_superseded(item.payload.freshness.as_ref()) => false,
					result = async {
						if let Some(remaining) = remaining {
							tokio::time::timeout(remaining, floor.wait_until_free(key)).await.is_ok()
						} else { floor.wait_until_free(key).await; true }
					} => result,
				};
				if !released {
					let reason = if item.payload.freshness.as_ref().is_some_and(|guard| guard.is_cancelled()) {
						item.payload.freshness.as_ref().unwrap().stop_reason()
					} else {
						"max_queue_wait_exceeded_waiting_for_floor"
					};
					drop_scheduled_item_at_stage(item, reason, queue.len(), "speech_floor");
					continue;
				}
			}
		}
		if item.is_expired(Instant::now()) {
			drop_scheduled_item_at_stage(item, "max_queue_wait_exceeded_after_synthesis", queue.len(), "before_playback");
			continue;
		}
		let result = match synthesis {
			Ok(Ok((wav, audio_path))) => {
				if item.payload.freshness.as_ref().is_some_and(|guard| guard.is_cancelled()) {
					drop_stopped_item(item, queue.len(), "before_playback");
					continue;
				}
				let audio = AudioContext { sink: audio_sink.as_ref() };
				let delivered = audio
					.play_wav_if(wav.clone(), || {
						item.payload.freshness.as_ref().is_none_or(|guard| guard.commit_playback())
					})
					.await;
				if item.payload.freshness.as_ref().is_some_and(|guard| guard.is_cancelled()) {
					drop_stopped_item(item, queue.len(), "playback_commit");
					continue;
				}
				match delivered {
					Ok(played) => {
						if played {
							item.payload.playback_segment.started(item.segment_index, item.payload.text.clone());
							// A missing sink or decoder error must not start speech-bound gestures.
							if let Some(observer) = item.payload.observer.as_ref() {
								observer.emit(TtsLifecyclePhase::PlaybackStarted, Some(true), None);
							}
							if let Some(config) = item.payload.lip_sync {
								if let Err(error) = avatar_motion.start_lip_sync(&wav, config) {
									log::warn!("《Speech LipSync》 {error}");
								}
							}
						}
						Ok(TtsOutcome { played, audio_path })
					}
					Err(error) => Err(error),
				}
			}
			Ok(Err(error)) => Err(error),
			Err(_) => Err(TtsError::Synthesis("TTS合成タスクが結果を返さず終了しました".into())),
		};
		let succeeded = result.is_ok();
		let played = result.as_ref().is_ok_and(|outcome| outcome.played);
		if !played {
			if let Some(guard) = item.payload.freshness.as_ref() {
				guard.cancel_pending();
			}
		}
		let failure = result.as_ref().err().map(ToString::to_string);
		let _ = item.payload.completion.send(result);

		if played {
			wait_until_audio_sink_empty(audio_sink.as_ref()).await;
			item.payload.playback_segment.completed();
		}
		drop(item.payload.playback_segment);
		if let Some(observer) = item.payload.observer.as_ref() {
			if succeeded {
				observer.emit(TtsLifecyclePhase::Completed, Some(played), None);
			} else {
				observer.emit(TtsLifecyclePhase::Failed, None, failure);
			}
		}
	}
}

fn enqueue_scheduled_item(queue: &mut FairSpeechQueue<ScheduledPayload>, item: SpeechItem<ScheduledPayload>) {
	let observer = item.payload.observer.clone();
	if let Some(dropped) = queue.push_bounded(item) {
		drop_scheduled_item(dropped, "max_pending_exceeded", queue.len());
	}
	if let Some(observer) = observer.as_ref() {
		observer.emit_queue(TtsLifecyclePhase::Queued, None, None, Some(queue.len()), Some(0));
	}
}

async fn wait_for_superseded(guard: Option<&crate::speech_playback::SpeechFreshness>) {
	match guard {
		Some(guard) => guard.cancelled().await,
		None => std::future::pending::<()>().await,
	}
}

fn drop_stopped_item(item: SpeechItem<ScheduledPayload>, queue_depth: usize, stage: &str) {
	let reason = item
		.payload
		.freshness
		.as_ref()
		.map_or("superseded_before_first_playback", |guard| guard.stop_reason());
	drop_scheduled_item_at_stage(item, reason, queue_depth, stage);
}

fn drop_scheduled_item(item: SpeechItem<ScheduledPayload>, reason: &str, queue_depth: usize) {
    drop_scheduled_item_at_stage(item, reason, queue_depth, "queue");
}

// Retain the cancellation reason independently of where the scheduler was waiting.
fn drop_scheduled_item_at_stage(item: SpeechItem<ScheduledPayload>, reason: &str, queue_depth: usize, stage: &str) {
	item.payload.synthesis_cancelled.store(true, Ordering::Release);
	let wait_ms = item.waiting_for(Instant::now()).as_millis().try_into().unwrap_or(u64::MAX);
	let message = format!(
		"Speech Schedulerが鮮度を失った発話を破棄しました: lane={} reason={} wait_ms={wait_ms} stage={stage} segment_index={}",
		item.lane.as_str(),
		reason,
		item.segment_index
	);
	if let Some(observer) = item.payload.observer.as_ref() {
		observer.emit_queue(
			TtsLifecyclePhase::Dropped,
			Some(false),
			Some(message.clone()),
			Some(queue_depth),
			Some(wait_ms),
		);
	}
	let _ = item.payload.completion.send(Err(TtsError::Playback(message)));
}

async fn wait_until_audio_sink_empty(audio_sink: Option<&crate::SharedAudioSink>) {
	let Some(audio_sink) = audio_sink else {
		return;
	};
	loop {
		let is_empty = audio_sink.lock().await.player.empty();
		if is_empty {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
}

#[cfg(test)]
#[path = "speech_scheduler_audio_probe.rs"]
mod audio_probe;

#[cfg(test)]
mod tests {
	#[test]
	fn lifecycle_preserves_registered_origin_through_cloned_observer() {
		let playback = crate::speech_playback::SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 57);
		let (channel, id) = guard.observation_origin();
		let (tx, mut rx) = tokio::sync::broadcast::channel(8);
		let observer = SpeechLifecycleObserver::new(tx.clone(), "speak".into(), "reply".into(), "test".into(), "test".into(), SpeechLane::Assistant, 12)
			.with_observation_origin(Some((channel.to_owned(), id)));
		observer.emit(TtsLifecyclePhase::Started, None, None);
		playback.observe_final("user", 58);
		assert!(guard.is_cancelled());
		observer.clone().emit(TtsLifecyclePhase::Dropped, Some(false), Some(guard.stop_reason().into()));
		for expected in [TtsLifecyclePhase::Started, TtsLifecyclePhase::Dropped] {
			let event = rx.try_recv().unwrap();
			if let ControlEvent::TtsLifecycle { phase, observation_channel, observation_datum_id, .. } = event {
				assert_eq!(phase, expected);
				assert_eq!(observation_channel.as_deref(), Some("user"));
				assert_eq!(observation_datum_id, Some(57));
			} else { panic!("Expected lifecycle event"); }
		}
		SpeechLifecycleObserver::new(tx, "chat".into(), String::new(), "test".into(), "test".into(), SpeechLane::Chat, 3)
			.emit(TtsLifecyclePhase::Started, None, None);
		let value = serde_json::to_value(rx.try_recv().unwrap()).unwrap();
		assert!(value.get("observation_channel").is_none());
		assert!(value.get("observation_datum_id").is_none());
	}

	use super::*;
	use async_trait::async_trait;
	use std::sync::atomic::AtomicUsize;

	fn item(lane: SpeechLane, id: &str) -> SpeechItem<&'static str> {
		SpeechItem::new(lane, id, 0, "")
	}

	#[test]
	fn assistant_yields_to_chat_after_configured_burst() {
		let mut queue = FairSpeechQueue::new(2);
		for id in ["a1", "a2", "a3", "a4"] {
			queue.push(item(SpeechLane::Assistant, id));
		}
		for id in ["c1", "c2"] {
			queue.push(item(SpeechLane::Chat, id));
		}

		let order = std::iter::from_fn(|| queue.pop_next().map(|entry| entry.utterance_id)).collect::<Vec<_>>();
		assert_eq!(order, ["a1", "a2", "c1", "a3", "a4", "c2"]);
	}

	#[test]
	fn chat_backlog_does_not_starve_assistant() {
		let mut queue = FairSpeechQueue::new(2);
		for id in ["c1", "c2", "c3", "c4"] {
			queue.push(item(SpeechLane::Chat, id));
		}
		queue.push(item(SpeechLane::Assistant, "a1"));

		assert_eq!(queue.pop_next().unwrap().utterance_id, "a1");
		assert_eq!(queue.pop_next().unwrap().utterance_id, "c1");
	}

	#[test]
	fn system_is_next_without_reordering_each_lane() {
		let mut queue = FairSpeechQueue::new(2);
		queue.push(item(SpeechLane::Assistant, "a1"));
		queue.push(item(SpeechLane::System, "s1"));
		queue.push(item(SpeechLane::Assistant, "a2"));
		queue.push(item(SpeechLane::System, "s2"));

		let order = std::iter::from_fn(|| queue.pop_next().map(|entry| entry.utterance_id)).collect::<Vec<_>>();
		assert_eq!(order, ["s1", "s2", "a1", "a2"]);
	}

	#[test]
	fn old_chat_is_reported_before_freshness_pruning() {
		let now = Instant::now();
		let mut queue = FairSpeechQueue::new(2);
		queue.push(item(SpeechLane::Chat, "old").with_enqueued_at(now - Duration::from_secs(30)));

		assert_eq!(queue.oldest_wait(now), Some(Duration::from_secs(30)));
		assert_eq!(queue.len(), 1);
		assert_eq!(queue.pop_next().unwrap().utterance_id, "old");
		assert!(queue.is_empty());
	}

	#[test]
	fn stale_chat_is_removed_while_assistant_is_preserved() {
		let now = Instant::now();
		let mut queue = FairSpeechQueue::new(2);
		queue.push(item(SpeechLane::Chat, "old-chat").with_enqueued_at(now - Duration::from_secs(31)));
		queue.push(item(SpeechLane::Assistant, "old-assistant").with_enqueued_at(now - Duration::from_secs(90)));

		let expired = queue.drain_expired(now);
		assert_eq!(expired.len(), 1);
		assert_eq!(expired[0].utterance_id, "old-chat");
		assert_eq!(queue.pop_next().unwrap().utterance_id, "old-assistant");
	}

	#[test]
	fn chat_pending_limit_discards_oldest_item() {
		let mut queue = FairSpeechQueue::new(2);
		let policy = SpeechQueuePolicy::with_overrides(SpeechLane::Chat, 0, 2);
		assert!(queue.push_bounded(item(SpeechLane::Chat, "c1").with_policy(policy)).is_none());
		assert!(queue.push_bounded(item(SpeechLane::Chat, "c2").with_policy(policy)).is_none());
		let dropped = queue.push_bounded(item(SpeechLane::Chat, "c3").with_policy(policy)).unwrap();
		assert_eq!(dropped.utterance_id, "c1");
		assert_eq!(queue.pop_next().unwrap().utterance_id, "c2");
		assert_eq!(queue.pop_next().unwrap().utterance_id, "c3");
	}

	#[test]
	fn negative_queue_overrides_are_explicitly_unlimited() {
		let policy = SpeechQueuePolicy::with_overrides(SpeechLane::Chat, -1, -1);
		assert_eq!(policy.max_wait, None);
		assert_eq!(policy.max_pending, None);
	}

	#[tokio::test]
	async fn dropping_queue_item_cancels_pending_synthesis() {
		let playback = crate::speech_playback::SpeechPlayback::default();
		let (_synthesis_tx, synthesis_rx) = oneshot::channel();
		let (completion_tx, completion_rx) = oneshot::channel();
		let cancelled = Arc::new(AtomicBool::new(false));
		let item = SpeechItem::new(
			SpeechLane::Chat,
			"cancel-chat",
			0,
			ScheduledPayload {
				text: "fixture".into(),
				freshness: None,
				speech_floor: None,
				playback_segment: playback.track_segment("cancel-chat"),
				synthesis: Some(synthesis_rx),
				synthesis_cancelled: cancelled.clone(),
				completion: completion_tx,
				observer: None,
				lip_sync: None,
			},
		);

		drop_scheduled_item(item, "fixture", 0);
		assert!(cancelled.load(Ordering::Acquire));
		assert!(completion_rx.await.unwrap().is_err());
		let error = playback.wait("cancel-chat", Duration::from_secs(30));
		assert!(tokio::time::timeout(Duration::from_secs(1), error).await.unwrap().is_err());
	}

	#[test]
	fn segment_metadata_and_payload_are_preserved() {
		let mut queue = FairSpeechQueue::new(2);
		queue.push(SpeechItem::new(SpeechLane::Assistant, "turn-1", 3, "第四片"));

		let item = queue.pop_next().unwrap();
		assert_eq!(item.lane, SpeechLane::Assistant);
		assert_eq!(item.utterance_id, "turn-1");
		assert_eq!(item.segment_index, 3);
		assert_eq!(item.payload, "第四片");
	}

	struct MockWavDriver {
		calls: Arc<AtomicUsize>,
	}

	struct FailingDriver;
	struct SlowWavDriver;

	#[async_trait]
	impl TtsDriver for MockWavDriver {
		fn name(&self) -> &'static str {
			"mock"
		}

		async fn synthesize(&self, _req: &TtsRequest) -> Result<TtsSynthesis, TtsError> {
			self.calls.fetch_add(1, Ordering::Relaxed);
			Ok(TtsSynthesis::Wav(vec![0, 1, 2]))
		}
	}

	#[async_trait]
	impl TtsDriver for FailingDriver {
		fn name(&self) -> &'static str {
			"failing"
		}

		async fn synthesize(&self, _req: &TtsRequest) -> Result<TtsSynthesis, TtsError> {
			Err(TtsError::Synthesis("fixture failure".into()))
		}
	}

	#[async_trait]
	impl TtsDriver for SlowWavDriver {
		fn name(&self) -> &'static str {
			"slow-mock"
		}

		async fn synthesize(&self, _req: &TtsRequest) -> Result<TtsSynthesis, TtsError> {
			tokio::time::sleep(Duration::from_millis(20)).await;
			Ok(TtsSynthesis::Wav(vec![0, 1, 2]))
		}
	}

	#[tokio::test]
	async fn superseded_speech_leaves_held_floor_and_rejects_late_segments() {
		let scheduler = SpeechScheduler::spawn(None, AvatarMotionController::spawn());
		let floor = Arc::new(crate::state::SpeechFloorManager::new());
		floor.set_held("human", true);
		let calls = Arc::new(AtomicUsize::new(0));
		let guard = scheduler.playback.register_freshness("old", "user".into(), 10);
		let request = TtsRequest {
			text: "test".into(),
			voice: String::new(),
			speed: 1.0,
			pitch: 0.0,
			volume: 1.0,
			endpoint: String::new(),
			save_path: String::new(),
			extra: Default::default(),
		};
		let pending = {
			let scheduler = scheduler.clone();
			let floor = floor.clone();
			let calls = calls.clone();
			let request = request.clone();
			tokio::spawn(async move {
				scheduler
					.speak_with_floor(
						SpeechLane::Assistant,
						"old".into(),
						0,
						SpeechQueuePolicy::for_lane(SpeechLane::Assistant),
						Arc::new(MockWavDriver { calls }),
						request,
						None,
						None,
						Some((floor, "human".into())),
					)
					.await
			})
		};
		tokio::time::timeout(Duration::from_secs(1), async {
			while calls.load(Ordering::Relaxed) == 0 {
				tokio::task::yield_now().await;
			}
		})
		.await
		.unwrap();
		assert!(!pending.is_finished());
		scheduler.playback.observe_final("user", 11);
		let result = tokio::time::timeout(Duration::from_secs(1), pending).await.unwrap().unwrap();
		assert!(matches!(result, Err(TtsError::Playback(message)) if message.contains("superseded_before_first_playback")));
		assert!(!guard.wait_for_delivery(Duration::ZERO).await);
		// A segment published after cancellation must not resurrect the reply.
		let late = tokio::time::timeout(
			Duration::from_secs(1),
			scheduler.speak_with_floor(
				SpeechLane::Assistant,
				"old".into(),
				1,
				SpeechQueuePolicy::for_lane(SpeechLane::Assistant),
				Arc::new(MockWavDriver { calls }),
				request,
				None,
				None,
				Some((floor, "human".into())),
			),
		)
		.await
		.unwrap();
		assert!(late.is_err());
	}

	#[tokio::test]
	async fn interrupted_continuation_releases_scheduler_while_human_keeps_speaking() {
		let scheduler = SpeechScheduler::spawn(None, AvatarMotionController::spawn());
		let floor = Arc::new(crate::state::SpeechFloorManager::new());
		floor.set_held("human", true);
		let calls = Arc::new(AtomicUsize::new(0));
		let guard = scheduler.playback.register_freshness("partial", "user".into(), 1);
		guard.set_continuation_gap_for_test(Duration::from_millis(40));
		assert!(guard.commit_playback());
		let mut first = scheduler.playback.track_segment("partial");
		first.started(0, "前半。".into());
		first.completed();
		drop(first);
		let request = TtsRequest {
			text: "後半。".into(),
			voice: String::new(),
			speed: 1.0,
			pitch: 0.0,
			volume: 1.0,
			endpoint: String::new(),
			save_path: String::new(),
			extra: Default::default(),
		};
		let result = tokio::time::timeout(
			Duration::from_secs(1),
			scheduler.speak_with_floor(
				SpeechLane::Assistant,
				"partial".into(),
				1,
				SpeechQueuePolicy::for_lane(SpeechLane::Assistant),
				Arc::new(MockWavDriver { calls: calls.clone() }),
				request.clone(),
				None,
				None,
				Some((floor.clone(), "human".into())),
			),
		)
		.await
		.unwrap();
		assert!(matches!(result, Err(TtsError::Playback(message)) if message.contains("continuation_gap_exceeded")));
		// The same held human floor must not strand unrelated chat behind the tail.
		let chat = tokio::time::timeout(
			Duration::from_secs(1),
			scheduler.speak_with_floor(
				SpeechLane::Chat,
				"chat".into(),
				0,
				SpeechQueuePolicy::for_lane(SpeechLane::Chat),
				Arc::new(MockWavDriver { calls }),
				request,
				None,
				None,
				None,
			),
		)
		.await
		.unwrap()
		.unwrap();
		assert!(!chat.played, "fixture has no audio sink");
		assert!(!guard.commit_playback());
	}

	#[tokio::test]
	async fn synthesized_segment_waits_for_human_before_delivery() {
		let scheduler = SpeechScheduler::spawn(None, AvatarMotionController::spawn());
		let floor = Arc::new(crate::state::SpeechFloorManager::new());
		floor.set_held("human", true);
		let calls = Arc::new(AtomicUsize::new(0));
		let request = TtsRequest {
			text: "test".into(),
			voice: String::new(),
			speed: 1.0,
			pitch: 0.0,
			volume: 1.0,
			endpoint: String::new(),
			save_path: String::new(),
			extra: Default::default(),
		};
		let task_floor = floor.clone();
		let task_calls = calls.clone();
		let task = tokio::spawn(async move {
			scheduler
				.speak_with_floor(
					SpeechLane::Assistant,
					"held".into(),
					0,
					SpeechQueuePolicy::for_lane(SpeechLane::Assistant),
					Arc::new(MockWavDriver { calls: task_calls }),
					request,
					None,
					None,
					Some((task_floor, "human".into())),
				)
				.await
		});
		tokio::time::timeout(Duration::from_secs(1), async {
			while calls.load(Ordering::Relaxed) == 0 {
				tokio::task::yield_now().await;
			}
		})
		.await
		.unwrap();
		tokio::time::sleep(Duration::from_millis(20)).await;
		assert!(
			!task.is_finished(),
			"synthesized audio must not be delivered while human holds the floor"
		);
		floor.set_held("human", false);
		let outcome = tokio::time::timeout(Duration::from_secs(1), task).await.unwrap().unwrap().unwrap();
		assert!(!outcome.played, "fixture has no audio sink");
	}

	#[tokio::test]
	async fn failed_synthesis_does_not_wait_for_human() {
		let scheduler = SpeechScheduler::spawn(None, AvatarMotionController::spawn());
		let floor = Arc::new(crate::state::SpeechFloorManager::new());
		floor.set_held("human", true);
		let request = TtsRequest {
			text: "test".into(),
			voice: String::new(),
			speed: 1.0,
			pitch: 0.0,
			volume: 1.0,
			endpoint: String::new(),
			save_path: String::new(),
			extra: Default::default(),
		};
		let result = tokio::time::timeout(
			Duration::from_secs(1),
			scheduler.speak_with_floor(
				SpeechLane::Assistant,
				"failed-held".into(),
				0,
				SpeechQueuePolicy::for_lane(SpeechLane::Assistant),
				Arc::new(FailingDriver),
				request,
				None,
				None,
				Some((floor, "human".into())),
			),
		)
		.await
		.expect("synthesis failure must complete even while human holds the floor");
		assert!(matches!(result, Err(TtsError::Synthesis(message)) if message == "fixture failure"));
	}

	#[tokio::test]
	async fn waiting_for_human_respects_queue_expiry() {
		let scheduler = SpeechScheduler::spawn(None, AvatarMotionController::spawn());
		let floor = Arc::new(crate::state::SpeechFloorManager::new());
		floor.set_held("human", true);
		let calls = Arc::new(AtomicUsize::new(0));
		let request = TtsRequest {
			text: "test".into(),
			voice: String::new(),
			speed: 1.0,
			pitch: 0.0,
			volume: 1.0,
			endpoint: String::new(),
			save_path: String::new(),
			extra: Default::default(),
		};
		let result = tokio::time::timeout(
			Duration::from_secs(2),
			scheduler.speak_with_floor(
				SpeechLane::Assistant,
				"expired-held".into(),
				0,
				SpeechQueuePolicy::with_overrides(SpeechLane::Assistant, 100, 0),
				Arc::new(MockWavDriver { calls: calls.clone() }),
				request,
				None,
				None,
				Some((floor, "human".into())),
			),
		)
		.await
		.expect("queue expiry must complete without waiting for floor release");
		assert_eq!(calls.load(Ordering::Relaxed), 1);
		assert!(matches!(result, Err(TtsError::Playback(message)) if message.contains("max_queue_wait_exceeded_waiting_for_floor") && message.contains("stage=speech_floor") && message.contains("segment_index=0")));
	}

	#[tokio::test]
	async fn scheduler_synthesizes_without_audio_sink() {
		let scheduler = SpeechScheduler::spawn(None, AvatarMotionController::spawn());
		let calls = Arc::new(AtomicUsize::new(0));
		let request = TtsRequest {
			text: "テスト".into(),
			voice: String::new(),
			speed: 1.0,
			pitch: 0.0,
			volume: 1.0,
			endpoint: String::new(),
			save_path: String::new(),
			extra: Default::default(),
		};

		let outcome = scheduler
			.speak(
				SpeechLane::Assistant,
				"turn-1".into(),
				0,
				SpeechQueuePolicy::for_lane(SpeechLane::Assistant),
				Arc::new(MockWavDriver { calls: calls.clone() }),
				request,
				None,
				None,
			)
			.await
			.unwrap();

		assert!(!outcome.played);
		assert_eq!(calls.load(Ordering::Relaxed), 1);
		assert!(scheduler.playback.wait("turn-1", Duration::ZERO).await.is_err());
	}

	#[tokio::test]
	async fn lifecycle_without_audio_sink_does_not_report_playback_started() {
		let scheduler = SpeechScheduler::spawn(None, AvatarMotionController::spawn());
		let calls = Arc::new(AtomicUsize::new(0));
		let (tx, mut rx) = tokio::sync::broadcast::channel(16);
		let observer = SpeechLifecycleObserver::new(
			tx,
			"tts".into(),
			"test-utterance".into(),
			"mock".into(),
			"voice".into(),
			SpeechLane::Assistant,
			3,
		);
		observer.emit(TtsLifecyclePhase::Started, None, None);
		let request = TtsRequest {
			text: "abc".into(),
			voice: "voice".into(),
			speed: 1.0,
			pitch: 0.0,
			volume: 1.0,
			endpoint: String::new(),
			save_path: String::new(),
			extra: Default::default(),
		};

		scheduler
			.speak(
				SpeechLane::Assistant,
				"turn-1".into(),
				0,
				SpeechQueuePolicy::for_lane(SpeechLane::Assistant),
				Arc::new(MockWavDriver { calls }),
				request,
				None,
				Some(observer),
			)
			.await
			.unwrap();

		let mut phases = Vec::new();
		while phases.last() != Some(&TtsLifecyclePhase::Completed) {
			let event = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await.unwrap().unwrap();
			if let ControlEvent::TtsLifecycle { phase, .. } = event {
				phases.push(phase);
			}
		}
		assert_eq!(
			phases,
			[
				TtsLifecyclePhase::Started,
				TtsLifecyclePhase::Queued,
				TtsLifecyclePhase::Synthesized,
				TtsLifecyclePhase::Completed,
			]
		);
	}

	#[tokio::test]
	async fn lifecycle_reports_synthesis_failure() {
		let scheduler = SpeechScheduler::spawn(None, AvatarMotionController::spawn());
		let (tx, mut rx) = tokio::sync::broadcast::channel(16);
		let observer = SpeechLifecycleObserver::new(
			tx,
			"tts".into(),
			"test-utterance".into(),
			"failing".into(),
			"voice".into(),
			SpeechLane::Assistant,
			3,
		);
		let request = TtsRequest {
			text: "abc".into(),
			voice: "voice".into(),
			speed: 1.0,
			pitch: 0.0,
			volume: 1.0,
			endpoint: String::new(),
			save_path: String::new(),
			extra: Default::default(),
		};

		let result = scheduler
			.speak(
				SpeechLane::Assistant,
				"turn-failure".into(),
				0,
				SpeechQueuePolicy::for_lane(SpeechLane::Assistant),
				Arc::new(FailingDriver),
				request,
				None,
				Some(observer),
			)
			.await;
		assert!(result.is_err());

		let mut failure = None;
		while let Ok(event) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
			match event.unwrap() {
				ControlEvent::TtsLifecycle {
					phase: TtsLifecyclePhase::Failed,
					error,
					..
				} => {
					failure = error;
					break;
				}
				_ => {}
			}
		}
		assert!(failure.as_deref().is_some_and(|error| error.contains("fixture failure")));
	}

	#[tokio::test]
	async fn stale_chat_is_dropped_after_synthesis_without_playback() {
		let scheduler = SpeechScheduler::spawn(None, AvatarMotionController::spawn());
		let (tx, mut rx) = tokio::sync::broadcast::channel(16);
		let observer = SpeechLifecycleObserver::new(
			tx,
			"tts".into(),
			"stale-chat".into(),
			"slow-mock".into(),
			"voice".into(),
			SpeechLane::Chat,
			3,
		);
		let request = TtsRequest {
			text: "abc".into(),
			voice: "voice".into(),
			speed: 1.0,
			pitch: 0.0,
			volume: 1.0,
			endpoint: String::new(),
			save_path: String::new(),
			extra: Default::default(),
		};

		let result = scheduler
			.speak(
				SpeechLane::Chat,
				"stale-chat".into(),
				0,
				SpeechQueuePolicy::with_overrides(SpeechLane::Chat, 1, -1),
				Arc::new(SlowWavDriver),
				request,
				None,
				Some(observer),
			)
			.await;
		assert!(result.is_err());

		let mut dropped = None;
		while let Ok(event) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
			if let ControlEvent::TtsLifecycle {
				phase: TtsLifecyclePhase::Dropped,
				lane,
				wait_ms,
				error,
				..
			} = event.unwrap()
			{
				dropped = Some((lane, wait_ms, error));
				break;
			}
		}
		let (lane, wait_ms, error) = dropped.expect("dropped lifecycle event");
		assert_eq!(lane, "chat");
		assert!(wait_ms.is_some_and(|wait| wait >= 1));
		assert!(error.as_deref().is_some_and(|message| message.contains("after_synthesis")));
	}
}

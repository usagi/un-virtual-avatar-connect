//! Bounded utterance-local playback state for speech-accompanying avatar output.
use std::{
	collections::VecDeque,
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
	Waiting,
	Playing,
	Ended,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SpeechPlayback {
	entries: Arc<Mutex<VecDeque<(String, usize, watch::Sender<Phase>)>>>,
	freshness: Arc<Mutex<VecDeque<(String, SpeechFreshness)>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryState {
	Pending,
	Committed,
	Cancelled,
	Undelivered,
	Interrupted,
}

/// Shared by all segments of one ordinary reply. Once playback is committed,
/// later observations cannot remove the rest of the sentence.
#[derive(Debug, Clone)]
pub(crate) struct SpeechFreshness {
	channel: String,
	observation_id: u64,
	state: watch::Sender<DeliveryState>,
	delivery: Arc<Mutex<DeliveryProgress>>,
	delivery_changed: watch::Sender<u64>,
}

/// Only actual playback creates an entry. Queuing, synthesis and generated
/// text do not establish that the listener heard a segment.
#[derive(Debug, Default)]
struct DeliveryProgress {
	segments: Vec<(String, bool)>,
	expected_text: Option<String>,
	expected_segments: Option<usize>,
	settled_segments: usize,
	playing: Option<usize>,
	idle_since: Option<Instant>,
	gap_limit: Option<Duration>,
}

impl DeliveryProgress {
	fn started(&mut self, text: String) -> usize {
		let receipt = self.segments.len();
		self.segments.push((text, false));
		self.playing = Some(receipt);
		self.idle_since = None;
		receipt
	}

	fn finished(&mut self, index: usize, now: Instant) {
		if self.playing != Some(index) {
			return;
		}
		if let Some((_, completed)) = self.segments.get_mut(index) {
			*completed = true;
		}
		self.playing = None;
		self.idle_since = Some(now);
	}

	fn started_text(&self) -> String {
		self.segments.iter().map(|(text, _)| text.as_str()).collect()
	}
}

impl SpeechFreshness {
	pub(crate) fn observation_origin(&self) -> (&str, u64) {
		(&self.channel, self.observation_id)
	}

	pub(crate) fn finish_generation(&self, segments: usize) {
		self.delivery.lock().unwrap().expected_segments = Some(segments);
		self.delivery_changed.send_modify(|revision| *revision = revision.wrapping_add(1));
	}

	pub(crate) fn delivery_updates(&self) -> watch::Receiver<u64> {
		self.delivery_changed.subscribe()
	}

	pub(crate) fn delivery_finished(&self) -> bool {
		if self.is_cancelled() {
			return true;
		}
		let delivery = self.delivery.lock().unwrap();
		delivery
			.expected_segments
			.is_some_and(|expected| delivery.settled_segments >= expected)
	}

	pub(crate) fn expect_text(&self, text: String) {
		self.delivery.lock().unwrap().expected_text = Some(text);
		self.delivery_changed.send_modify(|revision| *revision = revision.wrapping_add(1));
	}

	/// The model's complete answer is not evidence of delivery. Compare against
	/// actual sink submissions, even when an external graph rewrites TTS text.
	pub(crate) fn delivered_text(&self) -> (String, bool) {
		let delivery = self.delivery.lock().unwrap();
		let text = delivery.started_text();
		let complete = delivery.expected_text.as_ref().is_some_and(|expected| expected == &text);
		(text, complete)
	}

	pub(crate) fn set_continuation_gap(&self, limit: Option<Duration>) {
		self.delivery.lock().unwrap().gap_limit = limit;
		self.delivery_changed.send_modify(|revision| *revision = revision.wrapping_add(1));
	}
	#[cfg(test)]
	pub(crate) fn set_continuation_gap_for_test(&self, limit: Duration) {
		self.set_continuation_gap(Some(limit));
	}
	pub fn is_committed(&self) -> bool {
		matches!(*self.state.borrow(), DeliveryState::Committed | DeliveryState::Interrupted)
	}
	fn continuation_deadline(&self) -> Option<Instant> {
		let delivery = self.delivery.lock().unwrap();
		if delivery.playing.is_some() {
			return None;
		}
		delivery.idle_since?.checked_add(delivery.gap_limit?)
	}
	fn expire_continuation(&self) {
		let delivery = self.delivery.lock().unwrap();
		self.expire_delivery(&delivery);
	}
	fn expire_delivery(&self, delivery: &DeliveryProgress) {
		if delivery.playing.is_some()
			|| !delivery
				.idle_since
				.zip(delivery.gap_limit)
				.is_some_and(|(since, limit)| since.elapsed() >= limit)
		{
			return;
		}
		self.state.send_if_modified(|state| {
			if *state != DeliveryState::Committed {
				return false;
			}
			*state = DeliveryState::Interrupted;
			true
		});
	}
	pub fn stop_reason(&self) -> &'static str {
		match *self.state.borrow() {
			DeliveryState::Interrupted => "continuation_gap_exceeded",
			DeliveryState::Undelivered => "not_delivered_before_first_playback",
			_ => "superseded_before_first_playback",
		}
	}
	pub fn was_superseded(&self) -> bool {
		*self.state.borrow() == DeliveryState::Cancelled
	}
	pub fn cancel_pending(&self) {
		self.state.send_if_modified(|state| {
			if *state != DeliveryState::Pending {
				return false;
			}
			*state = DeliveryState::Undelivered;
			true
		});
	}
	/// Final text and episode memory must not claim an undelivered reply.
	pub async fn wait_for_delivery(&self, timeout: Duration) -> bool {
		let mut receiver = self.state.subscribe();
		if tokio::time::timeout(timeout, receiver.wait_for(|state| *state != DeliveryState::Pending))
			.await
			.is_err()
		{
			self.cancel_pending();
		}
		self.is_committed()
	}
	fn supersede(&self, channel: &str, observation_id: u64) {
		if self.channel != channel || observation_id <= self.observation_id {
			return;
		}
		self.state.send_if_modified(|state| {
			if *state != DeliveryState::Pending {
				return false;
			}
			*state = DeliveryState::Cancelled;
			true
		});
	}
	pub fn is_cancelled(&self) -> bool {
		self.expire_continuation();
		matches!(*self.state.borrow(), DeliveryState::Cancelled | DeliveryState::Undelivered | DeliveryState::Interrupted)
	}
	pub fn commit_playback(&self) -> bool {
		let mut delivery = self.delivery.lock().unwrap();
		self.expire_delivery(&delivery);
		let mut accepted = false;
		self.state.send_if_modified(|state| {
			accepted = matches!(*state, DeliveryState::Pending | DeliveryState::Committed);
			if *state != DeliveryState::Pending {
				return false;
			}
			*state = DeliveryState::Committed;
			true
		});
		if accepted {
			// Reserve delivery while the sink accepts this segment. A deadline
			// waiter must not interrupt between acceptance and the started hook.
			delivery.idle_since = None;
		}
		accepted
	}
	pub async fn cancelled(&self) {
		let mut receiver = self.state.subscribe();
		let mut delivery = self.delivery_changed.subscribe();
		loop {
			receiver.borrow_and_update();
			delivery.borrow_and_update();
			if self.is_cancelled() {
				return;
			}
			let deadline = self.continuation_deadline();
			tokio::select! {
				_ = receiver.changed() => {},
				_ = delivery.changed() => {},
				_ = async {
					match deadline {
						Some(at) => tokio::time::sleep_until(at.into()).await,
						None => std::future::pending::<()>().await,
					}
				} => {},
			}
		}
	}
}

/// Owned by the queued segment, not by its potentially cancelled caller.
pub(crate) struct SpeechPlaybackSegment {
	state: SpeechPlayback,
	id: String,
	freshness: Option<SpeechFreshness>,
	started_index: Option<usize>,
}

impl SpeechPlaybackSegment {
	pub fn started(&mut self, index: u32, text: String) {
		if let Some(guard) = self.freshness.as_ref() {
			self.started_index = Some(guard.delivery.lock().unwrap().started(text));
			guard.delivery_changed.send_modify(|revision| *revision = revision.wrapping_add(1));
		}
		log::trace!(
			"《Speech Delivery》 utterance={} source_segment={index} receipt={:?}",
			self.id,
			self.started_index
		);
		self.state.started(&self.id);
	}

	/// Called after the sink drained normally, never for a queued/dropped item.
	pub fn completed(&mut self) {
		if let (Some(guard), Some(index)) = (self.freshness.as_ref(), self.started_index.take()) {
			let mut delivery = guard.delivery.lock().unwrap();
			delivery.finished(index, Instant::now());
			log::debug!(
				"《Speech Delivery》 utterance={} started_chars={} completed_segments={} idle={}",
				self.id,
				delivery.started_text().chars().count(),
				delivery.segments.iter().filter(|(_, completed)| *completed).count(),
				delivery.idle_since.is_some(),
			);
			drop(delivery);
			guard.delivery_changed.send_modify(|revision| *revision = revision.wrapping_add(1));
		}
	}
}

impl Drop for SpeechPlaybackSegment {
	fn drop(&mut self) {
		if let Some(guard) = self.freshness.as_ref() {
			guard.delivery.lock().unwrap().settled_segments += 1;
			guard.delivery_changed.send_modify(|revision| *revision = revision.wrapping_add(1));
		}
		self.state.ended(&self.id);
	}
}

impl SpeechPlayback {
	pub fn register_freshness(&self, id: &str, channel: String, observation_id: u64) -> SpeechFreshness {
		let (state, _) = watch::channel(DeliveryState::Pending);
		let guard = SpeechFreshness {
			channel,
			observation_id,
			state,
			delivery: Arc::new(Mutex::new(DeliveryProgress::default())),
			delivery_changed: watch::channel(0).0,
		};
		let mut entries = self.freshness.lock().unwrap();
		while entries.len() >= 128 {
			if let Some((_, old)) = entries.pop_front() {
				old.cancel_pending();
			}
		}
		entries.push_back((id.to_owned(), guard.clone()));
		guard
	}
	pub fn freshness(&self, id: &str) -> Option<SpeechFreshness> {
		self.freshness
			.lock()
			.unwrap()
			.iter()
			.find(|(key, _)| key == id)
			.map(|(_, guard)| guard.clone())
	}
	pub fn observe_final(&self, channel: &str, observation_id: u64) {
		for (id, guard) in self.freshness.lock().unwrap().iter() {
			guard.supersede(channel, observation_id);
			if guard.is_cancelled() {
				// Cancel existing waiters without resurrecting old cache entries.
				// Otherwise stale replies can evict a committed, still-playing reply.
				// Late segment registration separately checks the freshness guard.
				if let Some((_, _, tx)) = self.entries.lock().unwrap().iter().find(|(key, _, _)| key == id) {
					tx.send_replace(Phase::Ended);
				}
			}
		}
	}
	pub fn track_segment(&self, id: &str) -> SpeechPlaybackSegment {
		self.queued(id);
		SpeechPlaybackSegment {
			state: self.clone(),
			id: id.to_owned(),
			freshness: self.freshness(id),
			started_index: None,
		}
	}
	fn sender(&self, id: &str) -> watch::Sender<Phase> {
		let mut entries = self.entries.lock().unwrap();
		if let Some((_, _, tx)) = entries.iter().find(|(key, _, _)| key == id) {
			return tx.clone();
		}
		while entries.len() >= 128 {
			if let Some((_, _, tx)) = entries.pop_front() {
				tx.send_replace(Phase::Ended);
			}
		}
		let (tx, _) = watch::channel(Phase::Waiting);
		entries.push_back((id.to_owned(), 0, tx.clone()));
		tx
	}

	pub fn started(&self, id: &str) {
		self.sender(id).send_replace(Phase::Playing);
	}
	pub fn queued(&self, id: &str) {
		let cancelled = self.freshness(id).is_some_and(|guard| guard.is_cancelled());
		self.sender(id);
		let mut entries = self.entries.lock().unwrap();
		if let Some((_, count, tx)) = entries.iter_mut().find(|(key, _, _)| key == id) {
			*count += 1;
			if cancelled {
				tx.send_replace(Phase::Ended);
			} else if *tx.borrow() != Phase::Playing {
				tx.send_replace(Phase::Waiting);
			}
		}
	}
	pub fn ended(&self, id: &str) {
		let cancelled = self.freshness(id).is_some_and(|guard| guard.is_cancelled());
		self.sender(id);
		let mut entries = self.entries.lock().unwrap();
		if let Some((_, count, tx)) = entries.iter_mut().find(|(key, _, _)| key == id) {
			*count = count.saturating_sub(1);
			tx.send_replace(if cancelled || *count == 0 { Phase::Ended } else { Phase::Waiting });
		}
	}

	pub async fn wait(&self, id: &str, timeout: Duration) -> Result<(), String> {
		let mut rx = self.sender(id).subscribe();
		let freshness = self.freshness(id);
		let mut delivery_updates = freshness.as_ref().map(SpeechFreshness::delivery_updates);
		tokio::time::timeout(timeout, async {
			loop {
				if let Some(updates) = delivery_updates.as_mut() {
					updates.borrow_and_update();
				}
				if freshness.as_ref().is_some_and(SpeechFreshness::is_cancelled) {
					return Err("speech was cancelled before avatar delivery".into());
				}
				match *rx.borrow_and_update() {
					Phase::Playing => return Ok(()),
					// A generated tail may not have reached the scheduler yet. Only
					// release the gesture on actual playback, and keep waiting through
					// this gap until every generated segment settles or is cancelled.
					Phase::Ended if freshness.as_ref().is_none_or(SpeechFreshness::delivery_finished) => {
						return Err("speech ended, failed, or was discarded before avatar delivery".into());
					}
					Phase::Ended => {}
					Phase::Waiting => {}
				}
				tokio::select! {
					result = rx.changed() => result.map_err(|_| "speech playback tracking closed".to_string())?,
					_ = async {
						match delivery_updates.as_mut() {
							Some(updates) => { let _ = updates.changed().await; }
							None => std::future::pending::<()>().await,
						}
					} => {},
					_ = async {
						match freshness.as_ref() {
							Some(guard) => guard.cancelled().await,
							None => std::future::pending::<()>().await,
						}
					} => {},
				}
			}
		})
		.await
		.map_err(|_| "speech playback did not start before avatar deadline".to_string())?
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn newer_input_does_not_resurrect_evicted_cancelled_entries_and_evict_live_reply() {
		let playback = SpeechPlayback::default();
		for index in 0..127 {
			let id = format!("old-{index}");
			let guard = playback.register_freshness(&id, "user".into(), index);
			playback.queued(&id);
			guard.cancel_pending();
		}
		let live = playback.register_freshness("live", "user".into(), 127);
		playback.queued("live");
		assert!(live.commit_playback());
		let phase = playback.sender("live").subscribe();
		// A chat utterance shares the bounded phase cache but has no AI freshness.
		playback.queued("chat");
		playback.observe_final("user", 128);
		assert!(!live.is_cancelled());
		assert_eq!(*phase.borrow(), Phase::Waiting, "a committed reply must retain its playback waiter");
		playback.started("live");
		assert_eq!(*phase.borrow(), Phase::Playing);
	}

	#[test]
	fn delivery_failure_and_supersession_keep_distinct_first_terminal_causes() {
		let playback = SpeechPlayback::default();
		let failed = playback.register_freshness("failed", "user".into(), 1);
		failed.cancel_pending();
		failed.supersede("user", 2);
		assert!(failed.is_cancelled());
		assert!(!failed.was_superseded());
		assert!(!failed.commit_playback());
		assert_eq!(failed.stop_reason(), "not_delivered_before_first_playback");
		let superseded = playback.register_freshness("superseded", "user".into(), 2);
		superseded.supersede("user", 3);
		superseded.cancel_pending();
		assert!(superseded.was_superseded());
		assert!(!superseded.commit_playback());
		assert_eq!(superseded.stop_reason(), "superseded_before_first_playback");
	}

	#[tokio::test]
	async fn avatar_wait_survives_gap_before_next_generated_segment_is_tracked() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		guard.finish_generation(2);
		assert!(guard.commit_playback());
		let mut first = playback.track_segment("reply");
		first.started(0, "前半。".into());
		first.completed();
		drop(first);
		let waiting = playback.clone();
		let task = tokio::spawn(async move { waiting.wait("reply", Duration::from_secs(1)).await });
		tokio::task::yield_now().await;
		assert!(!task.is_finished(), "a segment boundary is not the end of the generated reply");
		let mut second = playback.track_segment("reply");
		assert!(guard.commit_playback());
		second.started(1, "後半。".into());
		assert!(task.await.unwrap().is_ok());
		second.completed();
		drop(second);
		assert!(playback.wait("reply", Duration::from_secs(1)).await.is_err(), "no late expression after the whole reply");
	}

	#[tokio::test]
	async fn avatar_gap_wait_ends_when_generation_finishes_without_a_tail() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		assert!(guard.commit_playback());
		let mut first = playback.track_segment("reply");
		first.started(0, "前半。".into());
		first.completed();
		drop(first);
		let waiting = playback.clone();
		let task = tokio::spawn(async move { waiting.wait("reply", Duration::from_secs(30)).await });
		tokio::task::yield_now().await;
		assert!(!task.is_finished());
		guard.finish_generation(1);
		assert!(tokio::time::timeout(Duration::from_secs(1), task).await.unwrap().unwrap().is_err());
	}

	#[tokio::test]
	async fn avatar_gap_wait_honors_continuation_expiry() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		guard.finish_generation(2);
		guard.set_continuation_gap(Some(Duration::from_millis(20)));
		assert!(guard.commit_playback());
		let mut first = playback.track_segment("reply");
		first.started(0, "前半。".into());
		first.completed();
		drop(first);
		let result = tokio::time::timeout(Duration::from_secs(1), playback.wait("reply", Duration::from_secs(30)))
			.await.unwrap();
		assert!(result.is_err());
		assert_eq!(guard.stop_reason(), "continuation_gap_exceeded");
	}

	#[tokio::test]
	async fn partial_delivery_settles_after_failed_tail_without_inventing_its_text() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		guard.expect_text("前半。未再生の後半。".into());
		guard.finish_generation(2);
		let mut updates = guard.delivery_updates();
		assert!(!guard.delivery_finished());
		let mut first = playback.track_segment("reply");
		let tail = playback.track_segment("reply");
		assert!(guard.commit_playback());
		first.started(0, "前半。".into());
		first.completed();
		drop(first);
		updates.borrow_and_update();
		assert!(!guard.delivery_finished(), "the synthesized or queued tail is still pending");
		drop(tail);
		tokio::time::timeout(Duration::from_secs(1), updates.changed())
			.await
			.unwrap()
			.unwrap();
		assert!(guard.delivery_finished());
		assert_eq!(guard.delivered_text(), ("前半。".into(), false));
	}

	#[test]
	fn generation_completion_does_not_wait_for_a_segment_that_already_settled() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		guard.expect_text("早い応答。".into());
		assert!(guard.commit_playback());
		let mut segment = playback.track_segment("reply");
		segment.started(0, "早い応答。".into());
		segment.completed();
		drop(segment);
		assert!(!guard.delivery_finished(), "generation can still produce another segment");
		guard.finish_generation(1);
		assert!(guard.delivery_finished());
		assert_eq!(guard.delivered_text(), ("早い応答。".into(), true));
	}
	#[test]
	fn repeated_default_segment_index_keeps_actual_playback_order() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		let mut first = playback.track_segment("reply");
		let mut second = playback.track_segment("reply");
		first.started(0, "前半。".into());
		first.completed();
		second.started(0, "後半。".into());
		first.completed();
		assert!(guard.delivery.lock().unwrap().idle_since.is_none());
		second.completed();
		let delivery = guard.delivery.lock().unwrap();
		assert_eq!(delivery.started_text(), "前半。後半。");
		assert_eq!(delivery.segments.len(), 2);
		assert!(delivery.segments.iter().all(|(_, done)| *done));
	}
	#[tokio::test]
	async fn expired_continuation_keeps_played_prefix_and_rejects_late_tail() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		guard.set_continuation_gap_for_test(Duration::from_millis(5));
		assert!(guard.commit_playback());
		let mut first = playback.track_segment("reply");
		first.started(0, "前半。".into());
		assert!(tokio::time::timeout(Duration::from_millis(15), guard.cancelled()).await.is_err());
		first.completed();
		tokio::time::timeout(Duration::from_secs(1), guard.cancelled()).await.unwrap();
		assert_eq!(guard.stop_reason(), "continuation_gap_exceeded");
		assert!(guard.is_committed(), "interruption does not erase already spoken text");
		assert_eq!(guard.delivery.lock().unwrap().started_text(), "前半。");
		assert!(!guard.commit_playback(), "a late continuation cannot resurrect the reply");
	}

	#[test]
	fn short_gap_is_reserved_atomically_at_acceptance() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		guard.set_continuation_gap_for_test(Duration::from_secs(10));
		assert!(guard.commit_playback());
		{
			let mut delivery = guard.delivery.lock().unwrap();
			delivery.started("前半。".into());
			delivery.finished(0, Instant::now() - Duration::from_secs(2));
		}
		assert!(guard.commit_playback());
		assert!(guard.continuation_deadline().is_none());
		assert!(!guard.is_cancelled());
	}

	#[test]
	fn continuation_expiry_does_not_apply_without_explicit_policy() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		assert!(guard.commit_playback());
		{
			let mut delivery = guard.delivery.lock().unwrap();
			delivery.started("前半。".into());
			delivery.finished(0, Instant::now() - Duration::from_secs(42));
		}
		assert!(!guard.is_cancelled());
		assert!(guard.commit_playback());
		assert!(playback.freshness("priority").is_none());
	}
	#[test]
	fn delivery_records_only_played_prefix_and_normal_completion() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		let mut first = playback.track_segment("reply");
		let tail = playback.track_segment("reply");
		assert!(guard.delivery.lock().unwrap().started_text().is_empty());
		first.started(0, "前半。".into());
		assert_eq!(guard.delivery.lock().unwrap().started_text(), "前半。");
		assert!(guard.delivery.lock().unwrap().idle_since.is_none());
		first.completed();
		let ended_at = guard.delivery.lock().unwrap().idle_since.unwrap();
		drop(tail);
		let delivery = guard.delivery.lock().unwrap();
		assert_eq!(delivery.started_text(), "前半。", "discarded tail must not become spoken history");
		assert_eq!(
			delivery.idle_since,
			Some(ended_at),
			"dropping an unplayed item must not restart the gap clock"
		);
		assert!(delivery.segments[0].1);
	}

	#[test]
	fn long_audio_is_not_a_continuation_gap() {
		let mut delivery = DeliveryProgress::default();
		let base = Instant::now();
		delivery.started("長い前半。".into());
		assert!(delivery.idle_since.is_none());
		delivery.finished(0, base + Duration::from_secs(40));
		assert_eq!(delivery.idle_since, Some(base + Duration::from_secs(40)));
		delivery.started("続き。".into());
		assert!(delivery.idle_since.is_none());
		delivery.finished(0, base + Duration::from_secs(41));
		assert!(
			delivery.idle_since.is_none(),
			"stale completion must not mark the next segment idle"
		);
		delivery.finished(1, base + Duration::from_secs(45));
		assert_eq!(delivery.started_text(), "長い前半。続き。");
	}

	#[test]
	fn dropping_active_segment_does_not_claim_normal_completion() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 1);
		let mut segment = playback.track_segment("reply");
		segment.started(0, "発話中".into());
		drop(segment);
		let delivery = guard.delivery.lock().unwrap();
		assert!(!delivery.segments[0].1);
		assert!(delivery.idle_since.is_none());
	}

	#[tokio::test]
	async fn delivery_timeout_cancels_only_unstarted_speech() {
		let playback = SpeechPlayback::default();
		let pending = playback.register_freshness("pending", "user".into(), 1);
		assert!(!pending.wait_for_delivery(Duration::ZERO).await);
		assert!(!pending.commit_playback());
		let started = playback.register_freshness("started", "user".into(), 1);
		assert!(started.commit_playback());
		assert!(started.wait_for_delivery(Duration::ZERO).await);
		started.cancel_pending();
		assert!(started.is_committed());
	}
	#[tokio::test]
	async fn superseded_unstarted_reply_cancels_all_segments_and_avatar_wait() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 10);
		let first = playback.track_segment("reply");
		let second = playback.track_segment("reply");
		playback.observe_final("user", 11);
		assert!(guard.is_cancelled());
		assert!(!guard.commit_playback());
		let late = playback.track_segment("reply");
		drop(first);
		assert!(playback.wait("reply", Duration::from_secs(1)).await.is_err());
		tokio::time::timeout(Duration::from_secs(1), guard.cancelled()).await.unwrap();
		drop((second, late));
	}
	#[test]
	fn newer_input_preserves_committed_reply_and_unregistered_priority() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("ordinary", "user".into(), 10);
		assert!(guard.commit_playback());
		playback.observe_final("user", 11);
		assert!(!guard.is_cancelled());
		assert!(guard.commit_playback(), "remaining segments stay eligible");
		assert!(playback.freshness("priority").is_none());
	}
	#[test]
	fn unrelated_or_older_input_does_not_cancel_reply() {
		let playback = SpeechPlayback::default();
		let guard = playback.register_freshness("reply", "user".into(), 10);
		playback.observe_final("obs:game_frame", 11);
		playback.observe_final("user", 10);
		playback.observe_final("user", 9);
		assert!(!guard.is_cancelled());
	}
	#[tokio::test]
	async fn waits_for_own_utterance_and_remembers_early_start() {
		let state = SpeechPlayback::default();
		state.started("other");
		assert!(state.wait("mine", Duration::ZERO).await.is_err());
		state.started("mine");
		assert!(state.wait("mine", Duration::ZERO).await.is_ok());
	}
	#[tokio::test]
	async fn delayed_start_wakes_waiter() {
		let state = SpeechPlayback::default();
		let task_state = state.clone();
		let task = tokio::spawn(async move { task_state.wait("mine", Duration::from_secs(1)).await });
		tokio::task::yield_now().await;
		state.started("mine");
		assert!(task.await.unwrap().is_ok());
	}
	#[tokio::test]
	async fn failure_wakes_waiter_without_waiting_for_deadline() {
		let state = SpeechPlayback::default();
		state.queued("mine");
		let task_state = state.clone();
		let task = tokio::spawn(async move { task_state.wait("mine", Duration::from_secs(30)).await });
		tokio::task::yield_now().await;
		state.ended("mine");
		assert!(tokio::time::timeout(Duration::from_secs(1), task).await.unwrap().unwrap().is_err());
	}
	#[tokio::test]
	async fn finished_or_failed_speech_cannot_release_a_late_gesture() {
		let state = SpeechPlayback::default();
		state.started("old");
		state.ended("old");
		state.ended("failed");
		state.started("new");
		assert!(state.wait("old", Duration::ZERO).await.is_err());
		assert!(state.wait("failed", Duration::ZERO).await.is_err());
	}
	#[tokio::test]
	async fn eviction_closes_existing_waiter() {
		let state = SpeechPlayback::default();
		let mut old = state.sender("old").subscribe();
		for n in 0..128 {
			state.started(&n.to_string());
		}
		old.changed().await.unwrap();
		assert_eq!(*old.borrow(), Phase::Ended);
		assert_eq!(state.entries.lock().unwrap().len(), 128);
	}
	#[tokio::test]
	async fn segment_gap_is_not_utterance_completion() {
		let state = SpeechPlayback::default();
		state.queued("mine");
		state.queued("mine");
		state.started("mine");
		state.ended("mine");
		assert_eq!(*state.sender("mine").borrow(), Phase::Waiting);
		state.started("mine");
		assert!(state.wait("mine", Duration::ZERO).await.is_ok());
		state.ended("mine");
		assert!(state.wait("mine", Duration::ZERO).await.is_err());
	}
}

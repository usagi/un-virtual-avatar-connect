//! 配信中のAI自発参加を、通常の人間発話への応答とは別予算で判定する純粋ポリシー。

use super::config::AiInitiativeConf;

const HOUR_MS: u64 = 60 * 60 * 1_000;
const EVENTSUB_LIVENESS_MAX_AGE_MS: u64 = 120_000;

pub fn stream_online_available(declared_online: Option<bool>, declared_fresh: bool, eventsub_last_message_ms: u64, now_ms: u64) -> bool {
	declared_online == Some(true)
		&& (declared_fresh
			|| (eventsub_last_message_ms > 0 && now_ms.saturating_sub(eventsub_last_message_ms) <= EVENTSUB_LIVENESS_MAX_AGE_MS))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitiativeCandidateKind {
	StreamStart,
	OngoingConversation,
	LongSilence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitiativePolicyOutcome {
	SuppressedStreamUnavailable,
	SuppressedStartupGrace,
	SuppressedActiveConversation,
	SuppressedMinInterval,
	SuppressedAttemptBudget,
	SuppressedSpeechBudget,
	Candidate(InitiativeCandidateKind),
}

#[derive(Debug, Clone, Copy)]
pub struct InitiativePolicyInput<'a> {
	pub now_ms: u64,
	pub stream_online_fresh: bool,
	pub online_since_ms: Option<u64>,
	pub last_activity_ms: Option<u64>,
	pub attempt_times_ms: &'a [u64],
	pub speech_times_ms: &'a [u64],
}

pub fn evaluate(conf: &AiInitiativeConf, input: InitiativePolicyInput<'_>) -> InitiativePolicyOutcome {
	if !input.stream_online_fresh {
		return InitiativePolicyOutcome::SuppressedStreamUnavailable;
	}
	let Some(online_since_ms) = input.online_since_ms else {
		return InitiativePolicyOutcome::SuppressedStreamUnavailable;
	};
	if input.now_ms.saturating_sub(online_since_ms) < conf.startup_grace_secs.saturating_mul(1_000) {
		return InitiativePolicyOutcome::SuppressedStartupGrace;
	}
	let activity_after_online = input.last_activity_ms.filter(|at| *at > online_since_ms);
	if let Some(last_attempt_ms) = input.attempt_times_ms.last() {
		if input.now_ms.saturating_sub(*last_attempt_ms) < conf.min_interval_secs.saturating_mul(1_000) {
			return InitiativePolicyOutcome::SuppressedMinInterval;
		}
	}
	let candidate = match activity_after_online {
		Some(last_activity_ms) if input.now_ms.saturating_sub(last_activity_ms) < conf.idle_secs.saturating_mul(1_000) => {
			// 継続的な実況では「人間が無音になるまで待つ」と候補が永久に生まれない。
			// 初回だけは配信開始からmin_intervalをcadence基準にし、その後はattempt時刻で抑制する。
			if input.attempt_times_ms.is_empty()
				&& input.now_ms.saturating_sub(online_since_ms) < conf.min_interval_secs.saturating_mul(1_000)
			{
				return InitiativePolicyOutcome::SuppressedActiveConversation;
			}
			InitiativeCandidateKind::OngoingConversation
		}
		Some(_) => InitiativeCandidateKind::LongSilence,
		None => InitiativeCandidateKind::StreamStart,
	};
	let window_start = input.now_ms.saturating_sub(HOUR_MS);
	if input.attempt_times_ms.iter().filter(|at| **at >= window_start).count() >= conf.max_attempts_per_hour {
		return InitiativePolicyOutcome::SuppressedAttemptBudget;
	}
	if input.speech_times_ms.iter().filter(|at| **at >= window_start).count() >= conf.max_speeches_per_hour {
		return InitiativePolicyOutcome::SuppressedSpeechBudget;
	}
	InitiativePolicyOutcome::Candidate(candidate)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn conf() -> AiInitiativeConf {
		AiInitiativeConf {
			enabled: true,
			interval_secs: 15,
			startup_grace_secs: 60,
			idle_secs: 180,
			min_interval_secs: 300,
			max_attempts_per_hour: 4,
			max_speeches_per_hour: 2,
		}
	}

	fn input(now_ms: u64) -> InitiativePolicyInput<'static> {
		InitiativePolicyInput {
			now_ms,
			stream_online_fresh: true,
			online_since_ms: Some(1_000),
			last_activity_ms: None,
			attempt_times_ms: &[],
			speech_times_ms: &[],
		}
	}

	#[test]
	fn waits_during_startup_grace() {
		assert_eq!(evaluate(&conf(), input(60_999)), InitiativePolicyOutcome::SuppressedStartupGrace);
	}

	#[test]
	fn proposes_stream_start_after_grace_without_human_activity() {
		assert_eq!(
			evaluate(&conf(), input(61_000)),
			InitiativePolicyOutcome::Candidate(InitiativeCandidateKind::StreamStart)
		);
	}

	#[test]
	fn proposes_only_after_long_silence_when_conversation_started() {
		let mut active = input(200_000);
		active.last_activity_ms = Some(100_000);
		assert_eq!(evaluate(&conf(), active), InitiativePolicyOutcome::SuppressedActiveConversation);
		let mut idle = input(280_000);
		idle.last_activity_ms = Some(100_000);
		assert_eq!(
			evaluate(&conf(), idle),
			InitiativePolicyOutcome::Candidate(InitiativeCandidateKind::LongSilence)
		);
	}

	#[test]
	fn proposes_bounded_participation_while_commentary_continues() {
		let mut active = input(301_000);
		active.last_activity_ms = Some(300_000);
		assert_eq!(
			evaluate(&conf(), active),
			InitiativePolicyOutcome::Candidate(InitiativeCandidateKind::OngoingConversation)
		);

		let attempts = [301_000];
		active.now_ms = 450_000;
		active.last_activity_ms = Some(449_000);
		active.attempt_times_ms = &attempts;
		assert_eq!(evaluate(&conf(), active), InitiativePolicyOutcome::SuppressedMinInterval);
	}

	#[test]
	fn suppresses_when_stream_is_offline_or_stale() {
		let mut unavailable = input(500_000);
		unavailable.stream_online_fresh = false;
		assert_eq!(evaluate(&conf(), unavailable), InitiativePolicyOutcome::SuppressedStreamUnavailable);
	}

	#[test]
	fn applies_attempt_and_speech_budgets_independently() {
		let attempts = [500_000, 1_000_000, 2_000_000, 3_000_000];
		let mut attempt_limited = input(4_000_000);
		attempt_limited.attempt_times_ms = &attempts;
		assert_eq!(evaluate(&conf(), attempt_limited), InitiativePolicyOutcome::SuppressedAttemptBudget);

		let speeches = [2_000_000, 3_000_000];
		let mut speech_limited = input(4_000_000);
		speech_limited.speech_times_ms = &speeches;
		assert_eq!(evaluate(&conf(), speech_limited), InitiativePolicyOutcome::SuppressedSpeechBudget);
	}

	#[test]
	fn eventsub_keepalive_extends_online_state_but_not_forever() {
		assert!(stream_online_available(Some(true), false, 900_000, 1_000_000));
		assert!(!stream_online_available(Some(true), false, 800_000, 1_000_000));
		assert!(!stream_online_available(Some(false), true, 999_000, 1_000_000));
	}
}

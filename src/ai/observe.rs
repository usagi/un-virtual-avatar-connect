//! AI サービスが観測（observe）する Datum の通知型と、`ObservePolicyConf` を素に作るフィルタ補助。

use crate::ai::config::ObservePolicyConf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationRule { Excluded, All, OwnUtterance, Trigger, Additional, Unobserved }

/// AI サービスの event loop に流す観測通知。Datum の ID とチャンネル名のみ（本体はロード時に取得）。
#[derive(Clone, Debug)]
pub struct Observation {
	pub datum_id: u64,
	pub channel: String,
}

/// 文脈窓（memory window）や役割（user / assistant / system）付与のための、ペルソナ観点のフィルタ集合。
#[derive(Debug, Clone)]
pub struct ObserveSet {
	pub triggers: Vec<String>,
	pub channel_utterance: String,
	pub include_all: bool,
	pub include_additional: Vec<String>,
	pub exclude: Vec<String>,
}

impl ObserveSet {
	pub fn from_conf(channel_utterance: &str, policy: &ObservePolicyConf) -> Self {
		Self {
			triggers: policy.triggers.clone(),
			channel_utterance: channel_utterance.to_string(),
			include_all: policy.include_all,
			include_additional: policy.include_additional.clone(),
			exclude: policy.exclude.clone(),
		}
	}

	/// この発言を memory window に入れるか。
	pub fn is_observed(&self, channel: &str) -> bool {
		!matches!(self.rule_for(channel), ObservationRule::Excluded | ObservationRule::Unobserved)
	}

	/// The same precedence used for memory observation, exposed for configuration UI.
	pub fn rule_for(&self, channel: &str) -> ObservationRule {
		if self.exclude.iter().any(|c| c == channel) {
			return ObservationRule::Excluded;
		}
		if self.include_all {
			return ObservationRule::All;
		}
		if channel == self.channel_utterance {
			return ObservationRule::OwnUtterance;
		}
		if self.triggers.iter().any(|c| c == channel) {
			return ObservationRule::Trigger;
		}
		if self.include_additional.iter().any(|c| c == channel) {
			return ObservationRule::Additional;
		}
		ObservationRule::Unobserved
	}

	/// 発火のトリガーとなるチャンネルか（evaluate 起動の判定に使う）。
	pub fn is_trigger(&self, channel: &str) -> bool {
		self.triggers.iter().any(|c| c == channel)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn observation_reason_preserves_policy_precedence_and_trigger_independence() {
		// Exercise every overlap, including an excluded trigger: observing a
		// datum and using its channel as an evaluation trigger are distinct.
		for mask in 0u8..32 {
			let excluded = mask & 1 != 0;
			let all = mask & 2 != 0;
			let own = mask & 4 != 0;
			let trigger = mask & 8 != 0;
			let additional = mask & 16 != 0;
			let channels = |present| if present { vec!["obs:game_frame".into()] } else { vec![] };
			let policy = ObserveSet {
				triggers: channels(trigger),
				channel_utterance: if own { "obs:game_frame" } else { "ai:reply" }.into(),
				include_all: all,
				include_additional: channels(additional),
				exclude: channels(excluded),
			};
			let expected = if excluded { ObservationRule::Excluded }
				else if all { ObservationRule::All }
				else if own { ObservationRule::OwnUtterance }
				else if trigger { ObservationRule::Trigger }
				else if additional { ObservationRule::Additional }
				else { ObservationRule::Unobserved };
			assert_eq!(policy.rule_for("obs:game_frame"), expected, "mask={mask}");
			assert_eq!(policy.is_observed("obs:game_frame"), !excluded && (all || own || trigger || additional), "mask={mask}");
			assert_eq!(policy.is_trigger("obs:game_frame"), trigger, "mask={mask}");
			assert_eq!(policy.is_observed("obs:unrelated"), all, "mask={mask}");
		}
	}
}

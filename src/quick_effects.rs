use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub const QUICK_EFFECT_QUEUE_CAPACITY: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuickEffectStatus {
	Scheduled,
	Active,
	Expired,
	Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickEffectItem {
	pub id: String,
	pub label: String,
	pub channel: String,
	pub content: String,
	pub flags: Vec<String>,
	pub created_at: String,
	pub created_at_ms: u128,
	pub fire_at_ms: Option<u128>,
	pub fired_at_ms: Option<u128>,
	pub duration_ms: Option<u64>,
	pub expires_at_ms: Option<u128>,
	pub status: QuickEffectStatus,
	pub datum_id: u64,
	pub meta: JsonValue,
}

impl QuickEffectItem {
	pub fn effective_status(&self, now_ms: u128) -> QuickEffectStatus {
		if self.status == QuickEffectStatus::Cancelled {
			return QuickEffectStatus::Cancelled;
		}
		if self.status == QuickEffectStatus::Scheduled {
			return QuickEffectStatus::Scheduled;
		}
		if self.expires_at_ms.map(|expires| now_ms >= expires).unwrap_or(false) {
			return QuickEffectStatus::Expired;
		}
		QuickEffectStatus::Active
	}

	pub fn with_effective_status(mut self, now_ms: u128) -> Self {
		self.status = self.effective_status(now_ms);
		self
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn item(status: QuickEffectStatus) -> QuickEffectItem {
		QuickEffectItem {
			id: "qe-1".into(),
			label: "test".into(),
			channel: "effect".into(),
			content: "x".into(),
			flags: vec![],
			created_at: "2026-05-31T00:00:00Z".into(),
			created_at_ms: 1000,
			fire_at_ms: None,
			fired_at_ms: Some(1000),
			duration_ms: Some(100),
			expires_at_ms: Some(1100),
			status,
			datum_id: 1,
			meta: JsonValue::Object(Default::default()),
		}
	}

	#[test]
	fn scheduled_status_does_not_expire_before_fire_task_updates_it() {
		let mut effect = item(QuickEffectStatus::Scheduled);
		effect.fire_at_ms = Some(2000);
		effect.fired_at_ms = None;
		assert_eq!(effect.effective_status(3000), QuickEffectStatus::Scheduled);
	}

	#[test]
	fn active_status_expires_after_expires_at() {
		assert_eq!(item(QuickEffectStatus::Active).effective_status(1100), QuickEffectStatus::Expired);
	}
}

use std::sync::atomic::{AtomicBool, Ordering};

/// 音声認識workerを維持したまま、認識結果のFlowgraph投入だけを止める共有状態。
#[derive(Debug, Default)]
pub struct VoiceInputControl {
	muted: AtomicBool,
}

impl VoiceInputControl {
	pub fn new(muted: bool) -> Self {
		Self {
			muted: AtomicBool::new(muted),
		}
	}

	pub fn is_muted(&self) -> bool {
		self.muted.load(Ordering::SeqCst)
	}

	/// 状態が変化した場合だけtrueを返す。
	pub fn set_muted(&self, muted: bool) -> bool {
		self.muted.swap(muted, Ordering::SeqCst) != muted
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn mute_state_is_shared_and_idempotent() {
		let control = std::sync::Arc::new(VoiceInputControl::default());
		let clone = control.clone();
		assert!(!control.is_muted());
		assert!(control.set_muted(true));
		assert!(clone.is_muted());
		assert!(!clone.set_muted(true));
		assert!(clone.set_muted(false));
		assert!(!control.is_muted());
	}
}

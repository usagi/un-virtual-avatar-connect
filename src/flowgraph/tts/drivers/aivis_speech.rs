//! AivisSpeech Engine TTS ドライバ。
//!
//! VOICEVOX 互換 HTTP API のため、実装は `voicevox` ドライバと共通。既定エンドポイントだけ差し替え。

use super::super::driver::{TtsDriver, TtsError, TtsParamSchema, TtsRequest, TtsSynthesis};
use async_trait::async_trait;

pub struct AivisSpeechDriver;

const AIVIS_SPEECH_DEFAULT: &str = "http://127.0.0.1:10101";

#[async_trait]
impl TtsDriver for AivisSpeechDriver {
	fn name(&self) -> &'static str {
		"aivis_speech"
	}
	fn params_schema(&self) -> TtsParamSchema {
		super::voicevox::voicevox_like_schema()
	}
	async fn synthesize(&self, req: &TtsRequest) -> Result<TtsSynthesis, TtsError> {
		super::voicevox::synthesize_voicevox_like(AIVIS_SPEECH_DEFAULT, req).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn name_is_aivis_speech() {
		assert_eq!(AivisSpeechDriver.name(), "aivis_speech");
	}
}

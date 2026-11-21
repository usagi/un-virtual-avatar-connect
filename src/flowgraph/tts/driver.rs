//! TTS ドライバ共通インタフェース。

use crate::flowgraph::socket::SocketValue;
use async_trait::async_trait;
use std::collections::BTreeMap;

/// TTS ノードからドライバへ渡されるリクエスト。
/// ノードの入力ポートと 1:1 で対応する。
#[derive(Debug, Clone)]
pub struct TtsRequest {
	/// 合成対象テキスト（空文字は呼び出し側で事前に弾かれる）
	pub text: String,
	/// エンジン固有の意味論（OS TTS: 音声名部分一致、VOICEVOX 系: `"uuid:style"` or グローバル style_id、
	/// Bouyomichan: 数値 `"0"`〜`"7"`、など）
	pub voice: String,
	/// 正規化速度倍率（1.0 = 標準）
	pub speed: f64,
	/// 正規化ピッチオフセット（0.0 = 標準）
	pub pitch: f64,
	/// 正規化音量倍率（1.0 = 標準）
	pub volume: f64,
	/// HTTP base URL / TCP host:port 等（空ならドライバ既定）
	pub endpoint: String,
	/// WAV 保存先パステンプレ（`{T}` は UTC ISO 日時に展開）。空なら保存しない。
	/// Bouyomichan のように合成バイト列が得られないドライバでは黙って無視される。
	pub save_path: String,
	/// エンジン固有パラメータの escape hatch
	pub extra: BTreeMap<String, SocketValue>,
}

/// ドライバの処理結果。
#[derive(Debug, Default, Clone)]
pub struct TtsOutcome {
	/// 実際に再生シンクへ投入したか
	pub played: bool,
	/// 保存した WAV のパス（未保存なら空文字）
	pub audio_path: String,
}

/// TTS エンジンによる合成結果。
///
/// `Wav` は VAC の共有再生経路へ投入できるため、先行合成と順序制御が可能。
/// `SelfPlayed` はエンジン自身が再生するため、Speech Scheduler の制御対象外となる。
#[derive(Debug)]
pub enum TtsSynthesis {
	Wav(Vec<u8>),
	SelfPlayed,
}

/// TTS ドライバ一般化エラー。ノード側で `on_error` + `error: String` に落とし込む。
#[derive(Debug, thiserror::Error)]
pub enum TtsError {
	#[error("{0}")]
	Config(String),
	#[error("{0}")]
	Network(String),
	#[error("{0}")]
	Synthesis(String),
	#[error("{0}")]
	Playback(String),
	#[error("{0}")]
	Io(String),
	#[error("engine '{0}' not supported on this platform")]
	Unsupported(&'static str),
}

impl TtsError {
	/// 表示用にエンジン名を前置した文字列を返す。
	pub fn display_with_engine(&self, engine: &str) -> String {
		format!("[tts.{engine}] {self}")
	}
}

/// 音声再生シンクを抽象化した薄いヘルパ。
/// `ExecCtx::audio_sink` が `None` の場合は再生をスキップし、WAV 保存のみ行う。
pub struct AudioContext<'a> {
	pub sink: Option<&'a crate::SharedAudioSink>,
}

impl<'a> AudioContext<'a> {
	/// WAV bytes を再生キューに追加。`audio_sink` が `None` のときは `Ok(false)`。
	pub async fn play_wav(&self, wav: Vec<u8>) -> Result<bool, TtsError> {
		self.play_wav_if(wav, || true).await
	}

	pub(crate) async fn play_wav_if(&self, wav: Vec<u8>, accept: impl FnOnce() -> bool) -> Result<bool, TtsError> {
		let Some(sink) = self.sink else { return Ok(false) };
		let cursor = std::io::Cursor::new(wav);
		let decoded = rodio::Decoder::new(cursor).map_err(|e| TtsError::Playback(format!("WAV decode: {e}")))?;
		let mut locked = sink.lock().await;
		locked.ensure_device().map_err(|error| TtsError::Playback(format!("audio output: {error}")))?;
		if !accept() {
			return Ok(false);
		}
		locked.player.append(decoded);
		Ok(true)
	}
}

/// TTS ドライバ抽象。エンジン固有ロジックをここに閉じ込める。
///
/// 実装は副作用を持ち async であり、`flowgraph.tts.speak` ノード（EffectfulNode）経由でのみ叩かれる。
#[async_trait]
pub trait TtsDriver: Send + Sync {
	/// `engine` 入力値（例: `"voicevox"`）。`TtsRegistry` の key。
	fn name(&self) -> &'static str;

	/// δ-6 GUI 向けの extra パラメータ一覧（現段階は自由記述スキーマ）。
	fn params_schema(&self) -> TtsParamSchema {
		TtsParamSchema::default()
	}

	/// WAVを返してVAC側で再生順序を制御できるか。
	fn supports_deferred_playback(&self) -> bool {
		true
	}

	/// 音声を合成する。可能なドライバは再生せず WAV bytes を返す。
	async fn synthesize(&self, req: &TtsRequest) -> Result<TtsSynthesis, TtsError>;

	/// 従来の一括合成・再生経路。Flowgraph ノード向けの互換動作。
	async fn speak(&self, req: TtsRequest, audio: &AudioContext<'_>) -> Result<TtsOutcome, TtsError> {
		match self.synthesize(&req).await? {
			TtsSynthesis::Wav(wav) => {
				let played = audio.play_wav(wav.clone()).await?;
				let audio_path = maybe_save_wav(&wav, &req.save_path).await?;
				Ok(TtsOutcome { played, audio_path })
			}
			TtsSynthesis::SelfPlayed => {
				if !req.save_path.is_empty() {
					log::warn!(
						"[tts.{}] save_path は合成音声を返さないエンジンでは利用できません。無視します。",
						self.name()
					);
				}
				Ok(TtsOutcome {
					played: true,
					audio_path: String::new(),
				})
			}
		}
	}
}

/// ドライバが宣言する `extra` の許容 key と簡易型情報。δ-6 GUI バリデーション用。
/// 現段階はメタデータのみ（loader でのチェックは δ-5 以降で活用）。
#[derive(Debug, Default, Clone)]
pub struct TtsParamSchema {
	pub entries: Vec<TtsParamEntry>,
}

#[derive(Debug, Clone)]
pub struct TtsParamEntry {
	pub key: &'static str,
	pub ty: TtsParamType,
	pub description: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub enum TtsParamType {
	Bool,
	Int,
	Float,
	String,
	Json,
}

// ---------------------------------------------------------------------
// extra ヘルパ（各ドライバが共通で使う）
// ---------------------------------------------------------------------

pub fn extra_str<'a>(extra: &'a BTreeMap<String, SocketValue>, key: &str) -> Option<&'a str> {
	extra.get(key).and_then(|v| v.as_str().ok())
}

pub fn extra_i64(extra: &BTreeMap<String, SocketValue>, key: &str) -> Option<i64> {
	extra.get(key).and_then(|v| v.as_i64().ok())
}

pub fn extra_f64(extra: &BTreeMap<String, SocketValue>, key: &str) -> Option<f64> {
	extra.get(key).and_then(|v| v.as_f64().ok())
}

pub fn extra_bool(extra: &BTreeMap<String, SocketValue>, key: &str) -> Option<bool> {
	extra.get(key).and_then(|v| v.as_bool().ok())
}

// ---------------------------------------------------------------------
// `{T}` テンプレ置換（screenshot.capture と同ロジック）
// ---------------------------------------------------------------------

pub fn resolve_save_path(template: &str) -> String {
	if template.contains("{T}") {
		let t = jiff::Timestamp::now().to_string().replace([':', '-'], "");
		template.replace("{T}", &t)
	} else {
		template.to_string()
	}
}

/// WAV bytes を `save_path` に書き出し。空文字なら `Ok("")`。
pub async fn maybe_save_wav(wav: &[u8], save_path: &str) -> Result<String, TtsError> {
	if save_path.is_empty() {
		return Ok(String::new());
	}
	let resolved = resolve_save_path(save_path);
	let p = std::path::Path::new(&resolved);
	if let Some(parent) = p.parent() {
		if !parent.as_os_str().is_empty() && !parent.exists() {
			tokio::fs::create_dir_all(parent)
				.await
				.map_err(|e| TtsError::Io(format!("mkdir {parent:?}: {e}")))?;
		}
	}
	tokio::fs::write(p, wav)
		.await
		.map_err(|e| TtsError::Io(format!("write {resolved}: {e}")))?;
	Ok(resolved)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn unavailable_output_does_not_commit_playback() {
		let mut wav = b"RIFF".to_vec();
		wav.extend_from_slice(&38u32.to_le_bytes());
		wav.extend_from_slice(b"WAVEfmt ");
		wav.extend_from_slice(&16u32.to_le_bytes());
		for value in [1u16, 1] { wav.extend_from_slice(&value.to_le_bytes()); }
		for value in [16000u32, 32000] { wav.extend_from_slice(&value.to_le_bytes()); }
		for value in [2u16, 16] { wav.extend_from_slice(&value.to_le_bytes()); }
		wav.extend_from_slice(b"data");
		wav.extend_from_slice(&2u32.to_le_bytes());
		wav.extend_from_slice(&0i16.to_le_bytes());
		let sink = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::disconnected_for_test()));
		let audio = AudioContext { sink: Some(&sink) };
		let result = audio.play_wav_if(wav, || panic!("failed output must not commit speech")).await;
		assert!(matches!(result, Err(TtsError::Playback(message)) if message.contains("audio output")));
		assert!(sink.lock().await.player.empty());
	}

	#[test]
	fn resolve_save_path_replaces_template() {
		assert_eq!(resolve_save_path("out.wav"), "out.wav");
		let r = resolve_save_path("out_{T}.wav");
		assert!(r.starts_with("out_") && r.ends_with(".wav") && !r.contains("{T}"));
	}

	#[tokio::test]
	async fn maybe_save_wav_noop_on_empty_path() {
		let s = maybe_save_wav(&[0, 1, 2], "").await.unwrap();
		assert_eq!(s, "");
	}

	#[test]
	fn extra_helpers() {
		let mut m = BTreeMap::new();
		m.insert("n".into(), SocketValue::Int(42));
		m.insert("s".into(), SocketValue::String("hi".into()));
		m.insert("b".into(), SocketValue::Bool(true));
		assert_eq!(extra_i64(&m, "n"), Some(42));
		assert_eq!(extra_str(&m, "s"), Some("hi"));
		assert_eq!(extra_bool(&m, "b"), Some(true));
		assert_eq!(extra_i64(&m, "missing"), None);
	}
}

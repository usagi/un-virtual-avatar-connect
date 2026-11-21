//! Phase δ-9 D.2/D.3 (v1.0 前): V1 `Processor` trait / `ProcessorKind` / dispatch ループを除去した後の
//! **薄いモジュール群**。ここに残っているのは以下の 2 種類だけ:
//!
//! 1. **ingress 系**: Flowgraph bridge から呼び出される voice / twitch (IRC + EventSub) の
//!    低レベル worker 実装。V1 `[[processors]]` 経由の起動は廃止済みで、残存設定は警告のみ。
//! 2. **共通ユーティリティ**: `voicevox_engine` は Flowgraph の `tts.speak` ドライバ (VoiceVox) が直接呼ぶ。
//!
//! V1 変換系 (Modify/Command/翻訳/TTS/OCR/Screenshot など) は全て Flowgraph ノードに置き換わったため削除済み。
//! ただし `ocr` / `screenshot` モジュールは Flowgraph ノード実装から直接呼ばれる低レベルユーティリティ
//! として残している（Windows Media OCR バインディング、Win32 GDI キャプチャなど）。

pub(crate) mod ingress;
pub(crate) mod ocr;
pub(crate) mod screenshot;
pub(crate) mod voice;
pub(crate) mod voice_partial_preview;
#[cfg(feature = "voice-vosk")]
pub(crate) mod voice_vosk;
#[cfg(feature = "voice-vosk")]
pub(crate) mod voice_vosk_model;
#[cfg(feature = "voice-whisper")]
pub(crate) mod voice_whisper;
#[cfg(any(feature = "voice-whisper", feature = "stt-eval-whisper"))]
pub(crate) mod voice_whisper_model;
pub(crate) mod voicevox_engine;
#[cfg(feature = "voice-whisper")]
pub(crate) mod whisper_worker;

pub use voicevox_engine::{VoicevoxSpeaker, VoicevoxStyle};

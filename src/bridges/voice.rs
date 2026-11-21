//! `flowgraph.ingress.voice` 用ブリッジ（δ-9 Part E.2）。
//!
//! ## 責務
//!
//! - `flowgraph.ingress.voice` ノードの property から [`FlowgraphVoiceIngress`] を生成する。
//! - [`spawn`] でノード 1 件につき Vosk / Whisper ワーカースレッドを起動する。認識結果は
//!   `TriggerHandle::send` で該当ノードに流れ、graph 下流へ伝播する。
//!
//! ## 実装メモ
//!
//! 実際の Vosk / Whisper 呼び出し本体は V1 ingress と共用しており、
//! `processor::voice::spawn_from_flowgraph`（その下の [`crate::processor::voice::VoiceSink`] 抽象経由）
//! を叩く。Flowgraph 経路では V2 sink が組まれ、`channel_data` には push せず
//! `TriggerEvent` のみを投げる。
//!
//! 部分認識は観測イベントと speech floor の保持に使い、Flowgraph には確定テキストだけを投げる。

use crate::flowgraph::loader::LoadedNodeMeta;
use crate::flowgraph::node::TriggerHandle;
use crate::processor::voice::VoiceIngress;

#[derive(Debug, Clone)]
pub struct FlowgraphVoiceIngress {
	pub node_id: String,
	pub label: String,
	pub engine: String,
	pub model_path: String,
	/// 入力デバイス名の部分一致。空なら既定入力。
	#[allow(dead_code)]
	pub source: String,
	/// サンプリングレート（現状 Vosk=16kHz 固定、Whisper=16kHz 内部リサンプル。将来の上書き用）
	#[allow(dead_code)]
	pub sample_rate: i64,
	pub language_code: String,
	/// Whisper worker backend preference: auto, vulkan, or cpu.
	#[cfg_attr(not(feature = "voice-whisper"), allow(dead_code))]
	pub whisper_backend: String,
	/// Backend-specific GPU index. Empty means automatic dGPU selection.
	#[cfg_attr(not(feature = "voice-whisper"), allow(dead_code))]
	pub whisper_gpu_device: String,
	/// Vosk の constrained grammar (JSON) — 将来の拡張用で現状は未使用
	#[allow(dead_code)]
	pub grammar_json: String,
	pub fixed_channel: String,
	pub partial_channel: String,
	#[allow(dead_code)]
	pub partial_interval_ms: u64,
	#[allow(dead_code)]
	pub partial_min_audio_ms: u64,
	pub speech_floor_key: String,
	pub speech_floor_release_delay_ms: u64,
	#[allow(dead_code)]
	pub vad_threshold_db: f32,
	#[allow(dead_code)]
	pub vad_endpoint_ms: u64,
	#[allow(dead_code)]
	pub vad_pre_roll_ms: u64,
	#[allow(dead_code)]
	pub vad_min_speech_ms: u64,
}

impl FlowgraphVoiceIngress {
	pub fn from_meta(fq: &str, meta: &LoadedNodeMeta) -> Option<Self> {
		let p = &meta.properties;
		let get_str = |k: &str| -> String {
			p.get(k)
				.and_then(|v| v.as_str().ok())
				.map(str::trim)
				.map(str::to_string)
				.unwrap_or_default()
		};
		let get_int = |k: &str| -> i64 { p.get(k).and_then(|v| v.as_i64().ok()).unwrap_or(0) };
		let requested_engine = {
			let engine = get_str("engine").to_ascii_lowercase();
			if engine.is_empty() {
				"vosk".into()
			} else {
				engine
			}
		};
		let configured_model_path = get_str("model_path");
		let legacy_whisper_model_path = get_str("whisper_model_path");
		let (engine, model_path) = if matches!(requested_engine.as_str(), "hybrid" | "vosk-whisper") {
			log::warn!("《Flowgraph/Voice》: engine={requested_engine}は廃止されたためWhisper単独へ移行します node={fq}");
			(
				"whisper".into(),
				if legacy_whisper_model_path.is_empty() {
					configured_model_path
				} else {
					legacy_whisper_model_path
				},
			)
		} else {
			(requested_engine, configured_model_path)
		};
		Some(Self {
			node_id: fq.to_string(),
			label: get_str("label"),
			engine,
			model_path,
			source: get_str("source"),
			sample_rate: {
				let v = get_int("sample_rate");
				if v <= 0 {
					16000
				} else {
					v
				}
			},
			language_code: {
				let l = get_str("language_code");
				if l.is_empty() {
					"ja".into()
				} else {
					l
				}
			},
			whisper_backend: {
				let backend = get_str("whisper_backend").to_ascii_lowercase();
				if matches!(backend.as_str(), "vulkan" | "cpu") {
					backend
				} else {
					"auto".into()
				}
			},
			whisper_gpu_device: get_str("whisper_gpu_device"),
			grammar_json: get_str("grammar_json"),
			fixed_channel: get_str("fixed_channel"),
			partial_channel: get_str("partial_channel"),
			partial_interval_ms: p
				.get("partial_interval_ms")
				.and_then(|value| value.as_i64().ok())
				.map(|value| if value <= 0 { 0 } else { value.clamp(250, 5_000) as u64 })
				.unwrap_or(800),
			partial_min_audio_ms: p
				.get("partial_min_audio_ms")
				.and_then(|value| value.as_i64().ok())
				.map(|value| if value <= 0 { 1_300 } else { value.clamp(200, 5_000) as u64 })
				.unwrap_or(1_300),
			speech_floor_key: get_str("speech_floor_key"),
			speech_floor_release_delay_ms: p
				.get("speech_floor_release_delay_ms")
				.and_then(|v| v.as_i64().ok())
				.unwrap_or(700)
				.clamp(0, 10_000) as u64,
			vad_threshold_db: {
				let value = get_int("vad_threshold_db");
				if value == 0 {
					-45.0
				} else {
					value.clamp(-80, -10) as f32
				}
			},
			vad_endpoint_ms: {
				let value = get_int("vad_endpoint_ms");
				if value == 0 {
					600
				} else {
					value.clamp(200, 5_000) as u64
				}
			},
			vad_pre_roll_ms: {
				let value = get_int("vad_pre_roll_ms");
				if value == 0 {
					400
				} else {
					value.clamp(0, 2_000) as u64
				}
			},
			vad_min_speech_ms: {
				let value = get_int("vad_min_speech_ms");
				if value == 0 {
					100
				} else {
					value.clamp(20, 1_000) as u64
				}
			},
		})
	}
}

/// `flowgraph.ingress.voice` の全エントリに対してワーカースレッドを起動する。
///
/// `trigger` が None の場合（Flowgraph 未起動）は warn ログのみ出して何も spawn しない。
///
/// 戻り値は起動に成功した `VoiceIngress` ハンドル一覧。shutdown 時に `.finish().await` すること。
pub fn spawn(
	entries: &[FlowgraphVoiceIngress],
	trigger: Option<TriggerHandle>,
	tokio_handle: tokio::runtime::Handle,
	control_event_tx: tokio::sync::broadcast::Sender<crate::control_events::ControlEvent>,
	speech_floor: std::sync::Arc<crate::state::SpeechFloorManager>,
	voice_input: std::sync::Arc<crate::state::VoiceInputControl>,
	state: std::sync::Weak<tokio::sync::RwLock<crate::state::State>>,
) -> (Vec<VoiceIngress>, Vec<String>) {
	if entries.is_empty() {
		return (Vec::new(), Vec::new());
	}
	let Some(trigger) = trigger else {
		log::warn!(
			"《Flowgraph/Voice》 ingress ノード {} 件を検出しましたが、Flowgraph ランタイムが起動していないため voice bridge を開始できません。",
			entries.len()
		);
		return (Vec::new(), entries.iter().map(|entry| entry.node_id.clone()).collect());
	};

	let mut handles = Vec::new();
	let mut failed_nodes = Vec::new();
	for e in entries {
		log::info!(
			"《Flowgraph/Voice》 起動準備 node={} engine={} model_path={:?} source={:?} sample_rate={} lang={} fixed_channel={:?}",
			e.node_id,
			e.engine,
			e.model_path,
			e.source,
			e.sample_rate,
			e.language_code,
			e.fixed_channel
		);
		match crate::processor::voice::spawn_from_flowgraph(
			e,
			tokio_handle.clone(),
			trigger.clone(),
			control_event_tx.clone(),
			speech_floor.clone(),
			voice_input.clone(),
			state.clone(),
		) {
			Some(h) => {
				log::info!("《Flowgraph/Voice》 ワーカー起動成功 node={} engine={}", e.node_id, e.engine);
				handles.push(h);
			}
			None => {
				failed_nodes.push(e.node_id.clone());
				log::error!(
					"《Flowgraph/Voice》 ワーカー起動失敗 node={} engine={}（model_path / feature ビルドを確認してください）",
					e.node_id,
					e.engine
				);
			}
		}
	}
	(handles, failed_nodes)
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::node::InputMap;
	use crate::flowgraph::socket::SocketValue;
	use std::path::PathBuf;

	#[test]
	fn from_meta_reads_all_fields() {
		let mut props = InputMap::new();
		props.insert("engine".into(), SocketValue::String("whisper".into()));
		props.insert("model_path".into(), SocketValue::String("/m".into()));
		props.insert("source".into(), SocketValue::String("mic1".into()));
		props.insert("sample_rate".into(), SocketValue::Int(48000));
		props.insert("language_code".into(), SocketValue::String("en".into()));
		props.insert("whisper_backend".into(), SocketValue::String("vulkan".into()));
		props.insert("whisper_gpu_device".into(), SocketValue::String("1".into()));
		props.insert("fixed_channel".into(), SocketValue::String("vc".into()));
		props.insert("partial_channel".into(), SocketValue::String("user".into()));
		props.insert("partial_interval_ms".into(), SocketValue::Int(650));
		props.insert("partial_min_audio_ms".into(), SocketValue::Int(1300));
		props.insert("speech_floor_key".into(), SocketValue::String("human_voice".into()));
		props.insert("speech_floor_release_delay_ms".into(), SocketValue::Int(1200));
		props.insert("vad_threshold_db".into(), SocketValue::Int(-42));
		props.insert("vad_endpoint_ms".into(), SocketValue::Int(800));
		props.insert("vad_pre_roll_ms".into(), SocketValue::Int(500));
		props.insert("vad_min_speech_ms".into(), SocketValue::Int(100));
		let meta = LoadedNodeMeta {
			feature: "flowgraph.ingress.voice".into(),
			file: PathBuf::from("x.toml"),
			position: None,
			activation_chain: Vec::new(),
			properties: props,
		};
		let v = FlowgraphVoiceIngress::from_meta("ingress::voice", &meta).unwrap();
		assert_eq!(v.engine, "whisper");
		assert_eq!(v.model_path, "/m");
		assert_eq!(v.source, "mic1");
		assert_eq!(v.sample_rate, 48000);
		assert_eq!(v.language_code, "en");
		assert_eq!(v.whisper_backend, "vulkan");
		assert_eq!(v.whisper_gpu_device, "1");
		assert_eq!(v.fixed_channel, "vc");
		assert_eq!(v.partial_channel, "user");
		assert_eq!(v.partial_interval_ms, 650);
		assert_eq!(v.partial_min_audio_ms, 1300);
		assert_eq!(v.speech_floor_key, "human_voice");
		assert_eq!(v.speech_floor_release_delay_ms, 1200);
		assert_eq!(v.vad_threshold_db, -42.0);
		assert_eq!(v.vad_endpoint_ms, 800);
		assert_eq!(v.vad_pre_roll_ms, 500);
		assert_eq!(v.vad_min_speech_ms, 100);
	}

	#[test]
	fn defaults_for_empty_properties() {
		let meta = LoadedNodeMeta {
			feature: "flowgraph.ingress.voice".into(),
			file: PathBuf::from("x.toml"),
			position: None,
			activation_chain: Vec::new(),
			properties: InputMap::new(),
		};
		let v = FlowgraphVoiceIngress::from_meta("in", &meta).unwrap();
		assert_eq!(v.engine, "vosk");
		assert_eq!(v.sample_rate, 16000);
		assert_eq!(v.language_code, "ja");
		assert_eq!(v.whisper_backend, "auto");
		assert!(v.whisper_gpu_device.is_empty());
		assert_eq!(v.partial_min_audio_ms, 1300);
		assert_eq!(v.speech_floor_release_delay_ms, 700);
		assert_eq!(v.vad_threshold_db, -45.0);
		assert_eq!(v.vad_endpoint_ms, 600);
		assert_eq!(v.vad_pre_roll_ms, 400);
		assert_eq!(v.vad_min_speech_ms, 100);
	}

	#[test]
	fn legacy_hybrid_is_normalized_to_single_whisper_engine() {
		let mut props = InputMap::new();
		props.insert("engine".into(), SocketValue::String("hybrid".into()));
		props.insert("model_path".into(), SocketValue::String("vosk-model".into()));
		props.insert("whisper_model_path".into(), SocketValue::String("ggml-medium.bin".into()));
		let meta = LoadedNodeMeta {
			feature: "flowgraph.ingress.voice".into(),
			file: PathBuf::from("x.toml"),
			position: None,
			activation_chain: Vec::new(),
			properties: props,
		};
		let voice = FlowgraphVoiceIngress::from_meta("voice", &meta).unwrap();
		assert_eq!(voice.engine, "whisper");
		assert_eq!(voice.model_path, "ggml-medium.bin");
	}
}

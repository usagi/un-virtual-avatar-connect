//! `flowgraph.tts.speak`: 統合 TTS EffectfulNode（δ-4c）。
//!
//! V1 の 5 種プロセッサ（`os_tts` / `bouyomichan` / `voicevox` / `aivis_speech` / `coeiroink`）を
//! 単一ノードに統合し、**`engine` 入力で動的に切り替えられる**。
//! エンジン固有ロジックは [`crate::flowgraph::tts::TtsDriver`] 実装にカプセル化している。
//!
//! ## ポート
//!
//! - 入力:
//!   - `exec_in` (Exec)
//!   - `engine` (String): `"os"` / `"bouyomichan"` / `"voicevox"` / `"aivis_speech"` / `"coeiroink"` / `"voicepeak"`
//!   - `text` (String)
//!   - `voice` (String, default `""`): エンジン別の意味論（各 driver docs 参照）
//!   - `speed` (Float, default 1.0)
//!   - `pitch` (Float, default 0.0)
//!   - `volume` (Float, default 1.0)
//!   - `endpoint` (String, default `""`): HTTP base URL / TCP `host:port` / **VoicePeak では `voicepeak.exe` の絶対パス**。
//!     VoicePeak で空のときは起動時に解決した `[voicepeak]` + OS 既定パスが State から注入される。
//!   - `extra` (Map<Json>, default `{}`): エンジン固有の escape hatch
//!   - `save_path` (String, default `""`): 合成 WAV 保存先。`{T}` は UTC ISO 日時（`:`、`-` を除去）に展開
//! - 出力:
//!   - `on_success` / `on_error` (Exec): 従来の要求受理／同期実行結果
//!   - `on_played` / `on_playback_error` (Exec): 非同期実行でも再生結果を通知
//!   - `spoken_text` / `spoken_utterance_id` (String): 再生を確認した片だけを出力
//!   - `played` (Bool): VAC 共有シンクへ append できたか
//!   - `audio_path` (String): 保存したパス（空なら未保存）
//!   - `error` (String): `on_success` 時は `""`

use crate::avatar_motion::AvatarMotionConfig;
use crate::flowgraph::node::{
	get_optional_bool, get_optional_float, get_optional_int, get_optional_map, get_optional_string, get_required_string, EffectfulNode,
	ExecCtx, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, TriggerEvent,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use crate::flowgraph::tts::driver::{AudioContext, TtsRequest};
use crate::flowgraph::tts::registry::registry;
use crate::speech_scheduler::SpeechLifecycleObserver;
use async_trait::async_trait;
use serde_json::json;

pub struct TtsSpeakNode;

const PLAYBACK_RESULT: &str = "__tts_playback_result__";
const PLAYBACK_RESUME: &str = "__tts_playback_resume__";

impl NodeDescriptor for TtsSpeakNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.tts.speak".into(),
			title: "TTS: Speak".into(),
			category: "tts".into(),
			description: Some(
				"engine 入力で選んだ TTS ドライバに text を合成させ、VAC 共有 audio_sink で再生する。save_path 指定時は WAV も保存。"
					.into(),
			),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input(PLAYBACK_RESULT, "(internal)", SocketType::Json).with_default(SocketValue::Json(serde_json::Value::Null)),
				PortSpec::input("engine", "Engine", SocketType::String).with_closed_string_variants(registry().names().iter().copied()),
				PortSpec::input("text", "Text", SocketType::String),
				PortSpec::input("voice", "Voice", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("speed", "Speed", SocketType::Float).with_default(SocketValue::Float(1.0)),
				PortSpec::input("pitch", "Pitch", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("volume", "Volume", SocketType::Float).with_default(SocketValue::Float(1.0)),
				PortSpec::input("endpoint", "Endpoint", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("extra", "Extra", SocketType::Map(Box::new(SocketType::Json)))
					.with_default(SocketValue::Map(Default::default())),
				PortSpec::input("save_path", "Save Path", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("lane", "Speech Lane", SocketType::String)
					.with_default(SocketValue::String("chat".into()))
					.with_closed_string_variants(["assistant", "chat", "system"]),
				PortSpec::input("utterance_id", "Utterance ID", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("segment_index", "Segment Index", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("non_blocking", "Non-blocking Queue", SocketType::Bool).with_default(SocketValue::Bool(false)),
				PortSpec::input("respect_speech_floor", "Wait for Speech Floor", SocketType::String)
					.with_default(SocketValue::String(String::new())),
				described_port(
					PortSpec::input("max_queue_wait_ms", "Maximum Queue Wait", SocketType::Int).with_default(SocketValue::Int(0)),
					"0はlane既定（chat=30000ms、assistant/system=無制限）、負数は無制限、正数は明示上限。",
				),
				described_port(
					PortSpec::input("max_pending", "Maximum Pending Items", SocketType::Int).with_default(SocketValue::Int(0)),
					"0はlane既定（chat=20件、assistant/system=無制限）、負数は無制限、正数は明示上限。",
				),
				PortSpec::input("lip_sync_base_key", "Lip-sync UNMF/Z Base Key", SocketType::String)
					.with_default(SocketValue::String(String::new())),
				PortSpec::input("lip_sync_expression", "Lip-sync Expression", SocketType::String)
					.with_default(SocketValue::String("JawOpen".into())),
				PortSpec::input("lip_sync_target_link_id", "Lip-sync Target Link ID", SocketType::String)
					.with_default(SocketValue::String("avatar_lipsync".into())),
				PortSpec::input("lip_sync_gain", "Lip-sync Gain", SocketType::Float).with_default(SocketValue::Float(1.0)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				described_port(
					PortSpec::exec_output("on_played", "On Played"),
					"played=trueの結果だけを通知。schedulerでは共有sinkへの投入時。non_blockingでも通知する。実聴取や音声終了の保証ではない。",
				),
				PortSpec::exec_output("on_playback_error", "On Playback Error"),
				PortSpec::output("spoken_text", "Spoken Text", SocketType::String),
				PortSpec::output("spoken_utterance_id", "Spoken Utterance ID", SocketType::String),
				PortSpec::output("played", "Played", SocketType::Bool),
				PortSpec::output("audio_path", "Audio Path", SocketType::String),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Json))),
			],
			properties: vec![],
		}
	}
}

fn described_port(mut port: PortSpec, description: &str) -> PortSpec {
	port.description = Some(description.to_string());
	port
}

fn err_output(msg: impl Into<String>) -> NodeOutput {
	let msg = msg.into();
	NodeOutput::new()
		.set_data("played", SocketValue::Bool(false))
		.set_data("spoken_text", SocketValue::String(String::new()))
		.set_data("spoken_utterance_id", SocketValue::String(String::new()))
		.set_data("audio_path", SocketValue::String(String::new()))
		.set_data("error", SocketValue::String(msg.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(msg).with_code("tts.speak")))
		.fire_exec("on_error")
}

fn success_output(played: bool, audio_path: String) -> NodeOutput {
	let result = json!({
		"played": played,
		"audio_path": audio_path,
	});
	NodeOutput::new()
		.set_data("played", SocketValue::Bool(played))
		.set_data("spoken_text", SocketValue::String(String::new()))
		.set_data("spoken_utterance_id", SocketValue::String(String::new()))
		.set_data("audio_path", SocketValue::String(audio_path))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Json(result))))
		.fire_exec("on_success")
}

fn played_output(mut output: NodeOutput, played: bool, text: String, utterance_id: String) -> NodeOutput {
	if played {
		output = output
			.set_data("spoken_text", SocketValue::String(text))
			.set_data("spoken_utterance_id", SocketValue::String(utterance_id))
			.fire_exec("on_played");
	}
	output
}

// A deferred result resumes with the original inputs. Never fire on_success again:
// existing graphs use its first emission as queue acceptance, possibly with effects.
fn resumed_output(inputs: &InputMap) -> Result<NodeOutput, NodeExecError> {
	let Some(SocketValue::Json(result)) = inputs.get(PLAYBACK_RESULT) else {
		return Err(NodeExecError::MissingRequiredInput(PLAYBACK_RESULT.into()));
	};
	if let Some(error) = result.get("error").and_then(serde_json::Value::as_str) {
		let mut output = err_output(error);
		output.fired_exec = ExecFireSet::new();
		return Ok(output.fire_exec("on_playback_error"));
	}
	let played = result.get("played").and_then(serde_json::Value::as_bool).unwrap_or(false);
	let audio_path = result.get("audio_path").and_then(serde_json::Value::as_str).unwrap_or_default();
	let mut output = success_output(played, audio_path.into());
	output.fired_exec = ExecFireSet::new();
	Ok(played_output(
		output,
		played,
		get_required_string(inputs, "text")?,
		get_optional_string(inputs, "utterance_id", "")?,
	))
}

#[async_trait]
impl EffectfulNode for TtsSpeakNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if fired_exec.contains(PLAYBACK_RESUME) {
			return resumed_output(inputs);
		}
		if !fired_exec.contains("exec_in") {
			return Ok(NodeOutput::new());
		}

		let engine = get_required_string(inputs, "engine")?;
		let text = get_required_string(inputs, "text")?;
		if text.is_empty() {
			return Ok(err_output("text が空文字です"));
		}

		let voice = get_optional_string(inputs, "voice", "")?;
		let speed = get_optional_float(inputs, "speed", 1.0)?;
		let pitch = get_optional_float(inputs, "pitch", 0.0)?;
		let volume = get_optional_float(inputs, "volume", 1.0)?;
		let mut endpoint = get_optional_string(inputs, "endpoint", "")?;
		if engine.eq_ignore_ascii_case("voicepeak") && endpoint.is_empty() {
			if let Some(wk) = ctx.state_handle.as_ref().and_then(|w| w.upgrade()) {
				let st = wk.read().await;
				endpoint = st.voicepeak_fallback_exe.clone();
			}
		}
		let save_path = get_optional_string(inputs, "save_path", "")?;
		let lane_text = get_optional_string(inputs, "lane", "chat")?;
		let lane = match crate::speech_scheduler::SpeechLane::parse(&lane_text) {
			Some(lane) => lane,
			None => {
				return Ok(err_output(format!(
					"未知の speech lane '{lane_text}' です。利用可能: assistant, chat, system"
				)))
			}
		};
		let utterance_id = get_optional_string(inputs, "utterance_id", "")?;
		let segment_index = get_optional_int(inputs, "segment_index", 0)?.max(0) as u32;
		let non_blocking = get_optional_bool(inputs, "non_blocking", false)?;
		let floor_key = get_optional_string(inputs, "respect_speech_floor", "")?;
		let speech_floor = if floor_key.trim().is_empty() {
			None
		} else {
			let Some(state) = ctx.state_handle.as_ref().and_then(|weak| weak.upgrade()) else {
				return Ok(err_output("respect_speech_floor requires shared runtime state"));
			};
			let floor = state.read().await.speech_floor.clone();
			Some((floor, floor_key.trim().to_owned()))
		};
		let queue_policy = crate::speech_scheduler::SpeechQueuePolicy::with_overrides(
			lane,
			get_optional_int(inputs, "max_queue_wait_ms", 0)?,
			get_optional_int(inputs, "max_pending", 0)?,
		);
		let lip_sync_base_key = get_optional_string(inputs, "lip_sync_base_key", "")?;
		let lip_sync = if lip_sync_base_key.trim().is_empty() {
			None
		} else {
			Some(AvatarMotionConfig {
				base_key_expr: lip_sync_base_key,
				expression: get_optional_string(inputs, "lip_sync_expression", "JawOpen")?,
				target_link_id: get_optional_string(inputs, "lip_sync_target_link_id", "avatar_lipsync")?,
				gain: get_optional_float(inputs, "lip_sync_gain", 1.0)?.clamp(0.1, 4.0) as f32,
			})
		};
		let extra_map = get_optional_map(inputs, "extra")?;

		let driver = match crate::flowgraph::tts::registry().get(&engine) {
			Some(d) => d,
			None => {
				let names = crate::flowgraph::tts::registry().names();
				return Ok(err_output(format!("未知の engine '{engine}' です。利用可能: {}", names.join(", "))));
			}
		};
		let (control_event_tx, observation_origin) = match ctx.state_handle.as_ref().and_then(|weak| weak.upgrade()) { Some(state) => {
			let shared = state.read().await;
			let origin = shared.speech_scheduler.playback.freshness(&utterance_id).map(|guard| {
				let (channel, id) = guard.observation_origin();
				(channel.to_owned(), id)
			});
			(Some(shared.control_event_tx.clone()), origin)
		} _ => {
			(None, None)
		}};
		let text_chars = text.chars().count();
		let lifecycle = control_event_tx.map(|tx| {
			SpeechLifecycleObserver::new(
				tx,
				ctx.node_id.clone(),
				utterance_id.clone(),
				engine.clone(),
				voice.clone(),
				lane,
				text_chars,
			)
			.with_observation_origin(observation_origin)
		});
		if let Some(lifecycle) = lifecycle.as_ref() {
			lifecycle.emit(crate::control_events::TtsLifecyclePhase::Started, None, None);
		}

		let req = TtsRequest {
			text: text.clone(),
			voice: voice.clone(),
			speed,
			pitch,
			volume,
			endpoint,
			save_path,
			extra: extra_map.into_owned(),
		};
		let audio = AudioContext {
			sink: ctx.audio_sink.as_ref(),
		};

		let scheduler = if driver.supports_deferred_playback() {
			match ctx.state_handle.as_ref().and_then(|weak| weak.upgrade()) { Some(state) => {
				Some(state.read().await.speech_scheduler.clone())
			} _ => {
				None
			}}
		} else {
			None
		};
		if speech_floor.is_some() && scheduler.is_none() {
			return Ok(err_output("respect_speech_floor requires a scheduler-compatible TTS driver"));
		}
		if non_blocking {
			if let Some(scheduler) = scheduler.clone() {
				let task_engine = engine.clone();
				let resume = ctx.trigger.clone().map(|bus| {
					let mut event = TriggerEvent::new(ctx.node_id.clone()).with_exec(PLAYBACK_RESUME);
					event.data_overrides = inputs.clone();
					(bus, event)
				});
				tokio::spawn(async move {
					let result = match scheduler
						.speak_with_floor(
							lane,
							utterance_id,
							segment_index,
							queue_policy,
							driver,
							req,
							lip_sync,
							lifecycle,
							speech_floor,
						)
						.await
					{
						Ok(outcome) => json!({"played": outcome.played, "audio_path": outcome.audio_path}),
						Err(error) => {
							let error = error.display_with_engine(&task_engine);
							log::warn!("{error}");
							json!({"error": error})
						}
					};
					if let Some((bus, event)) = resume {
						let _ = bus.send(event.with_override(PLAYBACK_RESULT, SocketValue::Json(result)));
					}
				});
				return Ok(success_output(false, String::new()));
			}
		}

		let scheduled = if let Some(scheduler) = scheduler {
			Some(
				scheduler
					.speak_with_floor(
						lane,
						utterance_id.clone(),
						segment_index,
						queue_policy,
						driver.clone(),
						req.clone(),
						lip_sync,
						lifecycle.clone(),
						speech_floor,
					)
					.await,
			)
		} else {
			None
		};
		let scheduler_managed = scheduled.is_some();
		let result = match scheduled {
			Some(result) => result,
			None => driver.speak(req, &audio).await,
		};

		match result {
			Ok(outcome) => {
				if !scheduler_managed {
					if let Some(lifecycle) = lifecycle.as_ref() {
						lifecycle.emit(crate::control_events::TtsLifecyclePhase::Completed, Some(outcome.played), None);
					}
				}
				Ok(played_output(
					success_output(outcome.played, outcome.audio_path),
					outcome.played,
					text,
					utterance_id,
				))
			}
			Err(e) => {
				let error = e.display_with_engine(&engine);
				if !scheduler_managed {
					if let Some(lifecycle) = lifecycle.as_ref() {
						lifecycle.emit(crate::control_events::TtsLifecyclePhase::Failed, Some(false), Some(error.clone()));
					}
				}
				Ok(err_output(error))
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::node::ExecFireSet;
	use std::collections::HashMap;

	fn inputs_with(engine: &str, text: &str) -> HashMap<String, SocketValue> {
		let mut m = HashMap::new();
		m.insert("engine".into(), SocketValue::String(engine.into()));
		m.insert("text".into(), SocketValue::String(text.into()));
		m
	}

	#[tokio::test]
	async fn no_fire_is_no_op() {
		let n = TtsSpeakNode;
		let mut ctx = ExecCtx::default();
		let inputs = inputs_with("os", "hello");
		let out = n.execute(&mut ctx, &InputMap::new(), &inputs, &ExecFireSet::new()).await.unwrap();
		assert!(out.fired_exec.is_empty());
		assert!(out.data.is_empty());
	}

	#[tokio::test]
	async fn unknown_engine_fires_on_error() {
		let n = TtsSpeakNode;
		let mut ctx = ExecCtx::default();
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let out = n
			.execute(&mut ctx, &InputMap::new(), &inputs_with("nonexistent", "hi"), &fired)
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_error"));
		let err = out
			.data
			.get("error")
			.and_then(|v| v.as_str().ok().map(str::to_owned))
			.unwrap_or_default();
		assert!(err.contains("未知の engine"));
		match out.data.get("result").unwrap() {
			SocketValue::Result(result) => {
				assert!(!result.ok);
				assert_eq!(result.code.as_deref(), Some("tts.speak"));
			}
			other => panic!("expected result, got {other:?}"),
		}
	}

	#[test]
	fn success_output_includes_result() {
		let out = success_output(true, "out.wav".into());
		assert!(out.fired_exec.contains("on_success"));
		match out.data.get("result").unwrap() {
			SocketValue::Result(result) => {
				assert!(result.ok);
				assert_eq!(
					result.value.as_deref(),
					Some(&SocketValue::Json(json!({ "played": true, "audio_path": "out.wav" })))
				);
			}
			other => panic!("expected result, got {other:?}"),
		}
	}

	#[test]
	fn deferred_result_exposes_only_confirmed_text_without_repeating_acceptance() {
		let mut inputs = inputs_with("coeiroink", "実際に再生した片。");
		inputs.insert("utterance_id".into(), SocketValue::String("reply-1".into()));
		inputs.insert(
			PLAYBACK_RESULT.into(),
			SocketValue::Json(json!({"played": true, "audio_path": "part.wav"})),
		);
		let out = resumed_output(&inputs).unwrap();
		assert_eq!(out.fired_exec.len(), 1);
		assert!(out.fired_exec.contains("on_played"));
		assert_eq!(out.data["spoken_text"], SocketValue::String("実際に再生した片。".into()));
		assert_eq!(out.data["spoken_utterance_id"], SocketValue::String("reply-1".into()));

		inputs.insert(
			PLAYBACK_RESULT.into(),
			SocketValue::Json(json!({"played": false, "audio_path": "part.wav"})),
		);
		let out = resumed_output(&inputs).unwrap();
		assert!(out.fired_exec.is_empty(), "saving audio without a sink is not playback");
		assert_eq!(out.data["spoken_text"], SocketValue::String(String::new()));

		inputs.insert(
			PLAYBACK_RESULT.into(),
			SocketValue::Json(json!({"error": "continuation_gap_exceeded"})),
		);
		let out = resumed_output(&inputs).unwrap();
		assert_eq!(out.fired_exec.len(), 1);
		assert!(out.fired_exec.contains("on_playback_error"));
		assert_eq!(out.data["spoken_text"], SocketValue::String(String::new()));
	}

	#[tokio::test]
	async fn deferred_playback_routes_original_text_in_a_new_graph_generation() {
		use crate::flowgraph::{
			engine::{create_trigger_bus, FlowgraphBuilder, PortRef},
			node::NodeImpl,
			nodes::{ingress::AiSpeechIngressNode, log::LogNode},
		};
		use std::{sync::Arc, time::Duration};
		let mut builder = FlowgraphBuilder::new();
		builder.add_node("speech", NodeImpl::pure(Arc::new(AiSpeechIngressNode)), InputMap::new());
		builder.add_node("tts", NodeImpl::effectful(Arc::new(TtsSpeakNode)), InputMap::new());
		builder.add_node("log", NodeImpl::effectful(Arc::new(LogNode)), InputMap::new());
		builder.connect(PortRef::new("speech", "content"), PortRef::new("tts", "text"));
		builder.connect_exec(PortRef::new("tts", "on_played"), PortRef::new("log", "exec_in"));
		builder.connect(PortRef::new("tts", "spoken_text"), PortRef::new("log", "value"));
		let mut program = builder.build().unwrap();
		let (bus, rx) = create_trigger_bus();
		for text in ["first played chunk", "second played chunk"] {
			let mut event = TriggerEvent::new("tts").with_exec(PLAYBACK_RESUME);
			event.data_overrides = inputs_with("coeiroink", text);
			bus.send(event.with_override(PLAYBACK_RESULT, SocketValue::Json(json!({"played": true, "audio_path": ""}))))
				.unwrap();
		}
		let mut ctx = ExecCtx::default();
		program
			.run_forever_with_bus(&mut ctx, bus, rx, tokio::time::sleep(Duration::from_millis(50)), None)
			.await
			.unwrap();
		assert_eq!(ctx.trace.len(), 2);
		assert!(ctx.trace[0].contains("first played chunk"));
		assert!(ctx.trace[1].contains("second played chunk"));
	}

	#[tokio::test]
	async fn empty_text_fires_on_error() {
		let n = TtsSpeakNode;
		let mut ctx = ExecCtx::default();
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let out = n.execute(&mut ctx, &InputMap::new(), &inputs_with("os", ""), &fired).await.unwrap();
		assert!(out.fired_exec.contains("on_error"));
	}

	#[tokio::test]
	async fn voicevox_unreachable_endpoint_fires_on_error() {
		let n = TtsSpeakNode;
		let mut ctx = ExecCtx::default();
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let mut inputs = inputs_with("voicevox", "こんにちは");
		inputs.insert("voice".into(), SocketValue::String("1".into()));
		inputs.insert("endpoint".into(), SocketValue::String("http://127.0.0.1:1".into()));
		let out = n.execute(&mut ctx, &InputMap::new(), &inputs, &fired).await.unwrap();
		assert!(out.fired_exec.contains("on_error"));
		let err = out
			.data
			.get("error")
			.and_then(|v| v.as_str().ok().map(str::to_owned))
			.unwrap_or_default();
		assert!(err.contains("[tts.voicevox]"));
	}
}

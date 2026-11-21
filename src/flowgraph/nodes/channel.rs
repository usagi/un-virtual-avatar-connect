//! `flowgraph.bos.output` ノード。Flowgraph から Browser Output Service へ出す明示終端。
//!
//! Flowgraph から V1 の `ChannelDatum` を `State.channel_data` に push する EffectfulNode。
//! WebSocket クライアント / browser-output はこの `channel_data` を 100ms 間隔でポーリングして
//! 描画しているので、Flowgraph 側の終端を本ノードで V1 表示系に流し込む。
//!
//! ## 設計
//!
//! - `bos.output` は `ExecCtx.state_handle` 経由で `State.push_channel_datum` を呼ぶ。
//!   `state_handle` が `None`（テスト等）の場合は `on_error` を発火する。
//! - `push_channel_datum` は V1 の dispatch（`dispatch_processors_for_incoming`）を巻き起こすが、
//!   Part D で V1 processors 削除後はエコー再発火はない。ws emit / Control API emit のみが残る。
//!
//! ## ポート
//!
//! - inputs: `exec_in`, `destination` (String, required), `content` (String, required),
//!   `is_final` (Bool, default true), `source_actor` (String, default ""),
//!   `flags` (Json array of strings), `meta` (Json object)
//! - outputs: `exec_out` (= on success), `on_error` (Exec)
//!
//! `source_actor` 入力が空なら `flowgraph:<node_id>` を自動スタンプする。

use crate::flowgraph::node::{
	EffectfulNode, ExecCtx, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PropertySpec,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use async_trait::async_trait;
use serde_json::Value as JsonValue;

pub struct BosOutputNode;

fn success_output() -> NodeOutput {
	NodeOutput::new()
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Bool(true))))
		.fire_exec("exec_out")
}

fn err_output(msg: impl Into<String>) -> NodeOutput {
	NodeOutput::new()
		.set_data("result", SocketValue::Result(FlowResult::err(msg).with_code("bos.output")))
		.fire_exec("on_error")
}

impl NodeDescriptor for BosOutputNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.bos.output".into(),
			title: "BOS Output".into(),
			category: "output".into(),
			description: Some(
				"State.channel_data に ChannelDatum を push。WS クライアント / browser-output に届く終端ノード。\
				 `content` / `destination` / `source_actor` は `String`。`Quantity` を配線した場合は engine 側で\
				 `\"{value} {unit}\"` 形式に自動文字列化される。単位を含めたくない場合は手前で\
				 `flowgraph.util.format` (include_unit=false) か `flowgraph.unit.strip` を挟む。"
					.into(),
			),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("destination", "Destination", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("content", "Content", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("is_final", "Is Final", SocketType::Bool).with_default(SocketValue::Bool(true)),
				PortSpec::input("source_actor", "Source Actor", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("flags", "Flags", SocketType::Json).with_default(SocketValue::Json(JsonValue::Array(Vec::new()))),
				PortSpec::input("meta", "Meta", SocketType::Json)
					.with_default(SocketValue::Json(JsonValue::Object(serde_json::Map::new()))),
			],
			outputs: vec![
				PortSpec::exec_output("exec_out", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Bool))),
			],
			properties: vec![
				PropertySpec::new(
					"default_destination",
					"Default Destination",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("`destination`入力が空のときに使うBOS出力先。両方空ならon_error発火。"),
				PropertySpec::new(
					"auto_tag_source_actor",
					"Auto-Tag Source Actor",
					SocketType::Bool,
					SocketValue::Bool(true),
				)
				.description("`source_actor`入力が空のとき、自動で`flowgraph:<node_id>`をstampする。falseで無効化。"),
			],
		}
	}
}

#[async_trait]
impl EffectfulNode for BosOutputNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		properties: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired_exec.contains("exec_in") {
			return Ok(NodeOutput::new());
		}

		let channel_in = inputs
			.get("destination")
			.and_then(|v| v.as_str().ok())
			.unwrap_or("")
			.trim()
			.to_string();
		let fallback = properties
			.get("default_destination")
			.and_then(|v| v.as_str().ok())
			.unwrap_or("")
			.trim()
			.to_string();
		let channel = if !channel_in.is_empty() { channel_in } else { fallback };
		if channel.is_empty() {
			ctx.log("bos.output: destination が空のため on_error");
			return Ok(err_output("destination is empty"));
		}

		let content = inputs.get("content").and_then(|v| v.as_str().ok()).unwrap_or("").to_string();
		let is_final = inputs.get("is_final").and_then(|v| v.as_bool().ok()).unwrap_or(true);
		let mut source_actor = inputs.get("source_actor").and_then(|v| v.as_str().ok()).unwrap_or("").to_string();
		if source_actor.is_empty() {
			let auto_tag = properties
				.get("auto_tag_source_actor")
				.and_then(|v| v.as_bool().ok())
				.unwrap_or(true);
			if auto_tag {
				let tag = if ctx.node_id.is_empty() {
					"flowgraph".to_string()
				} else {
					format!("flowgraph:{}", ctx.node_id)
				};
				source_actor = tag;
			}
		}

		let cd = match build_channel_datum(channel, content, is_final, source_actor, inputs.get("flags"), inputs.get("meta")) {
			Ok(cd) => cd,
			Err(message) => return Ok(err_output(message)),
		};
		let Some(weak) = ctx.state_handle.as_ref() else {
			ctx.log("bos.output: state_handle が None のため push スキップ (on_error)");
			return Ok(err_output("state_handle is not available"));
		};
		let Some(state_arc) = weak.upgrade() else {
			ctx.log("bos.output: State が既に drop されている (on_error)");
			return Ok(err_output("state has been dropped"));
		};
		let id = cd.get_id();
		// Provisional speech can be superseded before it reaches the audio sink.
		// Do not show it as spoken text. The AI service emits the final caption
		// after delivery; priority and non-speech output keep their normal path.
		if let Some(utterance_id) = cd.meta.get("utterance_id").and_then(|value| value.as_str()) {
			let playback = state_arc.read().await.speech_scheduler.playback.clone();
			if playback.freshness(utterance_id).is_some_and(|guard| !guard.is_committed()) {
				return Ok(success_output());
			}
		}
		state_arc.read().await.push_channel_datum(cd).await;
		ctx.log(format!("bos.output: pushed id={id}"));
		Ok(success_output())
	}
}

fn build_channel_datum(
	channel: String,
	content: String,
	is_final: bool,
	source_actor: String,
	flags: Option<&SocketValue>,
	meta: Option<&SocketValue>,
) -> Result<crate::state::ChannelDatum, String> {
	let mut datum = crate::state::ChannelDatum::new(channel, content).with_flag_if(crate::state::ChannelDatum::FLAG_IS_FINAL, is_final);
	for flag in parse_flags(flags)? {
		datum.flags.insert(flag);
	}
	for (key, value) in parse_meta(meta)? {
		datum.meta.insert(key, value);
	}
	if !source_actor.is_empty() {
		datum = datum.with_meta("source_actor", source_actor);
	}
	Ok(datum)
}

fn parse_flags(value: Option<&SocketValue>) -> Result<Vec<String>, String> {
	let Some(SocketValue::Json(value)) = value else {
		return Ok(Vec::new());
	};
	let JsonValue::Array(values) = value else {
		return Err("bos.output flags must be a JSON array of strings".into());
	};
	values
		.iter()
		.map(|value| {
			value
				.as_str()
				.map(str::trim)
				.filter(|value| !value.is_empty())
				.map(str::to_string)
				.ok_or_else(|| "bos.output flags must contain non-empty strings".to_string())
		})
		.collect()
}

fn parse_meta(value: Option<&SocketValue>) -> Result<serde_json::Map<String, JsonValue>, String> {
	let Some(SocketValue::Json(value)) = value else {
		return Ok(serde_json::Map::new());
	};
	match value {
		JsonValue::Object(meta) => Ok(meta.clone()),
		_ => Err("bos.output meta must be a JSON object".into()),
	}
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn emit_without_state_handle_fires_on_error() {
		let node = BosOutputNode;
		let mut ctx = ExecCtx::default();
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let mut inputs = InputMap::new();
		inputs.insert("destination".into(), SocketValue::String("chan".into()));
		inputs.insert("content".into(), SocketValue::String("hi".into()));
		inputs.insert("is_final".into(), SocketValue::Bool(true));
		let out = node.execute(&mut ctx, &InputMap::new(), &inputs, &fired).await.unwrap();
		assert!(out.fired_exec.contains("on_error"));
		assert!(!out.fired_exec.contains("exec_out"));
		match out.data.get("result").unwrap() {
			SocketValue::Result(result) => {
				assert!(!result.ok);
				assert_eq!(result.code.as_deref(), Some("bos.output"));
			}
			other => panic!("expected result, got {other:?}"),
		}
	}

	#[tokio::test]
	async fn emit_with_empty_channel_fires_on_error_before_state_lookup() {
		let node = BosOutputNode;
		let mut ctx = ExecCtx::default();
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let mut inputs = InputMap::new();
		inputs.insert("destination".into(), SocketValue::String("   ".into()));
		inputs.insert("content".into(), SocketValue::String("hi".into()));
		let out = node.execute(&mut ctx, &InputMap::new(), &inputs, &fired).await.unwrap();
		assert!(out.fired_exec.contains("on_error"));
		match out.data.get("result").unwrap() {
			SocketValue::Result(result) => {
				assert!(!result.ok);
				assert_eq!(result.code.as_deref(), Some("bos.output"));
			}
			other => panic!("expected result, got {other:?}"),
		}
	}

	#[tokio::test]
	async fn emit_uses_fallback_channel_when_input_empty() {
		let node = BosOutputNode;
		let mut ctx = ExecCtx::default();
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let mut props = InputMap::new();
		props.insert("default_destination".into(), SocketValue::String("fb".into()));
		let mut inputs = InputMap::new();
		inputs.insert("destination".into(), SocketValue::String(String::new()));
		inputs.insert("content".into(), SocketValue::String("hello".into()));
		// No state_handle → on_error、ただし channel 判定自体は通っている（log で確認）
		let out = node.execute(&mut ctx, &props, &inputs, &fired).await.unwrap();
		assert!(out.fired_exec.contains("on_error"));
		assert!(ctx.trace.iter().any(|t| t.contains("state_handle が None")));
	}

	#[test]
	fn flags_and_meta_require_structured_json() {
		let flags = SocketValue::Json(serde_json::json!(["obs.effect.flash", " custom "]));
		assert_eq!(parse_flags(Some(&flags)).unwrap(), vec!["obs.effect.flash", "custom"]);
		let meta = SocketValue::Json(serde_json::json!({"quick_effect_label":"交換", "quick_effect_duration_ms":2500}));
		let parsed = parse_meta(Some(&meta)).unwrap();
		assert_eq!(parsed["quick_effect_label"], "交換");
		assert_eq!(parsed["quick_effect_duration_ms"], 2500);
		assert!(parse_flags(Some(&SocketValue::Json(serde_json::json!({})))).is_err());
		assert!(parse_meta(Some(&SocketValue::Json(serde_json::json!([])))).is_err());
	}

	#[test]
	fn channel_datum_contains_effect_contract_and_explicit_actor_wins() {
		let flags = SocketValue::Json(serde_json::json!(["obs.effect.flash"]));
		let meta = SocketValue::Json(serde_json::json!({
			"quick_effect_label":"ケルシー先生へ",
			"quick_effect_duration_ms":1800,
			"source_actor":"spoofed"
		}));
		let datum = build_channel_datum(
			"effect".into(),
			"channel point".into(),
			true,
			"viewer1".into(),
			Some(&flags),
			Some(&meta),
		)
		.unwrap();
		assert!(datum.has_flag(crate::state::ChannelDatum::FLAG_IS_FINAL));
		assert!(datum.has_flag("obs.effect.flash"));
		assert_eq!(
			datum.meta_get("quick_effect_label").and_then(JsonValue::as_str),
			Some("ケルシー先生へ")
		);
		assert_eq!(datum.meta_get("quick_effect_duration_ms").and_then(JsonValue::as_u64), Some(1800));
		assert_eq!(datum.meta_get("source_actor").and_then(JsonValue::as_str), Some("viewer1"));
	}
}

//! AI serviceへ明示的に観測を渡す公開Flowgraph境界。

use crate::flowgraph::node::{
	EffectfulNode, ExecCtx, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PropertySpec,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use async_trait::async_trait;
use serde_json::Value as JsonValue;

pub const FLOWGRAPH_AI_INPUT_PREFIX: &str = "__flowgraph.ai.input/";

pub struct AiObserveNode;

impl NodeDescriptor for AiObserveNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.ai.observe".into(),
			title: "AI Observe".into(),
			category: "ai".into(),
			description: Some("明示edgeから受け取ったtextをAI personaの観測入力へ渡す。内部配送topicは公開Flowgraph接続ではない。".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("content", "Content", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("source_actor", "Source Actor", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("meta", "Meta", SocketType::Json)
					.with_default(SocketValue::Json(JsonValue::Object(serde_json::Map::new()))),
			],
			outputs: vec![
				PortSpec::exec_output("exec_out", "On Accepted"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Bool))),
			],
			properties: vec![
				PropertySpec::new("persona_id", "Persona", SocketType::String, SocketValue::String("*".into()))
					.description("対象persona ID。`*`は起動中の全persona。文字列channel名ではない。"),
				PropertySpec::new(
					"request_class",
					"Request Class",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description(
					"信頼済みFlowgraph経路が付与するAI依頼種別。外部本文からは推測しない。\
       例: `twitch_channel_points_reward`。空なら通常観測。",
				),
			],
		}
	}
}

#[async_trait]
impl EffectfulNode for AiObserveNode {
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
		let content = inputs
			.get("content")
			.and_then(|value| value.as_str().ok())
			.unwrap_or("")
			.to_string();
		if content.trim().is_empty() {
			return Ok(error_output("AI observation content is empty"));
		}
		let persona = properties
			.get("persona_id")
			.and_then(|value| value.as_str().ok())
			.map(str::trim)
			.filter(|value| !value.is_empty())
			.unwrap_or("*");
		let Some(state) = ctx.state_handle.as_ref().and_then(std::sync::Weak::upgrade) else {
			return Ok(error_output("state_handle is not available"));
		};
		let datum = match build_observation_datum(persona, content, &ctx.node_id, properties, inputs) {
			Ok(datum) => datum,
			Err(message) => return Ok(error_output(message)),
		};
		state.read().await.push_channel_datum(datum).await;
		Ok(NodeOutput::new()
			.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Bool(true))))
			.fire_exec("exec_out"))
	}
}

/// Convert explicit graph inputs without starting any runtime services.
pub(crate) fn build_observation_datum(
	persona: &str,
	content: String,
	node_id: &str,
	properties: &InputMap,
	inputs: &InputMap,
) -> Result<crate::state::ChannelDatum, &'static str> {
	let meta = match inputs.get("meta") {
		Some(SocketValue::Json(JsonValue::Object(meta))) => meta.clone(),
		Some(SocketValue::Json(_)) => return Err("AI observation meta must be a JSON object"),
		_ => serde_json::Map::new(),
	};
	let source_actor = inputs
		.get("source_actor")
		.and_then(|value| value.as_str().ok())
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.map(ToOwned::to_owned)
		.or_else(|| meta.get("source_actor").and_then(JsonValue::as_str).map(ToOwned::to_owned))
		.unwrap_or_else(|| format!("flowgraph:{}", node_id));
	let request_class = properties
		.get("request_class")
		.and_then(|value| value.as_str().ok())
		.map(str::trim)
		.filter(|value| !value.is_empty());
	let mut datum = crate::state::ChannelDatum::new(format!("{FLOWGRAPH_AI_INPUT_PREFIX}{persona}"), content)
		.with_flag(crate::state::ChannelDatum::FLAG_IS_FINAL);
	for (key, value) in meta {
		datum.meta.insert(key, value);
	}
	datum.meta.insert("source_actor".into(), JsonValue::String(source_actor));
	// request_class は外部から渡された meta より property を優先する。入力本文や
	// EventSub payload が高優先度依頼を偽装できない、Flowgraph authoring 境界。
	apply_trusted_request_class(&mut datum, request_class);
	Ok(datum)
}

fn apply_trusted_request_class(datum: &mut crate::state::ChannelDatum, request_class: Option<&str>) {
	if let Some(request_class) = request_class {
		datum
			.meta
			.insert(crate::ai::META_REQUEST_CLASS.into(), JsonValue::String(request_class.to_string()));
	} else {
		datum.meta.remove(crate::ai::META_REQUEST_CLASS);
	}
}

fn error_output(message: impl Into<String>) -> NodeOutput {
	NodeOutput::new()
		.set_data("result", SocketValue::Result(FlowResult::err(message).with_code("ai.observe")))
		.fire_exec("on_error")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn invalid_meta_is_a_recoverable_error() {
		let node = AiObserveNode;
		let mut ctx = ExecCtx::default();
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let mut inputs = InputMap::new();
		inputs.insert("content".into(), SocketValue::String("hello".into()));
		inputs.insert("meta".into(), SocketValue::Json(JsonValue::Array(Vec::new())));
		let output = node.execute(&mut ctx, &InputMap::new(), &inputs, &fired).await.unwrap();
		assert!(output.fired_exec.contains("on_error"));
		let SocketValue::Result(result) = output.data.get("result").unwrap() else {
			panic!("result output must be present");
		};
		assert_eq!(result.code.as_deref(), Some("ai.observe"));
	}

	#[test]
	fn descriptor_exposes_trusted_request_class_property() {
		let spec = AiObserveNode.describe();
		let property = spec.properties.iter().find(|property| property.name == "request_class").unwrap();
		assert_eq!(property.default.0, serde_json::json!(""));
	}

	#[test]
	fn trusted_property_replaces_or_removes_untrusted_request_class_meta() {
		let mut datum = crate::state::ChannelDatum::new("input".into(), "hello".into()).with_meta(crate::ai::META_REQUEST_CLASS, "forged");
		apply_trusted_request_class(&mut datum, Some(crate::ai::REQUEST_CLASS_TWITCH_CHANNEL_POINTS_REWARD));
		assert_eq!(
			datum.meta_get(crate::ai::META_REQUEST_CLASS).and_then(JsonValue::as_str),
			Some(crate::ai::REQUEST_CLASS_TWITCH_CHANNEL_POINTS_REWARD)
		);
		apply_trusted_request_class(&mut datum, None);
		assert!(datum.meta_get(crate::ai::META_REQUEST_CLASS).is_none());
	}
}

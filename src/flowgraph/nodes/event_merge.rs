//! Typed event fan-in. Only the fired branch is demanded; no synchronization,
//! batching or downstream state duplication occurs.
use crate::flowgraph::node::{
	EffectfulNode, ExecCtx, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec,
};
use crate::flowgraph::socket::SocketType;
use async_trait::async_trait;

pub struct EventMergeNode {
	feature: String,
	ty: SocketType,
}

impl EventMergeNode {
	pub fn new(suffix: &str, ty: SocketType) -> Self {
		Self {
			feature: format!("flowgraph.flow.merge_{suffix}"),
			ty,
		}
	}

	fn selected(fired: &ExecFireSet) -> Result<Option<&'static str>, NodeExecError> {
		match (fired.contains("exec_a"), fired.contains("exec_b")) {
			(true, false) => Ok(Some("value_a")),
			(false, true) => Ok(Some("value_b")),
			(false, false) => Ok(None),
			(true, true) => Err(NodeExecError::Generic(anyhow::anyhow!(
				"event merge requires one branch per event; exec_a and exec_b fired together"
			))),
		}
	}
}

impl NodeDescriptor for EventMergeNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: self.feature.clone(),
			title: format!("Merge Events ({})", self.ty),
			category: "flow".into(),
			description: Some(
				"発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。".into(),
			),
			inputs: vec![
				PortSpec::exec_input("exec_a", "Event A"),
				PortSpec::input("value_a", "Value A", self.ty.clone()),
				PortSpec::exec_input("exec_b", "Event B"),
				PortSpec::input("value_b", "Value B", self.ty.clone()),
			],
			outputs: vec![
				PortSpec::exec_output("exec_out", "Event"),
				PortSpec::output("value", "Value", self.ty.clone()),
			],
			properties: vec![],
		}
	}

	fn input_is_active(&self, port: &str, fired: &ExecFireSet) -> Result<bool, NodeExecError> {
		Ok(Self::selected(fired)? == Some(port))
	}
}

#[async_trait]
impl EffectfulNode for EventMergeNode {
	async fn execute(
		&self,
		_ctx: &mut ExecCtx,
		_properties: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let Some(port) = Self::selected(fired)? else {
			return Ok(NodeOutput::new());
		};
		let value = inputs
			.get(port)
			.cloned()
			.ok_or_else(|| NodeExecError::MissingRequiredInput(port.into()))?;
		Ok(NodeOutput::new().set_data("value", value).fire_exec("exec_out"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::{
		engine::{FlowgraphBuilder, PortRef, ProgramRun},
		node::{NodeImpl, TriggerEvent},
		socket::SocketValue,
	};
	use std::sync::Arc;

	// A source that only has a value after an explicit event, like a file/API result.
	struct Producer;
	impl NodeDescriptor for Producer {
		fn describe(&self) -> NodeSpec {
			NodeSpec {
				feature: "test.producer".into(),
				title: "Producer".into(),
				category: "test".into(),
				description: None,
				inputs: vec![
					PortSpec::exec_input("exec_in", "Exec"),
					PortSpec::input("input", "Input", SocketType::String),
				],
				outputs: vec![
					PortSpec::exec_output("exec_out", "Exec"),
					PortSpec::output("value", "Value", SocketType::String),
				],
				properties: vec![],
			}
		}
	}
	#[async_trait]
	impl EffectfulNode for Producer {
		async fn execute(&self, _: &mut ExecCtx, _: &InputMap, inputs: &InputMap, _: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
			Ok(NodeOutput::new().set_data("value", inputs["input"].clone()).fire_exec("exec_out"))
		}
	}

	#[tokio::test]
	async fn merge_rejects_ambiguous_events_and_requires_selected_value() {
		let node = EventMergeNode::new("string", SocketType::String);
		let mut both = ExecFireSet::new(); both.insert("exec_a"); both.insert("exec_b");
		assert!(node.input_is_active("value_a", &both).is_err());
		assert!(node
			.execute(&mut ExecCtx::default(), &InputMap::new(), &InputMap::new(), &both)
			.await
			.is_err());
		let mut one = ExecFireSet::new(); one.insert("exec_a");
		assert!(node
			.execute(&mut ExecCtx::default(), &InputMap::new(), &InputMap::new(), &one)
			.await
			.is_err());
		assert!(!node.input_is_active("value_b", &one).unwrap());
	}

	#[tokio::test]
	async fn chained_merge_does_not_pull_unfired_sources() {
		let mut b = FlowgraphBuilder::new();
		for id in ["a", "b", "c"] {
			b.add_node(id, NodeImpl::effectful(Arc::new(Producer)), InputMap::new());
		}
		for id in ["m1", "m2"] {
			b.add_node(
				id,
				NodeImpl::effectful(Arc::new(EventMergeNode::new("string", SocketType::String))),
				InputMap::new(),
			);
		}
		b.add_node(
			"log",
			NodeImpl::effectful(Arc::new(crate::flowgraph::nodes::log::LogNode)),
			InputMap::new(),
		);
		for (src, dst, branch) in [("a", "m1", "a"), ("b", "m1", "b"), ("m1", "m2", "a"), ("c", "m2", "b")] {
			b.connect(PortRef::new(src, "exec_out"), PortRef::new(dst, format!("exec_{branch}")));
			b.connect(PortRef::new(src, "value"), PortRef::new(dst, format!("value_{branch}")));
		}
		b.connect(PortRef::new("m2", "exec_out"), PortRef::new("log", "exec_in"));
		b.connect(PortRef::new("m2", "value"), PortRef::new("log", "value"));
		let mut program = b.build().unwrap();
		let mut ctx = ExecCtx::default();
		// Use the public trigger bus; producers require exec events and no external services.
		let (tx, rx) = crate::flowgraph::engine::create_trigger_bus();
		for (id, text) in [("a", "first"), ("c", "second"), ("b", "third"), ("a", "fourth")] {
			tx.send(
				TriggerEvent::new(id)
					.with_exec("exec_in")
					.with_override("input", SocketValue::String(text.into())),
			)
			.unwrap();
		}
		let _run: ProgramRun = program
			.run_forever_with_bus(&mut ctx, tx, rx, tokio::time::sleep(std::time::Duration::from_millis(50)), None)
			.await
			.unwrap();
		assert_eq!(ctx.trace, ["log: first", "log: second", "log: third", "log: fourth"]);
	}
}

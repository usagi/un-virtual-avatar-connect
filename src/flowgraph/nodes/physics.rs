//! Audio / physics utility nodes for Phase omega.
//!
//! First slice: a scalar spring node. Vec2 / Vec3 spring variants can reuse the
//! same integration rule once UI and type surface are settled.

use crate::flowgraph::node::{
	get_optional_float, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PureNode, StatefulCtx,
	StatefulNode,
};
use crate::flowgraph::socket::{SocketType, SocketValue};
use crate::flowgraph::FlowgraphStateModel;
use async_trait::async_trait;
use serde_json::Value as JsonValue;
use std::any::Any;

pub struct PhysicsSpringNode;
pub struct PhysicsDamperNode;
pub struct PhysicsGravityNode;
pub struct PhysicsIntegratorNode;

#[derive(Default)]
pub struct PhysicsSpringState {
	value: f64,
	velocity: f64,
	initialized: bool,
}

#[derive(Default)]
pub struct PhysicsIntegratorState {
	value: f64,
	initialized: bool,
}

fn finite_port(name: &str, value: f64) -> Result<f64, NodeExecError> {
	if value.is_finite() {
		Ok(value)
	} else {
		Err(NodeExecError::Generic(anyhow::anyhow!(
			"physics.spring: `{name}` must be finite, got {value}"
		)))
	}
}

impl NodeDescriptor for PhysicsSpringNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.physics.spring".into(),
			title: "Physics Spring".into(),
			category: "physics".into(),
			description: Some("Stateful scalar spring integrator. `step` advances value toward target.".into()),
			inputs: vec![
				PortSpec::exec_input("step", "Step"),
				PortSpec::exec_input("reset", "Reset"),
				PortSpec::input("target", "Target", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("initial", "Initial", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("dt", "Delta Time", SocketType::Float).with_default(SocketValue::Float(1.0 / 60.0)),
				PortSpec::input("stiffness", "Stiffness", SocketType::Float).with_default(SocketValue::Float(120.0)),
				PortSpec::input("damping", "Damping", SocketType::Float).with_default(SocketValue::Float(20.0)),
			],
			outputs: vec![
				PortSpec::output("value", "Value", SocketType::Float),
				PortSpec::output("velocity", "Velocity", SocketType::Float),
				PortSpec::output("initialized", "Initialized", SocketType::Bool),
				PortSpec::exec_output("updated", "Updated"),
				PortSpec::exec_output("reset_done", "Reset Done"),
			],
			properties: vec![],
		}
	}
}

impl NodeDescriptor for PhysicsDamperNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.physics.damper".into(),
			title: "Physics Damper".into(),
			category: "physics".into(),
			description: Some("Pure scalar exponential damper from current toward target.".into()),
			inputs: vec![
				PortSpec::input("current", "Current", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("target", "Target", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("dt", "Delta Time", SocketType::Float).with_default(SocketValue::Float(1.0 / 60.0)),
				PortSpec::input("damping", "Damping", SocketType::Float).with_default(SocketValue::Float(10.0)),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Float)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for PhysicsDamperNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let current = finite_port("current", get_optional_float(inputs, "current", 0.0)?)?;
		let target = finite_port("target", get_optional_float(inputs, "target", 0.0)?)?;
		let dt = finite_port("dt", get_optional_float(inputs, "dt", 1.0 / 60.0)?)?.clamp(0.0, 1.0);
		let damping = finite_port("damping", get_optional_float(inputs, "damping", 10.0)?)?.max(0.0);
		let alpha = 1.0 - (-damping * dt).exp();
		Ok(NodeOutput::new().set_data("value", SocketValue::Float(current + (target - current) * alpha)))
	}
}

impl NodeDescriptor for PhysicsGravityNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.physics.gravity".into(),
			title: "Physics Gravity".into(),
			category: "physics".into(),
			description: Some("Pure scalar velocity integration with constant acceleration.".into()),
			inputs: vec![
				PortSpec::input("velocity", "Velocity", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("gravity", "Gravity", SocketType::Float).with_default(SocketValue::Float(-9.80665)),
				PortSpec::input("dt", "Delta Time", SocketType::Float).with_default(SocketValue::Float(1.0 / 60.0)),
			],
			outputs: vec![PortSpec::output("velocity", "Velocity", SocketType::Float)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for PhysicsGravityNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let velocity = finite_port("velocity", get_optional_float(inputs, "velocity", 0.0)?)?;
		let gravity = finite_port("gravity", get_optional_float(inputs, "gravity", -9.80665)?)?;
		let dt = finite_port("dt", get_optional_float(inputs, "dt", 1.0 / 60.0)?)?.clamp(0.0, 1.0);
		Ok(NodeOutput::new().set_data("velocity", SocketValue::Float(velocity + gravity * dt)))
	}
}

impl NodeDescriptor for PhysicsIntegratorNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.physics.integrator".into(),
			title: "Physics Integrator".into(),
			category: "physics".into(),
			description: Some("Stateful scalar integrator. `step` adds velocity * dt to value.".into()),
			inputs: vec![
				PortSpec::exec_input("step", "Step"),
				PortSpec::exec_input("reset", "Reset"),
				PortSpec::input("initial", "Initial", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("velocity", "Velocity", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("dt", "Delta Time", SocketType::Float).with_default(SocketValue::Float(1.0 / 60.0)),
			],
			outputs: vec![
				PortSpec::output("value", "Value", SocketType::Float),
				PortSpec::output("initialized", "Initialized", SocketType::Bool),
				PortSpec::exec_output("updated", "Updated"),
				PortSpec::exec_output("reset_done", "Reset Done"),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl StatefulNode for PhysicsSpringNode {
	fn init_state(&self) -> Box<dyn Any + Send> {
		Box::new(PhysicsSpringState::default())
	}

	fn state_model(&self) -> FlowgraphStateModel {
		FlowgraphStateModel::volatile_node_instance_json_snapshot()
	}

	fn snapshot_state(&self, state: &(dyn Any + Send)) -> Option<JsonValue> {
		let state = state.downcast_ref::<PhysicsSpringState>()?;
		Some(serde_json::json!({
			"value": state.value,
			"velocity": state.velocity,
			"initialized": state.initialized,
		}))
	}

	fn restore_state(&self, state: &mut (dyn Any + Send), value: &JsonValue) -> Result<(), String> {
		let state = state
			.downcast_mut::<PhysicsSpringState>()
			.ok_or_else(|| "PhysicsSpringState downcast failed".to_string())?;
		state.value = value
			.get("value")
			.and_then(|value| value.as_f64())
			.ok_or_else(|| "expected object with numeric field 'value'".to_string())?;
		state.velocity = value.get("velocity").and_then(|value| value.as_f64()).unwrap_or(0.0);
		state.initialized = value.get("initialized").and_then(|value| value.as_bool()).unwrap_or(true);
		Ok(())
	}

	async fn compute(
		&self,
		state: &mut (dyn Any + Send),
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
		_ctx: &StatefulCtx<'_>,
	) -> Result<NodeOutput, NodeExecError> {
		let state = state.downcast_mut::<PhysicsSpringState>().expect("PhysicsSpringState");
		let initial = finite_port("initial", get_optional_float(inputs, "initial", 0.0)?)?;
		if !state.initialized {
			state.value = initial;
			state.velocity = 0.0;
			state.initialized = true;
		}

		let mut fire_updated = false;
		let mut fire_reset = false;
		if fired_exec.contains("reset") {
			state.value = initial;
			state.velocity = 0.0;
			state.initialized = true;
			fire_reset = true;
		} else if fired_exec.contains("step") {
			let target = finite_port("target", get_optional_float(inputs, "target", 0.0)?)?;
			let dt = finite_port("dt", get_optional_float(inputs, "dt", 1.0 / 60.0)?)?.clamp(0.0, 1.0);
			let stiffness = finite_port("stiffness", get_optional_float(inputs, "stiffness", 120.0)?)?.max(0.0);
			let damping = finite_port("damping", get_optional_float(inputs, "damping", 20.0)?)?.max(0.0);
			let acceleration = (target - state.value) * stiffness - state.velocity * damping;
			state.velocity += acceleration * dt;
			state.value += state.velocity * dt;
			fire_updated = true;
		}

		let mut out = NodeOutput::new()
			.set_data("value", SocketValue::Float(state.value))
			.set_data("velocity", SocketValue::Float(state.velocity))
			.set_data("initialized", SocketValue::Bool(state.initialized));
		if fire_updated {
			out = out.fire_exec("updated");
		}
		if fire_reset {
			out = out.fire_exec("reset_done");
		}
		Ok(out)
	}
}

#[async_trait]
impl StatefulNode for PhysicsIntegratorNode {
	fn init_state(&self) -> Box<dyn Any + Send> {
		Box::new(PhysicsIntegratorState::default())
	}

	fn state_model(&self) -> FlowgraphStateModel {
		FlowgraphStateModel::volatile_node_instance_json_snapshot()
	}

	fn snapshot_state(&self, state: &(dyn Any + Send)) -> Option<JsonValue> {
		let state = state.downcast_ref::<PhysicsIntegratorState>()?;
		Some(serde_json::json!({
			"value": state.value,
			"initialized": state.initialized,
		}))
	}

	fn restore_state(&self, state: &mut (dyn Any + Send), value: &JsonValue) -> Result<(), String> {
		let state = state
			.downcast_mut::<PhysicsIntegratorState>()
			.ok_or_else(|| "PhysicsIntegratorState downcast failed".to_string())?;
		state.value = value
			.get("value")
			.and_then(|value| value.as_f64())
			.ok_or_else(|| "expected object with numeric field 'value'".to_string())?;
		state.initialized = value.get("initialized").and_then(|value| value.as_bool()).unwrap_or(true);
		Ok(())
	}

	async fn compute(
		&self,
		state: &mut (dyn Any + Send),
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
		_ctx: &StatefulCtx<'_>,
	) -> Result<NodeOutput, NodeExecError> {
		let state = state.downcast_mut::<PhysicsIntegratorState>().expect("PhysicsIntegratorState");
		let initial = finite_port("initial", get_optional_float(inputs, "initial", 0.0)?)?;
		if !state.initialized {
			state.value = initial;
			state.initialized = true;
		}

		let mut fire_updated = false;
		let mut fire_reset = false;
		if fired_exec.contains("reset") {
			state.value = initial;
			state.initialized = true;
			fire_reset = true;
		} else if fired_exec.contains("step") {
			let velocity = finite_port("velocity", get_optional_float(inputs, "velocity", 0.0)?)?;
			let dt = finite_port("dt", get_optional_float(inputs, "dt", 1.0 / 60.0)?)?.clamp(0.0, 1.0);
			state.value += velocity * dt;
			fire_updated = true;
		}

		let mut out = NodeOutput::new()
			.set_data("value", SocketValue::Float(state.value))
			.set_data("initialized", SocketValue::Bool(state.initialized));
		if fire_updated {
			out = out.fire_exec("updated");
		}
		if fire_reset {
			out = out.fire_exec("reset_done");
		}
		Ok(out)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fire(port: &str) -> ExecFireSet {
		let mut f = ExecFireSet::new();
		f.insert(port);
		f
	}

	fn sctx<'a>() -> StatefulCtx<'a> {
		StatefulCtx {
			node_id: "n",
			trigger: None,
		}
	}

	#[tokio::test]
	async fn physics_spring_steps_toward_target_and_snapshots() {
		let node = PhysicsSpringNode;
		let mut state = node.init_state();
		let sctx = sctx();
		let inputs: InputMap = [
			("initial".into(), SocketValue::Float(0.0)),
			("target".into(), SocketValue::Float(10.0)),
			("dt".into(), SocketValue::Float(0.1)),
			("stiffness".into(), SocketValue::Float(20.0)),
			("damping".into(), SocketValue::Float(4.0)),
		]
		.into_iter()
		.collect();

		let out = node
			.compute(state.as_mut(), &InputMap::new(), &inputs, &fire("step"), &sctx)
			.await
			.unwrap();
		assert!(out.fired_exec.contains("updated"));
		let value = out.data.get("value").unwrap().as_f64().unwrap();
		let velocity = out.data.get("velocity").unwrap().as_f64().unwrap();
		assert!(value > 0.0 && value < 10.0, "value={value}");
		assert!(velocity > 0.0, "velocity={velocity}");
		assert_eq!(out.data.get("initialized"), Some(&SocketValue::Bool(true)));
		assert!(node.snapshot_state(state.as_ref()).unwrap().get("value").is_some());
	}

	#[tokio::test]
	async fn physics_spring_reset_restores_initial() {
		let node = PhysicsSpringNode;
		let mut state = node.init_state();
		let sctx = sctx();
		let inputs: InputMap = [
			("initial".into(), SocketValue::Float(2.5)),
			("target".into(), SocketValue::Float(10.0)),
			("dt".into(), SocketValue::Float(0.1)),
		]
		.into_iter()
		.collect();
		node.compute(state.as_mut(), &InputMap::new(), &inputs, &fire("step"), &sctx)
			.await
			.unwrap();
		let out = node
			.compute(state.as_mut(), &InputMap::new(), &inputs, &fire("reset"), &sctx)
			.await
			.unwrap();
		assert!(out.fired_exec.contains("reset_done"));
		assert_eq!(out.data.get("value"), Some(&SocketValue::Float(2.5)));
		assert_eq!(out.data.get("velocity"), Some(&SocketValue::Float(0.0)));
	}

	#[tokio::test]
	async fn physics_damper_moves_toward_target() {
		let node = PhysicsDamperNode;
		let inputs: InputMap = [
			("current".into(), SocketValue::Float(0.0)),
			("target".into(), SocketValue::Float(10.0)),
			("dt".into(), SocketValue::Float(0.1)),
			("damping".into(), SocketValue::Float(10.0)),
		]
		.into_iter()
		.collect();
		let out = node
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let value = out.data.get("value").unwrap().as_f64().unwrap();
		assert!(value > 0.0 && value < 10.0, "value={value}");
	}

	#[tokio::test]
	async fn physics_gravity_integrates_velocity() {
		let node = PhysicsGravityNode;
		let inputs: InputMap = [
			("velocity".into(), SocketValue::Float(1.0)),
			("gravity".into(), SocketValue::Float(-10.0)),
			("dt".into(), SocketValue::Float(0.5)),
		]
		.into_iter()
		.collect();
		let out = node
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("velocity"), Some(&SocketValue::Float(-4.0)));
	}

	#[tokio::test]
	async fn physics_integrator_steps_and_resets() {
		let node = PhysicsIntegratorNode;
		let mut state = node.init_state();
		let sctx = sctx();
		let inputs: InputMap = [
			("initial".into(), SocketValue::Float(2.0)),
			("velocity".into(), SocketValue::Float(3.0)),
			("dt".into(), SocketValue::Float(0.5)),
		]
		.into_iter()
		.collect();
		let out = node
			.compute(state.as_mut(), &InputMap::new(), &inputs, &fire("step"), &sctx)
			.await
			.unwrap();
		assert!(out.fired_exec.contains("updated"));
		assert_eq!(out.data.get("value"), Some(&SocketValue::Float(3.5)));
		assert!(node.snapshot_state(state.as_ref()).unwrap().get("value").is_some());

		let out = node
			.compute(state.as_mut(), &InputMap::new(), &inputs, &fire("reset"), &sctx)
			.await
			.unwrap();
		assert!(out.fired_exec.contains("reset_done"));
		assert_eq!(out.data.get("value"), Some(&SocketValue::Float(2.0)));
	}
}

//! Stateful collection / buffer nodes for Flowgraph stdlib SL-3.
//!
//! These nodes keep worker-lifetime state in the graph engine. Snapshot / restore
//! uses JSON so they can participate in the existing LF-7 state contract.

use crate::flowgraph::node::{
	get_optional_float, get_optional_int, get_optional_string, get_required_json, ExecFireSet, InputMap, NodeDescriptor, NodeExecError,
	NodeOutput, NodeSpec, PortSpec, StatefulCtx, StatefulNode,
};
use crate::flowgraph::socket::{SocketType, SocketValue};
use crate::flowgraph::FlowgraphStateModel;
use async_trait::async_trait;
use serde_json::Value as JsonValue;
use std::any::Any;
use std::collections::VecDeque;

const MAX_BUFFER_CAPACITY: usize = 10_000;

fn capacity(inputs: &InputMap, default: i64) -> Result<usize, NodeExecError> {
	Ok(get_optional_int(inputs, "capacity", default)?.clamp(1, MAX_BUFFER_CAPACITY as i64) as usize)
}

fn json_list(items: impl Iterator<Item = JsonValue>) -> SocketValue {
	SocketValue::List(items.map(SocketValue::Json).collect())
}

fn hash_index(seed: &str, seen: i64) -> u64 {
	let mut hasher = blake3::Hasher::new();
	hasher.update(seed.as_bytes());
	hasher.update(&seen.to_le_bytes());
	let digest = hasher.finalize();
	let mut bytes = [0u8; 8];
	bytes.copy_from_slice(&digest.as_bytes()[..8]);
	u64::from_le_bytes(bytes)
}

// ---------------------------------------------------------------------
// collection.ring_buffer
// ---------------------------------------------------------------------

pub struct RingBufferNode;

#[derive(Default)]
pub struct RingBufferState {
	items: VecDeque<JsonValue>,
	dropped_count: i64,
}

impl NodeDescriptor for RingBufferNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.collection.ring_buffer".into(),
			title: "Ring Buffer".into(),
			category: "collection".into(),
			description: Some("Stateful fixed-capacity JSON ring buffer. push drops oldest when full.".into()),
			inputs: vec![
				PortSpec::exec_input("push", "Push"),
				PortSpec::exec_input("clear", "Clear"),
				PortSpec::input("item", "Item", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("capacity", "Capacity", SocketType::Int).with_default(SocketValue::Int(32)),
			],
			outputs: vec![
				PortSpec::output("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
				PortSpec::output("dropped_count", "Dropped Count", SocketType::Int),
				PortSpec::output("latest", "Latest", SocketType::Json),
				PortSpec::exec_output("updated", "Updated"),
				PortSpec::exec_output("cleared", "Cleared"),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl StatefulNode for RingBufferNode {
	fn init_state(&self) -> Box<dyn Any + Send> {
		Box::new(RingBufferState::default())
	}

	fn state_model(&self) -> FlowgraphStateModel {
		FlowgraphStateModel::volatile_node_instance_json_snapshot()
	}

	fn snapshot_state(&self, state: &(dyn Any + Send)) -> Option<JsonValue> {
		let state = state.downcast_ref::<RingBufferState>()?;
		Some(serde_json::json!({
			"items": state.items.iter().cloned().collect::<Vec<_>>(),
			"dropped_count": state.dropped_count,
		}))
	}

	fn restore_state(&self, state: &mut (dyn Any + Send), value: &JsonValue) -> Result<(), String> {
		let state = state
			.downcast_mut::<RingBufferState>()
			.ok_or_else(|| "RingBufferState downcast failed".to_string())?;
		let items = value
			.get("items")
			.and_then(|value| value.as_array())
			.ok_or_else(|| "expected object with array field 'items'".to_string())?;
		state.items = items.iter().cloned().collect();
		state.dropped_count = value.get("dropped_count").and_then(|value| value.as_i64()).unwrap_or(0);
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
		let state = state.downcast_mut::<RingBufferState>().expect("RingBufferState");
		let capacity = capacity(inputs, 32)?;
		let mut fire_updated = false;
		let mut fire_cleared = false;

		while state.items.len() > capacity {
			state.items.pop_front();
			state.dropped_count += 1;
		}
		if fired_exec.contains("clear") && !state.items.is_empty() {
			state.items.clear();
			fire_cleared = true;
		}
		if fired_exec.contains("push") {
			if state.items.len() >= capacity {
				state.items.pop_front();
				state.dropped_count += 1;
			}
			state.items.push_back(get_required_json(inputs, "item")?.clone());
			fire_updated = true;
		}

		let latest = state.items.back().cloned().unwrap_or(JsonValue::Null);
		let mut out = NodeOutput::new()
			.set_data("items", json_list(state.items.iter().cloned()))
			.set_data("count", SocketValue::Int(state.items.len() as i64))
			.set_data("dropped_count", SocketValue::Int(state.dropped_count))
			.set_data("latest", SocketValue::Json(latest));
		if fire_updated {
			out = out.fire_exec("updated");
		}
		if fire_cleared {
			out = out.fire_exec("cleared");
		}
		Ok(out)
	}
}

// ---------------------------------------------------------------------
// collection.deque
// ---------------------------------------------------------------------

pub struct DequeNode;

#[derive(Default)]
pub struct DequeState {
	items: VecDeque<JsonValue>,
	last_popped: JsonValue,
	has_popped: bool,
}

impl NodeDescriptor for DequeNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.collection.deque".into(),
			title: "Deque".into(),
			category: "collection".into(),
			description: Some("Stateful double-ended JSON queue with bounded capacity.".into()),
			inputs: vec![
				PortSpec::exec_input("push_back", "Push Back"),
				PortSpec::exec_input("push_front", "Push Front"),
				PortSpec::exec_input("pop_back", "Pop Back"),
				PortSpec::exec_input("pop_front", "Pop Front"),
				PortSpec::exec_input("clear", "Clear"),
				PortSpec::input("item", "Item", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("capacity", "Capacity", SocketType::Int).with_default(SocketValue::Int(1000)),
			],
			outputs: vec![
				PortSpec::output("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
				PortSpec::output("popped", "Popped", SocketType::Json),
				PortSpec::output("has_popped", "Has Popped", SocketType::Bool),
				PortSpec::exec_output("updated", "Updated"),
				PortSpec::exec_output("popped_exec", "Popped"),
				PortSpec::exec_output("cleared", "Cleared"),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl StatefulNode for DequeNode {
	fn init_state(&self) -> Box<dyn Any + Send> {
		Box::new(DequeState::default())
	}

	fn state_model(&self) -> FlowgraphStateModel {
		FlowgraphStateModel::volatile_node_instance_json_snapshot()
	}

	fn snapshot_state(&self, state: &(dyn Any + Send)) -> Option<JsonValue> {
		let state = state.downcast_ref::<DequeState>()?;
		Some(serde_json::json!({
			"items": state.items.iter().cloned().collect::<Vec<_>>(),
			"last_popped": state.last_popped,
			"has_popped": state.has_popped,
		}))
	}

	fn restore_state(&self, state: &mut (dyn Any + Send), value: &JsonValue) -> Result<(), String> {
		let state = state
			.downcast_mut::<DequeState>()
			.ok_or_else(|| "DequeState downcast failed".to_string())?;
		let items = value
			.get("items")
			.and_then(|value| value.as_array())
			.ok_or_else(|| "expected object with array field 'items'".to_string())?;
		state.items = items.iter().cloned().collect();
		state.last_popped = value.get("last_popped").cloned().unwrap_or(JsonValue::Null);
		state.has_popped = value.get("has_popped").and_then(|value| value.as_bool()).unwrap_or(false);
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
		let state = state.downcast_mut::<DequeState>().expect("DequeState");
		let capacity = capacity(inputs, 1000)?;
		let mut fire_updated = false;
		let mut fire_popped = false;
		let mut fire_cleared = false;

		while state.items.len() > capacity {
			state.items.pop_front();
		}
		if fired_exec.contains("clear") && !state.items.is_empty() {
			state.items.clear();
			fire_cleared = true;
		} else if fired_exec.contains("pop_front") {
			if let Some(value) = state.items.pop_front() {
				state.last_popped = value;
				state.has_popped = true;
				fire_popped = true;
			}
		} else if fired_exec.contains("pop_back") {
			if let Some(value) = state.items.pop_back() {
				state.last_popped = value;
				state.has_popped = true;
				fire_popped = true;
			}
		} else if fired_exec.contains("push_front") {
			if state.items.len() >= capacity {
				state.items.pop_back();
			}
			state.items.push_front(get_required_json(inputs, "item")?.clone());
			fire_updated = true;
		} else if fired_exec.contains("push_back") {
			if state.items.len() >= capacity {
				state.items.pop_front();
			}
			state.items.push_back(get_required_json(inputs, "item")?.clone());
			fire_updated = true;
		}

		let mut out = NodeOutput::new()
			.set_data("items", json_list(state.items.iter().cloned()))
			.set_data("count", SocketValue::Int(state.items.len() as i64))
			.set_data("popped", SocketValue::Json(state.last_popped.clone()))
			.set_data("has_popped", SocketValue::Bool(state.has_popped));
		if fire_updated {
			out = out.fire_exec("updated");
		}
		if fire_popped {
			out = out.fire_exec("popped_exec");
		}
		if fire_cleared {
			out = out.fire_exec("cleared");
		}
		Ok(out)
	}
}

// ---------------------------------------------------------------------
// collection.reservoir_sample
// ---------------------------------------------------------------------

pub struct ReservoirSampleNode;

#[derive(Default)]
pub struct ReservoirSampleState {
	items: Vec<JsonValue>,
	seen: i64,
	last_index: i64,
	last_accepted: bool,
}

// ---------------------------------------------------------------------
// collection.priority_queue
// ---------------------------------------------------------------------

pub struct PriorityQueueNode;

#[derive(Clone)]
pub struct PriorityQueueEntry {
	priority: f64,
	seq: i64,
	item: JsonValue,
}

#[derive(Default)]
pub struct PriorityQueueState {
	items: Vec<PriorityQueueEntry>,
	next_seq: i64,
	last_popped: JsonValue,
	last_priority: f64,
	has_popped: bool,
}

fn sort_priority_queue(items: &mut [PriorityQueueEntry]) {
	items.sort_by(|a, b| {
		b.priority
			.partial_cmp(&a.priority)
			.unwrap_or(std::cmp::Ordering::Equal)
			.then_with(|| a.seq.cmp(&b.seq))
	});
}

fn priority_queue_items_json(items: &[PriorityQueueEntry]) -> SocketValue {
	SocketValue::List(
		items
			.iter()
			.map(|entry| {
				SocketValue::Json(serde_json::json!({
					"priority": entry.priority,
					"item": entry.item,
				}))
			})
			.collect(),
	)
}

impl NodeDescriptor for PriorityQueueNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.collection.priority_queue".into(),
			title: "Priority Queue".into(),
			category: "collection".into(),
			description: Some("Stateful bounded max-priority queue for JSON items.".into()),
			inputs: vec![
				PortSpec::exec_input("push", "Push"),
				PortSpec::exec_input("pop", "Pop"),
				PortSpec::exec_input("clear", "Clear"),
				PortSpec::input("item", "Item", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("priority", "Priority", SocketType::Float).with_default(SocketValue::Float(0.0)),
				PortSpec::input("capacity", "Capacity", SocketType::Int).with_default(SocketValue::Int(1000)),
			],
			outputs: vec![
				PortSpec::output("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
				PortSpec::output("popped", "Popped", SocketType::Json),
				PortSpec::output("popped_priority", "Popped Priority", SocketType::Float),
				PortSpec::output("has_popped", "Has Popped", SocketType::Bool),
				PortSpec::exec_output("updated", "Updated"),
				PortSpec::exec_output("popped_exec", "Popped"),
				PortSpec::exec_output("cleared", "Cleared"),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl StatefulNode for PriorityQueueNode {
	fn init_state(&self) -> Box<dyn Any + Send> {
		Box::new(PriorityQueueState::default())
	}

	fn state_model(&self) -> FlowgraphStateModel {
		FlowgraphStateModel::volatile_node_instance_json_snapshot()
	}

	fn snapshot_state(&self, state: &(dyn Any + Send)) -> Option<JsonValue> {
		let state = state.downcast_ref::<PriorityQueueState>()?;
		Some(serde_json::json!({
			"items": state.items.iter().map(|entry| serde_json::json!({
				"priority": entry.priority,
				"seq": entry.seq,
				"item": entry.item,
			})).collect::<Vec<_>>(),
			"next_seq": state.next_seq,
			"last_popped": state.last_popped,
			"last_priority": state.last_priority,
			"has_popped": state.has_popped,
		}))
	}

	fn restore_state(&self, state: &mut (dyn Any + Send), value: &JsonValue) -> Result<(), String> {
		let state = state
			.downcast_mut::<PriorityQueueState>()
			.ok_or_else(|| "PriorityQueueState downcast failed".to_string())?;
		let items = value
			.get("items")
			.and_then(|value| value.as_array())
			.ok_or_else(|| "expected object with array field 'items'".to_string())?;
		state.items = items
			.iter()
			.map(|entry| PriorityQueueEntry {
				priority: entry.get("priority").and_then(|value| value.as_f64()).unwrap_or(0.0),
				seq: entry.get("seq").and_then(|value| value.as_i64()).unwrap_or(0),
				item: entry.get("item").cloned().unwrap_or(JsonValue::Null),
			})
			.collect();
		sort_priority_queue(&mut state.items);
		state.next_seq = value
			.get("next_seq")
			.and_then(|value| value.as_i64())
			.unwrap_or(state.items.len() as i64);
		state.last_popped = value.get("last_popped").cloned().unwrap_or(JsonValue::Null);
		state.last_priority = value.get("last_priority").and_then(|value| value.as_f64()).unwrap_or(0.0);
		state.has_popped = value.get("has_popped").and_then(|value| value.as_bool()).unwrap_or(false);
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
		let state = state.downcast_mut::<PriorityQueueState>().expect("PriorityQueueState");
		let capacity = capacity(inputs, 1000)?;
		let mut fire_updated = false;
		let mut fire_popped = false;
		let mut fire_cleared = false;

		if fired_exec.contains("clear") && !state.items.is_empty() {
			state.items.clear();
			fire_cleared = true;
		} else if fired_exec.contains("pop") {
			if !state.items.is_empty() {
				let entry = state.items.remove(0);
				state.last_popped = entry.item;
				state.last_priority = entry.priority;
				state.has_popped = true;
				fire_popped = true;
			}
		} else if fired_exec.contains("push") {
			let priority = get_optional_float(inputs, "priority", 0.0)?;
			if !priority.is_finite() {
				return Err(NodeExecError::Generic(anyhow::anyhow!(
					"collection.priority_queue: priority must be finite"
				)));
			}
			state.items.push(PriorityQueueEntry {
				priority,
				seq: state.next_seq,
				item: get_required_json(inputs, "item")?.clone(),
			});
			state.next_seq = state.next_seq.saturating_add(1);
			sort_priority_queue(&mut state.items);
			state.items.truncate(capacity);
			fire_updated = true;
		}
		state.items.truncate(capacity);

		let mut out = NodeOutput::new()
			.set_data("items", priority_queue_items_json(&state.items))
			.set_data("count", SocketValue::Int(state.items.len() as i64))
			.set_data("popped", SocketValue::Json(state.last_popped.clone()))
			.set_data("popped_priority", SocketValue::Float(state.last_priority))
			.set_data("has_popped", SocketValue::Bool(state.has_popped));
		if fire_updated {
			out = out.fire_exec("updated");
		}
		if fire_popped {
			out = out.fire_exec("popped_exec");
		}
		if fire_cleared {
			out = out.fire_exec("cleared");
		}
		Ok(out)
	}
}

impl NodeDescriptor for ReservoirSampleNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.collection.reservoir_sample".into(),
			title: "Reservoir Sample".into(),
			category: "collection".into(),
			description: Some("Stateful deterministic reservoir sampler for JSON event streams.".into()),
			inputs: vec![
				PortSpec::exec_input("observe", "Observe"),
				PortSpec::exec_input("clear", "Clear"),
				PortSpec::input("item", "Item", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("capacity", "Capacity", SocketType::Int).with_default(SocketValue::Int(32)),
				PortSpec::input("seed", "Seed", SocketType::String).with_default(SocketValue::String(String::new())),
			],
			outputs: vec![
				PortSpec::output("sample", "Sample", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("count_total", "Count Total", SocketType::Int),
				PortSpec::output("count_sampled", "Count Sampled", SocketType::Int),
				PortSpec::output("accepted", "Accepted", SocketType::Bool),
				PortSpec::output("index", "Index", SocketType::Int),
				PortSpec::exec_output("updated", "Updated"),
				PortSpec::exec_output("cleared", "Cleared"),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl StatefulNode for ReservoirSampleNode {
	fn init_state(&self) -> Box<dyn Any + Send> {
		Box::new(ReservoirSampleState::default())
	}

	fn state_model(&self) -> FlowgraphStateModel {
		FlowgraphStateModel::volatile_node_instance_json_snapshot()
	}

	fn snapshot_state(&self, state: &(dyn Any + Send)) -> Option<JsonValue> {
		let state = state.downcast_ref::<ReservoirSampleState>()?;
		Some(serde_json::json!({
			"items": state.items,
			"seen": state.seen,
			"last_index": state.last_index,
			"last_accepted": state.last_accepted,
		}))
	}

	fn restore_state(&self, state: &mut (dyn Any + Send), value: &JsonValue) -> Result<(), String> {
		let state = state
			.downcast_mut::<ReservoirSampleState>()
			.ok_or_else(|| "ReservoirSampleState downcast failed".to_string())?;
		let items = value
			.get("items")
			.and_then(|value| value.as_array())
			.ok_or_else(|| "expected object with array field 'items'".to_string())?;
		state.items = items.clone();
		state.seen = value
			.get("seen")
			.and_then(|value| value.as_i64())
			.unwrap_or(state.items.len() as i64);
		state.last_index = value.get("last_index").and_then(|value| value.as_i64()).unwrap_or(-1);
		state.last_accepted = value.get("last_accepted").and_then(|value| value.as_bool()).unwrap_or(false);
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
		let state = state.downcast_mut::<ReservoirSampleState>().expect("ReservoirSampleState");
		let capacity = capacity(inputs, 32)?;
		let seed = get_optional_string(inputs, "seed", "")?;
		let mut fire_updated = false;
		let mut fire_cleared = false;

		state.items.truncate(capacity);
		if fired_exec.contains("clear") && (state.seen != 0 || !state.items.is_empty()) {
			*state = ReservoirSampleState {
				last_index: -1,
				..ReservoirSampleState::default()
			};
			fire_cleared = true;
		}
		if fired_exec.contains("observe") {
			state.seen = state.seen.saturating_add(1);
			let item = get_required_json(inputs, "item")?.clone();
			if state.items.len() < capacity {
				state.items.push(item);
				state.last_index = state.items.len() as i64 - 1;
				state.last_accepted = true;
				fire_updated = true;
			} else {
				let index = (hash_index(&seed, state.seen) % state.seen.max(1) as u64) as usize;
				if index < capacity {
					state.items[index] = item;
					state.last_index = index as i64;
					state.last_accepted = true;
					fire_updated = true;
				} else {
					state.last_index = index as i64;
					state.last_accepted = false;
				}
			}
		}

		let mut out = NodeOutput::new()
			.set_data("sample", json_list(state.items.iter().cloned()))
			.set_data("count_total", SocketValue::Int(state.seen))
			.set_data("count_sampled", SocketValue::Int(state.items.len() as i64))
			.set_data("accepted", SocketValue::Bool(state.last_accepted))
			.set_data("index", SocketValue::Int(state.last_index));
		if fire_updated {
			out = out.fire_exec("updated");
		}
		if fire_cleared {
			out = out.fire_exec("cleared");
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
	async fn ring_buffer_push_drops_oldest_and_snapshots() {
		let node = RingBufferNode;
		let mut state = node.init_state();
		let sctx = sctx();
		for n in 1..=3 {
			let inputs: InputMap = [
				("item".into(), SocketValue::Json(serde_json::json!(n))),
				("capacity".into(), SocketValue::Int(2)),
			]
			.into_iter()
			.collect();
			let out = node
				.compute(state.as_mut(), &InputMap::new(), &inputs, &fire("push"), &sctx)
				.await
				.unwrap();
			assert!(out.fired_exec.contains("updated"));
		}
		let out = node
			.compute(state.as_mut(), &InputMap::new(), &InputMap::new(), &ExecFireSet::new(), &sctx)
			.await
			.unwrap();
		assert_eq!(out.data.get("count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("dropped_count"), Some(&SocketValue::Int(1)));
		assert_eq!(
			out.data.get("items"),
			Some(&SocketValue::List(vec![
				SocketValue::Json(serde_json::json!(2)),
				SocketValue::Json(serde_json::json!(3)),
			]))
		);
		assert_eq!(
			node.snapshot_state(state.as_ref()),
			Some(serde_json::json!({ "items": [2, 3], "dropped_count": 1 }))
		);
	}

	#[tokio::test]
	async fn deque_push_pop_both_ends() {
		let node = DequeNode;
		let mut state = node.init_state();
		let sctx = sctx();
		for (port, item) in [("push_back", "b"), ("push_front", "a")] {
			let inputs: InputMap = [("item".into(), SocketValue::Json(serde_json::json!(item)))].into_iter().collect();
			node.compute(state.as_mut(), &InputMap::new(), &inputs, &fire(port), &sctx)
				.await
				.unwrap();
		}
		let out = node
			.compute(state.as_mut(), &InputMap::new(), &InputMap::new(), &fire("pop_back"), &sctx)
			.await
			.unwrap();
		assert!(out.fired_exec.contains("popped_exec"));
		assert_eq!(out.data.get("popped"), Some(&SocketValue::Json(serde_json::json!("b"))));
		assert_eq!(
			out.data.get("items"),
			Some(&SocketValue::List(vec![SocketValue::Json(serde_json::json!("a"))]))
		);
	}

	#[tokio::test]
	async fn reservoir_sample_is_bounded_and_deterministic() {
		let node = ReservoirSampleNode;
		let mut a = node.init_state();
		let mut b = node.init_state();
		let sctx = sctx();
		for n in 0..20 {
			let inputs: InputMap = [
				("item".into(), SocketValue::Json(serde_json::json!(n))),
				("capacity".into(), SocketValue::Int(5)),
				("seed".into(), SocketValue::String("fixed".into())),
			]
			.into_iter()
			.collect();
			node.compute(a.as_mut(), &InputMap::new(), &inputs, &fire("observe"), &sctx)
				.await
				.unwrap();
			node.compute(b.as_mut(), &InputMap::new(), &inputs, &fire("observe"), &sctx)
				.await
				.unwrap();
		}
		let out_a = node
			.compute(a.as_mut(), &InputMap::new(), &InputMap::new(), &ExecFireSet::new(), &sctx)
			.await
			.unwrap();
		let out_b = node
			.compute(b.as_mut(), &InputMap::new(), &InputMap::new(), &ExecFireSet::new(), &sctx)
			.await
			.unwrap();
		assert_eq!(out_a.data.get("sample"), out_b.data.get("sample"));
		assert_eq!(out_a.data.get("count_total"), Some(&SocketValue::Int(20)));
		assert_eq!(out_a.data.get("count_sampled"), Some(&SocketValue::Int(5)));
	}

	#[tokio::test]
	async fn priority_queue_pops_highest_priority_fifo_for_ties() {
		let node = PriorityQueueNode;
		let mut state = node.init_state();
		let sctx = sctx();
		for (item, priority) in [("low", 1.0), ("high-a", 9.0), ("high-b", 9.0)] {
			let inputs: InputMap = [
				("item".into(), SocketValue::Json(serde_json::json!(item))),
				("priority".into(), SocketValue::Float(priority)),
				("capacity".into(), SocketValue::Int(10)),
			]
			.into_iter()
			.collect();
			node.compute(state.as_mut(), &InputMap::new(), &inputs, &fire("push"), &sctx)
				.await
				.unwrap();
		}
		let out = node
			.compute(state.as_mut(), &InputMap::new(), &InputMap::new(), &fire("pop"), &sctx)
			.await
			.unwrap();
		assert!(out.fired_exec.contains("popped_exec"));
		assert_eq!(out.data.get("popped"), Some(&SocketValue::Json(serde_json::json!("high-a"))));
		assert_eq!(out.data.get("popped_priority"), Some(&SocketValue::Float(9.0)));
		assert_eq!(out.data.get("count"), Some(&SocketValue::Int(2)));

		let out = node
			.compute(state.as_mut(), &InputMap::new(), &InputMap::new(), &fire("pop"), &sctx)
			.await
			.unwrap();
		assert_eq!(out.data.get("popped"), Some(&SocketValue::Json(serde_json::json!("high-b"))));
	}
}

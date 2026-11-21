//! List / Map 操作ノード（全て PureNode）。
//!
//! Flowgraph の List は要素型が型パラメータ付きだが、δ-2 の組込みノードは
//! `List<Json>` / `Map<Json>` を前提とする（汎用性重視）。
//! 特定型のリストが欲しい場合は利用者側で `convert` ノードを挟む。

use crate::flowgraph::node::{
	get_optional_bool, get_optional_int, get_optional_string, get_required_int, get_required_json, get_required_list, get_required_map,
	get_required_string, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PureNode,
};
use crate::flowgraph::socket::{SocketType, SocketValue};
use async_trait::async_trait;
use base64::Engine as _;
use std::cmp::Ordering;
use std::collections::BTreeMap;

// ----- list.len -------------------------------------------------------------

pub struct StringListGetNode;
impl NodeDescriptor for StringListGetNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.string_get".into(),
			title: "String List Get".into(), category: "list".into(),
			description: Some("文字列リストの指定位置を取得。範囲外は空文字とfound=false。".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::String))),
				PortSpec::input("index", "Index", SocketType::Int).with_default(SocketValue::Int(0)),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::String), PortSpec::output("found", "Found", SocketType::Bool)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for StringListGetNode {
	async fn compute(&self, _host: &crate::flowgraph::node::PureEvalHost, _props: &InputMap,
		inputs: &InputMap, _fired: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
		let list = get_required_list(inputs, "items")?;
		let index = get_optional_int(inputs, "index", 0)?;
		let value = usize::try_from(index).ok().and_then(|i| list.get(i));
		let text = match value {
			Some(SocketValue::String(s)) => s.clone(),
			None => String::new(),
			_ => return Err(NodeExecError::Generic(anyhow::anyhow!("list.string_get requires string elements"))),
		};
		Ok(NodeOutput::new().set_data("value", SocketValue::String(text)).set_data("found", SocketValue::Bool(value.is_some())))
	}
}

pub struct ListLenNode;
impl NodeDescriptor for ListLenNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.len".into(),
			title: "List Length".into(),
			category: "list".into(),
			description: None,
			inputs: vec![PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json)))],
			outputs: vec![PortSpec::output("len", "Length", SocketType::Int)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListLenNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let xs = get_required_list(inputs, "items")?;
		Ok(NodeOutput::new().set_data("len", SocketValue::Int(xs.len() as i64)))
	}
}

// ----- list.get -------------------------------------------------------------

pub struct ListGetNode;
impl NodeDescriptor for ListGetNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.get".into(),
			title: "List Get".into(),
			category: "list".into(),
			description: Some("index が範囲外なら Json::Null".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("index", "Index", SocketType::Int),
			],
			outputs: vec![PortSpec::output("item", "Item", SocketType::Json)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListGetNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let xs = get_required_list(inputs, "items")?;
		let idx = get_required_int(inputs, "index")?;
		let v = if idx < 0 || idx as usize >= xs.len() {
			SocketValue::Json(serde_json::Value::Null)
		} else {
			match &xs[idx as usize] {
				SocketValue::Json(j) => SocketValue::Json(j.clone()),
				other => SocketValue::Json(socket_value_to_json(other)),
			}
		};
		Ok(NodeOutput::new().set_data("item", v))
	}
}

// ----- list.is_empty --------------------------------------------------------

pub struct ListIsEmptyNode;
impl NodeDescriptor for ListIsEmptyNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.is_empty".into(),
			title: "List Is Empty".into(),
			category: "list".into(),
			description: None,
			inputs: vec![PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json)))],
			outputs: vec![PortSpec::output("result", "Result", SocketType::Bool)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListIsEmptyNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let xs = get_required_list(inputs, "items")?;
		Ok(NodeOutput::new().set_data("result", SocketValue::Bool(xs.is_empty())))
	}
}

// ----- list.range -----------------------------------------------------------

pub struct ListRangeNode;
impl NodeDescriptor for ListRangeNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.range".into(),
			title: "List Range".into(),
			category: "list".into(),
			description: Some("半開区間 [start, end) の Int list を作る。step=0 は空、最大 10000 要素。".into()),
			inputs: vec![
				PortSpec::input("start", "Start", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("end", "End", SocketType::Int),
				PortSpec::input("step", "Step", SocketType::Int).with_default(SocketValue::Int(1)),
			],
			outputs: vec![
				PortSpec::output("items", "Items", SocketType::List(Box::new(SocketType::Int))),
				PortSpec::output("count", "Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListRangeNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let start = get_optional_int(inputs, "start", 0)?;
		let end = get_required_int(inputs, "end")?;
		let step = get_optional_int(inputs, "step", 1)?;
		let mut values = Vec::new();
		if step != 0 {
			let mut cur = start;
			while values.len() < 10_000 && if step > 0 { cur < end } else { cur > end } {
				values.push(SocketValue::Int(cur));
				cur = cur.saturating_add(step);
			}
		}
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(values.len() as i64))
			.set_data("items", SocketValue::List(values)))
	}
}

// ----- list.slice -----------------------------------------------------------

pub struct ListSliceNode;
impl NodeDescriptor for ListSliceNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.slice".into(),
			title: "List Slice".into(),
			category: "list".into(),
			description: Some("List<Json> から start/count で部分列を取り出す。count < 0 は末尾まで。".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("start", "Start", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("count", "Count", SocketType::Int).with_default(SocketValue::Int(-1)),
			],
			outputs: vec![
				PortSpec::output("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListSliceNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let xs = get_required_list(inputs, "items")?;
		let start = get_optional_int(inputs, "start", 0)?.max(0) as usize;
		let count = get_optional_int(inputs, "count", -1)?;
		let iter = xs.iter().skip(start);
		let out: Vec<SocketValue> = if count < 0 {
			iter.cloned().collect()
		} else {
			iter.take(count.max(0) as usize).cloned().collect()
		};
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(out.len() as i64))
			.set_data("items", SocketValue::List(out)))
	}
}

// ----- list.map_json_get ----------------------------------------------------

pub struct ListMapJsonGetNode;
impl NodeDescriptor for ListMapJsonGetNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.map_json_get".into(),
			title: "List Map JSON Get".into(),
			category: "list".into(),
			description: Some("List<Json> の各 object から key を取り出す。欠損は null。".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String),
			],
			outputs: vec![PortSpec::output("items", "Items", SocketType::List(Box::new(SocketType::Json)))],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListMapJsonGetNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let xs = get_required_list(inputs, "items")?;
		let key = get_required_string(inputs, "key")?;
		let out = xs
			.iter()
			.map(socket_value_to_json)
			.map(|j| json_get_key(&j, &key).cloned().unwrap_or(serde_json::Value::Null))
			.map(SocketValue::Json)
			.collect();
		Ok(NodeOutput::new().set_data("items", SocketValue::List(out)))
	}
}

// ----- list.filter_json_eq --------------------------------------------------

pub struct ListFilterJsonEqNode;
impl NodeDescriptor for ListFilterJsonEqNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.filter_json_eq".into(),
			title: "List Filter JSON Eq".into(),
			category: "list".into(),
			description: Some("List<Json> を key の値または item 全体の JSON 等価で絞る。key 空なら item 全体。".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("equals", "Equals", SocketType::Json),
			],
			outputs: vec![
				PortSpec::output("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListFilterJsonEqNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let xs = get_required_list(inputs, "items")?;
		let key = get_optional_string(inputs, "key", "")?;
		let equals = get_required_json(inputs, "equals")?;
		let out: Vec<SocketValue> = xs
			.iter()
			.map(socket_value_to_json)
			.filter(|j| {
				let target = if key.trim().is_empty() { Some(j) } else { json_get_key(j, &key) };
				target.map(|v| v == equals).unwrap_or(false)
			})
			.map(SocketValue::Json)
			.collect();
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(out.len() as i64))
			.set_data("items", SocketValue::List(out)))
	}
}

// ----- list.window ----------------------------------------------------------

pub struct ListWindowNode;
impl NodeDescriptor for ListWindowNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.window".into(),
			title: "List Window".into(),
			category: "list".into(),
			description: Some("List<Json> から sliding window を作る。size/step は 1 以上。".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("size", "Size", SocketType::Int).with_default(SocketValue::Int(2)),
				PortSpec::input("step", "Step", SocketType::Int).with_default(SocketValue::Int(1)),
			],
			outputs: vec![
				PortSpec::output(
					"windows",
					"Windows",
					SocketType::List(Box::new(SocketType::List(Box::new(SocketType::Json)))),
				),
				PortSpec::output("count", "Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListWindowNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let xs = get_required_list(inputs, "items")?;
		let size = get_optional_int(inputs, "size", 2)?.max(1) as usize;
		let step = get_optional_int(inputs, "step", 1)?.max(1) as usize;
		let mut out = Vec::new();
		let mut i = 0usize;
		while i + size <= xs.len() && out.len() < 10_000 {
			out.push(SocketValue::List(xs[i..i + size].to_vec()));
			i = i.saturating_add(step);
		}
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(out.len() as i64))
			.set_data("windows", SocketValue::List(out)))
	}
}

// ----- list.aggregate_numbers ----------------------------------------------

pub struct ListAggregateNumbersNode;
impl NodeDescriptor for ListAggregateNumbersNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.aggregate_numbers".into(),
			title: "List Aggregate Numbers".into(),
			category: "list".into(),
			description: Some("List<Json> の数値を count/sum/min/max/avg へ集約。key 空なら item 全体。".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String).with_default(SocketValue::String(String::new())),
			],
			outputs: vec![
				PortSpec::output("count", "Count", SocketType::Int),
				PortSpec::output("sum", "Sum", SocketType::Float),
				PortSpec::output("min", "Min", SocketType::Float),
				PortSpec::output("max", "Max", SocketType::Float),
				PortSpec::output("avg", "Average", SocketType::Float),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListAggregateNumbersNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let xs = get_required_list(inputs, "items")?;
		let key = get_optional_string(inputs, "key", "")?;
		let nums: Vec<f64> = xs
			.iter()
			.map(socket_value_to_json)
			.filter_map(|j| {
				let target = if key.trim().is_empty() { Some(&j) } else { json_get_key(&j, &key) };
				target.and_then(json_as_f64)
			})
			.collect();
		let count = nums.len() as i64;
		let sum: f64 = nums.iter().sum();
		let min = nums.iter().copied().reduce(f64::min).unwrap_or(0.0);
		let max = nums.iter().copied().reduce(f64::max).unwrap_or(0.0);
		let avg = if nums.is_empty() { 0.0 } else { sum / nums.len() as f64 };
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(count))
			.set_data("sum", SocketValue::Float(sum))
			.set_data("min", SocketValue::Float(min))
			.set_data("max", SocketValue::Float(max))
			.set_data("avg", SocketValue::Float(avg)))
	}
}

// ----- list.sort_json -------------------------------------------------------

pub struct ListSortJsonNode;
impl NodeDescriptor for ListSortJsonNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.sort_json".into(),
			title: "List Sort JSON".into(),
			category: "list".into(),
			description: Some("Stable sort for List<Json>. key 空なら item 全体、descending=true で降順。".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("descending", "Descending", SocketType::Bool).with_default(SocketValue::Bool(false)),
			],
			outputs: vec![
				PortSpec::output("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListSortJsonNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let key = get_optional_string(inputs, "key", "")?;
		let descending = get_optional_bool(inputs, "descending", false)?;
		let mut out: Vec<serde_json::Value> = get_required_list(inputs, "items")?.iter().map(socket_value_to_json).collect();
		sort_json_values(&mut out, &key, descending);
		let items = out.into_iter().map(SocketValue::Json).collect::<Vec<_>>();
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(items.len() as i64))
			.set_data("items", SocketValue::List(items)))
	}
}

// ----- list.top_k_json ------------------------------------------------------

pub struct ListTopKJsonNode;
impl NodeDescriptor for ListTopKJsonNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.top_k_json".into(),
			title: "List Top-K JSON".into(),
			category: "list".into(),
			description: Some("Sort List<Json> by key and return the first k items. descending=true returns largest values.".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("k", "K", SocketType::Int).with_default(SocketValue::Int(10)),
				PortSpec::input("descending", "Descending", SocketType::Bool).with_default(SocketValue::Bool(true)),
			],
			outputs: vec![
				PortSpec::output("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListTopKJsonNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let key = get_optional_string(inputs, "key", "")?;
		let descending = get_optional_bool(inputs, "descending", true)?;
		let k = get_optional_int(inputs, "k", 10)?.clamp(0, 10_000) as usize;
		let mut out: Vec<serde_json::Value> = get_required_list(inputs, "items")?.iter().map(socket_value_to_json).collect();
		sort_json_values(&mut out, &key, descending);
		out.truncate(k);
		let items = out.into_iter().map(SocketValue::Json).collect::<Vec<_>>();
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(items.len() as i64))
			.set_data("items", SocketValue::List(items)))
	}
}

// ----- list.binary_search_json --------------------------------------------

pub struct ListBinarySearchJsonNode;
impl NodeDescriptor for ListBinarySearchJsonNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.list.binary_search_json".into(),
			title: "List Binary Search JSON".into(),
			category: "list".into(),
			description: Some("Binary search in an ascending List<Json>. key 空なら item 全体。".into()),
			inputs: vec![
				PortSpec::input("items", "Items", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("target", "Target", SocketType::Json),
				PortSpec::input("key", "Key", SocketType::String).with_default(SocketValue::String(String::new())),
			],
			outputs: vec![
				PortSpec::output("found", "Found", SocketType::Bool),
				PortSpec::output("index", "Index", SocketType::Int),
				PortSpec::output("item", "Item", SocketType::Json),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for ListBinarySearchJsonNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let key = get_optional_string(inputs, "key", "")?;
		let target = get_required_json(inputs, "target")?;
		let xs = get_required_list(inputs, "items")?;
		let mut lo = 0usize;
		let mut hi = xs.len();
		while lo < hi {
			let mid = lo + (hi - lo) / 2;
			let item = socket_value_to_json(&xs[mid]);
			let probe = comparable_json(&item, &key);
			match json_compare(&probe, target) {
				Ordering::Less => lo = mid + 1,
				Ordering::Greater => hi = mid,
				Ordering::Equal => {
					return Ok(NodeOutput::new()
						.set_data("found", SocketValue::Bool(true))
						.set_data("index", SocketValue::Int(mid as i64))
						.set_data("item", SocketValue::Json(item)));
				}
			}
		}
		Ok(NodeOutput::new()
			.set_data("found", SocketValue::Bool(false))
			.set_data("index", SocketValue::Int(-(lo as i64) - 1))
			.set_data("item", SocketValue::Json(serde_json::Value::Null)))
	}
}

// ----- map.get --------------------------------------------------------------

pub struct MapGetNode;
impl NodeDescriptor for MapGetNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.map.get".into(),
			title: "Map Get".into(),
			category: "map".into(),
			description: Some("キーが存在しない場合は Json::Null".into()),
			inputs: vec![
				PortSpec::input("m", "Map", SocketType::Map(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String),
			],
			outputs: vec![PortSpec::output("value", "Value", SocketType::Json)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for MapGetNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let m = get_required_map(inputs, "m")?;
		let k = get_required_string(inputs, "key")?;
		let v = match m.get(&k) {
			Some(SocketValue::Json(j)) => SocketValue::Json(j.clone()),
			Some(other) => SocketValue::Json(socket_value_to_json(other)),
			None => SocketValue::Json(serde_json::Value::Null),
		};
		Ok(NodeOutput::new().set_data("value", v))
	}
}

// ----- map.keys -------------------------------------------------------------

pub struct MapKeysNode;
impl NodeDescriptor for MapKeysNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.map.keys".into(),
			title: "Map Keys".into(),
			category: "map".into(),
			description: None,
			inputs: vec![PortSpec::input("m", "Map", SocketType::Map(Box::new(SocketType::Json)))],
			outputs: vec![PortSpec::output("keys", "Keys", SocketType::List(Box::new(SocketType::String)))],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for MapKeysNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let m = get_required_map(inputs, "m")?;
		let keys: Vec<SocketValue> = m.keys().cloned().map(SocketValue::String).collect();
		Ok(NodeOutput::new().set_data("keys", SocketValue::List(keys)))
	}
}

// ----- map.has --------------------------------------------------------------

pub struct MapHasNode;
impl NodeDescriptor for MapHasNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.map.has".into(),
			title: "Map Has".into(),
			category: "map".into(),
			description: None,
			inputs: vec![
				PortSpec::input("m", "Map", SocketType::Map(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String),
			],
			outputs: vec![PortSpec::output("result", "Result", SocketType::Bool)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for MapHasNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let m = get_required_map(inputs, "m")?;
		let k = get_required_string(inputs, "key")?;
		Ok(NodeOutput::new().set_data("result", SocketValue::Bool(m.contains_key(&k))))
	}
}

// ----- dictionary.from_entries --------------------------------------------

pub struct DictionaryFromEntriesNode;
impl NodeDescriptor for DictionaryFromEntriesNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dictionary.from_entries".into(),
			title: "Dictionary From Entries".into(),
			category: "dictionary".into(),
			description: Some("Build a generic string-key Dictionary from List<Json> object entries.".into()),
			inputs: vec![
				PortSpec::input("entries", "Entries", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::input("key_field", "Key Field", SocketType::String).with_default(SocketValue::String("key".into())),
				PortSpec::input("value_field", "Value Field", SocketType::String).with_default(SocketValue::String("value".into())),
			],
			outputs: vec![
				PortSpec::output("dictionary", "Dictionary", SocketType::Map(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for DictionaryFromEntriesNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let entries = get_required_list(inputs, "entries")?;
		let key_field = get_optional_string(inputs, "key_field", "key")?;
		let value_field = get_optional_string(inputs, "value_field", "value")?;
		let mut out = BTreeMap::new();
		for entry in entries {
			let json = socket_value_to_json(entry);
			let Some(key) = json_get_key(&json, &key_field).and_then(|value| value.as_str()) else {
				continue;
			};
			let value = json_get_key(&json, &value_field).cloned().unwrap_or(serde_json::Value::Null);
			out.insert(key.to_string(), SocketValue::Json(value));
		}
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(out.len() as i64))
			.set_data("dictionary", SocketValue::Map(out)))
	}
}

// ----- dictionary.get -------------------------------------------------------

pub struct DictionaryGetNode;
impl NodeDescriptor for DictionaryGetNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dictionary.get".into(),
			title: "Dictionary Get".into(),
			category: "dictionary".into(),
			description: Some("Get a value from a generic string-key Dictionary.".into()),
			inputs: vec![
				PortSpec::input("dictionary", "Dictionary", SocketType::Map(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String),
				PortSpec::input("default", "Default", SocketType::Json).with_default(SocketValue::Json(serde_json::Value::Null)),
			],
			outputs: vec![
				PortSpec::output("value", "Value", SocketType::Json),
				PortSpec::output("found", "Found", SocketType::Bool),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for DictionaryGetNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let dict = get_required_map(inputs, "dictionary")?;
		let key = get_required_string(inputs, "key")?;
		let default = inputs.get("default").map(socket_value_to_json).unwrap_or(serde_json::Value::Null);
		let (found, value) = match dict.get(&key) {
			Some(value) => (true, socket_value_to_json(value)),
			None => (false, default),
		};
		Ok(NodeOutput::new()
			.set_data("found", SocketValue::Bool(found))
			.set_data("value", SocketValue::Json(value)))
	}
}

// ----- dictionary.set -------------------------------------------------------

pub struct DictionarySetNode;
impl NodeDescriptor for DictionarySetNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dictionary.set".into(),
			title: "Dictionary Set".into(),
			category: "dictionary".into(),
			description: Some("Return a Dictionary with one key set to a JSON value.".into()),
			inputs: vec![
				PortSpec::input("dictionary", "Dictionary", SocketType::Map(Box::new(SocketType::Json)))
					.with_default(SocketValue::Map(BTreeMap::new())),
				PortSpec::input("key", "Key", SocketType::String),
				PortSpec::input("value", "Value", SocketType::Json),
			],
			outputs: vec![
				PortSpec::output("dictionary", "Dictionary", SocketType::Map(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
				PortSpec::output("replaced", "Replaced", SocketType::Bool),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for DictionarySetNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut dict = get_required_map(inputs, "dictionary")?.clone();
		let key = get_required_string(inputs, "key")?;
		let value = get_required_json(inputs, "value")?.clone();
		let replaced = dict.insert(key, SocketValue::Json(value)).is_some();
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(dict.len() as i64))
			.set_data("replaced", SocketValue::Bool(replaced))
			.set_data("dictionary", SocketValue::Map(dict)))
	}
}

// ----- dictionary.remove ----------------------------------------------------

pub struct DictionaryRemoveNode;
impl NodeDescriptor for DictionaryRemoveNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dictionary.remove".into(),
			title: "Dictionary Remove".into(),
			category: "dictionary".into(),
			description: Some("Return a Dictionary with one key removed.".into()),
			inputs: vec![
				PortSpec::input("dictionary", "Dictionary", SocketType::Map(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String),
			],
			outputs: vec![
				PortSpec::output("dictionary", "Dictionary", SocketType::Map(Box::new(SocketType::Json))),
				PortSpec::output("count", "Count", SocketType::Int),
				PortSpec::output("removed", "Removed", SocketType::Bool),
				PortSpec::output("value", "Value", SocketType::Json),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for DictionaryRemoveNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut dict = get_required_map(inputs, "dictionary")?.clone();
		let key = get_required_string(inputs, "key")?;
		let removed = dict.remove(&key).map(|value| socket_value_to_json(&value));
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(dict.len() as i64))
			.set_data("removed", SocketValue::Bool(removed.is_some()))
			.set_data("value", SocketValue::Json(removed.unwrap_or(serde_json::Value::Null)))
			.set_data("dictionary", SocketValue::Map(dict)))
	}
}

// ----- dictionary.keys / has ------------------------------------------------

pub struct DictionaryKeysNode;
impl NodeDescriptor for DictionaryKeysNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dictionary.keys".into(),
			title: "Dictionary Keys".into(),
			category: "dictionary".into(),
			description: Some("Return sorted keys from a generic string-key Dictionary.".into()),
			inputs: vec![PortSpec::input(
				"dictionary",
				"Dictionary",
				SocketType::Map(Box::new(SocketType::Json)),
			)],
			outputs: vec![
				PortSpec::output("keys", "Keys", SocketType::List(Box::new(SocketType::String))),
				PortSpec::output("count", "Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for DictionaryKeysNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let dict = get_required_map(inputs, "dictionary")?;
		let keys = dict.keys().cloned().map(SocketValue::String).collect::<Vec<_>>();
		Ok(NodeOutput::new()
			.set_data("count", SocketValue::Int(keys.len() as i64))
			.set_data("keys", SocketValue::List(keys)))
	}
}

pub struct DictionaryHasNode;
impl NodeDescriptor for DictionaryHasNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.dictionary.has".into(),
			title: "Dictionary Has".into(),
			category: "dictionary".into(),
			description: Some("Check whether a generic string-key Dictionary contains a key.".into()),
			inputs: vec![
				PortSpec::input("dictionary", "Dictionary", SocketType::Map(Box::new(SocketType::Json))),
				PortSpec::input("key", "Key", SocketType::String),
			],
			outputs: vec![PortSpec::output("result", "Result", SocketType::Bool)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for DictionaryHasNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let dict = get_required_map(inputs, "dictionary")?;
		let key = get_required_string(inputs, "key")?;
		Ok(NodeOutput::new().set_data("result", SocketValue::Bool(dict.contains_key(&key))))
	}
}

// ----- 内部ヘルパ ----------------------------------------------------------

fn socket_value_to_json(v: &SocketValue) -> serde_json::Value {
	match v {
		SocketValue::Bool(b) => serde_json::json!(b),
		SocketValue::Int(i) => serde_json::json!(i),
		SocketValue::Float(f) => serde_json::json!(f),
		SocketValue::String(s) => serde_json::json!(s),
		SocketValue::Bytes(bytes) => serde_json::json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
		SocketValue::Json(j) => j.clone(),
		SocketValue::List(xs) => serde_json::Value::Array(xs.iter().map(socket_value_to_json).collect()),
		SocketValue::Map(m) => {
			let obj: serde_json::Map<String, serde_json::Value> = m.iter().map(|(k, v)| (k.clone(), socket_value_to_json(v))).collect();
			serde_json::Value::Object(obj)
		}
		SocketValue::Option(value) => value
			.as_ref()
			.map(|value| socket_value_to_json(value))
			.unwrap_or(serde_json::Value::Null),
		SocketValue::Result(result) => {
			let mut obj = serde_json::Map::new();
			obj.insert("ok".into(), serde_json::Value::Bool(result.ok));
			if let Some(value) = result.value.as_ref() {
				obj.insert("value".into(), socket_value_to_json(value));
			}
			if let Some(error) = result.error.as_ref() {
				obj.insert("error".into(), serde_json::Value::String(error.clone()));
			}
			if let Some(code) = result.code.as_ref() {
				obj.insert("code".into(), serde_json::Value::String(code.clone()));
			}
			serde_json::Value::Object(obj)
		}
		SocketValue::Table(t) => t.to_json_array(),
		// Phase ξ §6.4: 外部 JSON 境界では value のみ（pass-through）。
		// unit を維持したい場合は `flowgraph.unit.to_json` を使う。
		SocketValue::Quantity(q) => serde_json::json!(q.value),
		// Phase π: DateTime は RFC3339 (Z suffix) 文字列として pass-through。
		SocketValue::DateTime(dt) => serde_json::Value::String(dt.to_rfc3339()),
		SocketValue::MotionFrame(m) => m.to_json_value(),
		SocketValue::Unmf(frame) => vac_motion::unmf::unmf_to_json_value(frame),
	}
}

fn json_get_key<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
	if key.trim().is_empty() {
		return Some(value);
	}
	let mut cur = value;
	for part in key.split('.') {
		if part.is_empty() {
			return None;
		}
		cur = match cur {
			serde_json::Value::Object(obj) => obj.get(part)?,
			serde_json::Value::Array(arr) => arr.get(part.parse::<usize>().ok()?)?,
			_ => return None,
		};
	}
	Some(cur)
}

fn comparable_json(value: &serde_json::Value, key: &str) -> serde_json::Value {
	if key.trim().is_empty() {
		value.clone()
	} else {
		json_get_key(value, key).cloned().unwrap_or(serde_json::Value::Null)
	}
}

fn json_type_rank(value: &serde_json::Value) -> u8 {
	match value {
		serde_json::Value::Null => 0,
		serde_json::Value::Bool(_) => 1,
		serde_json::Value::Number(_) => 2,
		serde_json::Value::String(_) => 3,
		serde_json::Value::Array(_) => 4,
		serde_json::Value::Object(_) => 5,
	}
}

fn json_compare(a: &serde_json::Value, b: &serde_json::Value) -> Ordering {
	let rank = json_type_rank(a).cmp(&json_type_rank(b));
	if rank != Ordering::Equal {
		return rank;
	}
	match (a, b) {
		(serde_json::Value::Null, serde_json::Value::Null) => Ordering::Equal,
		(serde_json::Value::Bool(a), serde_json::Value::Bool(b)) => a.cmp(b),
		(serde_json::Value::Number(a), serde_json::Value::Number(b)) => {
			let a = a.as_f64().unwrap_or(0.0);
			let b = b.as_f64().unwrap_or(0.0);
			a.partial_cmp(&b).unwrap_or(Ordering::Equal)
		}
		(serde_json::Value::String(a), serde_json::Value::String(b)) => a.cmp(b),
		(serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
			for (a, b) in a.iter().zip(b.iter()) {
				let ord = json_compare(a, b);
				if ord != Ordering::Equal {
					return ord;
				}
			}
			a.len().cmp(&b.len())
		}
		(serde_json::Value::Object(a), serde_json::Value::Object(b)) => serde_json::Value::Object(a.clone())
			.to_string()
			.cmp(&serde_json::Value::Object(b.clone()).to_string()),
		_ => Ordering::Equal,
	}
}

fn sort_json_values(values: &mut [serde_json::Value], key: &str, descending: bool) {
	values.sort_by(|a, b| {
		let ord = json_compare(&comparable_json(a, key), &comparable_json(b, key));
		if descending {
			ord.reverse()
		} else {
			ord
		}
	});
}

fn json_as_f64(value: &serde_json::Value) -> Option<f64> {
	match value {
		serde_json::Value::Number(n) => n.as_f64().filter(|v| v.is_finite()),
		serde_json::Value::String(s) => s.trim().parse::<f64>().ok().filter(|v| v.is_finite()),
		serde_json::Value::Bool(true) => Some(1.0),
		serde_json::Value::Bool(false) | serde_json::Value::Null | serde_json::Value::Array(_) | serde_json::Value::Object(_) => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn json_list(items: Vec<serde_json::Value>) -> SocketValue {
		SocketValue::List(items.into_iter().map(SocketValue::Json).collect())
	}

	fn json_map(entries: Vec<(&str, serde_json::Value)>) -> SocketValue {
		let mut m = std::collections::BTreeMap::new();
		for (k, v) in entries {
			m.insert(k.to_string(), SocketValue::Json(v));
		}
		SocketValue::Map(m)
	}

	#[tokio::test]
	async fn list_ops() {
		let items = json_list(vec![serde_json::json!(10), serde_json::json!(20), serde_json::json!(30)]);
		let inputs: InputMap = [("items".into(), items.clone())].into_iter().collect();
		let out = ListLenNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("len"), Some(&SocketValue::Int(3)));

		let inputs: InputMap = [("items".into(), items.clone()), ("index".into(), SocketValue::Int(1))]
			.into_iter()
			.collect();
		let out = ListGetNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("item"), Some(&SocketValue::Json(serde_json::json!(20))));

		let inputs: InputMap = [("items".into(), items), ("index".into(), SocketValue::Int(99))]
			.into_iter()
			.collect();
		let out = ListGetNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("item"), Some(&SocketValue::Json(serde_json::Value::Null)));

		let inputs: InputMap = [("items".into(), json_list(vec![]))].into_iter().collect();
		let out = ListIsEmptyNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("result"), Some(&SocketValue::Bool(true)));
	}

	#[tokio::test]
	async fn list_sl0_range_slice_map_filter_window_aggregate() {
		let inputs: InputMap = [
			("start".into(), SocketValue::Int(1)),
			("end".into(), SocketValue::Int(6)),
			("step".into(), SocketValue::Int(2)),
		]
		.into_iter()
		.collect();
		let out = ListRangeNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(
			out.data.get("items"),
			Some(&SocketValue::List(vec![
				SocketValue::Int(1),
				SocketValue::Int(3),
				SocketValue::Int(5)
			]))
		);

		let items = json_list(vec![
			serde_json::json!({"name":"a","score":1,"kind":"keep"}),
			serde_json::json!({"name":"b","score":2,"kind":"drop"}),
			serde_json::json!({"name":"c","score":3,"kind":"keep"}),
		]);
		let inputs: InputMap = [
			("items".into(), items.clone()),
			("start".into(), SocketValue::Int(1)),
			("count".into(), SocketValue::Int(2)),
		]
		.into_iter()
		.collect();
		let out = ListSliceNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("count"), Some(&SocketValue::Int(2)));

		let inputs: InputMap = [("items".into(), items.clone()), ("key".into(), SocketValue::String("score".into()))]
			.into_iter()
			.collect();
		let out = ListMapJsonGetNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(
			out.data.get("items"),
			Some(&SocketValue::List(vec![
				SocketValue::Json(serde_json::json!(1)),
				SocketValue::Json(serde_json::json!(2)),
				SocketValue::Json(serde_json::json!(3)),
			]))
		);

		let inputs: InputMap = [
			("items".into(), items.clone()),
			("key".into(), SocketValue::String("kind".into())),
			("equals".into(), SocketValue::Json(serde_json::json!("keep"))),
		]
		.into_iter()
		.collect();
		let out = ListFilterJsonEqNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("count"), Some(&SocketValue::Int(2)));

		let inputs: InputMap = [
			("items".into(), items.clone()),
			("size".into(), SocketValue::Int(2)),
			("step".into(), SocketValue::Int(1)),
		]
		.into_iter()
		.collect();
		let out = ListWindowNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("count"), Some(&SocketValue::Int(2)));

		let inputs: InputMap = [("items".into(), items), ("key".into(), SocketValue::String("score".into()))]
			.into_iter()
			.collect();
		let out = ListAggregateNumbersNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("count"), Some(&SocketValue::Int(3)));
		assert_eq!(out.data.get("sum"), Some(&SocketValue::Float(6.0)));
		assert_eq!(out.data.get("avg"), Some(&SocketValue::Float(2.0)));
	}

	#[tokio::test]
	async fn list_sort_top_k_and_binary_search_json() {
		let items = json_list(vec![
			serde_json::json!({"name":"b","score":2}),
			serde_json::json!({"name":"a","score":1}),
			serde_json::json!({"name":"c","score":3}),
		]);
		let inputs: InputMap = [("items".into(), items.clone()), ("key".into(), SocketValue::String("score".into()))]
			.into_iter()
			.collect();
		let sorted = ListSortJsonNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(
			sorted.data.get("items"),
			Some(&SocketValue::List(vec![
				SocketValue::Json(serde_json::json!({"name":"a","score":1})),
				SocketValue::Json(serde_json::json!({"name":"b","score":2})),
				SocketValue::Json(serde_json::json!({"name":"c","score":3})),
			]))
		);

		let inputs: InputMap = [
			("items".into(), items),
			("key".into(), SocketValue::String("score".into())),
			("k".into(), SocketValue::Int(2)),
			("descending".into(), SocketValue::Bool(true)),
		]
		.into_iter()
		.collect();
		let top = ListTopKJsonNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(
			top.data.get("items"),
			Some(&SocketValue::List(vec![
				SocketValue::Json(serde_json::json!({"name":"c","score":3})),
				SocketValue::Json(serde_json::json!({"name":"b","score":2})),
			]))
		);

		let inputs: InputMap = [
			("items".into(), sorted.data.get("items").unwrap().clone()),
			("key".into(), SocketValue::String("score".into())),
			("target".into(), SocketValue::Json(serde_json::json!(2))),
		]
		.into_iter()
		.collect();
		let found = ListBinarySearchJsonNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(found.data.get("found"), Some(&SocketValue::Bool(true)));
		assert_eq!(found.data.get("index"), Some(&SocketValue::Int(1)));
	}

	#[tokio::test]
	async fn map_ops() {
		let m = json_map(vec![("a", serde_json::json!(1)), ("b", serde_json::json!("hi"))]);
		let inputs: InputMap = [("m".into(), m.clone()), ("key".into(), SocketValue::String("a".into()))]
			.into_iter()
			.collect();
		let out = MapGetNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("value"), Some(&SocketValue::Json(serde_json::json!(1))));

		let inputs: InputMap = [("m".into(), m.clone()), ("key".into(), SocketValue::String("missing".into()))]
			.into_iter()
			.collect();
		let out = MapGetNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("value"), Some(&SocketValue::Json(serde_json::Value::Null)));

		let inputs: InputMap = [("m".into(), m.clone())].into_iter().collect();
		let out = MapKeysNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(
			out.data.get("keys"),
			Some(&SocketValue::List(vec![
				SocketValue::String("a".into()),
				SocketValue::String("b".into())
			]))
		);

		let inputs: InputMap = [("m".into(), m), ("key".into(), SocketValue::String("b".into()))]
			.into_iter()
			.collect();
		let out = MapHasNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("result"), Some(&SocketValue::Bool(true)));
	}

	#[tokio::test]
	async fn dictionary_generic_key_value_ops() {
		let entries = json_list(vec![
			serde_json::json!({"key":"scene","value":"intro"}),
			serde_json::json!({"key":"volume","value":0.8}),
		]);
		let inputs: InputMap = [("entries".into(), entries)].into_iter().collect();
		let built = DictionaryFromEntriesNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(built.data.get("count"), Some(&SocketValue::Int(2)));

		let dictionary = built.data.get("dictionary").unwrap().clone();
		let inputs: InputMap = [
			("dictionary".into(), dictionary.clone()),
			("key".into(), SocketValue::String("scene".into())),
		]
		.into_iter()
		.collect();
		let got = DictionaryGetNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(got.data.get("found"), Some(&SocketValue::Bool(true)));
		assert_eq!(got.data.get("value"), Some(&SocketValue::Json(serde_json::json!("intro"))));

		let inputs: InputMap = [
			("dictionary".into(), dictionary.clone()),
			("key".into(), SocketValue::String("scene".into())),
			("value".into(), SocketValue::Json(serde_json::json!("main"))),
		]
		.into_iter()
		.collect();
		let set = DictionarySetNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(set.data.get("replaced"), Some(&SocketValue::Bool(true)));

		let inputs: InputMap = [
			("dictionary".into(), set.data.get("dictionary").unwrap().clone()),
			("key".into(), SocketValue::String("volume".into())),
		]
		.into_iter()
		.collect();
		let removed = DictionaryRemoveNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(removed.data.get("removed"), Some(&SocketValue::Bool(true)));
		assert_eq!(removed.data.get("count"), Some(&SocketValue::Int(1)));

		let inputs: InputMap = [("dictionary".into(), removed.data.get("dictionary").unwrap().clone())]
			.into_iter()
			.collect();
		let keys = DictionaryKeysNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(
			keys.data.get("keys"),
			Some(&SocketValue::List(vec![SocketValue::String("scene".into())]))
		);
	}
}

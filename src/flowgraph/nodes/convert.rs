//! 型変換ノード（全て PureNode）。
//!
//! 暗黙変換は spec §2.1 に従い行われないため、型が違うデータを繋ぐときは
//! 明示的にこれらのノードを挟む。

use crate::flowgraph::node::{
	get_required_bool, get_required_bytes, get_required_float, get_required_int, get_required_json, get_required_string, ExecFireSet,
	InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PureNode,
};
use crate::flowgraph::socket::{SocketType, SocketValue};
use async_trait::async_trait;
use serde_json::Value as JsonValue;

// ----- int → string --------------------------------------------------------

pub struct BoolToStringNode;
impl NodeDescriptor for BoolToStringNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.bool_to_string".into(),
			title: "Bool → String".into(),
			category: "convert".into(),
			description: None,
			inputs: vec![PortSpec::input("value", "Value", SocketType::Bool)],
			outputs: vec![PortSpec::output("result", "Result", SocketType::String)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for BoolToStringNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let v = get_required_bool(inputs, "value")?;
		Ok(NodeOutput::new().set_data("result", SocketValue::String(v.to_string())))
	}
}

// ----- int → string --------------------------------------------------------

pub struct IntToStringNode;
impl NodeDescriptor for IntToStringNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.int_to_string".into(),
			title: "Int → String".into(),
			category: "convert".into(),
			description: None,
			inputs: vec![PortSpec::input("value", "Value", SocketType::Int)],
			outputs: vec![PortSpec::output("result", "Result", SocketType::String)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for IntToStringNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let v = get_required_int(inputs, "value")?;
		Ok(NodeOutput::new().set_data("result", SocketValue::String(v.to_string())))
	}
}

// ----- string → int --------------------------------------------------------

pub struct StringToIntNode;
impl NodeDescriptor for StringToIntNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.string_to_int".into(),
			title: "String → Int".into(),
			category: "convert".into(),
			description: Some("パース失敗はエラー halt".into()),
			inputs: vec![PortSpec::input("value", "Value", SocketType::String)],
			outputs: vec![PortSpec::output("result", "Result", SocketType::Int)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for StringToIntNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let s = get_required_string(inputs, "value")?;
		let v: i64 = s
			.trim()
			.parse()
			.map_err(|e| NodeExecError::Generic(anyhow::anyhow!("int parse error: {e} (input: {s:?})")))?;
		Ok(NodeOutput::new().set_data("result", SocketValue::Int(v)))
	}
}

// ----- float → string ------------------------------------------------------

pub struct FloatToStringNode;
impl NodeDescriptor for FloatToStringNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.float_to_string".into(),
			title: "Float → String".into(),
			category: "convert".into(),
			description: None,
			inputs: vec![PortSpec::input("value", "Value", SocketType::Float)],
			outputs: vec![PortSpec::output("result", "Result", SocketType::String)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for FloatToStringNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let v = get_required_float(inputs, "value")?;
		Ok(NodeOutput::new().set_data("result", SocketValue::String(v.to_string())))
	}
}

// ----- string → float ------------------------------------------------------

pub struct StringToFloatNode;
impl NodeDescriptor for StringToFloatNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.string_to_float".into(),
			title: "String → Float".into(),
			category: "convert".into(),
			description: Some("パース失敗はエラー halt".into()),
			inputs: vec![PortSpec::input("value", "Value", SocketType::String)],
			outputs: vec![PortSpec::output("result", "Result", SocketType::Float)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for StringToFloatNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let s = get_required_string(inputs, "value")?;
		let v: f64 = s
			.trim()
			.parse()
			.map_err(|e| NodeExecError::Generic(anyhow::anyhow!("float parse error: {e} (input: {s:?})")))?;
		Ok(NodeOutput::new().set_data("result", SocketValue::Float(v)))
	}
}

// ----- int → float ---------------------------------------------------------

pub struct IntToFloatNode;
impl NodeDescriptor for IntToFloatNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.int_to_float".into(),
			title: "Int → Float".into(),
			category: "convert".into(),
			description: None,
			inputs: vec![PortSpec::input("value", "Value", SocketType::Int)],
			outputs: vec![PortSpec::output("result", "Result", SocketType::Float)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for IntToFloatNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let v = get_required_int(inputs, "value")?;
		Ok(NodeOutput::new().set_data("result", SocketValue::Float(v as f64)))
	}
}

// ----- float → int ---------------------------------------------------------

pub struct FloatToIntNode;
impl NodeDescriptor for FloatToIntNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.float_to_int".into(),
			title: "Float → Int".into(),
			category: "convert".into(),
			description: Some("切り捨て（trunc）".into()),
			inputs: vec![PortSpec::input("value", "Value", SocketType::Float)],
			outputs: vec![PortSpec::output("result", "Result", SocketType::Int)],
			properties: vec![],
		}
	}
}
#[async_trait]
impl PureNode for FloatToIntNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let v = get_required_float(inputs, "value")?;
		Ok(NodeOutput::new().set_data("result", SocketValue::Int(v.trunc() as i64)))
	}
}

// ----- JSON / TOML / RON --------------------------------------------------

pub struct JsonToTomlNode;
impl NodeDescriptor for JsonToTomlNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.json_to_toml".into(),
			title: "JSON → TOML".into(),
			category: "convert".into(),
			description: Some("JSON 値を TOML 文字列へ変換する。".into()),
			inputs: vec![PortSpec::input("json", "JSON", SocketType::Json)],
			outputs: vec![PortSpec::output("toml", "TOML", SocketType::String)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for JsonToTomlNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let json = get_required_json(inputs, "json")?;
		let toml = toml::to_string_pretty(json).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("TOML serialize error: {e}")))?;
		Ok(NodeOutput::new().set_data("toml", SocketValue::String(toml)))
	}
}

pub struct TomlToJsonNode;
impl NodeDescriptor for TomlToJsonNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.toml_to_json".into(),
			title: "TOML → JSON".into(),
			category: "convert".into(),
			description: Some("TOML 文字列を JSON 値へ変換する。".into()),
			inputs: vec![PortSpec::input("toml", "TOML", SocketType::String)],
			outputs: vec![PortSpec::output("json", "JSON", SocketType::Json)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for TomlToJsonNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let toml_text = get_required_string(inputs, "toml")?;
		let value =
			toml::from_str::<toml::Value>(&toml_text).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("TOML parse error: {e}")))?;
		let json = serde_json::to_value(value).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("TOML to JSON error: {e}")))?;
		Ok(NodeOutput::new().set_data("json", SocketValue::Json(json)))
	}
}

pub struct TomlToRonNode;
impl NodeDescriptor for TomlToRonNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.toml_to_ron".into(),
			title: "TOML → RON".into(),
			category: "convert".into(),
			description: Some("TOML 文字列を JSON 値経由で RON 文字列へ変換する。".into()),
			inputs: vec![PortSpec::input("toml", "TOML", SocketType::String)],
			outputs: vec![PortSpec::output("ron", "RON", SocketType::String)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for TomlToRonNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let toml_text = get_required_string(inputs, "toml")?;
		let value =
			toml::from_str::<toml::Value>(&toml_text).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("TOML parse error: {e}")))?;
		let json = serde_json::to_value(value).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("TOML to JSON error: {e}")))?;
		let ron = ron::ser::to_string_pretty(&json, ron::ser::PrettyConfig::default())
			.map_err(|e| NodeExecError::Generic(anyhow::anyhow!("RON serialize error: {e}")))?;
		Ok(NodeOutput::new().set_data("ron", SocketValue::String(ron)))
	}
}

pub struct RonToTomlNode;
impl NodeDescriptor for RonToTomlNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.ron_to_toml".into(),
			title: "RON → TOML".into(),
			category: "convert".into(),
			description: Some("RON 文字列を JSON 値経由で TOML 文字列へ変換する。".into()),
			inputs: vec![PortSpec::input("ron", "RON", SocketType::String)],
			outputs: vec![PortSpec::output("toml", "TOML", SocketType::String)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for RonToTomlNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let ron_text = get_required_string(inputs, "ron")?;
		let json = ron::from_str::<JsonValue>(&ron_text).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("RON parse error: {e}")))?;
		let toml = toml::to_string_pretty(&json).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("TOML serialize error: {e}")))?;
		Ok(NodeOutput::new().set_data("toml", SocketValue::String(toml)))
	}
}

// ----- JSON / MessagePack -------------------------------------------------

pub struct JsonToMsgpackNode;
impl NodeDescriptor for JsonToMsgpackNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.json_to_msgpack".into(),
			title: "JSON → MessagePack".into(),
			category: "convert".into(),
			description: Some("JSON 値を MessagePack bytes へ変換する。".into()),
			inputs: vec![PortSpec::input("json", "JSON", SocketType::Json)],
			outputs: vec![
				PortSpec::output("bytes", "Bytes", SocketType::Bytes),
				PortSpec::output("byte_count", "Byte Count", SocketType::Int),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for JsonToMsgpackNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let json = get_required_json(inputs, "json")?;
		let bytes = rmp_serde::to_vec(json).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("MessagePack serialize error: {e}")))?;
		let byte_count = bytes.len() as i64;
		Ok(NodeOutput::new()
			.set_data("bytes", SocketValue::Bytes(bytes))
			.set_data("byte_count", SocketValue::Int(byte_count)))
	}
}

pub struct MsgpackToJsonNode;
impl NodeDescriptor for MsgpackToJsonNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.convert.msgpack_to_json".into(),
			title: "MessagePack → JSON".into(),
			category: "convert".into(),
			description: Some("MessagePack bytes を JSON 値へ変換する。".into()),
			inputs: vec![PortSpec::input("bytes", "Bytes", SocketType::Bytes)],
			outputs: vec![PortSpec::output("json", "JSON", SocketType::Json)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for MsgpackToJsonNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_p: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let bytes = get_required_bytes(inputs, "bytes")?;
		let json = rmp_serde::from_slice::<JsonValue>(bytes)
			.map_err(|e| NodeExecError::Generic(anyhow::anyhow!("MessagePack parse error: {e}")))?;
		Ok(NodeOutput::new().set_data("json", SocketValue::Json(json)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn int_string_roundtrip() {
		let inputs: InputMap = [("value".into(), SocketValue::Int(42))].into_iter().collect();
		let out = IntToStringNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("result"), Some(&SocketValue::String("42".into())));

		let inputs: InputMap = [("value".into(), SocketValue::String("42".into()))].into_iter().collect();
		let out = StringToIntNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("result"), Some(&SocketValue::Int(42)));
	}

	#[tokio::test]
	async fn bool_to_string_outputs_lowercase_literal() {
		let inputs: InputMap = [("value".into(), SocketValue::Bool(true))].into_iter().collect();
		let out = BoolToStringNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("result"), Some(&SocketValue::String("true".into())));
	}

	#[tokio::test]
	async fn string_to_int_fails_on_nonnumeric() {
		let inputs: InputMap = [("value".into(), SocketValue::String("abc".into()))].into_iter().collect();
		let e = StringToIntNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap_err();
		assert!(matches!(e, NodeExecError::Generic(_)));
	}

	#[tokio::test]
	async fn int_to_float_and_back() {
		let inputs: InputMap = [("value".into(), SocketValue::Int(3))].into_iter().collect();
		let out = IntToFloatNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("result"), Some(&SocketValue::Float(3.0)));

		let inputs: InputMap = [("value".into(), SocketValue::Float(3.7))].into_iter().collect();
		let out = FloatToIntNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("result"), Some(&SocketValue::Int(3)));
	}

	#[tokio::test]
	async fn float_string_roundtrip() {
		let inputs: InputMap = [("value".into(), SocketValue::Float(1.5))].into_iter().collect();
		let out = FloatToStringNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("result"), Some(&SocketValue::String("1.5".into())));

		let inputs: InputMap = [("value".into(), SocketValue::String("1.5".into()))].into_iter().collect();
		let out = StringToFloatNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(out.data.get("result"), Some(&SocketValue::Float(1.5)));
	}

	#[tokio::test]
	async fn json_toml_roundtrip_preserves_structure() {
		let json = serde_json::json!({
			"title": "UNVAC",
			"profile": {
				"name": "default",
				"enabled": true,
				"count": 2
			}
		});
		let inputs: InputMap = [("json".into(), SocketValue::Json(json.clone()))].into_iter().collect();
		let toml_out = JsonToTomlNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let toml_text = toml_out.data.get("toml").unwrap().as_str().unwrap().to_string();
		assert!(toml_text.contains("[profile]"));

		let inputs: InputMap = [("toml".into(), SocketValue::String(toml_text))].into_iter().collect();
		let json_out = TomlToJsonNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(json_out.data.get("json"), Some(&SocketValue::Json(json)));
	}

	#[tokio::test]
	async fn toml_ron_toml_keeps_key_values() {
		let toml_text = "title = \"UNVAC\"\n[profile]\nname = \"default\"\ncount = 2\n".to_string();
		let inputs: InputMap = [("toml".into(), SocketValue::String(toml_text))].into_iter().collect();
		let ron_out = TomlToRonNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let ron_text = ron_out.data.get("ron").unwrap().as_str().unwrap().to_string();
		assert!(ron_text.contains("UNVAC"));

		let inputs: InputMap = [("ron".into(), SocketValue::String(ron_text))].into_iter().collect();
		let toml_out = RonToTomlNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let toml_text = toml_out.data.get("toml").unwrap().as_str().unwrap();
		assert!(toml_text.contains("title = \"UNVAC\""));
		assert!(toml_text.contains("[profile]"));
		assert!(toml_text.contains("count = 2"));
	}

	#[tokio::test]
	async fn json_msgpack_roundtrip_preserves_structure() {
		let json = serde_json::json!({
			"title": "UNVAC",
			"count": 2,
			"enabled": true,
			"items": ["a", "b"]
		});
		let inputs: InputMap = [("json".into(), SocketValue::Json(json.clone()))].into_iter().collect();
		let packed = JsonToMsgpackNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		let bytes = packed.data.get("bytes").unwrap().as_bytes().unwrap().to_vec();
		assert!(!bytes.is_empty());
		assert_eq!(packed.data.get("byte_count"), Some(&SocketValue::Int(bytes.len() as i64)));

		let inputs: InputMap = [("bytes".into(), SocketValue::Bytes(bytes))].into_iter().collect();
		let unpacked = MsgpackToJsonNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert_eq!(unpacked.data.get("json"), Some(&SocketValue::Json(json)));
	}

	#[tokio::test]
	async fn msgpack_to_json_fails_on_invalid_bytes() {
		let inputs: InputMap = [("bytes".into(), SocketValue::Bytes(vec![0xc1]))].into_iter().collect();
		let err = MsgpackToJsonNode
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&inputs,
				&ExecFireSet::new(),
			)
			.await
			.unwrap_err();
		assert!(matches!(err, NodeExecError::Generic(_)));
	}
}

use crate::flowgraph::node::{
	get_optional_bool, get_required_bytes, get_required_json, get_required_string, EffectfulNode, ExecCtx, ExecFireSet, InputMap,
	NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, RecordedEffect,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use async_trait::async_trait;
use base64::Engine as _;
use serde_json::Value as JsonValue;

pub struct FileReadStringNode;
pub struct FileReadBytesNode;
pub struct FileReadJsonNode;
pub struct FileReadTomlNode;
pub struct FileReadRonNode;
pub struct FileWriteStringNode;
pub struct FileWriteBytesNode;
pub struct FileWriteJsonNode;
pub struct FileWriteTomlNode;
pub struct FileWriteRonNode;

impl NodeDescriptor for FileReadStringNode {
	fn describe(&self) -> NodeSpec {
		read_spec(
			"flowgraph.file.read_string",
			"File Read String",
			"テキストファイルを UTF-8 文字列として読み込む。Effectful",
			SocketType::String,
			"text",
			"Text",
		)
	}
}

impl NodeDescriptor for FileReadBytesNode {
	fn describe(&self) -> NodeSpec {
		read_spec(
			"flowgraph.file.read_bytes",
			"File Read Bytes",
			"ファイルを bytes として読み込む。Effectful",
			SocketType::Bytes,
			"bytes",
			"Bytes",
		)
	}
}

impl NodeDescriptor for FileReadJsonNode {
	fn describe(&self) -> NodeSpec {
		read_spec(
			"flowgraph.file.read_json",
			"File Read JSON",
			"JSON ファイルを読み込む。Effectful",
			SocketType::Json,
			"json",
			"JSON",
		)
	}
}

impl NodeDescriptor for FileReadTomlNode {
	fn describe(&self) -> NodeSpec {
		read_spec(
			"flowgraph.file.read_toml",
			"File Read TOML",
			"TOML ファイルを JSON 値へ変換して読み込む。Effectful",
			SocketType::Json,
			"json",
			"JSON",
		)
	}
}

impl NodeDescriptor for FileReadRonNode {
	fn describe(&self) -> NodeSpec {
		read_spec(
			"flowgraph.file.read_ron",
			"File Read RON",
			"RON ファイルを JSON 値へ変換して読み込む。Effectful",
			SocketType::Json,
			"json",
			"JSON",
		)
	}
}

impl NodeDescriptor for FileWriteStringNode {
	fn describe(&self) -> NodeSpec {
		write_spec(
			"flowgraph.file.write_string",
			"File Write String",
			"文字列を UTF-8 ファイルへ atomic rename で書き込む。Effectful",
			SocketType::String,
			"text",
			"Text",
			false,
		)
	}
}

impl NodeDescriptor for FileWriteBytesNode {
	fn describe(&self) -> NodeSpec {
		write_spec(
			"flowgraph.file.write_bytes",
			"File Write Bytes",
			"bytes をファイルへ atomic rename で書き込む。Effectful",
			SocketType::Bytes,
			"bytes",
			"Bytes",
			false,
		)
	}
}

impl NodeDescriptor for FileWriteJsonNode {
	fn describe(&self) -> NodeSpec {
		write_spec(
			"flowgraph.file.write_json",
			"File Write JSON",
			"JSON 値をファイルへ atomic rename で書き込む。Effectful",
			SocketType::Json,
			"json",
			"JSON",
			true,
		)
	}
}

impl NodeDescriptor for FileWriteTomlNode {
	fn describe(&self) -> NodeSpec {
		write_spec(
			"flowgraph.file.write_toml",
			"File Write TOML",
			"JSON 値を TOML としてファイルへ atomic rename で書き込む。Effectful",
			SocketType::Json,
			"json",
			"JSON",
			false,
		)
	}
}

impl NodeDescriptor for FileWriteRonNode {
	fn describe(&self) -> NodeSpec {
		write_spec(
			"flowgraph.file.write_ron",
			"File Write RON",
			"JSON 値を RON としてファイルへ atomic rename で書き込む。Effectful",
			SocketType::Json,
			"json",
			"JSON",
			true,
		)
	}
}

fn read_spec(feature: &str, title: &str, description: &str, value_type: SocketType, value_name: &str, value_label: &str) -> NodeSpec {
	NodeSpec {
		feature: feature.into(),
		title: title.into(),
		category: "file".into(),
		description: Some(format!("{description}。同梱素材は graph-file:assets/example を指定すると、このノードの定義元グラフから相対参照できます。")),
		inputs: vec![
			PortSpec::exec_input("exec_in", "Exec"),
			PortSpec::input("path", "Path", SocketType::String),
		],
		outputs: vec![
			PortSpec::exec_output("on_success", "On Success"),
			PortSpec::exec_output("on_error", "On Error"),
			PortSpec::output(value_name, value_label, value_type.clone()),
			PortSpec::output("bytes_read", "Bytes Read", SocketType::Int),
			PortSpec::output("error", "Error", SocketType::String),
			PortSpec::output("result", "Result", SocketType::Result(Box::new(value_type))),
		],
		properties: vec![],
	}
}

fn write_spec(
	feature: &str,
	title: &str,
	description: &str,
	value_type: SocketType,
	value_name: &str,
	value_label: &str,
	pretty_input: bool,
) -> NodeSpec {
	let mut inputs = vec![
		PortSpec::exec_input("exec_in", "Exec"),
		PortSpec::input("path", "Path", SocketType::String),
		PortSpec::input(value_name, value_label, value_type),
	];
	if pretty_input {
		inputs.push(PortSpec::input("pretty", "Pretty", SocketType::Bool).with_default(SocketValue::Bool(true)));
	}
	NodeSpec {
		feature: feature.into(),
		title: title.into(),
		category: "file".into(),
		description: Some(description.into()),
		inputs,
		outputs: vec![
			PortSpec::exec_output("on_success", "On Success"),
			PortSpec::exec_output("on_error", "On Error"),
			PortSpec::output("bytes_written", "Bytes Written", SocketType::Int),
			PortSpec::output("error", "Error", SocketType::String),
			PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Int))),
		],
		properties: vec![],
	}
}

#[async_trait]
impl EffectfulNode for FileReadStringNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		read_text_node(ctx, inputs, fired, "text", SocketType::String, |text| Ok(SocketValue::String(text))).await
	}
}

#[async_trait]
impl EffectfulNode for FileReadBytesNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let path = get_required_string(inputs, "path")?;
		let bytes = match read_bytes(ctx, &path).await {
			Ok(bytes) => bytes,
			Err(e) => return Ok(read_error_output("bytes", SocketType::Bytes, format!("read {path}: {e}"))),
		};
		let len = bytes.len() as i64;
		Ok(read_success_output("bytes", SocketValue::Bytes(bytes), len))
	}
}

#[async_trait]
impl EffectfulNode for FileReadJsonNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		read_text_node(ctx, inputs, fired, "json", SocketType::Json, |text| {
			serde_json::from_str::<JsonValue>(&text)
				.map(SocketValue::Json)
				.map_err(|e| format!("parse JSON: {e}"))
		})
		.await
	}
}

#[async_trait]
impl EffectfulNode for FileReadTomlNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		read_text_node(ctx, inputs, fired, "json", SocketType::Json, |text| {
			let value = toml::from_str::<toml::Value>(&text).map_err(|e| format!("parse TOML: {e}"))?;
			serde_json::to_value(value)
				.map(SocketValue::Json)
				.map_err(|e| format!("convert TOML to JSON: {e}"))
		})
		.await
	}
}

#[async_trait]
impl EffectfulNode for FileReadRonNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		read_text_node(ctx, inputs, fired, "json", SocketType::Json, |text| {
			ron::from_str::<JsonValue>(&text)
				.map(SocketValue::Json)
				.map_err(|e| format!("parse RON: {e}"))
		})
		.await
	}
}

async fn read_text_node<F>(
	ctx: &mut ExecCtx,
	inputs: &InputMap,
	fired: &ExecFireSet,
	value_name: &str,
	value_type: SocketType,
	parse: F,
) -> Result<NodeOutput, NodeExecError>
where
	F: FnOnce(String) -> Result<SocketValue, String>,
{
	if !fired.contains("exec_in") {
		return Ok(NodeOutput::new());
	}
	let path = get_required_string(inputs, "path")?;
	let text = match read_text(ctx, &path).await {
		Ok(text) => text,
		Err(e) => {
			let message = format!("read_to_string {path}: {e}");
			log::warn!("Flowgraph file read failed: {message}");
			return Ok(read_error_output(value_name, value_type, message));
		}
	};
	let len = text.as_bytes().len() as i64;
	match parse(text) {
		Ok(value) => Ok(read_success_output(value_name, value, len)),
		Err(e) => {
			log::warn!("Flowgraph structured file parse failed: path={path} error={e}");
			Ok(read_error_output(value_name, SocketType::Json, e))
		}
	}
}

async fn read_text(ctx: &mut ExecCtx, path: &str) -> Result<String, String> {
	if let Some(mock) = ctx
		.effect_mocks
		.as_ref()
		.and_then(|mocks| mocks.file_read_response(&ctx.node_id))
		.cloned()
	{
		ctx.log(format!("file.read mock: {path} -> node {}", ctx.node_id));
		if let Some(error) = mock.error {
			ctx.record_effect(RecordedEffect::file_read(
				ctx.node_id.clone(),
				path.to_string(),
				None,
				Some(error.clone()),
			));
			return Err(error);
		}
		ctx.record_effect(RecordedEffect::file_read(
			ctx.node_id.clone(),
			path.to_string(),
			Some(mock.contents.clone()),
			None,
		));
		return Ok(mock.contents);
	}
	tokio::fs::read_to_string(ctx.resolve_read_path(path)?).await.map_err(|e| e.to_string())
}

async fn read_bytes(ctx: &mut ExecCtx, path: &str) -> Result<Vec<u8>, String> {
	if let Some(mock) = ctx
		.effect_mocks
		.as_ref()
		.and_then(|mocks| mocks.file_read_response(&ctx.node_id))
		.cloned()
	{
		ctx.log(format!("file.read mock: {path} -> node {}", ctx.node_id));
		if let Some(error) = mock.error {
			ctx.record_effect(RecordedEffect::file_read(
				ctx.node_id.clone(),
				path.to_string(),
				None,
				Some(error.clone()),
			));
			return Err(error);
		}
		ctx.record_effect(RecordedEffect::file_read(
			ctx.node_id.clone(),
			path.to_string(),
			Some(mock.contents.clone()),
			None,
		));
		return Ok(mock.contents.into_bytes());
	}
	tokio::fs::read(ctx.resolve_read_path(path)?).await.map_err(|e| e.to_string())
}

fn read_success_output(value_name: &str, value: SocketValue, bytes_read: i64) -> NodeOutput {
	NodeOutput::new()
		.set_data(value_name, value.clone())
		.set_data("bytes_read", SocketValue::Int(bytes_read))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(value)))
		.fire_exec("on_success")
}

fn read_error_output(value_name: &str, result_type: SocketType, msg: impl Into<String>) -> NodeOutput {
	let msg = msg.into();
	let empty = result_type.default_value().unwrap_or(SocketValue::Json(JsonValue::Null));
	NodeOutput::new()
		.set_data(value_name, empty)
		.set_data("bytes_read", SocketValue::Int(0))
		.set_data("error", SocketValue::String(msg.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(msg).with_code("file.read")))
		.fire_exec("on_error")
}

#[async_trait]
impl EffectfulNode for FileWriteStringNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let path = get_required_string(inputs, "path")?;
		let text = get_required_string(inputs, "text")?;
		Ok(write_payload(ctx, &path, text.clone().into_bytes(), text).await)
	}
}

#[async_trait]
impl EffectfulNode for FileWriteBytesNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let path = get_required_string(inputs, "path")?;
		let bytes = get_required_bytes(inputs, "bytes")?.to_vec();
		let effect_contents = base64::engine::general_purpose::STANDARD.encode(&bytes);
		Ok(write_payload(ctx, &path, bytes, effect_contents).await)
	}
}

#[async_trait]
impl EffectfulNode for FileWriteJsonNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let path = get_required_string(inputs, "path")?;
		let json = get_required_json(inputs, "json")?;
		let pretty = get_optional_bool(inputs, "pretty", true)?;
		let text = if pretty {
			serde_json::to_string_pretty(json)
		} else {
			serde_json::to_string(json)
		}
		.map_err(|e| NodeExecError::Generic(anyhow::anyhow!("serialize JSON: {e}")))?;
		Ok(write_payload(ctx, &path, text.clone().into_bytes(), text).await)
	}
}

#[async_trait]
impl EffectfulNode for FileWriteTomlNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let path = get_required_string(inputs, "path")?;
		let json = get_required_json(inputs, "json")?;
		let text = toml::to_string_pretty(json).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("serialize TOML: {e}")))?;
		Ok(write_payload(ctx, &path, text.clone().into_bytes(), text).await)
	}
}

#[async_trait]
impl EffectfulNode for FileWriteRonNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let path = get_required_string(inputs, "path")?;
		let json = get_required_json(inputs, "json")?;
		let pretty = get_optional_bool(inputs, "pretty", true)?;
		let text = if pretty {
			ron::ser::to_string_pretty(json, ron::ser::PrettyConfig::default())
		} else {
			ron::to_string(json)
		}
		.map_err(|e| NodeExecError::Generic(anyhow::anyhow!("serialize RON: {e}")))?;
		Ok(write_payload(ctx, &path, text.clone().into_bytes(), text).await)
	}
}

async fn write_payload(ctx: &mut ExecCtx, path: &str, bytes: Vec<u8>, effect_contents: String) -> NodeOutput {
	let len = bytes.len() as i64;
	if let Some(mock) = ctx
		.effect_mocks
		.as_ref()
		.and_then(|mocks| mocks.file_write_response(&ctx.node_id))
		.cloned()
	{
		ctx.log(format!("file.write mock: {path} ({len} bytes) -> node {}", ctx.node_id));
		if let Some(error) = mock.error {
			ctx.record_effect(RecordedEffect::file_write(
				ctx.node_id.clone(),
				path.to_string(),
				len,
				effect_contents,
				Some(error.clone()),
			));
			return write_error_output(format!("write {path}: {error}"));
		}
		ctx.record_effect(RecordedEffect::file_write(
			ctx.node_id.clone(),
			path.to_string(),
			len,
			effect_contents,
			None,
		));
		return write_success_output(len);
	}
	let resolved = match ctx.resolve_write_path(path) {
		Ok(path) => path,
		Err(error) => return write_error_output(error),
	};
	if let Err(e) = crate::file_publish::replace_async(resolved, bytes).await {
		return write_error_output(format!("write {path}: {e}"));
	}
	write_success_output(len)
}

fn write_success_output(bytes_written: i64) -> NodeOutput {
	NodeOutput::new()
		.set_data("bytes_written", SocketValue::Int(bytes_written))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Int(bytes_written))))
		.fire_exec("on_success")
}

fn write_error_output(msg: impl Into<String>) -> NodeOutput {
	let msg = msg.into();
	NodeOutput::new()
		.set_data("bytes_written", SocketValue::Int(0))
		.set_data("error", SocketValue::String(msg.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(msg).with_code("file.write")))
		.fire_exec("on_error")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::node::{EffectMocks, FileReadMockResponse, FileWriteMockResponse};
	use std::collections::HashMap;
	use std::sync::Arc;

	fn fired() -> ExecFireSet {
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		fired
	}

	#[tokio::test]
	async fn read_json_uses_file_read_mock_and_parses_payload() {
		let mut mocks = EffectMocks::default();
		mocks.file_read.insert(
			"file1".into(),
			FileReadMockResponse {
				contents: r#"{"scene":"opening","count":2}"#.into(),
				error: None,
			},
		);
		let mut ctx = ExecCtx {
			node_id: "file1".into(),
			effect_mocks: Some(Arc::new(mocks)),
			..Default::default()
		};
		let inputs = HashMap::from([("path".into(), SocketValue::String("profile.json".into()))]);

		let out = FileReadJsonNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();

		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(out.data["bytes_read"], SocketValue::Int(29));
		assert_eq!(
			out.data["json"],
			SocketValue::Json(serde_json::json!({"scene": "opening", "count": 2}))
		);
		assert_eq!(ctx.recorded_effects.len(), 1);
		assert_eq!(ctx.recorded_effects[0].kind, "file_read");
	}

	#[tokio::test]
	async fn write_bytes_records_base64_contents_in_mock_mode() {
		let mut mocks = EffectMocks::default();
		mocks.file_write.insert("file1".into(), FileWriteMockResponse { error: None });
		let mut ctx = ExecCtx {
			node_id: "file1".into(),
			effect_mocks: Some(Arc::new(mocks)),
			..Default::default()
		};
		let inputs = HashMap::from([
			("path".into(), SocketValue::String("asset.bin".into())),
			("bytes".into(), SocketValue::Bytes(vec![0, 1, 255])),
		]);

		let out = FileWriteBytesNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();

		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(out.data["bytes_written"], SocketValue::Int(3));
		assert_eq!(ctx.recorded_effects[0].kind, "file_write");
		assert_eq!(ctx.recorded_effects[0].contents.as_deref(), Some("AAH/"));
	}

	#[tokio::test]
	async fn read_toml_returns_json_value() {
		let mut mocks = EffectMocks::default();
		mocks.file_read.insert(
			"file1".into(),
			FileReadMockResponse {
				contents: "title = \"UNVAC\"\n[profile]\nname = \"default\"\n".into(),
				error: None,
			},
		);
		let mut ctx = ExecCtx {
			node_id: "file1".into(),
			effect_mocks: Some(Arc::new(mocks)),
			..Default::default()
		};
		let inputs = HashMap::from([("path".into(), SocketValue::String("conf.toml".into()))]);

		let out = FileReadTomlNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();

		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(
			out.data["json"],
			SocketValue::Json(serde_json::json!({"title": "UNVAC", "profile": {"name": "default"}}))
		);
	}
}

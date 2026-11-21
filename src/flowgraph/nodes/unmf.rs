//! UNMotionFrame / UNMF/Z FlowGraph adapter nodes.
//!
//! UNMF is carried as a dedicated `unmf` socket in the graph. JSON remains a
//! compatible inspection/interchange shape through socket coercion.

use crate::flowgraph::node::{
	get_optional_bool, get_optional_float, get_optional_int, get_optional_string, get_required_bytes, get_required_motion_frame,
	get_required_unmf, EffectfulNode, ExecCtx, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec,
	PureNode,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use un_motion_frame_zenoh::{Subscriber, TopicMode, ZenohSessionBackend, ZenohTopicStrategy};
use vac_motion::unmf;
use zenoh::Wait;

const DEFAULT_UNMFZ_BASE_KEY: &str = "un-motion/frame";
const DEFAULT_UNMFZ_SCHEMA_MAJOR: i64 = 1;
const UNMFZ_MATCHING_WAIT: Duration = Duration::from_millis(500);

pub struct UnmfFromVmcNode;

impl NodeDescriptor for UnmfFromVmcNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmf.from_vmc".into(),
			title: "UNMF: From VMC".into(),
			category: "unmf".into(),
			description: Some("VMC互換 `motion_frame` をUNMotionFrameへ変換する。VMCはここで互換adapterになる。".into()),
			inputs: vec![
				PortSpec::input("frame", "VMC Frame", SocketType::MotionFrame),
				PortSpec::input("sequence", "Sequence", SocketType::Int).with_default(SocketValue::Int(0)),
			],
			outputs: vec![PortSpec::output("unmf", "UNMF", SocketType::Unmf)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for UnmfFromVmcNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let frame = get_required_motion_frame(inputs, "frame")?;
		let sequence = get_optional_int(inputs, "sequence", 0)?;
		let unmf = unmf::vmc_motion_frame_to_unmf(&frame, sequence.max(0) as u64);
		Ok(NodeOutput::new().set_data("unmf", SocketValue::Unmf(unmf)))
	}
}

pub struct UnmfToVmcNode;

impl NodeDescriptor for UnmfToVmcNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmf.to_vmc".into(),
			title: "UNMF: To VMC".into(),
			category: "unmf".into(),
			description: Some("UNMotionFrameをVMC互換 `motion_frame` へ変換する。既存VMC送出ノードとの接続用。".into()),
			inputs: vec![PortSpec::input("unmf", "UNMF", SocketType::Unmf)],
			outputs: vec![PortSpec::output("frame", "VMC Frame", SocketType::MotionFrame)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for UnmfToVmcNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let unmf = unmf_from_input(inputs, "unmf")?;
		let frame = unmf::unmf_to_vmc_motion_frame(&unmf);
		Ok(NodeOutput::new().set_data("frame", SocketValue::MotionFrame(frame)))
	}
}

pub struct UnmfzEncodeNode;

impl NodeDescriptor for UnmfzEncodeNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmfz.encode".into(),
			title: "UNMF/Z: Encode".into(),
			category: "unmf".into(),
			description: Some("UNMotionFrameをUNMF/Z MessagePack payload bytesへエンコードする。".into()),
			inputs: vec![PortSpec::input("unmf", "UNMF", SocketType::Unmf)],
			outputs: vec![PortSpec::output("payload", "Payload", SocketType::Bytes)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for UnmfzEncodeNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let unmf = unmf_from_input(inputs, "unmf")?;
		let payload = unmf::encode_unmfz_payload(&unmf).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("{e}")))?;
		Ok(NodeOutput::new().set_data("payload", SocketValue::Bytes(payload)))
	}
}

pub struct UnmfzDecodeNode;

impl NodeDescriptor for UnmfzDecodeNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmfz.decode".into(),
			title: "UNMF/Z: Decode".into(),
			category: "unmf".into(),
			description: Some("UNMF/Z MessagePack payload bytesをUNMotionFrameへデコードする。".into()),
			inputs: vec![PortSpec::input("payload", "Payload", SocketType::Bytes)],
			outputs: vec![PortSpec::output("unmf", "UNMF", SocketType::Unmf)],
			properties: vec![],
		}
	}
}

pub struct UnmfzResolveEndpointNode;

impl NodeDescriptor for UnmfzResolveEndpointNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmfz.resolve_endpoint".into(),
			title: "UNMF/Z: Resolve Endpoint".into(),
			category: "unmf".into(),
			description: Some("UNAA/Z app endpoint metadata から UNMF/Z の base key と topic mode を解決する。".into()),
			inputs: vec![
				PortSpec::input("endpoints", "Endpoints Metadata", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("app_id", "App ID", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("endpoint_id", "Endpoint ID", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("direction", "Direction", SocketType::String)
					.with_default(SocketValue::String("output".into()))
					.with_closed_string_variants(["input", "output", "both"]),
			],
			outputs: vec![
				PortSpec::output("found", "Found", SocketType::Bool),
				PortSpec::output("base_key_expr", "Base Key", SocketType::String),
				PortSpec::output("topic_mode", "Topic Mode", SocketType::String),
				PortSpec::output("schema_major", "Schema Major", SocketType::Int),
				PortSpec::output("endpoint", "Endpoint", SocketType::Json),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for UnmfzResolveEndpointNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let endpoints = optional_json(inputs, "endpoints").unwrap_or(&JsonValue::Null);
		let app_id = get_optional_string(inputs, "app_id", "")?;
		let endpoint_id = get_optional_string(inputs, "endpoint_id", "")?;
		let direction = get_optional_string(inputs, "direction", "output")?;
		let Some(endpoint) = find_unmfz_endpoint(endpoints, &app_id, &endpoint_id, &direction) else {
			return Ok(NodeOutput::new()
				.set_data("found", SocketValue::Bool(false))
				.set_data("base_key_expr", SocketValue::String(DEFAULT_UNMFZ_BASE_KEY.into()))
				.set_data("topic_mode", SocketValue::String("frame".into()))
				.set_data("schema_major", SocketValue::Int(DEFAULT_UNMFZ_SCHEMA_MAJOR))
				.set_data("endpoint", SocketValue::Json(JsonValue::Null)));
		};
		let resolved = endpoint_strategy_from_json(endpoint);
		Ok(NodeOutput::new()
			.set_data("found", SocketValue::Bool(true))
			.set_data("base_key_expr", SocketValue::String(resolved.base_key_expr))
			.set_data("topic_mode", SocketValue::String(resolved.topic_mode))
			.set_data("schema_major", SocketValue::Int(resolved.schema_major as i64))
			.set_data("endpoint", SocketValue::Json(endpoint.clone())))
	}
}

#[derive(Default)]
pub struct UnmfzPublishNode {
	backend: Mutex<Option<ZenohSessionBackend>>,
}

impl NodeDescriptor for UnmfzPublishNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmfz.publish".into(),
			title: "UNMF/Z: Publish".into(),
			category: "unmf".into(),
			description: Some("UNMotionFrame を UNMF/Z MessagePack として Zenoh に publish する。".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("unmf", "UNMF", SocketType::Unmf),
				PortSpec::input("base_key_expr", "Base Key", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("topic_mode", "Topic Mode", SocketType::String)
					.with_default(SocketValue::String(String::new()))
					.with_closed_string_variants(["", "frame", "primary_source", "stream_id"]),
				PortSpec::input("schema_major", "Schema Major", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("endpoint", "UNAA/Z Endpoint", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("dry_run", "Dry Run", SocketType::Bool).with_default(SocketValue::Bool(false)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("key_expr", "Key Expr", SocketType::String),
				PortSpec::output("bytes_sent", "Bytes Sent", SocketType::Int),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Int))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for UnmfzPublishNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired_exec.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let frame = unmf_from_input(inputs, "unmf")?;
		let strategy = strategy_from_inputs(inputs)?;
		let transport = unmfz_transport_from_ctx(ctx).await;
		let key_expr = strategy.key_expr_for_frame(&frame);
		let payload = unmf::encode_unmfz_payload(&frame).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("{e}")))?;
		if get_optional_bool(inputs, "dry_run", false)? {
			ctx.log(format!("unmfz.publish dry-run: {key_expr} ({} bytes)", payload.len()));
			return Ok(unmfz_publish_ok(key_expr, payload.len()));
		}
		let result = self
			.backend
			.lock()
			.map_err(|_| "UNMF/Z publisher backend lock poisoned".to_string())
			.and_then(|mut backend_slot| {
				if backend_slot.is_none() {
					*backend_slot = Some(transport.publisher_backend()?);
				}
				publish_unmfz_with_matching(
					backend_slot.as_ref().expect("publisher backend initialized"),
					&key_expr,
					&payload,
					UNMFZ_MATCHING_WAIT,
				)
			});
		match result {
			Ok(()) => {
				log::debug!("《UNMF/Z Publish》 key={} bytes={}", key_expr, payload.len());
				Ok(unmfz_publish_ok(key_expr, payload.len()))
			}
			Err(e) => {
				log::warn!("《UNMF/Z Publish》 key={} error={}", key_expr, e);
				Ok(unmfz_publish_err(key_expr, e))
			}
		}
	}
}

pub(super) fn publish_unmfz_with_matching(
	backend: &ZenohSessionBackend,
	key_expr: &str,
	payload: &[u8],
	timeout: Duration,
) -> Result<(), String> {
	let publisher = backend
		.session()
		.declare_publisher(key_expr.to_string())
		.wait()
		.map_err(|e| format!("zenoh declare publisher failed on {key_expr}: {e}"))?;
	let deadline = Instant::now() + timeout;
	while Instant::now() < deadline {
		let matching = publisher
			.matching_status()
			.wait()
			.map_err(|e| format!("zenoh matching status failed on {key_expr}: {e}"))?;
		if matching.matching() {
			break;
		}
		std::thread::sleep(Duration::from_millis(10));
	}
	publisher
		.put(payload.to_vec())
		.wait()
		.map_err(|e| format!("zenoh put failed on {key_expr}: {e}"))
}

pub struct UnmfzSubscribeNode;

impl NodeDescriptor for UnmfzSubscribeNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmfz.subscribe".into(),
			title: "UNMF/Z: Subscribe Once".into(),
			category: "unmf".into(),
			description: Some("Zenoh の UNMF/Z topic を購読し、exec 発火ごとに最大 1 フレーム受信する。".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("base_key_expr", "Base Key", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("topic_mode", "Topic Mode", SocketType::String)
					.with_default(SocketValue::String(String::new()))
					.with_closed_string_variants(["", "frame", "primary_source", "stream_id"]),
				PortSpec::input("schema_major", "Schema Major", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("endpoint", "UNAA/Z Endpoint", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("timeout_ms", "Timeout ms", SocketType::Int).with_default(SocketValue::Int(0)),
			],
			outputs: vec![
				PortSpec::exec_output("on_frame", "On Frame"),
				PortSpec::exec_output("on_timeout", "On Timeout"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("found", "Found", SocketType::Bool),
				PortSpec::output("unmf", "UNMF", SocketType::Unmf),
				PortSpec::output("key_expr", "Key Expr", SocketType::String),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Unmf))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for UnmfzSubscribeNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired_exec.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let strategy = strategy_from_inputs(inputs)?;
		let transport = unmfz_transport_from_ctx(ctx).await;
		let key_expr = strategy.subscribe_key_expr();
		let timeout_ms = get_optional_int(inputs, "timeout_ms", 0)?.max(0) as u64;
		let result = transport
			.subscriber_backend()
			.map_err(un_motion_frame_zenoh::Error::transport)
			.and_then(|mut backend| {
				let subscriber = Subscriber::declare(&mut backend, strategy)?;
				if timeout_ms == 0 {
					subscriber.try_recv_frame()
				} else {
					subscriber.recv_frame_timeout(Duration::from_millis(timeout_ms))
				}
			});
		match result {
			Ok(Some(frame)) => Ok(NodeOutput::new()
				.set_data("found", SocketValue::Bool(true))
				.set_data("unmf", SocketValue::Unmf(frame.clone()))
				.set_data("key_expr", SocketValue::String(key_expr))
				.set_data("error", SocketValue::String(String::new()))
				.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Unmf(frame))))
				.fire_exec("on_frame")),
			Ok(None) => Ok(NodeOutput::new()
				.set_data("found", SocketValue::Bool(false))
				.set_data("unmf", SocketValue::Unmf(unmf::UnMotionFrame::new(0)))
				.set_data("key_expr", SocketValue::String(key_expr))
				.set_data("error", SocketValue::String(String::new()))
				.set_data(
					"result",
					SocketValue::Result(FlowResult::err("timeout").with_code("unmfz.subscribe.timeout")),
				)
				.fire_exec("on_timeout")),
			Err(e) => Ok(unmfz_subscribe_err(key_expr, e.to_string())),
		}
	}
}

async fn unmfz_transport_from_ctx(ctx: &ExecCtx) -> crate::unmfz_transport::UnmfzTransportRuntime {
	let Some(state) = ctx.state_handle.as_ref().and_then(std::sync::Weak::upgrade) else {
		return crate::unmfz_transport::UnmfzTransportRuntime::default();
	};
	let transport = state.read().await.unmfz_transport.clone();
	transport
}

#[async_trait]
impl PureNode for UnmfzDecodeNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let payload = get_required_bytes(inputs, "payload")?;
		let frame = unmf::decode_unmfz_payload(payload).map_err(|e| NodeExecError::Generic(anyhow::anyhow!("{e}")))?;
		Ok(NodeOutput::new().set_data("unmf", SocketValue::Unmf(frame)))
	}
}

pub struct UnmfExtractBoneNode;

impl NodeDescriptor for UnmfExtractBoneNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmf.extract_bone".into(),
			title: "UNMF: Extract Bone".into(),
			category: "unmf".into(),
			description: Some(
				"UNMotionFrameからhumanoid bone transformを取り出す。bone名はVMC/Unity HumanBodyBones名を受け付ける。".into(),
			),
			inputs: vec![
				PortSpec::input("unmf", "UNMF", SocketType::Unmf),
				PortSpec::input("bone", "Bone", SocketType::String).with_default(SocketValue::String("Head".into())),
			],
			outputs: vec![
				PortSpec::output("found", "Found", SocketType::Bool),
				PortSpec::output("bone", "Bone", SocketType::String),
				PortSpec::output("pose", "Pose", SocketType::Json),
				PortSpec::output("px", "PX", SocketType::Float),
				PortSpec::output("py", "PY", SocketType::Float),
				PortSpec::output("pz", "PZ", SocketType::Float),
				PortSpec::output("rx", "RX", SocketType::Float),
				PortSpec::output("ry", "RY", SocketType::Float),
				PortSpec::output("rz", "RZ", SocketType::Float),
				PortSpec::output("rw", "RW", SocketType::Float),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for UnmfExtractBoneNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let frame = unmf_from_input(inputs, "unmf")?;
		let bone_name = get_optional_string(inputs, "bone", "Head")?;
		let Some((bone, sample)) = unmf::extract_bone(&frame, &bone_name) else {
			return Ok(bone_output(false, bone_name, JsonValue::Null, [0.0; 3], [0.0, 0.0, 0.0, 1.0]));
		};
		let translation = sample.transform.translation;
		let rotation = sample.transform.rotation;
		let pos = translation.map(|p| [p.x as f64, p.y as f64, p.z as f64]).unwrap_or([0.0; 3]);
		let rot = rotation
			.map(|r| [r.x as f64, r.y as f64, r.z as f64, r.w as f64])
			.unwrap_or([0.0, 0.0, 0.0, 1.0]);
		let name = unmf::unmf_bone_to_vmc_name(bone).unwrap_or("Unknown").to_string();
		let pose = json!({
			"bone": name,
			"position": pos,
			"rotation": rot,
			"confidence": sample.confidence,
			"state": format!("{:?}", sample.state),
		});
		Ok(bone_output(true, name, pose, pos, rot))
	}
}

pub struct UnmfExtractBlendshapeNode;

impl NodeDescriptor for UnmfExtractBlendshapeNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmf.extract_blendshape".into(),
			title: "UNMF: Extract BlendShape".into(),
			category: "unmf".into(),
			description: Some(
				"UNMotionFrameからexpression sampleを取り出す。nameは生の`Joy`を正とし、旧UNVAC互換として`vmc:Joy`も受け付ける。".into(),
			),
			inputs: vec![
				PortSpec::input("unmf", "UNMF", SocketType::Unmf),
				PortSpec::input("name", "Name", SocketType::String).with_default(SocketValue::String(String::new())),
			],
			outputs: vec![
				PortSpec::output("found", "Found", SocketType::Bool),
				PortSpec::output("name", "Name", SocketType::String),
				PortSpec::output("value", "Value", SocketType::Float),
				PortSpec::output("expression", "Expression", SocketType::Json),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for UnmfExtractBlendshapeNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let frame = unmf_from_input(inputs, "unmf")?;
		let name = get_optional_string(inputs, "name", "")?;
		let Some(sample) = unmf::extract_expression(&frame, &name) else {
			return Ok(NodeOutput::new()
				.set_data("found", SocketValue::Bool(false))
				.set_data("name", SocketValue::String(String::new()))
				.set_data("value", SocketValue::Float(0.0))
				.set_data("expression", SocketValue::Json(JsonValue::Null)));
		};
		Ok(NodeOutput::new()
			.set_data("found", SocketValue::Bool(true))
			.set_data("name", SocketValue::String(sample.name.clone()))
			.set_data("value", SocketValue::Float(sample.value as f64))
			.set_data(
				"expression",
				SocketValue::Json(serde_json::to_value(sample).unwrap_or(JsonValue::Null)),
			))
	}
}

pub struct UnmfFilterNode;

impl NodeDescriptor for UnmfFilterNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmf.filter".into(),
			title: "UNMF: Filter".into(),
			category: "unmf".into(),
			description: Some("UNMotionFrameのhumanoid bonesとexpressionsを絞り込む。bone_namesはカンマ区切り、空なら全bone維持。".into()),
			inputs: vec![
				PortSpec::input("unmf", "UNMF", SocketType::Unmf),
				PortSpec::input("bone_names", "Bone Names", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("expression_contains", "Expression Contains", SocketType::String)
					.with_default(SocketValue::String(String::new())),
			],
			outputs: vec![PortSpec::output("unmf", "UNMF", SocketType::Unmf)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for UnmfFilterNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let frame = unmf_from_input(inputs, "unmf")?;
		let bone_names = get_optional_string(inputs, "bone_names", "")?;
		let expression_contains = get_optional_string(inputs, "expression_contains", "")?;
		let filtered = unmf::filter_unmf(&frame, &bone_names, &expression_contains);
		Ok(NodeOutput::new().set_data("unmf", SocketValue::Unmf(filtered)))
	}
}

pub struct UnmfMapNode;

impl NodeDescriptor for UnmfMapNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unmf.map".into(),
			title: "UNMF: Map".into(),
			category: "unmf".into(),
			description: Some("UNMotionFrameの位置translationとexpression valueをスケールする。expressionは既定で0..1にclampする。".into()),
			inputs: vec![
				PortSpec::input("unmf", "UNMF", SocketType::Unmf),
				PortSpec::input("translation_scale", "Translation Scale", SocketType::Float).with_default(SocketValue::Float(1.0)),
				PortSpec::input("expression_scale", "Expression Scale", SocketType::Float).with_default(SocketValue::Float(1.0)),
				PortSpec::input("clamp_expressions", "Clamp Expressions", SocketType::Bool).with_default(SocketValue::Bool(true)),
			],
			outputs: vec![PortSpec::output("unmf", "UNMF", SocketType::Unmf)],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for UnmfMapNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let frame = unmf_from_input(inputs, "unmf")?;
		let translation_scale = get_optional_float(inputs, "translation_scale", 1.0)?;
		let expression_scale = get_optional_float(inputs, "expression_scale", 1.0)?;
		let clamp_expressions = get_optional_bool(inputs, "clamp_expressions", true)?;
		let mapped = unmf::map_unmf(&frame, translation_scale, expression_scale, clamp_expressions)
			.map_err(|e| NodeExecError::Generic(anyhow::anyhow!("{e}")))?;
		Ok(NodeOutput::new().set_data("unmf", SocketValue::Unmf(mapped)))
	}
}

fn unmf_from_input(inputs: &InputMap, key: &str) -> Result<unmf::UnMotionFrame, NodeExecError> {
	get_required_unmf(inputs, key)
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedEndpointStrategy {
	pub(super) base_key_expr: String,
	pub(super) topic_mode: String,
	pub(super) schema_major: u16,
}

pub(super) fn strategy_from_inputs(inputs: &InputMap) -> Result<ZenohTopicStrategy, NodeExecError> {
	let resolved = resolved_strategy_from_inputs(inputs)?;
	Ok(ZenohTopicStrategy::new(resolved.base_key_expr, parse_topic_mode(&resolved.topic_mode)?).with_schema_major(resolved.schema_major))
}

pub(super) fn resolved_strategy_from_inputs(inputs: &InputMap) -> Result<ResolvedEndpointStrategy, NodeExecError> {
	let endpoint = optional_json(inputs, "endpoint").unwrap_or(&JsonValue::Null);
	let resolved = endpoint_strategy_from_json(endpoint);
	let base = get_optional_string(inputs, "base_key_expr", "")?;
	let mode = get_optional_string(inputs, "topic_mode", "")?;
	let schema_major = get_optional_int(inputs, "schema_major", 0)?;
	let base = if base.trim().is_empty() {
		resolved.base_key_expr
	} else {
		base.trim().trim_end_matches('/').to_string()
	};
	let mode = if mode.trim().is_empty() {
		resolved.topic_mode
	} else {
		mode.trim().to_string()
	};
	let schema_major = if schema_major <= 0 {
		resolved.schema_major
	} else {
		u16::try_from(schema_major).map_err(|_| NodeExecError::Generic(anyhow::anyhow!("schema_major out of u16 range")))?
	};
	parse_topic_mode(&mode)?;
	Ok(ResolvedEndpointStrategy {
		base_key_expr: base,
		topic_mode: mode,
		schema_major,
	})
}

fn parse_topic_mode(input: &str) -> Result<TopicMode, NodeExecError> {
	match input.trim().to_ascii_lowercase().as_str() {
		"" | "frame" => Ok(TopicMode::Frame),
		"primary_source" | "by_primary_source" | "source" => Ok(TopicMode::ByPrimarySource),
		"stream_id" | "by_stream_id" | "stream" => Ok(TopicMode::ByStreamId),
		other => Err(NodeExecError::Generic(anyhow::anyhow!("unknown UNMF/Z topic_mode: {other}"))),
	}
}

fn endpoint_strategy_from_json(endpoint: &JsonValue) -> ResolvedEndpointStrategy {
	let base_key_expr = endpoint
		.pointer("/base_key_expr")
		.or_else(|| endpoint.pointer("/baseKeyExpr"))
		.or_else(|| endpoint.pointer("/unmfz/base_key_expr"))
		.or_else(|| endpoint.pointer("/unmfz/baseKeyExpr"))
		.and_then(JsonValue::as_str)
		.filter(|s| !s.trim().is_empty())
		.unwrap_or(DEFAULT_UNMFZ_BASE_KEY)
		.trim()
		.trim_end_matches('/')
		.to_string();
	let topic_mode = endpoint
		.pointer("/topic_mode")
		.or_else(|| endpoint.pointer("/topicMode"))
		.or_else(|| endpoint.pointer("/unmfz/topic_mode"))
		.or_else(|| endpoint.pointer("/unmfz/topicMode"))
		.and_then(JsonValue::as_str)
		.filter(|s| !s.trim().is_empty())
		.unwrap_or("frame")
		.to_string();
	let schema_major = endpoint
		.pointer("/schema_major")
		.or_else(|| endpoint.pointer("/schemaMajor"))
		.or_else(|| endpoint.pointer("/unmfz/schema_major"))
		.or_else(|| endpoint.pointer("/unmfz/schemaMajor"))
		.and_then(JsonValue::as_u64)
		.and_then(|n| u16::try_from(n).ok())
		.unwrap_or(DEFAULT_UNMFZ_SCHEMA_MAJOR as u16);
	ResolvedEndpointStrategy {
		base_key_expr,
		topic_mode,
		schema_major,
	}
}

fn optional_json<'a>(inputs: &'a InputMap, key: &str) -> Option<&'a JsonValue> {
	inputs.get(key).and_then(|v| v.as_json().ok())
}

fn find_unmfz_endpoint<'a>(value: &'a JsonValue, app_id: &str, endpoint_id: &str, direction: &str) -> Option<&'a JsonValue> {
	let candidates = endpoint_candidates(value);
	candidates.into_iter().find(|endpoint| {
		let protocol_ok = json_str_any(endpoint, &["protocol", "kind", "type"])
			.map(|s| s.eq_ignore_ascii_case("unmfz") || s.eq_ignore_ascii_case("unmf/z"))
			.unwrap_or_else(|| endpoint.get("unmfz").is_some());
		let app_ok = app_id.trim().is_empty()
			|| json_str_any(endpoint, &["app_id", "appId"])
				.map(|s| s == app_id.trim())
				.unwrap_or(false);
		let endpoint_ok = endpoint_id.trim().is_empty()
			|| json_str_any(endpoint, &["id", "endpoint_id", "endpointId", "name"])
				.map(|s| s == endpoint_id.trim())
				.unwrap_or(false);
		let direction_ok = direction_matches(json_str_any(endpoint, &["direction"]).unwrap_or("both"), direction);
		protocol_ok && app_ok && endpoint_ok && direction_ok
	})
}

fn endpoint_candidates(value: &JsonValue) -> Vec<&JsonValue> {
	if let Some(arr) = value.as_array() {
		return arr.iter().collect();
	}
	if let Some(arr) = value.get("endpoints").and_then(JsonValue::as_array) {
		return arr.iter().collect();
	}
	if value.is_object() {
		return vec![value];
	}
	Vec::new()
}

fn json_str_any<'a>(value: &'a JsonValue, keys: &[&str]) -> Option<&'a str> {
	keys.iter().find_map(|key| value.get(*key).and_then(JsonValue::as_str))
}

fn direction_matches(endpoint: &str, requested: &str) -> bool {
	let endpoint = endpoint.trim().to_ascii_lowercase();
	let requested = requested.trim().to_ascii_lowercase();
	endpoint.is_empty() || endpoint == "both" || requested.is_empty() || requested == "both" || endpoint == requested
}

fn unmfz_publish_ok(key_expr: String, bytes: usize) -> NodeOutput {
	NodeOutput::new()
		.set_data("key_expr", SocketValue::String(key_expr))
		.set_data("bytes_sent", SocketValue::Int(bytes as i64))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Int(bytes as i64))))
		.fire_exec("on_success")
}

fn unmfz_publish_err(key_expr: String, error: String) -> NodeOutput {
	NodeOutput::new()
		.set_data("key_expr", SocketValue::String(key_expr))
		.set_data("bytes_sent", SocketValue::Int(0))
		.set_data("error", SocketValue::String(error.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(error).with_code("unmfz.publish")))
		.fire_exec("on_error")
}

fn unmfz_subscribe_err(key_expr: String, error: String) -> NodeOutput {
	NodeOutput::new()
		.set_data("found", SocketValue::Bool(false))
		.set_data("unmf", SocketValue::Unmf(unmf::UnMotionFrame::new(0)))
		.set_data("key_expr", SocketValue::String(key_expr))
		.set_data("error", SocketValue::String(error.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(error).with_code("unmfz.subscribe")))
		.fire_exec("on_error")
}

fn bone_output(found: bool, bone: String, pose: JsonValue, pos: [f64; 3], rot: [f64; 4]) -> NodeOutput {
	NodeOutput::new()
		.set_data("found", SocketValue::Bool(found))
		.set_data("bone", SocketValue::String(bone))
		.set_data("pose", SocketValue::Json(pose))
		.set_data("px", SocketValue::Float(pos[0]))
		.set_data("py", SocketValue::Float(pos[1]))
		.set_data("pz", SocketValue::Float(pos[2]))
		.set_data("rx", SocketValue::Float(rot[0]))
		.set_data("ry", SocketValue::Float(rot[1]))
		.set_data("rz", SocketValue::Float(rot[2]))
		.set_data("rw", SocketValue::Float(rot[3]))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::node::PureEvalHost;
	use crate::flowgraph::vmc::{VMC_EXT_BLEND_VAL, VMC_EXT_BONE_POS};
	use crate::motion::{MotionFrame, OscMessageWire};

	fn fired_exec() -> ExecFireSet {
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		fired
	}

	#[tokio::test]
	async fn from_vmc_extract_and_unmfz_roundtrip() {
		let frame = MotionFrame {
			byte_len: 0,
			osc_messages: vec![
				OscMessageWire {
					address: VMC_EXT_BONE_POS.into(),
					args: vec![
						json!("Head"),
						json!(1.0),
						json!(2.0),
						json!(3.0),
						json!(0.0),
						json!(0.0),
						json!(0.0),
						json!(1.0),
					],
				},
				OscMessageWire {
					address: VMC_EXT_BLEND_VAL.into(),
					args: vec![json!("Joy"), json!(0.5)],
				},
			],
		};
		let mut inputs = InputMap::new();
		inputs.insert("frame".into(), SocketValue::MotionFrame(frame));
		let out = UnmfFromVmcNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &inputs, &ExecFireSet::new())
			.await
			.unwrap();
		let unmf_value = out.data.get("unmf").unwrap().clone();

		let mut extract_inputs = InputMap::new();
		extract_inputs.insert("unmf".into(), unmf_value.clone());
		extract_inputs.insert("name".into(), SocketValue::String("Joy".into()));
		let extracted = UnmfExtractBlendshapeNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &extract_inputs, &ExecFireSet::new())
			.await
			.unwrap();
		assert_eq!(extracted.data.get("found"), Some(&SocketValue::Bool(true)));
		assert_eq!(extracted.data.get("value"), Some(&SocketValue::Float(0.5)));

		let mut encode_inputs = InputMap::new();
		encode_inputs.insert("unmf".into(), unmf_value);
		let encoded = UnmfzEncodeNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &encode_inputs, &ExecFireSet::new())
			.await
			.unwrap();
		let payload = encoded.data.get("payload").unwrap().as_bytes().unwrap().to_vec();
		let mut decode_inputs = InputMap::new();
		decode_inputs.insert("payload".into(), SocketValue::Bytes(payload));
		let decoded = UnmfzDecodeNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &decode_inputs, &ExecFireSet::new())
			.await
			.unwrap();
		assert!(decoded.data.get("unmf").unwrap().as_unmf().is_ok());
	}

	#[tokio::test]
	async fn filter_and_map_nodes_transform_unmf() {
		let frame = MotionFrame {
			byte_len: 0,
			osc_messages: vec![
				OscMessageWire {
					address: VMC_EXT_BONE_POS.into(),
					args: vec![
						json!("Head"),
						json!(1.0),
						json!(2.0),
						json!(3.0),
						json!(0.0),
						json!(0.0),
						json!(0.0),
						json!(1.0),
					],
				},
				OscMessageWire {
					address: VMC_EXT_BONE_POS.into(),
					args: vec![
						json!("Hips"),
						json!(4.0),
						json!(5.0),
						json!(6.0),
						json!(0.0),
						json!(0.0),
						json!(0.0),
						json!(1.0),
					],
				},
				OscMessageWire {
					address: VMC_EXT_BLEND_VAL.into(),
					args: vec![json!("Joy"), json!(0.75)],
				},
			],
		};
		let mut inputs = InputMap::new();
		inputs.insert("frame".into(), SocketValue::MotionFrame(frame));
		let from = UnmfFromVmcNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &inputs, &ExecFireSet::new())
			.await
			.unwrap();
		let mut filter_inputs = InputMap::new();
		filter_inputs.insert("unmf".into(), from.data.get("unmf").unwrap().clone());
		filter_inputs.insert("bone_names".into(), SocketValue::String("Head".into()));
		let filtered = UnmfFilterNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &filter_inputs, &ExecFireSet::new())
			.await
			.unwrap();
		let mut map_inputs = InputMap::new();
		map_inputs.insert("unmf".into(), filtered.data.get("unmf").unwrap().clone());
		map_inputs.insert("translation_scale".into(), SocketValue::Float(2.0));
		map_inputs.insert("expression_scale".into(), SocketValue::Float(2.0));
		let mapped = UnmfMapNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &map_inputs, &ExecFireSet::new())
			.await
			.unwrap();
		let mut extract_inputs = InputMap::new();
		extract_inputs.insert("unmf".into(), mapped.data.get("unmf").unwrap().clone());
		extract_inputs.insert("bone".into(), SocketValue::String("Head".into()));
		let extracted = UnmfExtractBoneNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &extract_inputs, &ExecFireSet::new())
			.await
			.unwrap();
		assert_eq!(extracted.data.get("px"), Some(&SocketValue::Float(2.0)));
	}

	#[tokio::test]
	async fn resolve_endpoint_reads_unaaz_metadata() {
		let endpoints = json!({
			"endpoints": [
				{
					"id": "main-out",
					"app_id": "un-motion",
					"protocol": "unmfz",
					"direction": "output",
					"base_key_expr": "un/test/frame",
					"topic_mode": "primary_source",
					"schema_major": 1
				}
			]
		});
		let inputs: InputMap = [
			("endpoints".into(), SocketValue::Json(endpoints)),
			("app_id".into(), SocketValue::String("un-motion".into())),
			("endpoint_id".into(), SocketValue::String("main-out".into())),
			("direction".into(), SocketValue::String("output".into())),
		]
		.into_iter()
		.collect();
		let out = UnmfzResolveEndpointNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &inputs, &ExecFireSet::new())
			.await
			.unwrap();
		assert_eq!(out.data.get("found"), Some(&SocketValue::Bool(true)));
		assert_eq!(out.data.get("base_key_expr"), Some(&SocketValue::String("un/test/frame".into())));
		assert_eq!(out.data.get("topic_mode"), Some(&SocketValue::String("primary_source".into())));
	}

	#[tokio::test]
	async fn publish_dry_run_uses_strategy_without_opening_zenoh() {
		let mut frame = unmf::UnMotionFrame::new(9);
		frame.header.stream_id = Some("cam/main".into());
		let inputs: InputMap = [
			("unmf".into(), SocketValue::Unmf(frame)),
			("base_key_expr".into(), SocketValue::String("un/test/frame".into())),
			("topic_mode".into(), SocketValue::String("stream_id".into())),
			("dry_run".into(), SocketValue::Bool(true)),
		]
		.into_iter()
		.collect();
		let mut ctx = ExecCtx::default();
		let out = UnmfzPublishNode::default()
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired_exec())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(
			out.data.get("key_expr"),
			Some(&SocketValue::String("un/test/frame/v1/cam_main".into()))
		);
		assert!(matches!(out.data.get("bytes_sent"), Some(SocketValue::Int(n)) if *n > 0));
	}

	#[tokio::test]
	async fn subscribe_without_exec_is_noop() {
		let mut ctx = ExecCtx::default();
		let out = UnmfzSubscribeNode
			.execute(&mut ctx, &InputMap::new(), &InputMap::new(), &ExecFireSet::new())
			.await
			.unwrap();
		assert!(out.data.is_empty());
		assert!(out.fired_exec.is_empty());
	}
}

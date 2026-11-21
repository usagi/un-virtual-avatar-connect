//! UN App API over Zenoh (UNAA/Z) FlowGraph helper nodes.
//!
//! These first-slice nodes consume UNAA/Z discovery JSON already surfaced by
//! Control API diagnostics. They do not open Zenoh sessions by themselves.

use crate::flowgraph::node::{
	get_optional_bool, get_optional_float, get_optional_int, get_optional_string, get_required_json, EffectfulNode, ExecCtx, ExecFireSet,
	InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PureNode,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};

pub struct UnAppStatusNode;
pub struct UnAppCommandNode;
pub struct UnAppEventNode;
pub struct UnAvatarCameraEffectNode;

const CAMERA_EASINGS: &[&str] = &[
	"none",
	"linear",
	"ease_in_sine",
	"ease_out_sine",
	"ease_in_out_sine",
	"ease_in_quad",
	"ease_out_quad",
	"ease_in_out_quad",
	"ease_in_cubic",
	"ease_out_cubic",
	"ease_in_out_cubic",
	"ease_in_quart",
	"ease_out_quart",
	"ease_in_out_quart",
	"ease_in_quint",
	"ease_out_quint",
	"ease_in_out_quint",
];

impl NodeDescriptor for UnAppStatusNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.un.app.status".into(),
			title: "UN App: Status".into(),
			category: "un_app".into(),
			description: Some("UNAA/Z discovery snapshot JSON から app readiness / capabilities / endpoint count を読む。".into()),
			inputs: vec![
				PortSpec::input("snapshot", "UNAA/Z Snapshot", SocketType::Json),
				PortSpec::input("app_id", "App ID", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("instance_id", "Instance ID", SocketType::String).with_default(SocketValue::String(String::new())),
			],
			outputs: vec![
				PortSpec::output("found", "Found", SocketType::Bool),
				PortSpec::output("ready", "Ready", SocketType::Bool),
				PortSpec::output("running", "Running", SocketType::Bool),
				PortSpec::output("stale", "Stale", SocketType::Bool),
				PortSpec::output("app_id", "App ID", SocketType::String),
				PortSpec::output("instance_id", "Instance ID", SocketType::String),
				PortSpec::output("session_id", "Session ID", SocketType::String),
				PortSpec::output("display_name", "Display Name", SocketType::String),
				PortSpec::output("discovery_source", "Discovery Source", SocketType::String),
				PortSpec::output("capability_count", "Capability Count", SocketType::Int),
				PortSpec::output("command_count", "Command Count", SocketType::Int),
				PortSpec::output("endpoint_count", "Endpoint Count", SocketType::Int),
				PortSpec::output("capabilities", "Capabilities", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("commands", "Commands", SocketType::List(Box::new(SocketType::Json))),
				PortSpec::output("app", "App", SocketType::Json),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl PureNode for UnAppStatusNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		_fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let snapshot = get_required_json(inputs, "snapshot")?;
		let app_id = get_optional_string(inputs, "app_id", "")?;
		let instance_id = get_optional_string(inputs, "instance_id", "")?;
		let Some(app) = find_app(snapshot, &app_id, &instance_id) else {
			return Ok(empty_status(&app_id, &instance_id));
		};
		Ok(status_output(app))
	}
}

impl NodeDescriptor for UnAppCommandNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.un.app.command".into(),
			title: "UN App: Command".into(),
			category: "un_app".into(),
			description: Some("UNAA/Z safe command を app command key へ publish する。dry_run では送信せず envelope だけ作る。".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("snapshot", "UNAA/Z Snapshot", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("app_id", "App ID", SocketType::String),
				PortSpec::input("instance_id", "Instance ID", SocketType::String).with_default(SocketValue::String("main".into())),
				PortSpec::input("command", "Command", SocketType::String)
					.with_default(SocketValue::String("open_gui".into()))
					.with_closed_string_variants(crate::un_app_api::safe_commands().iter().copied()),
				PortSpec::input("args", "Args", SocketType::Json).with_default(SocketValue::Json(JsonValue::Object(Default::default()))),
				PortSpec::input("timeout_ms", "Timeout ms", SocketType::Int).with_default(SocketValue::Int(800)),
				PortSpec::input("wait_response_ms", "Wait Response ms", SocketType::Int).with_default(SocketValue::Int(0)),
				PortSpec::input("dry_run", "Dry Run", SocketType::Bool).with_default(SocketValue::Bool(false)),
			],
			outputs: vec![
				PortSpec::exec_output("on_accepted", "On Accepted"),
				PortSpec::exec_output("on_response", "On Response"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("accepted", "Accepted", SocketType::Bool),
				PortSpec::output("response_observed", "Response Observed", SocketType::Bool),
				PortSpec::output("request_id", "Request ID", SocketType::String),
				PortSpec::output("key_expr", "Key Expr", SocketType::String),
				PortSpec::output("response_key_expr", "Response Key", SocketType::String),
				PortSpec::output("event_key_expr", "Event Key", SocketType::String),
				PortSpec::output("response", "Response", SocketType::Json),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Json))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for UnAppCommandNode {
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
		let snapshot = get_required_json(inputs, "snapshot").unwrap_or(&JsonValue::Null);
		let app_id = get_optional_string(inputs, "app_id", "")?;
		let instance_id = get_optional_string(inputs, "instance_id", "main")?;
		let command = get_optional_string(inputs, "command", "open_gui")?;
		let args = get_required_json(inputs, "args")
			.cloned()
			.unwrap_or_else(|_| JsonValue::Object(Default::default()));
		let timeout_ms = get_optional_int(inputs, "timeout_ms", 800)?.max(1) as u64;
		let wait_response_ms = get_optional_int(inputs, "wait_response_ms", 0)?.max(0) as u64;
		let dry_run = get_optional_bool(inputs, "dry_run", false)?;

		let discovered_app = find_app(snapshot, &app_id, &instance_id);
		if let Some(app) = discovered_app {
			let allowed = allowed_commands_from_app_json(app);
			if !allowed.is_empty() && !allowed.iter().any(|allowed| allowed == &command) {
				return Ok(command_error_output(format!(
					"UNAA/Z command {command:?} is not advertised by {app_id}/{instance_id}; allowed={allowed:?}"
				)));
			}
		}
		if command == "camera.effect" {
			let Some(app) = discovered_app else {
				return Ok(command_error_output(
					"camera.effect requires a discovery snapshot; use UNAvatar: Camera Effect for AvatarLink-safe routing",
				));
			};
			if let Err(error) = validate_camera_effect_json_target(app, &args) {
				return Ok(command_error_output(error));
			}
		}

		let dispatch =
			match crate::un_app_api::build_command_dispatch(app_id.clone(), instance_id.clone(), command.clone(), args, Some(timeout_ms)) {
				Ok(dispatch) => dispatch,
				Err(e) => return Ok(command_error_output(e)),
			};

		if dry_run {
			ctx.log(format!("un.app.command dry-run: {} -> {}", dispatch.command, dispatch.key_expr));
			return Ok(command_ok_output(&dispatch, None, true).fire_exec("on_accepted"));
		}

		match crate::un_app_api::publish_command_with_response_wait(&dispatch, wait_response_ms).await {
			Ok(observation) => {
				ctx.log(format!("un.app.command: {} -> {}", dispatch.command, dispatch.key_expr));
				let response_observed = observation.is_some();
				let out = command_ok_output(&dispatch, observation, false);
				Ok(if response_observed {
					out.fire_exec("on_response")
				} else {
					out.fire_exec("on_accepted")
				})
			}
			Err(e) => {
				ctx.log(format!("un.app.command: {} -> {} error: {e}", dispatch.command, dispatch.key_expr));
				Ok(command_error_output(e))
			}
		}
	}
}

impl NodeDescriptor for UnAvatarCameraEffectNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.unavatar.camera_effect".into(),
			title: "UNAvatar: Camera Effect".into(),
			category: "un_app".into(),
			description: Some(
				"AvatarLinkに対応する単一Rendererへ、進入・保持・開始時状態への復帰を原子的なUNAA/Z camera.effectとして送る。".into(),
			),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("snapshot", "UNAA/Z Snapshot", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("target_link_id", "Target Link ID", SocketType::String),
				PortSpec::input("pose", "Camera Pose", SocketType::Json),
				PortSpec::input("diagonal_fov_deg", "Diagonal FOV (deg)", SocketType::Float).with_default(SocketValue::Float(28.0)),
				PortSpec::input("enter_duration_ms", "Enter Duration ms", SocketType::Int).with_default(SocketValue::Int(450)),
				PortSpec::input("enter_easing", "Enter Easing", SocketType::String)
					.with_default(SocketValue::String("ease_in_out_cubic".into()))
					.with_closed_string_variants(CAMERA_EASINGS.iter().copied()),
				PortSpec::input("hold_ms", "Hold ms", SocketType::Int).with_default(SocketValue::Int(5000)),
				PortSpec::input("leave_duration_ms", "Leave Duration ms", SocketType::Int).with_default(SocketValue::Int(650)),
				PortSpec::input("leave_easing", "Leave Easing", SocketType::String)
					.with_default(SocketValue::String("ease_in_out_cubic".into()))
					.with_closed_string_variants(CAMERA_EASINGS.iter().copied()),
				PortSpec::input("conflict_policy", "Conflict Policy", SocketType::String)
					.with_default(SocketValue::String("replace".into()))
					.with_closed_string_variants(["replace", "queue", "ignore_if_active"]),
				PortSpec::input("timeout_ms", "Timeout ms", SocketType::Int).with_default(SocketValue::Int(800)),
				PortSpec::input("wait_response_ms", "Wait Response ms", SocketType::Int).with_default(SocketValue::Int(800)),
				PortSpec::input("dry_run", "Dry Run", SocketType::Bool).with_default(SocketValue::Bool(false)),
			],
			outputs: vec![
				PortSpec::exec_output("on_accepted", "On Accepted"),
				PortSpec::exec_output("on_response", "On Response"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("accepted", "Accepted", SocketType::Bool),
				PortSpec::output("response_observed", "Response Observed", SocketType::Bool),
				PortSpec::output("app_id", "App ID", SocketType::String),
				PortSpec::output("instance_id", "Renderer Instance ID", SocketType::String),
				PortSpec::output("session_id", "Renderer Session ID", SocketType::String),
				PortSpec::output("request_id", "Request ID", SocketType::String),
				PortSpec::output("key_expr", "Command Key", SocketType::String),
				PortSpec::output("response_key_expr", "Response Key", SocketType::String),
				PortSpec::output("event_key_expr", "Event Key", SocketType::String),
				PortSpec::output("response", "Response", SocketType::Json),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Json))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for UnAvatarCameraEffectNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired_exec.contains("exec_in") { return Ok(NodeOutput::new()); }
		let target = get_optional_string(inputs, "target_link_id", "").unwrap_or_default();
		camera_diagnostic(ctx, &target, "started", JsonValue::Null).await;
		let result = self.execute_camera_effect(ctx, props, inputs, fired_exec).await;
		let (phase, details) = camera_result_diagnostic(&result, get_optional_bool(inputs, "dry_run", false).unwrap_or(false));
		camera_diagnostic(ctx, &target, phase, details).await;
		result
	}
}

impl UnAvatarCameraEffectNode {
	async fn execute_camera_effect(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired_exec.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let target_link_id = get_optional_string(inputs, "target_link_id", "")?;
		let supplied_snapshot = get_required_json(inputs, "snapshot").ok().filter(|value| !value.is_null()).cloned();
		let (links, state) = match ctx.state_handle.as_ref().and_then(std::sync::Weak::upgrade) {
			Some(state) => {
				let links = state.read().await.avatar_links.clone();
				(links, Some(state))
			}
			None => return Ok(camera_effect_error("UNAvatar camera effect requires runtime State")),
		};
		let (target_result, snapshot, attempts) = match supplied_snapshot {
			Some(snapshot) => (resolve_camera_effect_target(&links, &snapshot, &target_link_id), snapshot, 1),
			None => {
				resolve_camera_target_with_retry(&links, &target_link_id, std::time::Duration::from_secs(5), |window| {
					let state = state.as_ref().expect("state exists").clone();
					async move {
					let snapshot = crate::un_app_api::camera_discovery_snapshot_from_state(&state, window).await;
					serde_json::to_value(snapshot).unwrap_or(JsonValue::Null)
				}}).await
			}
		};
		let target = match target_result {
			Ok(target) => target,
			Err(error) => {
				camera_diagnostic(ctx, &target_link_id, "target_rejected", json!({
					"error": error,
					"discovery_attempts": attempts,
					"discovery_errors": snapshot.get("discovery_errors"),
					"candidates": snapshot.get("apps").and_then(JsonValue::as_array).map(|apps| apps.iter().map(|app| json!({
						"app_id":app.get("app_id"),"instance_id":app.get("instance_id"),"session_id":app.get("session_id"),
						"ready":app.get("ready"),"running":app.get("running"),"stale":app.get("stale"),
					})).collect::<Vec<_>>()),
				})).await;
				return Ok(camera_effect_error(error));
			}
		};
		camera_diagnostic(ctx, &target_link_id, "target_resolved", json!({
			"discovery_attempts":attempts, "instance_id":target.instance_id,
			"session_id":target.session_id, "discovery_errors":snapshot.get("discovery_errors"),
		})).await;
		let args = json!({
			"target_session_id": target.session_id,
			"pose": get_required_json(inputs, "pose")?.clone(),
			"diagonal_fov_deg": get_optional_float(inputs, "diagonal_fov_deg", 28.0)?,
			"enter": {
				"duration_ms": get_optional_int(inputs, "enter_duration_ms", 450)?,
				"easing": get_optional_string(inputs, "enter_easing", "ease_in_out_cubic")?,
			},
			"hold_ms": get_optional_int(inputs, "hold_ms", 5000)?,
			"leave": {
				"duration_ms": get_optional_int(inputs, "leave_duration_ms", 650)?,
				"easing": get_optional_string(inputs, "leave_easing", "ease_in_out_cubic")?,
			},
			"conflict_policy": get_optional_string(inputs, "conflict_policy", "replace")?,
			"return_to": "captured_at_start",
		});
		if let Err(error) = crate::un_app_api::validate_camera_effect_args(&args) {
			return Ok(camera_effect_error(error));
		}
		let timeout_ms = get_optional_int(inputs, "timeout_ms", 800)?.max(1) as u64;
		let wait_response_ms = get_optional_int(inputs, "wait_response_ms", 800)?.max(0) as u64;
		let dry_run = get_optional_bool(inputs, "dry_run", false)?;
		let dispatch = match crate::un_app_api::build_command_dispatch(
			target.app_id.clone(),
			target.instance_id.clone(),
			"camera.effect".into(),
			args,
			Some(timeout_ms),
		) {
			Ok(dispatch) => dispatch,
			Err(error) => return Ok(camera_effect_error(error)),
		};
		if dry_run {
			ctx.log(format!(
				"unavatar.camera_effect dry-run: {} -> {}",
				target_link_id, dispatch.key_expr
			));
			return Ok(camera_effect_output(&dispatch, &target.session_id, None, true).fire_exec("on_accepted"));
		}
		match crate::un_app_api::publish_command_with_response_wait(&dispatch, wait_response_ms).await {
			Ok(observation) => {
				ctx.log(format!("unavatar.camera_effect: {} -> {}", target_link_id, dispatch.key_expr));
				if let Err(error) = validate_camera_observation(observation.as_ref(), wait_response_ms) {
					return Ok(camera_effect_output(&dispatch, &target.session_id, observation, false)
						.set_data("accepted", SocketValue::Bool(false))
						.set_data("error", SocketValue::String(error.clone()))
						.set_data("result", SocketValue::Result(FlowResult::err(error).with_code("unaa_z.camera_unconfirmed")))
						.fire_exec("on_error"));
				}
				let observed = observation.is_some();
				let output = camera_effect_output(&dispatch, &target.session_id, observation, false);
				Ok(output.fire_exec(if observed { "on_response" } else { "on_accepted" }))
			}
			Err(error) => {
				ctx.log(format!(
					"unavatar.camera_effect: {} -> {} error: {error}",
					target_link_id, dispatch.key_expr
				));
				Ok(camera_effect_error(error))
			}
		}
	}
}

async fn camera_diagnostic(ctx: &ExecCtx, target: &str, phase: &str, details: JsonValue) {
	if let Some(state) = ctx.state_handle.as_ref().and_then(std::sync::Weak::upgrade) {
		let _ = state.read().await.control_event_tx.send(crate::control_events::ControlEvent::AvatarCameraEffect {
			node_id: ctx.node_id.clone(), target_link_id: target.to_string(), phase: phase.to_string(), details,
		});
	}
}

fn camera_result_diagnostic(result: &Result<NodeOutput, NodeExecError>, dry_run: bool) -> (&'static str, JsonValue) {
	let output = match result {
		Ok(output) => output,
		Err(error) => return ("failed", json!({"error": error.to_string()})),
	};
	let string = |name: &str| output.data.get(name).and_then(|value| value.as_str().ok()).unwrap_or("");
	if !string("error").is_empty() { return ("failed", json!({"error":string("error")})); }
	let response = output.data.get("response").and_then(|value| value.as_json().ok());
	let payload = response.and_then(|value| value.get("payload"));
	let phase = if dry_run { "dry_run" } else if payload.is_some() { "response_observed" } else { "sent_without_response" };
	(phase, json!({
		"request_id":string("request_id"), "app_id":string("app_id"),
		"instance_id":string("instance_id"), "session_id":string("session_id"),
		"response_ok":payload.and_then(|v|v.get("ok")).and_then(JsonValue::as_bool),
		"response_status":payload.and_then(|v|v.get("status")).and_then(JsonValue::as_str),
		"response_message":payload.and_then(|v|v.get("message")).and_then(JsonValue::as_str),
		"response_error":payload.and_then(|v|v.get("error")).and_then(JsonValue::as_str),
		"response_code":payload.and_then(|v|v.get("code")).cloned(),
	}))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CameraEffectTarget {
	app_id: String,
	instance_id: String,
	session_id: String,
}

// Only discovery is retried; command publication remains outside this function.
async fn resolve_camera_target_with_retry<F, Fut>(
	links: &std::collections::BTreeMap<String, crate::conf::AvatarLinkSpec>,
	link_id: &str,
	budget: std::time::Duration,
	mut discover: F,
) -> (Result<CameraEffectTarget, String>, JsonValue, usize)
where
	F: FnMut(std::time::Duration) -> Fut,
	Fut: std::future::Future<Output = JsonValue>,
{
	let deadline = tokio::time::Instant::now() + budget;
	let mut last = json!({"apps":[]});
	let mut attempts = 0;
	loop {
		// Leave time to return already collected replies before the outer bound.
		let window = deadline.saturating_duration_since(tokio::time::Instant::now()).saturating_sub(std::time::Duration::from_millis(25));
		if window.is_zero() {
			let reason = resolve_camera_effect_target(links, &last, link_id).err().unwrap_or_else(|| "target could not be refreshed".into());
			return (Err(format!("Renderer discovery deadline exceeded: {reason}")), last, attempts);
		}
		let snapshot = match tokio::time::timeout_at(deadline, discover(window)).await {
			Ok(snapshot) => snapshot,
			Err(_) => {
				let reason = resolve_camera_effect_target(links, &last, link_id).err().unwrap_or_else(|| "target could not be refreshed".into());
				return (Err(format!("Renderer discovery deadline exceeded: {reason}")), last, attempts);
			}
		};
		attempts += 1;
		let result = resolve_camera_effect_target(links, &snapshot, link_id);
		let configured = links.get(link_id.trim()).is_some_and(|link| link.has_capability("camera_control") && link.unaa_z.is_some());
		if result.is_ok() || !configured || tokio::time::Instant::now() >= deadline {
			return (result, snapshot, attempts);
		}
		last = snapshot;
		tokio::time::sleep_until((tokio::time::Instant::now() + std::time::Duration::from_millis(100)).min(deadline)).await;
	}
}

fn resolve_camera_effect_target(
	links: &std::collections::BTreeMap<String, crate::conf::AvatarLinkSpec>,
	snapshot: &JsonValue,
	target_link_id: &str,
) -> Result<CameraEffectTarget, String> {
	let link_id = target_link_id.trim();
	let link = links.get(link_id).ok_or_else(|| format!("AvatarLink {link_id:?} was not found"))?;
	if !link.has_capability("camera_control") {
		return Err(format!("AvatarLink {link_id:?} does not allow camera_control"));
	}
	let identity = link
		.unaa_z
		.as_ref()
		.ok_or_else(|| format!("AvatarLink {link_id:?} has no UNAA/Z identity"))?;
	let apps = snapshot
		.get("apps")
		.or_else(|| snapshot.get("unaa_z").and_then(|value| value.get("apps")))
		.and_then(JsonValue::as_array)
		.ok_or_else(|| "UNAA/Z snapshot has no apps array".to_string())?;
	let matches: Vec<_> = apps
		.iter()
		.filter(|app| {
			app.get("app_id").and_then(JsonValue::as_str) == Some(identity.app_id.as_str())
				&& app.get("instance_id").and_then(JsonValue::as_str) == Some(identity.instance_id.as_str())
		})
		.collect();
	if matches.len() != 1 {
		return Err(format!(
			"AvatarLink {link_id:?} resolved to {} Renderer instances; expected exactly one",
			matches.len()
		));
	}
	let app = matches[0];
	if !app.get("running").and_then(JsonValue::as_bool).unwrap_or(false)
		|| !app.get("ready").and_then(JsonValue::as_bool).unwrap_or(false)
		|| app.get("stale").and_then(JsonValue::as_bool).unwrap_or(true)
	{
		return Err(format!("AvatarLink {link_id:?} Renderer is not fresh, running, and ready"));
	}
	let has_token = |field: &str, token: &str| {
		app.get(field)
			.and_then(JsonValue::as_array)
			.is_some_and(|values| values.iter().any(|value| value.as_str() == Some(token)))
	};
	if !has_token("capabilities", "camera_control") || !has_token("commands", "camera.effect") {
		return Err(format!(
			"AvatarLink {link_id:?} Renderer does not advertise camera_control and camera.effect"
		));
	}
	let session_id = app
		.get("session_id")
		.or_else(|| app.get("sessionId"))
		.and_then(JsonValue::as_str)
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| format!("AvatarLink {link_id:?} Renderer has no session_id"))?;
	Ok(CameraEffectTarget {
		app_id: identity.app_id.clone(),
		instance_id: identity.instance_id.clone(),
		session_id: session_id.to_string(),
	})
}

fn camera_effect_output(
	dispatch: &crate::un_app_api::UnAppApiCommandDispatch,
	session_id: &str,
	observation: Option<crate::un_app_api::UnAppApiCommandObservation>,
	dry_run: bool,
) -> NodeOutput {
	let mut output = command_ok_output(dispatch, observation, dry_run);
	output.data.insert("app_id".into(), SocketValue::String(dispatch.app_id.clone()));
	output
		.data
		.insert("instance_id".into(), SocketValue::String(dispatch.instance_id.clone()));
	output.data.insert("session_id".into(), SocketValue::String(session_id.to_string()));
	output
}

fn validate_camera_observation(
	observation: Option<&crate::un_app_api::UnAppApiCommandObservation>,
	wait_response_ms: u64,
) -> Result<(), String> {
	match observation {
		None if wait_response_ms > 0 => Err("camera.effect response timed out; execution is unconfirmed (not retried)".into()),
		Some(observation) if observation.payload.get("ok").and_then(JsonValue::as_bool) != Some(true) => {
			Err(format!("camera.effect was not accepted by Renderer: {}", observation.payload))
		}
		_ => Ok(()), // explicit non-waiting mode keeps its existing send-only contract
	}
}

#[test]
fn camera_acknowledgement_distinguishes_timeout_rejection_and_acceptance() {
	assert!(validate_camera_observation(None, 800).unwrap_err().contains("unconfirmed"));
	assert!(validate_camera_observation(None, 0).is_ok());
	let mut response = crate::un_app_api::UnAppApiCommandObservation {
		key_expr: "test/response".into(), source: "response".into(),
		payload: json!({"ok":false,"error":"target_session_mismatch"}),
	};
	assert!(validate_camera_observation(Some(&response), 800).unwrap_err().contains("target_session_mismatch"));
	response.payload = json!({"ok":true,"message":"accepted"});
	assert!(validate_camera_observation(Some(&response), 800).is_ok());
}

fn camera_effect_error(error: impl Into<String>) -> NodeOutput {
	let mut output = command_error_output(error);
	output.data.insert("app_id".into(), SocketValue::String(String::new()));
	output.data.insert("instance_id".into(), SocketValue::String(String::new()));
	output.data.insert("session_id".into(), SocketValue::String(String::new()));
	output
}

fn validate_camera_effect_json_target(app: &JsonValue, args: &JsonValue) -> Result<(), String> {
	crate::un_app_api::validate_camera_effect_args(args)?;
	if !app.get("running").and_then(JsonValue::as_bool).unwrap_or(false)
		|| !app.get("ready").and_then(JsonValue::as_bool).unwrap_or(false)
		|| app.get("stale").and_then(JsonValue::as_bool).unwrap_or(true)
	{
		return Err("camera.effect target must be fresh, running, and ready".into());
	}
	let has_token = |field: &str, token: &str| {
		app.get(field)
			.and_then(JsonValue::as_array)
			.is_some_and(|values| values.iter().any(|value| value.as_str() == Some(token)))
	};
	if !has_token("capabilities", "camera_control") || !has_token("commands", "camera.effect") {
		return Err("camera.effect target must explicitly advertise camera_control and camera.effect".into());
	}
	let discovered_session = app
		.get("session_id")
		.or_else(|| app.get("sessionId"))
		.and_then(JsonValue::as_str)
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| "camera.effect target discovery has no session_id".to_string())?;
	let requested_session = args
		.get("target_session_id")
		.and_then(JsonValue::as_str)
		.ok_or_else(|| "camera.effect target_session_id is missing".to_string())?;
	if requested_session != discovered_session {
		return Err("camera.effect target_session_id does not match the current Renderer session".into());
	}
	Ok(())
}

impl NodeDescriptor for UnAppEventNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.un.app.event".into(),
			title: "UN App: Event".into(),
			category: "un_app".into(),
			description: Some("UNAA/Z command response / event key を bounded wait で読む polling node。".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("app_id", "App ID", SocketType::String),
				PortSpec::input("instance_id", "Instance ID", SocketType::String).with_default(SocketValue::String("main".into())),
				PortSpec::input("request_id", "Request ID", SocketType::String),
				PortSpec::input("wait_response_ms", "Wait Response ms", SocketType::Int).with_default(SocketValue::Int(800)),
				PortSpec::input("dry_run", "Dry Run", SocketType::Bool).with_default(SocketValue::Bool(false)),
			],
			outputs: vec![
				PortSpec::exec_output("on_event", "On Event"),
				PortSpec::exec_output("on_timeout", "On Timeout"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("observed", "Observed", SocketType::Bool),
				PortSpec::output("source", "Source", SocketType::String),
				PortSpec::output("key_expr", "Key Expr", SocketType::String),
				PortSpec::output("response_key_expr", "Response Key", SocketType::String),
				PortSpec::output("event_key_expr", "Event Key", SocketType::String),
				PortSpec::output("payload", "Payload", SocketType::Json),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Json))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for UnAppEventNode {
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
		let app_id = get_optional_string(inputs, "app_id", "")?;
		let instance_id = get_optional_string(inputs, "instance_id", "main")?;
		let request_id = get_optional_string(inputs, "request_id", "")?;
		let wait_response_ms = get_optional_int(inputs, "wait_response_ms", 800)?.max(1) as u64;
		let dry_run = get_optional_bool(inputs, "dry_run", false)?;
		let keys = command_event_keys(&app_id, &instance_id, &request_id);
		if dry_run {
			ctx.log(format!(
				"un.app.event dry-run: response={} event={}",
				keys.response_key_expr, keys.event_key_expr
			));
			return Ok(event_timeout_output(keys, true).fire_exec("on_timeout"));
		}
		match crate::un_app_api::wait_for_command_observation(app_id, instance_id, request_id, wait_response_ms).await {
			Ok(Some(observation)) => {
				ctx.log(format!("un.app.event: {} from {}", observation.source, observation.key_expr));
				Ok(event_observed_output(keys, observation).fire_exec("on_event"))
			}
			Ok(None) => {
				ctx.log(format!(
					"un.app.event timeout: response={} event={}",
					keys.response_key_expr, keys.event_key_expr
				));
				Ok(event_timeout_output(keys, false).fire_exec("on_timeout"))
			}
			Err(e) => {
				ctx.log(format!("un.app.event error: {e}"));
				Ok(event_error_output(e))
			}
		}
	}
}

fn find_app<'a>(snapshot: &'a JsonValue, app_id: &str, instance_id: &str) -> Option<&'a JsonValue> {
	let apps = snapshot
		.get("apps")
		.or_else(|| snapshot.get("unaa_z").and_then(|unaa_z| unaa_z.get("apps")))?
		.as_array()?;
	apps.iter().find(|app| {
		let app_matches = app_id.is_empty() || app.get("app_id").and_then(JsonValue::as_str) == Some(app_id);
		let instance_matches = instance_id.is_empty() || app.get("instance_id").and_then(JsonValue::as_str) == Some(instance_id);
		app_matches && instance_matches
	})
}

fn status_output(app: &JsonValue) -> NodeOutput {
	let app_id = str_field(app, "app_id");
	let instance_id = str_field(app, "instance_id");
	let session_id = str_field(app, "session_id");
	let display_name = str_field(app, "display_name");
	let discovery_source = str_field(app, "discovery_source");
	let capabilities = string_array_as_json_list(app.get("capabilities"));
	let commands = string_array_as_json_list(app.get("commands"));
	let endpoint_count = app
		.get("endpoints")
		.and_then(JsonValue::as_array)
		.map(|items| items.len())
		.unwrap_or(0);
	NodeOutput::new()
		.set_data("found", SocketValue::Bool(true))
		.set_data(
			"ready",
			SocketValue::Bool(app.get("ready").and_then(JsonValue::as_bool).unwrap_or(false)),
		)
		.set_data(
			"running",
			SocketValue::Bool(app.get("running").and_then(JsonValue::as_bool).unwrap_or(false)),
		)
		.set_data(
			"stale",
			SocketValue::Bool(app.get("stale").and_then(JsonValue::as_bool).unwrap_or(false)),
		)
		.set_data("app_id", SocketValue::String(app_id))
		.set_data("instance_id", SocketValue::String(instance_id))
		.set_data("session_id", SocketValue::String(session_id))
		.set_data("display_name", SocketValue::String(display_name))
		.set_data("discovery_source", SocketValue::String(discovery_source))
		.set_data("capability_count", SocketValue::Int(capabilities.len() as i64))
		.set_data("command_count", SocketValue::Int(commands.len() as i64))
		.set_data("endpoint_count", SocketValue::Int(endpoint_count as i64))
		.set_data("capabilities", SocketValue::List(capabilities))
		.set_data("commands", SocketValue::List(commands))
		.set_data("app", SocketValue::Json(app.clone()))
}

fn empty_status(app_id: &str, instance_id: &str) -> NodeOutput {
	NodeOutput::new()
		.set_data("found", SocketValue::Bool(false))
		.set_data("ready", SocketValue::Bool(false))
		.set_data("running", SocketValue::Bool(false))
		.set_data("stale", SocketValue::Bool(false))
		.set_data("app_id", SocketValue::String(app_id.to_string()))
		.set_data("instance_id", SocketValue::String(instance_id.to_string()))
		.set_data("session_id", SocketValue::String(String::new()))
		.set_data("display_name", SocketValue::String(String::new()))
		.set_data("discovery_source", SocketValue::String(String::new()))
		.set_data("capability_count", SocketValue::Int(0))
		.set_data("command_count", SocketValue::Int(0))
		.set_data("endpoint_count", SocketValue::Int(0))
		.set_data("capabilities", SocketValue::List(Vec::new()))
		.set_data("commands", SocketValue::List(Vec::new()))
		.set_data("app", SocketValue::Json(JsonValue::Null))
}

fn command_ok_output(
	dispatch: &crate::un_app_api::UnAppApiCommandDispatch,
	observation: Option<crate::un_app_api::UnAppApiCommandObservation>,
	dry_run: bool,
) -> NodeOutput {
	let response_json = observation
		.as_ref()
		.map(|observation| {
			json!({
				"key_expr": observation.key_expr,
				"source": observation.source,
				"payload": observation.payload,
			})
		})
		.unwrap_or(JsonValue::Null);
	let value = json!({
		"dry_run": dry_run,
		"app_id": dispatch.app_id,
		"instance_id": dispatch.instance_id,
		"command": dispatch.command,
		"request_id": dispatch.request_id,
		"key_expr": dispatch.key_expr,
		"response_key_expr": dispatch.response_key_expr,
		"event_key_expr": dispatch.event_key_expr,
		"response_observed": observation.is_some(),
		"response": response_json,
	});
	NodeOutput::new()
		.set_data("accepted", SocketValue::Bool(true))
		.set_data("response_observed", SocketValue::Bool(observation.is_some()))
		.set_data("request_id", SocketValue::String(dispatch.request_id.clone()))
		.set_data("key_expr", SocketValue::String(dispatch.key_expr.clone()))
		.set_data("response_key_expr", SocketValue::String(dispatch.response_key_expr.clone()))
		.set_data("event_key_expr", SocketValue::String(dispatch.event_key_expr.clone()))
		.set_data("response", SocketValue::Json(response_json))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Json(value))))
}

fn command_error_output(error: impl Into<String>) -> NodeOutput {
	let error = error.into();
	NodeOutput::new()
		.set_data("accepted", SocketValue::Bool(false))
		.set_data("response_observed", SocketValue::Bool(false))
		.set_data("request_id", SocketValue::String(String::new()))
		.set_data("key_expr", SocketValue::String(String::new()))
		.set_data("response_key_expr", SocketValue::String(String::new()))
		.set_data("event_key_expr", SocketValue::String(String::new()))
		.set_data("response", SocketValue::Json(JsonValue::Null))
		.set_data("error", SocketValue::String(error.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(error).with_code("unaa_z.command")))
		.fire_exec("on_error")
}

#[derive(Debug, Clone)]
struct CommandEventKeys {
	response_key_expr: String,
	event_key_expr: String,
}

fn command_event_keys(app_id: &str, instance_id: &str, request_id: &str) -> CommandEventKeys {
	CommandEventKeys {
		response_key_expr: format!("un/app/{app_id}/{instance_id}/response/{request_id}"),
		event_key_expr: format!("un/app/{app_id}/{instance_id}/event"),
	}
}

fn event_observed_output(keys: CommandEventKeys, observation: crate::un_app_api::UnAppApiCommandObservation) -> NodeOutput {
	let value = json!({
		"observed": true,
		"source": observation.source,
		"key_expr": observation.key_expr,
		"payload": observation.payload,
	});
	NodeOutput::new()
		.set_data("observed", SocketValue::Bool(true))
		.set_data("source", SocketValue::String(observation.source))
		.set_data("key_expr", SocketValue::String(observation.key_expr))
		.set_data("response_key_expr", SocketValue::String(keys.response_key_expr))
		.set_data("event_key_expr", SocketValue::String(keys.event_key_expr))
		.set_data("payload", SocketValue::Json(observation.payload))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Json(value))))
}

fn event_timeout_output(keys: CommandEventKeys, dry_run: bool) -> NodeOutput {
	let value = json!({
		"observed": false,
		"dry_run": dry_run,
		"response_key_expr": keys.response_key_expr,
		"event_key_expr": keys.event_key_expr,
	});
	NodeOutput::new()
		.set_data("observed", SocketValue::Bool(false))
		.set_data("source", SocketValue::String(String::new()))
		.set_data("key_expr", SocketValue::String(String::new()))
		.set_data("response_key_expr", SocketValue::String(keys.response_key_expr))
		.set_data("event_key_expr", SocketValue::String(keys.event_key_expr))
		.set_data("payload", SocketValue::Json(JsonValue::Null))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Json(value))))
}

fn event_error_output(error: impl Into<String>) -> NodeOutput {
	let error = error.into();
	NodeOutput::new()
		.set_data("observed", SocketValue::Bool(false))
		.set_data("source", SocketValue::String(String::new()))
		.set_data("key_expr", SocketValue::String(String::new()))
		.set_data("response_key_expr", SocketValue::String(String::new()))
		.set_data("event_key_expr", SocketValue::String(String::new()))
		.set_data("payload", SocketValue::Json(JsonValue::Null))
		.set_data("error", SocketValue::String(error.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(error).with_code("unaa_z.event")))
		.fire_exec("on_error")
}

fn str_field(app: &JsonValue, key: &str) -> String {
	app.get(key).and_then(JsonValue::as_str).unwrap_or_default().to_string()
}

fn string_array_as_json_list(value: Option<&JsonValue>) -> Vec<SocketValue> {
	value
		.and_then(JsonValue::as_array)
		.into_iter()
		.flatten()
		.filter_map(JsonValue::as_str)
		.map(|item| SocketValue::Json(json!(item)))
		.collect()
}

fn allowed_commands_from_app_json(app: &JsonValue) -> Vec<String> {
	let commands = string_array(app.get("commands"));
	if !commands.is_empty() {
		return crate::un_app_api::safe_commands()
			.iter()
			.filter(|command| commands.iter().any(|advertised| advertised == **command))
			.map(|command| (*command).to_string())
			.collect();
	}
	let capabilities = string_array(app.get("capabilities"));
	crate::un_app_api::safe_commands()
		.iter()
		.filter(|command| capabilities_allow_command(&capabilities, command))
		.map(|command| (*command).to_string())
		.collect()
}

fn string_array(value: Option<&JsonValue>) -> Vec<String> {
	value
		.and_then(JsonValue::as_array)
		.into_iter()
		.flatten()
		.filter_map(JsonValue::as_str)
		.map(ToOwned::to_owned)
		.collect()
}

fn capabilities_allow_command(capabilities: &[String], command: &str) -> bool {
	capabilities.iter().any(|capability| {
		let capability = capability.trim().to_ascii_lowercase();
		match command {
			"focus" | "open_gui" => matches!(
				capability.as_str(),
				"supervisor_control" | "app_control" | "window_control" | "gui" | "renderer"
			),
			"reload.preview" | "profile.switch.preview" => {
				matches!(capability.as_str(), "supervisor_control" | "app_control" | "profile_control")
			}
			"camera.effect" => capability == "camera_control",
			_ => false,
		}
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::conf::{AvatarLinkSpec, AvatarLinkUnaaZSpec};
	use crate::flowgraph::node::{ExecCtx, PureEvalHost};

	#[test]
	fn camera_effect_diagnostics_distinguish_rejection_no_response_and_dry_run() {
		let dispatch = crate::un_app_api::build_command_dispatch("un-avatar".into(), "usagi".into(), "camera.effect".into(),
			json!({"target_session_id":"session","diagonal_fov_deg":42}), Some(800)).unwrap();
		let no_response = Ok(camera_effect_output(&dispatch, "session", None, false));
		assert_eq!(camera_result_diagnostic(&no_response, false).0, "sent_without_response");
		assert_eq!(camera_result_diagnostic(&no_response, true).0, "dry_run");
		let rejected = Ok(camera_effect_output(&dispatch, "session", Some(crate::un_app_api::UnAppApiCommandObservation {
			key_expr:dispatch.response_key_expr.clone(), source:"response".into(),
			payload:json!({"ok":false,"message":"rejected","error":"session_mismatch","unexpected_secret":"omit"}),
		}), false));
		let (phase, details) = camera_result_diagnostic(&rejected, false);
		assert_eq!(phase, "response_observed");
		assert_eq!(details["response_ok"], false);
		assert_eq!(details["response_error"], "session_mismatch");
		assert_eq!(details["request_id"], dispatch.request_id);
		assert!(details.get("unexpected_secret").is_none());
		assert!(details.get("completed").is_none());
		let (phase, details) = camera_result_diagnostic(&Ok(camera_effect_error("target unavailable")), false);
		assert_eq!(phase, "failed");
		assert_eq!(details["error"], "target unavailable");
	}

	fn camera_links() -> std::collections::BTreeMap<String, AvatarLinkSpec> {
		std::collections::BTreeMap::from([
			(
				"dr_usagi".into(),
				AvatarLinkSpec {
					capabilities: vec!["reference".into(), "camera_control".into()],
					unaa_z: Some(AvatarLinkUnaaZSpec {
						app_id: "un-avatar".into(),
						instance_id: "renderer-dr-usagi".into(),
					}),
					..Default::default()
				},
			),
			(
				"kaltsit_ai".into(),
				AvatarLinkSpec {
					capabilities: vec!["reference".into(), "camera_control".into()],
					unaa_z: Some(AvatarLinkUnaaZSpec {
						app_id: "un-avatar".into(),
						instance_id: "renderer-kaltsit".into(),
					}),
					..Default::default()
				},
			),
		])
	}

	#[tokio::test]
	async fn camera_effect_discovery_waits_for_exact_ready_target() {
		let mut calls = 0;
		let (target, _, attempts) = resolve_camera_target_with_retry(&camera_links(), "dr_usagi", std::time::Duration::from_secs(1), |_| {
			calls += 1;
			let apps = if calls == 1 { json!([]) } else { json!([{
				"app_id":"un-avatar","instance_id":"renderer-dr-usagi","session_id":"fresh",
				"running":true,"ready":true,"stale":false,"capabilities":["camera_control"],"commands":["camera.effect"]
			}]) };
			async move { json!({"apps":apps}) }
		}).await;
		assert_eq!(target.unwrap().session_id, "fresh");
		assert_eq!(attempts, 2);
		assert_eq!(calls, 2);
	}

	#[tokio::test]
	async fn camera_effect_discovery_retains_late_reply_with_remaining_budget() {
		let mut windows = Vec::new();
		let (target, _, attempts) = resolve_camera_target_with_retry(&camera_links(), "dr_usagi", std::time::Duration::from_millis(400), |window| {
			windows.push(window);
			let first = windows.len() == 1;
			async move {
				if first { return json!({"apps":[]}); }
				tokio::time::sleep(window.saturating_sub(std::time::Duration::from_millis(5))).await;
				json!({"apps":[{"app_id":"un-avatar","instance_id":"renderer-dr-usagi","session_id":"late",
					"running":true,"ready":true,"stale":false,"capabilities":["camera_control"],"commands":["camera.effect"]}]})
			}
		}).await;
		assert_eq!(target.unwrap().session_id, "late");
		assert_eq!(attempts, 2);
		assert!(windows[1] < windows[0]);
	}

	#[tokio::test]
	async fn camera_effect_discovery_has_deadline_and_never_substitutes_other_avatar() {
		let (target, _, attempts) = resolve_camera_target_with_retry(&camera_links(), "dr_usagi", std::time::Duration::from_millis(100), |_| async {
			json!({"apps":[{"app_id":"un-avatar","instance_id":"renderer-kaltsit","session_id":"other",
			"running":true,"ready":true,"stale":false,"capabilities":["camera_control"],"commands":["camera.effect"]}]})
		}).await;
		assert!(target.is_err());
		assert!(attempts > 0);
	}

	#[test]
	fn camera_effect_resolves_only_the_explicit_avatar_link_renderer() {
		let snapshot = json!({
			"apps": [
				{
					"app_id": "un-avatar", "instance_id": "renderer-dr-usagi", "session_id": "dr-session",
					"running": true, "ready": true, "stale": false,
					"capabilities": ["camera_control"], "commands": ["camera.effect"]
				},
				{
					"app_id": "un-avatar", "instance_id": "renderer-kaltsit", "session_id": "kaltsit-session",
					"running": true, "ready": true, "stale": false,
					"capabilities": ["camera_control"], "commands": ["camera.effect"]
				}
			]
		});
		let target = resolve_camera_effect_target(&camera_links(), &snapshot, "dr_usagi").unwrap();
		assert_eq!(
			target,
			CameraEffectTarget {
				app_id: "un-avatar".into(),
				instance_id: "renderer-dr-usagi".into(),
				session_id: "dr-session".into(),
			}
		);
	}

	#[test]
	fn camera_effect_fails_closed_for_stale_or_sessionless_renderer() {
		let stale = json!({"apps": [{
			"app_id": "un-avatar", "instance_id": "renderer-dr-usagi", "session_id": "old-session",
			"running": true, "ready": true, "stale": true,
			"capabilities": ["camera_control"], "commands": ["camera.effect"]
		}]});
		assert!(resolve_camera_effect_target(&camera_links(), &stale, "dr_usagi").is_err());

		let no_session = json!({"apps": [{
			"app_id": "un-avatar", "instance_id": "renderer-dr-usagi",
			"running": true, "ready": true, "stale": false,
			"capabilities": ["camera_control"], "commands": ["camera.effect"]
		}]});
		assert!(resolve_camera_effect_target(&camera_links(), &no_session, "dr_usagi").is_err());
	}

	#[test]
	fn generic_camera_command_requires_matching_current_session() {
		let app = json!({
			"app_id": "un-avatar", "instance_id": "renderer-dr-usagi", "session_id": "current-session",
			"running": true, "ready": true, "stale": false,
			"capabilities": ["camera_control"], "commands": ["camera.effect"]
		});
		let args = json!({
			"target_session_id": "current-session",
			"pose": {"kind": "orbit", "target_xyz": [0.0, 1.35, 0.0], "longitude_deg": 180.0, "latitude_deg": 3.0, "radius": 0.85},
			"diagonal_fov_deg": 28.0,
			"enter": {"duration_ms": 450, "easing": "ease_in_out_cubic"},
			"hold_ms": 5000,
			"leave": {"duration_ms": 650, "easing": "ease_in_out_cubic"},
			"conflict_policy": "replace",
			"return_to": "captured_at_start"
		});
		validate_camera_effect_json_target(&app, &args).unwrap();

		let mut old_session = args.clone();
		old_session["target_session_id"] = json!("old-session");
		assert!(validate_camera_effect_json_target(&app, &old_session).is_err());
	}

	#[tokio::test]
	async fn un_app_status_reads_matching_app_from_snapshot() {
		let mut inputs = InputMap::new();
		inputs.insert(
			"snapshot".into(),
			SocketValue::Json(json!({
				"schema_version": 1,
				"apps": [{
					"app_id": "un-avatar",
					"instance_id": "renderer-main",
					"display_name": "UN Avatar Renderer",
					"discovery_source": "zenoh",
					"ready": true,
					"running": true,
					"stale": false,
					"capabilities": ["renderer", "motion_input"],
					"commands": ["focus", "open_gui"],
					"endpoints": [{ "endpoint_id": "motion-in" }]
				}]
			})),
		);
		inputs.insert("app_id".into(), SocketValue::String("un-avatar".into()));
		inputs.insert("instance_id".into(), SocketValue::String("renderer-main".into()));

		let out = UnAppStatusNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &inputs, &ExecFireSet::new())
			.await
			.unwrap();

		assert_eq!(out.data.get("found"), Some(&SocketValue::Bool(true)));
		assert_eq!(out.data.get("ready"), Some(&SocketValue::Bool(true)));
		assert_eq!(
			out.data.get("display_name"),
			Some(&SocketValue::String("UN Avatar Renderer".into()))
		);
		assert_eq!(out.data.get("capability_count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("command_count"), Some(&SocketValue::Int(2)));
		assert_eq!(out.data.get("endpoint_count"), Some(&SocketValue::Int(1)));
	}

	#[tokio::test]
	async fn un_app_status_accepts_full_diagnostics_shape_and_missing_app() {
		let mut inputs = InputMap::new();
		inputs.insert(
			"snapshot".into(),
			SocketValue::Json(json!({
				"unaa_z": {
					"apps": [{
						"app_id": "un-motion",
						"instance_id": "main",
						"ready": false
					}]
				}
			})),
		);
		inputs.insert("app_id".into(), SocketValue::String("missing".into()));

		let out = UnAppStatusNode
			.compute(&PureEvalHost::default(), &InputMap::new(), &inputs, &ExecFireSet::new())
			.await
			.unwrap();

		assert_eq!(out.data.get("found"), Some(&SocketValue::Bool(false)));
		assert_eq!(out.data.get("app_id"), Some(&SocketValue::String("missing".into())));
		assert_eq!(out.data.get("app"), Some(&SocketValue::Json(JsonValue::Null)));
	}

	#[tokio::test]
	async fn un_app_command_dry_run_builds_safe_dispatch() {
		let mut inputs = InputMap::new();
		inputs.insert(
			"snapshot".into(),
			SocketValue::Json(json!({
				"apps": [{
					"app_id": "un-avatar",
					"instance_id": "renderer-main",
					"commands": ["open_gui"]
				}]
			})),
		);
		inputs.insert("app_id".into(), SocketValue::String("un-avatar".into()));
		inputs.insert("instance_id".into(), SocketValue::String("renderer-main".into()));
		inputs.insert("command".into(), SocketValue::String("open_gui".into()));
		inputs.insert("args".into(), SocketValue::Json(json!({})));
		inputs.insert("dry_run".into(), SocketValue::Bool(true));
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let mut ctx = ExecCtx {
			node_id: "node".into(),
			..Default::default()
		};

		let out = UnAppCommandNode.execute(&mut ctx, &InputMap::new(), &inputs, &fired).await.unwrap();

		assert_eq!(out.data.get("accepted"), Some(&SocketValue::Bool(true)));
		assert_eq!(
			out.data.get("key_expr"),
			Some(&SocketValue::String("un/app/un-avatar/renderer-main/command".into()))
		);
		assert!(out.fired_exec.contains("on_accepted"));
	}

	#[tokio::test]
	async fn un_app_command_rejects_unadvertised_command() {
		let mut inputs = InputMap::new();
		inputs.insert(
			"snapshot".into(),
			SocketValue::Json(json!({
				"apps": [{
					"app_id": "un-avatar",
					"instance_id": "renderer-main",
					"commands": ["focus"]
				}]
			})),
		);
		inputs.insert("app_id".into(), SocketValue::String("un-avatar".into()));
		inputs.insert("instance_id".into(), SocketValue::String("renderer-main".into()));
		inputs.insert("command".into(), SocketValue::String("open_gui".into()));
		inputs.insert("args".into(), SocketValue::Json(json!({})));
		inputs.insert("dry_run".into(), SocketValue::Bool(true));
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let mut ctx = ExecCtx {
			node_id: "node".into(),
			..Default::default()
		};

		let out = UnAppCommandNode.execute(&mut ctx, &InputMap::new(), &inputs, &fired).await.unwrap();

		assert_eq!(out.data.get("accepted"), Some(&SocketValue::Bool(false)));
		assert!(out.fired_exec.contains("on_error"));
	}

	#[tokio::test]
	async fn un_app_event_dry_run_reports_response_and_event_keys() {
		let mut inputs = InputMap::new();
		inputs.insert("app_id".into(), SocketValue::String("un-avatar".into()));
		inputs.insert("instance_id".into(), SocketValue::String("renderer-main".into()));
		inputs.insert("request_id".into(), SocketValue::String("unvac-123".into()));
		inputs.insert("dry_run".into(), SocketValue::Bool(true));
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let mut ctx = ExecCtx {
			node_id: "node".into(),
			..Default::default()
		};

		let out = UnAppEventNode.execute(&mut ctx, &InputMap::new(), &inputs, &fired).await.unwrap();

		assert_eq!(out.data.get("observed"), Some(&SocketValue::Bool(false)));
		assert_eq!(
			out.data.get("response_key_expr"),
			Some(&SocketValue::String("un/app/un-avatar/renderer-main/response/unvac-123".into()))
		);
		assert_eq!(
			out.data.get("event_key_expr"),
			Some(&SocketValue::String("un/app/un-avatar/renderer-main/event".into()))
		);
		assert!(out.fired_exec.contains("on_timeout"));
	}
}

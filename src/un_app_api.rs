//! UN App API over Zenoh (UNAA/Z) discovery surface.
//!
//! This module owns the Control API snapshot shape and a bounded Zenoh discovery
//! probe. The reader intentionally treats unknown payloads as JSON metadata and
//! never blocks diagnostics for long.

use crate::SharedState;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::runtime::Builder as TokioRuntimeBuilder;
use tokio::sync::OnceCell;

const DEFAULT_DISCOVERY_TIMEOUT_MS: u64 = 180;
const DISCOVERY_REPLY_DRAIN_MS: u64 = 80;
const DEFAULT_ANNOUNCEMENT_KEYS: &[&str] = &["un/app/**/announce", "unaa_z/**/announce"];
const DEFAULT_QUERY_KEYS: &[&str] = &["un/app/**/snapshot", "unaa_z/**/snapshot"];
const DEFAULT_DISCOVERY_STALE_MS: i128 = 10_000;
const SAFE_COMMANDS: &[&str] = &["focus", "open_gui", "reload.preview", "profile.switch.preview", "camera.effect"];

static COMMAND_SEQ: AtomicU64 = AtomicU64::new(1);
static DISCOVERY_SESSION: OnceCell<zenoh::Session> = OnceCell::const_new();

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnAppApiDiscoverySnapshot {
	pub schema_version: u32,
	pub source: String,
	pub generated_at: Timestamp,
	pub apps: Vec<UnAppApiDiscoveredApp>,
	pub endpoints: Vec<JsonValue>,
	pub discovery_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnAppApiDiscoveredApp {
	pub app_id: String,
	pub instance_id: String,
	/// Renderer起動ごとに変わるidentity。再起動前の遅延commandを拒否するために使う。
	pub session_id: Option<String>,
	pub display_name: String,
	pub managed_app_id: Option<String>,
	pub discovery_source: String,
	pub discovery_source_key: Option<String>,
	pub discovery_payload_kind: String,
	pub observed_at: Timestamp,
	pub reported_at: Option<Timestamp>,
	pub age_ms: Option<i128>,
	pub stale: bool,
	pub running: bool,
	pub ready: bool,
	pub capabilities: Vec<String>,
	pub commands: Vec<String>,
	pub endpoints: Vec<JsonValue>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UnAppApiEndpointCandidateDiagnostic {
	pub severity: String,
	pub code: String,
	pub message: String,
	pub field: String,
	pub candidate_id: Option<String>,
	pub app_id: Option<String>,
	pub endpoint_id: Option<String>,
	pub source_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UnAppApiDiscoveryHealthSummary {
	pub source: String,
	pub app_count: usize,
	pub managed_registry_app_count: usize,
	pub zenoh_app_count: usize,
	pub stale_app_count: usize,
	pub root_endpoint_count: usize,
	pub endpoint_candidate_count: usize,
	pub endpoint_warning_count: usize,
	pub endpoint_warning_app_count: usize,
	pub discovery_error_count: usize,
	pub query_key_count: usize,
	pub announcement_key_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnAppApiCommandEnvelope {
	pub schema_version: u32,
	pub kind: String,
	pub app_id: String,
	pub instance_id: String,
	pub command: String,
	pub args: JsonValue,
	pub request_id: String,
	pub timeout_ms: u64,
	pub issued_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnAppApiCommandDispatch {
	pub app_id: String,
	pub instance_id: String,
	pub command: String,
	pub request_id: String,
	pub key_expr: String,
	pub response_key_expr: String,
	pub event_key_expr: String,
	pub payload: UnAppApiCommandEnvelope,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnAppApiCommandObservation {
	pub key_expr: String,
	pub source: String,
	pub payload: JsonValue,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnAppApiCommandHistoryItem {
	pub at: String,
	pub app_id: String,
	pub instance_id: String,
	pub command: String,
	pub request_id: String,
	pub key_expr: String,
	pub response_key_expr: String,
	pub event_key_expr: String,
	pub status: String,
	pub response_observed: bool,
	pub response: Option<UnAppApiCommandObservation>,
	pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnAppApiCommandContract {
	pub schema_version: u32,
	pub snapshot_query_key_exprs: Vec<String>,
	pub announcement_key_exprs: Vec<String>,
	pub command_key_expr_template: String,
	pub response_key_expr_template: String,
	pub event_key_expr_template: String,
	pub safe_commands: Vec<String>,
	pub capability_command_fallbacks: Vec<UnAppApiCapabilityCommandFallback>,
	pub sample_snapshot: JsonValue,
	pub sample_command_response: JsonValue,
	pub sample_command_event: JsonValue,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct UnAppApiCapabilityCommandFallback {
	pub capabilities: Vec<String>,
	pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct DiscoveryPayload {
	source_key: String,
	kind: String,
	payload: JsonValue,
}

impl UnAppApiDiscoverySnapshot {
	pub fn endpoint_candidates(&self) -> Vec<JsonValue> {
		self.endpoints
			.iter()
			.cloned()
			.chain(self.apps.iter().flat_map(|app| {
				app.endpoints
					.iter()
					.cloned()
					.map(|endpoint| endpoint_with_app_provenance(endpoint, app))
			}))
			.map(endpoint_with_validation_warnings)
			.collect()
	}

	pub fn endpoint_candidate_diagnostics(&self) -> Vec<UnAppApiEndpointCandidateDiagnostic> {
		self.endpoint_candidates().iter().flat_map(endpoint_candidate_diagnostics).collect()
	}

	pub fn health_summary(&self) -> UnAppApiDiscoveryHealthSummary {
		let endpoint_candidates = self.endpoint_candidates();
		let endpoint_diagnostics = endpoint_candidates
			.iter()
			.flat_map(endpoint_candidate_diagnostics)
			.collect::<Vec<_>>();
		let endpoint_warning_apps = endpoint_diagnostics
			.iter()
			.filter_map(|diagnostic| diagnostic.app_id.as_deref())
			.collect::<std::collections::BTreeSet<_>>();
		UnAppApiDiscoveryHealthSummary {
			source: self.source.clone(),
			app_count: self.apps.len(),
			managed_registry_app_count: self
				.apps
				.iter()
				.filter(|app| app.discovery_source == "managed_app_registry")
				.count(),
			zenoh_app_count: self.apps.iter().filter(|app| app.discovery_source == "zenoh").count(),
			stale_app_count: self.apps.iter().filter(|app| app.stale).count(),
			root_endpoint_count: self.endpoints.len(),
			endpoint_candidate_count: endpoint_candidates.len(),
			endpoint_warning_count: endpoint_diagnostics.len(),
			endpoint_warning_app_count: endpoint_warning_apps.len(),
			discovery_error_count: self.discovery_errors.len(),
			query_key_count: DEFAULT_QUERY_KEYS.len(),
			announcement_key_count: DEFAULT_ANNOUNCEMENT_KEYS.len(),
		}
	}
}

pub async fn discovery_snapshot_from_state(state: &SharedState) -> UnAppApiDiscoverySnapshot {
	discovery_snapshot_with_window(state, None).await
}

/// Camera actions need a continuous reply window, not repeated 80ms queries
/// that discard replies from busy renderers. The caller retains its 5s budget.
pub(crate) async fn camera_discovery_snapshot_from_state(state: &SharedState, window: Duration) -> UnAppApiDiscoverySnapshot {
	discovery_snapshot_with_window(state, Some(window.as_millis().min(2600) as u64)).await
}


async fn discovery_snapshot_with_window(state: &SharedState, window_ms: Option<u64>) -> UnAppApiDiscoverySnapshot {
	let managed_apps = {
		let s = state.read().await;
		s.managed_apps.clone()
	};
	let registry = managed_apps.read().await;
	let generated_at = Timestamp::now();
	let apps = registry
		.specs
		.iter()
		.map(|spec| {
			let status = registry
				.statuses
				.get(&spec.id)
				.cloned()
				.unwrap_or_else(|| crate::managed_app::ManagedAppStatus::unknown(&spec.id));
			UnAppApiDiscoveredApp {
				app_id: spec.unaa_z_app_id.clone().unwrap_or_else(|| spec.id.clone()),
				instance_id: spec
					.unaa_z_instance_id
					.clone()
					.or_else(|| spec.unaa_z_app_id.clone())
					.unwrap_or_else(|| spec.id.clone()),
				session_id: None,
				display_name: spec.label.clone(),
				managed_app_id: Some(spec.id.clone()),
				discovery_source: "managed_app_registry".into(),
				discovery_source_key: Some(format!("managed_app_registry/{}", spec.id)),
				discovery_payload_kind: "managed_app_registry".into(),
				observed_at: generated_at,
				reported_at: Some(status.checked_at),
				age_ms: timestamp_age_ms(generated_at, status.checked_at),
				stale: false,
				running: status.running,
				ready: status.running,
				capabilities: Vec::new(),
				commands: Vec::new(),
				endpoints: Vec::new(),
			}
		})
		.collect();
	// Registry data is owned now. Network discovery must not hold up app
	// start/stop/status writers while waiting for replies from other processes.
	drop(registry);

	let mut snapshot = UnAppApiDiscoverySnapshot {
		schema_version: 1,
		source: "managed_app_registry".into(),
		generated_at,
		apps,
		endpoints: Vec::new(),
		discovery_errors: Vec::new(),
	};
	merge_zenoh_discovery_with_window(&mut snapshot, window_ms).await;
	snapshot
}

pub fn safe_commands() -> &'static [&'static str] {
	SAFE_COMMANDS
}

pub fn command_contract() -> UnAppApiCommandContract {
	UnAppApiCommandContract {
		schema_version: 1,
		snapshot_query_key_exprs: DEFAULT_QUERY_KEYS.iter().map(|s| (*s).to_string()).collect(),
		announcement_key_exprs: DEFAULT_ANNOUNCEMENT_KEYS.iter().map(|s| (*s).to_string()).collect(),
		command_key_expr_template: "un/app/{app_id}/{instance_id}/command".into(),
		response_key_expr_template: "un/app/{app_id}/{instance_id}/response/{request_id}".into(),
		event_key_expr_template: "un/app/{app_id}/{instance_id}/event".into(),
		safe_commands: SAFE_COMMANDS.iter().map(|s| (*s).to_string()).collect(),
		capability_command_fallbacks: vec![
			UnAppApiCapabilityCommandFallback {
				capabilities: vec![
					"supervisor_control".into(),
					"app_control".into(),
					"window_control".into(),
					"gui".into(),
					"renderer".into(),
				],
				commands: vec!["focus".into(), "open_gui".into()],
			},
			UnAppApiCapabilityCommandFallback {
				capabilities: vec!["supervisor_control".into(), "app_control".into(), "profile_control".into()],
				commands: vec!["reload.preview".into(), "profile.switch.preview".into()],
			},
			UnAppApiCapabilityCommandFallback {
				capabilities: vec!["camera_control".into()],
				commands: vec!["camera.effect".into()],
			},
		],
		sample_snapshot: serde_json::json!({
			"schema_version": 1,
			"app_id": "un-avatar",
			"instance_id": "main",
			"session_id": "renderer-session-1",
			"display_name": "UN Avatar",
			"running": true,
			"ready": true,
			"capabilities": ["renderer", "profile_control", "motion_input", "camera_control"],
			"commands": ["focus", "open_gui", "reload.preview", "camera.effect"],
			"endpoints": [{
				"protocol": "unmfz",
				"endpoint_id": "motion-in",
				"direction": "input",
				"base_key_expr": "un/avatar/main/unmfz/motion-in",
				"schema_major": 1
			}]
		}),
		sample_command_response: serde_json::json!({
			"schema_version": 1,
			"kind": "app.command.response",
			"request_id": "unvac-1",
			"ok": true,
			"command": "open_gui",
			"message": "accepted"
		}),
		sample_command_event: serde_json::json!({
			"schema_version": 1,
			"kind": "app.command.event",
			"request_id": "unvac-1",
			"status": "completed",
			"command": "open_gui"
		}),
	}
}

pub fn is_safe_command(command: &str) -> bool {
	SAFE_COMMANDS.iter().any(|allowed| *allowed == command)
}

fn validate_safe_command_args(command: &str, args: &JsonValue) -> Result<(), String> {
	if command == "camera.effect" {
		validate_camera_effect_args(args)?;
	}
	Ok(())
}

/// `camera.effect`はRendererが開始時状態を捕捉し、進入・保持・復帰を原子的に所有する。
/// 汎用command nodeから送る場合も同じ境界を通し、未検査JSONをRendererへ渡さない。
pub fn validate_camera_effect_args(args: &JsonValue) -> Result<(), String> {
	let object = args.as_object().ok_or_else(|| "camera.effect args must be an object".to_string())?;
	let session_id = required_json_string(object, "target_session_id")?;
	validate_key_segment("target_session_id", session_id)?;
	let mut changes_camera = object.contains_key("diagonal_fov_deg");
	if let Some(pose_value) = object.get("pose") {
		let pose = pose_value
			.as_object()
			.ok_or_else(|| "camera.effect pose must be an object".to_string())?;
		match required_json_string(pose, "kind")? {
			"orbit" => {
				reject_json_fields(pose, &["position_xyz", "rotation_euler_deg", "rotation_order"], "orbit pose")?;
				validate_optional_vec3(pose.get("target_xyz"), "pose.target_xyz", -1_000.0..=1_000.0)?;
				validate_optional_number(pose.get("longitude_deg"), "pose.longitude_deg", -3_600.0..=3_600.0)?;
				validate_optional_number(pose.get("latitude_deg"), "pose.latitude_deg", -89.0..=89.0)?;
				validate_optional_number(pose.get("radius"), "pose.radius", 0.05..=100.0)?;
				changes_camera |= ["target_xyz", "longitude_deg", "latitude_deg", "radius"]
					.iter()
					.any(|field| pose.contains_key(*field));
			}
			"cartesian" => {
				reject_json_fields(pose, &["target_xyz", "longitude_deg", "latitude_deg", "radius"], "cartesian pose")?;
				validate_optional_vec3(pose.get("position_xyz"), "pose.position_xyz", -1_000.0..=1_000.0)?;
				changes_camera |= pose.contains_key("position_xyz");
				if let Some(rotation_value) = pose.get("rotation_euler_deg") {
					let rotation = rotation_value
						.as_object()
						.ok_or_else(|| "camera.effect pose.rotation_euler_deg must be an object".to_string())?;
					for axis in ["yaw", "pitch", "roll"] {
						validate_optional_number(rotation.get(axis), &format!("pose.rotation_euler_deg.{axis}"), -3_600.0..=3_600.0)?;
					}
					if rotation
						.get("roll")
						.and_then(JsonValue::as_f64)
						.is_some_and(|roll| roll.abs() > 0.001)
					{
						return Err(
							"camera.effect pose.rotation_euler_deg.roll must be zero because the current orbit camera has no roll axis"
								.into(),
						);
					}
					changes_camera |= ["yaw", "pitch", "roll"].iter().any(|axis| rotation.contains_key(*axis));
				}
				if let Some(rotation_order) = pose.get("rotation_order") {
					if rotation_order.as_str() != Some("yxz") {
						return Err("camera.effect pose.rotation_order must be `yxz`".into());
					}
				}
			}
			other => return Err(format!("camera.effect pose.kind {other:?} is not supported")),
		}
	}
	validate_optional_number(object.get("diagonal_fov_deg"), "diagonal_fov_deg", 1.0..=160.0)?;
	validate_optional_camera_transition(object.get("enter"), "enter", 450)?;
	validate_optional_integer(object.get("hold_ms"), "hold_ms", 0..=3_600_000)?;
	validate_optional_camera_transition(object.get("leave"), "leave", 650)?;
	if let Some(policy) = object.get("conflict_policy") {
		match policy.as_str() {
			Some("replace" | "queue" | "ignore_if_active") => {}
			_ => return Err(format!("camera.effect conflict_policy {policy:?} is not supported")),
		}
	}
	if object
		.get("return_to")
		.is_some_and(|value| value.as_str() != Some("captured_at_start"))
	{
		return Err("camera.effect return_to must be `captured_at_start`".into());
	}
	if !changes_camera {
		return Err("camera.effect must change at least one pose or FOV field".into());
	}
	Ok(())
}

/// Discoveryで確認した同一Rendererセッション以外へのcamera effect送信を拒否する。
/// `instance_id`は論理個体、`session_id`は起動ごとに変わる実行個体として扱う。
pub fn validate_camera_effect_target(app: &UnAppApiDiscoveredApp, args: &JsonValue) -> Result<(), String> {
	validate_camera_effect_args(args)?;
	if !app.running || !app.ready || app.stale {
		return Err("camera.effect target must be fresh, running, and ready".into());
	}
	if !app.capabilities.iter().any(|value| value == "camera_control") || !app.commands.iter().any(|value| value == "camera.effect") {
		return Err("camera.effect target must explicitly advertise camera_control and camera.effect".into());
	}
	let expected_session = app
		.session_id
		.as_deref()
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| "camera.effect target discovery has no session_id".to_string())?;
	let requested_session = args
		.get("target_session_id")
		.and_then(JsonValue::as_str)
		.ok_or_else(|| "camera.effect target_session_id is missing".to_string())?;
	if requested_session != expected_session {
		return Err(format!(
			"camera.effect target_session_id does not match the current Renderer session ({expected_session})"
		));
	}
	Ok(())
}

/// release統合診断向けに、exact Rendererを選び、現在sessionを固定したcamera.effect dispatchを組み立てる。
/// 実際のpublishは呼び出し側が行うため、対象選択と安全検証をZenohなしで試験できる。
pub fn prepare_camera_effect_probe_dispatch(
	snapshot: &UnAppApiDiscoverySnapshot,
	instance_id: &str,
	args_json: &str,
) -> Result<(String, UnAppApiCommandDispatch), String> {
	let instance_id = instance_id.trim();
	if instance_id.is_empty() {
		return Err("instance_id is empty".into());
	}
	let app = snapshot
		.apps
		.iter()
		.find(|app| app.app_id == "un-avatar" && app.instance_id == instance_id)
		.ok_or_else(|| format!("UNAvatar Renderer instance {instance_id:?} was not discovered"))?;
	let session_id = app
		.session_id
		.as_deref()
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| "Renderer discovery has no session_id".to_string())?
		.to_string();
	let mut args = serde_json::from_str::<JsonValue>(args_json).map_err(|error| format!("camera effect args JSON is invalid: {error}"))?;
	let object = args
		.as_object_mut()
		.ok_or_else(|| "camera effect args must be a JSON object".to_string())?;
	object.insert("target_session_id".to_string(), JsonValue::String(session_id.clone()));
	validate_camera_effect_target(app, &args)?;
	let dispatch = build_command_dispatch(
		"un-avatar".to_string(),
		instance_id.to_string(),
		"camera.effect".to_string(),
		args,
		Some(2_000),
	)?;
	Ok((session_id, dispatch))
}

fn reject_json_fields(object: &serde_json::Map<String, JsonValue>, fields: &[&str], context: &str) -> Result<(), String> {
	if let Some(field) = fields.iter().find(|field| object.contains_key(**field)) {
		return Err(format!("camera.effect {context} must not contain {field}"));
	}
	Ok(())
}

fn required_json_string<'a>(object: &'a serde_json::Map<String, JsonValue>, field: &str) -> Result<&'a str, String> {
	object
		.get(field)
		.and_then(JsonValue::as_str)
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| format!("camera.effect {field} must be a non-empty string"))
}

fn validate_vec3(value: Option<&JsonValue>, field: &str, range: std::ops::RangeInclusive<f64>) -> Result<(), String> {
	let values = value
		.and_then(JsonValue::as_array)
		.filter(|values| values.len() == 3)
		.ok_or_else(|| format!("camera.effect {field} must be a 3-number array"))?;
	for value in values {
		validate_number(Some(value), field, range.clone())?;
	}
	Ok(())
}

fn validate_optional_vec3(value: Option<&JsonValue>, field: &str, range: std::ops::RangeInclusive<f64>) -> Result<(), String> {
	match value {
		Some(value) => validate_vec3(Some(value), field, range),
		None => Ok(()),
	}
}

fn validate_number(value: Option<&JsonValue>, field: &str, range: std::ops::RangeInclusive<f64>) -> Result<(), String> {
	let number = value
		.and_then(JsonValue::as_f64)
		.filter(|number| number.is_finite() && range.contains(number))
		.ok_or_else(|| format!("camera.effect {field} must be within {}..={}", range.start(), range.end()))?;
	let _ = number;
	Ok(())
}

fn validate_optional_number(value: Option<&JsonValue>, field: &str, range: std::ops::RangeInclusive<f64>) -> Result<(), String> {
	match value {
		Some(value) => validate_number(Some(value), field, range),
		None => Ok(()),
	}
}

fn validate_integer(value: Option<&JsonValue>, field: &str, range: std::ops::RangeInclusive<u64>) -> Result<u64, String> {
	value
		.and_then(JsonValue::as_u64)
		.filter(|number| range.contains(number))
		.ok_or_else(|| format!("camera.effect {field} must be within {}..={}", range.start(), range.end()))
}

fn validate_optional_integer(value: Option<&JsonValue>, field: &str, range: std::ops::RangeInclusive<u64>) -> Result<(), String> {
	match value {
		Some(value) => validate_integer(Some(value), field, range).map(|_| ()),
		None => Ok(()),
	}
}

fn validate_optional_camera_transition(value: Option<&JsonValue>, field: &str, default_duration_ms: u64) -> Result<(), String> {
	let Some(value) = value else {
		return Ok(());
	};
	let object = value
		.as_object()
		.ok_or_else(|| format!("camera.effect {field} must be an object"))?;
	let easing = object.get("easing").and_then(JsonValue::as_str).unwrap_or("ease_in_out_cubic");
	const EASINGS: &[&str] = &[
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
	if !EASINGS.contains(&easing) {
		return Err(format!("camera.effect {field}.easing {easing:?} is not supported"));
	}
	let duration_ms = match object.get("duration_ms") {
		Some(value) => validate_integer(Some(value), &format!("{field}.duration_ms"), 0..=300_000)?,
		None if easing == "none" => 0,
		None => default_duration_ms,
	};
	if easing == "none" && duration_ms != 0 {
		return Err(format!("camera.effect {field}.easing `none` requires duration_ms=0"));
	}
	Ok(())
}

pub fn allowed_commands_for_app(app: &UnAppApiDiscoveredApp) -> Vec<String> {
	if !app.commands.is_empty() {
		return SAFE_COMMANDS
			.iter()
			.filter(|command| app.commands.iter().any(|advertised| advertised == **command))
			.map(|command| (*command).to_string())
			.collect();
	}
	SAFE_COMMANDS
		.iter()
		.filter(|command| capabilities_allow_command(&app.capabilities, command))
		.map(|command| (*command).to_string())
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

pub fn build_command_dispatch(
	app_id: String,
	instance_id: String,
	command: String,
	args: JsonValue,
	timeout_ms: Option<u64>,
) -> Result<UnAppApiCommandDispatch, String> {
	if !is_safe_command(&command) {
		return Err(format!(
			"UNAA/Z command {command:?} is not allowed by the first-slice safe allow-list"
		));
	}
	validate_safe_command_args(&command, &args)?;
	validate_key_segment("app_id", &app_id)?;
	validate_key_segment("instance_id", &instance_id)?;
	let request_id = format!("unvac-{}", COMMAND_SEQ.fetch_add(1, Ordering::Relaxed));
	let key_expr = format!("un/app/{app_id}/{instance_id}/command");
	let response_key_expr = format!("un/app/{app_id}/{instance_id}/response/{request_id}");
	let event_key_expr = format!("un/app/{app_id}/{instance_id}/event");
	let payload = UnAppApiCommandEnvelope {
		schema_version: 1,
		kind: "app.command".into(),
		app_id: app_id.clone(),
		instance_id: instance_id.clone(),
		command: command.clone(),
		args,
		request_id: request_id.clone(),
		timeout_ms: timeout_ms.unwrap_or(800),
		issued_at: Timestamp::now(),
	};
	Ok(UnAppApiCommandDispatch {
		app_id,
		instance_id,
		command,
		request_id,
		key_expr,
		response_key_expr,
		event_key_expr,
		payload,
	})
}

pub async fn publish_command_with_response_wait(
	dispatch: &UnAppApiCommandDispatch,
	wait_response_ms: u64,
) -> Result<Option<UnAppApiCommandObservation>, String> {
	let dispatch = dispatch.clone();
	run_on_zenoh_runtime(move || async move { publish_command_with_response_wait_inner(&dispatch, wait_response_ms).await }).await
}

pub async fn wait_for_command_observation(
	app_id: String,
	instance_id: String,
	request_id: String,
	wait_response_ms: u64,
) -> Result<Option<UnAppApiCommandObservation>, String> {
	validate_key_segment("app_id", &app_id)?;
	validate_key_segment("instance_id", &instance_id)?;
	validate_key_segment("request_id", &request_id)?;
	let response_key_expr = format!("un/app/{app_id}/{instance_id}/response/{request_id}");
	let event_key_expr = format!("un/app/{app_id}/{instance_id}/event");
	run_on_zenoh_runtime(move || async move {
		wait_for_command_observation_inner(response_key_expr, event_key_expr, request_id, wait_response_ms).await
	})
	.await
}

async fn publish_command_with_response_wait_inner(
	dispatch: &UnAppApiCommandDispatch,
	wait_response_ms: u64,
) -> Result<Option<UnAppApiCommandObservation>, String> {
	let session = persistent_zenoh_session().await?;
	if wait_response_ms == 0 {
		publish_command_with_session(&session, dispatch).await?;
		return Ok(None);
	}
	let response_subscriber = session
		.declare_subscriber(dispatch.response_key_expr.as_str())
		.await
		.map_err(|e| format!("zenoh subscribe {:?} failed: {e}", dispatch.response_key_expr))?;
	let event_subscriber = session
		.declare_subscriber(dispatch.event_key_expr.as_str())
		.await
		.map_err(|e| format!("zenoh subscribe {:?} failed: {e}", dispatch.event_key_expr))?;
	publish_command_with_session(&session, dispatch).await?;
	let wait = Duration::from_millis(wait_response_ms);
	tokio::time::timeout(wait, async {
		loop {
			tokio::select! {
				result = response_subscriber.recv_async() => {
					let sample = result.map_err(|e| format!("zenoh response receive failed on {:?}: {e}", dispatch.response_key_expr))?;
					if let Some(observation) = observation_from_sample(&dispatch.response_key_expr, "response", sample.payload(), &dispatch.request_id, true)? {
						return Ok(Some(observation));
					}
				}
				result = event_subscriber.recv_async() => {
					let sample = result.map_err(|e| format!("zenoh event receive failed on {:?}: {e}", dispatch.event_key_expr))?;
					if let Some(observation) = observation_from_sample(&dispatch.event_key_expr, "event", sample.payload(), &dispatch.request_id, false)? {
						return Ok(Some(observation));
					}
				}
			}
		}
	})
	.await
	.unwrap_or(Ok(None))
}

async fn wait_for_command_observation_inner(
	response_key_expr: String,
	event_key_expr: String,
	request_id: String,
	wait_response_ms: u64,
) -> Result<Option<UnAppApiCommandObservation>, String> {
	let session = zenoh::open(zenoh::Config::default())
		.await
		.map_err(|e| format!("zenoh open failed: {e}"))?;
	let response_subscriber = session
		.declare_subscriber(response_key_expr.as_str())
		.await
		.map_err(|e| format!("zenoh subscribe {response_key_expr:?} failed: {e}"))?;
	let event_subscriber = session
		.declare_subscriber(event_key_expr.as_str())
		.await
		.map_err(|e| format!("zenoh subscribe {event_key_expr:?} failed: {e}"))?;
	let wait = Duration::from_millis(wait_response_ms);
	tokio::time::timeout(wait, async {
		loop {
			tokio::select! {
				result = response_subscriber.recv_async() => {
					let sample = result.map_err(|e| format!("zenoh response receive failed on {response_key_expr:?}: {e}"))?;
					if let Some(observation) = observation_from_sample(&response_key_expr, "response", sample.payload(), &request_id, true)? {
						return Ok(Some(observation));
					}
				}
				result = event_subscriber.recv_async() => {
					let sample = result.map_err(|e| format!("zenoh event receive failed on {event_key_expr:?}: {e}"))?;
					if let Some(observation) = observation_from_sample(&event_key_expr, "event", sample.payload(), &request_id, false)? {
						return Ok(Some(observation));
					}
				}
			}
		}
	})
	.await
	.unwrap_or(Ok(None))
}

async fn publish_command_with_session(session: &zenoh::Session, dispatch: &UnAppApiCommandDispatch) -> Result<(), String> {
	let payload = serde_json::to_string(&dispatch.payload).map_err(|e| format!("UNAA/Z command payload encode failed: {e}"))?;
	if dispatch.command == "camera.effect" {
		// Discovery announcements do not establish that this session knows the
		// command subscriber yet. A one-shot put with no matching subscriber can
		// succeed locally while delivering nothing. Do not retry an effect after
		// publication: the receiver may have acted even if its reply was lost.
		let publisher = session.declare_publisher(dispatch.key_expr.as_str()).await
			.map_err(|e| format!("camera.effect publisher: {e}"))?;
		tokio::time::timeout(Duration::from_millis(dispatch.payload.timeout_ms.max(1)), async {
			loop {
				if publisher.matching_status().await.map_err(|e| e.to_string())?.matching() { return Ok::<_, String>(()); }
				tokio::time::sleep(Duration::from_millis(20)).await;
			}
		}).await.map_err(|_| "camera.effect command subscriber unavailable; command was not sent".to_string())??;
		return publisher.put(payload).await.map_err(|e| format!("camera.effect publish: {e}"));
	}
	session
		.put(dispatch.key_expr.as_str(), payload)
		.await
		.map_err(|e| format!("zenoh put {:?} failed: {e}", dispatch.key_expr))
}

fn observation_from_sample(
	key_expr: &str,
	source: &str,
	payload: &zenoh::bytes::ZBytes,
	request_id: &str,
	exact_response_key: bool,
) -> Result<Option<UnAppApiCommandObservation>, String> {
	let text = payload
		.try_to_string()
		.map_err(|e| format!("zenoh payload decode failed on {key_expr:?}: {e}"))?;
	let value =
		serde_json::from_str::<JsonValue>(&text).map_err(|e| format!("UNAA/Z response payload on {key_expr:?} is not JSON: {e}"))?;
	if !exact_response_key && !json_request_id_matches(&value, request_id) {
		return Ok(None);
	}
	Ok(Some(UnAppApiCommandObservation {
		key_expr: key_expr.to_string(),
		source: source.to_string(),
		payload: value,
	}))
}

fn json_request_id_matches(value: &JsonValue, request_id: &str) -> bool {
	value.get("request_id").and_then(JsonValue::as_str) == Some(request_id)
		|| value.get("requestId").and_then(JsonValue::as_str) == Some(request_id)
}

fn validate_key_segment(label: &str, value: &str) -> Result<(), String> {
	if value.trim().is_empty() {
		return Err(format!("{label} is empty"));
	}
	if value.contains('/') || value.contains('*') || value.contains('?') || value.contains('#') {
		return Err(format!("{label} contains characters that are not valid in a UNAA/Z key segment"));
	}
	Ok(())
}

async fn merge_zenoh_discovery(snapshot: &mut UnAppApiDiscoverySnapshot) {
	merge_zenoh_discovery_with_window(snapshot, None).await;
}

async fn merge_zenoh_discovery_with_window(snapshot: &mut UnAppApiDiscoverySnapshot, window_ms: Option<u64>) {
	if env_flag_false("UNVAC_UNAAZ_ZENOH_DISCOVERY") {
		return;
	}
	let timeout_ms = window_ms.unwrap_or_else(|| std::env::var("UNVAC_UNAAZ_DISCOVERY_TIMEOUT_MS")
		.ok()
		.and_then(|s| s.parse::<u64>().ok())
		.unwrap_or(DEFAULT_DISCOVERY_TIMEOUT_MS));
	let announcement_keys = env_string_list("UNVAC_UNAAZ_DISCOVERY_KEYS")
		.unwrap_or_else(|| DEFAULT_ANNOUNCEMENT_KEYS.iter().map(|s| (*s).to_string()).collect());
	let query_keys = env_string_list("UNVAC_UNAAZ_DISCOVERY_QUERY_KEYS")
		.unwrap_or_else(|| DEFAULT_QUERY_KEYS.iter().map(|s| (*s).to_string()).collect());

	let discovery_result = run_on_zenoh_runtime(move || async move {
		Ok(collect_discovery_with_deadline(timeout_ms, |payloads| Box::pin(async move {
			read_zenoh_payloads_with_window(&announcement_keys, &query_keys, payloads, window_ms.unwrap_or(DISCOVERY_REPLY_DRAIN_MS)).await
		})).await)
	})
	.await;
	match discovery_result {
		Ok((payloads, error)) => {
			if let Some(error) = error {
				snapshot.discovery_errors.push(error);
			}
			if !payloads.is_empty() {
				snapshot.source = "managed_app_registry+zenoh".into();
				merge_json_payloads(snapshot, payloads);
			}
		}
		Err(e) => snapshot.discovery_errors.push(e),
	}
}

/// 管理アプリregistryを使わず、現在のUNAA/Z query/announceだけを能動取得するrelease診断入口。
/// CLIのcold startではZenoh peer scoutingが通常の短い診断窓に間に合わないため、
/// 同一sessionを再利用しつつ有限時間だけ再取得する。
pub async fn zenoh_discovery_snapshot_only() -> UnAppApiDiscoverySnapshot {
	const PROBE_DISCOVERY_WAIT: Duration = Duration::from_secs(3);
	const PROBE_RETRY_INTERVAL: Duration = Duration::from_millis(200);
	let mut snapshot = UnAppApiDiscoverySnapshot {
		schema_version: 1,
		source: "zenoh".to_string(),
		generated_at: Timestamp::now(),
		apps: Vec::new(),
		endpoints: Vec::new(),
		discovery_errors: Vec::new(),
	};
	let deadline = tokio::time::Instant::now() + PROBE_DISCOVERY_WAIT;
	loop {
		merge_zenoh_discovery(&mut snapshot).await;
		if !snapshot.apps.is_empty() || tokio::time::Instant::now() >= deadline {
			break;
		}
		tokio::time::sleep(PROBE_RETRY_INTERVAL).await;
	}
	snapshot
}

pub(crate) async fn run_on_zenoh_runtime<F, Fut, T>(task: F) -> Result<T, String>
where
	F: FnOnce() -> Fut + Send + 'static,
	Fut: Future<Output = Result<T, String>> + Send + 'static,
	T: Send + 'static,
{
	// The shared session and its background tasks must outlive every caller,
	// including requests running on Actix current-thread workers.
	static RUNTIME: std::sync::OnceLock<Result<tokio::runtime::Runtime, String>> = std::sync::OnceLock::new();
	let runtime = RUNTIME.get_or_init(|| {
		TokioRuntimeBuilder::new_multi_thread()
			.worker_threads(1)
			.thread_name("vac-zenoh")
			.enable_all()
			.build()
			.map_err(|e| format!("zenoh runtime build failed: {e}"))
	})
	.as_ref()
	.map_err(Clone::clone)?;
	runtime.spawn(async move { task().await })
	.await
	.map_err(|e| format!("zenoh runtime task failed: {e}"))?
}

/// Synchronous Zenoh probes must not block the shared runtime's async worker.
pub(crate) async fn run_blocking_on_zenoh_runtime<F, T>(task: F) -> Result<T, String>
where
	F: FnOnce() -> Result<T, String> + Send + 'static,
	T: Send + 'static,
{
	run_on_zenoh_runtime(move || async move { tokio::task::block_in_place(task) }).await
}

// Keep received evidence outside the cancellable future. A slow later discovery
// source must not discard an already received Renderer identity/session.
async fn collect_discovery_with_deadline<F>(timeout_ms: u64, collect: F) -> (Vec<DiscoveryPayload>, Option<String>)
where
	F: for<'a> FnOnce(&'a mut Vec<DiscoveryPayload>) -> std::pin::Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>,
{
	let mut payloads = Vec::new();
	let error = match tokio::time::timeout(Duration::from_millis(timeout_ms), collect(&mut payloads)).await {
		Ok(Ok(())) => None,
		Ok(Err(error)) => Some(error),
		Err(_) => Some(format!("zenoh discovery timeout after {timeout_ms}ms")),
	};
	(payloads, error)
}

#[cfg(test)]
async fn read_zenoh_json_payloads_into(announcement_keys: &[String], query_keys: &[String], payloads: &mut Vec<DiscoveryPayload>) -> Result<(), String> {
	read_zenoh_payloads_with_window(announcement_keys, query_keys, payloads, DISCOVERY_REPLY_DRAIN_MS).await
}

async fn read_zenoh_payloads_with_window(announcement_keys: &[String], query_keys: &[String], payloads: &mut Vec<DiscoveryPayload>, reply_wait_ms: u64) -> Result<(), String> {
	let session = persistent_zenoh_session().await?;
	if reply_wait_ms > DISCOVERY_REPLY_DRAIN_MS {
		return read_zenoh_payloads_concurrently(session, announcement_keys, query_keys, payloads, reply_wait_ms).await;
	}
	read_zenoh_query_payloads(session, query_keys, payloads, reply_wait_ms).await?;
	read_zenoh_announcement_payloads(session, announcement_keys, payloads).await
}

// Keep subscriptions alive while queries run. A busy query source must not
// prevent observing the renderer's periodic announcement. JoinSet aborts the
// readers when the caller's bounded collection window ends.
async fn read_zenoh_payloads_concurrently(
	session: &zenoh::Session,
	announcement_keys: &[String],
	query_keys: &[String],
	payloads: &mut Vec<DiscoveryPayload>,
	window_ms: u64,
) -> Result<(), String> {
	let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, &'static str, zenoh::bytes::ZBytes)>(64);
	let mut readers = tokio::task::JoinSet::<Result<(), String>>::new();
	for key in announcement_keys {
		let subscriber = session.declare_subscriber(key.as_str()).await.map_err(|e| format!("zenoh subscribe {key:?} failed: {e}"))?;
		let key = key.clone();
		let tx = tx.clone();
		readers.spawn(async move {
			loop {
				let sample = subscriber.recv_async().await.map_err(|e| format!("zenoh receive {key:?} failed: {e}"))?;
				if tx.send((key.clone(), "announce", sample.payload().clone())).await.is_err() { return Ok(()); }
			}
		});
	}
	for key in query_keys {
		let key = key.clone();
		let session = session.clone();
		let tx = tx.clone();
		readers.spawn(async move {
			loop {
				let replies = session.get(key.as_str())
					.consolidation(zenoh::query::ConsolidationMode::None)
					.timeout(Duration::from_millis(window_ms)).await
					.map_err(|e| format!("zenoh get {key:?} failed: {e}"))?;
				while let Ok(reply) = replies.recv_async().await {
					if let Ok(sample) = reply.result() {
						if tx.send((key.clone(), "query", sample.payload().clone())).await.is_err() { return Ok(()); }
					}
				}
				// Retry discovery if the query completed before peers connected.
				tokio::time::sleep(Duration::from_millis(100)).await;
			}
		});
	}
	drop(tx);
	loop {
		tokio::select! {
			sample = rx.recv() => match sample {
				Some((key, kind, bytes)) => parse_zenoh_json_payload(&key, kind, &bytes, payloads)?,
				None => return Ok(()),
			},
			result = readers.join_next(), if !readers.is_empty() => match result {
				Some(Ok(Err(error))) => return Err(error),
				Some(Err(error)) => return Err(format!("zenoh discovery reader failed: {error}")),
				_ => {},
			},
		}
	}
}

async fn persistent_zenoh_session() -> Result<&'static zenoh::Session, String> {
	if let Some(session) = DISCOVERY_SESSION.get() {
		return Ok(session);
	}
	// Zenoh's default peer scouting delay exceeds an individual discovery
	// window. Let initialization finish even if that short window is cancelled.
	// OnceCell serializes concurrent callers so they share a single session.
	tokio::spawn(async {
		DISCOVERY_SESSION
		.get_or_try_init(|| async {
			zenoh::open(zenoh::Config::default())
				.await
				.map_err(|e| format!("zenoh open failed: {e}"))
		})
		.await
	})
	.await
	.map_err(|e| format!("zenoh session initialization task failed: {e}"))?
}

async fn read_zenoh_query_payloads(session: &zenoh::Session, keys: &[String], payloads: &mut Vec<DiscoveryPayload>, reply_wait_ms: u64) -> Result<(), String> {
	for key in keys {
		let replies = session
			.get(key.as_str())
			// Discovery needs each current reply as it arrives; AUTO/LATEST can
			// hold all replies until every queried peer finishes.
			.consolidation(zenoh::query::ConsolidationMode::None)
			.timeout(Duration::from_millis(reply_wait_ms))
			.await
			.map_err(|e| format!("zenoh get {key:?} failed: {e}"))?;
		while let Ok(result) = tokio::time::timeout(Duration::from_millis(reply_wait_ms), replies.recv_async()).await {
			match result {
				Ok(reply) => match reply.result() {
					Ok(sample) => parse_zenoh_json_payload(key, "query", sample.payload(), payloads)?,
					Err(e) => log::debug!("UNAA/Z query reply on {key:?} is error: {e}"),
				},
				Err(e) => {
					log::debug!("UNAA/Z query replies drained on {key:?}: {e}");
					break;
				}
			}
		}
	}
	Ok(())
}

async fn read_zenoh_announcement_payloads(
	session: &zenoh::Session,
	keys: &[String],
	payloads: &mut Vec<DiscoveryPayload>,
) -> Result<(), String> {
	for key in keys {
		let subscriber = session
			.declare_subscriber(key.as_str())
			.await
			.map_err(|e| format!("zenoh subscribe {key:?} failed: {e}"))?;
		while let Ok(result) = tokio::time::timeout(Duration::from_millis(DISCOVERY_REPLY_DRAIN_MS), subscriber.recv_async()).await {
			match result {
				Ok(sample) => {
					parse_zenoh_json_payload(key, "announce", sample.payload(), payloads)?;
				}
				Err(e) => return Err(format!("zenoh receive failed on {key:?}: {e}")),
			}
		}
	}
	Ok(())
}

fn parse_zenoh_json_payload(
	key: &str,
	kind: &str,
	payload: &zenoh::bytes::ZBytes,
	payloads: &mut Vec<DiscoveryPayload>,
) -> Result<(), String> {
	let text = payload
		.try_to_string()
		.map_err(|e| format!("zenoh payload decode failed on {key:?}: {e}"))?;
	match serde_json::from_str::<JsonValue>(&text) {
		Ok(value) => payloads.push(DiscoveryPayload {
			source_key: key.to_string(),
			kind: kind.to_string(),
			payload: value,
		}),
		Err(e) => log::debug!("UNAA/Z discovery payload on {key:?} is not JSON: {e}"),
	}
	Ok(())
}

fn merge_json_payloads(snapshot: &mut UnAppApiDiscoverySnapshot, payloads: Vec<DiscoveryPayload>) {
	for payload in payloads {
		merge_json_payload(snapshot, payload);
	}
}

fn merge_json_payload(snapshot: &mut UnAppApiDiscoverySnapshot, discovered_payload: DiscoveryPayload) {
	let source_key = discovered_payload.source_key.as_str();
	let kind = discovered_payload.kind.as_str();
	let payload = discovered_payload.payload;
	if let Some(apps) = payload.get("apps").and_then(JsonValue::as_array) {
		for app in apps {
			if let Some(discovered) = app_from_json(app, snapshot.generated_at, Some(source_key), kind) {
				upsert_app(snapshot, discovered);
			}
		}
	}
	if let Some(endpoints) = payload.get("endpoints").and_then(JsonValue::as_array) {
		snapshot.endpoints.extend(
			endpoints
				.iter()
				.cloned()
				.map(|endpoint| endpoint_with_discovery_provenance(endpoint, Some(source_key), Some(kind), Some("zenoh"))),
		);
	}
	if app_from_json(&payload, snapshot.generated_at, Some(source_key), kind).is_some() {
		upsert_app(
			snapshot,
			app_from_json(&payload, snapshot.generated_at, Some(source_key), kind).expect("checked"),
		);
		return;
	}
	if looks_like_endpoint(&payload) {
		snapshot.endpoints.push(endpoint_with_discovery_provenance(
			payload,
			Some(source_key),
			Some(kind),
			Some("zenoh"),
		));
	}
}

fn endpoint_with_app_provenance(endpoint: JsonValue, app: &UnAppApiDiscoveredApp) -> JsonValue {
	let endpoint = endpoint_with_discovery_provenance(
		endpoint,
		app.discovery_source_key.as_deref(),
		Some(app.discovery_payload_kind.as_str()),
		Some(app.discovery_source.as_str()),
	);
	let mut endpoint = endpoint;
	let Some(object) = endpoint.as_object_mut() else {
		return endpoint;
	};
	object.entry("app_id").or_insert_with(|| JsonValue::String(app.app_id.clone()));
	object
		.entry("instance_id")
		.or_insert_with(|| JsonValue::String(app.instance_id.clone()));
	object
		.entry("source_app_id")
		.or_insert_with(|| JsonValue::String(app.app_id.clone()));
	object
		.entry("source_instance_id")
		.or_insert_with(|| JsonValue::String(app.instance_id.clone()));
	object
		.entry("source_app_display_name")
		.or_insert_with(|| JsonValue::String(app.display_name.clone()));
	object.entry("discovery_stale").or_insert_with(|| JsonValue::Bool(app.stale));
	if let Some(age_ms) = app.age_ms {
		object.entry("discovery_age_ms").or_insert_with(|| serde_json::json!(age_ms));
	}
	endpoint
}

fn endpoint_with_discovery_provenance(
	mut endpoint: JsonValue,
	source_key: Option<&str>,
	payload_kind: Option<&str>,
	discovery_source: Option<&str>,
) -> JsonValue {
	let Some(object) = endpoint.as_object_mut() else {
		return endpoint;
	};
	if let Some(discovery_source) = discovery_source.filter(|s| !s.trim().is_empty()) {
		object
			.entry("discovery_source")
			.or_insert_with(|| JsonValue::String(discovery_source.to_string()));
	}
	if let Some(source_key) = source_key.filter(|s| !s.trim().is_empty()) {
		object
			.entry("discovery_source_key")
			.or_insert_with(|| JsonValue::String(source_key.to_string()));
	}
	if let Some(payload_kind) = payload_kind.filter(|s| !s.trim().is_empty()) {
		object
			.entry("discovery_payload_kind")
			.or_insert_with(|| JsonValue::String(payload_kind.to_string()));
	}
	endpoint
}

fn endpoint_with_validation_warnings(mut endpoint: JsonValue) -> JsonValue {
	let diagnostics = endpoint_candidate_diagnostics(&endpoint);
	if diagnostics.is_empty() {
		return endpoint;
	}
	let Some(object) = endpoint.as_object_mut() else {
		return endpoint;
	};
	object
		.entry("validation_warnings")
		.or_insert_with(|| serde_json::to_value(diagnostics).unwrap_or_else(|_| JsonValue::Array(Vec::new())));
	endpoint
}

fn endpoint_candidate_diagnostics(endpoint: &JsonValue) -> Vec<UnAppApiEndpointCandidateDiagnostic> {
	if !looks_like_unmfz_endpoint(endpoint) {
		return Vec::new();
	}
	let app_id = endpoint_app_id(endpoint);
	let endpoint_id = endpoint_endpoint_id(endpoint);
	let source_key = string_json_field(endpoint, "discovery_source_key").or_else(|| string_json_field(endpoint, "discoverySourceKey"));
	let candidate_id = string_json_field(endpoint, "candidate_id").or_else(|| {
		let direction = endpoint_direction(endpoint).unwrap_or_else(|| "both".into());
		let base_key_expr = endpoint_base_key_expr(endpoint).unwrap_or_default();
		Some(format!(
			"{}:{}:{}:{}",
			app_id.clone().unwrap_or_default(),
			endpoint_id.clone().unwrap_or_else(|| "unmfz".into()),
			direction,
			base_key_expr
		))
	});
	let mut out = Vec::new();
	let mut push = |code: &str, field: &str, message: &str| {
		out.push(UnAppApiEndpointCandidateDiagnostic {
			severity: "warning".into(),
			code: code.into(),
			message: message.into(),
			field: field.into(),
			candidate_id: candidate_id.clone(),
			app_id: app_id.clone(),
			endpoint_id: endpoint_id.clone(),
			source_key: source_key.clone(),
		});
	};
	if app_id.as_deref().unwrap_or("").trim().is_empty() {
		push(
			"missing_app_id",
			"app_id",
			"UNMF/Z endpoint candidate is missing app_id/source_app_id; resolver graph cannot target a stable UNAA/Z app.",
		);
	}
	if endpoint_id.as_deref().unwrap_or("").trim().is_empty() {
		push(
			"missing_endpoint_id",
			"endpoint_id",
			"UNMF/Z endpoint candidate is missing endpoint_id; resolver graph cannot identify the endpoint.",
		);
	}
	if endpoint_base_key_expr(endpoint).as_deref().unwrap_or("").trim().is_empty() {
		push(
			"missing_base_key_expr",
			"base_key_expr",
			"UNMF/Z endpoint candidate is missing base_key_expr; GUI can fall back, but generated graph may publish/subscribe to the wrong key.",
		);
	}
	match endpoint_direction(endpoint).as_deref() {
		Some("input" | "output" | "both") => {}
		Some(_) => push(
			"invalid_direction",
			"direction",
			"UNMF/Z endpoint candidate direction must be input, output, or both.",
		),
		None => push(
			"missing_direction",
			"direction",
			"UNMF/Z endpoint candidate is missing direction; GUI treats it as both.",
		),
	}
	out
}

fn looks_like_unmfz_endpoint(value: &JsonValue) -> bool {
	let Some(object) = value.as_object() else {
		return false;
	};
	object.get("unmfz").is_some() || matches!(object.get("protocol").and_then(JsonValue::as_str), Some("unmfz" | "unmf/z"))
}

fn endpoint_app_id(value: &JsonValue) -> Option<String> {
	string_json_field(value, "source_app_id")
		.or_else(|| string_json_field(value, "sourceAppId"))
		.or_else(|| string_json_field(value, "app_id"))
		.or_else(|| string_json_field(value, "appId"))
		.or_else(|| string_json_field(value, "app"))
}

fn endpoint_endpoint_id(value: &JsonValue) -> Option<String> {
	string_json_field(value, "endpoint_id")
		.or_else(|| string_json_field(value, "endpointId"))
		.or_else(|| string_json_field(value, "id"))
		.or_else(|| string_json_field(value, "name"))
}

fn endpoint_direction(value: &JsonValue) -> Option<String> {
	string_json_field(value, "direction").map(|s| s.to_ascii_lowercase())
}

fn endpoint_base_key_expr(value: &JsonValue) -> Option<String> {
	let unmfz = value.get("unmfz");
	string_json_field(value, "base_key_expr")
		.or_else(|| string_json_field(value, "baseKeyExpr"))
		.or_else(|| unmfz.and_then(|v| string_json_field(v, "base_key_expr")))
		.or_else(|| unmfz.and_then(|v| string_json_field(v, "baseKeyExpr")))
}

fn app_from_json(value: &JsonValue, observed_at: Timestamp, source_key: Option<&str>, payload_kind: &str) -> Option<UnAppApiDiscoveredApp> {
	let object = value.as_object()?;
	let app_id = string_json_field(value, "app_id")
		.or_else(|| string_json_field(value, "appId"))
		.or_else(|| string_json_field(value, "id"))?;
	let instance_id = string_json_field(value, "instance_id")
		.or_else(|| string_json_field(value, "instanceId"))
		.unwrap_or_else(|| app_id.clone());
	let display_name = string_json_field(value, "display_name")
		.or_else(|| string_json_field(value, "displayName"))
		.or_else(|| string_json_field(value, "name"))
		.unwrap_or_else(|| app_id.clone());
	let endpoints = object
		.get("endpoints")
		.and_then(JsonValue::as_array)
		.map(|items| items.iter().filter(|v| looks_like_endpoint(v)).cloned().collect())
		.unwrap_or_default();
	let reported_at = timestamp_json_field(value, "reported_at")
		.or_else(|| timestamp_json_field(value, "reportedAt"))
		.or_else(|| timestamp_json_field(value, "updated_at"))
		.or_else(|| timestamp_json_field(value, "updatedAt"))
		.or_else(|| timestamp_json_field(value, "announced_at"))
		.or_else(|| timestamp_json_field(value, "announcedAt"))
		.or_else(|| timestamp_json_field(value, "timestamp"))
		.or_else(|| timestamp_json_field(value, "time"));
	let age_ms = reported_at.and_then(|reported_at| timestamp_age_ms(observed_at, reported_at));
	let stale = age_ms.map(|age_ms| age_ms > discovery_stale_ms()).unwrap_or(false);
	Some(UnAppApiDiscoveredApp {
		app_id,
		instance_id,
		session_id: string_json_field(value, "session_id").or_else(|| string_json_field(value, "sessionId")),
		display_name,
		managed_app_id: string_json_field(value, "managed_app_id").or_else(|| string_json_field(value, "managedAppId")),
		discovery_source: string_json_field(value, "discovery_source")
			.or_else(|| string_json_field(value, "discoverySource"))
			.unwrap_or_else(|| "zenoh".into()),
		discovery_source_key: string_json_field(value, "discovery_source_key")
			.or_else(|| string_json_field(value, "discoverySourceKey"))
			.or_else(|| source_key.map(ToOwned::to_owned)),
		discovery_payload_kind: string_json_field(value, "discovery_payload_kind")
			.or_else(|| string_json_field(value, "discoveryPayloadKind"))
			.unwrap_or_else(|| payload_kind.to_string()),
		observed_at,
		reported_at,
		age_ms,
		stale,
		running: bool_json_field(value, "running").unwrap_or(true),
		ready: bool_json_field(value, "ready").unwrap_or(true),
		capabilities: object
			.get("capabilities")
			.and_then(JsonValue::as_array)
			.map(|items| items.iter().filter_map(JsonValue::as_str).map(ToOwned::to_owned).collect())
			.unwrap_or_default(),
		commands: command_list_from_json(value),
		endpoints,
	})
}

fn upsert_app(snapshot: &mut UnAppApiDiscoverySnapshot, app: UnAppApiDiscoveredApp) {
	if let Some(existing) = snapshot
		.apps
		.iter_mut()
		.find(|existing| existing.app_id == app.app_id && existing.instance_id == app.instance_id)
	{
		*existing = app;
	} else {
		snapshot.apps.push(app);
	}
}

fn looks_like_endpoint(value: &JsonValue) -> bool {
	let Some(object) = value.as_object() else {
		return false;
	};
	object.get("unmfz").is_some()
		|| matches!(object.get("protocol").and_then(JsonValue::as_str), Some("unmfz" | "unmf/z" | "vmc"))
		|| object.contains_key("base_key_expr")
		|| object.contains_key("baseKeyExpr")
}

fn string_json_field(value: &JsonValue, key: &str) -> Option<String> {
	value
		.get(key)
		.and_then(JsonValue::as_str)
		.filter(|s| !s.trim().is_empty())
		.map(|s| s.trim().to_string())
}

fn bool_json_field(value: &JsonValue, key: &str) -> Option<bool> {
	value.get(key).and_then(JsonValue::as_bool)
}

fn timestamp_json_field(value: &JsonValue, key: &str) -> Option<Timestamp> {
	string_json_field(value, key).and_then(|s| s.parse::<Timestamp>().ok())
}

fn timestamp_age_ms(observed_at: Timestamp, reported_at: Timestamp) -> Option<i128> {
	let age_ns = observed_at.as_nanosecond().checked_sub(reported_at.as_nanosecond())?;
	Some(age_ns / 1_000_000)
}

fn discovery_stale_ms() -> i128 {
	std::env::var("UNVAC_UNAAZ_DISCOVERY_STALE_MS")
		.ok()
		.and_then(|s| s.parse::<i128>().ok())
		.filter(|v| *v >= 0)
		.unwrap_or(DEFAULT_DISCOVERY_STALE_MS)
}

fn command_list_from_json(value: &JsonValue) -> Vec<String> {
	let Some(items) = value.get("commands").and_then(JsonValue::as_array) else {
		return Vec::new();
	};
	let mut commands = Vec::new();
	for item in items {
		let command = item.as_str().map(str::to_string).or_else(|| {
			item.as_object().and_then(|object| {
				object
					.get("command")
					.or_else(|| object.get("name"))
					.or_else(|| object.get("id"))
					.and_then(JsonValue::as_str)
					.map(str::to_string)
			})
		});
		let Some(command) = command.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else {
			continue;
		};
		if !commands.iter().any(|existing| existing == &command) {
			commands.push(command);
		}
	}
	commands
}

fn env_string_list(key: &str) -> Option<Vec<String>> {
	std::env::var(key)
		.ok()
		.map(|s| {
			s.split(',')
				.map(str::trim)
				.filter(|s| !s.is_empty())
				.map(ToOwned::to_owned)
				.collect::<Vec<_>>()
		})
		.filter(|items| !items.is_empty())
}

fn env_flag_false(key: &str) -> bool {
	std::env::var(key)
		.ok()
		.map(|s| matches!(s.trim().to_ascii_lowercase().as_str(), "0" | "false" | "off" | "no"))
		.unwrap_or(false)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
	async fn camera_publish_waits_for_delayed_subscriber_and_refuses_missing_receiver() {
		let mut config = zenoh::Config::default();
		config.insert_json5("scouting/multicast/enabled", "false").unwrap();
		config.insert_json5("listen/endpoints", "[]").unwrap();
		let session = zenoh::open(config).await.unwrap();
		let dispatch = build_command_dispatch("un-avatar".into(), "isolated-test".into(), "camera.effect".into(),
			serde_json::json!({"target_session_id":"test-session", "pose":{"kind":"orbit","radius":1.0}, "hold_ms":100}), Some(500)).unwrap();
		let receiver_session = session.clone();
		let key = dispatch.key_expr.clone();
		let receiver = tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(80)).await;
			let subscriber = receiver_session.declare_subscriber(key).await.unwrap();
			let received = tokio::time::timeout(Duration::from_secs(2), subscriber.recv_async()).await.unwrap().unwrap();
			received.payload().try_to_string().unwrap().into_owned()
		});
		publish_command_with_session(&session, &dispatch).await.unwrap();
		let payload: JsonValue = serde_json::from_str(&receiver.await.unwrap()).unwrap();
		assert_eq!(payload["request_id"], dispatch.request_id);
		let mut missing = dispatch.clone();
		missing.key_expr = "un/app/un-avatar/missing-test/command".into();
		missing.payload.timeout_ms = 40;
		assert!(publish_command_with_session(&session, &missing).await.unwrap_err().contains("was not sent"));
		session.close().await.unwrap();
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn concurrent_discovery_receives_announcement_while_query_is_pending() {
		let mut config = zenoh::Config::default();
		config.insert_json5("listen/endpoints", "[]").unwrap();
		config.insert_json5("scouting/multicast/enabled", "false").unwrap();
		config.insert_json5("scouting/delay", "0").unwrap();
		let session = zenoh::open(config).await.unwrap();
		let query_key = "unvac/test/concurrent/snapshot".to_string();
		let announce_key = "unvac/test/concurrent/announce".to_string();
		let queryable = session.declare_queryable(query_key.as_str()).await.unwrap();
		let publisher = session.clone();
		let announce = announce_key.clone();
		let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
		let mut responder = tokio::spawn(async move {
			let query = queryable.recv_async().await.unwrap();
			publisher.put(announce, r#"{"app_id":"un-avatar","instance_id":"test-usagi","session_id":"current","ready":true}"#).await.unwrap();
			let _ = release_rx.await;
			drop(query);
		});
		let (payloads, error) = collect_discovery_with_deadline(500, move |payloads| Box::pin(async move {
			read_zenoh_payloads_concurrently(&session, &[announce_key], &[query_key], payloads, 1000).await
		})).await;
		let _ = release_tx.send(());
		let completed = tokio::time::timeout(Duration::from_secs(2), &mut responder).await;
		responder.abort();
		completed.expect("local query must reach the responder").unwrap();
		assert!(error.is_some(), "collection should end at its bounded deadline");
		assert!(payloads.iter().any(|p| p.kind == "announce" && p.payload["instance_id"] == "test-usagi"), "announcement must arrive before the pending query finishes");
	}

	#[tokio::test]
	async fn blocking_probe_leaves_shared_discovery_runtime_responsive() {
		let (started_tx, started_rx) = tokio::sync::oneshot::channel();
		let (release_tx, release_rx) = std::sync::mpsc::channel();
		let probe = tokio::spawn(run_blocking_on_zenoh_runtime(move || {
			let _ = started_tx.send(());
			release_rx.recv_timeout(Duration::from_secs(2)).map_err(|e| e.to_string())
		}));
		started_rx.await.unwrap();
		let responsive = tokio::time::timeout(Duration::from_millis(500), run_on_zenoh_runtime(|| async { Ok(()) })).await;
		let _ = release_tx.send(());
		probe.await.unwrap().unwrap();
		assert!(responsive.is_ok(), "a blocking probe must not stall concurrent discovery work");
		responsive.unwrap().unwrap();
	}

	#[tokio::test]
	async fn discovery_runtime_keeps_session_background_tasks_between_calls() {
		let (tx, rx) = tokio::sync::oneshot::channel();
		run_on_zenoh_runtime(move || async move {
			tokio::spawn(async move {
				tokio::time::sleep(Duration::from_millis(30)).await;
				let _ = tx.send(());
			});
			Ok(())
		}).await.unwrap();
		assert!(tokio::time::timeout(Duration::from_secs(1), rx).await.unwrap().is_ok(), "discovery runtime must outlive an individual query");
	}

	#[tokio::test]
	async fn discovery_deadline_preserves_received_renderer_payload() {
		let expected = DiscoveryPayload {
			source_key: "un/app/un-avatar/usagi/snapshot".into(),
			kind: "query".into(),
			payload: serde_json::json!({"app_id":"un-avatar","instance_id":"usagi","session_id":"live-session"}),
		};
		let received = expected.clone();
		let (payloads, error) = collect_discovery_with_deadline(10, |payloads| Box::pin(async move {
			payloads.push(received);
			std::future::pending::<Result<(), String>>().await
		})).await;
		assert_eq!(payloads, vec![expected]);
		assert_eq!(error.as_deref(), Some("zenoh discovery timeout after 10ms"));
	}

	#[tokio::test]
	async fn discovery_deadline_retains_errors_without_inventing_results() {
		let (payloads, error) = collect_discovery_with_deadline(10, |_| Box::pin(async {
			Err("source failed".into())
		})).await;
		assert!(payloads.is_empty());
		assert_eq!(error.as_deref(), Some("source failed"));
		let (payloads, error) = collect_discovery_with_deadline(10, |_| Box::pin(async { Ok(()) })).await;
		assert!(payloads.is_empty());
		assert!(error.is_none());
	}

	#[test]
	fn endpoint_candidates_flattens_root_and_app_endpoints() {
		let snapshot = UnAppApiDiscoverySnapshot {
			schema_version: 1,
			source: "test".into(),
			generated_at: Timestamp::now(),
			apps: vec![UnAppApiDiscoveredApp {
				app_id: "una".into(),
				instance_id: "una-1".into(),
				session_id: Some("session-1".into()),
				display_name: "UN Avatar".into(),
				managed_app_id: Some("una".into()),
				discovery_source: "zenoh".into(),
				discovery_source_key: Some("un/app/una/una-1/snapshot".into()),
				discovery_payload_kind: "query".into(),
				observed_at: Timestamp::now(),
				reported_at: None,
				age_ms: None,
				stale: false,
				running: true,
				ready: true,
				capabilities: vec!["motion_input".into()],
				commands: vec!["focus".into()],
				endpoints: vec![serde_json::json!({
					"protocol": "unmfz",
					"endpoint_id": "motion-in"
				})],
			}],
			endpoints: vec![serde_json::json!({
				"protocol": "unmfz",
				"endpoint_id": "root"
			})],
			discovery_errors: Vec::new(),
		};

		let candidates = snapshot.endpoint_candidates();
		assert_eq!(candidates.len(), 2);
		assert_eq!(candidates[0]["endpoint_id"], "root");
		assert_eq!(candidates[1]["endpoint_id"], "motion-in");
		assert_eq!(candidates[1]["app_id"], "una");
		assert_eq!(candidates[1]["source_app_id"], "una");
		assert_eq!(candidates[1]["source_instance_id"], "una-1");
		assert_eq!(candidates[1]["source_app_display_name"], "UN Avatar");
		assert_eq!(candidates[1]["discovery_source_key"], "un/app/una/una-1/snapshot");
		assert_eq!(candidates[1]["discovery_payload_kind"], "query");
		assert_eq!(candidates[1]["discovery_stale"], false);
	}

	#[test]
	fn merge_json_payload_accepts_snapshot_apps_and_endpoints() {
		let mut snapshot = UnAppApiDiscoverySnapshot {
			schema_version: 1,
			source: "test".into(),
			generated_at: Timestamp::now(),
			apps: Vec::new(),
			endpoints: Vec::new(),
			discovery_errors: Vec::new(),
		};
		merge_json_payload(
			&mut snapshot,
			DiscoveryPayload {
				source_key: "un/app/una/main/snapshot".into(),
				kind: "query".into(),
				payload: serde_json::json!({
					"apps": [{
						"app_id": "una",
						"display_name": "UN Avatar",
						"capabilities": ["motion_input"],
						"commands": ["focus", {"command": "open_gui"}],
						"endpoints": [{
							"protocol": "unmfz",
							"endpoint_id": "motion-in",
							"direction": "input"
						}]
					}],
					"endpoints": [{
						"protocol": "unmfz",
						"endpoint_id": "global-motion"
					}]
				}),
			},
		);

		assert_eq!(snapshot.apps.len(), 1);
		assert_eq!(snapshot.apps[0].app_id, "una");
		assert_eq!(snapshot.apps[0].discovery_source_key.as_deref(), Some("un/app/una/main/snapshot"));
		assert_eq!(snapshot.apps[0].discovery_payload_kind, "query");
		assert_eq!(snapshot.apps[0].endpoints.len(), 1);
		assert_eq!(snapshot.apps[0].commands, vec!["focus", "open_gui"]);
		assert_eq!(snapshot.endpoints.len(), 1);
		let candidates = snapshot.endpoint_candidates();
		assert_eq!(candidates.len(), 2);
		assert_eq!(candidates[0]["endpoint_id"], "global-motion");
		assert_eq!(candidates[0]["discovery_source"], "zenoh");
		assert_eq!(candidates[0]["discovery_source_key"], "un/app/una/main/snapshot");
		assert_eq!(candidates[0]["discovery_payload_kind"], "query");
		assert_eq!(candidates[1]["endpoint_id"], "motion-in");
		assert_eq!(candidates[1]["source_app_id"], "una");
		assert_eq!(candidates[1]["source_app_display_name"], "UN Avatar");
	}

	#[test]
	fn endpoint_candidate_diagnostics_warn_on_incomplete_unmfz_metadata() {
		let snapshot = UnAppApiDiscoverySnapshot {
			schema_version: 1,
			source: "test".into(),
			generated_at: Timestamp::now(),
			apps: Vec::new(),
			endpoints: vec![serde_json::json!({
				"protocol": "unmfz",
				"direction": "sideways",
				"discovery_source_key": "un/app/bad/main/snapshot"
			})],
			discovery_errors: Vec::new(),
		};

		let diagnostics = snapshot.endpoint_candidate_diagnostics();
		let codes = diagnostics.iter().map(|d| d.code.as_str()).collect::<Vec<_>>();
		assert!(codes.contains(&"missing_app_id"));
		assert!(codes.contains(&"missing_endpoint_id"));
		assert!(codes.contains(&"missing_base_key_expr"));
		assert!(codes.contains(&"invalid_direction"));
		assert!(diagnostics
			.iter()
			.all(|d| d.source_key.as_deref() == Some("un/app/bad/main/snapshot")));

		let candidates = snapshot.endpoint_candidates();
		assert_eq!(candidates[0]["validation_warnings"].as_array().unwrap().len(), diagnostics.len());
	}

	#[test]
	fn discovery_health_summary_counts_sources_and_warnings() {
		let now = Timestamp::now();
		let snapshot = UnAppApiDiscoverySnapshot {
			schema_version: 1,
			source: "managed_app_registry+zenoh".into(),
			generated_at: now,
			apps: vec![
				UnAppApiDiscoveredApp {
					app_id: "unvac-managed".into(),
					instance_id: "main".into(),
					session_id: None,
					display_name: "UNVAC Managed".into(),
					managed_app_id: Some("unvac-managed".into()),
					discovery_source: "managed_app_registry".into(),
					discovery_source_key: Some("managed_app_registry/unvac-managed".into()),
					discovery_payload_kind: "managed_app_registry".into(),
					observed_at: now,
					reported_at: Some(now),
					age_ms: Some(0),
					stale: false,
					running: true,
					ready: true,
					capabilities: Vec::new(),
					commands: vec!["focus".into()],
					endpoints: Vec::new(),
				},
				UnAppApiDiscoveredApp {
					app_id: "un-motion".into(),
					instance_id: "main".into(),
					session_id: Some("motion-session-1".into()),
					display_name: "UN Motion".into(),
					managed_app_id: None,
					discovery_source: "zenoh".into(),
					discovery_source_key: Some("un/app/un-motion/main/snapshot".into()),
					discovery_payload_kind: "query".into(),
					observed_at: now,
					reported_at: Some(now),
					age_ms: Some(12_000),
					stale: true,
					running: true,
					ready: true,
					capabilities: vec!["motion_output".into()],
					commands: Vec::new(),
					endpoints: vec![serde_json::json!({
						"protocol": "unmfz",
						"endpoint_id": "motion-out",
						"direction": "sideways"
					})],
				},
			],
			endpoints: vec![serde_json::json!({
				"protocol": "unmfz",
				"endpoint_id": "root",
				"direction": "output",
				"base_key_expr": "un-motion/frame/root"
			})],
			discovery_errors: vec!["zenoh discovery failed: timeout".into()],
		};

		let health = snapshot.health_summary();
		assert_eq!(health.source, "managed_app_registry+zenoh");
		assert_eq!(health.app_count, 2);
		assert_eq!(health.managed_registry_app_count, 1);
		assert_eq!(health.zenoh_app_count, 1);
		assert_eq!(health.stale_app_count, 1);
		assert_eq!(health.root_endpoint_count, 1);
		assert_eq!(health.endpoint_candidate_count, 2);
		assert_eq!(health.endpoint_warning_count, 3);
		assert_eq!(health.endpoint_warning_app_count, 1);
		assert_eq!(health.discovery_error_count, 1);
		assert_eq!(health.query_key_count, DEFAULT_QUERY_KEYS.len());
		assert_eq!(health.announcement_key_count, DEFAULT_ANNOUNCEMENT_KEYS.len());
	}

	#[test]
	fn discovery_fixture_preserves_gui_candidate_contract_and_validation() {
		let payload: JsonValue =
			serde_json::from_str(include_str!("../tests/fixtures/unaa_z/discovery_snapshot_mixed.json")).expect("fixture json");
		let mut snapshot = UnAppApiDiscoverySnapshot {
			schema_version: 1,
			source: "test".into(),
			generated_at: "2026-05-30T00:00:05Z".parse::<Timestamp>().expect("timestamp"),
			apps: Vec::new(),
			endpoints: Vec::new(),
			discovery_errors: Vec::new(),
		};
		merge_json_payload(
			&mut snapshot,
			DiscoveryPayload {
				source_key: "un/app/**/snapshot".into(),
				kind: "query".into(),
				payload,
			},
		);

		assert_eq!(snapshot.apps.len(), 2);
		assert_eq!(snapshot.endpoints.len(), 1);
		let candidates = snapshot.endpoint_candidates();
		assert_eq!(candidates.len(), 3);

		let renderer = candidates
			.iter()
			.find(|candidate| candidate.get("endpoint_id").and_then(JsonValue::as_str) == Some("motion-in"))
			.expect("renderer endpoint");
		assert_eq!(renderer["app_id"], "un-avatar");
		assert_eq!(renderer["source_app_display_name"], "UN Avatar Renderer");
		assert_eq!(renderer["discovery_source_key"], "un/app/**/snapshot");
		assert_eq!(renderer["discovery_payload_kind"], "query");
		assert_eq!(renderer["base_key_expr"], "un-motion/frame/renderer-main");
		assert!(renderer.get("validation_warnings").is_none());

		let invalid = candidates
			.iter()
			.find(|candidate| candidate.get("endpoint_id").and_then(JsonValue::as_str) == Some("motion-out"))
			.expect("invalid endpoint");
		let warnings = invalid
			.get("validation_warnings")
			.and_then(JsonValue::as_array)
			.expect("validation warnings");
		let warning_codes = warnings
			.iter()
			.filter_map(|warning| warning.get("code").and_then(JsonValue::as_str))
			.collect::<Vec<_>>();
		assert_eq!(warning_codes, vec!["missing_base_key_expr", "invalid_direction"]);

		let diagnostics = snapshot.endpoint_candidate_diagnostics();
		assert_eq!(diagnostics.len(), 3);
		assert_eq!(
			diagnostics
				.iter()
				.filter(|diagnostic| diagnostic.app_id.as_deref() == Some("un-motion"))
				.count(),
			2
		);
		assert!(diagnostics
			.iter()
			.any(|diagnostic| diagnostic.code == "missing_app_id" && diagnostic.endpoint_id.as_deref() == Some("global-motion")));
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
	async fn zenoh_queryable_snapshot_flows_into_endpoint_candidates() {
		let unique = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap_or_default()
			.as_nanos();
		let key = format!("un/app/unvac-discovery-test-{}/main/snapshot", unique);
		let responder_session = match zenoh::open(zenoh::Config::default()).await {
			Ok(session) => session,
			Err(e) => {
				eprintln!("skipping UNAA/Z discovery queryable smoke: zenoh open failed: {e}");
				return;
			}
		};
		let queryable = match responder_session.declare_queryable(key.as_str()).await {
			Ok(queryable) => queryable,
			Err(e) => {
				eprintln!("skipping UNAA/Z discovery queryable smoke: queryable failed: {e}");
				return;
			}
		};
		let reply_key = key.clone();
		let responder = tokio::spawn(async move {
			let query = queryable.recv_async().await.map_err(|e| format!("query receive failed: {e}"))?;
			query
				.reply(
					reply_key.as_str(),
					serde_json::json!({
						"apps": [{
							"app_id": "unvac-discovery-test",
							"instance_id": "main",
							"display_name": "UNVAC Discovery Test",
							"ready": true,
							"capabilities": ["motion_input"],
							"commands": ["focus", {"command": "open_gui"}],
							"endpoints": [{
								"protocol": "unmfz",
								"endpoint_id": "motion-in",
								"label": "Discovery Motion In",
								"direction": "input",
								"base_key_expr": "un-motion/frame/discovery-test",
								"topic_mode": "frame",
								"schema_major": 1
							}, {
								"protocol": "unmfz",
								"endpoint_id": "broken-out",
								"direction": "sideways"
							}]
						}]
					})
					.to_string(),
				)
				.await
				.map_err(|e| format!("query reply failed: {e}"))?;
			Ok::<(), String>(())
		});
		tokio::time::sleep(std::time::Duration::from_millis(300)).await;

		let mut payloads = Vec::new();
		match read_zenoh_json_payloads_into(&[], std::slice::from_ref(&key), &mut payloads).await {
			Ok(()) => (),
			Err(e) => {
				responder.abort();
				eprintln!("skipping UNAA/Z discovery queryable smoke: read failed: {e}");
				return;
			}
		};
		if payloads.is_empty() {
			responder.abort();
			eprintln!("skipping UNAA/Z discovery queryable smoke: no query reply observed");
			return;
		}
		tokio::time::timeout(std::time::Duration::from_secs(2), responder)
			.await
			.expect("fake discovery app should finish promptly")
			.expect("fake discovery app should not panic")
			.expect("fake discovery app should complete");

		let mut snapshot = UnAppApiDiscoverySnapshot {
			schema_version: 1,
			source: "test".into(),
			generated_at: Timestamp::now(),
			apps: Vec::new(),
			endpoints: Vec::new(),
			discovery_errors: Vec::new(),
		};
		merge_json_payloads(&mut snapshot, payloads);

		assert_eq!(snapshot.apps.len(), 1);
		let app = &snapshot.apps[0];
		assert_eq!(app.app_id, "unvac-discovery-test");
		assert_eq!(app.discovery_source_key.as_deref(), Some(key.as_str()));
		assert_eq!(app.discovery_payload_kind, "query");
		assert_eq!(app.commands, vec!["focus", "open_gui"]);
		let candidates = snapshot.endpoint_candidates();
		assert_eq!(candidates.len(), 2);
		let ok = candidates
			.iter()
			.find(|candidate| candidate.get("endpoint_id").and_then(JsonValue::as_str) == Some("motion-in"))
			.expect("motion-in candidate");
		assert_eq!(ok["source_app_id"], "unvac-discovery-test");
		assert_eq!(ok["source_app_display_name"], "UNVAC Discovery Test");
		assert_eq!(ok["discovery_source_key"], key);
		assert_eq!(ok["discovery_payload_kind"], "query");
		assert_eq!(ok["base_key_expr"], "un-motion/frame/discovery-test");
		let diagnostics = snapshot.endpoint_candidate_diagnostics();
		assert!(diagnostics
			.iter()
			.any(|diagnostic| diagnostic.endpoint_id.as_deref() == Some("broken-out") && diagnostic.code == "invalid_direction"));
	}

	#[test]
	fn command_dispatch_allows_only_safe_commands() {
		let ok = build_command_dispatch("un-avatar".into(), "main".into(), "open_gui".into(), serde_json::json!({}), None).unwrap();
		assert_eq!(ok.key_expr, "un/app/un-avatar/main/command");
		assert_eq!(ok.response_key_expr, format!("un/app/un-avatar/main/response/{}", ok.request_id));
		assert_eq!(ok.event_key_expr, "un/app/un-avatar/main/event");
		assert_eq!(ok.payload.kind, "app.command");
		assert!(build_command_dispatch("un-avatar".into(), "main".into(), "reset".into(), serde_json::json!({}), None,).is_err());
	}

	#[test]
	fn command_dispatch_rejects_bad_key_segments() {
		assert!(build_command_dispatch("un/avatar".into(), "main".into(), "focus".into(), serde_json::json!({}), None,).is_err());
	}

	fn camera_effect_args() -> JsonValue {
		serde_json::json!({
			"target_session_id": "renderer-session-1",
			"pose": {
				"kind": "orbit",
				"target_xyz": [0.0, 1.35, 0.0],
				"longitude_deg": 180.0,
				"latitude_deg": 3.0,
				"radius": 0.85
			},
			"diagonal_fov_deg": 28.0,
			"enter": {"duration_ms": 450, "easing": "ease_in_out_cubic"},
			"hold_ms": 5000,
			"leave": {"duration_ms": 650, "easing": "ease_in_out_cubic"},
			"conflict_policy": "replace",
			"return_to": "captured_at_start"
		})
	}

	#[test]
	fn camera_effect_accepts_ignore_policy_and_ten_minute_hold() {
		let mut args = camera_effect_args();
		args["conflict_policy"] = serde_json::json!("ignore_if_active");
		args["hold_ms"] = serde_json::json!(600000);
		assert!(validate_camera_effect_args(&args).is_ok());
		args["conflict_policy"] = serde_json::json!("ignore_everything");
		assert!(validate_camera_effect_args(&args).is_err());
	}

	#[test]
	fn camera_effect_dispatch_requires_bounded_atomic_effect() {
		let dispatch = build_command_dispatch(
			"un-avatar".into(),
			"renderer-dr-usagi".into(),
			"camera.effect".into(),
			camera_effect_args(),
			Some(800),
		)
		.unwrap();
		assert_eq!(dispatch.key_expr, "un/app/un-avatar/renderer-dr-usagi/command");
		assert_eq!(dispatch.payload.args["return_to"], "captured_at_start");

		let mut bad = camera_effect_args();
		bad["target_session_id"] = JsonValue::Null;
		assert!(build_command_dispatch("un-avatar".into(), "renderer-dr-usagi".into(), "camera.effect".into(), bad, None,).is_err());

		let mut unsafe_fov = camera_effect_args();
		unsafe_fov["diagonal_fov_deg"] = serde_json::json!(180.0);
		assert!(validate_camera_effect_args(&unsafe_fov).is_err());
	}

	#[test]
	fn camera_effect_accepts_explicit_cartesian_pose_and_immediate_transition() {
		let mut args = camera_effect_args();
		args["pose"] = serde_json::json!({
			"kind": "cartesian",
			"position_xyz": [0.0, 1.5, 1.2],
			"rotation_euler_deg": {"yaw": 180.0, "pitch": 0.0, "roll": 0.0},
			"rotation_order": "yxz"
		});
		args["enter"] = serde_json::json!({"duration_ms": 0, "easing": "none"});
		validate_camera_effect_args(&args).unwrap();

		args["pose"]["rotation_euler_deg"]["roll"] = serde_json::json!(1.0);
		assert!(validate_camera_effect_args(&args).unwrap_err().contains("roll must be zero"));
	}

	#[test]
	fn camera_effect_accepts_partial_camera_patch_and_omitted_lifecycle_fields() {
		let radius_only = serde_json::json!({
			"target_session_id": "renderer-session-1",
			"pose": {"kind": "orbit", "radius": 1.05}
		});
		validate_camera_effect_args(&radius_only).unwrap();

		let fov_only = serde_json::json!({
			"target_session_id": "renderer-session-1",
			"diagonal_fov_deg": 48.0,
			"enter": {"easing": "none"},
			"leave": {"duration_ms": 0}
		});
		validate_camera_effect_args(&fov_only).unwrap();

		let cartesian_position_only = serde_json::json!({
			"target_session_id": "renderer-session-1",
			"pose": {"kind": "cartesian", "position_xyz": [0.0, 1.5, 1.2]}
		});
		validate_camera_effect_args(&cartesian_position_only).unwrap();

		let cartesian_rotation_only = serde_json::json!({
			"target_session_id": "renderer-session-1",
			"pose": {"kind": "cartesian", "rotation_euler_deg": {"yaw": 180.0}}
		});
		validate_camera_effect_args(&cartesian_rotation_only).unwrap();
	}

	#[test]
	fn camera_effect_rejects_patch_without_camera_change() {
		let args = serde_json::json!({
			"target_session_id": "renderer-session-1",
			"pose": {"kind": "cartesian", "rotation_order": "yxz"}
		});
		assert!(validate_camera_effect_args(&args).unwrap_err().contains("at least one"));
	}

	#[test]
	fn camera_effect_target_requires_current_advertised_renderer_session() {
		let app = app_from_json(
			&serde_json::json!({
				"app_id": "un-avatar",
				"instance_id": "renderer-dr-usagi",
				"session_id": "renderer-session-1",
				"running": true,
				"ready": true,
				"capabilities": ["renderer", "camera_control"],
				"commands": ["camera.effect"]
			}),
			Timestamp::now(),
			Some("un/app/un-avatar/renderer-dr-usagi/snapshot"),
			"query",
		)
		.unwrap();
		validate_camera_effect_target(&app, &camera_effect_args()).unwrap();

		let mut stale_session = camera_effect_args();
		stale_session["target_session_id"] = serde_json::json!("previous-session");
		assert!(validate_camera_effect_target(&app, &stale_session).is_err());

		let mut mixed_pose = camera_effect_args();
		mixed_pose["pose"]["position_xyz"] = serde_json::json!([0.0, 1.0, 2.0]);
		assert!(validate_camera_effect_args(&mixed_pose).is_err());
	}

	#[test]
	fn camera_effect_probe_selects_exact_renderer_and_injects_current_session() {
		let observed_at = Timestamp::now();
		let apps = [("renderer-dr-usagi", "dr-session"), ("new-avatar-3", "kaltsit-session")]
			.into_iter()
			.map(|(instance_id, session_id)| {
				app_from_json(
					&serde_json::json!({
						"app_id": "un-avatar",
						"instance_id": instance_id,
						"session_id": session_id,
						"running": true,
						"ready": true,
						"capabilities": ["renderer", "camera_control"],
						"commands": ["camera.effect"]
					}),
					observed_at,
					Some("un/app/un-avatar/test/snapshot"),
					"query",
				)
				.unwrap()
			})
			.collect();
		let snapshot = UnAppApiDiscoverySnapshot {
			schema_version: 1,
			source: "test".into(),
			generated_at: observed_at,
			apps,
			endpoints: Vec::new(),
			discovery_errors: Vec::new(),
		};
		let mut args = camera_effect_args();
		args.as_object_mut().unwrap().remove("target_session_id");
		let (session_id, dispatch) = prepare_camera_effect_probe_dispatch(&snapshot, "new-avatar-3", &args.to_string()).unwrap();

		assert_eq!(session_id, "kaltsit-session");
		assert_eq!(dispatch.instance_id, "new-avatar-3");
		assert_eq!(dispatch.payload.args["target_session_id"], "kaltsit-session");
		assert_eq!(dispatch.key_expr, "un/app/un-avatar/new-avatar-3/command");
		assert!(prepare_camera_effect_probe_dispatch(&snapshot, "missing", &args.to_string()).is_err());
	}

	#[tokio::test]
	async fn command_observation_wait_rejects_bad_request_id_before_zenoh_open() {
		let err = wait_for_command_observation("un-avatar".into(), "main".into(), "bad/id".into(), 1)
			.await
			.expect_err("bad request_id should be rejected before Zenoh I/O");
		assert!(err.contains("request_id contains characters"));
	}

	#[test]
	fn request_id_match_accepts_snake_and_camel_case() {
		assert!(json_request_id_matches(&serde_json::json!({"request_id": "r1"}), "r1"));
		assert!(json_request_id_matches(&serde_json::json!({"requestId": "r1"}), "r1"));
		assert!(!json_request_id_matches(&serde_json::json!({"request_id": "r2"}), "r1"));
	}

	#[test]
	fn allowed_commands_intersects_advertised_commands_with_safe_list() {
		let app = app_from_json(
			&serde_json::json!({
				"app_id": "una",
				"commands": [
					"focus",
					{"command": "open_gui"},
					{"name": "reset"},
					{"id": "reload.preview"}
				]
			}),
			Timestamp::now(),
			Some("un/app/una/main/snapshot"),
			"query",
		)
		.unwrap();
		assert_eq!(app.commands, vec!["focus", "open_gui", "reset", "reload.preview"]);
		assert_eq!(
			allowed_commands_for_app(&app),
			vec!["focus".to_string(), "open_gui".to_string(), "reload.preview".to_string()]
		);
	}

	#[test]
	fn allowed_commands_can_fall_back_to_capabilities() {
		let app = app_from_json(
			&serde_json::json!({
				"app_id": "una",
				"capabilities": ["window_control", "profile_control"]
			}),
			Timestamp::now(),
			Some("un/app/una/main/snapshot"),
			"query",
		)
		.unwrap();
		assert_eq!(
			allowed_commands_for_app(&app),
			vec![
				"focus".to_string(),
				"open_gui".to_string(),
				"reload.preview".to_string(),
				"profile.switch.preview".to_string()
			]
		);
	}

	#[test]
	fn command_contract_contains_safe_commands_and_samples() {
		let contract = command_contract();
		assert_eq!(
			contract.safe_commands,
			vec!["focus", "open_gui", "reload.preview", "profile.switch.preview", "camera.effect"]
		);
		assert_eq!(contract.sample_snapshot["commands"][0], "focus");
		assert_eq!(contract.sample_command_response["request_id"], "unvac-1");
		assert_eq!(contract.sample_command_event["status"], "completed");
	}

	#[test]
	fn protocol_manual_mentions_current_contract_surface() {
		let manual = include_str!("../docs/manual/un-app-api-over-zenoh.md");
		let contract = command_contract();
		assert!(manual.contains("GET /api/v1/control/un_app_api/contract"));
		assert!(manual.contains("un/app/{app_id}/{instance_id}/snapshot"));
		assert!(manual.contains("un/app/{app_id}/{instance_id}/announce"));
		assert!(manual.contains("un/app/{app_id}/{instance_id}/live"));
		for key_expr in contract
			.snapshot_query_key_exprs
			.iter()
			.chain(contract.announcement_key_exprs.iter())
		{
			assert!(manual.contains(key_expr), "manual missing key expression {key_expr}");
		}
		assert!(manual.contains(&contract.command_key_expr_template));
		assert!(manual.contains(&contract.response_key_expr_template));
		assert!(manual.contains(&contract.event_key_expr_template));
		for command in &contract.safe_commands {
			assert!(manual.contains(command), "manual missing safe command {command}");
		}
		for fallback in &contract.capability_command_fallbacks {
			for capability in &fallback.capabilities {
				assert!(manual.contains(capability), "manual missing fallback capability {capability}");
			}
			for command in &fallback.commands {
				assert!(manual.contains(command), "manual missing fallback command {command}");
			}
		}
		assert!(manual.contains("UNA / UNM real-app interoperability tests should be added after those apps implement this contract."));
	}

	#[test]
	fn app_from_json_marks_old_reported_timestamp_stale() {
		let observed_at: Timestamp = "2026-05-30T00:00:20Z".parse().unwrap();
		let app = app_from_json(
			&serde_json::json!({
				"app_id": "una",
				"updated_at": "2026-05-30T00:00:00Z"
			}),
			observed_at,
			Some("un/app/una/main/announce"),
			"announce",
		)
		.unwrap();
		assert_eq!(app.age_ms, Some(20_000));
		assert!(app.stale);
		assert_eq!(app.discovery_source_key.as_deref(), Some("un/app/una/main/announce"));
		assert_eq!(app.discovery_payload_kind, "announce");
	}
}

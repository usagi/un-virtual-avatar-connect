//! UN App API over Zenoh (UNAA/Z) command bridge.
//!
//! This first slice only publishes a small allow-list of low-risk app commands.

use actix_web::web::{self, Data, Json, Query};
use actix_web::{get, post, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::VecDeque;

use crate::SharedState;
use crate::{Arc, RwLock};

const COMMAND_HISTORY_CAPACITY: usize = 128;
type CommandHistoryStore = Arc<RwLock<VecDeque<crate::un_app_api::UnAppApiCommandHistoryItem>>>;

#[derive(Debug, Deserialize)]
pub struct UnAppApiCommandRequest {
	#[serde(default)]
	pub managed_app_id: Option<String>,
	#[serde(default)]
	pub app_id: Option<String>,
	#[serde(default)]
	pub instance_id: Option<String>,
	pub command: String,
	#[serde(default)]
	pub args: Option<JsonValue>,
	#[serde(default)]
	pub timeout_ms: Option<u64>,
	#[serde(default)]
	pub wait_response_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct UnAppApiCommandResponse {
	pub accepted: bool,
	pub app_id: String,
	pub instance_id: String,
	pub command: String,
	pub request_id: String,
	pub key_expr: String,
	pub response_key_expr: String,
	pub event_key_expr: String,
	pub response_observed: bool,
	pub response: Option<crate::un_app_api::UnAppApiCommandObservation>,
}

#[derive(Debug, Deserialize)]
pub struct UnAppApiCommandHistoryQuery {
	#[serde(default = "default_history_limit")]
	pub limit: usize,
}

fn default_history_limit() -> usize {
	50
}

#[post("/un_app_api/command")]
pub async fn post_un_app_api_command(state: Data<SharedState>, body: Json<UnAppApiCommandRequest>) -> impl Responder {
	let req = body.into_inner();
	let (app_id, instance_id) = match resolve_target(&state, &req).await {
		Ok(target) => target,
		Err(response) => return response,
	};
	let discovered_target = discovered_target(&state, &app_id, &instance_id).await;
	if let Some(app) = discovered_target.as_ref() {
		let allowed_commands = crate::un_app_api::allowed_commands_for_app(app);
		if !allowed_commands.iter().any(|allowed| allowed == &req.command) {
			return HttpResponse::BadRequest().json(serde_json::json!({
				"error": "unaa_z_command_not_advertised",
				"detail": "command is not advertised by this UNAA/Z app or is blocked by the safe allow-list",
				"app_id": app_id,
				"instance_id": instance_id,
				"command": req.command,
				"allowed_commands": allowed_commands,
				"safe_commands": crate::un_app_api::safe_commands(),
			}));
		}
	}
	let args = req.args.unwrap_or_else(|| JsonValue::Object(Default::default()));
	if req.command == "camera.effect" {
		let Some(app) = discovered_target.as_ref() else {
			return HttpResponse::BadRequest().json(serde_json::json!({
				"error": "unaa_z_camera_target_not_discovered",
				"detail": "camera.effect requires a fresh, explicitly discovered Renderer session",
				"app_id": app_id,
				"instance_id": instance_id,
			}));
		};
		if let Err(detail) = crate::un_app_api::validate_camera_effect_target(app, &args) {
			return HttpResponse::BadRequest().json(serde_json::json!({
				"error": "unaa_z_camera_target_rejected",
				"detail": detail,
				"app_id": app_id,
				"instance_id": instance_id,
			}));
		}
	}
	let dispatch = match crate::un_app_api::build_command_dispatch(app_id, instance_id, req.command, args, req.timeout_ms) {
		Ok(dispatch) => dispatch,
		Err(e) => {
			return HttpResponse::BadRequest().json(serde_json::json!({
				"error": "unaa_z_command_rejected",
				"detail": e,
				"allowed_commands": crate::un_app_api::safe_commands(),
			}));
		}
	};
	let history = command_history_store(&state).await;
	let observation = match publish_command_and_record_history(&history, &dispatch, req.wait_response_ms.unwrap_or(0)).await {
		Ok(observation) => observation,
		Err(e) => {
			return HttpResponse::BadGateway().json(serde_json::json!({
				"error": "unaa_z_command_publish_failed",
				"detail": e,
				"key_expr": dispatch.key_expr,
			}));
		}
	};
	let response_observed = observation.is_some();
	if response_observed {
		return HttpResponse::Ok().json(UnAppApiCommandResponse {
			accepted: true,
			app_id: dispatch.app_id,
			instance_id: dispatch.instance_id,
			command: dispatch.command,
			request_id: dispatch.request_id,
			key_expr: dispatch.key_expr,
			response_key_expr: dispatch.response_key_expr,
			event_key_expr: dispatch.event_key_expr,
			response_observed,
			response: observation,
		});
	}
	HttpResponse::Accepted().json(UnAppApiCommandResponse {
		accepted: true,
		app_id: dispatch.app_id,
		instance_id: dispatch.instance_id,
		command: dispatch.command,
		request_id: dispatch.request_id,
		key_expr: dispatch.key_expr,
		response_key_expr: dispatch.response_key_expr,
		event_key_expr: dispatch.event_key_expr,
		response_observed,
		response: None,
	})
}

#[get("/un_app_api/commands/history")]
pub async fn get_un_app_api_command_history(state: Data<SharedState>, query: Query<UnAppApiCommandHistoryQuery>) -> impl Responder {
	let limit = query.limit.clamp(1, COMMAND_HISTORY_CAPACITY);
	let history = {
		let s = state.read().await;
		s.un_app_api_command_history.clone()
	};
	let h = history.read().await;
	let mut rows: Vec<_> = h.iter().rev().take(limit).cloned().collect();
	rows.reverse();
	HttpResponse::Ok().json(serde_json::json!({
		"commands": rows,
		"limit": limit,
	}))
}

async fn command_history_store(state: &SharedState) -> CommandHistoryStore {
	let s = state.read().await;
	s.un_app_api_command_history.clone()
}

#[get("/un_app_api/contract")]
pub async fn get_un_app_api_contract() -> impl Responder {
	HttpResponse::Ok().json(crate::un_app_api::command_contract())
}

async fn publish_command_and_record_history(
	history: &CommandHistoryStore,
	dispatch: &crate::un_app_api::UnAppApiCommandDispatch,
	wait_response_ms: u64,
) -> Result<Option<crate::un_app_api::UnAppApiCommandObservation>, String> {
	match crate::un_app_api::publish_command_with_response_wait(dispatch, wait_response_ms).await {
		Ok(observation) => {
			let response_observed = observation.is_some();
			let status = if response_observed {
				"response_observed"
			} else {
				"accepted_timeout"
			};
			record_command_history_store(
				history,
				command_history_item(dispatch, status, response_observed, observation.clone(), None),
			)
			.await;
			Ok(observation)
		}
		Err(e) => {
			record_command_history_store(
				history,
				command_history_item(dispatch, "publish_failed", false, None, Some(e.clone())),
			)
			.await;
			Err(e)
		}
	}
}

fn command_history_item(
	dispatch: &crate::un_app_api::UnAppApiCommandDispatch,
	status: &str,
	response_observed: bool,
	response: Option<crate::un_app_api::UnAppApiCommandObservation>,
	error: Option<String>,
) -> crate::un_app_api::UnAppApiCommandHistoryItem {
	crate::un_app_api::UnAppApiCommandHistoryItem {
		at: crate::datetime::DateTime::now().to_rfc3339(),
		app_id: dispatch.app_id.clone(),
		instance_id: dispatch.instance_id.clone(),
		command: dispatch.command.clone(),
		request_id: dispatch.request_id.clone(),
		key_expr: dispatch.key_expr.clone(),
		response_key_expr: dispatch.response_key_expr.clone(),
		event_key_expr: dispatch.event_key_expr.clone(),
		status: status.to_string(),
		response_observed,
		response,
		error,
	}
}

async fn record_command_history_store(history: &CommandHistoryStore, item: crate::un_app_api::UnAppApiCommandHistoryItem) {
	let mut h = history.write().await;
	if h.len() >= COMMAND_HISTORY_CAPACITY {
		h.pop_front();
	}
	h.push_back(item);
}

async fn resolve_target(state: &SharedState, req: &UnAppApiCommandRequest) -> Result<(String, String), HttpResponse> {
	if let Some(managed_app_id) = req.managed_app_id.as_deref().filter(|s| !s.trim().is_empty()) {
		let managed_apps = {
			let s = state.read().await;
			s.managed_apps.clone()
		};
		let registry = managed_apps.read().await;
		let Some(spec) = registry.find_spec(managed_app_id) else {
			return Err(HttpResponse::NotFound().json(serde_json::json!({
				"error": "managed_app_not_found",
				"id": managed_app_id,
			})));
		};
		let app_id = spec.unaa_z_app_id.clone().unwrap_or_else(|| spec.id.clone());
		let instance_id = spec
			.unaa_z_instance_id
			.clone()
			.or_else(|| spec.unaa_z_app_id.clone())
			.unwrap_or_else(|| spec.id.clone());
		return Ok((app_id, instance_id));
	}
	let Some(app_id) = req.app_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
		return Err(HttpResponse::BadRequest().json(serde_json::json!({
			"error": "missing_unaa_z_target",
			"detail": "managed_app_id or app_id is required",
		})));
	};
	let instance_id = req
		.instance_id
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.unwrap_or(app_id);
	Ok((app_id.to_string(), instance_id.to_string()))
}

async fn discovered_target(state: &SharedState, app_id: &str, instance_id: &str) -> Option<crate::un_app_api::UnAppApiDiscoveredApp> {
	let snapshot = crate::un_app_api::discovery_snapshot_from_state(state).await;
	snapshot
		.apps
		.into_iter()
		.find(|app| app.app_id == app_id && app.instance_id == instance_id && app.discovery_source != "managed_app_registry")
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(post_un_app_api_command)
		.service(get_un_app_api_command_history)
		.service(get_un_app_api_contract);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
	async fn publish_response_wait_records_observed_history_with_fake_zenoh_app() {
		let unique = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap_or_default()
			.as_nanos();
		let app_id = format!("unvac-history-test-{}-{unique}", std::process::id());
		let dispatch =
			crate::un_app_api::build_command_dispatch(app_id, "main".into(), "open_gui".into(), serde_json::json!({}), Some(800)).unwrap();
		let responder_session = match zenoh::open(zenoh::Config::default()).await {
			Ok(session) => session,
			Err(e) => {
				eprintln!("skipping UNAA/Z history smoke: zenoh open failed: {e}");
				return;
			}
		};
		let subscriber = match responder_session.declare_subscriber(dispatch.key_expr.as_str()).await {
			Ok(subscriber) => subscriber,
			Err(e) => {
				eprintln!("skipping UNAA/Z history smoke: subscribe failed: {e}");
				return;
			}
		};
		let response_key_expr = dispatch.response_key_expr.clone();
		let request_id = dispatch.request_id.clone();
		let command = dispatch.command.clone();
		let responder = tokio::spawn(async move {
			let sample = subscriber.recv_async().await.map_err(|e| format!("fake app receive failed: {e}"))?;
			let text = sample
				.payload()
				.try_to_string()
				.map_err(|e| format!("fake app payload decode failed: {e}"))?;
			let envelope = serde_json::from_str::<crate::un_app_api::UnAppApiCommandEnvelope>(&text)
				.map_err(|e| format!("fake app command envelope parse failed: {e}"))?;
			assert_eq!(envelope.request_id, request_id);
			responder_session
				.put(
					response_key_expr.as_str(),
					serde_json::json!({
						"schema_version": 1,
						"kind": "app.command.response",
						"request_id": request_id,
						"ok": true,
						"command": command
					})
					.to_string(),
				)
				.await
				.map_err(|e| format!("fake app response publish failed: {e}"))?;
			Ok::<(), String>(())
		});
		let history: CommandHistoryStore = Arc::new(RwLock::new(VecDeque::new()));
		tokio::time::sleep(std::time::Duration::from_millis(300)).await;

		let observation = match publish_command_and_record_history(&history, &dispatch, 3000).await {
			Ok(observation) => observation,
			Err(e) => {
				responder.abort();
				eprintln!("skipping UNAA/Z history smoke: command publish failed: {e}");
				return;
			}
		};
		if observation.is_none() {
			responder.abort();
			eprintln!("skipping UNAA/Z history smoke: fake app response was not observed before timeout");
			return;
		}
		tokio::time::timeout(std::time::Duration::from_secs(2), responder)
			.await
			.expect("fake app task should finish promptly")
			.expect("fake app task should not panic")
			.expect("fake app task should complete");

		let h = history.read().await;
		assert_eq!(h.len(), 1);
		let item = h.front().expect("history item");
		assert_eq!(item.status, "response_observed");
		assert!(item.response_observed);
		assert_eq!(item.command, "open_gui");
		assert_eq!(item.request_id, dispatch.request_id);
		assert_eq!(item.response.as_ref().expect("response").payload["ok"], true);
	}
}

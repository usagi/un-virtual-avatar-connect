//! 配信前セルフテスト。設定編集や汎用effect triggerの開放をせず、TTSと字幕出力の実動作だけを検査する。

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use actix_web::web::{self, Data, Json};
use actix_web::{get, post, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::control_events::{ControlEvent, TtsLifecyclePhase};
use crate::flowgraph::node::TriggerEvent;
use crate::flowgraph::socket::SocketValue;
use crate::state::DataSource;
use crate::streaming_contract::{ChatOutputRoute, ContractCheck, ContractStatus, StreamingContractAudit};
use crate::{ChannelDatum, SharedState};

const DEFAULT_TTS_TEXT: &str = "配信前の音声確認。";
const DEFAULT_SUBTITLE_TEXT: &str = "UNVAC 字幕出力テスト";
const DEFAULT_CHAT_TEXT: &str = "コメント読み上げの経路確認。";
const MAX_TTS_TEXT_CHARS: usize = 80;
const MAX_SUBTITLE_TEXT_CHARS: usize = 120;
const MAX_CHANNELS: usize = 4;
const TTS_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize)]
pub struct PreflightTtsNode {
	pub id: String,
	pub file: String,
	pub active: bool,
	pub evidence: &'static str,
	pub elapsed_ms: Option<u64>,
	pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PreflightSelfTestStatus {
	pub tts_nodes: Vec<PreflightTtsNode>,
	pub default_subtitle_channels: Vec<String>,
	pub streaming_contract: StreamingContractAudit,
	pub chat_output_routes: Vec<ChatOutputRoute>,
	pub avatar_targets: Vec<PreflightAvatarTarget>,
	pub v2_release_gate: ManagedProfileReleaseGate,
}

#[derive(Debug, Serialize)]
pub struct ManagedProfileReleaseGate {
	pub release_ready: bool,
	pub automated_status: ContractStatus,
	pub live_blockers: Vec<&'static str>,
}

fn managed_profile_release_gate(streaming_contract: &StreamingContractAudit) -> ManagedProfileReleaseGate {
	ManagedProfileReleaseGate {
		// 実資格情報、実redemption、mode切替、再起動、長時間配信の証拠は
		// offline self-testから推測しない。release checklistの有人記録が揃うまでfalse固定。
		release_ready: false,
		automated_status: streaming_contract.status,
		live_blockers: vec![
			"daily mode: マイク、AI会話、字幕、AI TTSの継続動作と不要managed app非起動",
			"streaming mode: Twitch実コメント、字幕、COEIROINK、AI会話、speech floorの同時動作",
			"AI Avatar: 瞬き、lip sync、pose、reactionのRenderer到達",
			"Twitch channel points: 実交換のAI/BOS/effect/Avatar到達",
			"mode切替、Flowgraph reload、VAC再起動後のprofile復元とprocess ownership",
			"Studio overviewからの経路追跡とinactive/trace/diagnosticsによる原因特定",
			"長時間配信: 過剰発話、TTS混線、chat欠落、STT停止、CPU/GPU負荷",
		],
	}
}

#[derive(Debug, Clone, Serialize)]
pub struct PreflightAvatarTarget {
	pub target_link_id: String,
	pub base_key_expr: String,
}

#[derive(Debug, Deserialize)]
pub struct TtsSelfTestRequest {
	pub node_id: String,
	#[serde(default)]
	pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TtsSelfTestResponse {
	pub node_id: String,
	pub completed: bool,
	pub played: bool,
	pub elapsed_ms: u64,
	pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SubtitleSelfTestRequest {
	#[serde(default)]
	pub channels: Vec<String>,
	#[serde(default)]
	pub text: Option<String>,
	#[serde(default)]
	pub hold_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct SubtitleSelfTestResponse {
	pub channels: Vec<String>,
	pub datum_ids: Vec<u64>,
	pub hold_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct ChatOutputSelfTestRequest {
	#[serde(default)]
	pub text: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatOutputSelfTestResponse {
	pub subscription_node_id: String,
	pub tts_node_id: String,
	pub subtitle_reached: bool,
	pub completed: bool,
	pub played: bool,
	pub elapsed_ms: u64,
	pub error: Option<String>,
}

fn bad_request(reason: impl Into<String>) -> HttpResponse {
	HttpResponse::BadRequest().json(serde_json::json!({
		"error": "bad_request",
		"reason": reason.into(),
	}))
}

fn bounded_text(value: Option<String>, fallback: &str, max_chars: usize, field: &str) -> Result<String, HttpResponse> {
	let value = value.unwrap_or_else(|| fallback.to_string());
	let value = value.trim();
	if value.is_empty() {
		return Err(bad_request(format!("{field} must not be empty")));
	}
	if value.chars().count() > max_chars {
		return Err(bad_request(format!("{field} must be at most {max_chars} characters")));
	}
	Ok(value.to_string())
}

fn normalize_channels(channels: Vec<String>) -> Result<Vec<String>, HttpResponse> {
	let channels = if channels.is_empty() {
		vec!["user".into(), "ai-synth".into()]
	} else {
		channels
	};
	let mut unique = BTreeSet::new();
	for channel in channels {
		let channel = channel.trim();
		if channel.is_empty() || channel.chars().count() > 64 || channel.chars().any(char::is_control) {
			return Err(bad_request("channel must be 1..64 characters without control characters"));
		}
		unique.insert(channel.to_string());
	}
	if unique.len() > MAX_CHANNELS {
		return Err(bad_request(format!("channels must contain at most {MAX_CHANNELS} values")));
	}
	Ok(unique.into_iter().collect())
}

fn is_matching_preflight_probe(datum: &ChannelDatum, probe_id: &str) -> bool {
	datum.meta.get("source_actor").and_then(serde_json::Value::as_str) == Some(probe_id)
}

fn tts_evidence(phase: TtsLifecyclePhase, played: Option<bool>) -> &'static str {
	match phase {
		TtsLifecyclePhase::Completed if played == Some(true) => "ready",
		TtsLifecyclePhase::Dropped | TtsLifecyclePhase::Failed | TtsLifecyclePhase::Completed => "failed",
		_ => "pending",
	}
}

async fn tts_nodes(state: &SharedState) -> Vec<PreflightTtsNode> {
	let (flowgraph, history) = {
		let state = state.read().await;
		(state.flowgraph.clone(), state.control_event_history.clone())
	};
	let guard = flowgraph.read().await;
	let Some(runtime) = guard.as_ref() else {
		return Vec::new();
	};
	let mut nodes = runtime
		.node_meta
		.iter()
		.filter(|(_, meta)| meta.feature == "flowgraph.tts.speak")
		.map(|(id, meta)| PreflightTtsNode {
			id: id.clone(),
			file: meta.file.display().to_string(),
			active: runtime.trigger_gate.as_ref().is_none_or(|gate| gate.is_exec_active(id)),
			evidence: "untested",
			elapsed_ms: None,
			error: None,
		})
		.collect::<Vec<_>>();
	drop(guard);
	let history = history.read().await;
	for node in &mut nodes {
		let latest = history.iter().rev().find_map(|item| match &item.event {
			ControlEvent::TtsLifecycle {
				node_id,
				phase,
				played,
				elapsed_ms,
				error,
				..
			} if node_id == &node.id => Some((*phase, *played, *elapsed_ms, error.clone())),
			_ => None,
		});
		if let Some((phase, played, elapsed_ms, error)) = latest {
			node.evidence = tts_evidence(phase, played);
			node.elapsed_ms = Some(elapsed_ms);
			node.error = error;
		}
	}
	nodes.sort_by(|a, b| a.id.cmp(&b.id));
	nodes
}

async fn streaming_contract(state: &SharedState) -> StreamingContractAudit {
	let root = {
		let flowgraph = state.read().await.flowgraph.clone();
		let flowgraph = flowgraph.read().await;
		flowgraph.as_ref().map(|runtime| runtime.root_dir.clone())
	};
	let Some(root) = root else {
		return StreamingContractAudit {
			status: ContractStatus::Inactive,
			checks: Vec::new(),
		};
	};
	match tokio::task::spawn_blocking(move || crate::streaming_contract::audit_streaming_contract(&root)).await {
		Ok(Ok(audit)) => audit,
		Ok(Err(error)) => contract_audit_error(error.to_string()),
		Err(error) => contract_audit_error(error.to_string()),
	}
}

async fn chat_output_routes(state: &SharedState) -> Result<Vec<ChatOutputRoute>, String> {
	let root = {
		let flowgraph = state.read().await.flowgraph.clone();
		let flowgraph = flowgraph.read().await;
		flowgraph.as_ref().map(|runtime| runtime.root_dir.clone())
	}
	.ok_or_else(|| "Flowgraphが起動していない。".to_string())?;
	tokio::task::spawn_blocking(move || crate::streaming_contract::chat_output_routes(&root))
		.await
		.map_err(|error| error.to_string())?
		.map_err(|error| error.to_string())
}

fn contract_audit_error(detail: String) -> StreamingContractAudit {
	StreamingContractAudit {
		status: ContractStatus::Blocked,
		checks: vec![ContractCheck {
			id: "audit",
			status: ContractStatus::Blocked,
			detail: format!("配信経路を監査できない: {detail}"),
		}],
	}
}

#[get("/preflight/self-test")]
async fn get_self_test_status(state: Data<SharedState>) -> impl Responder {
	let chat_output_routes = chat_output_routes(state.get_ref()).await.unwrap_or_default();
	let streaming_contract = streaming_contract(state.get_ref()).await;
	let avatar_targets = state
		.read()
		.await
		.avatar_motion
		.configured_targets()
		.into_iter()
		.map(|target| PreflightAvatarTarget {
			target_link_id: target.target_link_id,
			base_key_expr: target.base_key_expr,
		})
		.collect();
	HttpResponse::Ok().json(PreflightSelfTestStatus {
		tts_nodes: tts_nodes(state.get_ref()).await,
		default_subtitle_channels: vec!["user".into(), "ai-synth".into()],
		v2_release_gate: managed_profile_release_gate(&streaming_contract),
		streaming_contract,
		chat_output_routes,
		avatar_targets,
	})
}

#[post("/preflight/self-test/avatar-motion")]
async fn post_avatar_motion_self_test(state: Data<SharedState>) -> impl Responder {
	let (targets, evidence) = {
		let state = state.read().await;
		(state.avatar_motion.configured_targets(), state.avatar_renderer_probe.clone())
	};
	let expected_base_keys = targets.into_iter().map(|target| target.base_key_expr).collect();
	let result =
		match crate::un_app_api::run_blocking_on_zenoh_runtime(move || { Ok(crate::avatar_renderer_probe::probe(expected_base_keys)) })
			.await
		{
			Ok(result) => result,
			Err(error) => {
				return HttpResponse::InternalServerError().json(serde_json::json!({
					"error": "avatar_probe_failed",
					"reason": error,
				}))
			}
		};
	*evidence.write().await = Some(result.clone());
	HttpResponse::Ok().json(result)
}

#[post("/preflight/self-test/tts")]
async fn post_tts_self_test(state: Data<SharedState>, body: Json<TtsSelfTestRequest>) -> impl Responder {
	let request = body.into_inner();
	let node_id = request.node_id.trim().to_string();
	if node_id.is_empty() {
		return bad_request("node_id is required");
	}
	let text = match bounded_text(request.text, DEFAULT_TTS_TEXT, MAX_TTS_TEXT_CHARS, "text") {
		Ok(value) => value,
		Err(response) => return response,
	};

	let (trigger, mut events) = {
		let state_guard = state.read().await;
		let event_rx = state_guard.control_event_tx.subscribe();
		let flowgraph = state_guard.flowgraph.clone();
		drop(state_guard);
		let runtime_guard = flowgraph.read().await;
		let Some(runtime) = runtime_guard.as_ref() else {
			return HttpResponse::ServiceUnavailable().json(serde_json::json!({
				"error": "flowgraph_unavailable"
			}));
		};
		let Some(meta) = runtime.node_meta.get(&node_id) else {
			return HttpResponse::NotFound().json(serde_json::json!({
				"error": "node_not_found",
				"node_id": node_id,
			}));
		};
		if meta.feature != "flowgraph.tts.speak" {
			return bad_request("node_id must identify flowgraph.tts.speak");
		}
		if runtime.trigger_gate.as_ref().is_some_and(|gate| !gate.is_exec_active(&node_id)) {
			return HttpResponse::Conflict().json(serde_json::json!({
				"error": "node_inactive",
				"node_id": node_id,
			}));
		}
		let Some(trigger) = runtime.trigger() else {
			return HttpResponse::ServiceUnavailable().json(serde_json::json!({
				"error": "flowgraph_worker_unavailable"
			}));
		};
		(trigger, event_rx)
	};

	let event = TriggerEvent::new(node_id.clone())
		.with_exec("exec_in")
		.with_override("text", SocketValue::String(text))
		.with_override("lane", SocketValue::String("system".into()))
		.with_override("utterance_id", SocketValue::String(format!("preflight-{}", jiff::Timestamp::now())))
		.with_override("segment_index", SocketValue::Int(0))
		.with_override("non_blocking", SocketValue::Bool(true));
	if let Err(error) = trigger.send(event) {
		return HttpResponse::ServiceUnavailable().json(serde_json::json!({
			"error": "trigger_send_failed",
			"reason": error.to_string(),
		}));
	}

	let outcome = tokio::time::timeout(TTS_TIMEOUT, async {
		loop {
			match events.recv().await {
				Ok(ControlEvent::TtsLifecycle {
					phase,
					node_id: event_node_id,
					played,
					elapsed_ms,
					error,
					..
				}) if event_node_id == node_id
					&& matches!(
						phase,
						TtsLifecyclePhase::Completed | TtsLifecyclePhase::Dropped | TtsLifecyclePhase::Failed
					) =>
				{
					break TtsSelfTestResponse {
						node_id: node_id.clone(),
						completed: phase == TtsLifecyclePhase::Completed,
						played: played.unwrap_or(false),
						elapsed_ms,
						error,
					};
				}
				Ok(_) => {}
				Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
				Err(tokio::sync::broadcast::error::RecvError::Closed) => {
					break TtsSelfTestResponse {
						node_id: node_id.clone(),
						completed: false,
						played: false,
						elapsed_ms: 0,
						error: Some("control event channel closed".into()),
					};
				}
			}
		}
	})
	.await;

	match outcome {
		Ok(response) => HttpResponse::Ok().json(response),
		Err(_) => HttpResponse::GatewayTimeout().json(serde_json::json!({
			"error": "tts_self_test_timeout",
			"node_id": node_id,
			"timeout_ms": TTS_TIMEOUT.as_millis(),
		})),
	}
}

#[post("/preflight/self-test/chat-output")]
async fn post_chat_output_self_test(state: Data<SharedState>, body: Json<ChatOutputSelfTestRequest>) -> impl Responder {
	let text = match bounded_text(body.into_inner().text, DEFAULT_CHAT_TEXT, MAX_TTS_TEXT_CHARS, "text") {
		Ok(value) => value,
		Err(response) => return response,
	};
	let routes = match chat_output_routes(state.get_ref()).await {
		Ok(routes) => routes,
		Err(reason) => {
			return HttpResponse::ServiceUnavailable().json(serde_json::json!({ "error": "route_audit_failed", "reason": reason }))
		}
	};
	if routes.len() != 1 {
		return HttpResponse::Conflict().json(serde_json::json!({
			"error": "chat_output_route_ambiguous",
			"reason": format!("コメント出力経路は1本必要だが、{}本見つかった。", routes.len()),
		}));
	}
	let route = routes.into_iter().next().expect("length checked");
	let probe_id = format!("{}{}", ChannelDatum::PREFLIGHT_ACTOR_PREFIX, jiff::Timestamp::now());
	let (trigger, mut events) = {
		let state_guard = state.read().await;
		let event_rx = state_guard.control_event_tx.subscribe();
		let flowgraph = state_guard.flowgraph.clone();
		drop(state_guard);
		let runtime_guard = flowgraph.read().await;
		let Some(runtime) = runtime_guard.as_ref() else {
			return HttpResponse::ServiceUnavailable().json(serde_json::json!({ "error": "flowgraph_unavailable" }));
		};
		for node_id in [&route.subscription_node_id, &route.tts_node_id] {
			if !runtime.node_meta.contains_key(node_id) {
				return HttpResponse::Conflict().json(serde_json::json!({ "error": "route_node_not_loaded", "node_id": node_id }));
			}
			if runtime.trigger_gate.as_ref().is_some_and(|gate| !gate.is_exec_active(node_id)) {
				return HttpResponse::Conflict().json(serde_json::json!({ "error": "route_node_inactive", "node_id": node_id }));
			}
		}
		let Some(trigger) = runtime.trigger() else {
			return HttpResponse::ServiceUnavailable().json(serde_json::json!({ "error": "flowgraph_worker_unavailable" }));
		};
		(trigger, event_rx)
	};

	let event = TriggerEvent::new(route.subscription_node_id.clone())
		.with_exec("__trigger__")
		.with_override("__channel__", SocketValue::String("chat".into()))
		.with_override("__content__", SocketValue::String(text))
		.with_override("__source_actor__", SocketValue::String(probe_id.clone()))
		.with_override("__trace_id__", SocketValue::String(probe_id.clone()))
		.with_override("__is_final__", SocketValue::Bool(true))
		.with_override("__meta__", SocketValue::Json(serde_json::json!({ "preflight_probe": true })));
	if let Err(error) = trigger.send(event) {
		return HttpResponse::ServiceUnavailable().json(serde_json::json!({
			"error": "trigger_send_failed",
			"reason": error.to_string(),
		}));
	}

	let started = Instant::now();
	let outcome = tokio::time::timeout(TTS_TIMEOUT, async {
		let mut subtitle_reached = false;
		let mut tts_terminal: Option<(TtsLifecyclePhase, bool, u64, Option<String>)> = None;
		loop {
			match events.recv().await {
				Ok(ControlEvent::ChannelDatum { id, .. }) => {
					if state
						.read()
						.await
						.rfind_channel_datum(id)
						.await
						.is_some_and(|datum| is_matching_preflight_probe(&datum, &probe_id))
					{
						subtitle_reached = true;
					}
				}
				Ok(ControlEvent::TtsLifecycle {
					phase,
					node_id,
					utterance_id,
					played,
					elapsed_ms,
					error,
					..
				}) if node_id == route.tts_node_id
					&& utterance_id == probe_id
					&& matches!(
						phase,
						TtsLifecyclePhase::Completed | TtsLifecyclePhase::Dropped | TtsLifecyclePhase::Failed
					) =>
				{
					tts_terminal = Some((phase, played.unwrap_or(false), elapsed_ms, error));
				}
				Ok(_) => {}
				Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
				Err(tokio::sync::broadcast::error::RecvError::Closed) => {
					return ChatOutputSelfTestResponse {
						subscription_node_id: route.subscription_node_id.clone(),
						tts_node_id: route.tts_node_id.clone(),
						subtitle_reached,
						completed: false,
						played: false,
						elapsed_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
						error: Some("control event channel closed".into()),
					};
				}
			}
			if subtitle_reached {
				if let Some((phase, played, elapsed_ms, error)) = tts_terminal.take() {
					return ChatOutputSelfTestResponse {
						subscription_node_id: route.subscription_node_id.clone(),
						tts_node_id: route.tts_node_id.clone(),
						subtitle_reached,
						completed: phase == TtsLifecyclePhase::Completed,
						played,
						elapsed_ms,
						error,
					};
				}
			}
		}
	})
	.await;

	match outcome {
		Ok(response) => HttpResponse::Ok().json(response),
		Err(_) => HttpResponse::GatewayTimeout().json(serde_json::json!({
			"error": "chat_output_self_test_timeout",
			"subscription_node_id": route.subscription_node_id,
			"tts_node_id": route.tts_node_id,
			"timeout_ms": TTS_TIMEOUT.as_millis(),
		})),
	}
}

#[post("/preflight/self-test/subtitles")]
async fn post_subtitle_self_test(state: Data<SharedState>, body: Json<SubtitleSelfTestRequest>) -> impl Responder {
	let request = body.into_inner();
	let channels = match normalize_channels(request.channels) {
		Ok(value) => value,
		Err(response) => return response,
	};
	let text = match bounded_text(request.text, DEFAULT_SUBTITLE_TEXT, MAX_SUBTITLE_TEXT_CHARS, "text") {
		Ok(value) => value,
		Err(response) => return response,
	};
	let hold_ms = request.hold_ms.unwrap_or(2000).clamp(500, 5000);
	let mut datum_ids = Vec::with_capacity(channels.len());
	{
		let state_guard = state.read().await;
		for channel in &channels {
			let mut datum =
				ChannelDatum::new(channel.clone(), text.clone()).with_source(DataSource::new("control.preflight").with_actor("gui"));
			datum.meta.insert("preflight_probe".into(), serde_json::Value::Bool(true));
			datum_ids.push(datum.get_id());
			state_guard.push_channel_datum_output_only(datum).await;
		}
	}

	let clear_state = state.get_ref().clone();
	let clear_channels = channels.clone();
	tokio::spawn(async move {
		tokio::time::sleep(Duration::from_millis(hold_ms)).await;
		let state_guard = clear_state.read().await;
		for channel in clear_channels {
			let mut datum = ChannelDatum::new(channel, String::new()).with_source(DataSource::new("control.preflight").with_actor("gui"));
			datum.meta.insert("preflight_probe_clear".into(), serde_json::Value::Bool(true));
			state_guard.push_channel_datum_output_only(datum).await;
		}
	});

	HttpResponse::Ok().json(SubtitleSelfTestResponse {
		channels,
		datum_ids,
		hold_ms,
	})
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(get_self_test_status)
		.service(post_tts_self_test)
		.service(post_chat_output_self_test)
		.service(post_avatar_motion_self_test)
		.service(post_subtitle_self_test);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn subtitle_channels_are_trimmed_deduplicated_and_sorted() {
		let channels = normalize_channels(vec![" user ".into(), "ai-synth".into(), "user".into()]).unwrap();
		assert_eq!(channels, ["ai-synth", "user"]);
	}

	#[test]
	fn subtitle_channel_limit_is_enforced() {
		let channels = (0..=MAX_CHANNELS).map(|index| format!("ch-{index}")).collect();
		assert!(normalize_channels(channels).is_err());
	}

	#[test]
	fn chat_output_probe_matches_exact_actor_across_bos_destinations() {
		let probe_id = format!("{}123", ChannelDatum::PREFLIGHT_ACTOR_PREFIX);
		let datum = ChannelDatum::new("chat-synth".into(), "test".into()).with_meta("source_actor", probe_id.clone());
		assert!(is_matching_preflight_probe(&datum, &probe_id));
		assert!(!is_matching_preflight_probe(&datum, "control.preflight:other"));
	}

	#[test]
	fn self_test_text_is_bounded() {
		assert!(bounded_text(Some(" ".into()), DEFAULT_TTS_TEXT, MAX_TTS_TEXT_CHARS, "text").is_err());
		assert!(bounded_text(
			Some("x".repeat(MAX_TTS_TEXT_CHARS + 1)),
			DEFAULT_TTS_TEXT,
			MAX_TTS_TEXT_CHARS,
			"text"
		)
		.is_err());
		assert_eq!(
			bounded_text(None, DEFAULT_TTS_TEXT, MAX_TTS_TEXT_CHARS, "text").unwrap(),
			DEFAULT_TTS_TEXT
		);
	}

	#[test]
	fn tts_evidence_requires_completed_playback() {
		assert_eq!(tts_evidence(TtsLifecyclePhase::Started, None), "pending");
		assert_eq!(tts_evidence(TtsLifecyclePhase::Completed, Some(true)), "ready");
		assert_eq!(tts_evidence(TtsLifecyclePhase::Completed, Some(false)), "failed");
		assert_eq!(tts_evidence(TtsLifecyclePhase::Failed, Some(false)), "failed");
	}

	#[test]
	fn release_gate_never_promotes_offline_contract_to_release_ready() {
		let contract = StreamingContractAudit {
			status: ContractStatus::Ready,
			checks: Vec::new(),
		};
		let gate = managed_profile_release_gate(&contract);
		assert!(!gate.release_ready);
		assert_eq!(gate.automated_status, ContractStatus::Ready);
		assert_eq!(gate.live_blockers.len(), 7);
	}
}

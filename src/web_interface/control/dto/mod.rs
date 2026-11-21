//! Control API の **`GET /snapshot` 用 read-only DTO**。
//!
//! 外部型（`AiPersonaConf` など）をそのまま serialize するとフィールドのカップリングや将来の互換に弱いので、
//! ここで型を切り分けて GUI 向けの表示・診断用 DTO を明示する。
//!
//! δ-9 (v0.9.x) で V1 processor 層を除去したため、`processors` フィールドはスキーマから外している。
//! GUI のフローレイアウトは `GET /api/v1/control/flowgraph/*` 系で取得する想定。
//!
//! 外部サービスの認可検証は専用APIへ分離し、snapshot構築中には外部通信しない。

mod readiness;

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

use crate::control_events::ControlEvent;
use crate::state::{AiRuntime, AiUsageTelemetry, State};

use readiness::{build_readiness, ReadinessFacts, TtsRuntimeEvidence};

/// `/api/v1/control/snapshot` のルート DTO。
#[derive(Debug, Serialize)]
pub struct StateSnapshot {
	/// API DTO のスキーマバージョン。互換性破壊時にインクリメント。
	/// v2でOAuth外部検証を専用APIへ分離した際にschema=3へ更新。
	pub schema: u32,
	pub app_version: &'static str,
	pub now: String,
	pub runtime: RuntimeSummary,
	pub ai_personas: Vec<AiPersonaSummary>,
	pub voice_input: VoiceInputSummary,
	pub bridges: BridgeSummary,
	pub readiness: ReadinessSummary,
	pub twitch: Option<TwitchSummary>,
}

#[derive(Debug, Serialize)]
pub struct RuntimeSummary {
	pub session_id: String,
	pub root: String,
	pub session_dir: String,
	pub inline_max_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct AiPersonaSummary {
	pub index: usize,
	pub id: Option<String>,
	pub display_name: String,
	pub paused: bool,
	pub usage: AiUsageSummary,
}

#[derive(Debug, Serialize)]
pub struct VoiceInputSummary {
	pub muted: bool,
	pub worker_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BridgeSummary {
	pub voice_nodes: usize,
	pub voice_workers: usize,
	pub voice_worker_statuses: Vec<VoiceWorkerStatusSummary>,
	pub voice_ingresses: Vec<VoiceIngressSummary>,
	pub twitch_irc_nodes: usize,
	pub twitch_irc_workers: usize,
	pub twitch_irc_ready_workers: usize,
	pub twitch_irc_joined_channels: usize,
	pub twitch_irc_expected_channels: usize,
	pub twitch_irc_ingresses: Vec<TwitchIrcIngressSummary>,
	pub twitch_eventsub_nodes: usize,
	pub twitch_eventsub_workers: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct VoiceIngressSummary {
	pub node_id: String,
	pub label: String,
	pub source: String,
	pub engine: String,
	pub active_backend: Option<String>,
	pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TwitchIrcIngressSummary {
	pub node_id: String,
	pub label: String,
	pub credential_key: Option<String>,
	pub configured_channels: Vec<String>,
	pub joined_channels: Vec<String>,
	pub ready: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct VoiceWorkerStatusSummary {
	pub node_id: String,
	pub engine: String,
	pub requested_backend: String,
	pub active_backend: String,
	pub partial_interval_ms: u64,
	pub partial_min_audio_ms: u64,
	pub partial_attempt_count: u64,
	pub partial_count: u64,
	pub partial_deadline_miss_count: u64,
	pub final_count: u64,
	pub error_count: u64,
	pub last_partial_inference_ms: Option<u64>,
	pub last_final_inference_ms: Option<u64>,
	pub last_activity_at: Option<String>,
	pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
	Ready,
	Attention,
	Blocked,
	Inactive,
	Unverified,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadinessCheck {
	pub id: String,
	pub area: String,
	pub status: ReadinessStatus,
	pub title: String,
	pub detail: String,
	pub required: bool,
	pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadinessSummary {
	pub status: ReadinessStatus,
	pub mode: Option<String>,
	pub checks: Vec<ReadinessCheck>,
}

#[derive(Debug, Serialize)]
pub struct AiUsageSummary {
	/// AI処理が正常完了した回数。音声または字幕として応答した回数とは限らない。
	pub completed_reactions: u64,
	pub failed_reactions: u64,
	pub api_requests: u64,
	pub tool_calls: u64,
	pub input_tokens: u64,
	pub cached_tokens: u64,
	pub cache_write_tokens: u64,
	pub output_tokens: u64,
	pub reasoning_tokens: u64,
	pub last_model: Option<String>,
	pub last_latency_ms: Option<u64>,
	pub last_updated_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TwitchSummary {
	pub username: String,
	pub channel_to: String,
	pub reads: Option<Vec<String>>,
	pub eventsub_enabled: bool,
	pub moderator_enabled: bool,
	pub moderator_login: Option<String>,
	pub ignore_logins: Vec<String>,
}

/// `State` から構築できる read-only な snapshot を組み立てる。
pub async fn snapshot(state: &State) -> StateSnapshot {
	let runtime = RuntimeSummary {
		session_id: state.runtime_paths.session_id.clone(),
		root: state.runtime_paths.root.display().to_string(),
		session_dir: state.runtime_paths.session_dir.display().to_string(),
		inline_max_bytes: state.runtime_paths.inline_max_bytes,
	};

	let ai_personas = {
		let guard = state.ai_runtimes.read().await;
		let mut summaries = Vec::with_capacity(guard.len());
		for (index, runtime) in guard.iter().enumerate() {
			summaries.push(ai_persona_summary(index, runtime).await);
		}
		summaries
	};

	let twitch = if let Some(t) = state.twitch.as_ref() {
		let ignore_logins = t.ignore_logins.as_ref().cloned().unwrap_or_default();

		Some(TwitchSummary {
			username: t.username.clone(),
			channel_to: t.effective_channel_to().to_string(),
			reads: t.reads.clone(),
			eventsub_enabled: t.eventsub.as_ref().map(|e| e.enabled).unwrap_or(false),
			moderator_enabled: t.moderator.as_ref().map(|m| m.enabled).unwrap_or(false),
			moderator_login: t.moderator.as_ref().and_then(|m| m.login.clone()),
			ignore_logins,
		})
	} else {
		None
	};
	let (voice_input, bridges, readiness) = {
		let (catalog, is_active, flowgraph_ok, flowgraph_error_count, tts_node_ids, bos_output_nodes, avatar_motion_nodes) = {
			let flowgraph = state.flowgraph.read().await;
			match flowgraph.as_ref() {
				Some(runtime) => {
					let gate = runtime.trigger_gate.clone();
					let (tts_node_ids, bos_output_nodes, avatar_motion_nodes) = {
						let active = |node_id: &str| gate.as_ref().map(|value| value.is_exec_active(node_id)).unwrap_or(true);
						let active_feature_ids = |feature: &str| {
							runtime
								.node_meta
								.iter()
								.filter(|(id, meta)| meta.feature == feature && active(id))
								.map(|(id, _)| id.clone())
								.collect::<Vec<_>>()
						};
						let active_feature_count = |feature: &str| {
							runtime
								.node_meta
								.iter()
								.filter(|(id, meta)| meta.feature == feature && active(id))
								.count()
						};
						(
							active_feature_ids("flowgraph.tts.speak"),
							active_feature_count("flowgraph.bos.output"),
							active_feature_count("flowgraph.avatar.intent_play_unmfz")
								+ active_feature_count("flowgraph.avatar.pose_asset_play_unmfz"),
						)
					};
					(
						crate::bridges::collect_all(&runtime.node_meta),
						gate,
						runtime.ok,
						runtime
							.diagnostics
							.iter()
							.filter(|item| item.severity == crate::flowgraph::loader::Severity::Error)
							.count(),
						tts_node_ids,
						bos_output_nodes,
						avatar_motion_nodes,
					)
				}
				None => (crate::bridges::BridgeCatalog::default(), None, false, 1, Vec::new(), 0, 0),
			}
		};
		let active = |node_id: &str| is_active.as_ref().map(|value| value.is_exec_active(node_id)).unwrap_or(true);
		let active_voice_nodes = catalog.voice.iter().filter(|entry| active(&entry.node_id)).count();
		let active_twitch_nodes = catalog
			.twitch
			.iter()
			.filter(|entry| active(&entry.node_id) && entry.mode == "irc")
			.count();
		let active_eventsub_nodes = catalog.twitch_eventsub.iter().filter(|entry| active(&entry.node_id)).count();
		let handles = state.bridge_handles.lock().await;
		let (twitch_irc_joined_channels, twitch_irc_expected_channels) = handles.twitch_irc_channel_counts();
		let voice_worker_statuses = handles
			.voice_worker_statuses()
			.into_iter()
			.map(|status| VoiceWorkerStatusSummary {
				node_id: status.node_id,
				engine: status.engine,
				requested_backend: status.requested_backend,
				active_backend: status.active_backend,
				partial_interval_ms: status.partial_interval_ms,
				partial_min_audio_ms: status.partial_min_audio_ms,
				partial_attempt_count: status.partial_attempt_count,
				partial_count: status.partial_count,
				partial_deadline_miss_count: status.partial_deadline_miss_count,
				final_count: status.final_count,
				error_count: status.error_count,
				last_partial_inference_ms: status.last_partial_inference_ms,
				last_final_inference_ms: status.last_final_inference_ms,
				last_activity_at: status.last_activity_at,
				last_error: status.last_error,
			})
			.collect::<Vec<_>>();
		let voice_ingresses = catalog
			.voice
			.iter()
			.filter(|entry| active(&entry.node_id))
			.map(|entry| {
				let status = voice_worker_statuses.iter().find(|status| status.node_id == entry.node_id);
				VoiceIngressSummary {
					node_id: entry.node_id.clone(),
					label: display_label(&entry.label, &entry.node_id),
					source: if entry.source.trim().is_empty() {
						"既定の入力デバイス".into()
					} else {
						entry.source.clone()
					},
					engine: entry.engine.clone(),
					active_backend: status.map(|status| status.active_backend.clone()),
					running: status.is_some(),
				}
			})
			.collect();
		let twitch_irc_ingresses = catalog
			.twitch
			.iter()
			.filter(|entry| active(&entry.node_id) && entry.mode == "irc")
			.map(|entry| {
				let handle = handles.twitch.iter().find(|handle| handle.node_id() == entry.node_id);
				TwitchIrcIngressSummary {
					node_id: entry.node_id.clone(),
					label: display_label(&entry.label, &entry.node_id),
					credential_key: (!entry.token_key.trim().is_empty()).then(|| entry.token_key.clone()),
					configured_channels: entry.subscription_channels(),
					joined_channels: handle.map(|handle| handle.joined_channels()).unwrap_or_default(),
					ready: handle.is_some_and(|handle| handle.is_ready()),
				}
			})
			.collect();
		let bridges = BridgeSummary {
			voice_nodes: active_voice_nodes,
			voice_workers: handles.voice_worker_count(),
			voice_worker_statuses,
			voice_ingresses,
			twitch_irc_nodes: active_twitch_nodes,
			twitch_irc_workers: handles.twitch_irc_worker_count(),
			twitch_irc_ready_workers: handles.twitch_irc_ready_worker_count(),
			twitch_irc_joined_channels,
			twitch_irc_expected_channels,
			twitch_irc_ingresses,
			twitch_eventsub_nodes: active_eventsub_nodes,
			twitch_eventsub_workers: handles.twitch_eventsub_worker_count(),
		};
		let voice_input = VoiceInputSummary {
			muted: state.voice_input.is_muted(),
			worker_count: handles.voice_worker_count(),
		};
		drop(handles);
		let runtime_issues = state.runtime_health.snapshot().await;
		let twitch_eventsub_error = runtime_issues
			.iter()
			.find(|item| item.component == "twitch_eventsub")
			.map(|item| item.message.clone());
		let ai_active = ai_personas.iter().filter(|item| !item.paused).count();
		let mode = state.runtime_mode_id.read().ok().and_then(|value| value.clone());
		let history = state.control_event_history.read().await;
		let active_tts_nodes = tts_node_ids.iter().cloned().collect::<BTreeSet<_>>();
		let mut tts_runtime_by_node = BTreeMap::new();
		for item in history.iter().rev() {
			if let ControlEvent::TtsLifecycle {
				phase,
				node_id,
				engine,
				played,
				elapsed_ms,
				error,
				..
			} = &item.event
			{
				if active_tts_nodes.contains(node_id) && !tts_runtime_by_node.contains_key(node_id) {
					tts_runtime_by_node.insert(
						node_id.clone(),
						TtsRuntimeEvidence {
							node_id: node_id.clone(),
							phase: *phase,
							engine: engine.clone(),
							played: *played,
							elapsed_ms: *elapsed_ms,
							error: error.clone(),
						},
					);
				}
			}
		}
		let tts_runtime = tts_runtime_by_node.into_values().collect();
		let output_channels_seen: Vec<String> = history
			.iter()
			.filter_map(|item| {
				if let ControlEvent::ChannelDatum { channel, content, .. } = &item.event {
					(!content.trim().is_empty()).then(|| channel.trim().to_string())
				} else {
					None
				}
			})
			.filter(|channel| !channel.is_empty())
			.collect::<BTreeSet<_>>()
			.into_iter()
			.collect();
		let twitch_message_received = history
			.iter()
			.any(|item| matches!(item.event, ControlEvent::TwitchChatReceived { .. }));
		drop(history);
		let twitch_eventsub_event_received = state.twitch_eventsub_event_received.load(std::sync::atomic::Ordering::Acquire);
		let keys = state.avatar_motion.configured_targets().into_iter().map(|t| t.base_key_expr).collect::<Vec<_>>();
		crate::avatar_renderer_probe::refresh_in_background(state.avatar_renderer_probe.clone(), state.avatar_renderer_probe_refresh.clone(), keys.clone());
		let avatar_motion_probe = if keys.is_empty() && avatar_motion_nodes > 0 {
			Some(crate::avatar_renderer_probe::AvatarRendererProbeResult {
				checked_at: jiff::Timestamp::now().to_string(), expected_base_keys: vec![], renderers: vec![],
			})
		} else {
			state.avatar_renderer_probe.read().await.clone().filter(|p| p.fresh_for(&keys))
		};
		let readiness = build_readiness(ReadinessFacts {
			mode,
			flowgraph_ok,
			flowgraph_error_count,
			ai_total: ai_personas.len(),
			ai_active,
            ai_errors: runtime_issues.iter().filter(|issue| issue.component == "ai_reaction" && ai_personas.iter().any(|p|
                !p.paused && ["request_or_tool_loop", "turn_output_parse"].iter().any(|stage| issue.key == format!("ai_reaction:{}:{stage}", p.id.as_deref().unwrap_or("-")))
            )).cloned().collect(),
			voice_nodes: bridges.voice_nodes,
			voice_workers: bridges.voice_workers,
			voice_worker_statuses: bridges.voice_worker_statuses.clone(),
			voice_muted: voice_input.muted,
			tts_node_ids,
			tts_runtime,
			bos_output_nodes,
			output_channels_seen,
			twitch_irc_nodes: bridges.twitch_irc_nodes,
            twitch_irc_unconfigured: catalog.twitch.iter().filter(|entry| active(&entry.node_id) && entry.mode == "irc" && entry.subscription_channels().is_empty()).count(),
			twitch_irc_workers: bridges.twitch_irc_workers,
			twitch_irc_ready_workers: bridges.twitch_irc_ready_workers,
			twitch_irc_joined_channels: bridges.twitch_irc_joined_channels,
			twitch_irc_expected_channels: bridges.twitch_irc_expected_channels,
			twitch_message_received,
			twitch_eventsub_nodes: bridges.twitch_eventsub_nodes,
            twitch_eventsub_setup_missing: state.twitch.as_ref().and_then(|t| t.eventsub.as_ref()).is_none(),
			twitch_eventsub_workers: bridges.twitch_eventsub_workers,
			twitch_eventsub_error,
			twitch_eventsub_event_received,
			avatar_motion_nodes,
			avatar_motion_probe,
		});
		(voice_input, bridges, readiness)
	};
	StateSnapshot {
		schema: 3,
		app_version: env!("CARGO_PKG_VERSION"),
		now: jiff::Timestamp::now().to_string(),
		runtime,
		ai_personas,
		voice_input,
		bridges,
		readiness,
		twitch,
	}
}

async fn ai_persona_summary(index: usize, rt: &AiRuntime) -> AiPersonaSummary {
	let usage = rt.usage.read().await.clone();
	AiPersonaSummary {
		index,
		id: rt.persona_id.clone(),
		display_name: rt
			.display_name
			.as_deref()
			.filter(|value| !value.trim().is_empty())
			.or(rt.persona_id.as_deref())
			.unwrap_or("AIパートナー")
			.to_string(),
		paused: rt.is_paused(),
		usage: ai_usage_summary(usage),
	}
}

fn display_label(label: &str, node_id: &str) -> String {
	let label = label.trim();
	if label.is_empty() {
		node_id.to_string()
	} else {
		label.to_string()
	}
}

fn ai_usage_summary(usage: AiUsageTelemetry) -> AiUsageSummary {
	AiUsageSummary {
		completed_reactions: usage.completed_reactions,
		failed_reactions: usage.failed_reactions,
		api_requests: usage.api_requests,
		tool_calls: usage.tool_calls,
		input_tokens: usage.input_tokens,
		cached_tokens: usage.cached_tokens,
		cache_write_tokens: usage.cache_write_tokens,
		output_tokens: usage.output_tokens,
		reasoning_tokens: usage.reasoning_tokens,
		last_model: usage.last_model,
		last_latency_ms: usage.last_latency_ms,
		last_updated_at: usage.last_updated_at,
	}
}

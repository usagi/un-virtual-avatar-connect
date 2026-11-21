use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use super::{Result, DEFAULT_DESKTOP_BASE_URL as DEFAULT_BASE_URL};

const DEFAULT_CHANNELS: &str = "user,ai-synth";
// AI observe / AvatarIntentは公開channelではなくtyped edgeなので、ChannelDatumで自動照合できる
// 外部BOS effectだけを既定にする。AI/Avatarはrelease gateのruntime trace + Renderer有人証跡で判定する。
const DEFAULT_REWARD_CHANNELS: &str = "effect";
const DEFAULT_TTS_NODES: &str = "assistant-output/main::speak,twitch-chat/main::speak";
const STREAM_EVIDENCE_EVENT_LIMIT: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
	Ready,
	Attention,
	Blocked,
}

#[derive(Debug, Serialize)]
struct Check {
	id: String,
	status: CheckStatus,
	detail: String,
}

#[derive(Debug, Serialize)]
struct Report {
	status: CheckStatus,
	base_url: String,
	checks: Vec<Check>,
}

struct Options {
	base_url: String,
	token: String,
	allow_non_streaming: bool,
	json: bool,
	channels: Vec<String>,
	expected_reward_id: Option<String>,
	expected_reward_title: Option<String>,
	expected_reward_channels: Vec<String>,
	live_session_from: Option<PathBuf>,
	expected_live_minutes: u64,
	expected_tts_nodes: Vec<String>,
	evidence_out: Option<PathBuf>,
	force: bool,
}

#[derive(Debug, Serialize)]
struct Evidence<'a> {
	schema_version: u32,
	recorded_at: String,
	report: &'a Report,
	snapshot: Value,
	subtitle_output: Value,
	event_history: Value,
	quick_effects: Value,
}

pub(crate) fn run(args: Vec<String>) -> Result<()> {
	let options = parse_options(&args)?;
	let snapshot = get_json(&format!("{}/api/v1/control/snapshot", options.base_url), &options.token)?;
	let mut checks = snapshot_checks(&snapshot, options.allow_non_streaming);
	for channel in &options.channels {
		checks.push(check_obs_template(&options, channel));
	}
	let (subtitle_check, subtitle_output) = check_output_channels(&options, &options.channels);
	checks.push(subtitle_check);
	let need_event_history = options.evidence_out.is_some()
		|| options.expected_reward_id.is_some()
		|| options.expected_reward_title.is_some()
		|| options.live_session_from.is_some();
	let event_history = if need_event_history {
		Some(get_json(
			&format!(
				"{}/api/v1/control/events/history?limit={STREAM_EVIDENCE_EVENT_LIMIT}",
				options.base_url
			),
			&options.token,
		)?)
	} else {
		None
	};
	if options.expected_reward_id.is_some() || options.expected_reward_title.is_some() {
		checks.push(check_reward_route(
			event_history.as_ref().expect("event history is fetched for reward checks"),
			options.expected_reward_id.as_deref(),
			options.expected_reward_title.as_deref(),
			&options.expected_reward_channels,
		));
	}
	if let Some(path) = options.live_session_from.as_deref() {
		let started_at = read_evidence_recorded_at(path, &options.base_url)?;
		checks.extend(check_live_session(
			event_history.as_ref().expect("event history is fetched for live-session checks"),
			started_at,
			jiff::Timestamp::now(),
			options.expected_live_minutes,
			&options.channels,
			&options.expected_tts_nodes,
		));
	}
	let status = report_status(&checks);
	let report = Report {
		status,
		base_url: options.base_url.clone(),
		checks,
	};
	if let Some(path) = options.evidence_out.as_deref() {
		let quick_effects = get_json(
			&format!("{}/api/v1/control/quick_effects?limit=100", options.base_url),
			&options.token,
		)?;
		let evidence = Evidence {
			schema_version: 1,
			recorded_at: jiff::Timestamp::now().to_string(),
			report: &report,
			snapshot: redact_runtime_text(snapshot),
			subtitle_output: redact_runtime_text(subtitle_output),
			event_history: redact_runtime_text(event_history.clone().unwrap_or_else(|| json!({"events": []}))),
			quick_effects: redact_runtime_text(quick_effects),
		};
		write_evidence(path, &evidence, options.force)?;
		if !options.json {
			println!("配信証拠を保存: {}", path.display());
		}
	}
	if options.json {
		println!("{}", serde_json::to_string_pretty(&report)?);
	} else {
		println!("UNVAC 配信前チェック: {:?}", report.status);
		for check in &report.checks {
			println!("- [{:?}] {}: {}", check.status, check.id, check.detail);
		}
	}
	if status == CheckStatus::Blocked {
		return Err("stream check found blocking issue(s)".into());
	}
	Ok(())
}

fn parse_options(args: &[String]) -> Result<Options> {
	let mut options = Options {
		base_url: DEFAULT_BASE_URL.into(),
		token: String::new(),
		allow_non_streaming: false,
		json: false,
		channels: parse_channels(DEFAULT_CHANNELS)?,
		expected_reward_id: None,
		expected_reward_title: None,
		expected_reward_channels: parse_csv(DEFAULT_REWARD_CHANNELS, "--expect-reward-channels")?,
		live_session_from: None,
		expected_live_minutes: 10,
		expected_tts_nodes: parse_csv(DEFAULT_TTS_NODES, "--expect-tts-nodes")?,
		evidence_out: None,
		force: false,
	};
	let mut index = 0;
	while index < args.len() {
		match args[index].as_str() {
			"--base-url" => {
				options.base_url = args
					.get(index + 1)
					.ok_or("--base-url requires a value")?
					.trim_end_matches('/')
					.to_string();
				index += 2;
			}
			"--token" => {
				options.token = args.get(index + 1).ok_or("--token requires a value")?.clone();
				index += 2;
			}
			"--channels" => {
				options.channels = parse_channels(args.get(index + 1).ok_or("--channels requires a value")?)?;
				index += 2;
			}
			"--expect-reward-id" => {
				options.expected_reward_id = Some(parse_non_empty(
					args.get(index + 1).ok_or("--expect-reward-id requires a value")?,
					"--expect-reward-id",
				)?);
				index += 2;
			}
			"--expect-reward-title" => {
				options.expected_reward_title = Some(parse_non_empty(
					args.get(index + 1).ok_or("--expect-reward-title requires a value")?,
					"--expect-reward-title",
				)?);
				index += 2;
			}
			"--expect-reward-channels" => {
				options.expected_reward_channels = parse_csv(
					args.get(index + 1).ok_or("--expect-reward-channels requires a value")?,
					"--expect-reward-channels",
				)?;
				index += 2;
			}
			"--expect-live-from" => {
				options.live_session_from = Some(PathBuf::from(args.get(index + 1).ok_or("--expect-live-from requires a value")?));
				index += 2;
			}
			"--expect-live-minutes" => {
				options.expected_live_minutes = parse_positive_u64(
					args.get(index + 1).ok_or("--expect-live-minutes requires a value")?,
					"--expect-live-minutes",
				)?;
				index += 2;
			}
			"--expect-tts-nodes" => {
				options.expected_tts_nodes = parse_csv(
					args.get(index + 1).ok_or("--expect-tts-nodes requires a value")?,
					"--expect-tts-nodes",
				)?;
				index += 2;
			}
			"--allow-non-streaming" => {
				options.allow_non_streaming = true;
				index += 1;
			}
			"--json" => {
				options.json = true;
				index += 1;
			}
			"--evidence-out" => {
				options.evidence_out = Some(PathBuf::from(args.get(index + 1).ok_or("--evidence-out requires a value")?));
				index += 2;
			}
			"--force" => {
				options.force = true;
				index += 1;
			}
			other => return Err(format!("unknown stream-check option: {other}").into()),
		}
	}
	if options.live_session_from.is_none()
		&& args
			.iter()
			.any(|arg| matches!(arg.as_str(), "--expect-live-minutes" | "--expect-tts-nodes"))
	{
		return Err("--expect-live-minutes and --expect-tts-nodes require --expect-live-from".into());
	}
	if options.expected_reward_id.is_none()
		&& options.expected_reward_title.is_none()
		&& args.iter().any(|arg| arg == "--expect-reward-channels")
	{
		return Err("--expect-reward-channels requires --expect-reward-id or --expect-reward-title".into());
	}
	Ok(options)
}

fn parse_channels(value: &str) -> Result<Vec<String>> {
	parse_csv(value, "--channels")
}

fn parse_csv(value: &str, option: &str) -> Result<Vec<String>> {
	let channels = value
		.split(',')
		.map(str::trim)
		.filter(|channel| !channel.is_empty())
		.map(str::to_string)
		.collect::<Vec<_>>();
	if channels.is_empty() {
		return Err(format!("{option} must contain at least one value").into());
	}
	Ok(channels)
}

fn parse_non_empty(value: &str, option: &str) -> Result<String> {
	let value = value.trim();
	if value.is_empty() {
		return Err(format!("{option} must not be empty").into());
	}
	Ok(value.to_string())
}

fn parse_positive_u64(value: &str, option: &str) -> Result<u64> {
	let value = value.parse::<u64>().map_err(|_| format!("{option} must be a positive integer"))?;
	if value == 0 {
		return Err(format!("{option} must be greater than zero").into());
	}
	Ok(value)
}

fn read_evidence_recorded_at(path: &Path, base_url: &str) -> Result<jiff::Timestamp> {
	let evidence: Value = serde_json::from_slice(&fs::read(path)?)?;
	let recorded_url = evidence.pointer("/report/base_url").and_then(Value::as_str)
		.ok_or("Preflight evidence has no connection identity; capture a new preflight")?;
	if recorded_url.trim_end_matches('/') != base_url.trim_end_matches('/') {
		return Err("Preflight evidence belongs to a different VAC endpoint; do not combine these sessions".into());
	}
	let value = evidence
		.get("recorded_at")
		.and_then(Value::as_str)
		.ok_or_else(|| format!("{} has no recorded_at", path.display()))?;
	value
		.parse::<jiff::Timestamp>()
		.map_err(|error| format!("invalid recorded_at in {}: {error}", path.display()).into())
}

fn check_live_session(
	history: &Value,
	started_at: jiff::Timestamp,
	now: jiff::Timestamp,
	expected_minutes: u64,
	expected_channels: &[String],
	expected_tts_nodes: &[String],
) -> Vec<Check> {
	let Some(events) = history.get("events").and_then(Value::as_array) else {
		return vec![Check {
			id: "live_event_history".into(),
			status: CheckStatus::Blocked,
			detail: "Control Event履歴にevents配列がない。".into(),
		}];
	};
	let elapsed = now.duration_since(started_at);
	let elapsed_secs = elapsed.as_secs().max(0) as u64;
	let expected_secs = expected_minutes.saturating_mul(60);
	let mut checks = vec![Check {
		id: "live_duration".into(),
		status: if elapsed_secs >= expected_secs {
			CheckStatus::Ready
		} else {
			CheckStatus::Blocked
		},
		detail: format!(
			"preflightから{}分{}秒経過（必要{}分）。",
			elapsed_secs / 60,
			elapsed_secs % 60,
			expected_minutes
		),
	}];
	let window = events
		.iter()
		.filter(|row| {
			row.get("at")
				.and_then(Value::as_str)
				.and_then(|at| at.parse::<jiff::Timestamp>().ok())
				.is_some_and(|at| at >= started_at && at <= now)
		})
		.collect::<Vec<_>>();
	let earliest = events
		.first()
		.and_then(|row| row.get("at"))
		.and_then(Value::as_str)
		.and_then(|at| at.parse::<jiff::Timestamp>().ok());
	let truncated = events.len() >= STREAM_EVIDENCE_EVENT_LIMIT && earliest.is_some_and(|at| at > started_at);
	checks.push(Check {
		id: "live_event_history".into(),
		status: if truncated { CheckStatus::Blocked } else { CheckStatus::Ready },
		detail: if truncated {
			format!("履歴が{}件上限に達し、preflight時刻より前の証拠を保持できていない。", events.len())
		} else {
			format!("preflight以降のControl Eventを{}件保持。", window.len())
		},
	});
	let count_kind = |kind: &str| window.iter().filter(|row| row["event"]["kind"] == kind).count();
	let lagged = count_kind("lagged");
	checks.push(Check {
		id: "live_event_delivery".into(),
		status: if lagged == 0 { CheckStatus::Ready } else { CheckStatus::Blocked },
		detail: if lagged == 0 {
			"Control Eventのlagged通知なし。".into()
		} else {
			format!("Control Eventのlagged通知が{lagged}件ある。")
		},
	});
	let voice_partial = count_event_field(&window, "voice_recognition", "phase", "partial");
	let voice_final = count_event_field(&window, "voice_recognition", "phase", "final");
	checks.push(count_check(
		"live_voice",
		voice_partial > 0 && voice_final > 0,
		format!("音声認識 partial {voice_partial}件 / final {voice_final}件。"),
	));
	let ai_decisions = count_kind("ai_turn_decision");
	checks.push(count_check(
		"live_ai_turns",
		ai_decisions > 0,
		format!("AI発話判断 {ai_decisions}件。"),
	));
	let ai_events = window
		.iter()
		.filter(|row| row["event"]["kind"] == "ai_turn_decision")
		.copied()
		.collect::<Vec<_>>();
	let (spoken, silent, max_consecutive_spoken) = ai_cadence(&ai_events);
	checks.push(Check {
		id: "live_ai_cadence".into(),
		status: if spoken == 0 || silent == 0 {
			CheckStatus::Blocked
		} else if max_consecutive_spoken > 3 {
			CheckStatus::Attention
		} else {
			CheckStatus::Ready
		},
		detail: format!("AI判断の発話 {spoken}件 / 沈黙 {silent}件 / 最大連続発話 {max_consecutive_spoken}件（4件以上は要観察）。"),
	});
	let closed = ai_events
		.iter()
		.filter(|row| {
			row["event"]["action"] == "close"
				|| matches!(
					row["event"].get("episode_phase").and_then(Value::as_str),
					Some("refractory" | "completed")
				)
		})
		.count();
	checks.push(count_check(
		"live_ai_episode_end",
		closed > 0,
		format!("会話終端または抑制期間への遷移 {closed}件。"),
	));
	let avatar_intent_requested = ai_events
		.iter()
		.filter_map(|row| row["event"].get("avatar_intent_requested_count").and_then(Value::as_u64))
		.sum::<u64>();
	let avatar_intent_emitted = ai_events
		.iter()
		.filter_map(|row| row["event"].get("avatar_intent_emitted_count").and_then(Value::as_u64))
		.sum::<u64>();
	let avatar_pose_requested = ai_events
		.iter()
		.filter(|row| row["event"].get("avatar_pose_requested").and_then(Value::as_bool) == Some(true))
		.count() as u64;
	let avatar_pose_emitted = ai_events
		.iter()
		.filter(|row| row["event"].get("avatar_pose_emitted").and_then(Value::as_bool) == Some(true))
		.count() as u64;
	let avatar_errors = ai_events
		.iter()
		.filter(|row| {
			["avatar_intent_error", "avatar_pose_error"].iter().any(|field| {
				row["event"]
					.get(*field)
					.and_then(Value::as_str)
					.is_some_and(|error| !error.trim().is_empty())
			})
		})
		.count();
	let avatar_failed =
		avatar_intent_requested.saturating_sub(avatar_intent_emitted) + avatar_pose_requested.saturating_sub(avatar_pose_emitted);
	checks.push(count_check(
		"live_ai_avatar_action",
		avatar_intent_emitted + avatar_pose_emitted > 0 && avatar_failed == 0 && avatar_errors == 0,
		format!(
			"AI身体表現: AvatarIntent 要求 {avatar_intent_requested} / 発行 {avatar_intent_emitted}、named pose 要求 {avatar_pose_requested} / 発行 {avatar_pose_emitted}、失敗要求 {avatar_failed}、validation error {avatar_errors}件。"
		),
	));
	let twitch_chat = count_kind("twitch_chat_received");
	checks.push(count_check(
		"live_twitch_chat",
		twitch_chat > 0,
		format!("Twitch実コメント受信 {twitch_chat}件。"),
	));
	let missing_channels = expected_channels
		.iter()
		.filter(|channel| {
			!window
				.iter()
				.any(|row| row["event"]["kind"] == "channel_datum" && row["event"]["channel"] == channel.as_str())
		})
		.cloned()
		.collect::<Vec<_>>();
	checks.push(count_check(
		"live_subtitles",
		missing_channels.is_empty(),
		if missing_channels.is_empty() {
			format!("字幕channel {} の到達を確認。", expected_channels.join(", "))
		} else {
			format!("字幕channel {} の到達がない。", missing_channels.join(", "))
		},
	));
	let mut missing_tts = Vec::new();
	let mut failed_tts = Vec::new();
	for node in expected_tts_nodes {
		let completed = count_tts_phase(&window, node, "completed");
		let failed = count_tts_phase(&window, node, "failed");
		if completed == 0 {
			missing_tts.push(node.clone());
		}
		if failed > 0 {
			failed_tts.push(format!("{node}={failed}"));
		}
	}
	checks.push(count_check(
		"live_tts",
		missing_tts.is_empty() && failed_tts.is_empty(),
		format!(
			"TTS完了必須node: {}。未完了: {}。失敗: {}。",
			expected_tts_nodes.join(", "),
			if missing_tts.is_empty() {
				"なし".into()
			} else {
				missing_tts.join(", ")
			},
			if failed_tts.is_empty() {
				"なし".into()
			} else {
				failed_tts.join(", ")
			}
		),
	));
	let overlaps = tts_playback_overlaps(&window);
	checks.push(count_check(
		"live_tts_overlap",
		overlaps.is_empty(),
		if overlaps.is_empty() {
			"TTS playback_startedからcompletedまでの音声重複なし。".into()
		} else {
			format!("同時再生を{}件検出: {}。", overlaps.len(), overlaps.join(", "))
		},
	));
	checks
}

fn count_check(id: &str, ready: bool, detail: String) -> Check {
	Check {
		id: id.into(),
		status: if ready { CheckStatus::Ready } else { CheckStatus::Blocked },
		detail,
	}
}

fn count_event_field(events: &[&Value], kind: &str, field: &str, expected: &str) -> usize {
	events
		.iter()
		.filter(|row| row["event"]["kind"] == kind && row["event"][field] == expected)
		.count()
}

fn count_tts_phase(events: &[&Value], node_id: &str, phase: &str) -> usize {
	events
		.iter()
		.filter(|row| row["event"]["kind"] == "tts_lifecycle" && row["event"]["node_id"] == node_id && row["event"]["phase"] == phase)
		.count()
}

fn ai_cadence(events: &[&Value]) -> (usize, usize, usize) {
	let mut spoken = 0;
	let mut silent = 0;
	let mut consecutive = 0;
	let mut max_consecutive = 0;
	for row in events {
		let has_speech = row["event"]
			.get("speak")
			.and_then(Value::as_str)
			.is_some_and(|speak| !speak.trim().is_empty());
		if has_speech {
			spoken += 1;
			consecutive += 1;
			max_consecutive = max_consecutive.max(consecutive);
		} else {
			silent += 1;
			consecutive = 0;
		}
	}
	(spoken, silent, max_consecutive)
}

fn tts_playback_overlaps(events: &[&Value]) -> Vec<String> {
	let mut active: Option<String> = None;
	let mut overlaps = Vec::new();
	for row in events {
		let event = &row["event"];
		if event["kind"] != "tts_lifecycle" {
			continue;
		}
		let utterance = event.get("utterance_id").and_then(Value::as_str).unwrap_or("unknown");
		match event.get("phase").and_then(Value::as_str) {
			Some("playback_started") => {
				if let Some(previous) = active.as_deref().filter(|previous| *previous != utterance) {
					overlaps.push(format!("{previous}->{utterance}"));
				}
				active = Some(utterance.to_string());
			}
			Some("completed" | "failed") if active.as_deref() == Some(utterance) => active = None,
			_ => {}
		}
	}
	if let Some(utterance) = active {
		overlaps.push(format!("{utterance}->未完了"));
	}
	overlaps
}

fn check_reward_route(history: &Value, expected_id: Option<&str>, expected_title: Option<&str>, expected_channels: &[String]) -> Check {
	let Some(events) = history.get("events").and_then(Value::as_array) else {
		return Check {
			id: "twitch_reward_route".into(),
			status: CheckStatus::Blocked,
			detail: "Control Event履歴にevents配列がない。".into(),
		};
	};
	let matched = events.iter().rev().find(|row| {
		let event = &row["event"];
		event["kind"] == "twitch_event_sub_received"
			&& event["event_type"] == "channel.channel_points_custom_reward_redemption.add"
			&& expected_id.is_none_or(|id| event["reward_id"] == id)
			&& expected_title.is_none_or(|title| event["reward_title"] == title)
	});
	let Some(received) = matched else {
		let identity = expected_id.or(expected_title).unwrap_or("(unspecified)");
		return Check {
			id: "twitch_reward_route".into(),
			status: CheckStatus::Blocked,
			detail: format!(
				"保持中のControl Event {}件に対象チャネルポイント交換 {identity} のEventSub実受信がない。",
				events.len()
			),
		};
	};
	let received_event = &received["event"];
	let reward_id = received_event.get("reward_id").and_then(Value::as_str).or(expected_id);
	let reward_title = received_event.get("reward_title").and_then(Value::as_str).or(expected_title);
	let reached = expected_channels
		.iter()
		.filter(|channel| {
			events.iter().any(|row| {
				let event = &row["event"];
				if event["kind"] != "channel_datum" || event["channel"] != channel.as_str() {
					return false;
				}
				let meta = &event["diagnostic_meta"];
				reward_id
					.map(|id| meta["reward_id"] == id)
					.unwrap_or_else(|| reward_title.is_some_and(|title| meta["reward_title"] == title))
			})
		})
		.cloned()
		.collect::<Vec<_>>();
	let missing = expected_channels
		.iter()
		.filter(|channel| !reached.contains(channel))
		.cloned()
		.collect::<Vec<_>>();
	let observed_at = received_event.get("observed_at").and_then(Value::as_str).unwrap_or("時刻不明");
	if missing.is_empty() {
		Check {
			id: "twitch_reward_route".into(),
			status: CheckStatus::Ready,
			detail: format!("EventSub実受信（{observed_at}）と同一報酬の到達を {} で確認。", reached.join(", ")),
		}
	} else {
		Check {
			id: "twitch_reward_route".into(),
			status: CheckStatus::Blocked,
			detail: format!(
				"EventSub実受信（{observed_at}）はあるが、同一報酬が {} に到達していない。到達済み: {}。",
				missing.join(", "),
				if reached.is_empty() { "なし".into() } else { reached.join(", ") }
			),
		}
	}
}

fn snapshot_checks(snapshot: &Value, allow_non_streaming: bool) -> Vec<Check> {
	let Some(readiness) = snapshot.get("readiness") else {
		return vec![Check {
			id: "snapshot".into(),
			status: CheckStatus::Blocked,
			detail: "Control API snapshotにreadinessがない。".into(),
		}];
	};
	let mode = readiness.get("mode").and_then(Value::as_str).unwrap_or("unknown");
	let mut checks = vec![Check {
		id: "runtime_mode".into(),
		status: if mode == "streaming" || allow_non_streaming {
			CheckStatus::Ready
		} else {
			CheckStatus::Blocked
		},
		detail: format!("現在モード: {mode}"),
	}];
	if let Some(items) = readiness.get("checks").and_then(Value::as_array) {
		for item in items {
			let required = item.get("required").and_then(Value::as_bool).unwrap_or(false);
			let id = item.get("id").and_then(Value::as_str).unwrap_or("unknown");
			if !required && !matches!(id, "twitch_chat" | "subtitle" | "avatar_motion") {
				continue;
			}
			checks.push(Check {
				id: format!("runtime_{id}"),
				status: parse_status(item.get("status").and_then(Value::as_str)),
				detail: item.get("detail").and_then(Value::as_str).unwrap_or("詳細なし").to_string(),
			});
		}
	}
	checks
}

fn check_obs_template(options: &Options, channel: &str) -> Check {
	let uri = format!(
		"{}/obs-template/subtitle?channel={channel}&theme=minimal&poll_ms=200&limit=1",
		options.base_url
	);
	match get_text(&uri, &options.token) {
		Ok(body) if subtitle_template_matches(&body, channel) => Check {
			id: format!("obs_template_{channel}"),
			status: CheckStatus::Ready,
			detail: format!("字幕テンプレートがchannel={channel}で配信されている。"),
		},
		Ok(_) => Check {
			id: format!("obs_template_{channel}"),
			status: CheckStatus::Blocked,
			detail: "字幕テンプレートの内容が期待する契約と一致しない。".into(),
		},
		Err(error) => Check {
			id: format!("obs_template_{channel}"),
			status: CheckStatus::Blocked,
			detail: error.to_string(),
		},
	}
}

fn subtitle_template_matches(body: &str, channel: &str) -> bool {
	body.contains("UNVAC Subtitle") && body.contains(&format!("channels: [\"{channel}\"]"))
}

fn check_output_channels(options: &Options, channels: &[String]) -> (Check, Value) {
	let body = json!({
		"channels": channels.iter().map(|channel| json!({"name": channel, "count": 1})).collect::<Vec<_>>()
	});
	match post_json(&format!("{}/output", options.base_url), &options.token, &body) {
		Ok(response) => (classify_output_channels(&response, channels), response),
		Err(error) => {
			let detail = error.to_string();
			(
				Check {
					id: "subtitle_output".into(),
					status: CheckStatus::Blocked,
					detail: detail.clone(),
				},
				json!({"error": detail}),
			)
		}
	}
}

fn redact_runtime_text(mut value: Value) -> Value {
	redact_runtime_text_in_place(&mut value);
	value
}

fn redact_runtime_text_in_place(value: &mut Value) {
	match value {
		Value::Object(object) => {
			for key in [
				"password",
				"token",
				"access_token",
				"refresh_token",
				"client_secret",
				"authorization",
			] {
				if object.contains_key(key) {
					object.insert(key.into(), Value::String("<redacted:secret>".into()));
				}
			}
			for key in ["content", "text", "speak", "episode_memory"] {
				if let Some(field) = object.get_mut(key) {
					if let Some(text) = field.as_str() {
						let chars = text.chars().count();
						*field = Value::String(format!("<redacted:{chars} chars>"));
					}
				}
			}
			for field in object.values_mut() {
				redact_runtime_text_in_place(field);
			}
		}
		Value::Array(items) => {
			for item in items {
				redact_runtime_text_in_place(item);
			}
		}
		_ => {}
	}
}

fn write_evidence(path: &Path, evidence: &Evidence<'_>, force: bool) -> Result<()> {
	if path.as_os_str().is_empty() {
		return Err("--evidence-out must not be empty".into());
	}
	if path.exists() && !force {
		return Err(format!("refusing to overwrite {}; pass --force", path.display()).into());
	}
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}
	fs::write(path, serde_json::to_vec_pretty(evidence)?)?;
	Ok(())
}

fn classify_output_channels(response: &Value, channels: &[String]) -> Check {
	let Some(output) = response.get("channel_data").and_then(Value::as_object) else {
		return Check {
			id: "subtitle_output".into(),
			status: CheckStatus::Blocked,
			detail: "/output応答にchannel_dataがない。".into(),
		};
	};
	let missing = channels
		.iter()
		.filter(|channel| !output.contains_key(channel.as_str()))
		.cloned()
		.collect::<Vec<_>>();
	if !missing.is_empty() {
		return Check {
			id: "subtitle_output".into(),
			status: CheckStatus::Blocked,
			detail: format!("/outputに字幕channel {}がない。", missing.join(", ")),
		};
	}
	let empty = channels
		.iter()
		.filter(|channel| {
			output
				.get(channel.as_str())
				.and_then(Value::as_array)
				.is_none_or(|items| items.is_empty())
		})
		.cloned()
		.collect::<Vec<_>>();
	if !empty.is_empty() {
		return Check {
			id: "subtitle_output".into(),
			status: CheckStatus::Attention,
			detail: format!("字幕channel {}は存在するが、起動後のデータがない。", empty.join(", ")),
		};
	}
	Check {
		id: "subtitle_output".into(),
		status: CheckStatus::Ready,
		detail: format!("/outputで字幕channel {}のデータ到達を確認。", channels.join(", ")),
	}
}

fn parse_status(status: Option<&str>) -> CheckStatus {
	match status {
		Some("ready") | Some("inactive") => CheckStatus::Ready,
		Some("attention") => CheckStatus::Attention,
		_ => CheckStatus::Blocked,
	}
}

fn report_status(checks: &[Check]) -> CheckStatus {
	if checks.iter().any(|check| check.status == CheckStatus::Blocked) {
		CheckStatus::Blocked
	} else if checks.iter().any(|check| check.status == CheckStatus::Attention) {
		CheckStatus::Attention
	} else {
		CheckStatus::Ready
	}
}

fn get_json(uri: &str, token: &str) -> Result<Value> {
	let response = request_with_token(ureq::get(uri), token)
		.call()
		.map_err(|error| format!("GET {uri}: {error}"))?;
	read_json(response)
}

fn get_text(uri: &str, token: &str) -> Result<String> {
	let response = request_with_token(ureq::get(uri), token)
		.call()
		.map_err(|error| format!("GET {uri}: {error}"))?;
	read_text(response)
}

fn post_json(uri: &str, token: &str, body: &Value) -> Result<Value> {
	let request = request_with_token(ureq::post(uri).header("Content-Type", "application/json; charset=utf-8"), token);
	let response = request
		.send(serde_json::to_vec(body)?)
		.map_err(|error| format!("POST {uri}: {error}"))?;
	read_json(response)
}

fn request_with_token<T>(request: ureq::RequestBuilder<T>, token: &str) -> ureq::RequestBuilder<T> {
	if token.trim().is_empty() {
		request
	} else {
		request.header("Authorization", &format!("Bearer {token}"))
	}
}

fn read_json(response: ureq::http::Response<ureq::Body>) -> Result<Value> {
	Ok(serde_json::from_str(&read_text(response)?)?)
}

fn read_text(response: ureq::http::Response<ureq::Body>) -> Result<String> {
	let mut reader = response.into_body().into_reader();
	let mut text = String::new();
	reader.read_to_string(&mut text)?;
	Ok(text)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn streaming_snapshot_keeps_optional_avatar_attention() {
		let checks = snapshot_checks(
			&json!({
				"readiness": {
					"mode": "streaming",
					"checks": [
						{"id":"voice","status":"ready","required":true,"detail":"ok"},
						{"id":"twitch_chat","status":"ready","required":true,"detail":"joined"},
						{"id":"avatar_motion","status":"attention","required":false,"detail":"visual pending"}
					]
				}
			}),
			false,
		);
		assert_eq!(report_status(&checks), CheckStatus::Attention);
		assert!(checks.iter().any(|check| check.id == "runtime_twitch_chat"));
	}

	#[test]
	fn daily_mode_blocks_default_stream_check() {
		let checks = snapshot_checks(&json!({"readiness":{"mode":"daily","checks":[]}}), false);
		assert_eq!(report_status(&checks), CheckStatus::Blocked);
	}

	#[test]
	fn empty_output_channels_are_attention() {
		let channels = vec!["user".into(), "ai-synth".into()];
		let check = classify_output_channels(&json!({"channel_data":{"user":[],"ai-synth":[]}}), &channels);
		assert_eq!(check.status, CheckStatus::Attention);
		assert!(check.detail.contains("user, ai-synth"));
	}

	#[test]
	fn populated_output_channels_are_ready() {
		let channels = vec!["user".into(), "ai-synth".into()];
		let check = classify_output_channels(
			&json!({"channel_data":{"user":[{"content":"hello"}],"ai-synth":[{"content":"response"}]}}),
			&channels,
		);
		assert_eq!(check.status, CheckStatus::Ready);
	}

	#[test]
	fn subtitle_template_matches_current_config_contract() {
		let body = r#"<title>UNVAC Subtitle</title><script>const CONFIG = { channels: ["user"], pollMs: 200 };</script>"#;
		assert!(subtitle_template_matches(body, "user"));
		assert!(!subtitle_template_matches(body, "ai-synth"));
	}

	#[test]
	fn evidence_options_are_explicit_and_force_is_opt_in() {
		let options = parse_options(&[
			"--evidence-out".into(),
			"target/stream-evidence/preflight.json".into(),
			"--force".into(),
		])
		.unwrap();
		assert_eq!(
			options.evidence_out.as_deref(),
			Some(Path::new("target/stream-evidence/preflight.json"))
		);
		assert!(options.force);
	}

	#[test]
	fn reward_options_default_to_external_bos_destination() {
		let options = parse_options(&[
			"--expect-reward-id".into(),
			"reward-123".into(),
			"--expect-reward-title".into(),
			"ケルシー先生へ".into(),
		])
		.unwrap();
		assert_eq!(options.expected_reward_id.as_deref(), Some("reward-123"));
		assert_eq!(options.expected_reward_title.as_deref(), Some("ケルシー先生へ"));
		assert_eq!(options.expected_reward_channels, ["effect"]);
	}

	#[test]
	fn live_session_options_use_real_profile_defaults() {
		let options = parse_options(&[
			"--expect-live-from".into(),
			"target/stream-evidence/preflight.json".into(),
			"--expect-live-minutes".into(),
			"12".into(),
		])
		.unwrap();
		assert_eq!(
			options.live_session_from.as_deref(),
			Some(Path::new("target/stream-evidence/preflight.json"))
		);
		assert_eq!(options.expected_live_minutes, 12);
		assert_eq!(
			options.expected_tts_nodes,
			["assistant-output/main::speak", "twitch-chat/main::speak"]
		);
		assert!(parse_options(&["--expect-live-minutes".into(), "10".into()]).is_err());
		assert!(parse_options(&["--expect-tts-nodes".into(), "speak".into()]).is_err());
	}

	fn live_event(at: &str, event: Value) -> Value {
		json!({"at": at, "event": event})
	}

	fn complete_live_history() -> Value {
		json!({"events": [
			live_event("2026-08-03T00:00:10Z", json!({"kind":"voice_recognition","phase":"partial"})),
			live_event("2026-08-03T00:00:11Z", json!({"kind":"voice_recognition","phase":"final"})),
			live_event("2026-08-03T00:00:12Z", json!({
				"kind":"ai_turn_decision",
				"action":"respond",
				"speak":"了解した",
				"avatar_intent_requested_count":1,
				"avatar_intent_emitted_count":1,
				"avatar_pose_requested":true,
				"avatar_pose_emitted":true
			})),
			live_event("2026-08-03T00:00:12.500Z", json!({"kind":"ai_turn_decision","action":"close","speak":null,"episode_phase":"refractory"})),
			live_event("2026-08-03T00:00:13Z", json!({"kind":"twitch_chat_received"})),
			live_event("2026-08-03T00:00:14Z", json!({"kind":"channel_datum","channel":"user"})),
			live_event("2026-08-03T00:00:15Z", json!({"kind":"channel_datum","channel":"ai-synth"})),
			live_event("2026-08-03T00:00:16Z", json!({"kind":"tts_lifecycle","phase":"playback_started","node_id":"assistant-output/main::speak","utterance_id":"ai-1"})),
			live_event("2026-08-03T00:00:20Z", json!({"kind":"tts_lifecycle","phase":"completed","node_id":"assistant-output/main::speak","utterance_id":"ai-1"})),
			live_event("2026-08-03T00:00:21Z", json!({"kind":"tts_lifecycle","phase":"playback_started","node_id":"twitch-chat/main::speak","utterance_id":"chat-1"})),
			live_event("2026-08-03T00:00:24Z", json!({"kind":"tts_lifecycle","phase":"completed","node_id":"twitch-chat/main::speak","utterance_id":"chat-1"}))
		]})
	}

	#[test]
	fn live_session_requires_each_real_runtime_path() {
		let checks = check_live_session(
			&complete_live_history(),
			"2026-08-03T00:00:00Z".parse().unwrap(),
			"2026-08-03T00:11:00Z".parse().unwrap(),
			10,
			&["user".into(), "ai-synth".into()],
			&["assistant-output/main::speak".into(), "twitch-chat/main::speak".into()],
		);
		assert_eq!(report_status(&checks), CheckStatus::Ready, "{checks:#?}");

		let mut missing_chat = complete_live_history();
		missing_chat["events"]
			.as_array_mut()
			.unwrap()
			.retain(|row| row["event"]["kind"] != "twitch_chat_received");
		let checks = check_live_session(
			&missing_chat,
			"2026-08-03T00:00:00Z".parse().unwrap(),
			"2026-08-03T00:11:00Z".parse().unwrap(),
			10,
			&["user".into(), "ai-synth".into()],
			&["assistant-output/main::speak".into(), "twitch-chat/main::speak".into()],
		);
		assert_eq!(
			checks.iter().find(|check| check.id == "live_twitch_chat").unwrap().status,
			CheckStatus::Blocked
		);
	}

	#[test]
	fn live_session_detects_tts_overlap_and_unfinished_playback() {
		let events = vec![
			live_event(
				"2026-08-03T00:00:01Z",
				json!({"kind":"tts_lifecycle","phase":"playback_started","utterance_id":"ai-1"}),
			),
			live_event(
				"2026-08-03T00:00:02Z",
				json!({"kind":"tts_lifecycle","phase":"playback_started","utterance_id":"chat-1"}),
			),
		];
		let refs = events.iter().collect::<Vec<_>>();
		let overlaps = tts_playback_overlaps(&refs);
		assert_eq!(overlaps, ["ai-1->chat-1", "chat-1->未完了"]);
	}

	#[test]
	fn live_session_cadence_counts_speech_runs_and_silence() {
		let events = (0..4)
			.map(|index| {
				live_event(
					"2026-08-03T00:00:01Z",
					json!({"kind":"ai_turn_decision","speak":format!("発話{index}")}),
				)
			})
			.chain(std::iter::once(live_event(
				"2026-08-03T00:00:02Z",
				json!({"kind":"ai_turn_decision","speak":null}),
			)))
			.collect::<Vec<_>>();
		let refs = events.iter().collect::<Vec<_>>();
		assert_eq!(ai_cadence(&refs), (4, 1, 4));
	}

	#[test]
	fn live_session_requires_successful_ai_avatar_action() {
		let mut history = complete_live_history();
		let decision = history["events"]
			.as_array_mut()
			.unwrap()
			.iter_mut()
			.find(|row| row["event"]["kind"] == "ai_turn_decision" && row["event"]["action"] == "respond")
			.unwrap();
		decision["event"]["avatar_intent_emitted_count"] = json!(0);
		decision["event"]["avatar_pose_emitted"] = json!(false);
		decision["event"]["avatar_pose_error"] = json!("preset is not allowed");

		let checks = check_live_session(
			&history,
			"2026-08-03T00:00:00Z".parse().unwrap(),
			"2026-08-03T00:11:00Z".parse().unwrap(),
			10,
			&["user".into(), "ai-synth".into()],
			&["assistant-output/main::speak".into(), "twitch-chat/main::speak".into()],
		);
		let avatar = checks.iter().find(|check| check.id == "live_ai_avatar_action").unwrap();
		assert_eq!(avatar.status, CheckStatus::Blocked);
		assert!(avatar.detail.contains("失敗要求 2"));
		assert!(avatar.detail.contains("validation error 1件"));
	}

	fn reward_history(channels: &[&str]) -> Value {
		let mut events = vec![json!({
			"at": "2026-08-03T01:00:00Z",
			"event": {
				"kind": "twitch_event_sub_received",
				"event_type": "channel.channel_points_custom_reward_redemption.add",
				"reward_id": "reward-123",
				"reward_title": "ケルシー先生へ",
				"observed_at": "2026-08-03T01:00:00Z"
			}
		})];
		events.extend(channels.iter().map(|channel| {
			json!({
				"at": "2026-08-03T01:00:00.100Z",
				"event": {
					"kind": "channel_datum",
					"channel": channel,
					"diagnostic_meta": {"reward_id": "reward-123"}
				}
			})
		}));
		json!({"events": events})
	}

	#[test]
	fn reward_route_requires_eventsub_and_every_expected_channel() {
		let channels = vec!["user".into(), "avatar_intent".into(), "effect".into()];
		let ready = check_reward_route(
			&reward_history(&["user", "avatar_intent", "effect"]),
			Some("reward-123"),
			Some("ケルシー先生へ"),
			&channels,
		);
		assert_eq!(ready.status, CheckStatus::Ready);
		assert!(ready.detail.contains("user, avatar_intent, effect"));

		let blocked = check_reward_route(
			&reward_history(&["user", "effect"]),
			Some("reward-123"),
			Some("ケルシー先生へ"),
			&channels,
		);
		assert_eq!(blocked.status, CheckStatus::Blocked);
		assert!(blocked.detail.contains("avatar_intent"));
	}

	#[test]
	fn reward_route_ignores_unrelated_reward_metadata() {
		let history = json!({"events": [
			{
				"event": {
					"kind": "twitch_event_sub_received",
					"event_type": "channel.channel_points_custom_reward_redemption.add",
					"reward_id": "reward-123",
					"observed_at": "2026-08-03T01:00:00Z"
				}
			},
			{
				"event": {
					"kind": "channel_datum",
					"channel": "effect",
					"diagnostic_meta": {"reward_id": "different-reward"}
				}
			}
		]});
		let check = check_reward_route(&history, Some("reward-123"), None, &["effect".into()]);
		assert_eq!(check.status, CheckStatus::Blocked);
		assert!(check.detail.contains("effect"));
	}

	#[test]
	fn evidence_redacts_transcripts_but_keeps_diagnostic_identifiers() {
		let redacted = redact_runtime_text(json!({
			"token": "must-not-leak",
			"events": [{
				"event": {
					"kind": "twitch_event_sub_received",
					"reward_id": "reward-123",
					"reward_title": "ケルシー先生へ",
					"content": "読み上げてほしくない本文",
					"episode_memory": "会話の内部要約"
				}
			}]
		}));
		let rendered = serde_json::to_string(&redacted).unwrap();
		assert!(rendered.contains("reward-123"));
		assert!(rendered.contains("ケルシー先生へ"));
		assert!(!rendered.contains("must-not-leak"));
		assert!(rendered.contains("redacted:secret"));
		assert!(!rendered.contains("読み上げてほしくない本文"));
		assert!(!rendered.contains("会話の内部要約"));
		assert!(rendered.contains("redacted"));
	}

	#[test]
	fn live_evidence_rejects_other_or_unidentified_endpoint() {
		let path = std::env::temp_dir().join(format!("unvac-stream-origin-{}.json", std::process::id()));
		let original = json!({"recorded_at":"2026-11-05T00:00:00Z", "report":{"base_url":"http://127.0.0.1:46000/"}});
		fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
		assert!(read_evidence_recorded_at(&path, "http://127.0.0.1:46000").is_ok());
		assert!(read_evidence_recorded_at(&path, "http://127.0.0.1:46001").is_err());
		assert!(read_evidence_recorded_at(&path, "http://other-host:46000").is_err());
		assert_eq!(serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap(), original);
		fs::write(&path, br#"{"recorded_at":"2026-11-05T00:00:00Z"}"#).unwrap();
		assert!(read_evidence_recorded_at(&path, "http://127.0.0.1:46000").is_err());
		fs::remove_file(path).unwrap();
	}

	#[test]
	fn evidence_writer_refuses_implicit_overwrite() {
		let path = std::env::temp_dir().join(format!("unvac-stream-evidence-{}.json", std::process::id()));
		fs::write(&path, b"existing").unwrap();
		let report = Report {
			status: CheckStatus::Ready,
			base_url: "http://127.0.0.1:46000".into(),
			checks: Vec::new(),
		};
		let evidence = Evidence {
			schema_version: 1,
			recorded_at: "2026-08-03T00:00:00Z".into(),
			report: &report,
			snapshot: json!({}),
			subtitle_output: json!({}),
			event_history: json!({}),
			quick_effects: json!({}),
		};
		assert!(write_evidence(&path, &evidence, false).is_err());
		assert_eq!(fs::read(&path).unwrap(), b"existing");
		fs::remove_file(path).unwrap();
	}
}

use super::{ReadinessCheck, ReadinessStatus, ReadinessSummary, VoiceWorkerStatusSummary};
use crate::control_events::TtsLifecyclePhase;

#[derive(Clone)]
pub(super) struct TtsRuntimeEvidence {
	pub node_id: String,
	pub phase: TtsLifecyclePhase,
	pub engine: String,
	pub played: Option<bool>,
	pub elapsed_ms: u64,
	pub error: Option<String>,
}

pub(super) struct ReadinessFacts {
	pub mode: Option<String>,
	pub flowgraph_ok: bool,
	pub flowgraph_error_count: usize,
	pub ai_total: usize,
	pub ai_active: usize,
	pub ai_errors: Vec<crate::runtime_health::RuntimeIssue>,
	pub voice_nodes: usize,
	pub voice_workers: usize,
	pub voice_worker_statuses: Vec<VoiceWorkerStatusSummary>,
	pub voice_muted: bool,
	pub tts_node_ids: Vec<String>,
	pub tts_runtime: Vec<TtsRuntimeEvidence>,
	pub bos_output_nodes: usize,
	pub output_channels_seen: Vec<String>,
	pub twitch_irc_nodes: usize,
	pub twitch_irc_unconfigured: usize,
	pub twitch_irc_workers: usize,
	pub twitch_irc_ready_workers: usize,
	pub twitch_irc_joined_channels: usize,
	pub twitch_irc_expected_channels: usize,
	pub twitch_message_received: bool,
	pub twitch_eventsub_nodes: usize,
	pub twitch_eventsub_setup_missing: bool,
	pub twitch_eventsub_workers: usize,
	pub twitch_eventsub_error: Option<String>,
	pub twitch_eventsub_event_received: bool,
	pub avatar_motion_nodes: usize,
	pub avatar_motion_probe: Option<crate::avatar_renderer_probe::AvatarRendererProbeResult>,
}

fn tts_phase_label(phase: TtsLifecyclePhase) -> &'static str {
	match phase {
		TtsLifecyclePhase::Started => "合成開始",
		TtsLifecyclePhase::Queued => "再生待ち",
		TtsLifecyclePhase::Synthesized => "合成完了",
		TtsLifecyclePhase::PlaybackStarted => "再生開始",
		TtsLifecyclePhase::Completed => "再生完了",
		TtsLifecyclePhase::Dropped => "鮮度超過で破棄",
		TtsLifecyclePhase::Failed => "失敗",
	}
}

fn short_detail(value: Option<&str>) -> String {
	let value = value.unwrap_or("詳細なし");
	if value.chars().count() <= 160 {
		return value.to_string();
	}
	value.chars().take(159).collect::<String>() + "…"
}

fn tts_failure_detail(evidence: &TtsRuntimeEvidence) -> String {
	let engine = evidence.engine.trim();
	let guidance = match engine {
		"coeiroink" => "連携画面でCOEIROINKを起動し、50032番ポートの待受を確認する。",
		"voicevox" => "VOICEVOX ENGINEが起動しているか確認する。",
		"aivis_speech" => "AivisSpeech Engineが起動しているか確認する。",
		"bouyomichan" => "棒読みちゃんが起動し、TCP連携を受け付けているか確認する。",
		"voicepeak" => "VOICEPEAKの実行パスと起動権限を確認する。",
		_ => "音声エンジンの起動状態と接続先を確認する。",
	};
	format!(
		"{} の{}処理に失敗: {} 次の操作: {}",
		evidence.node_id,
		engine,
		short_detail(evidence.error.as_deref()),
		guidance
	)
}

fn check(
	id: &str,
	area: &str,
	status: ReadinessStatus,
	title: &str,
	detail: impl Into<String>,
	required: bool,
	action: Option<&str>,
) -> ReadinessCheck {
	ReadinessCheck {
		id: id.into(),
		area: area.into(),
		status,
		title: title.into(),
		detail: detail.into(),
		required,
		action: action.map(str::to_string),
	}
}

pub(super) fn build_readiness(facts: ReadinessFacts) -> ReadinessSummary {
	let streaming = facts.mode.as_deref() == Some("streaming");
	let mut checks = Vec::new();

	checks.push(if facts.flowgraph_ok && facts.flowgraph_error_count == 0 {
		check(
			"flowgraph",
			"system",
			ReadinessStatus::Ready,
			"自動化",
			"Flowgraphは正常に読み込まれている。",
			true,
			Some("flowgraph"),
		)
	} else {
		check(
			"flowgraph",
			"system",
			ReadinessStatus::Blocked,
			"自動化",
			format!("Flowgraphに{}件のエラーがある。", facts.flowgraph_error_count.max(1)),
			true,
			Some("flowgraph"),
		)
	});

	for issue in &facts.ai_errors {
        checks.push(check(&issue.key, "conversation", ReadinessStatus::Attention,
            "AI共演者の応答", format!("{} {}", issue.message, issue.resolution.as_deref().unwrap_or("")),
            true, Some("observability")));
    }
	checks.push(match (facts.ai_total, facts.ai_active) {
		(0, _) => check(
			"ai",
			"conversation",
			ReadinessStatus::Inactive,
			"AI共演者",
			"未使用。AIなしでも配信支援・自動化を利用できる。",
			false,
			Some("resources"),
		),
		(_, 0) => check(
			"ai",
			"conversation",
			ReadinessStatus::Inactive,
			"AI共演者",
			"全AI共演者が一時停止中。",
			false,
			Some("live"),
		),
		(total, active) => check(
			"ai",
			"conversation",
			ReadinessStatus::Ready,
			"AI共演者",
			format!("{active}/{total}人が有効。一時停止の状態を示す。入力・応答・音声再生の確認は別途必要。"),
			true,
			Some("live"),
		),
	});

	checks.push(if facts.voice_nodes == 0 {
		check(
			"voice",
			"conversation",
			ReadinessStatus::Inactive,
			"マイク音声認識",
			"未使用。マイク入力を使う場合に追加する。",
			false,
			Some("flowgraph"),
		)
	} else if facts.voice_workers < facts.voice_nodes {
		check(
			"voice",
			"conversation",
			ReadinessStatus::Blocked,
			"マイク音声認識",
			format!("{}経路中{}ワーカーだけが稼働している。", facts.voice_nodes, facts.voice_workers),
			true,
			Some("flowgraph"),
		)
	} else if facts.voice_muted {
		check(
			"voice",
			"conversation",
			ReadinessStatus::Inactive,
			"マイク音声認識",
			"ソフトミュート中。",
			false,
			Some("live"),
		)
	} else {
		let backend = facts
			.voice_worker_statuses
			.first()
			.map(|status| format!("{} / {}", status.engine, status.active_backend))
			.unwrap_or_else(|| "backend不明".into());
		check(
			"voice",
			"conversation",
			ReadinessStatus::Ready,
			"マイク音声認識",
			format!("{}ワーカーが稼働中（{}）。", facts.voice_workers, backend),
			true,
			Some("live"),
		)
	});

	checks.push(if facts.tts_node_ids.is_empty() {
		check(
			"tts",
			"conversation",
			ReadinessStatus::Inactive,
			"音声合成",
			"未使用。読み上げを使う場合に追加する。",
			false,
			Some("flowgraph"),
		)
	} else if let Some(evidence) = facts
		.tts_runtime
		.iter()
		.find(|evidence| evidence.phase == TtsLifecyclePhase::Failed)
	{
		check(
			"tts",
			"conversation",
			ReadinessStatus::Blocked,
			"音声合成",
			tts_failure_detail(evidence),
			true,
			Some("resources"),
		)
	} else {
		let completed = facts
			.tts_runtime
			.iter()
			.filter(|evidence| evidence.phase == TtsLifecyclePhase::Completed && evidence.played == Some(true))
			.count();
		if completed == facts.tts_node_ids.len() {
			let slowest = facts.tts_runtime.iter().map(|evidence| evidence.elapsed_ms).max().unwrap_or(0);
			check(
				"tts",
				"conversation",
				ReadinessStatus::Ready,
				"音声合成",
				format!(
					"{completed}/{}経路の実再生を確認済み（最長{}ms）。",
					facts.tts_node_ids.len(),
					slowest
				),
				true,
				Some("flowgraph"),
			)
		} else {
			let pending = facts
				.tts_runtime
				.iter()
				.find(|evidence| evidence.phase != TtsLifecyclePhase::Completed)
				.map(|evidence| format!(" {} は{}。", evidence.node_id, tts_phase_label(evidence.phase)))
				.unwrap_or_default();
			check(
				"tts",
				"conversation",
				ReadinessStatus::Ready,
				"音声合成",
				format!(
					"{}経路を構成済み。{completed}経路の実再生を確認済み。未使用の経路は初回利用時に確認する。{pending}",
					facts.tts_node_ids.len()
				),
				true,
				Some("live"),
			)
		}
	});

	checks.push(if facts.bos_output_nodes == 0 {
		check(
			"subtitle",
			"stream",
			ReadinessStatus::Inactive,
			"画面出力",
			"未使用。画面出力を使う場合に追加する。",
			false,
			Some("flowgraph"),
		)
	} else {
		let evidence = if facts.output_channels_seen.is_empty() {
			"出力履歴はまだない。初回利用時に確認する。".to_string()
		} else {
			format!("出力履歴のある宛先: {}。", facts.output_channels_seen.join(", "))
		};
		check(
			"subtitle",
			"stream",
			ReadinessStatus::Ready,
			"画面出力",
			format!(
				"{}個の出力経路を構成済み。{evidence} この履歴だけでは各経路の動作やOBSでの表示までは確認できない。",
				facts.bos_output_nodes
			),
			streaming,
			Some("live"),
		)
	});

	checks.push(if facts.twitch_eventsub_nodes == 0 {
		check(
			"twitch_events",
			"stream",
			ReadinessStatus::Inactive,
			"Twitch配信イベント",
			"EventSubを使う自動化は有効ではない。",
			false,
			Some("resources"),
		)
	} else if facts.twitch_eventsub_setup_missing {
        check("twitch_events", "stream", ReadinessStatus::Attention, "Twitch配信イベント",
            "Twitch接続を設定してください。「連携」→「Twitch接続」で配信者と投稿用アカウントを保存し、適用後に認証します。",
            streaming, Some("resources"))
	} else if facts.twitch_eventsub_workers < facts.twitch_eventsub_nodes {
		check(
			"twitch_events",
			"stream",
			if streaming {
				ReadinessStatus::Blocked
			} else {
				ReadinessStatus::Attention
			},
			"Twitch配信イベント",
			format!(
				"{}経路中{}ワーカーだけが稼働している。チャネポ交換などを受信できない。",
				facts.twitch_eventsub_nodes, facts.twitch_eventsub_workers
			),
			streaming,
			Some("resources"),
		)
	} else if let Some(error) = facts.twitch_eventsub_error.as_deref() {
		check(
			"twitch_events",
			"stream",
			if streaming {
				ReadinessStatus::Blocked
			} else {
				ReadinessStatus::Attention
			},
			"Twitch配信イベント",
			format!("EventSubへ接続できない: {}", short_detail(Some(error))),
			streaming,
			Some("resources"),
		)
	} else if !facts.twitch_eventsub_event_received {
		check(
			"twitch_events",
			"stream",
			ReadinessStatus::Ready,
			"Twitch配信イベント",
			format!(
				"{}経路のEventSub購読が稼働中。実イベントは発生時に受信を記録する。",
				facts.twitch_eventsub_workers
			),
			streaming,
			Some("resources"),
		)
	} else {
		check(
			"twitch_events",
			"stream",
			ReadinessStatus::Ready,
			"Twitch配信イベント",
			format!("{}経路のEventSub購読が稼働中。", facts.twitch_eventsub_workers),
			streaming,
			Some("resources"),
		)
	});

	checks.push(if facts.twitch_irc_nodes == 0 {
		check(
			"twitch_chat",
			"stream",
			ReadinessStatus::Inactive,
			"Twitchコメント",
			"未使用。チャット入力を使う場合に追加する。",
			false,
			Some("flowgraph"),
		)
	} else if facts.twitch_irc_unconfigured > 0 {
        check("twitch_chat", "stream", ReadinessStatus::Attention, "Twitchコメント",
            "読み取るTwitchチャンネルが未設定です。「連携」→「Twitch接続」で設定してください。個別の入力を使う場合はFlowgraphの対象ノードでチャンネルを指定できます。",
            streaming, Some("resources"))
	} else if facts.twitch_irc_workers < facts.twitch_irc_nodes {
		check(
			"twitch_chat",
			"stream",
			ReadinessStatus::Blocked,
			"Twitchコメント",
			format!(
				"{}経路中{}ワーカーだけが稼働している。",
				facts.twitch_irc_nodes, facts.twitch_irc_workers
			),
			streaming,
			Some("resources"),
		)
	} else if facts.twitch_irc_ready_workers < facts.twitch_irc_nodes {
		check(
			"twitch_chat",
			"stream",
			ReadinessStatus::Blocked,
			"Twitchコメント",
			format!(
				"チャンネル参加を確認中（{}/{}）。",
				facts.twitch_irc_joined_channels, facts.twitch_irc_expected_channels
			),
			streaming,
			Some("resources"),
		)
	} else if !facts.twitch_message_received {
		check(
			"twitch_chat",
			"stream",
			ReadinessStatus::Ready,
			"Twitchコメント",
			format!(
				"{}ワーカーが{}チャンネルへ参加済み。コメントを受信可能。",
				facts.twitch_irc_ready_workers, facts.twitch_irc_joined_channels
			),
			streaming,
			Some("resources"),
		)

	} else {
		check(
			"twitch_chat",
			"stream",
			ReadinessStatus::Ready,
			"Twitchコメント",
			format!(
				"{}チャンネルで実コメントを受信済み。",
				facts.twitch_irc_joined_channels
			),
			streaming,
			Some("resources"),
		)
	});

	checks.push(
		if facts.avatar_motion_nodes > 0 && facts.avatar_motion_probe.as_ref().is_some_and(|probe| probe.ready()) {
			let probe = facts.avatar_motion_probe.as_ref().expect("ready probe checked");
			check(
				"avatar_motion",
				"avatar",
				ReadinessStatus::Ready,
				"AIアバター動作",
				probe.detail(),
				false,
				Some("live"),
			)
		} else if facts.avatar_motion_nodes > 0 {
			let detail = facts.avatar_motion_probe.as_ref().map_or_else(
				|| {
					format!(
						"{}個の動作経路が構成済み。Rendererの状態を自動確認中。受信・反映は未確認。",
						facts.avatar_motion_nodes
					)
				},
				crate::avatar_renderer_probe::AvatarRendererProbeResult::detail,
			);
			check(
				"avatar_motion",
				"avatar",
				if facts.avatar_motion_probe.as_ref().is_some_and(|p| p.needs_attention()) { ReadinessStatus::Attention } else { ReadinessStatus::Unverified },
				"AIアバター動作",
				detail,
				false,
				Some("live"),
			)
		} else {
			check(
				"avatar_motion",
				"avatar",
				ReadinessStatus::Inactive,
				"AIアバター動作",
				"任意機能。会話とコメント読み上げには影響しない。",
				false,
				Some("flowgraph"),
			)
		},
	);

	let status = if checks.iter().any(|item| item.required && item.status == ReadinessStatus::Blocked) {
		ReadinessStatus::Blocked
	} else if checks.iter().any(|item| item.required && item.status == ReadinessStatus::Attention) {
		ReadinessStatus::Attention
	} else {
		ReadinessStatus::Ready
	};
	ReadinessSummary {
		status,
		mode: facts.mode,
		checks,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn completed_tts(node_id: &str) -> TtsRuntimeEvidence {
		TtsRuntimeEvidence {
			node_id: node_id.into(),
			phase: TtsLifecyclePhase::Completed,
			engine: "coeiroink".into(),
			played: Some(true),
			elapsed_ms: 120,
			error: None,
		}
	}

	fn ready_facts(mode: &str) -> ReadinessFacts {
		ReadinessFacts {
			mode: Some(mode.into()),
			flowgraph_ok: true,
			flowgraph_error_count: 0,
			ai_total: 1,
			ai_active: 1,
			ai_errors: vec![],
			voice_nodes: 1,
			voice_workers: 1,
			voice_worker_statuses: vec![VoiceWorkerStatusSummary {
				node_id: "voice::in".into(),
				engine: "whisper".into(),
				requested_backend: "auto".into(),
				active_backend: "vulkan".into(),
				..Default::default()
			}],
			voice_muted: false,
			tts_node_ids: vec!["ai::speak".into(), "chat::speak".into()],
			tts_runtime: vec![completed_tts("ai::speak"), completed_tts("chat::speak")],
			bos_output_nodes: 2,
			output_channels_seen: vec!["user".into(), "ai-synth".into()],
			twitch_irc_nodes: 1,
			twitch_irc_unconfigured: 0,
			twitch_irc_workers: 1,
			twitch_irc_ready_workers: 1,
			twitch_irc_joined_channels: 1,
			twitch_irc_expected_channels: 1,
			twitch_message_received: true,
			twitch_eventsub_nodes: 1,
			twitch_eventsub_setup_missing: false,
			twitch_eventsub_workers: 1,
			twitch_eventsub_error: None,
			twitch_eventsub_event_received: true,
			avatar_motion_nodes: 1,
			avatar_motion_probe: None,
		}
	}

	#[test]
	fn non_ai_automation_does_not_require_unused_features() {
		let mut facts = ready_facts("streaming");
		facts.ai_total = 0; facts.ai_active = 0;
		facts.voice_nodes = 0; facts.voice_workers = 0;
		facts.tts_node_ids.clear(); facts.tts_runtime.clear();
		facts.bos_output_nodes = 0;
		facts.twitch_irc_nodes = 0; facts.twitch_eventsub_nodes = 0;
		facts.avatar_motion_nodes = 0;
		let result = build_readiness(facts);
		assert_eq!(result.status, ReadinessStatus::Ready);
		assert!(result.checks.iter().filter(|c| c.id != "flowgraph").all(|c| !c.required && c.status == ReadinessStatus::Inactive));
	}
	#[test]
	fn chat_only_does_not_require_overlay_or_ai() {
		let mut facts = ready_facts("streaming");
		facts.ai_total = 0; facts.ai_active = 0;
		facts.voice_nodes = 0; facts.voice_workers = 0;
		facts.tts_node_ids.clear(); facts.tts_runtime.clear();
		facts.bos_output_nodes = 0;
		assert_eq!(build_readiness(facts).status, ReadinessStatus::Ready);
	}

	#[test]
	fn voice_readiness_reports_selected_runtime_backend() {
		let result = build_readiness(ready_facts("streaming"));
		let voice = result.checks.iter().find(|item| item.id == "voice").unwrap();
		assert_eq!(voice.status, ReadinessStatus::Ready);
		assert!(voice.detail.contains("whisper / vulkan"));
	}

	#[test]
	fn streaming_requires_twitch_worker() {
		let mut facts = ready_facts("streaming");
		facts.twitch_irc_workers = 0;
		facts.twitch_irc_ready_workers = 0;
		facts.twitch_irc_joined_channels = 0;
		let result = build_readiness(facts);
		assert_eq!(result.status, ReadinessStatus::Blocked);
		assert_eq!(
			result.checks.iter().find(|item| item.id == "twitch_chat").unwrap().status,
			ReadinessStatus::Blocked
		);
	}

	#[test]
	fn streaming_waits_for_twitch_room_join() {
		let mut facts = ready_facts("streaming");
		facts.twitch_irc_ready_workers = 0;
		facts.twitch_irc_joined_channels = 0;
		let result = build_readiness(facts);
		let twitch = result.checks.iter().find(|item| item.id == "twitch_chat").unwrap();
		assert_eq!(result.status, ReadinessStatus::Blocked);
		assert_eq!(twitch.status, ReadinessStatus::Blocked);
		assert!(twitch.detail.contains("0/1"));
	}

	#[test]
	fn joined_twitch_room_is_ready_before_the_first_message() {
		let mut facts = ready_facts("streaming");
		facts.twitch_message_received = false;
		let result = build_readiness(facts);
		let twitch = result.checks.iter().find(|item| item.id == "twitch_chat").unwrap();
		assert_eq!(result.status, ReadinessStatus::Ready);
		assert_eq!(twitch.status, ReadinessStatus::Ready);
		assert!(twitch.detail.contains("コメントを受信可能"));
	}

	#[test]
	fn received_chat_does_not_require_unrelated_overlay_output() {
		for channels in [vec![], vec!["raid-notice".into()], vec!["custom-chat".into()]] {
			let mut facts = ready_facts("streaming");
			facts.ai_total = 0;
			facts.ai_active = 0;
			facts.output_channels_seen = channels;
			let result = build_readiness(facts);
			let twitch = result.checks.iter().find(|item| item.id == "twitch_chat").unwrap();
			assert_eq!(result.status, ReadinessStatus::Ready);
			assert_eq!(twitch.status, ReadinessStatus::Ready);
			assert!(twitch.detail.contains("実コメントを受信済み"));
			assert!(!twitch.detail.contains("到達"));
		}
	}

	#[test]
	fn daily_does_not_require_twitch() {
		let mut facts = ready_facts("daily");
		facts.twitch_irc_nodes = 0;
		facts.twitch_irc_workers = 0;
		facts.twitch_irc_ready_workers = 0;
		facts.twitch_irc_joined_channels = 0;
		let result = build_readiness(facts);
		assert_eq!(result.status, ReadinessStatus::Ready);
		assert!(!result.checks.iter().find(|item| item.id == "twitch_chat").unwrap().required);
	}

	#[test]
	fn streaming_blocks_when_configured_eventsub_cannot_connect() {
		let mut facts = ready_facts("streaming");
		facts.twitch_eventsub_error = Some("subscription rejected".into());
		let result = build_readiness(facts);
		let events = result.checks.iter().find(|item| item.id == "twitch_events").unwrap();
		assert_eq!(result.status, ReadinessStatus::Blocked);
		assert_eq!(events.status, ReadinessStatus::Blocked);
		assert!(events.detail.contains("subscription rejected"));
	}

	#[test]
	fn connected_eventsub_is_ready_before_the_first_event() {
		let mut facts = ready_facts("streaming");
		facts.twitch_eventsub_event_received = false;
		let result = build_readiness(facts);
		let events = result.checks.iter().find(|item| item.id == "twitch_events").unwrap();
		assert_eq!(result.status, ReadinessStatus::Ready);
		assert_eq!(events.status, ReadinessStatus::Ready);
		assert!(events.detail.contains("実イベントは発生時に受信を記録"));
	}

	#[test]
	fn profiles_without_eventsub_are_not_blocked() {
		let mut facts = ready_facts("streaming");
		facts.twitch_eventsub_nodes = 0;
		facts.twitch_eventsub_workers = 0;
		let result = build_readiness(facts);
		let events = result.checks.iter().find(|item| item.id == "twitch_events").unwrap();
		assert_eq!(events.status, ReadinessStatus::Inactive);
		assert!(!events.required);
		assert_eq!(result.status, ReadinessStatus::Ready);
	}

	#[test]
	fn muted_voice_is_intentional_inactive_state() {
		let mut facts = ready_facts("streaming");
		facts.voice_muted = true;
		let result = build_readiness(facts);
		let voice = result.checks.iter().find(|item| item.id == "voice").unwrap();
		assert_eq!(result.status, ReadinessStatus::Ready);
		assert_eq!(voice.status, ReadinessStatus::Inactive);
		assert!(!voice.required);
	}

	#[test]
	fn multiple_voice_ingresses_are_supported_when_all_workers_run() {
		let mut facts = ready_facts("streaming");
		facts.voice_nodes = 2;
		facts.voice_workers = 2;
		let result = build_readiness(facts);
		let voice = result.checks.iter().find(|item| item.id == "voice").unwrap();
		assert_eq!(voice.status, ReadinessStatus::Ready);
	}

	#[test]
	fn configured_but_untested_tts_is_ready_for_first_use() {
		let mut facts = ready_facts("streaming");
		facts.tts_runtime.clear();
		let result = build_readiness(facts);
		let tts = result.checks.iter().find(|item| item.id == "tts").unwrap();
		assert_eq!(result.status, ReadinessStatus::Ready);
		assert_eq!(tts.status, ReadinessStatus::Ready);
		assert!(tts.detail.contains("2経路を構成済み"));
	}

	#[test]
	fn one_successful_tts_keeps_unused_route_ready_for_first_use() {
		let mut facts = ready_facts("streaming");
		facts.tts_runtime = vec![completed_tts("chat::speak")];
		let result = build_readiness(facts);
		let tts = result.checks.iter().find(|item| item.id == "tts").unwrap();
		assert_eq!(result.status, ReadinessStatus::Ready);
		assert_eq!(tts.status, ReadinessStatus::Ready);
		assert!(tts.detail.contains("1経路の実再生を確認済み"));
	}

	#[test]
	fn failed_tts_blocks_readiness() {
		let mut facts = ready_facts("streaming");
		facts.tts_runtime = vec![TtsRuntimeEvidence {
			node_id: "chat::speak".into(),
			phase: TtsLifecyclePhase::Failed,
			engine: "coeiroink".into(),
			played: None,
			elapsed_ms: 80,
			error: Some("connection refused".into()),
		}];
		let result = build_readiness(facts);
		let tts = result.checks.iter().find(|item| item.id == "tts").unwrap();
		assert_eq!(result.status, ReadinessStatus::Blocked);
		assert!(tts.detail.contains("connection refused"));
		assert!(tts.detail.contains("COEIROINKを起動"));
		assert_eq!(tts.action.as_deref(), Some("resources"));
	}

	#[test]
	fn configured_output_does_not_require_personal_caption_channels() {
		let mut facts = ready_facts("streaming");
		facts.ai_total = 0; facts.ai_active = 0;
		facts.output_channels_seen.clear();
		let result = build_readiness(facts);
		let output = result.checks.iter().find(|item| item.id == "subtitle").unwrap();
		assert_eq!(result.status, ReadinessStatus::Ready);
		assert!(output.detail.contains("出力履歴はまだない"));
		assert!(!output.detail.contains("user"));
		assert!(!output.detail.contains("ai-synth"));
	}

	#[test]
	fn arbitrary_output_history_is_visible_without_claiming_route_or_render_success() {
		let mut facts = ready_facts("streaming");
		facts.output_channels_seen = vec!["raid-notice".into(), "custom-overlay".into()];
		let result = build_readiness(facts);
		let output = result.checks.iter().find(|item| item.id == "subtitle").unwrap();
		assert_eq!(output.status, ReadinessStatus::Ready);
		assert!(output.detail.contains("raid-notice, custom-overlay"));
		assert!(output.detail.contains("各経路の動作やOBSでの表示までは確認できない"));
		assert!(!output.detail.contains("到達を確認済み"));
		assert!(!output.detail.contains("ai-synth"));
	}

	#[test]
	fn configured_avatar_motion_does_not_claim_runtime_readiness() {
		let result = build_readiness(ready_facts("streaming"));
		let avatar = result.checks.iter().find(|item| item.id == "avatar_motion").unwrap();
		assert_eq!(avatar.status, ReadinessStatus::Unverified);
		assert!(avatar.detail.contains("受信・反映は未確認"));
		assert_eq!(
			result.status,
			ReadinessStatus::Ready,
			"optional avatar attention must not block streaming"
		);
	}

	#[test]
	fn moving_matching_renderer_marks_avatar_ready() {
		let mut facts = ready_facts("streaming");
		facts.avatar_motion_probe = Some(crate::avatar_renderer_probe::AvatarRendererProbeResult {
			checked_at: "2026-08-01T00:00:00Z".into(),
			expected_base_keys: vec!["un-motion/frame/ai".into()],
			renderers: vec![crate::avatar_renderer_probe::AvatarRendererProbe {
				pid: 1,
				profile_hint: None,
				runtime_bus_key: Some("un/avatar/1".into()),
				connected: true,
				unmotion_zenoh_enabled: true,
				unmotion_zenoh_key: Some("un-motion/frame/ai".into()),
				key_matches: true,
				received_before: 100,
				received_after: 130,
				received_delta: 30,
				applied_before: 90,
				applied_after: 120,
				applied_delta: 30,
				error: None,
			}],
		});
		let result = build_readiness(facts);
		let avatar = result.checks.iter().find(|item| item.id == "avatar_motion").unwrap();
		assert_eq!(avatar.status, ReadinessStatus::Ready);
		assert!(avatar.detail.contains("受信・反映中"));
	}

    #[test]
    fn failure_diagnostic_readiness_exposes_actionable_error() {
        let mut facts = ready_facts("streaming");
        facts.ai_errors.push(crate::runtime_health::RuntimeIssue::error(
            "ai_reaction:test:request_or_tool_loop", "ai_reaction", "quota_exhausted",
            "API残高不足", "応答できない", "observability", None).with_resolution("残高を確認する"));
        let readiness = build_readiness(facts);
        let item = readiness.checks.iter().find(|c| c.id.starts_with("ai_reaction:")).unwrap();
        assert!(matches!(item.status, ReadinessStatus::Attention));
        assert!(item.detail.contains("残高を確認する"));
        assert_eq!(item.action.as_deref(), Some("observability"));
    }

    #[test]
    fn twitch_setup_readiness_guides_new_user_without_worker_jargon() {
        let mut facts = ready_facts("standard");
        facts.twitch_irc_unconfigured = 1;
        facts.twitch_irc_workers = 0;
        facts.twitch_eventsub_setup_missing = true;
        facts.twitch_eventsub_error = Some("raw implementation error".into());
        let summary = build_readiness(facts);
        for id in ["twitch_events", "twitch_chat"] {
            let check = summary.checks.iter().find(|c| c.id == id).unwrap();
            assert!(matches!(check.status, ReadinessStatus::Attention));
            assert!(check.detail.contains("Twitch接続"));
            assert!(!check.detail.contains("ワーカー") && !check.detail.contains("raw implementation"));
            assert_eq!(check.action.as_deref(), Some("resources"));
        }
    }

    #[test]
    fn twitch_setup_readiness_does_not_require_unused_or_node_configured_inputs() {
        let mut facts = ready_facts("standard");
        facts.twitch_eventsub_nodes = 0;
        facts.twitch_eventsub_setup_missing = true;
        facts.twitch_irc_unconfigured = 0;
        let summary = build_readiness(facts);
        assert!(matches!(summary.checks.iter().find(|c|c.id=="twitch_events").unwrap().status, ReadinessStatus::Inactive));
        assert!(matches!(summary.checks.iter().find(|c|c.id=="twitch_chat").unwrap().status, ReadinessStatus::Ready));
    }
}

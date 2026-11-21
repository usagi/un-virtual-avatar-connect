//! Control API WebSocket (`/api/v1/control/events`) で配信するイベント型。
//!
//! GUI / 外部ツールが現状把握に使う単一ストリーム。配信ルーティングは
//! [`crate::web_interface::control::ws`]、型の再エクスポートは
//! [`crate::web_interface::control::events`]。
//!
//! 設計メモ:
//!   - `broadcast::Sender<ControlEvent>` を [`crate::state::State`] に 1 本持たせ、
//!     `push_channel_datum` 等の節目で fire-and-forget で `send()` する。
//!   - 受信者が 0 件でも致命的ではない（送信エラーは debug ログのみ）。
//!   - 「新規」「ストリーム中の更新」「確定」「黙認（quiet）」を `phase` で区別する。
//!     GUI 側は phase で UI の描画方法を切り替える（例: Updated は既存行の in-place 更新）。
//!   - `ChannelDatum` から `ChannelDatum` イベントを組む `ControlEvent::from_channel_datum` は
//!     [`crate::state`] に `impl ControlEvent` として置き、本モジュールと `state` の循環を避ける。

use serde::Serialize;
use std::collections::BTreeMap;

/// AI入力へ変換した追加観測。本文・画像実体・ファイルパスは含めない。
#[derive(Debug, Clone, Serialize)]
pub struct AiContextObservation {
	pub datum_id: u64,
	pub channel: String,
	pub source_kind: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub source_subtype: Option<String>,
	pub observed_at: Option<String>,
	pub attachment_count: usize,
	pub input_image_count: usize,
}

/// First API request datum mapping; excludes text, actor names and image bytes.
#[derive(Debug, Clone, Serialize)]
pub struct AiContextDatum {
    pub datum_id: u64,
    /// Zero-based index in the first request's input array.
    pub input_index: usize,
    pub role: String,
    pub source_kind: Option<String>,
}

/// 音声認識 1 発話の更新フェーズ。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRecognitionPhase {
	/// ストリーミング認識中の暫定結果。
	Partial,
	/// AI / Flowgraph へ渡す確定結果。
	Final,
	/// 暫定結果はあったが、空の確定結果で破棄された。
	Discarded,
}

/// TTS 1発話の処理フェーズ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsLifecyclePhase {
	/// TTS driver 呼び出し直前。
	Started,
	/// Speech Schedulerが発話要求を受理。
	Queued,
	/// TTS engineによる音声合成が完了。
	Synthesized,
	/// 合成音声のデコードと共有再生シンクへの投入に成功。出力なし・投入失敗では発行しない。
	PlaybackStarted,
	/// 合成音声の再生が完了。
	Completed,
	/// 鮮度上限またはlane保留数の上限により、再生せず明示的に破棄。
	Dropped,
	/// 合成または再生キュー投入に失敗。
	Failed,
}

/// AI観測がモデル呼び出しへ進む前のactivation判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiActivationOutcome {
	Accepted,
	Activated,
	SkippedByScore,
	SkippedByMinInterval,
	DiscardedSuperseded,
	DiscardedUndelivered,
}

/// AI自発参加schedulerの候補化・抑制結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiInitiativeOutcome {
	SuppressedStreamUnavailable,
	SuppressedStartupGrace,
	SuppressedActiveConversation,
	SuppressedMinInterval,
	SuppressedAttemptBudget,
	SuppressedSpeechBudget,
	CandidateStreamStart,
	CandidateOngoingConversation,
	CandidateLongSilence,
	SkippedBeforeRequest,
	CompletedWithoutSpeech,
	Spoke,
	Failed,
}

/// AI function tool 1回の実行フェーズ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiToolCallPhase {
	Started,
	Completed,
}

/// `ProcessorInvoked` の結果種別。GUI はこれで pulse の色（OK / break / error）を変える。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessorInvocationOutcome {
	/// 正常終了。後続 processor も実行される。
	Continued,
	/// `CompletedAnd::Break` で後続をスキップ。
	Break,
	/// `process()` が `Err` を返した。
	Error,
}

/// ChannelDatum の発生フェーズ。WebSocket 購読者はこれで UI の描画方法を切り替える。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelDatumPhase {
	/// 新規 `push_channel_datum`（Processor 連鎖あり）
	Pushed,
	/// `push_channel_datum_quiet`（Processor 未実行・ストリーミング中間表示用）
	PushedQuiet,
	/// `update_channel_datum_content_by_id`（同一 id の content 差し替え）
	Updated,
	/// `finalize_channel_datum_and_dispatch`（is_final 付与後の Processor 連鎖起点）
	Finalized,
}

/// RM-5: mode 遷移に伴う Managed App 操作 1 行ぶん（`POST /modes/transit` の `managed_apps` と同形）。
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeModeManagedAppOp {
	pub id: String,
	pub op: String,
	pub ok: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub detail: Option<String>,
}

/// WebSocket で配信する 1 イベント。serde で JSON 化して `ws::text` で流す。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlEvent {
	/// ChannelDatum の発生・更新・確定イベント。
	ChannelDatum {
		phase: ChannelDatumPhase,
		id: u64,
		channel: String,
		content: String,
		flags: Vec<String>,
		datetime: String,
		/// 縦断診断用に明示許可した小さなメタデータ。ChannelDatum.meta全体は公開しない。
		#[serde(skip_serializing_if = "BTreeMap::is_empty")]
		diagnostic_meta: BTreeMap<String, serde_json::Value>,
	},
	/// Voice worker が生成した partial / final。Flowgraph へ partial を流さず観測だけ可能にする。
	VoiceRecognition {
		phase: VoiceRecognitionPhase,
		node_id: String,
		utterance_id: u64,
		text: String,
		/// 同じ utterance の最初の partial からの経過時間。partial を持たない engine では 0。
		elapsed_ms: u64,
		observed_at: String,
	},
	/// Twitch IRCがPRIVMSGを受信した事実。本文はChannelDatum側だけに保持し、診断イベントには複製しない。
	TwitchChatReceived {
		node_id: String,
		channel_login: String,
		sender_login: String,
		text_chars: usize,
		observed_at: String,
	},
	/// Chat-send node result only. No message text, tokens or remote error body.
	/// `sent` means Helix is_sent=true, not viewer display confirmation.
	TwitchChatSendResult {
		node_id: String,
		outcome: String,
	},
	/// Twitch EventSub WebSocketから実通知を受信し、Flowgraph triggerへ渡した事実。
	/// 生payloadや視聴者入力本文は複製せず、縦断診断に必要な識別子だけを保持する。
	TwitchEventSubReceived {
		node_id: String,
		event_type: String,
		broadcaster_login: String,
		actor_login: String,
		#[serde(skip_serializing_if = "Option::is_none")]
		reward_id: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		reward_title: Option<String>,
		user_input_chars: usize,
		observed_at: String,
	},
	/// Camera command diagnostics; delivery/response never implies rendered completion.
	AvatarCameraEffect {
		node_id: String,
		target_link_id: String,
		phase: String,
		details: serde_json::Value,
	},
	/// VAC内部のVoice Ingressソフトミュート状態が変化した。
	VoiceInputState { muted: bool, worker_count: usize },
	/// TTS driver の開始から合成・待機・再生完了までを観測する。
	TtsLifecycle {
		phase: TtsLifecyclePhase,
		node_id: String,
		/// 同じ発話要求を開始から再生完了まで照合する識別子。
		utterance_id: String,
		/// Original input behind this utterance, when registered by the AI service.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		observation_channel: Option<String>,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		observation_datum_id: Option<u64>,
		engine: String,
		voice: String,
		lane: String,
		text_chars: usize,
		elapsed_ms: u64,
		#[serde(skip_serializing_if = "Option::is_none")]
		queue_depth: Option<usize>,
		#[serde(skip_serializing_if = "Option::is_none")]
		wait_ms: Option<u64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		played: Option<bool>,
		#[serde(skip_serializing_if = "Option::is_none")]
		error: Option<String>,
		observed_at: String,
	},
	/// API送信直前の入力対応表。API成功や意味理解の成功を表さない。
	AiRequestContext {
		persona_id: String,
		observation_datum_id: u64,
		model: String,
		request_class: Option<String>,
		context_collected_at: String,
		prepared_at: String,
		observations: Vec<AiContextObservation>,
        input_datums: Vec<AiContextDatum>,
        /// Exact bounded untrusted history included in this request.
        #[serde(skip_serializing_if = "Option::is_none")]
        visual_history_json: Option<String>,
	},
	/// Failure stage/category only; never persist the provider error body.
	AiReactionFailure {
		persona_id: String,
		observation_datum_id: u64,
		stage: String,
		category: String,
		http_status: Option<u16>,
		observed_at: String,
	},
	/// AI が観測に対して発話するか、沈黙するかを決めた結果。
	AiTurnDecision {
		persona_id: String,
		observation_channel: String,
		observation_datum_id: u64,
		/// observe / respond / react / close
		action: String,
		speak: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		episode_id: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		episode_phase: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		episode_memory: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		episode_turn_count: Option<u32>,
		#[serde(skip_serializing_if = "Option::is_none")]
		episode_phase_remaining_secs: Option<u64>,
		/// Turn Managerの構造化出力に含まれたAvatarIntent数。
		avatar_intent_requested_count: usize,
		/// Validated requested parameters, not proof of Renderer delivery.
		/// Kept in the diagnostic journal; never contains dialogue text.
		#[serde(default, skip_serializing_if = "Vec::is_empty")]
		avatar_intent_parameters: Vec<serde_json::Value>,
		/// validationを通過し、avatar_intent channelへ送信したAvatarIntent数。
		avatar_intent_emitted_count: usize,
		#[serde(skip_serializing_if = "Option::is_none")]
		avatar_intent_target_link_id: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		avatar_intent_error: Option<String>,
		/// Turn Managerの構造化出力に名前付きポーズが含まれた。
		avatar_pose_requested: bool,
		/// validationを通過し、avatar_pose channelへ送信した。
		avatar_pose_emitted: bool,
		#[serde(skip_serializing_if = "Option::is_none")]
		avatar_pose_preset: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		avatar_pose_target_link_id: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		avatar_pose_error: Option<String>,
		/// Full structured turn available, before delivery/synchronization waits.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		output_ready_at: Option<String>,
		/// Correlates this observation with TTS lifecycle events without storing text.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		speech_utterance_id: Option<String>,
		/// Legacy timestamp: recorded after delivery/synchronization, not model completion.
		decided_at: String,
	},
	/// AI観測の受理からDecision Engineまで。本文を複製せず、沈黙理由だけを記録する。
	AiActivationDecision {
		persona_id: String,
		observation_channel: String,
		observation_datum_id: u64,
		outcome: AiActivationOutcome,
		#[serde(skip_serializing_if = "Option::is_none")]
		score: Option<f64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		threshold: Option<f64>,
		#[serde(skip_serializing_if = "Option::is_none")]
		remaining_interval_secs: Option<u64>,
		speech_floor_wait_ms: u64,
		observed_at: String,
	},
	/// 配信状態・静穏時間・独立予算に基づく自発参加判断。人間発話本文は保持しない。
	AiInitiativeDecision {
		persona_id: String,
		outcome: AiInitiativeOutcome,
		idle_secs: u64,
		attempts_last_hour: usize,
		speeches_last_hour: usize,
		observed_at: String,
	},
	/// AIが選択したlocal function toolの実行結果。引数と戻り値の本文は秘匿する。
	AiToolCall {
		phase: AiToolCallPhase,
		persona_id: String,
		tool_name: String,
		argument_chars: usize,
		elapsed_ms: u64,
		#[serde(skip_serializing_if = "Option::is_none")]
		success: Option<bool>,
		#[serde(skip_serializing_if = "Option::is_none")]
		error: Option<String>,
		observed_at: String,
	},
	/// 購読が追いつかず `broadcast::Receiver::Lagged(n)` を食らったことの通知。
	Lagged { dropped: u64 },
	/// 接続維持用のキープアライブ。サーバー起点で定期的に送る。
	Heartbeat {
		/// サーバー現在時刻（RFC3339）。
		now: String,
	},
	/// Control API からの pause/resume 反映をストリームにも流す（GUI の即時反映用）。
	PauseState {
		/// true = paused, false = resumed
		paused: bool,
		/// "all" / "processors" / "ais" / "processor" / "ai"
		target: &'static str,
		processors_affected: Vec<usize>,
		ais_affected: Vec<usize>,
	},
	/// Phase VI-α-4: Control API `/reload` が成功したときの通知。
	/// `detail` はターゲット種別ごとに中身が違う:
	///   - `ai_persona`: [`crate::ai::AiReloadReport`]
	///   - `modify_files`: [`Vec<ModifyReport>`]
	Reloaded {
		/// "ai_persona" / "modify_files"
		target: &'static str,
		/// 個別 id が指定されていたか（全体対象の場合は None）
		id: Option<String>,
		/// ターゲット別の詳細（JSON 値でそのまま埋め込む）
		detail: serde_json::Value,
	},
	/// Phase VI-β-8: Processor が 1 つ発火したときの通知。
	///
	/// `dispatch_processors_for_incoming` が `is_channel_from(channel_from)` で一致した
	/// Processor を `process()` 呼び出しするタイミングで 1 イベント送る。GUI の Svelte Flow
	/// パイプライン・ビューが当該ノードをパルスさせるのに使う。
	///
	/// 注意: `process()` の内部でさらに新しい ChannelDatum が push されるとそちらからも
	/// `ChannelDatum` / `ProcessorInvoked` イベントが連鎖的に飛ぶ。この設計のまま素直に
	/// 時系列に並べて食わせれば、GUI は「発火の波」を描ける。
	ProcessorInvoked {
		/// `State::processors` 配列での位置。0-origin。
		index: usize,
		/// 設定ファイルの `feature` 名（例: `"modify"`, `"openai_chat"`）。
		feature: String,
		/// conf の `id`（省略可）。
		id: Option<String>,
		/// トリガとなった ChannelDatum の連番 ID。
		channel_datum_id: u64,
		/// トリガとなった ChannelDatum の channel。`is_channel_from` がマッチしたもの。
		trigger_channel: String,
		/// `process()` 呼び出しから戻るまでの経過時間（ms）。
		elapsed_ms: u64,
		/// 結果。
		outcome: ProcessorInvocationOutcome,
	},
	/// Phase VI-γ-1: 再起動 API `/restart` が成功した直後、現プロセスが `exit` する前に送る通知。
	///
	/// GUI は WS が切れた後の自動再接続と組み合わせて「再起動中…」のトーストを出したり、
	/// プロファイル切替を完了表示に遷移させたりできる。新プロセスの WS が生きてから
	/// `Heartbeat` が来るまでの間を「restarting」状態として扱う想定。
	Restarting {
		/// 再起動に使った conf のパス（絶対）。現 conf 継続なら現 conf と同じ値。
		new_conf_path: String,
		/// spawn した新プロセスの PID（Windows では起動直後の値で、後続で子プロセスが再生成されることはない）。
		new_pid: u32,
		/// 現プロセスが exit するまでの猶予 ms。
		graceful_ms: u64,
		/// 現プロセス (= 再起動呼び出しを受けた側) の PID。
		current_pid: u32,
	},
	/// Phase δ-6: Flowgraph がリロードされたときの通知。
	///
	/// トリガは 2 つ:
	///   1. `POST/PUT/DELETE /flowgraph/file*` による write 操作（編集直後に発火）
	///   2. 明示的な `POST /flowgraph/reload` 呼び出し（GUI の「Reload」ボタン / 外部エディタ編集後）
	///
	/// GUI は本イベントを受けて `GET /flowgraph/diagnostics` / `GET /flowgraph/tree` を再取得し、
	/// パレット以外のノードビュー全体を更新する。diagnostics 本体は bandwidth 節約のため
	/// ここでは「件数サマリ」だけ載せて、詳細は pull する設計（spec §8.3 / §8.6）。
	FlowgraphReloaded {
		/// リロード対象ディレクトリ（絶対パス寄り）。
		root_dir: String,
		/// error 診断が 1 件もなければ true。
		ok: bool,
		error_count: usize,
		warning_count: usize,
		/// ロード済みノード件数（node_meta.len()）。
		node_count: usize,
	},

	/// Phase VI-γ-2b: Managed App の running / pids が変化したとき。
	///
	/// `managed_app::run_monitor` タスクが定期 sysinfo ポーリングで前回と差分を検出したとき、
	/// またはユーザー操作 (`/managed_apps/.../start|stop`) 完了時に送る。
	/// GUI の右肩ドロワーが受動的に反映する。
	ManagedAppState {
		/// 対象 Managed App の ID（`run_with` の `id` or 自動採番）。
		id: String,
		/// このイベント時点で running か。
		running: bool,
		/// マッチした PID 一覧（代表ではなく全部、順不同）。
		pids: Vec<u32>,
		/// 観測時刻（RFC3339）。
		checked_at: String,
	},
	/// ζ-3: flowgraph reload 後に、再起動無しでは新しい `flowgraph.ingress.web_input` を反映できない
	/// といった「本プロセス再起動が必要な差分」が発生したときの通知。
	///
	/// GUI はトースト表示で「再起動推奨」「差分の概要」を伝え、任意で `/api/v1/control/restart` 呼び出しに繋ぐ。
	/// reason は短文（例: `"web_input endpoints changed after flowgraph reload"`）、details は自由情報。
	RestartRecommended {
		reason: String,
		/// 任意メタデータ（例: `{"changed_web_input": 2}`）。UI 側は詳細をそのまま表示できる。
		details: serde_json::Value,
	},

	/// RM-5 先取り: Runtime Mode のスロットまたは実効 ID が変化したとき。
	///
	/// `PUT /modes/current` / `POST /modes/transit` / `flowgraph.mode.transit` が実際に状態を変えた場合に送る。
	/// `noop` 相当（同一スロット・同一実効）では発行しない。
	RuntimeModeChanged {
		#[serde(skip_serializing_if = "Option::is_none")]
		previous_slot: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		current_slot: Option<String>,
		previous_effective_id: String,
		current_effective_id: String,
		#[serde(skip_serializing_if = "Option::is_none")]
		reason: Option<String>,
	},

	/// RM-5: Runtime Mode 遷移直後に `[modes.*].managed_apps` を適用した結果（1 件以上のときのみ発行）。
	RuntimeModeManagedApps {
		previous_effective_id: String,
		current_effective_id: String,
		ops: Vec<RuntimeModeManagedAppOp>,
	},

	/// Phase VI-α-5: Twitch Device Code Flow のセッション状態が変化したとき。
	/// Control API `/oauth/twitch/{account}/start|cancel` や、ポーリングタスク完了時に送る。
	OAuthStatus {
		/// "broadcaster" / "moderator"
		account: &'static str,
		/// 新しい status
		status: crate::twitch_oauth_sessions::OAuthSessionStatus,
		/// セッションのスナップショット（`device_code` のような秘匿情報は含まない）
		view: crate::twitch_oauth_sessions::OAuthSessionView,
	},
}

impl ControlEvent {
	pub fn kind(&self) -> &'static str {
		match self {
			ControlEvent::ChannelDatum { .. } => "channel_datum",
			ControlEvent::VoiceRecognition { .. } => "voice_recognition",
			ControlEvent::TwitchChatReceived { .. } => "twitch_chat_received",
			ControlEvent::TwitchChatSendResult { .. } => "twitch_chat_send_result",
			ControlEvent::TwitchEventSubReceived { .. } => "twitch_event_sub_received",
			ControlEvent::AvatarCameraEffect { .. } => "avatar_camera_effect",
			ControlEvent::VoiceInputState { .. } => "voice_input_state",
			ControlEvent::TtsLifecycle { .. } => "tts_lifecycle",
			ControlEvent::AiTurnDecision { .. } => "ai_turn_decision",
			ControlEvent::AiRequestContext { .. } => "ai_request_context",
			ControlEvent::AiReactionFailure { .. } => "ai_reaction_failure",
			ControlEvent::AiActivationDecision { .. } => "ai_activation_decision",
			ControlEvent::AiInitiativeDecision { .. } => "ai_initiative_decision",
			ControlEvent::AiToolCall { .. } => "ai_tool_call",
			ControlEvent::Lagged { .. } => "lagged",
			ControlEvent::Heartbeat { .. } => "heartbeat",
			ControlEvent::PauseState { .. } => "pause_state",
			ControlEvent::Reloaded { .. } => "reloaded",
			ControlEvent::ProcessorInvoked { .. } => "processor_invoked",
			ControlEvent::Restarting { .. } => "restarting",
			ControlEvent::FlowgraphReloaded { .. } => "flowgraph_reloaded",
			ControlEvent::ManagedAppState { .. } => "managed_app_state",
			ControlEvent::RestartRecommended { .. } => "restart_recommended",
			ControlEvent::RuntimeModeChanged { .. } => "runtime_mode_changed",
			ControlEvent::RuntimeModeManagedApps { .. } => "runtime_mode_managed_apps",
			ControlEvent::OAuthStatus { .. } => "oauth_status",
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn serializes_channel_datum_with_phase_tag() {
		let ev = ControlEvent::ChannelDatum {
			phase: ChannelDatumPhase::Pushed,
			id: 42,
			channel: "ai".to_string(),
			content: "hi".to_string(),
			flags: vec!["is_final".to_string()],
			datetime: "2026-04-17T01:00:00Z".to_string(),
			diagnostic_meta: BTreeMap::new(),
		};
		let s = serde_json::to_string(&ev).unwrap();
		// 外から見える形: kind / phase / id / channel / ...
		assert!(s.contains(r#""kind":"channel_datum""#), "missing kind tag: {s}");
		assert!(s.contains(r#""phase":"pushed""#), "missing phase: {s}");
		assert!(s.contains(r#""id":42"#), "missing id: {s}");
		assert!(!s.contains("diagnostic_meta"), "empty diagnostic meta must be omitted: {s}");
	}

	#[test]
	fn serializes_voice_recognition_with_phase_and_utterance() {
		let ev = ControlEvent::VoiceRecognition {
			phase: VoiceRecognitionPhase::Partial,
			node_id: "solo/voice::in".to_string(),
			utterance_id: 7,
			text: "配信テスト".to_string(),
			elapsed_ms: 320,
			observed_at: "2026-07-25T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&ev).unwrap();
		assert_eq!(value["kind"], "voice_recognition");
		assert_eq!(value["phase"], "partial");
		assert_eq!(value["utterance_id"], 7);
		assert_eq!(ev.kind(), "voice_recognition");
	}

	#[test]
	fn serializes_voice_input_state() {
		let ev = ControlEvent::VoiceInputState {
			muted: true,
			worker_count: 1,
		};
		let value = serde_json::to_value(&ev).unwrap();
		assert_eq!(value["kind"], "voice_input_state");
		assert_eq!(value["muted"], true);
		assert_eq!(value["worker_count"], 1);
		assert_eq!(ev.kind(), "voice_input_state");
	}

	#[test]
	fn serializes_twitch_chat_evidence_without_message_text() {
		let ev = ControlEvent::TwitchChatReceived {
			node_id: "solo/twitch-chat::in".to_string(),
			channel_login: "usaginetwork".to_string(),
			sender_login: "viewer".to_string(),
			text_chars: 5,
			observed_at: "2026-08-01T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&ev).unwrap();
		assert_eq!(value["kind"], "twitch_chat_received");
		assert_eq!(value["text_chars"], 5);
		assert!(value.get("content").is_none());
		assert!(value.get("message_text").is_none());
		assert_eq!(ev.kind(), "twitch_chat_received");
	}

	#[test]
	fn serializes_twitch_eventsub_evidence_without_user_input() {
		let ev = ControlEvent::TwitchEventSubReceived {
			node_id: "twitch-channel-points/main::eventsub".to_string(),
			event_type: "channel.channel_points_custom_reward_redemption.add".to_string(),
			broadcaster_login: "usaginetwork".to_string(),
			actor_login: "viewer".to_string(),
			reward_id: Some("reward-id".to_string()),
			reward_title: Some("ケルシー先生へ".to_string()),
			user_input_chars: 12,
			observed_at: "2026-08-01T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&ev).unwrap();
		assert_eq!(value["kind"], "twitch_event_sub_received");
		assert_eq!(value["reward_id"], "reward-id");
		assert_eq!(value["user_input_chars"], 12);
		assert!(value.get("payload").is_none());
		assert!(value.get("user_input").is_none());
		assert_eq!(ev.kind(), "twitch_event_sub_received");
	}

	#[test]
	fn serializes_tts_lifecycle_with_timing() {
		let ev = ControlEvent::TtsLifecycle {
			phase: TtsLifecyclePhase::Queued,
			node_id: "solo/ai-tts::speak".to_string(),
			utterance_id: "turn-12:0".to_string(),
			observation_channel: Some("user".into()),
			observation_datum_id: Some(57),
			engine: "voicepeak".to_string(),
			voice: "Asumi Ririse".to_string(),
			lane: "assistant".to_string(),
			text_chars: 18,
			elapsed_ms: 1875,
			queue_depth: Some(2),
			wait_ms: Some(1875),
			played: Some(true),
			error: None,
			observed_at: "2026-07-25T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&ev).unwrap();
		assert_eq!(value["kind"], "tts_lifecycle");
		assert_eq!(value["phase"], "queued");
		assert_eq!(value["utterance_id"], "turn-12:0");
		assert_eq!(value["observation_channel"], "user");
		assert_eq!(value["observation_datum_id"], 57);
		assert_eq!(value["lane"], "assistant");
		assert_eq!(value["queue_depth"], 2);
		assert_eq!(value["elapsed_ms"], 1875);
		assert_eq!(value["played"], true);
		assert!(value.get("error").is_none());
		assert_eq!(ev.kind(), "tts_lifecycle");
	}

	#[test]
	fn serializes_ai_turn_decision_without_speech() {
		let ev = ControlEvent::AiTurnDecision {
			persona_id: "kaltsit".to_string(),
			observation_channel: "user".to_string(),
			observation_datum_id: 12,
			action: "observe".to_string(),
			speak: None,
			episode_id: None,
			episode_phase: None,
			episode_memory: None,
			episode_turn_count: None,
			episode_phase_remaining_secs: None,
			avatar_intent_requested_count: 0,
			avatar_intent_parameters: vec![],
			avatar_intent_emitted_count: 0,
			avatar_intent_target_link_id: None,
			avatar_intent_error: None,
			avatar_pose_requested: false,
			avatar_pose_emitted: false,
			avatar_pose_preset: None,
			avatar_pose_target_link_id: None,
			avatar_pose_error: None,
			output_ready_at: None,
			speech_utterance_id: None,
			decided_at: "2026-07-25T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&ev).unwrap();
		assert_eq!(value["kind"], "ai_turn_decision");
		assert_eq!(value["action"], "observe");
		assert!(value["speak"].is_null());
		assert_eq!(value["avatar_intent_requested_count"], 0);
		assert_eq!(value["avatar_intent_emitted_count"], 0);
		assert!(value.get("avatar_intent_error").is_none());
		assert_eq!(value["avatar_pose_requested"], false);
		assert_eq!(value["avatar_pose_emitted"], false);
		assert!(value.get("avatar_pose_error").is_none());
		assert_eq!(ev.kind(), "ai_turn_decision");
	}

	#[test]
	fn serializes_ai_activation_without_observation_text() {
		let event = ControlEvent::AiActivationDecision {
			persona_id: "kaltsit".to_string(),
			observation_channel: "user".to_string(),
			observation_datum_id: 42,
			outcome: AiActivationOutcome::SkippedByScore,
			score: Some(10.0),
			threshold: Some(50.0),
			remaining_interval_secs: None,
			speech_floor_wait_ms: 725,
			observed_at: "2026-08-09T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&event).unwrap();
		assert_eq!(value["kind"], "ai_activation_decision");
		assert_eq!(value["outcome"], "skipped_by_score");
		assert_eq!(value["speech_floor_wait_ms"], 725);
		assert!(value.get("content").is_none());
		assert_eq!(event.kind(), "ai_activation_decision");
	}

	#[test]
	fn serializes_ai_initiative_budget_evidence() {
		let event = ControlEvent::AiInitiativeDecision {
			persona_id: "kaltsit".to_string(),
			outcome: AiInitiativeOutcome::SuppressedSpeechBudget,
			idle_secs: 480,
			attempts_last_hour: 3,
			speeches_last_hour: 2,
			observed_at: "2026-08-09T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&event).unwrap();
		assert_eq!(value["kind"], "ai_initiative_decision");
		assert_eq!(value["outcome"], "suppressed_speech_budget");
		assert_eq!(value["idle_secs"], 480);
		assert_eq!(event.kind(), "ai_initiative_decision");
	}

	#[test]
	fn serializes_ongoing_conversation_initiative_candidate() {
		let event = ControlEvent::AiInitiativeDecision {
			persona_id: "kaltsit".to_string(),
			outcome: AiInitiativeOutcome::CandidateOngoingConversation,
			idle_secs: 1,
			attempts_last_hour: 0,
			speeches_last_hour: 0,
			observed_at: "2026-09-03T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&event).unwrap();
		assert_eq!(value["outcome"], "candidate_ongoing_conversation");
	}

	#[test]
	fn serializes_ai_tool_call_without_arguments_or_output() {
		let ev = ControlEvent::AiToolCall {
			phase: AiToolCallPhase::Completed,
			persona_id: "kaltsit".to_string(),
			tool_name: "vac_emit_avatar_intent".to_string(),
			argument_chars: 128,
			elapsed_ms: 4,
			success: Some(false),
			error: Some("target_link_id is required".to_string()),
			observed_at: "2026-08-01T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&ev).unwrap();
		assert_eq!(value["kind"], "ai_tool_call");
		assert_eq!(value["phase"], "completed");
		assert_eq!(value["tool_name"], "vac_emit_avatar_intent");
		assert_eq!(value["success"], false);
		assert!(value.get("arguments").is_none());
		assert!(value.get("output").is_none());
		assert_eq!(ev.kind(), "ai_tool_call");
	}

	#[test]
	fn serializes_ai_turn_decision_episode_snapshot() {
		let ev = ControlEvent::AiTurnDecision {
			persona_id: "kaltsit".to_string(),
			observation_channel: "user".to_string(),
			observation_datum_id: 13,
			action: "respond".to_string(),
			speak: Some("判断材料が足りない。".to_string()),
			episode_id: Some("kaltsit:13".to_string()),
			episode_phase: Some("yielding".to_string()),
			episode_memory: Some("戦闘ルートを検討中".to_string()),
			episode_turn_count: Some(1),
			episode_phase_remaining_secs: Some(25),
			avatar_intent_requested_count: 2,
			avatar_intent_parameters: vec![serde_json::json!({"kind":"head_nod","count":1,"intensity":0.3,"duration_ms":700})],
			avatar_intent_emitted_count: 2,
			avatar_intent_target_link_id: Some("kaltsit_ai".to_string()),
			avatar_intent_error: None,
			avatar_pose_requested: true,
			avatar_pose_emitted: true,
			avatar_pose_preset: Some("open_hands".to_string()),
			avatar_pose_target_link_id: Some("kaltsit_ai".to_string()),
			avatar_pose_error: None,
			output_ready_at: None,
			speech_utterance_id: None,
			decided_at: "2026-07-25T00:00:00Z".to_string(),
		};
		let value = serde_json::to_value(&ev).unwrap();
		assert_eq!(value["episode_id"], "kaltsit:13");
		assert_eq!(value["episode_phase"], "yielding");
		assert_eq!(value["episode_turn_count"], 1);
		assert_eq!(value["episode_phase_remaining_secs"], 25);
		assert_eq!(value["avatar_intent_requested_count"], 2);
		assert_eq!(value["avatar_intent_emitted_count"], 2);
		assert_eq!(value["avatar_intent_target_link_id"], "kaltsit_ai");
		assert_eq!(value["avatar_intent_parameters"][0]["kind"], "head_nod");
		assert_eq!(value["avatar_pose_requested"], true);
		assert_eq!(value["avatar_pose_emitted"], true);
		assert_eq!(value["avatar_pose_preset"], "open_hands");
	}

	#[test]
	fn serializes_lagged_event() {
		let ev = ControlEvent::Lagged { dropped: 7 };
		let s = serde_json::to_string(&ev).unwrap();
		assert!(s.contains(r#""kind":"lagged""#), "bad: {s}");
		assert!(s.contains(r#""dropped":7"#), "bad: {s}");
	}

	#[test]
	fn serializes_runtime_mode_changed() {
		let ev = ControlEvent::RuntimeModeChanged {
			previous_slot: Some("daily".into()),
			current_slot: Some("streaming".into()),
			previous_effective_id: "daily".into(),
			current_effective_id: "streaming".into(),
			reason: Some("api".into()),
		};
		let s = serde_json::to_string(&ev).unwrap();
		assert!(s.contains(r#""kind":"runtime_mode_changed""#), "bad: {s}");
		assert!(s.contains(r#""current_effective_id":"streaming""#), "bad: {s}");
	}

	#[test]
	fn serializes_runtime_mode_managed_apps() {
		let ev = ControlEvent::RuntimeModeManagedApps {
			previous_effective_id: "a".into(),
			current_effective_id: "b".into(),
			ops: vec![RuntimeModeManagedAppOp {
				id: "obs".into(),
				op: "stop".into(),
				ok: true,
				detail: Some("closed=1 terminated=0".into()),
			}],
		};
		let s = serde_json::to_string(&ev).unwrap();
		assert!(s.contains(r#""kind":"runtime_mode_managed_apps""#), "bad: {s}");
		assert!(s.contains(r#""op":"stop""#), "bad: {s}");
	}

	#[test]
	fn serializes_managed_app_state_event() {
		let ev = ControlEvent::ManagedAppState {
			id: "coeiroink".to_string(),
			running: true,
			pids: vec![1234, 5678],
			checked_at: "2026-04-17T01:00:00Z".to_string(),
		};
		let s = serde_json::to_string(&ev).unwrap();
		assert!(s.contains(r#""kind":"managed_app_state""#), "bad: {s}");
		assert!(s.contains(r#""id":"coeiroink""#), "bad: {s}");
		assert!(s.contains(r#""running":true"#), "bad: {s}");
		assert!(s.contains(r#""pids":[1234,5678]"#), "bad: {s}");
	}

	#[test]
	fn serializes_pause_state_with_target_string() {
		let ev = ControlEvent::PauseState {
			paused: true,
			target: "all",
			processors_affected: vec![0, 1],
			ais_affected: vec![0],
		};
		let s = serde_json::to_string(&ev).unwrap();
		assert!(s.contains(r#""kind":"pause_state""#));
		assert!(s.contains(r#""target":"all""#));
		assert!(s.contains(r#""paused":true"#));
	}

	#[test]
	fn serializes_processor_invoked() {
		let ev = ControlEvent::ProcessorInvoked {
			index: 2,
			feature: "modify".to_string(),
			id: Some("pre-command".to_string()),
			channel_datum_id: 17,
			trigger_channel: "user".to_string(),
			elapsed_ms: 3,
			outcome: ProcessorInvocationOutcome::Continued,
		};
		let s = serde_json::to_string(&ev).unwrap();
		assert!(s.contains(r#""kind":"processor_invoked""#), "missing kind: {s}");
		assert!(s.contains(r#""index":2"#));
		assert!(s.contains(r#""feature":"modify""#));
		assert!(s.contains(r#""id":"pre-command""#));
		assert!(s.contains(r#""channel_datum_id":17"#));
		assert!(s.contains(r#""trigger_channel":"user""#));
		assert!(s.contains(r#""elapsed_ms":3"#));
		assert!(s.contains(r#""outcome":"continued""#), "missing outcome: {s}");
	}

	#[test]
	fn serializes_restarting_event() {
		let ev = ControlEvent::Restarting {
			new_conf_path: "C:/path/to/conf.toml".to_string(),
			new_pid: 12345,
			graceful_ms: 800,
			current_pid: 6789,
		};
		let s = serde_json::to_string(&ev).unwrap();
		assert!(s.contains(r#""kind":"restarting""#), "missing kind: {s}");
		assert!(s.contains(r#""new_pid":12345"#));
		assert!(s.contains(r#""graceful_ms":800"#));
		assert!(s.contains(r#""current_pid":6789"#));
		assert!(s.contains(r#""new_conf_path":"C:/path/to/conf.toml""#));
	}

	#[test]
	fn processor_invocation_outcome_serializes_snake_case() {
		assert_eq!(
			serde_json::to_string(&ProcessorInvocationOutcome::Continued).unwrap(),
			r#""continued""#
		);
		assert_eq!(serde_json::to_string(&ProcessorInvocationOutcome::Break).unwrap(), r#""break""#);
		assert_eq!(serde_json::to_string(&ProcessorInvocationOutcome::Error).unwrap(), r#""error""#);
	}
}

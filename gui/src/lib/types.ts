/**
 * VAC Control API の DTO を TypeScript に写したもの。
 *
 * **本ファイルは Rust 側の真実の写し** であり、以下のファイルと 1:1 で整合させること:
 *   - src/web_interface/control/ping/mod.rs   → PingResponse / WhoAmIResponse
 *   - src/web_interface/control/dto/mod.rs  → StateSnapshot / *Summary
 *   - src/web_interface/control/actions/mod.rs → PauseTarget / PauseOutcome
 *   - src/web_interface/control/reload/mod.rs → ReloadRequest / ReloadResponse
 *   - src/twitch_oauth_sessions.rs（Control API は oauth_twitch/）→ OAuth* 一式
 *   - src/control_events.rs                   → ControlEvent (discriminated union)
 *   - src/ai/reload.rs                        → AiReloadRequest / AiReloadReport
 *
 * 受信側（WS の ControlEvent）は必ず `switch (ev.kind)` で分岐し、
 * 末尾に `const _: never = ev;` を置いて exhaustive check する。Rust 側で variant を増やしたとき
 * TypeScript 側のハンドラ漏れが **compile-time に** 検出される設計。
 *
 * I/O 境界での型擬態（`fetch().json()` が `any` を返す）は承知のうえで、
 * ここでは型を "信頼" する選択をしている。将来 valibot/zod での runtime validation を挟む予定。
 */

// ---------------------------------------------------------------------------
// /ping, /whoami
// ---------------------------------------------------------------------------

export type PingResponse = {
	ok: boolean;
	service: string;
	version: string;
	now: string;
};

export type WhoAmIResponse = {
	peer_addr: string | null;
	is_loopback: boolean;
	required_token: boolean;
	token_source: "env" | "config" | "generated";
	token_file: string | null;
};

// ---------------------------------------------------------------------------
// /snapshot
// ---------------------------------------------------------------------------

export type StateSnapshot = {
	schema: number;
	app_version: string;
	now: string;
	runtime: RuntimeSummary;
	/** schema 1 互換。schema 2 ではV1 processor層を廃止したため省略される。 */
	processors?: ProcessorSummary[];
	ai_personas: AiPersonaSummary[];
	voice_input: VoiceInputSummary;
	bridges: BridgeSummary;
	readiness: ReadinessSummary;
	twitch: TwitchSummary | null;
};

export type RuntimeSummary = {
	session_id: string;
	root: string;
	session_dir: string;
	inline_max_bytes: number;
};

export type ProcessorSummary = {
	index: number;
	feature: string;
	id: string | null;
	paused: boolean;
	last_invoked_at: string | null;
	/** Phase VI-γ-8a: `feature === "modify"` のときだけ present。`writable_dictionary_file` が設定されている processor のみ値が入る。 */
	writable_dictionary_file?: string;
	writable_regex_file?: string;
};

export type AiPersonaSummary = {
	index: number;
	id: string | null;
	display_name: string;
	paused: boolean;
	usage: AiUsageSummary;
};

export type VoiceInputSummary = {
	muted: boolean;
	worker_count: number;
};

export type BridgeSummary = {
	voice_nodes: number;
	voice_workers: number;
	voice_worker_statuses: VoiceWorkerStatusSummary[];
	voice_ingresses: VoiceIngressSummary[];
	twitch_irc_nodes: number;
	twitch_irc_workers: number;
	twitch_irc_ready_workers: number;
	twitch_irc_joined_channels: number;
	twitch_irc_expected_channels: number;
	twitch_irc_ingresses: TwitchIrcIngressSummary[];
	twitch_eventsub_nodes: number;
	twitch_eventsub_workers: number;
};

export type VoiceIngressSummary = {
	node_id: string;
	label: string;
	source: string;
	engine: string;
	active_backend: string | null;
	running: boolean;
};

export type TwitchIrcIngressSummary = {
	node_id: string;
	label: string;
	credential_key: string | null;
	configured_channels: string[];
	joined_channels: string[];
	ready: boolean;
};

export type VoiceWorkerStatusSummary = {
	node_id: string;
	engine: string;
	requested_backend: string;
	active_backend: string;
	partial_interval_ms: number;
	partial_min_audio_ms: number;
	partial_attempt_count: number;
	partial_count: number;
	partial_deadline_miss_count: number;
	final_count: number;
	error_count: number;
	last_partial_inference_ms?: number | null;
	last_final_inference_ms?: number | null;
	last_activity_at?: string | null;
	last_error?: string | null;
};

export type ReadinessStatus = "ready" | "attention" | "blocked" | "inactive" | "unverified";

export type ReadinessCheck = {
	id: string;
	area: "system" | "conversation" | "stream" | "avatar" | string;
	status: ReadinessStatus;
	title: string;
	detail: string;
	required: boolean;
	action: string | null;
};

export type ReadinessSummary = {
	status: ReadinessStatus;
	mode: string | null;
	checks: ReadinessCheck[];
};

export type PreflightTtsNode = {
	id: string;
	file: string;
	active: boolean;
	evidence: "untested" | "pending" | "ready" | "failed";
	elapsed_ms: number | null;
	error: string | null;
};

export type PreflightSelfTestStatus = {
	tts_nodes: PreflightTtsNode[];
	default_subtitle_channels: string[];
	streaming_contract: StreamingContractAudit;
	chat_output_routes: ChatOutputRoute[];
	avatar_targets: PreflightAvatarTarget[];
};

export type PreflightAvatarTarget = {
	target_link_id: string;
	base_key_expr: string;
};

export type AvatarRendererProbe = {
	pid: number;
	profile_hint: string | null;
	runtime_bus_key: string | null;
	connected: boolean;
	unmotion_zenoh_enabled: boolean;
	unmotion_zenoh_key: string | null;
	key_matches: boolean;
	received_before: number;
	received_after: number;
	received_delta: number;
	applied_before: number;
	applied_after: number;
	applied_delta: number;
	error: string | null;
};

export type AvatarMotionSelfTestResponse = {
	checked_at: string;
	expected_base_keys: string[];
	renderers: AvatarRendererProbe[];
};

export type ChatOutputRoute = {
	subscription_node_id: string;
	tts_node_id: string;
};

export type StreamingContractAudit = {
	status: "ready" | "attention" | "blocked" | "inactive";
	checks: StreamingContractCheck[];
};

export type StreamingContractCheck = {
	id: string;
	status: "ready" | "attention" | "blocked" | "inactive";
	detail: string;
};

export type TtsSelfTestRequest = {
	node_id: string;
	text?: string;
};

export type TtsSelfTestResponse = {
	node_id: string;
	completed: boolean;
	played: boolean;
	elapsed_ms: number;
	error: string | null;
};

export type SubtitleSelfTestRequest = {
	channels?: string[];
	text?: string;
	hold_ms?: number;
};

export type SubtitleSelfTestResponse = {
	channels: string[];
	datum_ids: number[];
	hold_ms: number;
};

export type ChatOutputSelfTestRequest = {
	text?: string;
};

export type ChatOutputSelfTestResponse = {
	subscription_node_id: string;
	tts_node_id: string;
	subtitle_reached: boolean;
	completed: boolean;
	played: boolean;
	elapsed_ms: number;
	error: string | null;
};

export type AiUsageSummary = {
	completed_reactions: number;
	failed_reactions: number;
	api_requests: number;
	tool_calls: number;
	input_tokens: number;
	cached_tokens: number;
	cache_write_tokens: number;
	output_tokens: number;
	reasoning_tokens: number;
	last_model: string | null;
	last_latency_ms: number | null;
	last_updated_at: string | null;
};

export type TwitchSummary = {
	username: string;
	channel_to: string;
	reads: string[] | null;
	eventsub_enabled: boolean;
	moderator_enabled: boolean;
	moderator_login: string | null;
	ignore_logins: string[];
};

// ---------------------------------------------------------------------------
// /pause, /resume
// ---------------------------------------------------------------------------

export type PauseTarget =
	| { target: "all" }
	| { target: "processors" }
	| { target: "ais" }
	| { target: "processor"; id?: string | null; index?: number | null }
	| { target: "ai"; id?: string | null; index?: number | null };

export type PauseOutcome = {
	processors_affected: number[];
	ais_affected: number[];
	paused: boolean;
};

export type VoiceInputState = VoiceInputSummary & { persistence_error?: string };

export type PutVoiceInputState = {
	muted: boolean;
};

// ---------------------------------------------------------------------------
// /reload
// ---------------------------------------------------------------------------

export type AiReloadRequest = {
	/** `null`: 変更しない / `""`: instructions クリア / `"xxx"`: 差し替え */
	custom_instructions?: string | null;
	system_instructions_extra?: string | null;
	heartbeat_enabled?: boolean | null;
	decision_threshold?: number | null;
};

export type AiReloadReport = {
	custom_instructions_changed: boolean;
	system_instructions_extra_changed: boolean;
	heartbeat_enabled_changed: boolean;
	decision_threshold_changed: boolean;
	rebuilt_request_template: boolean;
	rebuilt_decision_spec: boolean;
	warnings: string[];
};

export type ReloadRequest =
	| ({ target: "ai_persona"; id?: string | null } & AiReloadRequest)
	| { target: "modify_files"; id?: string | null };

export type ModifyReport = {
	processor_index: number;
	processor_id: string | null;
	feature: string;
	dictionary_entries: number;
	regex_entries: number;
};

export type ReloadResponse = {
	target: "ai_persona" | "modify_files";
	ai_report?: AiReloadReport;
	modify_reports?: ModifyReport[];
};

// ---------------------------------------------------------------------------
// /modes
// ---------------------------------------------------------------------------

export type ModesListResponse = {
	mode_ids: string[];
	modes: RuntimeModeSummary[];
};

export type RuntimeModeSummary = {
	id: string;
	display_name?: string | null;
};

export type CurrentModeResponse = {
	mode: string | null;
	managed_apps?: RuntimeModeManagedAppOp[] | null;
};

export type PutCurrentModeBody = {
	mode: string | null;
};

export type RuntimeModeManagedAppOp = {
	id: string;
	op: string;
	ok: boolean;
	detail?: string | null;
};

export type FlowgraphGroupsModeSpec = {
	enable: string[];
	disable: string[];
};

export type ManagedAppsModeDirective = {
	start: string[];
	stop: string[];
	minimize: string[];
	leave: string[];
};

export type CapabilityPolicyModeOverlay = {
	allow: string[];
	deny: string[];
};

export type ModeTransitionPlan = {
	from_slot: string | null;
	to_slot: string | null;
	from_effective_id: string;
	to_effective_id: string;
	noop: boolean;
	target_flowgraph_groups: FlowgraphGroupsModeSpec;
	target_managed_apps: ManagedAppsModeDirective;
	target_capability_policy: CapabilityPolicyModeOverlay;
	capability_denied_by_target: string[];
	capability_unlisted_by_target: string[];
	flowgraph_enable_added_vs_from: string[];
	flowgraph_disable_added_vs_from: string[];
};

export type ModePlanRequest = {
	target?: string | null;
};

export type ModeTransitRequest = {
	mode?: string | null;
	dry_run?: boolean;
	reason?: string | null;
};

export type ModeTransitResponse = {
	dry_run: boolean;
	mode: string | null;
	plan: ModeTransitionPlan;
	managed_apps?: RuntimeModeManagedAppOp[] | null;
};

export type RuntimeModeTransitionPhase =
	| "idle"
	| "planning"
	| "suppressing_flowgraph"
	| "applying_mode"
	| "applying_managed_apps"
	| "firing_flowgraph_hook"
	| "completed"
	| "failed";

export type RuntimeModeTransitionStatus = {
	seq: number;
	active: boolean;
	phase: RuntimeModeTransitionPhase;
	step_index: number;
	step_count: number;
	message: string;
	started_at?: string | null;
	updated_at: string;
	finished_at?: string | null;
	error?: string | null;
	plan?: ModeTransitionPlan | null;
	managed_apps: RuntimeModeManagedAppOp[];
};

// ---------------------------------------------------------------------------
// /oauth/twitch/*
// ---------------------------------------------------------------------------

export type OAuthAccount = "broadcaster" | "moderator";

export type OAuthSessionStatus = "pending" | "authorized" | "expired" | "canceled" | "failed";

export type OAuthSessionView = {
	account: OAuthAccount;
	status: OAuthSessionStatus;
	user_code: string;
	verification_uri: string;
	interval_secs: number;
	expires_at: string;
	started_at: string;
	last_error?: string;
};

export type OAuthStartResponse = {
	outcome: "started" | "already_pending" | "already_authorized";
	session: OAuthSessionView | null;
};

export type TwitchAuthorizationResponse = {
	account: OAuthAccount;
	configured: boolean;
	authorized: boolean;
};

export type OAuthCancelResponse = {
	canceled: boolean;
	session: OAuthSessionView;
};

/**
 * `DELETE /oauth/twitch/{account}/tokens` のレスポンス。
 *   - `deleted`: 実際にファイルが削除されたか（元々無ければ false）
 *   - `path`: 削除されたファイルのパス（存在しなければ null）
 *   - `session_removed`: 既存のセッションが除去されたか
 */
export type OAuthDeleteTokensResponse = {
	account: OAuthAccount;
	deleted: boolean;
	path: string | null;
	session_removed: boolean;
};

export type TwitchCustomReward = {
	id: string;
	title: string;
	prompt: string;
	cost: number;
	is_enabled: boolean;
	is_paused: boolean;
	is_in_stock: boolean;
	is_user_input_required: boolean;
	background_color: string;
};

export type TwitchRewardsResponse = {
 available?: boolean;
 reason?: string | null;
	broadcaster_login: string;
	broadcaster_id: string;
	rewards: TwitchCustomReward[];
};

// ---------------------------------------------------------------------------
// /oauth/google/sheets/*
// ---------------------------------------------------------------------------

export type GoogleOAuthStatusResponse = {
	configured: boolean;
	client_id_source: "request" | "env" | null;
	token_file: string;
	token_file_exists: boolean;
	has_access_token: boolean;
	has_refresh_token: boolean;
	scope: string | null;
};

export type GoogleOAuthStartRequest = {
	client_id?: string;
	scope?: string;
	token_file?: string;
	open_browser?: boolean;
};

export type GoogleOAuthStartResponse = {
	outcome: "started";
	client_id_source: "request" | "env";
	device_code: string;
	user_code: string;
	verification_url: string;
	verification_url_complete?: string | null;
	expires_in: number;
	interval: number;
	scope: string;
	token_file: string;
};

export type GoogleOAuthPollRequest = {
	client_id?: string;
	device_code: string;
	token_file?: string;
};

export type GoogleOAuthPollResponse = {
	status: "authorized" | "pending" | "failed";
	token_file: string;
	interval?: number | null;
	error?: string | null;
};

export type GoogleOAuthDeleteTokensResponse = {
	deleted: boolean;
	path: string;
};

// ---------------------------------------------------------------------------
// /ingress
// ---------------------------------------------------------------------------

/** ChannelDatum.source に載せる由来情報。 */
export type DataSource = {
	/** 例: `"control.ingress"`, `"twitch.eventsub"`, `"voice.whisper"`。 */
	kind: string;
	subtype?: string | null;
	/** 表示用の当事者名（ユーザー名、発言者名など）。 */
	actor?: string | null;
};

/**
 * `POST /api/v1/control/ingress` のリクエスト。
 * `channel` だけが必須で、他はすべて省略可能。
 */
export type IngressRequest = {
	channel: string;
	content: string;
	/** 省略時 backend 側で `true` 扱い。 */
	is_final?: boolean;
	/** 追加で立てたい flags。`is_final` 以外を想定。 */
	flags?: string[];
	/** 未指定時は backend が `{ kind: "control.ingress", actor: "gui" }` を補う。 */
	source?: DataSource;
	/** 自由形式の構造化メタ。JSON object。 */
	meta?: Record<string, unknown>;
};

export type IngressResponse = {
	id: number;
	channel: string;
	accepted: boolean;
};

// ---------------------------------------------------------------------------
// /quick_effects
// ---------------------------------------------------------------------------

export type QuickEffectStatus = "scheduled" | "active" | "expired" | "cancelled";

export type QuickEffectItem = {
	id: string;
	label: string;
	channel: string;
	content: string;
	flags: string[];
	created_at: string;
	created_at_ms: number;
	fire_at_ms: number | null;
	fired_at_ms: number | null;
	duration_ms: number | null;
	expires_at_ms: number | null;
	status: QuickEffectStatus;
	datum_id: number;
	meta: unknown;
};

export type QuickEffectEnqueueRequest = {
	label: string;
	channel: string;
	content: string;
	flags?: string[];
	duration_ms?: number | null;
	fire_at_ms?: number | null;
	meta?: Record<string, unknown>;
};

export type QuickEffectMoveRequest = {
	direction: "up" | "down" | "top" | "bottom" | "index";
	to_index?: number | null;
};

export type QuickEffectPreset = {
	id: string;
	label: string;
	channel: string;
	content: string;
	flags: string[];
	duration_ms?: number | null;
};

export type QuickEffectsResponse = {
	effects: QuickEffectItem[];
	limit: number;
	capacity: number;
};

export type QuickEffectPresetsResponse = {
	revision?: string | null;
	presets: QuickEffectPreset[];
	source_path: string | null;
	backup?: string | null;
	warning?: string | null;
};

export type QuickEffectPresetsPutRequest = {
	expected_revision?: string;
	presets: QuickEffectPreset[];
};

// ---------------------------------------------------------------------------
// /restart, /profiles (Phase VI-γ-1)
// ---------------------------------------------------------------------------

export type RestartRequest = {
	/** 現 conf と同ディレクトリの相対パスまたは絶対パス。未指定時は現 conf を引き継いで再起動。 */
	conf?: string;
	/** 再起動前の猶予 ms（default 800, max 10000）。 */
	graceful_ms?: number;
};

export type RestartResponse = {
	new_conf_path: string;
	new_pid: number;
	graceful_ms: number;
	current_pid: number;
};

// ---------------------------------------------------------------------------
// /shutdown (Phase ε-1)
// ---------------------------------------------------------------------------

export type ShutdownRequest = {
	/** 予約済み。cleanup の待ち時間ヒント。現状はサーバ側で無視される。 */
	graceful_ms?: number;
};

export type ShutdownResponse = {
	/** 常に `"shutting_down"`。冪等に何度叩いても同じ値が返る。 */
	status: string;
	/** 停止要求を受け付けたプロセスの PID。 */
	current_pid: number;
};

export type ProfileEntry = {
	path: string;
	filename: string;
	label: string;
	size: number;
	modified: string | null;
	is_current: boolean;
};

export type ProfilesResponse = {
	directory: string | null;
	entries: ProfileEntry[];
	current: string | null;
};

export type DesktopProfileEntry = {
	id: string;
	label: string;
	conf_path: string;
	is_active: boolean;
	template_id: string | null;
};

export type DesktopProfileTemplateEntry = {
	id: string;
	label: string;
	description: string;
	recommended: boolean;
};

export type DesktopProfilesResponse = {
	root: string;
	managed_mode: boolean;
	onboarding_completed: boolean;
	active_profile_id: string;
	/** Running process identity; active_profile_id is only the next-launch default. */
	current_profile_id?: string | null;
	current_conf_path?: string | null;
	profiles: DesktopProfileEntry[];
};

export type CreateDesktopProfileRequest = {
	id: string;
	label: string;
	conf?: string;
	template_id?: string;
};

export type DesktopProfileMutationResponse = {
	profile: DesktopProfileEntry;
};

export type DesktopAiSetupStatus = {
	revision?: string;
	configured: boolean;
	enabled: boolean;
	persona_id: string | null;
	display_name: string | null;
	model: string | null;
	custom_instructions: string | null;
	observe_triggers: string[];
	avatar_actions: boolean;
	avatar_link_id: string | null;
	avatar_base_key: string | null;
	avatar_publish: boolean;
	credential_source: "environment" | "profile_file" | "conf" | "missing" | string;
	template_id: string | null;
};

export type ConfigureDesktopAiRequest = {
	expected_revision?: string;
	persona_id: string;
	display_name: string;
	model: string;
	api_key?: string;
	custom_instructions: string;
	observe_triggers: string[];
	avatar_actions: boolean;
	avatar_link_id?: string;
	avatar_base_key?: string;
	avatar_publish: boolean;
};

export type DesktopAiMutationResponse = {
	status: DesktopAiSetupStatus;
	restart_required: boolean;
};

// ---------------------------------------------------------------------------
// /profiles/* operations (Phase VI-γ-4a)
// ---------------------------------------------------------------------------

export type ProfileContentResponse = {
	revision?: string;
	filename: string;
	path: string;
	size: number;
	content: string;
	is_current: boolean;
};

// ---------------------------------------------------------------------------
// /processors & /ai_personas PUT (Phase VI-γ-3b)
// ---------------------------------------------------------------------------

export type NodePutConfigRequest = {
	config: JsonValue;
	expected_revision?: string;
};

export type NodePutConfigResponse = {
	index: number;
	source_path: string;
	backup: string;
	warning: string;
};

export type ProfileOpResponse = {
	filename: string;
	path: string;
	/** 操作時に生成されたバックアップファイル名（あれば）。 */
	backup: string | null;
	/** 付随する警告（例: 現 conf を更新したので再起動が必要）。 */
	warning?: string;
};

export type ProfileCloneRequest = {
	source: string;
	new_filename: string;
};

export type ProfileRenameRequest = {
	new_filename: string;
};

export type ProfilePutContentRequest = {
	expected_revision?: string;
	content: string;
};

// ---------------------------------------------------------------------------
// /run_with (Phase VI-γ-5a)
// ---------------------------------------------------------------------------

export type RunWithTableDto = {
	mode_actions?: Record<string, string>;
	command: string;
	if_not_running?: string | null;
	start_on_launch?: boolean | null;
	run_as_admin?: boolean | null;
	working_dir?: string | null;
	minimized?: boolean | null;
	id?: string | null;
	label?: string | null;
	unaa_z_app_id?: string | null;
	unaa_z_instance_id?: string | null;
	shutdown?: {
		scope?: "process_tree" | "matched_processes" | null;
		on_vac_exit?: "close_and_wait" | "close_only" | "leave_running" | null;
		action?: "close_and_wait" | "close_only" | "leave_running" | null;
		method?: "syscommand" | "wm_close" | "both" | null;
		grace_ms?: number | null;
		app_specific?: string | null;
	} | null;
};

/** 文字列 (`"notepad"`) or table 形式。JSON 上は untagged。 */
export type RunWithDto = string | RunWithTableDto;

export type RunWithView = {
	modes?: { id: string; label: string; action: string }[];
	index: number;
	effective_id: string;
	display_label: string;
	supports_status: boolean;
	entry: RunWithDto;
};

export type RunWithListResponse = {
	modes?: { id: string; label: string; action: string }[];
	revision?: string | null;
	entries: RunWithView[];
	source_path: string | null;
};

export type RunWithMutationResponse = {
	modes?: { id: string; label: string; action: string }[];
	revision?: string | null;
	entries: RunWithView[];
	source_path: string | null;
	backup: string | null;
	warning?: string | null;
};

// ---------------------------------------------------------------------------
// /bos (Phase VI-γ-2a)
// ---------------------------------------------------------------------------

export type BosCategory = "subtitles" | "bgm" | "effects" | "other";

export type BosEntry = {
	id: string;
	title: string;
	category: BosCategory;
	/** `/browser-output/<id>/` 等の相対 URL。GUI 側で origin と合成。 */
	url: string;
	path: string;
	supports_channel_param: boolean;
	user_defined: boolean;
};

export type BosResponse = {
	document_root: string | null;
	url_prefix: string;
	entries: BosEntry[];
};

// ---------------------------------------------------------------------------
// /managed_apps (Phase VI-γ-2b)
// ---------------------------------------------------------------------------

export type ManagedAppStatus = {
	id: string;
	running: boolean;
	pids: number[];
	/** RFC3339 datetime */
	checked_at: string;
};

export type ManagedAppUnaaZStatus = {
	app_id: string;
	instance_id: string;
	discovery_source: string;
	discovery_source_key: string | null;
	discovery_payload_kind: string;
	discovery_observed_at: string;
	discovery_reported_at: string | null;
	discovery_age_ms: number | null;
	discovery_stale: boolean;
	zenoh_seen: boolean;
	running: boolean;
	ready: boolean;
	capability_count: number;
	capabilities: string[];
	command_count: number;
	commands: string[];
	allowed_command_count: number;
	allowed_commands: string[];
	endpoint_count: number;
};

export type ManagedAppView = {
	id: string;
	label: string;
	command: string;
	process_marker: string | null;
	minimized: boolean;
	run_as_admin: boolean;
	working_dir: string | null;
	supports_status: boolean;
	unaa_z_app_id: string | null;
	unaa_z_instance_id: string | null;
	status: ManagedAppStatus;
	unaa_z_status: ManagedAppUnaaZStatus;
};

export type ManagedAppsResponse = {
	entries: ManagedAppView[];
	unaa_z_source: string;
	unaa_z_discovery_errors: string[];
};

export type ManagedAppStopRequest = {
	grace_ms?: number;
};

export type ManagedAppStopResponse = {
	id: string;
	closed_windows: number;
	terminated_pids: number;
	note?: string;
};

export type ManagedAppStartResponse = {
	id: string;
	was_running: boolean;
};

export type ManagedAppMinimizeResponse = {
	id: string;
	scheduled_pids: number;
};

export type ManagedAppRestartResponse = {
	id: string;
	closed_windows: number;
	terminated_pids: number;
	was_running: boolean;
};

export type UnAppApiCommandRequest = {
	managed_app_id?: string | null;
	app_id?: string | null;
	instance_id?: string | null;
	command: string;
	args?: Record<string, unknown> | null;
	timeout_ms?: number | null;
	wait_response_ms?: number | null;
};

export type UnAppApiCommandObservation = {
	key_expr: string;
	source: string;
	payload: Record<string, unknown>;
};

export type UnAppApiCommandHistoryItem = {
	at: string;
	app_id: string;
	instance_id: string;
	command: string;
	request_id: string;
	key_expr: string;
	response_key_expr: string;
	event_key_expr: string;
	status: string;
	response_observed: boolean;
	response: UnAppApiCommandObservation | null;
	error: string | null;
};

export type UnAppApiCommandHistoryResponse = {
	commands: UnAppApiCommandHistoryItem[];
	limit: number;
};

export type UnAppApiCommandResponse = {
	accepted: boolean;
	app_id: string;
	instance_id: string;
	command: string;
	request_id: string;
	key_expr: string;
	response_key_expr: string;
	event_key_expr: string;
	response_observed: boolean;
	response: UnAppApiCommandObservation | null;
};

// ---------------------------------------------------------------------------
// /vmc/status (Phase M3)
// ---------------------------------------------------------------------------

export type VmcPassthroughBindState = "configured" | "skipped" | "running" | "failed" | "stopped";

export type VmcPassthroughStatusView = {
	id: string;
	label: string | null;
	enabled: boolean;
	bind: string;
	forward_to: string[];
	state: VmcPassthroughBindState;
	error: string | null;
	packets_received: number;
	bytes_received: number;
	packets_forwarded: number;
	send_errors: number;
	last_packet_at: string | null;
};

export type VmcStatusResponse = {
	entries: VmcPassthroughStatusView[];
};

export type VmcForwardRequest = {
	dest: string;
};

export type VmcBindRequest = {
	bind: string;
	forward_to: string[];
	label?: string | null;
};

// ---------------------------------------------------------------------------
// /modify/:id/dictionary|regex/entries (Phase VI-γ-8a)
// ---------------------------------------------------------------------------

export type DictionaryEntryRequest = {
	to: string;
	from: string;
};

export type RegexEntryRequest = {
	replacement: string;
	pattern: string;
};

export type DictionaryEntryResponse = {
	processor_index: number;
	processor_id: string;
	file: string;
	total_entries: number;
	already_present: boolean;
	removed_lines: number;
};

export type RegexEntryResponse = DictionaryEntryResponse;

/**
 * 外部エディタ委譲（γ-9）レスポンス。
 * サーバ側で `cmd /C start` / `open` / `xdg-open` を fire-and-forget で起動した結果を返す。
 */
export type OpenExternalResponse = {
	processor_index: number;
	processor_id: string;
	kind: "dictionary" | "regex";
	file: string;
	command: string;
	spawned: boolean;
};

// ---------------------------------------------------------------------------
// Modify content read-only views (Phase VI-γ-8d)
// ---------------------------------------------------------------------------

/** 辞書 1 行ぶん（`to<space>from`）。 */
export type DictionaryRow = { to: string; from: string };

export type DictionaryFileView = {
	file: string;
	is_writable: boolean;
	entries: DictionaryRow[];
	error?: string | null;
};

export type DictionaryContentResponse = {
	processor_index: number;
	processor_id: string;
	writable_file: string | null;
	files: DictionaryFileView[];
};

/** 正規表現 1 行ぶん。`valid: false` のときはメモリにロードされていない（= 置換対象から外れている）。 */
export type RegexRow = {
	replacement: string;
	pattern: string;
	valid: boolean;
	parse_error?: string | null;
};

export type RegexFileView = {
	file: string;
	is_writable: boolean;
	is_csv: boolean;
	entries: RegexRow[];
	error?: string | null;
};

export type RegexContentResponse = {
	processor_index: number;
	processor_id: string;
	writable_file: string | null;
	files: RegexFileView[];
};

// ---------------------------------------------------------------------------
// /processors/:index/config, /ai_personas/:index/config (Phase VI-γ-3a)
// ---------------------------------------------------------------------------

/** 任意の JSON 値。ProcessorConf / AiPersonaConf は巨大な構造体で、GUI 側では JSON ビューに生で流す。 */
export type JsonValue = null | boolean | number | string | JsonValue[] | { [k: string]: JsonValue };

export type ProcessorConfigResponse = {
	index: number;
	feature: string | null;
	id: string | null;
	config: JsonValue;
	source_path: string | null;
};

export type AiPersonaConfigResponse = {
	index: number;
	id: string | null;
	config: JsonValue;
	source_path: string | null;
	revision: string;
};

// ---------------------------------------------------------------------------
// /events — WebSocket の discriminated union
// ---------------------------------------------------------------------------

export type ChannelDatumPhase = "pushed" | "pushed_quiet" | "updated" | "finalized";
export type VoiceRecognitionPhase = "partial" | "final" | "discarded";

/** `ProcessorInvoked.outcome` の種別。pulse の色分けに使う。 */
export type ProcessorInvocationOutcome = "continued" | "break" | "error";
export type TtsLifecyclePhase = "started" | "queued" | "synthesized" | "playback_started" | "completed" | "dropped" | "failed";
export type AiActivationOutcome = "accepted" | "activated" | "skipped_by_score" | "skipped_by_min_interval" | "discarded_superseded" | "discarded_undelivered";
export type AiInitiativeOutcome =
	| "suppressed_stream_unavailable"
	| "suppressed_startup_grace"
	| "suppressed_active_conversation"
	| "suppressed_min_interval"
	| "suppressed_attempt_budget"
	| "suppressed_speech_budget"
	| "candidate_stream_start"
	| "candidate_long_silence"
	| "skipped_before_request"
	| "completed_without_speech"
	| "spoke"
	| "failed";
export type AiToolCallPhase = "started" | "completed";

/** Rust の `ControlEvent` enum と完全対応。`kind` discriminator で分岐する。 */
export type ControlEvent =
	| {
			kind: "channel_datum";
			phase: ChannelDatumPhase;
			id: number;
			channel: string;
			content: string;
			flags: string[];
			datetime: string;
			diagnostic_meta?: Record<string, unknown>;
	  }
	| {
			kind: "voice_recognition";
			phase: VoiceRecognitionPhase;
			node_id: string;
			utterance_id: number;
			text: string;
			elapsed_ms: number;
			observed_at: string;
	  }
	| {
			kind: "twitch_chat_received";
			node_id: string;
			channel_login: string;
			sender_login: string;
			text_chars: number;
			observed_at: string;
	  }
	| {
			kind: "twitch_chat_send_result";
			node_id: string;
			outcome: string;
	  }
	| {
			kind: "twitch_event_sub_received";
			node_id: string;
			event_type: string;
			broadcaster_login: string;
			actor_login: string;
			reward_id?: string;
			reward_title?: string;
			user_input_chars: number;
			observed_at: string;
	  }
	| {
			kind: "avatar_camera_effect";
			node_id: string;
			target_link_id: string;
			phase: string;
			details: Record<string, unknown> | null;
	  }
	| {
			kind: "voice_input_state";
			muted: boolean;
			worker_count: number;
	  }
	| {
			kind: "tts_lifecycle";
			phase: TtsLifecyclePhase;
			node_id: string;
			utterance_id: string;
			observation_channel?: string;
			observation_datum_id?: number;
			engine: string;
			voice: string;
			lane: "assistant" | "chat" | "system";
			text_chars: number;
			elapsed_ms: number;
			queue_depth?: number;
			wait_ms?: number;
			played?: boolean;
			error?: string;
			observed_at: string;
	  }
	| {
			kind: "ai_request_context";
			persona_id: string;
			observation_datum_id: number;
			model: string;
			request_class: string | null;
			context_collected_at: string;
			prepared_at: string;
			input_datums?: { datum_id: number; input_index: number; role: string; source_kind: string | null }[];
			observations: {
				datum_id: number;
				channel: string;
				source_kind: string;
				source_subtype?: string;
				observed_at: string | null;
				attachment_count: number;
				input_image_count: number;
			}[];
	  }
	| {
			kind: "ai_turn_decision";
			persona_id: string;
			observation_channel: string;
			observation_datum_id: number;
			action: "observe" | "respond" | "react" | "close";
			speak: string | null;
			episode_id?: string;
			episode_phase?: "engaged" | "yielding" | "refractory";
			episode_memory?: string;
			episode_turn_count?: number;
			episode_phase_remaining_secs?: number;
			output_ready_at?: string;
			speech_utterance_id?: string;
			decided_at: string;
	  }
	| {
			kind: "ai_activation_decision";
			persona_id: string;
			observation_channel: string;
			observation_datum_id: number;
			outcome: AiActivationOutcome;
			score?: number;
			threshold?: number;
			remaining_interval_secs?: number;
			speech_floor_wait_ms: number;
			observed_at: string;
	  }
	| {
			kind: "ai_initiative_decision";
			persona_id: string;
			outcome: AiInitiativeOutcome;
			idle_secs: number;
			attempts_last_hour: number;
			speeches_last_hour: number;
			observed_at: string;
	  }
	| {
			kind: "ai_tool_call";
			phase: AiToolCallPhase;
			persona_id: string;
			tool_name: string;
			argument_chars: number;
			elapsed_ms: number;
			success?: boolean;
			error?: string;
			observed_at: string;
	  }
	| {
			kind: "lagged";
			dropped: number;
	  }
	| {
			kind: "heartbeat";
			now: string;
	  }
	| {
			kind: "pause_state";
			paused: boolean;
			target: "all" | "processors" | "ais" | "processor" | "ai";
			processors_affected: number[];
			ais_affected: number[];
	  }
	| {
			kind: "reloaded";
			target: "ai_persona" | "modify_files";
			id: string | null;
			detail: unknown; // AiReloadReport | ModifyReport[] だが実装側で narrow する
	  }
	| {
			kind: "oauth_status";
			account: OAuthAccount;
			status: OAuthSessionStatus;
			view: OAuthSessionView;
	  }
	| {
			kind: "processor_invoked";
			index: number;
			feature: string;
			id: string | null;
			channel_datum_id: number;
			trigger_channel: string;
			elapsed_ms: number;
			outcome: ProcessorInvocationOutcome;
	  }
	| {
			kind: "restarting";
			new_conf_path: string;
			new_pid: number;
			graceful_ms: number;
			current_pid: number;
	  }
	| {
			kind: "managed_app_state";
			id: string;
			running: boolean;
			pids: number[];
			/** RFC3339 datetime */
			checked_at: string;
	  }
	| {
			/** Phase δ-6: Flowgraph ロード結果の更新通知。GUI は diagnostics / tree を pull し直す。 */
			kind: "flowgraph_reloaded";
			root_dir: string;
			ok: boolean;
			error_count: number;
			warning_count: number;
			node_count: number;
	  }
	| {
			kind: "restart_recommended";
			reason: string;
			details: unknown;
	  }
	| {
			kind: "runtime_mode_changed";
			previous_slot?: string | null;
			current_slot?: string | null;
			previous_effective_id: string;
			current_effective_id: string;
			reason?: string | null;
	  }
	| {
			kind: "runtime_mode_managed_apps";
			previous_effective_id: string;
			current_effective_id: string;
			ops: RuntimeModeManagedAppOp[];
	  };

/** ControlEvent の kind 文字列一覧（`never` チェック用ユーティリティ）。 */
export type ControlEventKind = ControlEvent["kind"];

export type ControlEventHistoryItem = {
	at: string;
	event: ControlEvent;
};

export type ControlEventHistoryResponse = {
	events: ControlEventHistoryItem[];
	limit: number;
};

/**
 * 使用例:
 * ```ts
 * switch (ev.kind) {
 *   case 'channel_datum': ...; break;
 *   case 'lagged': ...; break;
 *   // ... 全 variant を書かないと下で型エラー
 *   default: assertNever(ev);
 * }
 * ```
 */
export function assertNever(x: never): never {
	throw new Error(`unreachable: unexpected ControlEvent ${JSON.stringify(x)}`);
}

// ---------------------------------------------------------------------------
// Phase δ-6: Flowgraph Control API DTOs
// ---------------------------------------------------------------------------
//
// Rust 側: src/web_interface/control/flowgraph/mod.rs 等
//         + src/flowgraph/node.rs (NodeSpec / PortSpec / PropertySpec)
//         + src/flowgraph/loader/{file,diagnostic}.rs
//
// SocketType は Rust 側で文字列化されたうえで JSON に載る。例: "string" / "list<string>" / "map<json>" / "exec"。

/** ```rust
 * #[serde(rename_all = "lowercase")] pub enum Severity { Error, Warning, Info }
 * ``` */
export type FlowgraphSeverity = "error" | "warning" | "info";

/** ```rust
 * #[serde(rename_all = "kebab-case")] pub enum DiagnosticCode { ... }
 * ``` */
export type FlowgraphDiagnosticCode =
 | "runtime"
	| "toml-parse"
	| "unknown-feature"
	| "duplicate-node-id"
	| "invalid-port-ref"
	| "unresolved-node-ref"
	| "unknown-port"
	| "property-type-mismatch"
	| "missing-required-property"
	| "unknown-property"
	| "engine-build"
	| "io"
	| "ambiguous-main-ref"
	| "closed-string-literal-out-of-enum"
	| "duplicate-enum-id"
	| "invalid-enum-definition"
	| "unknown-library-ref"
	| "library-dependency-cycle"
	| "invalid-mode-metadata"
	| "orphan-mode-group"
	| "state-restore";

export type FlowgraphDiagnostic = {
	severity: FlowgraphSeverity;
	code: FlowgraphDiagnosticCode;
	message: string;
	/** 発生源ファイルの (OS ネイティブ区切りの) パス。GUI ではそのまま表示する。 */
	file?: string;
	node?: string;
	hint?: string;
};

/** SocketType 文字列（`"bool" | "int" | "float" | "string" | "json" | "exec" | "list<...>" | "map<...>"`）。 */
export type FlowgraphSocketType = string;

export type FlowgraphSocketTypeExpr = {
	kind: "primitive" | "generic";
	name: string;
	display: string;
	args?: FlowgraphSocketTypeExpr[];
};

export type FlowgraphPortDirection = "input" | "output";

export type FlowgraphPortSpec = {
	name: string;
	label: string;
	ty: FlowgraphSocketType;
	type_expr?: FlowgraphSocketTypeExpr;
	direction: FlowgraphPortDirection;
	is_exec: boolean;
	optional: boolean;
	/** Rust 側 `SocketValueRepr` は newtype で素の JSON として serialize される。 */
	default?: unknown;
	multi: boolean;
	description?: string;
	/** Phase λ: 閉集合 string（`ty === "string"` のとき）。 */
	closed_string_variants?: string[] | null;
	/** Phase ξ-5: node-catalog 注入。`default` から復元できた非無次元 Quantity のみ。 */
	quantity_dim?: string | null;
	quantity_unit_badge?: string | null;
	quantity_unit_full?: string | null;
};

export type FlowgraphContractPort = {
	name: string;
	label?: string;
	type: FlowgraphSocketType;
	type_expr?: FlowgraphSocketTypeExpr;
	direction: FlowgraphPortDirection;
	exec: boolean;
	optional: boolean;
	multi: boolean;
	default?: unknown;
	enum?: string[] | null;
};

export type FlowgraphContractProperty = {
	name: string;
	label?: string;
	type: FlowgraphSocketType;
	type_expr?: FlowgraphSocketTypeExpr;
	default?: unknown;
	required: boolean;
	validator?: string;
	enum?: string[] | null;
};

export type FlowgraphNodeContract = {
	version: 1;
	kind: "node";
	feature: string;
	inputs: FlowgraphContractPort[];
	outputs: FlowgraphContractPort[];
	properties: FlowgraphContractProperty[];
	summary: {
		input_count: number;
		output_count: number;
		property_count: number;
		has_exec_input: boolean;
		has_exec_output: boolean;
	};
};

export type FlowgraphEffectClass = "pure" | "stateful" | "effectful" | "unknown";

export type FlowgraphCapability =
	| "file_read"
	| "file_write"
	| "file_watch"
	| "network"
	| "db_read"
	| "db_write"
	| "process_control"
	| "window_control"
	| "desktop_capture"
	| "desktop_notification"
	| "obs_control"
	| "twitch_api"
	| "credential_access"
	| "audio_output"
	| "trace_write"
	| string;

export type FlowgraphPropertySpec = {
	name: string;
	label: string;
	ty: FlowgraphSocketType;
	type_expr?: FlowgraphSocketTypeExpr;
	default: unknown;
	required: boolean;
	description?: string;
	validator?: string;
	/** `"json"` / `"wav_path"` 等、GUI エディタのヒント。型厳密には任意文字列。 */
	ui_hint?: string;
	/**
	 * Phase ο-2: enum-style constraint. When present (and `ty === "string"`),
	 * the property editor renders a `<select>` dropdown instead of a plain text
	 * input. Server-side node impls still validate the value themselves.
	 */
	choices?: string[];
};

export type FlowgraphNodeSpec = {
	feature: string;
	title: string;
	category: string;
	description?: string;
	/** Phase λ: パレット行の一意キー（同一 feature の合成エントリ用）。 */
	palette_key?: string;
	inputs: FlowgraphPortSpec[];
	outputs: FlowgraphPortSpec[];
	properties: FlowgraphPropertySpec[];
	/**
	 * Phase φ-6: Control API `POST /flowgraph/{instance}/trigger/{node_id}` で
	 * このノードを外部から発火できるか。サーバ側では `NodeDescriptor::control_triggerable()`
	 * の opt-in が source of truth で、node-catalog レスポンス JSON にだけこの field が注入される。
	 * 古いバックエンドに繋いだ場合は undefined になる可能性があるため `?` を付ける。
	 */
	control_triggerable?: boolean;
	/** LF-1: node signature / library signature 共通化に向けた machine-readable contract。 */
	contract?: FlowgraphNodeContract;
	/** LF-2: Pure / Stateful / Effectful の catalog metadata。 */
	effect_class?: FlowgraphEffectClass;
	/** LF-2: node が要求する外部 capability。現段階では read-only metadata。 */
	capabilities?: FlowgraphCapability[];
};

export type FlowgraphNodeCatalogResponse = {
	specs: FlowgraphNodeSpec[];
	count: number;
};

export type FlowgraphTreeFileEntry = {
	source_digest?: string | null;
	/** `<root>/<fq>.flowgraph.toml` の fq 部分（`/` 区切り、拡張子なし）。 */
	fq: string;
	/** root 相対パス（`/` 区切り）。 */
	path: string;
	title: string | null;
	node_count: number | null;
	edge_count: number | null;
	parse_error: string | null;
};

export type FlowgraphTreeResponse = {
	root_dir: string;
	exists: boolean;
	files: FlowgraphTreeFileEntry[];
};

/** TOML 上の 1 ノードエントリ（`[[nodes]]`）。 */
export type FlowgraphNodeEntry = {
	id: string;
	feature: string;
	/** `[x, y]` / GUI でのみ使用。 */
	position?: [number, number] | null;
	/** `toml::Table` → JSON object に変換されたもの。key = プロパティ名。 */
	properties: Record<string, unknown>;
};

export type FlowgraphEdgeEntry = {
	from: string;
	to: string;
};

export type FlowgraphFileMeta = {
	title?: string | null;
	description?: string | null;
	tags?: string[] | null;
	author?: string | null;
	name?: string | null;
	version?: string | null;
	license?: string | null;
	repos?: string | null;
	mode_groups?: string[];
	default_enabled?: boolean;
};

export type FlowgraphGraphInterfaceInput = { name: string; type: string; to: string };
export type FlowgraphGraphInterfaceOutput = { name: string; type: string; from: string };
export type FlowgraphGraphDeclaration = {
	kind: "entry" | "module";
	inputs: FlowgraphGraphInterfaceInput[];
	outputs: FlowgraphGraphInterfaceOutput[];
};

export type FlowgraphGraphInstance = {
	id: string;
	graph: string;
	position?: [number, number] | null;
};

export type FlowgraphPackageManifest = {
	id?: string | null;
	version?: string | null;
	exports: string[];
	dependencies: Record<string, string>;
};

export type FlowgraphTypeDef = {
	id: string;
	kind: string;
	description?: string | null;
	fields: Record<string, string>;
};

export type FlowgraphEnumDef = {
	id: string;
	primitive?: string | null;
	variants: string[];
};

export type FlowgraphGroupDef = {
	id: string;
	label?: string | null;
	node_ids: string[];
	color?: string | null;
};

export type FlowgraphFileDocument = {
	meta: FlowgraphFileMeta | null;
	graph: FlowgraphGraphDeclaration;
	package?: FlowgraphPackageManifest | null;
	nodes: FlowgraphNodeEntry[];
	edges: FlowgraphEdgeEntry[];
	enums?: FlowgraphEnumDef[];
	types?: FlowgraphTypeDef[];
	groups?: FlowgraphGroupDef[];
	instances?: FlowgraphGraphInstance[];
};

export type FlowgraphGraphInterfacePortSummary = { name: string; ty: string; mapping: string };
export type FlowgraphGraphDefinitionSummary = {
	fq: string;
	kind: "entry" | "module";
	inputs: FlowgraphGraphInterfacePortSummary[];
	outputs: FlowgraphGraphInterfacePortSummary[];
};
export type FlowgraphGraphInstanceSummary = {
	owner_fq: string;
	id: string;
	graph_fq: string;
	runtime_fq: string;
	position: [number, number] | null;
	active: boolean;
};
export type FlowgraphCrossFilePortalSummary = {
	owner_fq: string;
	from: string;
	to: string;
	from_fq: string;
	to_fq: string;
	from_active: boolean;
	to_active: boolean;
	active: boolean;
};
export type FlowgraphCompositionSummary = {
	graphs: FlowgraphGraphDefinitionSummary[];
	instances: FlowgraphGraphInstanceSummary[];
	portals: FlowgraphCrossFilePortalSummary[];
};

export type FlowgraphRuntimeTraceEvent = {
	generation: number;
	runtime_node_id: string;
	source_fq: string;
	instance_path: string;
	port: string;
	phase: "exec_input" | "exec_output" | "data_output" | "exec_suppressed" | "log";
	value_summary: string;
	active: boolean;
};

export type FlowgraphFileResponse = {
	root_dir?: string;
	fq: string;
	path: string;
	/** `.flowgraph.toml` の生テキスト。テキストエディタビュー用。 */
	raw_toml: string;
	/** TOML パース成功時のみ。 */
	parsed: FlowgraphFileDocument | null;
	parse_diagnostics: FlowgraphDiagnostic[];
};

export type FlowgraphLoadedNodeMeta = {
	feature: string;
	file: string;
	position?: [number, number] | null;
};

export type FlowgraphCapabilityNode = {
	node: string;
	feature: string;
	effect_class: string;
	capabilities: string[];
};

export type FlowgraphStateNode = {
	node: string;
	feature: string;
	scope: "none" | "node_instance";
	storage: "none" | "volatile";
	lifetime: "none" | "program_instance";
	reinitialized_on_reload: boolean;
	snapshot_supported: boolean;
	snapshot_policy: "unsupported" | "explicit";
	snapshot_format: "none" | "json";
	restore_supported: boolean;
	restore_policy: "unsupported" | "explicit";
	migration_policy: "none";
	persistence_policy: "none";
};

export type FlowgraphStateModel = {
	version: number;
	stateful: boolean;
	scope: "none" | "node_instance";
	storage: "none" | "volatile";
	lifetime: "none" | "program_instance";
	reinitialized_on_reload: boolean;
	snapshot_supported: boolean;
	snapshot_policy: "unsupported" | "explicit";
	snapshot_format: "none" | "json";
	restore_supported: boolean;
	restore_policy: "unsupported" | "explicit";
	migration_policy: "none";
	persistence_policy: "none";
};

export type FlowgraphProgramStateNode = {
	node: string;
	feature: string;
	version: number;
	state_model: FlowgraphStateModel;
};

export type FlowgraphProgramStateSummary = {
	stateful_node_count: number;
	snapshot_supported_node_count: number;
	restore_supported_node_count: number;
	nodes: FlowgraphProgramStateNode[];
};

export type FlowgraphProgramStateSnapshotNode = {
	node: string;
	feature: string;
	version: number;
	format: "none" | "json";
	value: JsonValue;
};

export type FlowgraphProgramStateSnapshot = {
	snapshot_node_count: number;
	nodes: FlowgraphProgramStateSnapshotNode[];
};

export type FlowgraphProgramStateRestoreNode = {
	node: string;
	feature: string;
	version: number;
};

export type FlowgraphProgramStateRestoreReport = {
	restored_node_count: number;
	nodes: FlowgraphProgramStateRestoreNode[];
};

export type FlowgraphFileActivationMeta = {
	mode_groups: string[];
	default_enabled: boolean;
};

export type FlowgraphCapabilitySummary = {
	node_count: number;
	effectful_node_count: number;
	stateful_node_count: number;
	volatile_state_node_count: number;
	snapshot_supported_state_node_count: number;
	restore_supported_state_node_count: number;
	capabilities: string[];
	capability_counts: Record<string, number>;
	nodes: FlowgraphCapabilityNode[];
	state_nodes: FlowgraphStateNode[];
};

export type FlowgraphSignatureFile = {
	fq: string;
	title?: string;
	description?: string;
	library_id?: string;
	package_id?: string;
	package_version?: string;
	package_exports: string[];
	mode_groups: string[];
	default_enabled: boolean;
};

export type FlowgraphSignaturePort = {
	node: string;
	feature: string;
	port: string;
	label: string;
	ty: string;
	type_expr?: FlowgraphSocketTypeExpr;
	direction: "input" | "output";
	exec: boolean;
	optional: boolean;
	multi: boolean;
};

export type FlowgraphSignatureTrigger = {
	node: string;
	feature: string;
	trigger_kind: "ingress" | "control" | string;
	exec_inputs: string[];
	data_inputs: FlowgraphSignaturePort[];
};

export type FlowgraphSignature = {
	version: number;
	kind: "flowgraph" | string;
	file_count: number;
	node_count: number;
	edge_count: number;
	effectful_node_count: number;
	stateful_node_count: number;
	snapshot_supported_state_node_count: number;
	restore_supported_state_node_count: number;
	required_capabilities: string[];
	files: FlowgraphSignatureFile[];
	type_schemas: FlowgraphTypeSchemaSummary[];
	type_schema_dependency_order: string[];
	type_schema_graph_summary: FlowgraphTypeSchemaGraphSummary;
	external_triggers: FlowgraphSignatureTrigger[];
	boundary_inputs: FlowgraphSignaturePort[];
	boundary_outputs: FlowgraphSignaturePort[];
};

export type FlowgraphPackageManifestSummary = {
	source_fq: string;
	source_digest?: string;
	id?: string;
	version?: string;
	exports: string[];
	dependencies: Record<string, string>;
};

export type FlowgraphPackageLockEntry = {
	id: string;
	version?: string;
	source_fq: string;
	source_digest: string;
	digest: string;
	dependencies: Record<string, string>;
};

export type FlowgraphTypeSchemaSummary = {
	source_fq: string;
	id: string;
	kind: string;
	description?: string;
	fields: Record<string, string>;
	field_type_exprs: Record<string, FlowgraphSocketTypeExpr>;
	field_record_refs: Record<string, string[]>;
	record_refs: string[];
	external_record_refs: string[];
	referenced_by: string[];
};

export type FlowgraphTypeSchemaGraphSummary = {
	schema_count: number;
	dependency_edge_count: number;
	external_ref_count: number;
	root_schema_count: number;
	leaf_schema_count: number;
};

export type FlowgraphDiagnosticsResponse = {
	/** Loaded topology, not current mode eligibility or playback evidence. Absent on older servers. */
	speech_routes?: Array<{
		runtime_node: string;
		source_file: string;
		source_graph: string | null;
		instance_path: string | null;
		definition_instances: number;
		incoming: Array<{from_node:string;from_port:string;to_port:string;exec:boolean}>;
	}>;
	/** Exact file bytes observed by the loader; absent on older runtimes. */
	source_digests?: Record<string, string>;
	root_dir: string;
	ok: boolean;
	diagnostics: FlowgraphDiagnostic[];
	runtime_issues: RuntimeIssue[];
	/** `fq_name` → 定義元メタ。 */
	node_meta: Record<string, FlowgraphLoadedNodeMeta>;
	capability_summary: FlowgraphCapabilitySummary;
	graph_signature: FlowgraphSignature;
	package_manifests: FlowgraphPackageManifestSummary[];
	package_dependency_order: string[];
	package_lock_preview: FlowgraphPackageLockEntry[];
	type_schemas: FlowgraphTypeSchemaSummary[];
	type_schema_dependency_order: string[];
	type_schema_graph_summary: FlowgraphTypeSchemaGraphSummary;
	package_lock_preview_digest: string | null;
	loaded_state_summary: FlowgraphProgramStateSummary;
	loaded_state_snapshot: FlowgraphProgramStateSnapshot;
	loaded_state_restore_report: FlowgraphProgramStateRestoreReport | null;
	state_snapshot_file_path: string | null;
	state_snapshot_file_exists: boolean | null;
	file_activation: Record<string, FlowgraphFileActivationMeta>;
	composition: FlowgraphCompositionSummary;
	inactive_exec_nodes: string[];
	runtime_trace: FlowgraphRuntimeTraceEvent[];
	unaa_z: UnAppApiDiscoverySnapshot;
	unaa_z_health: UnAppApiDiscoveryHealthSummary;
	unaa_z_endpoint_candidates: Record<string, unknown>[];
	unmfz_transport: UnmfzTransportSnapshot;
	unaa_z_endpoint_candidate_diagnostics: UnAppApiEndpointCandidateDiagnostic[];
};

export type UnmfzTransportSnapshot = {
	mode: "automatic" | "listen" | "connect" | "listen_connect";
	automatic_discovery: boolean;
	listen_endpoint: string | null;
	connect_endpoints: string[];
};

export type RuntimeIssue = {
	key: string;
	severity: "warning" | "error";
	component: string;
	code: string;
	message: string;
	impact: string;
	resolution?: string;
	action: string;
	node: string | null;
	updated_at: string;
};

export type UnAppApiDiscoveryHealthSummary = {
	source: string;
	app_count: number;
	managed_registry_app_count: number;
	zenoh_app_count: number;
	stale_app_count: number;
	root_endpoint_count: number;
	endpoint_candidate_count: number;
	endpoint_warning_count: number;
	endpoint_warning_app_count: number;
	discovery_error_count: number;
	query_key_count: number;
	announcement_key_count: number;
};

export type UnAppApiEndpointCandidateDiagnostic = {
	severity: string;
	code: string;
	message: string;
	field: string;
	candidate_id: string | null;
	app_id: string | null;
	endpoint_id: string | null;
	source_key: string | null;
};

export type UnAppApiDiscoverySnapshot = {
	schema_version: number;
	source: string;
	generated_at: string;
	apps: UnAppApiDiscoveredApp[];
	endpoints: Record<string, unknown>[];
	discovery_errors: string[];
};

export type UnAppApiDiscoveredApp = {
	app_id: string;
	instance_id: string;
	display_name: string;
	managed_app_id: string | null;
	discovery_source: string;
	discovery_source_key: string | null;
	discovery_payload_kind: string;
	observed_at: string;
	reported_at: string | null;
	age_ms: number | null;
	stale: boolean;
	running: boolean;
	ready: boolean;
	capabilities: string[];
	commands: string[];
	endpoints: Record<string, unknown>[];
};

export type FlowgraphPackageLockPreviewResponse = {
	digest: string | null;
	entries: FlowgraphPackageLockEntry[];
	entry_count: number;
};

export type FlowgraphSavePackageLockPreviewResponse = {
	path: string;
	digest: string | null;
	entry_count: number;
	written: boolean;
};

export type FlowgraphPackageLockStatusResponse = {
	path: string;
	exists: boolean;
	preview_digest: string | null;
	lock_digest: string | null;
	matches_preview: boolean | null;
	entry_count: number | null;
	diff: {
		added_ids: string[];
		removed_ids: string[];
		changed_ids: string[];
		unchanged_ids: string[];
	} | null;
	file: {
		kind: string;
		schema_version: number;
		created_at_unix_ms: number;
		digest: string | null;
		entry_count: number;
		entries: FlowgraphPackageLockEntry[];
	} | null;
	error: string | null;
};

export type FlowgraphCreateFileRequest = {
	fq: string;
	initial_toml?: string;
};

export type FlowgraphPutFileRequest = {
	expected_root?: string;
	content: string;
	expected_content?: string;
};

export type FlowgraphWriteFileResponse = {
	restart_required?: boolean;
	fq: string;
	path: string;
	diagnostics: FlowgraphDiagnostic[];
	ok: boolean;
	backup: string | null;
};

export type FlowgraphOpenExternalResponse = {
	fq: string;
	path: string;
	command: string;
	spawned: boolean;
};

export type FlowgraphReloadResponse = {
	root_dir: string;
	ok: boolean;
	diagnostics: FlowgraphDiagnostic[];
	node_count: number;
	restart_required?: boolean;
};

export type FlowgraphReloadPreservingStateResponse = {
	restart_required?: boolean;
	root_dir: string;
	path: string;
	ok: boolean;
	diagnostics: FlowgraphDiagnostic[];
	node_count: number;
	saved_node_count: number;
	restored_node_count: number | null;
};

export type FlowgraphRestoreStateSnapshotResponse = {
	restart_required?: boolean;
	root_dir: string;
	path: string;
	ok: boolean;
	diagnostics: FlowgraphDiagnostic[];
	node_count: number;
	restored_node_count: number | null;
};

export type FlowgraphSaveLoadedStateSnapshotResponse = {
	path: string;
	snapshot_node_count: number;
	written: boolean;
};

export type FlowgraphSaveLiveStateSnapshotResponse = {
	path: string;
	snapshot_node_count: number;
	written: boolean;
};

// ---------------------------------------------------------------------------
// Phase δ-7: Fragment Share / Export / Import DTOs
// ---------------------------------------------------------------------------
//
// Rust 側:
//   - src/flowgraph/fragment/mod.rs  (CopyRequest / CopyTarget / Fragment / FragmentDangling …)
//   - src/flowgraph/fragment/paste.rs (PasteRequest / PasteTarget / PasteOptions / PasteReport …)
//   - src/flowgraph/fragment/zip_codec.rs (ZipImportOptions / ZipImportOutcome …)
//   - src/web_interface/control/flowgraph/fragment_zip.rs (CopyResponse / PasteResponse)

export type FragmentScope = "nodes" | "file" | "folder" | "mixed";

/** Saved-file dependency candidates, not a guarantee that an export is portable. */
export type FlowgraphSharePlan = {
	asset_candidates?: Array<{ from_file: string; field: string; raw: string; target: string | null; error: string | null }>;
	root_dir?: string;
	source_digests?: Record<string, string>;
	files: string[];
	portable_feature_verified: false;
	unverified: string[];
	references: Array<{ from_file: string; instance_id: string; graph: string; target: string | null; error: string | null }>;
	package_references: Array<{ from_file: string; package_id: string; requirement: string; candidates: string[]; target: string | null; error: string | null }>;
	static_references: Array<{ from_file: string; field: string; raw: string; target: string | null; error: string | null }>;
	type_references: Array<{ from_file: string; field: string; expression: string; schema_id: string | null; candidates: string[]; target: string | null; error: string | null }>;
};

/** `POST /flowgraph/fragment/copy` の body。 */
export type FragmentCopyRequest = {
	scope: FragmentScope;
	targets: FragmentCopyTarget[];
	origin?: string;
};

export type FragmentCopyTarget =
	| { kind: "node"; fq_file: string; node_id: string }
	| { kind: "file"; fq: string }
	| { kind: "folder"; path: string };

/** ZIP only: explicitly selected root-relative companion files. */
export type ZipExportRequest = FragmentCopyRequest & { companions?: string[]; expected_root?: string; expected_graph_sources?: Record<string, string> };
export type FlowgraphCompanionInventory = {
	root_dir: string;
	files: { path: string; bytes: number }[];
	truncated: boolean;
};

/** Exact preview returned by recovery-plan and required by restore-file. */
export type FlowgraphRecoveryPlan = {
	root_dir: string;
	fq: string;
	backup: string;
	backup_content: string;
	current: { state: "missing" } | { state: "present"; content: string };
};

export type FragmentHeader = {
	schema: string;
	exported_at: string;
	origin?: string | null;
	scope: FragmentScope;
};

export type FragmentFile = {
	path: string;
	meta?: FlowgraphFileMeta | null;
	/** Optional when reading legacy v1 fragments. */
	graph?: FlowgraphGraphDeclaration;
	package?: FlowgraphPackageManifest | null;
	enums?: FlowgraphEnumDef[];
	types?: FlowgraphTypeDef[];
	instances?: FlowgraphGraphInstance[];
	nodes: FlowgraphNodeEntry[];
	edges: FlowgraphEdgeEntry[];
	groups?: FlowgraphGroupDef[];
};

export type FragmentDangling = {
	kind: string;
	original: string;
	reason: string;
	in_file?: string | null;
	edge_from?: string | null;
	edge_to?: string | null;
	external_side?: string | null;
};

export type Fragment = {
	/** serde の `#[serde(rename = "fragment")]` に合わせる。 */
	header: FragmentHeader;
	files: FragmentFile[];
	danglings: FragmentDangling[];
};

export type FragmentCopyResponse = {
	fragment_toml: string;
	fragment: Fragment;
	file_count: number;
	dangling_count: number;
};

export type FragmentOnConflict = "suffix" | "skip" | "overwrite";
export type FragmentOnConflictFile = "overwrite" | "skip" | "rename" | "merge";

export type FragmentPasteOptions = {
	on_conflict_node?: FragmentOnConflict;
	on_conflict_file?: FragmentOnConflictFile;
	position_offset?: [number, number] | null;
	/** value=null は明示的 drop、未指定は unresolved に残る。 */
	remap?: Record<string, string | null>;
};

export type FragmentPasteTarget = { kind: "file"; fq: string } | { kind: "folder"; path: string };

export type FragmentPasteRequest = {
	fragment_toml: string;
	target: FragmentPasteTarget;
	options?: FragmentPasteOptions;
};

export type FragmentPasteWrittenFile = { fq: string; path: string; original_fq?: string };
export type FragmentPasteImportedNode = {
	fq_file: string;
	original_id: string;
	new_id: string;
};
export type FragmentPasteSkippedNode = {
	fq_file: string;
	original_id: string;
	reason: string;
};

export type FragmentPasteReport = {
	written_files: FragmentPasteWrittenFile[];
	imported_nodes: FragmentPasteImportedNode[];
	skipped_nodes: FragmentPasteSkippedNode[];
	skipped_files: string[];
	unresolved_danglings: FragmentDangling[];
	backups: string[];
};

export type FragmentPasteResponse = {
	restart_required?: boolean;
	report: FragmentPasteReport;
	reload_ok: boolean;
	diagnostics_count: number;
	node_count: number;
};

// ZIP Import

export type ZipImportEntry = {
	zip_path: string;
	dest_path: string;
	size: number;
};

export type ZipImportPreview = {
	purpose?: PortablePurpose;
	kind: "preview";
	manifest: FragmentHeader;
	file_count: number;
	entries: ZipImportEntry[];
	danglings: FragmentDangling[];
	target_prefix: string;
	conflicts: string[];
	companions: ZipImportEntry[];
};

export type PortablePurpose = { schema_version: number; name: string; roots: string[] };
export type ImportedPurpose = { name: string; roots: string[]; missing_roots: string[] };

export type ZipImportReport = {
	/** Present on newer servers. Registration is a separate operation. */
	purpose?: ImportedPurpose;
	file_mappings?: FragmentPasteWrittenFile[];
	kind: "report";
	manifest: FragmentHeader;
	written_files: string[];
	written_companions: string[];
	danglings_unresolved: FragmentDangling[];
	target_prefix: string;
	reload_ok?: boolean;
	restart_required?: boolean;
	diagnostics_count?: number;
};

export type ZipImportOutcome = ZipImportPreview | ZipImportReport;

// ---------------------------------------------------------------------------
// Phase φ-1/φ-2/φ-3: Control Table CRUD + Flowgraph Trigger DTOs
//
// Rust 側:
//   - src/web_interface/control/table/mod.rs      (TableCatalogItem / TableFileDto 等)
//   - src/web_interface/control/flowgraph/trigger.rs (TriggerNodeRequest / TriggerNodeResponse)
//   - src/conf/mod.rs                              (ControlTableQuickAdd)
// ---------------------------------------------------------------------------

/** `[[control_api.tables]].quick_add` のサブ設定。 */
export type ControlTableQuickAdd = {
	learn_observe?: { node_id: string; port: string; outcome: "saved" | "unchanged" | "conflict" | "failed" }[];
	forget_observe?: { node_id: string; port: string; outcome: "saved" | "unchanged" | "conflict" | "failed" }[];
	request_format?: "direct" | "control_meta";
	/** `glossary.learn` ノードの fq ID（`"main::learn"` など）。 */
	node_id: string;
	/** 既定の `kind`（`"literal"` | `"regex"` | 省略）。 */
	kind?: string | null;
	/**
	 * Phase φ-4: `glossary.forget` ノードの fq ID。履歴の [Undo] 用。
	 * 未設定なら GUI 側で Undo 不可として扱う。
	 */
	forget_node_id?: string | null;
};

/** `GET /api/v1/control/tables` の 1 エントリ。 */
export type TableCatalogItem = {
	/** URL / allow-list 上のキー。`/api/v1/control/table/{key}`。 */
	key: string;
	/** ディスク上の実パス（表示用）。 */
	path: string;
	label?: string | null;
	/** `"glossary"` | `"scene-registry"` | `"generic"` | 自由文字列。 */
	role?: string | null;
	editable: boolean;
	quick_add?: ControlTableQuickAdd | null;
	/** ファイルが現在ディスクに存在するか。false でも空 Table として GET は成功する。 */
	exists: boolean;
};

export type TableCatalogResponse = {
	tables: TableCatalogItem[];
	count: number;
};

/** TSV 1 行ぶん。`values` は schema の列名 → JSON 値。 */
export type TableEntryDto = {
	row_index: number;
	values: Record<string, unknown>;
};

/** `GET /api/v1/control/table/{key}` のレスポンス。 */
export type TableFileDto = {
	key: string;
	path: string;
	/** 挿入順の列名。 */
	columns: string[];
	rows: TableEntryDto[];
	/**
	 * 現内容の blake3 ハッシュ（64 hex、prefix 無し）。
	 * mutation 系で `If-Match: b3:<content_hash>` として送ると楽観ロックが効く。
	 */
	content_hash: string;
	editable: boolean;
};

/** `PUT /api/v1/control/table/{key}` の body。 */
export type PutTableRequest = {
	/**
	 * 列順を明示する場合に指定。未指定かつ現 Table が空でなければ現 schema 列順を踏襲する。
	 * 現 Table が空（ファイル未存在など）の場合は必須。
	 */
	columns?: string[];
	rows: Array<Record<string, unknown>>;
};

/** `POST /table/{key}/entry` および `PATCH /table/{key}/entry/{row_index}` の body。 */
export type TableEntryRequest = {
	values: Record<string, unknown>;
};

/** mutation 系（PUT / POST / PATCH / DELETE）共通レスポンス。 */
export type TableMutationResponse = {
	ok: boolean;
	/** 書き込み後の新しい content_hash（次の If-Match に使う）。 */
	content_hash: string;
	row_count: number;
	affected_row_index: number | null;
};

// --- Flowgraph Trigger Endpoint (φ-2) ---------------------------------------

/**
 * `POST /api/v1/control/flowgraph/{instance_id}/trigger/{node_id}` の body。
 *
 * `inputs` の value は以下いずれかを受け付ける:
 *   - 生 JSON: `"ドクターウサギ"` / `42` / `true` / ...
 *   - 型注釈ラッパー: `{ type: "string", value: "..." }`（サーバ側で value を取り出して coerce）
 *
 * `exec_port` は複数の exec 入力を持つノード向け。省略時は最初の exec 入力を自動選択（通常 `exec_in`）。
 */
export type TriggerNodeRequest = {
	observe?: [string, string][];
	wait_ms?: number;
	expected_root?: string;
	expected_source_digest?: string;
	inputs?: Record<string, unknown>;
	exec_port?: string;
};

export type TriggerNodeResponse = {
	/** End of this synchronous chain is not necessarily persistence success. */
	receipt?: {
		phase: "pending" | "running" | "finished" | "failed";
		generation: number | null;
		outputs: [string, string][];
	};
	accepted: boolean;
	instance_id: string;
	node_id: string;
	feature: string;
	exec_port: string;
	/** 実際に coerce して送った override key 名（debug 用）。 */
	overridden_inputs: string[];
};

// ---------------------------------------------------------------------------
// エラー型
// ---------------------------------------------------------------------------

export class ControlApiError extends Error {
	constructor(
		public readonly status: number,
		public readonly statusText: string,
		public readonly body: unknown
	) {
		super(`Control API ${status} ${statusText}: ${JSON.stringify(body)}`);
		this.name = "ControlApiError";
	}
}

<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "./api";
	import type { ControlEvent, ControlEventHistoryItem, ControlEventKind } from "./types";

	type HistoryScope = "all" | "ai" | "flowgraph" | "runtime_mode" | "managed_app";

	let loading = $state(false);
	let error = $state<string | null>(null);
	let events = $state<ControlEventHistoryItem[]>([]);
	let scope = $state<HistoryScope>("all");

	const kindByScope: Record<Exclude<HistoryScope, "all">, ControlEventKind[]> = {
		ai: ["ai_activation_decision", "ai_initiative_decision", "ai_turn_decision", "ai_tool_call"],
		flowgraph: ["flowgraph_reloaded", "restart_recommended"],
		runtime_mode: ["runtime_mode_changed", "runtime_mode_managed_apps"],
		managed_app: ["managed_app_state", "runtime_mode_managed_apps"],
	};

	const filtered = $derived.by(() => {
		if (scope === "all") return events;
		const kinds = new Set(kindByScope[scope]);
		return events.filter((item) => kinds.has(item.event.kind));
	});

	async function load(): Promise<void> {
		loading = true;
		error = null;
		try {
			const res = await api.eventHistory(200);
			events = res.events;
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	function summarize(ev: ControlEvent): string {
		switch (ev.kind) {
			case "flowgraph_reloaded":
				return `${ev.ok ? "ok" : "issues"} nodes=${ev.node_count} errors=${ev.error_count} warnings=${ev.warning_count}`;
			case "restart_recommended":
				return ev.reason;
			case "runtime_mode_changed":
				return `${ev.previous_effective_id || "(none)"} -> ${ev.current_effective_id || "(none)"}`;
			case "runtime_mode_managed_apps":
				return ev.ops.map((op) => `${op.op}:${op.id}:${op.ok ? "ok" : "failed"}`).join(", ") || "no ops";
			case "managed_app_state":
				return `${ev.id} ${ev.running ? "running" : "stopped"} pids=[${ev.pids.join(",")}]`;
			case "pause_state":
				return `${ev.target} ${ev.paused ? "paused" : "resumed"}`;
			case "reloaded":
				return `${ev.target}${ev.id ? `:${ev.id}` : ""}`;
			case "processor_invoked":
				return `${ev.feature} ${ev.outcome} ${ev.elapsed_ms}ms`;
			case "channel_datum":
				return `${ev.phase} #${ev.id} ${ev.channel}`;
			case "voice_recognition":
				return `${ev.phase} ${ev.node_id} utterance=${ev.utterance_id} +${ev.elapsed_ms}ms ${ev.text}`;
			case "twitch_chat_received":
				return `${ev.channel_login} @${ev.sender_login} ${ev.text_chars} chars`;
			case "twitch_chat_send_result":
				return `${ev.node_id}: ${ev.outcome === "sent" ? "Twitch送信承認" : ev.outcome === "skipped_empty" ? "空文のため送信なし" : "送信失敗"}`;
			case "twitch_event_sub_received":
				return `${ev.event_type} reward=${ev.reward_title ?? ev.reward_id ?? "-"} actor=${ev.actor_login}`;
			case "voice_input_state":
				return `${ev.muted ? "muted" : "active"} workers=${ev.worker_count}`;
			case "avatar_camera_effect":
				return `カメラ ${ev.target_link_id}: ${ev.phase} ${JSON.stringify(ev.details)}`;
			case "tts_lifecycle":
				return `${ev.phase} ${ev.lane} ${ev.engine}/${ev.voice} utterance=${ev.utterance_id || "-"} ${ev.text_chars} chars ${ev.elapsed_ms}ms${ev.wait_ms === undefined ? "" : ` wait=${ev.wait_ms}ms`}${ev.error ? ` ${ev.error}` : ""}`;
			case "ai_activation_decision":
				return `${ev.persona_id} ${ev.outcome} @${ev.observation_channel}#${ev.observation_datum_id} floor=${ev.speech_floor_wait_ms}ms${ev.score === undefined ? "" : ` score=${ev.score}/${ev.threshold}`}${ev.remaining_interval_secs === undefined ? "" : ` remaining=${ev.remaining_interval_secs}s`}`;
			case "ai_initiative_decision":
				return `${ev.persona_id} ${ev.outcome} idle=${ev.idle_secs}s attempts=${ev.attempts_last_hour}/h speeches=${ev.speeches_last_hour}/h`;
			case "ai_turn_decision":
				return `${ev.persona_id} ${ev.action} ${ev.speak ?? "（発話なし）"}`;
			case "ai_request_context":
				return `${ev.persona_id} 入力 #${ev.observation_datum_id} ${ev.model} 画像=${ev.observations.reduce((n, o) => n + o.input_image_count, 0)}`;
			case "ai_tool_call":
				return `${ev.persona_id} ${ev.tool_name} ${ev.phase}${ev.success === undefined ? "" : ev.success ? " ok" : " failed"} ${ev.elapsed_ms}ms${ev.error ? ` ${ev.error}` : ""}`;
			case "oauth_status":
				return `${ev.account} ${ev.status}`;
			case "restarting":
				return `pid=${ev.new_pid} graceful=${ev.graceful_ms}ms`;
			case "heartbeat":
				return ev.now;
			case "lagged":
				return `dropped ${ev.dropped}`;
		}
	}

	onMount(() => {
		void load();
	});
</script>

<section class="rounded border border-surface-200-800 bg-surface-50-950">
	<div class="flex flex-wrap items-center justify-between gap-2 border-b border-surface-200-800 px-3 py-2">
		<div>
			<h3 class="text-sm font-semibold">イベント履歴</h3>
			<p class="text-xs opacity-60">運用確認用の server-side ring buffer。</p>
		</div>
		<div class="flex items-center gap-2">
			<select class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs" bind:value={scope}>
				<option value="all">すべて</option>
				<option value="ai">AI判断</option>
				<option value="flowgraph">Flowgraph</option>
				<option value="runtime_mode">Runtime Mode</option>
				<option value="managed_app">連携アプリ</option>
			</select>
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-100-900 disabled:opacity-50"
				disabled={loading}
				onclick={() => void load()}
			>
				{loading ? "読み込み中..." : "更新"}
			</button>
		</div>
	</div>

	{#if error}
		<div class="px-3 py-3 text-sm text-error-500">{error}</div>
	{:else if filtered.length === 0}
		<div class="px-3 py-6 text-center text-sm opacity-60">まだイベント履歴はありません。</div>
	{:else}
		<ol class="max-h-72 overflow-y-auto">
			<!-- Identical events can share a timestamp; history rows have no unique server ID. -->
			{#each filtered.toReversed() as item}
				<li class="border-b border-surface-200-800 px-3 py-2 last:border-b-0">
					<div class="flex items-center gap-2 text-[11px] opacity-60">
						<span>{new Date(item.at).toLocaleString()}</span>
						<code>{item.event.kind}</code>
					</div>
					<div class="mt-1 font-mono text-xs break-all">{summarize(item.event)}</div>
				</li>
			{/each}
		</ol>
	{/if}
</section>

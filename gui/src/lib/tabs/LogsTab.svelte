<script lang="ts">
	import FeatureAnchor from "../FeatureAnchor.svelte";
	import { onMount } from "svelte";
	import { api } from "../api";
	import EventHistoryPanel from "../EventHistoryPanel.svelte";
	import EventStream from "../EventStream.svelte";
	import SnapshotView from "../SnapshotView.svelte";
	import UnAppApiCommandHistoryPanel from "../UnAppApiCommandHistoryPanel.svelte";

	let loading = $state(true);
	let loadError = $state<string | null>(null);
	let overviewLoaded = $state(false);
	let flowgraphErrors = $state(0);
	let flowgraphWarnings = $state(0);
	let discoveryErrors = $state(0);
	let endpointWarnings = $state(0);
	let runtimeIssues = $state<import("../types").RuntimeIssue[]>([]);
	let voiceWorkers = $state<import("../types").VoiceWorkerStatusSummary[]>([]);
	let runtimeStatus = $state("確認中");
	let runtimeSummary = $state("状態を取得していない");
	let automationStatus = $state("確認中");
	let unAppStatus = $state("確認中");

	onMount(() => {
		void refreshOverview();
	});

	async function refreshOverview(): Promise<void> {
		loading = true;
		loadError = null;
		try {
			const [nextSnapshot, nextDiagnostics] = await Promise.all([api.snapshot(), api.flowgraphDiagnostics()]);
			const flowgraphDiagnostics = nextDiagnostics.diagnostics ?? [];
			flowgraphErrors = flowgraphDiagnostics.filter((item) => item.severity === "error").length;
			flowgraphWarnings = flowgraphDiagnostics.filter((item) => item.severity === "warning").length;
			discoveryErrors = nextDiagnostics.unaa_z_health.discovery_error_count;
			endpointWarnings = nextDiagnostics.unaa_z_health.endpoint_warning_count;
			runtimeIssues = nextDiagnostics.runtime_issues ?? [];
			voiceWorkers = nextSnapshot.bridges.voice_worker_statuses ?? [];
			const runtimeErrors = runtimeIssues.filter((item) => item.severity === "error").length;
			const runtimeWarnings = runtimeIssues.filter((item) => item.severity === "warning").length;
			runtimeStatus = runtimeErrors > 0 ? "修正が必要" : runtimeWarnings > 0 ? "警告あり" : "稼働中";
			runtimeSummary = `${runtimeErrors} エラー / ${runtimeWarnings} 警告 / AI ${nextSnapshot.ai_personas?.length ?? 0}`;
			automationStatus = flowgraphErrors > 0 ? "修正が必要" : flowgraphWarnings > 0 ? "警告あり" : "正常";
			unAppStatus = discoveryErrors > 0 ? "接続エラー" : endpointWarnings > 0 ? "警告あり" : "正常";
			overviewLoaded = true;
		} catch (error) {
			loadError = error instanceof Error ? error.message : String(error);
			if (!overviewLoaded) {
				runtimeStatus = "不明";
				automationStatus = "不明";
				unAppStatus = "不明";
			}
		} finally {
			loading = false;
		}
	}
</script>

<section class="grid min-w-0 gap-5">
	<header class="flex flex-wrap items-start justify-between gap-3">
		<div>
			<h2 class="text-xl font-semibold">診断</h2>
			<p class="text-sm opacity-65">動作状態を確認し、問題がある箇所の記録を調べる。</p>
		</div>
		<button
			type="button"
			class="rounded border border-surface-300-700 px-3 py-1.5 text-xs hover:bg-surface-100-900 disabled:opacity-50"
			disabled={loading}
			onclick={() => void refreshOverview()}
		>
			{loading ? "確認中..." : "状態を更新"}
		</button>
	</header>

	{#if loadError}
		<div class="rounded border border-error-500 bg-error-100-900 px-4 py-3 text-sm text-error-900-100">
			状態を取得できません: {loadError}
		</div>
	{/if}

	<div class="grid gap-3 md:grid-cols-3">
		<article class="vac-stat-card p-4">
			<div class="vac-stat-label">ランタイム</div>
			<div
				class={`mt-3 text-lg font-semibold ${runtimeIssues.some((item) => item.severity === "error") ? "text-error-500" : runtimeIssues.length > 0 ? "text-warning-500" : "text-success-500"}`}
			>
				{runtimeStatus}
			</div>
			<p class="mt-1 text-xs opacity-60">{runtimeSummary}</p>
		</article>
		<article class="vac-stat-card p-4">
			<div class="vac-stat-label">Flowgraph</div>
			<div
				class={`mt-3 text-lg font-semibold ${flowgraphErrors > 0 ? "text-error-500" : flowgraphWarnings > 0 ? "text-warning-500" : "text-success-500"}`}
			>
				{automationStatus}
			</div>
			<p class="mt-1 text-xs opacity-60">{flowgraphErrors} エラー / {flowgraphWarnings} 警告</p>
		</article>
		<article class="vac-stat-card p-4">
			<div class="vac-stat-label">UNアプリ連携</div>
			<div
				class={`mt-3 text-lg font-semibold ${discoveryErrors > 0 ? "text-error-500" : endpointWarnings > 0 ? "text-warning-500" : "text-success-500"}`}
			>
				{unAppStatus}
			</div>
			<p class="mt-1 text-xs opacity-60">{discoveryErrors} エラー / {endpointWarnings} 警告</p>
		</article>
	</div>

	<div class="grid min-w-0 gap-1">
		{#if runtimeIssues.length > 0}
			<details class="border-t border-surface-200-800" open>
				<summary class="cursor-pointer py-3 text-sm font-semibold">ランタイム問題 ({runtimeIssues.length})</summary>
				<ul class="grid gap-2 pb-4">
					{#each runtimeIssues as item (item.key)}
						<li class="rounded border border-surface-200-800 px-3 py-2 text-sm">
							<div class={item.severity === "error" ? "text-error-500" : "text-warning-500"}>{item.message}</div>
							<p class="mt-1 text-xs opacity-60">影響: {item.impact}</p>
							{#if item.resolution}<p class="mt-1 text-xs">修正手順: {item.resolution}</p>{/if}
							{#if item.node}<p class="mt-1 font-mono text-xs opacity-50">{item.node}</p>{/if}
						</li>
					{/each}
				</ul>
			</details>
		{/if}
		<details class="border-t border-surface-200-800">
			<summary class="cursor-pointer py-3 text-sm font-semibold">音声認識 ({voiceWorkers.length})</summary>
			<div class="grid gap-2 pb-4">
				{#each voiceWorkers as worker (worker.node_id)}
					<article class="grid gap-2 rounded border border-surface-200-800 px-3 py-3 text-sm sm:grid-cols-[minmax(0,1fr)_auto]">
						<div class="min-w-0">
							<div class="font-semibold">{worker.engine} / {worker.active_backend}</div>
							<div class="truncate font-mono text-xs opacity-55" title={worker.node_id}>{worker.node_id}</div>
							{#if worker.last_error}<p class="mt-2 text-xs text-error-500">{worker.last_error}</p>{/if}
						</div>
						<div class="grid grid-cols-2 gap-x-5 gap-y-1 text-xs sm:text-right">
							<span class="opacity-55">途中推論</span><span>{worker.partial_attempt_count} 回</span>
							<span class="opacity-55">字幕更新</span><span>{worker.partial_count} 回</span>
							{#if worker.engine === "whisper"}
								<span class="opacity-55">途中認識設定</span><span
									>{worker.partial_min_audio_ms} ms後 / {worker.partial_interval_ms} ms周期</span
								>
								<span class="opacity-55">周期超過</span>
								<span class={worker.partial_deadline_miss_count > 0 ? "text-warning-500" : ""}
									>{worker.partial_deadline_miss_count} 回</span
								>
							{/if}
							<span class="opacity-55">確定認識</span><span>{worker.final_count} 回</span>
							<span class="opacity-55">認識エラー</span><span class={worker.error_count > 0 ? "text-error-500" : ""}
								>{worker.error_count} 回</span
							>
							{#if worker.engine === "whisper"}
								<span class="opacity-55">直近の途中推論</span>
								<span
									class={worker.last_partial_inference_ms != null &&
									worker.partial_interval_ms > 0 &&
									worker.last_partial_inference_ms > worker.partial_interval_ms
										? "text-warning-500"
										: ""}
									>{worker.last_partial_inference_ms == null
										? "未計測"
										: `${worker.last_partial_inference_ms} / ${worker.partial_interval_ms} ms`}</span
								>
							{/if}
							<span class="opacity-55">直近の確定推論</span>
							<span>{worker.last_final_inference_ms == null ? "未計測" : `${worker.last_final_inference_ms} ms`}</span>
						</div>
					</article>
				{:else}
					<p class="text-xs opacity-60">稼働中の音声認識はない。</p>
				{/each}
			</div>
		</details>
		<details class="border-t border-surface-200-800">
			<summary class="cursor-pointer py-3 text-sm font-semibold">ランタイム状態</summary>
			<div class="min-w-0 pb-4"><FeatureAnchor id="snapshot"><SnapshotView /></FeatureAnchor></div>
		</details>
		<details class="border-t border-surface-200-800">
			<summary class="cursor-pointer py-3 text-sm font-semibold">ライブイベント</summary>
			<div class="min-w-0 pb-4"><FeatureAnchor id="events"><EventStream /></FeatureAnchor></div>
		</details>
		<details class="border-t border-surface-200-800">
			<summary class="cursor-pointer py-3 text-sm font-semibold">UNAA/Zの検出とコマンド履歴</summary>
			<div class="min-w-0 pb-4"><FeatureAnchor id="commands"><UnAppApiCommandHistoryPanel /></FeatureAnchor></div>
		</details>
		<details class="border-y border-surface-200-800">
			<summary class="cursor-pointer py-3 text-sm font-semibold">イベント履歴</summary>
			<div class="min-w-0 pb-4"><FeatureAnchor id="history"><EventHistoryPanel /></FeatureAnchor></div>
		</details>
	</div>
</section>

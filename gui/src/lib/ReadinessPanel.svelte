<script lang="ts">
	import { onMount } from "svelte";
	import { runtimeSnapshotStore } from "./runtimeSnapshot.svelte";
	import { tabNavStore, type TabId } from "./tabs.svelte";
	import type { ReadinessCheck, ReadinessStatus } from "./types";
	import { visibleReadinessStatus } from "./readinessPresentation";

	let {
		requiredOnly = false,
		summaryOnly = false,
		currentTab,
	}: { requiredOnly?: boolean; summaryOnly?: boolean; currentTab: TabId } = $props();

	onMount(() => {
		void runtimeSnapshotStore.refresh().catch(() => undefined);
	});

	const readiness = $derived(runtimeSnapshotStore.snapshot?.readiness ?? null);
	const checks = $derived((readiness?.checks ?? []).filter((item) => !requiredOnly || item.required));
	const displayStatus = $derived(readiness ? visibleReadinessStatus(readiness, requiredOnly) : null);
	const blocking = $derived(checks.filter((item) => item.status === "blocked").length);
	const attention = $derived(checks.filter((item) => item.status === "attention").length);
	const unverified = $derived(checks.filter((item) => item.status === "unverified").length);
	const visibleChecks = $derived(
		summaryOnly ? checks.filter((item) => item.status === "blocked" || item.status === "attention" || item.status === "unverified") : checks
	);
	const summaryLabel = $derived(
		readiness ? blocking > 0 ? `${blocking}件の対応が必要` : attention > 0 ? `${attention}件を確認` : unverified > 0 ? `利用可能・${unverified}件は未確認` : statusLabel(readiness.status) : "確認中…"
	);

	function statusLabel(status: ReadinessStatus): string {
		switch (status) {
			case "ready":
				return "準備完了";
			case "attention":
				return "要確認";
			case "blocked":
				return "要対応";
			case "inactive":
				return "未使用";
			case "unverified":
				return "未確認";
		}
	}

	function statusClass(status: ReadinessStatus): string {
		switch (status) {
			case "ready":
				return "text-success-500";
			case "attention":
				return "text-warning-500";
			case "blocked":
				return "text-error-500";
			case "inactive":
				return "opacity-45";
			case "unverified":
				return "opacity-65";
		}
	}

	function actionTab(check: ReadinessCheck): TabId | null {
		const action = check.action;
		if (action === "live" || action === "resources" || action === "flowgraph" || action === "observability") return action;
		return null;
	}
</script>

<section class="vac-panel" aria-label={summaryOnly ? "利用状態" : "配信前チェック"}>
	<header class="vac-panel-header flex flex-wrap items-center justify-between gap-2 px-4 py-2.5">
		<div class="flex min-w-0 items-center gap-3">
			<h3 class="text-sm font-semibold">{summaryOnly ? "利用状態" : "配信前チェック"}</h3>
			{#if runtimeSnapshotStore.error}
				<span class="text-xs font-semibold text-warning-500">更新未確認{readiness ? `（前回: ${summaryLabel}）` : ""}</span>
			{:else if readiness}
				<span class={`text-xs font-semibold ${statusClass(displayStatus ?? readiness.status)}`}>
					{summaryLabel}
				</span>
			{:else}
				<span class="text-xs opacity-50">確認中…</span>
			{/if}
		</div>
		{#if summaryOnly}
			<button
				type="button"
				class="text-xs text-primary-600-400 hover:underline"
				onclick={() => tabNavStore.setActive("observability")}>診断を見る</button
			>
		{:else}
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2.5 py-1 text-xs hover:bg-surface-100-900 disabled:opacity-50"
				disabled={runtimeSnapshotStore.loading}
				onclick={() => void runtimeSnapshotStore.refresh().catch(() => undefined)}
			>
				{runtimeSnapshotStore.loading ? "確認中…" : "更新"}
			</button>
		{/if}
	</header>

	{#if runtimeSnapshotStore.error}
		<p class="border-t border-error-500/40 bg-error-500/10 px-4 py-3 text-sm text-error-500">
			状態を取得できない: {runtimeSnapshotStore.error}
		</p>
	{:else if readiness && visibleChecks.length > 0}
		<ul class="grid divide-y divide-surface-200-800 lg:grid-cols-2 lg:divide-y-0">
			{#each visibleChecks as check, index (check.id)}
				{@const target = actionTab(check)}
				{@const spansLastRow = visibleChecks.length % 2 === 1 && index === visibleChecks.length - 1}
				<li
					class={`flex min-h-16 items-center gap-3 px-4 py-2.5 ${index >= 2 ? "lg:border-t lg:border-surface-200-800" : ""} ${index % 2 === 1 ? "lg:border-l lg:border-surface-200-800" : ""} ${spansLastRow ? "lg:col-span-2" : ""}`}
				>
					<span class={`w-16 shrink-0 text-xs font-semibold ${statusClass(check.status)}`}>{statusLabel(check.status)}</span>
					<div class="min-w-0 flex-1">
						<p class="text-sm font-medium">{check.title}</p>
						<p class="text-xs opacity-60">{check.detail}</p>
					</div>
					{#if target && target !== currentTab && check.status !== "ready"}
						<button
							type="button"
							class="shrink-0 text-xs text-primary-600-400 hover:underline"
							onclick={() => tabNavStore.setActive(target)}
						>
							確認
						</button>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</section>

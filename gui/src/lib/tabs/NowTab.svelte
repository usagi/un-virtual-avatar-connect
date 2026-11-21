<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "../api";
	import { eventsStore } from "../events.svelte";
	import { runtimeSnapshotStore } from "../runtimeSnapshot.svelte";
	import VoiceInputControl from "../VoiceInputControl.svelte";
	import InstalledFeatureSummary from "../flowgraph/InstalledFeatureSummary.svelte";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	import RuntimeModeControl from "../RuntimeModeControl.svelte";
	import LiveAiControlPanel from "../live/LiveAiControlPanel.svelte";
	import ReadinessPanel from "../ReadinessPanel.svelte";
	import { visibleReadinessStatus } from "../readinessPresentation";
	import { twitchInputPresentation } from "../twitchInputPresentation";
	import { tabNavStore, type RepairFocus, type TabId } from "../tabs.svelte";
	import type { FlowgraphDiagnosticsResponse, ManagedAppsResponse } from "../types";

	type ConfirmationItem = RepairFocus & {
		action: TabId;
	};

	let currentMode = $state<string | null>(null);
	let managedApps = $state<ManagedAppsResponse | null>(null);
	let diagnostics = $state<FlowgraphDiagnosticsResponse | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let failedSources = $state<string[]>([]);
	let refreshRequest = 0;
	function readHomePreview(): boolean {
		if (import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1") return true;
		if (typeof window === "undefined") return false;
		const params = new URL(window.location.href).searchParams;
		const layout = document.documentElement.dataset.previewHomeLayout ?? params.get("homeLayout");
		return layout ? layout === "operations" : params.get("home") === "active";
	}
	let activeHomePreview = $state(readHomePreview());
	const unusedAi = $derived(activeHomePreview && runtimeSnapshotStore.snapshot !== null && !runtimeSnapshotStore.error && runtimeSnapshotStore.snapshot.ai_personas.length === 0);

	async function refreshNow() {
		const request = ++refreshRequest;
		loading = true;
		const results = await Promise.allSettled([
			runtimeSnapshotStore.refresh(),
			api.currentMode().then((value) => {
				if (request === refreshRequest) currentMode = value.mode;
				return value;
			}),
			api.managedApps().then((value) => {
				if (request === refreshRequest) managedApps = value;
				return value;
			}),
			api.flowgraphDiagnostics().then((value) => {
				if (request === refreshRequest) diagnostics = value;
				return value;
			}),
			flowgraphStore.refreshTree(),
		]);
		if (request !== refreshRequest) return;
		failedSources = results.flatMap((result, index) => result.status === "rejected" ? [["稼働状態", "動作モード", "連携アプリ", "診断", "保存済み機能"][index]] : []);
		error = failedSources.length ? `確認できない項目: ${failedSources.join("、")}。取得できた情報は表示しています。` : null;
		loading = false;
	}

	onMount(() => {
		// The isolated comparison toolbar changes layout without reloading the
		// application, so mode drafts and the current target remain intact.
		const previewObserver = import.meta.env.VITE_UN_DESIGN_PREVIEW === "1"
			? new MutationObserver(() => { activeHomePreview = readHomePreview(); }) : null;
		previewObserver?.observe(document.documentElement, { attributes: true, attributeFilter: ["data-preview-home-layout"] });
		activeHomePreview = readHomePreview();
		void refreshNow();
		const unsubscribe = eventsStore.subscribe((event) => {
			if (
				event.event.kind === "managed_app_state" ||
				event.event.kind === "flowgraph_reloaded" ||
				event.event.kind === "runtime_mode_changed"
			) {
				void refreshNow();
			}
		});
		return () => { ++refreshRequest; unsubscribe(); previewObserver?.disconnect(); };
	});

	const twitchInputs = $derived(runtimeSnapshotStore.snapshot?.bridges.twitch_irc_ingresses ?? []);
	const twitchPresentation = $derived(twitchInputPresentation(twitchInputs));
	const managedTotal = $derived(managedApps?.entries.length ?? 0);
	const managedRunning = $derived(managedApps?.entries.filter((app) => app.status.running).length ?? 0);
	const runningAppLabels = $derived(managedApps?.entries.filter((app) => app.status.running).map((app) => app.label) ?? []);
	const confirmationItems = $derived.by((): ConfirmationItem[] => {
		const optionalOutsideStreaming = new Set(["twitch_irc", "twitch_eventsub", "stream_perception"]);
		const items: ConfirmationItem[] = (diagnostics?.runtime_issues ?? [])
			// AI response health is shown by the periodically refreshed ReadinessPanel.
			// Keeping a second copy here would retain old errors after recovery.
			.filter((item) => item.component !== "ai_reaction")
			.filter((item) => currentMode === null || failedSources.includes("動作モード") || currentMode === "streaming" || !optionalOutsideStreaming.has(item.component))
			.map((item) => ({
				key: item.key,
				severity: item.severity,
				message: item.message,
				impact: item.impact,
				resolution: item.resolution,
				component: item.component,
				code: item.code,
				node: item.node,
				action: (["resources", "flowgraph", "settings", "observability"] as TabId[]).includes(item.action as TabId)
					? (item.action as TabId)
					: "observability",
			}));
		for (const [index, item] of (diagnostics?.diagnostics ?? []).entries()) {
			items.push({
				key: `flowgraph:${item.code}:${item.node ?? ""}:${index}`,
				severity: item.severity,
				message: item.message,
				code: item.code,
				file: item.file,
				node: item.node,
				action: "flowgraph",
			});
		}
		return items.slice(0, 4);
	});
	const health = $derived.by(() => {
		if (error || runtimeSnapshotStore.error || eventsStore.connection === "closed" || eventsStore.connection === "error") {
			return { label: "アプリの状態を確認してください", tone: "text-error-500" };
		}
		if (!runtimeSnapshotStore.snapshot) return { label: "状態を確認中…", tone: "opacity-65" };
		switch (visibleReadinessStatus(runtimeSnapshotStore.snapshot.readiness)) {
			case "blocked":
				return { label: "対応が必要な項目があります", tone: "text-error-500" };
			case "attention":
				return { label: "確認事項があります", tone: "text-warning-500" };
		}
		return { label: "利用できます", tone: "text-success-500" };
	});

	function go(tab: TabId) {
		tabNavStore.setActive(tab);
	}

	function repair(item: ConfirmationItem) {
		tabNavStore.openRepair(item.action, item);
	}
</script>

<section class="grid gap-5">
	<header class="flex flex-wrap items-start justify-between gap-3">
		<div>
			<h2 class="text-xl font-semibold">ホーム</h2>
			<p class={`text-sm ${health.tone}`}>{health.label}</p>
		</div>
		<button
			type="button"
			class="rounded border border-surface-300-700 px-3 py-1.5 text-xs hover:bg-surface-100-900 disabled:opacity-50"
			disabled={loading}
			onclick={() => void refreshNow()}
		>
			{loading ? "更新中…" : "更新"}
		</button>
	</header>

	<ReadinessPanel currentTab="now" summaryOnly />
	<div class="flex flex-wrap items-center gap-3 text-xs">
		<button class="rounded border border-primary-500 px-3 py-2" onclick={() => tabNavStore.openFeature("flowgraph", "feature-library")}>用途から機能を追加</button>
		<span class="opacity-65">通知・クリップ紹介・読み上げ・自動化。AIは必要な場合だけ追加できます。</span>
	</div>

	<div class="grid items-start gap-4 xl:grid-cols-2">
		<RuntimeModeControl compact />
		<VoiceInputControl />
	</div>

	{#if error}
		<div class="rounded border border-error-500/50 bg-error-500/10 px-3 py-2 text-sm text-error-600-400">{error}</div>
	{/if}

	{#if unusedAi}
		<details class="vac-panel" aria-label="未使用のAI機能">
			<summary class="cursor-pointer px-4 py-3 text-sm">
				<span class="font-semibold">AIパートナー</span>
				<span class="ml-2 opacity-65">未使用 · 必要なときに追加</span>
			</summary>
			<LiveAiControlPanel />
		</details>
	{:else}
		<LiveAiControlPanel />
	{/if}
	<InstalledFeatureSummary diagnostics={failedSources.includes("診断") ? null : diagnostics} />

	<div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
		<article class="vac-stat-card flex min-h-32 flex-col p-4">
			<div class="vac-stat-label">Twitchチャット入力</div>
			{#if runtimeSnapshotStore.error}<p class="text-xs text-warning-500">更新未確認{runtimeSnapshotStore.snapshot ? " · 前回の情報" : ""}</p>{/if}
			<div class="mt-3 text-lg font-semibold">
				{runtimeSnapshotStore.snapshot === null
					? "確認中…"
					: twitchPresentation.label}
			</div>
			<p class="mt-1 line-clamp-2 text-xs opacity-60">
				{runtimeSnapshotStore.snapshot === null ? "入力構成をまだ確認できていません" : twitchPresentation.detail}
			</p>
			<div class="mt-auto flex flex-wrap gap-x-4 gap-y-1 pt-2">
				<button type="button" class="text-xs text-primary-600-400 hover:underline" onclick={() => tabNavStore.openFeature("flowgraph", "feature-inventory")}
					>構成を見る</button
				>
				<button type="button" class="text-xs text-primary-600-400 hover:underline" onclick={() => tabNavStore.openFeature("resources", "twitch")}
					>認証を管理</button
				>
			</div>
		</article>

		<article class="vac-stat-card flex min-h-32 flex-col p-4">
			<div class="vac-stat-label">連携アプリ</div>
			{#if failedSources.includes("連携アプリ")}<p class="text-xs text-warning-500">更新未確認{managedApps ? " · 前回の情報" : ""}</p>{/if}
			<div class="mt-3 text-lg font-semibold">
				{managedApps === null ? (failedSources.includes("連携アプリ") ? "確認できません" : "確認中…") : managedRunning > 0 ? `${managedRunning}個を使用中` : managedTotal > 0 ? "停止中" : "未設定"}
			</div>
			<p class="mt-1 line-clamp-2 text-xs opacity-60">
				{managedApps === null ? "登録内容をまだ確認できていません" : runningAppLabels.length > 0
					? runningAppLabels.join("、")
					: managedTotal > 0
						? "必要なときに起動できます"
						: "外部アプリは登録されていません"}
			</p>
			<button type="button" class="mt-auto self-start text-xs text-primary-600-400 hover:underline" onclick={() => tabNavStore.openFeature("resources", "run-with")}
				>管理する</button
			>
		</article>


	</div>


	{#if confirmationItems.length > 0}
		<section class="vac-panel">
			{#if failedSources.includes("診断")}<p class="px-4 py-2 text-xs text-warning-500">診断は更新未確認 · 前回の確認事項</p>{/if}
			<header class="vac-panel-header flex items-center justify-between px-4 py-2">
				<h3 class="text-sm font-semibold">確認事項</h3>
				<button type="button" class="text-xs text-primary-600-400 hover:underline" onclick={() => go("observability")}>詳細</button>
			</header>
			<ul class="divide-y divide-surface-200-800">
				{#each confirmationItems as item (item.key)}
					<li class="px-4 py-3 text-sm">
						<div class={item.severity === "error" ? "text-error-500" : "text-warning-500"}>
							{item.severity === "error" ? "対応が必要" : item.severity === "warning" ? "要確認" : "お知らせ"}
						</div>
						<p class="mt-1">{item.message}</p>
						{#if item.impact}<p class="mt-1 text-xs opacity-60">影響: {item.impact}</p>{/if}
						<button type="button" class="mt-2 text-xs text-primary-600-400 hover:underline" onclick={() => repair(item)}>
							確認する
						</button>
					</li>
				{/each}
			</ul>
		</section>
	{/if}
</section>

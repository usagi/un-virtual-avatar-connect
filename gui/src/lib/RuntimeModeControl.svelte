<script lang="ts">
	let { compact = false }: { compact?: boolean } = $props();
	import { onMount } from "svelte";
	import { api } from "./api";
	import { eventsStore } from "./events.svelte";
	import {
		buildDisplayModes,
		capabilityPolicyRows,
		capabilityPolicyWarnings,
		formatControlApiError,
		joinList,
		managedDirectiveRows,
	} from "./control/runtimeModes";
	import { type ModeTransitionPlan, type RuntimeModeManagedAppOp, type RuntimeModeTransitionStatus } from "./types";
	import { capabilityLabel } from "./flowgraph/effectMetadata";

	let configuredModes = $state<import("./types").RuntimeModeSummary[]>([]);
	let currentModeId = $state<string | null>(null);
	let selectedModeId = $state("");
	let directBusy = $state(false);
	let confirmation = $state<{ target: string; plan: string } | null>(null);
	let loading = $state(true);
	let loadError = $state<string | null>(null);
	let mutating = $state(false);
	let mutationError = $state<string | null>(null);
	let planLoading = $state(false);
	let planError = $state<string | null>(null);
	let transitionPlan = $state<ModeTransitionPlan | null>(null);
	let transitionStatus = $state<RuntimeModeTransitionStatus | null>(null);
	let managedAppOps = $state<RuntimeModeManagedAppOp[]>([]);
	let showTransitionFeedback = $state(false);
	let completionHideTimer: number | null = null;
	let planRequestSeq = 0;
	let modesRequestSeq = 0;
	let statusRequestSeq = 0;

	const displayModes = $derived(buildDisplayModes(configuredModes));

	const selectedMode = $derived(displayModes.find((m) => m.id === selectedModeId) ?? displayModes[0]);
	const managedDesiredRows = $derived(managedDirectiveRows(transitionPlan));
	const capabilityPolicyPreviewRows = $derived(capabilityPolicyRows(transitionPlan));
	const capabilityWarnings = $derived(capabilityPolicyWarnings(transitionPlan));
	const transitionPercent = $derived.by(() => {
		if (!transitionStatus || transitionStatus.step_count <= 0) return 0;
		return Math.min(100, Math.round((transitionStatus.step_index / transitionStatus.step_count) * 100));
	});
	onMount(() => {
		void refreshModes();
		void refreshTransitionStatus();
		const timer = window.setInterval(() => {
			void refreshTransitionStatus();
		}, 1200);
		const unsubscribe = eventsStore.subscribe((event) => {
			if (event.event.kind === "runtime_mode_changed") void refreshModes();
		});
		return () => {
			modesRequestSeq += 1;
			statusRequestSeq += 1;
			planRequestSeq += 1;
			unsubscribe();
			window.clearInterval(timer);
			if (completionHideTimer !== null) window.clearTimeout(completionHideTimer);
		};
	});

	$effect(() => {
		const target = selectedMode?.id ?? null;
		const current = currentModeId;
		if (!target || loadError) {
			planRequestSeq += 1;
			planLoading = false;
			transitionPlan = null;
			planError = null;
			return;
		}
		void current;
		void refreshTransitionPlan(target);
	});

	async function refreshModes(preserveSelection = false): Promise<void> {
		const seq = ++modesRequestSeq;
		loading = true;
		loadError = null;
		try {
			const [list, current] = await Promise.all([api.modesList(), api.currentMode()]);
			if (seq !== modesRequestSeq) return;
			configuredModes = list.modes;
			currentModeId = current.mode;
			if (!preserveSelection) {
				if (current.mode) selectedModeId = current.mode;
				else if (list.modes.length > 0) selectedModeId = list.modes[0].id;
			}
		} catch (e) {
			if (seq !== modesRequestSeq) return;
			loadError = formatControlApiError(e);
		} finally {
			if (seq === modesRequestSeq) loading = false;
		}
	}

	async function requestMode(target: string, confirmedPlan?: string): Promise<void> {
		if (loading || loadError || mutating || directBusy || transitionStatus?.active || target === currentModeId || !displayModes.some(m => m.id === target)) return;
		directBusy = true;
		mutationError = null;
		confirmation = null;
		selectedModeId = target;
		try {
			const plan = await api.modePlan({ target });
			const signature = JSON.stringify(plan);
			if (plan.to_effective_id !== target) throw new Error("切替対象が一致しません。再確認してください。");
			if ((plan.target_managed_apps.stop.length > 0 || plan.capability_denied_by_target.length > 0 || plan.capability_unlisted_by_target.length > 0) && signature !== confirmedPlan) {
				confirmation = { target, plan: signature };
				transitionPlan = plan;
				return;
			}
			mutating = true;
			showTransitionFeedback = true;
			const result = await api.modeTransit({ mode: target, dry_run: false, reason: "gui" });
			currentModeId = result.mode;
			managedAppOps = result.managed_apps ?? [];
			await refreshTransitionStatus();
		} catch (e) {
			mutationError = formatControlApiError(e);
		} finally {
			mutating = false;
			directBusy = false;
		}
	}

	async function refreshTransitionStatus(): Promise<void> {
		const seq = ++statusRequestSeq;
		try {
			const status = await api.modeTransition();
			if (seq !== statusRequestSeq) return;
			transitionStatus = status;
			if (!status.active && status.phase === "completed") {
				if (status.managed_apps.length > 0) managedAppOps = status.managed_apps;
				if (showTransitionFeedback && completionHideTimer === null) {
					completionHideTimer = window.setTimeout(() => {
						showTransitionFeedback = false;
						completionHideTimer = null;
					}, 3000);
				}
			}
		} catch {
			// The main Runtime Mode API status already reports backend availability.
		}
	}

	async function refreshTransitionPlan(target: string): Promise<void> {
		const seq = ++planRequestSeq;
		planLoading = true;
		planError = null;
		transitionPlan = null;
		try {
			const plan = await api.modePlan({ target });
			if (seq !== planRequestSeq) return;
			transitionPlan = plan;
		} catch (e) {
			if (seq !== planRequestSeq) return;
			transitionPlan = null;
			planError = formatControlApiError(e);
		} finally {
			if (seq === planRequestSeq) planLoading = false;
		}
	}
</script>

<section class={compact ? "vac-panel grid gap-3 p-4" : "grid gap-4"}>
	<div class="flex flex-wrap items-start justify-between gap-3">
		<div>
			<h2 class={compact ? "text-base font-semibold" : "text-xl font-semibold"}>動作モード</h2>
			{#if !compact}<p class="text-sm opacity-65">用途に合わせて、処理と連携アプリの動作をまとめて切り替える。</p>{/if}
		</div>
		<div class="vac-panel px-3 py-2 text-xs">
			<span class="opacity-60">状態:</span>
			<span
				class="ml-1 font-semibold"
				class:text-warning-500={loading}
				class:text-error-500={loadError !== null}
				class:text-success-500={!loading && loadError === null}
			>
				{loading ? "確認中" : loadError ? "接続エラー" : "利用可能"}
			</span>
		</div>
	</div>

	<div class={compact ? "text-sm" : "vac-panel px-4 py-3 text-sm"}>
		<span class="font-semibold">現在のモード:</span>
		<span class="ml-2">{loading ? "確認中…" : displayModes.find((mode) => mode.id === currentModeId)?.label ?? currentModeId ?? "モード未選択"}</span>
		{#if !compact}<span class="ml-3 opacity-65">利用可能: {configuredModes.length}</span>{/if}
	</div>

	{#if loadError}
		<div class="rounded border border-error-500 bg-error-100-900 px-4 py-3 text-sm text-error-900-100">
			動作モードを取得できません: {loadError}
			<button type="button" class="ml-3 underline" disabled={loading} onclick={() => refreshModes(true)}>再取得</button>
		</div>
	{/if}
	{#if planError && !loadError}
		<div class="rounded border border-error-500 px-4 py-3 text-sm text-error-500">
			切替内容を取得できません: {planError}
			<button type="button" class="ml-3 underline" disabled={planLoading || !selectedMode} onclick={() => selectedMode && refreshTransitionPlan(selectedMode.id)}>再取得</button>
		</div>
	{/if}
	{#if mutationError}
		<div class="rounded border border-error-500 bg-error-100-900 px-4 py-3 text-sm text-error-900-100">
			動作モードの切替に失敗しました: {mutationError}
		</div>
	{/if}
	{#if transitionStatus && (transitionStatus.active || transitionStatus.phase === "failed" || showTransitionFeedback)}
		<section class="vac-panel px-4 py-3">
			<div class="flex flex-wrap items-center justify-between gap-2">
				<div>
					<div class="text-sm font-semibold">遷移進捗</div>
					<div class="mt-1 text-xs opacity-65">
						{transitionStatus.message}
						<span class="ml-2 font-mono">{transitionStatus.phase}</span>
					</div>
				</div>
				<div class="text-xs opacity-65">
					step {transitionStatus.step_index}/{transitionStatus.step_count}
				</div>
			</div>
			<div class="mt-3 h-2 overflow-hidden rounded bg-surface-200-800">
				<div
					class="h-full {transitionStatus.phase === 'failed' ? 'bg-error-500' : 'bg-primary-500'}"
					style={`width: ${transitionPercent}%`}
				></div>
			</div>
			{#if transitionStatus.error}
				<div class="mt-2 text-xs text-error-500">{transitionStatus.error}</div>
			{/if}
		</section>
	{/if}

	<div class={compact ? "grid gap-3" : "grid gap-4 xl:grid-cols-[minmax(0,1fr)_360px]"}>
		<div class="mode-grid" role="group" aria-label="動作モードを切り替える">
			{#each displayModes as mode (mode.id)}
				<button type="button" class="mode-button" class:mode-current={currentModeId === mode.id}
					aria-pressed={currentModeId === mode.id}
					disabled={loading || loadError !== null || mutating || directBusy || transitionStatus?.active === true || confirmation !== null || currentModeId === mode.id}
					onclick={() => void requestMode(mode.id)}>
					<strong>{mode.label}</strong>
					<span>{currentModeId === mode.id ? "✓ 使用中" : directBusy && selectedModeId === mode.id ? "切替内容を確認中…" : "このモードへ切替"}</span>
				</button>
			{/each}
		</div>
		{#if confirmation}
			<section class="vac-panel p-4" aria-label="モード切替の確認">
				<p class="font-semibold">{displayModes.find(m => m.id === confirmation?.target)?.label}への切替を確認</p>
				<p class="mt-2 text-sm">終了要求または機能の許可範囲の変更を含みます。切替内容の詳細を確認してください。</p>
				<p class="mt-2 break-words text-sm">終了要求: {JSON.parse(confirmation.plan).target_managed_apps.stop.join("、") || "なし"}</p>
				<div class="mt-3 flex gap-3">
					<button class="btn preset-filled-primary-500" onclick={() => confirmation && void requestMode(confirmation.target, confirmation.plan)}>確認して切り替える</button>
					<button class="btn preset-outlined" onclick={() => confirmation = null}>キャンセル</button>
				</div>
			</section>
		{/if}
		<aside class={compact ? "" : "vac-panel"}>
			{#if !compact}<div class="vac-panel-header px-4 py-2 text-sm font-semibold">選択中のモード</div>{/if}
			<div class={compact ? "grid gap-3 text-sm" : "grid gap-4 p-4 text-sm"}>
				{#if selectedMode}
					<details class="border-t border-surface-200-800 pt-3">
						<summary class="cursor-pointer text-xs font-semibold opacity-70">切替内容の詳細</summary>
						<div class="mt-3 grid gap-4">
							<div>
								<div class="text-xs uppercase tracking-wide opacity-60">Target</div>
								<div class="mt-1 text-lg font-semibold">{selectedMode.label}</div>
								<p class="mt-1 text-xs opacity-70">{selectedMode.description}</p>
							</div>

							<div>
								<div class="mb-1 text-xs font-semibold opacity-70">Flowgraph</div>
								{#if transitionPlan}
									<dl class="grid grid-cols-[64px_minmax(0,1fr)] gap-x-2 gap-y-1 text-xs">
										<dt class="opacity-60">Enable</dt>
										<dd class="truncate font-mono">{joinList(transitionPlan.target_flowgraph_groups.enable)}</dd>
										<dt class="opacity-60">Disable</dt>
										<dd class="truncate font-mono">{joinList(transitionPlan.target_flowgraph_groups.disable)}</dd>
									</dl>
								{:else}
									<div class="text-xs opacity-60">切替内容を確認中。</div>
								{/if}
							</div>

							{#if transitionPlan}
								<div>
									<div class="mb-1 text-xs font-semibold opacity-70">Capability policy preview</div>
									{#if capabilityPolicyPreviewRows.length > 0}
										<dl class="grid grid-cols-[64px_minmax(0,1fr)] gap-x-2 gap-y-1 text-xs">
											{#each capabilityPolicyPreviewRows as row (row.label)}
												<dt class="opacity-60">{row.label}</dt>
												<dd class="truncate font-mono">
													{row.values.map((capability) => capabilityLabel(capability)).join(", ")}
												</dd>
											{/each}
										</dl>
									{:else}
										<div class="vac-subtle-row px-2 py-1 text-xs opacity-60">Capability policy は未指定です。</div>
									{/if}
									{#if capabilityWarnings.length > 0}
										<div class="mt-2 grid gap-1">
											{#each capabilityWarnings as warning (warning.kind + warning.capability)}
												<div class="rounded border border-warning-500/50 px-2 py-1 text-xs text-warning-700-300">
													{warning.kind === "deny" ? "Deny と衝突" : "Allow 外"}: {capabilityLabel(
														warning.capability
													)}
												</div>
											{/each}
										</div>
									{/if}
								</div>
							{/if}

							<div class="vac-panel-muted p-3">
								<div class="mb-2 flex items-center justify-between gap-2">
									<div class="text-xs font-semibold opacity-70">切替計画</div>
									<div class="text-[10px] uppercase opacity-55">
										{planLoading ? "loading" : transitionPlan?.noop ? "noop" : "change"}
									</div>
								</div>
								{#if planError}
									<div class="text-xs text-error-500">{planError}</div>
								{:else if transitionPlan}
									<dl class="grid grid-cols-[76px_minmax(0,1fr)] gap-x-2 gap-y-1 text-xs">
										<dt class="opacity-60">From</dt>
										<dd class="truncate font-mono">{transitionPlan.from_effective_id || "(none)"}</dd>
										<dt class="opacity-60">To</dt>
										<dd class="truncate font-mono">{transitionPlan.to_effective_id || "(none)"}</dd>
										<dt class="opacity-60">Enable +</dt>
										<dd class="truncate font-mono">
											{transitionPlan.flowgraph_enable_added_vs_from.length > 0
												? transitionPlan.flowgraph_enable_added_vs_from.join(", ")
												: "-"}
										</dd>
										<dt class="opacity-60">Disable +</dt>
										<dd class="truncate font-mono">
											{transitionPlan.flowgraph_disable_added_vs_from.length > 0
												? transitionPlan.flowgraph_disable_added_vs_from.join(", ")
												: "-"}
										</dd>
									</dl>
								{:else}
									<div class="text-xs opacity-60">切替計画を確認中。</div>
								{/if}
							</div>

							<div>
								<div class="mb-1 text-xs font-semibold opacity-70">連携アプリ</div>
								{#if managedDesiredRows.length > 0}
									<dl class="grid grid-cols-[64px_minmax(0,1fr)] gap-x-2 gap-y-1 text-xs">
										{#each managedDesiredRows as row (row.label)}
											<dt class="opacity-60">{row.label}</dt>
											<dd class="truncate font-mono">{joinList(row.values)}</dd>
										{/each}
									</dl>
								{:else if transitionPlan}
									<div class="vac-subtle-row px-2 py-1 text-xs opacity-60">Managed App の変更はありません。</div>
								{:else}
									<div class="text-xs opacity-60">切替内容を確認中。</div>
								{/if}
							</div>
						</div>
					</details>

					{#if managedAppOps.length > 0}
						<div>
							<div class="mb-1 text-xs font-semibold opacity-70">前回の連携アプリ操作</div>
							<ul class="grid gap-1">
								{#each managedAppOps as op (`${op.op}:${op.id}:${op.ok}:${op.detail ?? ""}`)}
									<li class="vac-subtle-row px-2 py-1 text-xs">
										<span class="font-mono">{op.op}</span>
										<span class="ml-1">{op.id}</span>
										<span class={op.ok ? "ml-2 text-success-500" : "ml-2 text-error-500"}
											>{op.ok ? "ok" : "failed"}</span
										>
									</li>
								{/each}
							</ul>
						</div>
					{/if}
				{:else}
					{#if loading}
						<p class="text-sm opacity-70">動作モードを読み込み中…</p>
					{:else if !loadError}
						<p class="text-sm opacity-70">切替用のモードは未設定です。モード切替を使わずに利用できます。</p>
					{/if}
				{/if}
			</div>
		</aside>
	</div>
</section>

<style>
.mode-grid { display:grid; grid-template-columns:repeat(auto-fit,minmax(min(180px,100%),1fr)); gap:12px; align-content:start }
.mode-button { min-width:0; border:1px solid var(--color-surface-400); border-radius:12px; padding:16px; text-align:left; overflow-wrap:anywhere }
.mode-button strong { display:block; font-size:1rem }
.mode-button span { display:block; margin-top:8px; font-size:.75rem }
.mode-button:hover:not(:disabled) { border-color:var(--vac-theme-accent) }
.mode-button:disabled:not(.mode-current) { opacity:.55 }
.mode-button.mode-current { opacity:1; color:inherit; border:2px solid var(--vac-theme-accent); background:color-mix(in srgb,var(--vac-theme-accent) 18%,transparent); box-shadow:inset 3px 0 var(--vac-theme-accent) }
.mode-button:focus-visible { outline:3px solid var(--color-primary-300); outline-offset:3px }
</style>

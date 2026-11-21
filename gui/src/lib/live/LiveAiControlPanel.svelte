<script lang="ts">
	import { runtimeSnapshotStore } from "../runtimeSnapshot.svelte";
	import { tabNavStore } from "../tabs.svelte";
	import { toastStore } from "../toasts.svelte";
	import { api } from "../api";
	import { ControlApiError, type AiPersonaSummary, type PauseTarget } from "../types";

	let busyIndex = $state<number | null>(null);

	const personas = $derived(runtimeSnapshotStore.snapshot?.ai_personas ?? []);
	const activeCount = $derived(personas.filter((persona) => !persona.paused).length);

	function label(persona: AiPersonaSummary): string {
		return persona.display_name?.trim() || persona.id || `AI ${persona.index + 1}`;
	}

	function usageLabel(persona: AiPersonaSummary): string {
		const usage = persona.usage;
		if (usage.last_updated_at === null) return "まだ動作履歴がない";
		const requests = new Intl.NumberFormat("ja-JP").format(usage.api_requests);
		const latency = usage.last_latency_ms === null ? "-" : `${Math.round(usage.last_latency_ms)} ms`;
		return `${requests} requests / 最終 ${latency}`;
	}

	async function setPaused(persona: AiPersonaSummary, paused: boolean) {
		if (busyIndex !== null) return;
		busyIndex = persona.index;
		const target: PauseTarget = { target: "ai", index: persona.index };
		try {
			if (paused) await api.pause(target);
			else await api.resume(target);
			await runtimeSnapshotStore.refresh();
			toastStore.success(paused ? "AIを一時停止した" : "AIを再開した", label(persona));
		} catch (value) {
			const message =
				value instanceof ControlApiError
					? `${value.status} ${value.statusText}`
					: value instanceof Error
						? value.message
						: String(value);
			toastStore.error("AIの状態を変更できなかった", message);
		} finally {
			busyIndex = null;
		}
	}
</script>

<section class="vac-panel" aria-label="AI稼働制御">
	<header class="vac-panel-header flex flex-wrap items-center justify-between gap-3 px-4 py-3">
		<div>
			<h3 class="text-sm font-semibold">AIパートナー</h3>
			<p class="mt-0.5 text-xs opacity-60">
				{runtimeSnapshotStore.snapshot === null ? "状態を確認中…" : `${activeCount}人が有効、${personas.length - activeCount}人が一時停止中`}
			</p>
		</div>
		<button type="button" class="text-xs text-primary-600-400 hover:underline" onclick={() => tabNavStore.setActive("ai")}>AIの設定</button>
		<button
			type="button"
			class="text-xs text-primary-600-400 hover:underline"
			onclick={() => void runtimeSnapshotStore.refresh()}
			disabled={runtimeSnapshotStore.loading}
		>
			{runtimeSnapshotStore.loading ? "更新中…" : "更新"}
		</button>
	</header>

	{#if runtimeSnapshotStore.error}
		<p class="border-b border-error-500/40 bg-error-500/10 px-4 py-2 text-xs text-error-600-400">
			状態を取得できない: {runtimeSnapshotStore.error}
		</p>
	{/if}

	{#if runtimeSnapshotStore.snapshot === null}
		<div class="grid min-h-20 content-center gap-2 px-4 py-5" aria-live="polite" aria-label="AI共演者の状態を確認中">
			<div class="h-3 w-36 animate-pulse rounded bg-surface-200-800"></div>
			<div class="h-3 w-52 animate-pulse rounded bg-surface-200-800"></div>
		</div>
	{:else if personas.length === 0}
		<div class="px-4 py-5">
			<p class="text-sm">AIパートナーは未使用。</p>
			<p class="mt-1 text-xs opacity-60">AIを使わずに、チャット支援や通知・演出を利用できます。</p>
			<button
				type="button"
				class="mt-3 rounded bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white hover:bg-primary-600"
				onclick={() => tabNavStore.setActive("ai")}
			>
				AIを設定
			</button>
		</div>
	{:else}
		<ul class="divide-y divide-surface-200-800">
			{#each personas as persona (persona.index)}
				<li class="flex flex-wrap items-center gap-3 px-4 py-3">
					<span
						class={`h-2.5 w-2.5 shrink-0 rounded-full ${persona.paused ? "bg-warning-500" : "bg-success-500"}`}
						aria-hidden="true"
					></span>
					<div class="min-w-0 flex-1">
						<div class="truncate text-sm font-semibold">{label(persona)}</div>
						<div class="text-xs opacity-60">{persona.paused ? "一時停止中" : "有効"}{persona.usage.last_model ? ` · ${persona.usage.last_model}` : ""}</div>
						<details class="mt-1 text-xs opacity-65">
							<summary class="cursor-pointer">利用状況</summary>
							<p>{usageLabel(persona)}</p>
							{#if persona.id && persona.id !== label(persona)}<p>ID: {persona.id}</p>{/if}
						</details>
					</div>
					<button
						type="button"
						class={`min-w-24 rounded border px-3 py-1.5 text-xs font-semibold disabled:opacity-50 ${persona.paused ? "border-success-500 text-success-500 hover:bg-success-500/10" : "border-warning-500 text-warning-600-400 hover:bg-warning-500/10"}`}
						disabled={busyIndex !== null || runtimeSnapshotStore.error !== null || runtimeSnapshotStore.loading}
						aria-label={`${label(persona)}を${persona.paused ? "再開" : "一時停止"}`}
						onclick={() => void setPaused(persona, !persona.paused)}
					>
						{busyIndex === persona.index ? "変更中…" : persona.paused ? "再開" : "一時停止"}
					</button>
				</li>
			{/each}
		</ul>
	{/if}
</section>

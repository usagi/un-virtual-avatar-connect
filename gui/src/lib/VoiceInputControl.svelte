<script lang="ts">
	import { api } from "./api";
	import { runtimeSnapshotStore } from "./runtimeSnapshot.svelte";
	import { toastStore } from "./toasts.svelte";
	import { ControlApiError } from "./types";

	let busy = $state(false);
	let persistenceError = $state<string | null>(null);

	const voiceInput = $derived(runtimeSnapshotStore.snapshot?.voice_input ?? null);
	const ingresses = $derived(runtimeSnapshotStore.snapshot?.bridges.voice_ingresses ?? []);
	const muted = $derived(voiceInput?.muted ?? false);

	async function saveAcceptance(nextMuted: boolean): Promise<void> {
		if (voiceInput === null || busy) return;
		busy = true;
		try {
			const result = await api.setVoiceInput({ muted: nextMuted });
			persistenceError = result.persistence_error ?? null;
			await runtimeSnapshotStore.refresh();
		} catch (value) {
			const message =
				value instanceof ControlApiError
					? `${value.status} ${value.statusText}`
					: value instanceof Error
						? value.message
						: String(value);
			toastStore.error("音声入力の状態を変更できなかった", message);
		} finally {
			busy = false;
		}
	}
</script>

<section class="vac-panel" aria-label="音声入力">
	<header class="vac-panel-header flex min-h-20 items-center gap-4 px-4 py-3">
		<div class="min-w-0 flex-1">
			<h3 class="text-sm font-semibold">音声入力</h3>
			<p class="mt-0.5 text-xs opacity-60">
				{voiceInput === null
					? "状態を確認中…"
					: ingresses.length === 0
						? `使用中の音声入力なし · 入力開始後の受付は${muted ? "OFF" : "ON"}`
						: muted
							? `${ingresses.length}件すべてを一時停止しています`
							: `${ingresses.filter((input) => input.running).length}/${ingresses.length}件を使用中`}
			</p>
		</div>
		<span class="shrink-0 text-xs opacity-60">認識結果の受付</span>
		<button
			type="button"
			role="switch"
			aria-checked={!muted}
			aria-label={ingresses.length === 0 ? `入力開始後の音声認識結果の受付を${muted ? "ON" : "OFF"}にする` : muted ? "すべての音声入力を再開" : "すべての音声入力を一時停止"}
			title={ingresses.length === 0 ? "入力を開始した後の受付を設定します。マイクや音声認識は起動しません。" : "すべての音声認識結果の受付を切り替えます。マイク自体のミュートではありません。"}
			class={`relative h-8 w-14 shrink-0 rounded-full border transition-colors disabled:opacity-50 ${
				muted ? "border-warning-500 bg-warning-500/25" : "border-success-500 bg-success-500/25"
			}`}
			disabled={voiceInput === null || busy}
			onclick={() => void saveAcceptance(!muted)}
		>
			<span class={`absolute top-1 h-6 w-6 rounded-full transition-all ${muted ? "left-1 bg-warning-500" : "left-7 bg-success-500"}`}
			></span>
		</button>
	</header>
	{#if persistenceError}
        <div role="alert" class="px-4 py-3 text-sm">
            <p>現在の受付は変更したが、次回起動用の設定を保存できなかった。</p>
            <button type="button" class="mt-2 rounded border border-warning-500 px-3 py-2 font-semibold hover:bg-warning-500/15 focus-visible:outline-2 focus-visible:outline-offset-2 disabled:opacity-50" disabled={busy} onclick={() => void saveAcceptance(muted)}>現在の受付状態を再保存</button>
            <details><summary>診断情報</summary><p class="break-all">{persistenceError}</p></details>
        </div>
    {/if}
	{#if ingresses.length > 0}
		<ul class="divide-y divide-surface-200-800">
			{#each ingresses as input (input.node_id)}
				<li class="flex min-w-0 items-center gap-3 px-4 py-3">
					<span
						class={`h-2.5 w-2.5 shrink-0 rounded-full ${muted ? "bg-warning-500" : input.running ? "bg-success-500" : "bg-error-500"}`}
						aria-hidden="true"
					></span>
					<div class="min-w-0 flex-1">
						<div class="truncate text-sm font-semibold">{input.label}</div>
						<p class="truncate text-xs opacity-60">{input.source}</p>
					</div>
					<span class="shrink-0 text-xs opacity-55">
						{muted ? "一時停止中" : input.running ? (input.active_backend ?? input.engine) : "停止中"}
					</span>
				</li>
			{/each}
		</ul>
	{/if}
</section>

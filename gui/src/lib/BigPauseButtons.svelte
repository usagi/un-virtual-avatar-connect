<script lang="ts">
	/**
	 * Phase VI-γ-2c: 配信中にすぐ押せる大型 Pause / 再開ボタン。
	 *
	 * 挙動:
	 *   - snapshot の ai_personas の paused 集計から「すべて paused 状態」を判定
	 *   - ボタン押下で `/pause` or `/resume` を { target: 'all' } で叩く
	 *   - 実行中は spinner を出す。結果は `pause_state` event からの再取得で反映
	 *   - paused の間は上部に目立つ警告帯（固定色）を出し、誤操作配信を避けるためのサインにする
	 *
	 * API 拡張は無し（γ-1 で用意した `api.pause/resume` と snapshot を再利用）。
	 */
	import { api } from "./api";
	import { runtimeSnapshotStore } from "./runtimeSnapshot.svelte";
	import { ControlApiError } from "./types";
	import { toastStore } from "./toasts.svelte";

	let busy = $state(false);

	const totalAi = $derived(runtimeSnapshotStore.snapshot?.ai_personas.length ?? 0);
	const pausedAi = $derived(runtimeSnapshotStore.snapshot?.ai_personas.filter((a) => a.paused).length ?? 0);
	const allPaused = $derived(totalAi > 0 && pausedAi === totalAi);
	// 部分的に paused = 0 < paused < total
	const partiallyPaused = $derived(!allPaused && pausedAi > 0);

	async function act(pause: boolean) {
		busy = true;
		try {
			if (pause) {
				await api.pause({ target: "all" });
			} else {
				await api.resume({ target: "all" });
			}
			await runtimeSnapshotStore.refresh();
		} catch (e) {
			const msg = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
			toastStore.error(pause ? "Pause に失敗しました" : "Resume に失敗しました", msg);
		} finally {
			busy = false;
		}
	}
</script>

<section
	class="rounded-lg border p-4 transition-colors {allPaused
		? 'border-error-500 bg-error-500/10'
		: partiallyPaused
			? 'border-warning-500 bg-warning-500/10'
			: 'border-surface-200-800 bg-surface-100-900'}"
>
	<header class="mb-3 flex items-baseline justify-between gap-2">
		<h2 class="text-sm font-semibold">全体の一時停止</h2>
		<span class="text-xs opacity-70 tabular-nums">
			AI {totalAi - pausedAi} / {totalAi}
		</span>
	</header>

	{#if allPaused}
		<p class="mb-3 rounded border border-error-500/50 bg-error-500/10 px-3 py-2 text-sm font-semibold">
			すべて停止中: 配信は VAC から発話されません。再開ボタンを押すまでこの状態です。
		</p>
	{:else if partiallyPaused}
		<p class="mb-3 rounded border border-warning-500/50 bg-warning-500/10 px-3 py-2 text-sm">
			{pausedAi}件のAIが停止中。AI共演者の一覧で個別に確認できる。
		</p>
	{/if}

	<div class="flex flex-wrap gap-2">
		<button
			type="button"
			class="flex-1 rounded-lg border-2 border-error-500 px-4 py-3 text-lg font-bold text-error-500 transition-colors hover:bg-error-500 hover:text-white disabled:opacity-50"
			onclick={() => act(true)}
			disabled={busy || allPaused}
		>
			{#if busy && !allPaused}
				実行中…
			{:else}
				すべて一時停止
			{/if}
		</button>
		<button
			type="button"
			class="flex-1 rounded-lg border-2 border-success-500 px-4 py-3 text-lg font-bold text-success-500 transition-colors hover:bg-success-500 hover:text-white disabled:opacity-50"
			onclick={() => act(false)}
			disabled={busy || (!allPaused && !partiallyPaused)}
		>
			{#if busy && (allPaused || partiallyPaused)}
				実行中…
			{:else}
				すべて再開
			{/if}
		</button>
	</div>
</section>

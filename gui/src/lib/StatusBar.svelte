<script lang="ts">
	/**
	 * 常駐ステータスバー（画面下端）。
	 *
	 * 表示項目:
	 *   - WS 接続状態
	 *   - snapshot 由来: AI persona 数、pause 中数
	 *   - 直近 heartbeat からの経過時間
	 *   - conf sync phase（γ-4a で本格稼働）
	 *
	 * δ-9 D.5: V1 `PROC` / `FIRE` インジケータは廃止（processor dispatch ビューは Flowgraph タブ側に移行）。
	 */
	import { eventsStore } from "./events.svelte";
	import { confSyncStore } from "./confSync.svelte";
	import { runtimeSnapshotStore } from "./runtimeSnapshot.svelte";

	const aiCount = $derived(runtimeSnapshotStore.snapshot?.ai_personas.length ?? 0);
	const aiPausedCount = $derived(runtimeSnapshotStore.snapshot?.ai_personas.filter((a) => a.paused).length ?? 0);
	const wsConn = $derived(eventsStore.connection);
	const wsColor = $derived(import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" ? "bg-surface-400" : wsConn === "open" ? "bg-success-500" : wsConn === "connecting" ? "bg-warning-500" : "bg-error-500");
	const wsLabel = $derived(
		import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" ? "比較用・稼働通知なし" : wsConn === "open" ? "稼働中" : wsConn === "connecting" ? "起動中" : wsConn === "idle" ? "待機中" : "接続に問題があります"
	);
	const aiLabel = $derived(
		runtimeSnapshotStore.snapshot === null
			? "AIを確認中"
			: aiCount === 0
				? "AI未使用"
				: aiPausedCount === aiCount
					? "AI一時停止中"
					: "AI有効"
	);

	const confPhaseLabel = $derived.by(() => {
		switch (confSyncStore.phase) {
			case "synced":
				return { text: "synced", tone: "text-success-500" };
			case "dirty":
				return { text: "dirty", tone: "text-warning-500" };
			case "saved_restart_needed":
				return { text: "restart needed", tone: "text-warning-500" };
			case "restart_in_flight":
				return { text: "restarting…", tone: "text-primary-500" };
		}
	});
</script>

<footer class="vac-status-bar hidden flex-wrap items-center justify-between gap-x-4 gap-y-1 border-t px-6 py-1.5 text-xs sm:flex">
	<div class="flex flex-wrap items-center gap-x-4 gap-y-1">
		<span class="flex items-center gap-1.5" title={`アプリとの接続: ${wsConn}`}>
			<span class="inline-block h-2 w-2 rounded-full {wsColor}"></span>
			<span>{wsLabel}</span>
		</span>
		<span
			class:opacity-70={runtimeSnapshotStore.snapshot === null || aiCount === 0}
			class:text-warning-500={aiCount > 0 && aiPausedCount === aiCount}
		>
			{aiLabel}
		</span>
	</div>
	{#if confSyncStore.phase !== "synced"}
		<div class="flex items-center gap-x-4">
			<span class={confPhaseLabel.tone} title={`設定状態: ${confPhaseLabel.text}`}>
				{confSyncStore.phase === "dirty"
					? "未保存の変更"
					: confSyncStore.phase === "saved_restart_needed"
						? "再起動が必要"
						: "再起動中…"}
			</span>
		</div>
	{/if}
</footer>

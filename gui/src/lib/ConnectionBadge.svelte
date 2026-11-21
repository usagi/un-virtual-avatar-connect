<script lang="ts">
	// WebSocket 接続状態の小さなバッジ。
	// eventsStore は runes ベースなのでここで import するだけで自動追従する。
	import { eventsStore } from "./events.svelte";

	const classByState: Record<string, string> = {
		idle: "preset-tonal",
		connecting: "preset-tonal-warning",
		open: "preset-filled-success-500",
		closed: "preset-filled-error-500",
		error: "preset-filled-error-500",
	};

	const labelByState: Record<string, string> = {
		idle: "待機中",
		connecting: "起動中…",
		open: "稼働中",
		closed: "接続に問題があります",
		error: "接続に問題があります",
	};
</script>

<div class="flex items-center gap-2 text-xs">
	<span class="badge {classByState[eventsStore.connection]}">
		{import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" ? "比較用・稼働通知なし" : labelByState[eventsStore.connection]}
	</span>
</div>

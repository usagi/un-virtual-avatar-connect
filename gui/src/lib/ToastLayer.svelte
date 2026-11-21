<script lang="ts">
	/**
	 * 通知欄。アプリの通常レイアウト内に置き、編集領域を覆わない。
	 *
	 * Control イベントと紐付けるブリッジは `ToastBridge.svelte` で別管理し、
	 * 本コンポーネントは純粋な描画層とする（テストしやすさのため）。
	 */
	import { toastStore, type ToastMessage } from "./toasts.svelte";
	const errorCount = $derived(toastStore.items.filter(t => t.tone === "error").length);

	function toneClass(t: ToastMessage["tone"]): string {
		switch (t) {
			case "info":
				return "bg-primary-500/15 text-primary-900-100 border-primary-500/40";
			case "success":
				return "bg-success-500/15 text-success-900-100 border-success-500/40";
			case "warn":
				return "bg-warning-500/15 text-warning-900-100 border-warning-500/40";
			case "error":
				return "bg-error-500/15 text-error-900-100 border-error-500/40";
		}
	}
</script>

{#if toastStore.items.length}
<div
	class="flex shrink-0 min-w-0 flex-col gap-2 border-t border-surface-300-700 bg-surface-100-900 px-4 py-2 text-surface-900-100"
	role="region"
	aria-label="Notifications"
	aria-live="polite"
>
	<div class="text-xs">
		通知 {toastStore.items.length}件{#if errorCount} · エラー {errorCount}件{/if}
		<span class="ml-1 opacity-70">スクロールで確認</span>
	</div>
	<!-- svelte-ignore a11y_no_noninteractive_tabindex (Scrollable notifications must support keyboard scrolling.) -->
	<div class="toast-scroll flex flex-col gap-2 overflow-y-auto overscroll-contain rounded" tabindex="0" role="region" aria-label="通知一覧">
	{#each toastStore.items as t (t.id)}
		<div class="shrink-0 rounded border px-3 py-2 text-sm shadow-lg {toneClass(t.tone)}" role="status">
			<div class="flex items-start justify-between gap-2">
				<div class="font-semibold">{t.title}</div>
				<button
					type="button"
					class="-mr-1 -mt-0.5 text-lg leading-none opacity-70 hover:opacity-100"
					aria-label="Dismiss"
					disabled={t.action_pending}
					onclick={() => toastStore.dismiss(t.id)}
				>
					×
				</button>
			</div>
			{#if t.detail}
				<div class="mt-0.5 whitespace-pre-wrap break-words text-xs opacity-80">{t.detail}</div>
			{/if}
			{#if t.action}
				{#if t.action_error}<p class="mt-1 text-xs" role="alert">{t.action_error}</p>{/if}
				<div class="mt-1.5 flex justify-end">
					<button
						type="button"
						class="rounded border border-current/40 px-2 py-0.5 text-xs font-medium hover:bg-current/10"
						disabled={t.action_pending}
						onclick={() => toastStore.triggerAction(t.id)}
					>
						{t.action_pending ? "処理中…" : t.action.label}
					</button>
				</div>
			{/if}
		</div>
	{/each}
	</div>
</div>
{/if}

<style>
	.toast-scroll {
		max-height: min(10rem, 26dvh);
		scrollbar-gutter: stable;
	}
	.toast-scroll:focus-visible {
		outline: 2px solid var(--color-primary-500);
		outline-offset: 2px;
	}
</style>

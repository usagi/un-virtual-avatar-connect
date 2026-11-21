<script lang="ts">
	let { open = $bindable(false), busy, error, target, connection, onconfirm }: {
		open: boolean; busy: boolean; error: string | null; target: string; connection: string;
		onconfirm: () => Promise<void>;
	} = $props();
	let dialog: HTMLDialogElement;
	let cancelButton: HTMLButtonElement;
	$effect(() => {
		if (!dialog) return;
		if (open && !dialog.open) {
			dialog.showModal();
			cancelButton?.focus();
		} else if (!open && dialog.open) dialog.close();
	});
</script>

<dialog bind:this={dialog} aria-labelledby="shutdown-title"
	class="m-auto w-[calc(100%-2rem)] max-w-lg rounded-lg border border-surface-200-800 bg-surface-50-950 p-0 text-surface-950-50 shadow-xl backdrop:bg-black/50"
	oncancel={(event) => { if (busy) event.preventDefault(); else open = false; }}
	onclose={() => { open = false; }}>
	<header class="border-b border-surface-200-800 px-5 py-3">
		<h2 id="shutdown-title" class="text-base font-semibold">このVACを終了</h2>
	</header>
	<div class="space-y-3 px-5 py-4 text-sm">
		<p class="font-semibold">{target}</p>
		<p class="break-all text-xs opacity-65">接続先: {connection}</p>
		<p>このVACと、このVACが起動した連携アプリの停止を試みます。操作画面との接続も切断されます。</p>
		{#if error}
			<div role="alert" class="rounded border border-error-500/40 p-3 text-error-700-300">
				<p>終了を要求できませんでした。キャンセルして接続を確認するか、もう一度お試しください。</p>
				<details class="mt-2 text-xs"><summary>詳細</summary><p class="mt-1 break-words">{error}</p></details>
			</div>
		{/if}
	</div>
	<footer class="flex justify-end gap-3 border-t border-surface-200-800 px-5 py-3">
		<button bind:this={cancelButton} type="button" class="rounded border border-surface-300-700 px-4 py-2" disabled={busy} onclick={() => { open = false; }}>キャンセル</button>
		<button type="button" class="rounded border border-error-500 px-4 py-2 text-error-700-300" disabled={busy} onclick={() => void onconfirm()}>{busy ? "停止中…" : "このVACを終了する"}</button>
	</footer>
</dialog>

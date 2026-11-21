<script lang="ts">
	let { root, fq, error, busy, oncancel, onconfirm }: {
		root: string; fq: string; error: string | null; busy: boolean;
		oncancel: () => void; onconfirm: () => void;
	} = $props();
	let dialog: HTMLDialogElement;
	let cancelButton: HTMLButtonElement;
	$effect(() => {
		if (!dialog) return;
		const opener = document.activeElement;
		if (!dialog.open) { dialog.showModal(); cancelButton?.focus(); }
		return () => {
			dialog.close();
			if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
		};
	});
</script>

<dialog bind:this={dialog} aria-labelledby="discard-draft-title"
	class="m-auto w-[calc(100%-2rem)] max-w-lg rounded-lg border border-surface-200-800 bg-surface-50-950 p-0 text-surface-950-50 shadow-xl backdrop:bg-black/50"
	oncancel={oncancel} onclose={oncancel}>
	<header class="border-b border-surface-200-800 px-5 py-3">
		<h2 id="discard-draft-title" class="text-base font-semibold">未保存の編集を破棄</h2>
	</header>
	<div class="space-y-3 px-5 py-4 text-sm">
		<p class="break-all font-semibold">{fq}</p>
		<p class="break-all text-xs opacity-65">保存先: {root}</p>
		<p>このファイルの未保存の編集を破棄します。保存済みのファイルは変更しません。</p>
		{#if error}<p role="alert" class="text-error-700-300">{error}</p>{/if}
	</div>
	<footer class="flex justify-end gap-3 border-t border-surface-200-800 px-5 py-3">
		<button bind:this={cancelButton} type="button" class="rounded border border-surface-300-700 px-4 py-2" onclick={oncancel}>キャンセル</button>
		<button type="button" class="rounded border border-error-500 px-4 py-2 text-error-700-300" disabled={busy || error !== null} onclick={onconfirm}>この編集を破棄する</button>
	</footer>
</dialog>

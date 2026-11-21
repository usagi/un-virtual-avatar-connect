<script lang="ts">
	import { onMount, tick } from "svelte";
	import { api } from "../api";
	import { tabNavStore } from "../tabs.svelte";
	import { ControlApiError } from "../types";
	let { onRestored }: { onRestored: () => void } = $props();
	let entries = $state<{ filename: string; original: string; size: number }[]>([]);
	let selected = $state("");
	let filename = $state("");
	let directory = $state("");
	let busy = $state(false);
	let loaded = $state(false);
	let error = $state("");
	let result = $state("");
	let completedKey = $state("");
	let discardReview = $state<{ selected: string; filename: string; directory: string } | null>(null);
	let keepInputButton = $state<HTMLButtonElement>();
	$effect(() => { if (discardReview) void tick().then(() => keepInputButton?.focus()); });
	function restoreKey() { return JSON.stringify([selected, filename.trim()]); }
	function hasDraft() { return Boolean(selected || filename.trim()) && restoreKey() !== completedKey; }
	function mayLeave() {
		if (busy || discardReview) return false;
		if (hasDraft()) { error = "復元先の入力が残っています。復元するか、「入力を取り消す」から破棄してください。"; return false; }
		return true;
	}
	function requestDiscard() {
		if (busy || discardReview || !hasDraft()) return;
		discardReview = { selected, filename, directory };
	}
	function confirmDiscard() {
		if (busy || !discardReview) return;
		const target = discardReview;
		discardReview = null;
		if (target.selected !== selected || target.filename !== filename || target.directory !== directory) {
			error = "確認中に入力または対象が変わりました。内容を確認し直してください。"; return;
		}
		selected = ""; filename = ""; completedKey = ""; error = ""; result = "";
	}
	onMount(() => tabNavStore.guardLeaving(mayLeave));
	function message(e: unknown): string {
		if (e instanceof ControlApiError) return (e.body as { detail?: string })?.detail ?? e.message;
		return e instanceof Error ? e.message : String(e);
	}
	async function load() {
		if (busy || discardReview) return;
		busy = true; error = "";
		try { const response = await api.profileBackups(); entries = response.entries; directory = response.directory; loaded = true; }
		catch (e) { error = message(e); } finally { busy = false; }
	}
	function choose() {
		if (busy || discardReview) return;
		const entry = entries.find(e => e.filename === selected);
		filename = entry ? entry.original.replace(/\.toml$/i, ".restored.toml") : "";
		result = ""; error = "";
	}
	async function restore() {
		if (busy || discardReview || !selected || !filename.trim()) return;
		const submittedKey = restoreKey();
		busy = true; error = ""; result = "";
		try {
			const response = await api.profileRestore(selected, filename.trim());
			result = `${response.filename}: ${response.warning ?? "復元しました"}`;
			completedKey = submittedKey;
			onRestored();
		} catch (e) { error = message(e); } finally { busy = false; }
	}
</script>

<svelte:window onbeforeunload={(event) => { if (busy || hasDraft()) { event.preventDefault(); event.returnValue = ""; } }} />

<details class="border-t border-surface-200-800 p-4" ontoggle={(event) => { if (event.currentTarget.open && !loaded && !busy) void load(); }}>
	<summary class="cursor-pointer text-sm font-semibold">バックアップから復元</summary>
	<section class="mt-3 grid gap-3 text-sm" aria-label="設定ファイルの復元">
		<p class="text-xs opacity-70">編集・削除前の設定を新しいファイルへ復元します。同名の既存ファイルは上書きしません。復元後は一覧の「編集」で内容を確認し、「切替起動」で適用できます。外部ファイルや認証情報はこの復元の対象外です。</p>
		<p class="break-all text-xs opacity-60">対象フォルダー: {directory || "読込中…"}</p>
		{#if discardReview}
			<div role="region" aria-label="復元入力を破棄する確認" class="rounded border border-warning-500 p-3">
				<p>復元先「{discardReview.filename || "未入力"}」への入力を取り消す？ バックアップや設定ファイルは削除しない。</p>
				<p class="mt-1 break-all text-xs">復元元：{discardReview.selected} · {discardReview.directory}</p>
				<div class="mt-3 flex flex-wrap gap-3">
					<button bind:this={keepInputButton} class="rounded border px-3 py-2" onclick={() => discardReview = null}>入力を残して戻る</button>
					<button class="rounded border border-warning-500 px-3 py-2" onclick={confirmDiscard}>入力を破棄する</button>
				</div>
			</div>
		{/if}
		<button type="button" class="justify-self-start rounded border px-3 py-1" onclick={load} disabled={busy || discardReview !== null}>バックアップを再取得</button>
		{#if loaded && entries.length === 0}<p>バックアップはありません。</p>{/if}
		{#if entries.length}
			<label class="grid gap-1">復元元<select class="rounded border bg-surface-50-950 p-2" bind:value={selected} onchange={choose} disabled={busy || discardReview !== null}><option value="">バックアップを選択</option>{#each entries as entry}<option value={entry.filename}>{entry.filename} · {entry.size} B</option>{/each}</select></label>
			<label class="grid gap-1">復元先のファイル名<input class="rounded border bg-surface-50-950 p-2" bind:value={filename} disabled={busy || discardReview !== null} placeholder="conf.restored.toml" /></label>
			<button type="button" class="justify-self-start rounded border px-3 py-2" disabled={busy || discardReview !== null || !selected || !filename.trim()} onclick={restore}>{busy ? "処理中…" : "新しいファイルへ復元"}</button>
			<button type="button" class="justify-self-start rounded border px-3 py-2" disabled={busy || discardReview !== null || !hasDraft()} onclick={requestDiscard}>入力を取り消す</button>
		{/if}
		{#if error}<p role="alert" class="text-error-500">{error}</p>{/if}
		{#if result}<p role="status">{result}</p>{/if}
	</section>
</details>

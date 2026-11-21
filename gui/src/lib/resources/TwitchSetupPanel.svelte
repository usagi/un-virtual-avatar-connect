<script lang="ts">
	import { onMount, tick } from "svelte";
	import { api } from "../api";
	import { ControlApiError } from "../types";
	import { tabNavStore } from "../tabs.svelte";
	import { confSyncStore } from "../confSync.svelte";
	import RestartDialog from "../RestartDialog.svelte";
	let { onapplied, onpending }: { onapplied?: () => void; onpending?: (pending: boolean) => void } = $props();
	let status = $state<Awaited<ReturnType<typeof api.twitchSetup>> | null>(null);
	let broadcaster = $state("");
	let moderator = $state("");
	let busy = $state(false);
	let error = $state("");
	let saved = $state(false);
	let restartOpen = $state(false);
	let discardReview = $state<{ broadcaster: string; moderator: string; revision: string; source: string } | null>(null);
	let keepInputButton = $state<HTMLButtonElement>();
	$effect(() => { if (discardReview) void tick().then(() => keepInputButton?.focus()); });
	const dirty = $derived(status !== null && (broadcaster !== status.broadcaster || moderator !== status.moderator));
	$effect(() => onpending?.(Boolean(status?.restart_required || dirty || busy)));
	function message(value: unknown): string {
		if (value instanceof ControlApiError && value.body && typeof value.body === "object" && "reason" in value.body) return String(value.body.reason);
		return value instanceof Error ? value.message : String(value);
	}
	onMount(() => {
		void load();
		const unguard = tabNavStore.guardLeaving(canLeave);
		let checking = false;
		const timer = setInterval(async () => {
			if (checking || busy || dirty || discardReview || !status?.restart_required) return;
			checking = true;
			const requestedRevision = status.revision;
			try {
				const next = await api.twitchSetup();
				if (!busy && !dirty && !discardReview && status?.revision === requestedRevision
                    && status.source_path === next.source_path
                    && broadcaster === next.broadcaster && moderator === next.moderator
                    && !next.restart_required) {
					status = next; onapplied?.();
				}
			} catch { /* Restart temporarily disconnects this VAC. Keep the saved form. */ }
			finally { checking = false; }
		}, 3000);
		return () => { unguard(); clearInterval(timer); };
	});
	function canLeave() {
		if (busy || discardReview) return false;
		if (dirty) { error = "Twitch設定に未保存の入力があります。保存するか、「保存内容を読み直す」から破棄を確認してください。"; return false; }
		return true;
	}
	function requestReload() {
		if (busy || discardReview) return;
		if (dirty && status) {
			discardReview = { broadcaster, moderator, revision: status.revision, source: status.source_path };
			return;
		}
		void load();
	}
	async function confirmReload() {
		if (busy || !discardReview) return;
		const review = discardReview;
		discardReview = null;
		if (review.broadcaster !== broadcaster || review.moderator !== moderator ||
			review.revision !== status?.revision || review.source !== status?.source_path) {
			error = "確認中に入力または保存対象が変わりました。もう一度読み直しを選んでください。";
			return;
		}
		await load();
	}
	async function load() {
		if (busy) return;
		busy = true; error = "";
		try {
			status = await api.twitchSetup(); broadcaster = status.broadcaster; moderator = status.moderator; saved = false;
		} catch (e) { error = message(e); }
		finally { busy = false; }
	}
	async function save() {
		if (!status || busy || discardReview) return;
		busy = true; error = ""; saved = false;
		try {
			status = await api.saveTwitchSetup({ broadcaster, moderator, revision: status.revision, expected_source: status.source_path });
			broadcaster = status.broadcaster; moderator = status.moderator; saved = true;
			if (status.restart_required) confSyncStore.markSavedRequiringRestart();
		} catch (e) { error = message(e); }
		finally { busy = false; }
	}
</script>

<svelte:window onbeforeunload={(event) => { if (dirty || busy) { event.preventDefault(); event.returnValue = ""; } }} />
<section class="vac-panel p-4" aria-label="Twitchの基本設定">
	<h3 class="font-semibold">1. 配信先と投稿アカウント</h3>
	<p class="my-2 text-sm opacity-75">AIを使わなくても、通知・チャット・クリップ紹介を利用できます。</p>
	{#if discardReview}
		<div class="my-3 rounded border border-warning-500 p-3 text-sm" role="region" aria-label="Twitch設定の入力を破棄する確認">
			<p>未保存のログイン名を破棄して、保存内容を読み直す？</p>
			<p class="mt-1 break-all text-xs opacity-65">対象：{discardReview.source}</p>
			<div class="mt-3 flex flex-wrap gap-3">
				<button bind:this={keepInputButton} type="button" class="btn preset-outlined-surface-500" onclick={() => discardReview = null}>入力を残して戻る</button>
				<button type="button" class="btn preset-outlined-warning-500" disabled={busy} onclick={() => void confirmReload()}>入力を破棄して読み直す</button>
			</div>
		</div>
	{/if}
	{#if status}
		<p class="mb-3 break-all text-xs opacity-65">保存対象: {status.source_path}</p>
		<form onsubmit={(event) => { event.preventDefault(); void save(); }} class="grid gap-3">
			<label class="grid gap-1 text-sm">配信者のログイン名
				<input class="input" aria-label="配信者のログイン名" aria-describedby="twitch-broadcaster-help" bind:value={broadcaster} required pattern="[A-Za-z0-9_]+" maxlength="25" autocomplete="off" disabled={busy} placeholder="your_channel" />
				<span id="twitch-broadcaster-help" class="text-xs opacity-65">twitch.tv/ の後の名前。日本語の表示名ではありません。</span>
			</label>
			<label class="grid gap-1 text-sm">チャット投稿用アカウントのログイン名
				<input class="input" aria-label="チャット投稿用アカウントのログイン名" aria-describedby="twitch-moderator-help" bind:value={moderator} required pattern="[A-Za-z0-9_]+" maxlength="25" autocomplete="off" disabled={busy} placeholder="your_bot" />
				<span id="twitch-moderator-help" class="text-xs opacity-65">配信者と同じアカウントでも使えます。別のボットを使う場合は、そのアカウントで認証し、配信先で必要な権限を与えてください。</span>
			</label>
			<div class="flex flex-wrap gap-2">
				<button class="btn preset-filled-primary-500" type="submit" disabled={busy || discardReview !== null || !dirty || !broadcaster.trim() || !moderator.trim()}>Twitch設定を保存</button>
				<button class="btn preset-outlined-surface-500" type="button" disabled={busy || discardReview !== null} onclick={() => { moderator = broadcaster; }}>投稿にも配信者を使う</button>
			</div>
		</form>
		<p class="mt-3 text-xs opacity-65">保存はこの設定ファイルだけに反映します。通知の詳細・既存の認証情報・Flowgraphは保持します。認証情報は標準では同じPCのVAC間で共有されるため、別アカウントへの再認証・認証解除は他のVACにも影響します。</p>
		{#if status.restart_required}
			<div class="mt-3 rounded border border-warning-500 p-3 text-sm" role="status">
				<p>保存済み・適用待ち。このVACを再起動し、下の「Twitch接続」で両アカウントを認証してください。再起動は現在の処理を中断します。</p>
				<button class="btn mt-2 preset-outlined-surface-500" disabled={busy || dirty} onclick={() => restartOpen = true}>再起動して適用する…</button>
			</div>
		{:else if saved}<p class="mt-3 text-sm" role="status">保存済み。下のTwitch接続で認証状態を確認してください。</p>{/if}
	{/if}
	{#if error}<p class="my-2 text-sm text-error-500" role="alert">{error}</p>{/if}
	<button class="mt-3 text-xs underline" disabled={busy || discardReview !== null} onclick={requestReload}>保存内容を読み直す</button>
</section>
<RestartDialog bind:open={restartOpen} currentOnly />

<script lang="ts">
	import { onMount, onDestroy, tick } from "svelte";
	import { api } from "./api";
	import { ControlApiError, type GoogleOAuthStartResponse, type GoogleOAuthStatusResponse } from "./types";

	let status = $state<GoogleOAuthStatusResponse | null>(null);
	let session = $state<GoogleOAuthStartResponse | null>(null);
	let clientId = $state("");
	let busy = $state(false);
	let error = $state<string | null>(null);
	let pollMessage = $state<string | null>(null);

	let deleteReview = $state<string | null>(null);
	let keepTokenButton = $state<HTMLButtonElement>();
	let statusRequest = 0;
	let disposed = false;
	onDestroy(() => { disposed=true;statusRequest++; });
	$effect(() => { if (deleteReview) void tick().then(()=>keepTokenButton?.focus()); });
	function reviewIdentity() { return JSON.stringify([status,session,clientId]); }
	function requestDeleteTokens() {
		if (busy || disposed || deleteReview || !status?.has_access_token) return;
		deleteReview=reviewIdentity();
	}
	async function confirmDeleteTokens() {
		if (busy || disposed || !deleteReview) return;
		const expected=deleteReview;deleteReview=null;
		if (expected!==reviewIdentity()) { error="確認中に接続設定や認証状態が変わりました。もう一度確認してください。";return; }
		await deleteTokens();
	}
	onMount(() => {
		void refresh();
	});

	async function refresh(internal = false): Promise<void> {
		if (disposed || (!internal && (busy || deleteReview))) return;
		const request=++statusRequest;
		try {
			const next=await api.googleSheetsOAuthStatus();
			if (disposed || request!==statusRequest) return;
			status=next;error=null;
		} catch (e) {
			if (!disposed && request===statusRequest) error=formatError(e);
		}
	}

	async function start(): Promise<void> {
		if (busy || disposed || deleteReview) return;
		statusRequest++;
		busy = true;
		error = null;
		pollMessage = null;
		try {
			const nextSession = await api.googleSheetsOAuthStart({
				client_id: clientId.trim() || undefined,
				open_browser: true,
			});
			if (disposed) return;
			session=nextSession;
			await refresh(true);
		} catch (e) {
			if (!disposed) error = formatError(e);
		} finally {
			busy = false;
		}
	}

	async function poll(): Promise<void> {
		if (busy || disposed || deleteReview || !session) return;
		statusRequest++;
		busy = true;
		error = null;
		try {
			const res = await api.googleSheetsOAuthPoll({
				client_id: clientId.trim() || undefined,
				device_code: session.device_code,
				token_file: session.token_file,
			});
			if (disposed) return;
			pollMessage = res.status === "authorized" ? "authorized" : (res.error ?? res.status);
			if (res.status === "authorized") {
				session = null;
				await refresh(true);
			}
		} catch (e) {
			if (!disposed) error = formatError(e);
		} finally {
			busy = false;
		}
	}

	async function deleteTokens(): Promise<void> {
		if (busy || disposed || deleteReview) return;
		statusRequest++;
		busy = true;
		error = null;
		try {
			await api.googleSheetsOAuthDeleteTokens();
			if (disposed) return;
			statusRequest++;
			if (status) status={...status,token_file_exists:false,has_access_token:false,has_refresh_token:false};
			session = null;
			pollMessage = null;
			await refresh(true);
		} catch (e) {
			if (!disposed) error = formatError(e);
		} finally {
			busy = false;
		}
	}

	async function copyText(text: string): Promise<void> {
		try {
			await navigator.clipboard.writeText(text);
		} catch {
			// Manual copy remains available.
		}
	}

	function formatError(e: unknown): string {
		if (e instanceof ControlApiError) {
			return `${e.status} ${e.statusText}: ${typeof e.body === "string" ? e.body : JSON.stringify(e.body)}`;
		}
		return e instanceof Error ? e.message : String(e);
	}
</script>

<section class="rounded-lg border border-surface-300-700 bg-surface-100-900 p-4">
	<header class="mb-3 flex items-center justify-between">
		<h3 class="text-sm font-semibold">OAuth (Google Sheets)</h3>
		<button
			type="button"
			class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-0.5 text-[10px] hover:bg-surface-200-800"
			disabled={busy || deleteReview !== null}
			onclick={() => void refresh()}
		>
			↻ 再取得
		</button>
	</header>

	{#if deleteReview}
		<div class="mb-3 rounded border border-warning-500 p-3 text-sm" role="region" aria-label="Google Sheets認証情報の削除確認">
			<p>Google Sheetsの保存済み認証情報を削除する？ 次の接続には再認証が必要になる。</p>
			<p class="mt-1 break-all text-xs">保存先：{status?.token_file}</p>
			<p class="mt-1 text-xs">このファイルを共有する他のVACにも影響する。</p>
			<div class="mt-3 flex flex-wrap gap-3">
				<button bind:this={keepTokenButton} type="button" class="rounded border border-surface-300-700 px-3 py-2" onclick={()=>deleteReview=null}>認証情報を残して戻る</button>
				<button type="button" class="rounded border border-error-500 px-3 py-2" disabled={busy} onclick={()=>void confirmDeleteTokens()}>保存済み認証情報を削除</button>
			</div>
		</div>
	{/if}
	{#if error}
		<div class="mb-2 rounded bg-error-200-800 px-2 py-1 text-xs text-error-900-100">
			ERROR: {error}
		</div>
	{/if}

	<div class="space-y-3 text-xs">
		<div class="grid gap-2 sm:grid-cols-[1fr_auto]">
			<label class="block">
				<span class="mb-1 block opacity-70">client_id</span>
				<input
					class="w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 font-mono text-xs"
					bind:value={clientId}
					placeholder="env UNVAC_GOOGLE_OAUTH_CLIENT_ID を使う場合は空"
				/>
			</label>
			<div class="flex items-end gap-2">
				<button
					type="button"
					class="rounded bg-primary-500 px-3 py-1 font-semibold text-primary-950 hover:bg-primary-400 disabled:opacity-50"
					onclick={() => void start()}
					disabled={busy || deleteReview !== null}
				>
					{busy ? "処理中…" : "Device Flow 開始"}
				</button>
			</div>
		</div>

		{#if status}
			<div class="rounded border border-surface-300-700 bg-surface-50-950 p-3">
				<div class="mb-1 flex flex-wrap items-center gap-2">
					<span
						class="rounded px-2 py-0.5 font-mono text-[10px] uppercase {status.has_access_token
							? 'bg-success-200-800 text-success-900-100'
							: 'bg-surface-300-700 text-surface-800-200'}"
					>
						{status.has_access_token ? "authorized" : "not authorized"}
					</span>
					<span class="opacity-70">client={status.configured ? status.client_id_source : "missing"}</span>
					<span class="opacity-70">refresh={status.has_refresh_token ? "yes" : "no"}</span>
				</div>
				<div class="truncate font-mono text-[11px] opacity-80" title={status.token_file}>
					{status.token_file}
				</div>
			</div>
		{/if}

		{#if session}
			{@const currentSession = session}
			<div class="rounded-lg border border-primary-500 bg-primary-50-950 p-3">
				<div class="mb-2 flex flex-wrap items-center gap-2">
					<span class="font-semibold">認証コード</span>
					<code class="rounded bg-surface-50-950 px-3 py-1 text-xl font-bold tracking-widest">
						{currentSession.user_code}
					</code>
					<button
						type="button"
						class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 hover:bg-surface-200-800"
						onclick={() => void copyText(currentSession.user_code)}
					>
						Copy
					</button>
				</div>
				<div class="mb-2 flex flex-wrap items-center gap-2">
					<a
						href={currentSession.verification_url_complete ?? currentSession.verification_url}
						target="_blank"
						rel="noopener noreferrer"
						class="rounded bg-primary-500 px-2 py-1 font-semibold text-primary-950 hover:bg-primary-400"
					>
						Open
					</a>
					<button
						type="button"
						class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 hover:bg-surface-200-800 disabled:opacity-50"
						onclick={() => void poll()}
						disabled={busy || deleteReview !== null}
					>
						認可状態を確認
					</button>
					<span class="opacity-70">poll {currentSession.interval}s / expires {currentSession.expires_in}s</span>
				</div>
				{#if pollMessage}
					<div class="text-[11px] opacity-80">{pollMessage}</div>
				{/if}
			</div>
		{/if}

		{#if status?.has_access_token}
			<button
				type="button"
				class="rounded border border-error-500 bg-error-500/20 px-3 py-1 font-semibold text-error-900-100 hover:bg-error-500/40 disabled:opacity-50"
				onclick={requestDeleteTokens}
				disabled={busy || deleteReview !== null}
			>
				トークンファイルを削除
			</button>
		{/if}
	</div>
</section>

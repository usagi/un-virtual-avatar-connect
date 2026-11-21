<script lang="ts">
	// Twitch Device Code Flow を GUI から回すためのパネル。
	//
	// 仕様（Rust 側 /api/v1/control/oauth/twitch/{broadcaster|moderator}/*）:
	//   - POST /start  → outcome ∈ {started, already_pending, already_authorized} + session (pending 時のみ user_code/uri を含む)
	//   - GET  /status → OAuthSessionView or 404 (セッション無し)
	//   - POST /cancel → canceled + session (最終スナップショット)
	//
	// UX 要点:
	//   - 両アカウント（broadcaster / moderator）をタブで切替
	//   - 現セッションがあれば user_code と verification_uri を大きく提示し、Copy/Open を一押しで済ませる
	//   - `ControlEvent.oauth_status` を WS で受信したら即時 state 反映
	//   - pending 中は残り時間（expires_at）を秒単位で更新

	import { onMount, onDestroy, tick as nextTick } from "svelte";
	import { api } from "./api";
	import { ControlApiError, type OAuthAccount, type OAuthSessionStatus, type OAuthSessionView } from "./types";
	import { eventsStore } from "./events.svelte";

	const ACCOUNTS: readonly OAuthAccount[] = ["broadcaster", "moderator"];

	let tab: OAuthAccount = $state("broadcaster");

	/** 各 account の現在のセッションビュー（未開始なら null） */
	let sessions = $state<Record<OAuthAccount, OAuthSessionView | null>>({
		broadcaster: null,
		moderator: null,
	});

	/** 保存済みトークンの検証結果。通常のruntime snapshotとは分離して取得する。 */
	let authorization = $state<Record<OAuthAccount, boolean | null>>({
		broadcaster: null,
		moderator: null,
	});
	let configured = $state<Record<OAuthAccount, boolean | null>>({
		broadcaster: null,
		moderator: null,
	});

	/** この画面で完了を確認した認可。再検証中の未認可flickerを防ぐ。 */
	let authorizedThisSession = $state<Record<OAuthAccount, boolean>>({
		broadcaster: false,
		moderator: false,
	});

	/** 操作中ロック */
	let busy = $state<Record<OAuthAccount, boolean>>({
		broadcaster: false,
		moderator: false,
	});

	/** 各アカウントの操作エラー（直近 1 件） */
	let errors = $state<Record<OAuthAccount, string | null>>({
		broadcaster: null,
		moderator: null,
	});

	/** 1 秒ごとに更新されるシンプルなカウンタ（カウントダウン表示用） */
	let tick = $state(0);
	let tickTimer: ReturnType<typeof setInterval> | null = null;
	let statusTimer: ReturnType<typeof setInterval> | null = null;

	let unsubscribe: (() => void) | null = null;
	let disposed = false;
	const statusRequest: Record<OAuthAccount, number> = { broadcaster: 0, moderator: 0 };
	const authorizationRequest: Record<OAuthAccount, number> = { broadcaster: 0, moderator: 0 };
	function invalidateReads(acc: OAuthAccount): number {
		authorizationRequest[acc]++;
		return ++statusRequest[acc];
	}

	onMount(() => {
		// 認可検証は外部通信を伴うため、通常のruntime snapshotではなく本画面だけで行う。
		for (const acc of ACCOUNTS) {
			void refreshAuthorization(acc);
			void refreshStatus(acc);
		}

		// WS イベント: oauth_status を直接反映（polling 不要）
		unsubscribe = eventsStore.subscribe((ev) => {
			if (ev.event.kind === "oauth_status") {
				const { account, view } = ev.event;
				invalidateReads(account);
				if (view.status === "authorized") {
					// バックエンドは token 保存後にだけ authorized を通知する。ここで即時確定し、
					// token保存後の再検証中に未認可画面を挟まない。
					authorizedThisSession = { ...authorizedThisSession, [account]: true };
					authorization = { ...authorization, [account]: true };
					sessions = { ...sessions, [account]: null };
				} else {
					sessions = { ...sessions, [account]: view };
					// expired/canceled/failedでも保存トークン状態が変わりうるので再検証する。
					if (view.status !== "pending") void refreshAuthorization(account);
				}
			}
		});

		// 残り秒数を 1Hz で更新
		tickTimer = setInterval(() => {
			tick += 1;
		}, 1_000);

		// OAuth 完了の主経路は WS event。接続瞬断やイベント取りこぼし時にも
		// pending 表示が残り続けないよう、進行中セッションだけ定期再照会する。
		statusTimer = setInterval(() => {
			for (const acc of ACCOUNTS) {
				if (sessions[acc]?.status === "pending" && !busy[acc]) {
					void refreshStatus(acc);
				}
			}
		}, 2_000);
	});

	onDestroy(() => {
		disposed = true;
		if (unsubscribe) unsubscribe();
		if (tickTimer !== null) clearInterval(tickTimer);
		if (statusTimer !== null) clearInterval(statusTimer);
	});

	async function refreshAuthorization(acc: OAuthAccount): Promise<void> {
		const request = ++authorizationRequest[acc];
		try {
			const result = await api.oauthAuthorization(acc);
			if (disposed || request !== authorizationRequest[acc]) return;
			authorization = { ...authorization, [acc]: result.authorized };
			configured = { ...configured, [acc]: result.configured };
		} catch (e) {
			if (disposed || request !== authorizationRequest[acc]) return;
			errors = { ...errors, [acc]: formatError(e) };
		}
	}

	async function refreshStatus(acc: OAuthAccount): Promise<void> {
		const request = ++statusRequest[acc];
		try {
			const view = await api.oauthStatusOrNull(acc);
			if (disposed || request !== statusRequest[acc]) return;
			// authorizedセッションは認可状態へ集約するため表示用には保持しない。
			if (view?.status === "authorized") {
				authorizedThisSession = { ...authorizedThisSession, [acc]: true };
				authorization = { ...authorization, [acc]: true };
				sessions = { ...sessions, [acc]: null };
			} else {
				sessions = { ...sessions, [acc]: view };
			}
			errors = { ...errors, [acc]: null };
		} catch (e) {
			if (disposed || request !== statusRequest[acc]) return;
			errors = { ...errors, [acc]: formatError(e) };
		}
	}

	async function start(acc: OAuthAccount): Promise<void> {
		if (busy[acc]) return;
		const request = invalidateReads(acc);
		busy = { ...busy, [acc]: true };
		errors = { ...errors, [acc]: null };
		try {
			authorizedThisSession = { ...authorizedThisSession, [acc]: false };
			const res = await api.oauthStart(acc);
			if (disposed || request !== statusRequest[acc]) return;
			// `started` / `already_pending` は session が入ってくる → sessions を差し替え
			// `already_authorized`はsession=null。
			if (res.session) {
				sessions = { ...sessions, [acc]: res.session };
			} else if (res.outcome === "already_authorized") {
				authorization = { ...authorization, [acc]: true };
			}
		} catch (e) {
			if (disposed || request !== statusRequest[acc]) return;
			errors = { ...errors, [acc]: formatError(e) };
		} finally {
			busy = { ...busy, [acc]: false };
		}
	}
	async function openBrowser(acc: OAuthAccount): Promise<void> {
		if (busy[acc]) return;
		busy = { ...busy, [acc]: true };
		try { await api.oauthOpen(acc); }
		catch (e) { errors = { ...errors, [acc]: formatError(e) }; }
		finally { busy = { ...busy, [acc]: false }; }
	}

	async function cancel(acc: OAuthAccount): Promise<void> {
		if (busy[acc]) return;
		const request = invalidateReads(acc);
		busy = { ...busy, [acc]: true };
		errors = { ...errors, [acc]: null };
		try {
			const res = await api.oauthCancel(acc);
			if (disposed || request !== statusRequest[acc]) return;
			sessions = { ...sessions, [acc]: res.session };
		} catch (e) {
			if (disposed || request !== statusRequest[acc]) return;
			errors = { ...errors, [acc]: formatError(e) };
		} finally {
			busy = { ...busy, [acc]: false };
		}
	}

	let deleteReview = $state<{account:OAuthAccount;snapshot:string}|null>(null);
	let keepTokenButton = $state<HTMLButtonElement>();
	$effect(() => { if (deleteReview) void nextTick().then(() => keepTokenButton?.focus()); });
	function tokenReviewIdentity(acc:OAuthAccount) { return JSON.stringify([sessions[acc],authorization[acc],configured[acc],authorizedThisSession[acc]]); }
	function requestDeleteTokens(acc:OAuthAccount) {
		if (disposed || busy[acc] || deleteReview) return;
		deleteReview={account:acc,snapshot:tokenReviewIdentity(acc)};
	}
	async function confirmDeleteTokens() {
		if (!deleteReview || disposed || busy[deleteReview.account]) return;
		const review=deleteReview;deleteReview=null;
		if (review.account!==tab || review.snapshot!==tokenReviewIdentity(review.account)) {
			errors={...errors,[review.account]:"確認中に認証状態が変わりました。もう一度対象を確認してください。"};return;
		}
		await deleteTokens(review.account);
	}
	async function deleteTokens(acc: OAuthAccount): Promise<void> {
		if (busy[acc]) return;
		invalidateReads(acc);
		busy = { ...busy, [acc]: true };
		errors = { ...errors, [acc]: null };
		try {
			await api.oauthDeleteTokens(acc);
			if (disposed) return;
			// Also invalidate reads started by events while deletion was pending.
			invalidateReads(acc);
			// サーバ側で session も除去されるので、こちらも反映。
			sessions = { ...sessions, [acc]: null };
			authorizedThisSession = { ...authorizedThisSession, [acc]: false };
			authorization = { ...authorization, [acc]: false };
		} catch (e) {
			if (disposed) return;
			errors = { ...errors, [acc]: formatError(e) };
		} finally {
			busy = { ...busy, [acc]: false };
		}
	}

	function formatError(e: unknown): string {
		if (e instanceof ControlApiError) {
			return `${e.status} ${e.statusText}: ${typeof e.body === "string" ? e.body : JSON.stringify(e.body)}`;
		}
		return e instanceof Error ? e.message : String(e);
	}

	// ---- UI helper ----------------------------------------------------------

	function statusTone(s: OAuthSessionStatus): string {
		switch (s) {
			case "pending":
				return "bg-warning-200-800 text-warning-900-100";
			case "authorized":
				return "bg-success-200-800 text-success-900-100";
			case "expired":
			case "canceled":
				return "bg-surface-300-700 text-surface-800-200";
			case "failed":
				return "bg-error-200-800 text-error-900-100";
		}
	}

	/** expires_at までの残り秒（負なら 0）。tick 依存で 1Hz 更新。 */
	function remainingSecs(view: OAuthSessionView | null): number {
		if (!view) return 0;
		// tick を参照しておくことで reactivity を発火させる
		void tick;
		const ms = new Date(view.expires_at).getTime() - Date.now();
		return Math.max(0, Math.floor(ms / 1000));
	}

	function formatCountdown(secs: number): string {
		const m = Math.floor(secs / 60);
		const s = secs % 60;
		return `${m}:${String(s).padStart(2, "0")}`;
	}

	async function copyText(text: string): Promise<void> {
		try {
			await navigator.clipboard.writeText(text);
		} catch {
			// clipboard API が使えない環境はサイレントに失敗（表示自体はあるので手動コピー可能）
		}
	}

	/**
	 * 「保存済みトークンで既に認可済みか」のフラグ。
	 * 専用authorization API由来で、DCF開始前から判定できる。
	 * さらに「現セッションが authorized 状態である」もここで OR しておくことで、
	 * /start → /status → WS event で authorized になった瞬間も即反映される。
	 */
	const authorizedMap = $derived<Record<OAuthAccount, boolean>>({
		broadcaster:
			authorizedThisSession.broadcaster || authorization.broadcaster === true || sessions.broadcaster?.status === "authorized",
		moderator: authorizedThisSession.moderator || authorization.moderator === true || sessions.moderator?.status === "authorized",
	});

	/**
	 * 「そのアカウントで認可を扱うための設定が揃っているか」。
	 * 揃っていないタブでは DCF 開始・トークン削除などの操作ボタンを無効化する。
	 *   - broadcaster: `[twitch.eventsub]` があり enabled=true であること
	 *   - moderator:   `[twitch.moderator]`セクション自体があること
	 */
	const configuredMap = $derived<Record<OAuthAccount, boolean>>({
		broadcaster: configured.broadcaster === true,
		moderator: configured.moderator === true,
	});

	/** 未設定時に表示する理由文（設定済みなら null）。 */
	const currentNotConfiguredReason: string | null = $derived.by(() => {
		if (configured[tab] === null) return "Twitch認可設定を確認しています。";
		if (tab === "broadcaster") {
			return configured.broadcaster ? null : "配信者の接続設定がまだありません。上の基本設定を保存・適用してください。";
		}
		return configured.moderator ? null : "投稿用アカウントの設定がまだありません。上の基本設定を保存・適用してください。";
	});

	const currentSession = $derived(sessions[tab]);
	const currentBusy = $derived(busy[tab]);
	const currentError = $derived(errors[tab]);
	const currentAuthorized = $derived(authorizedMap[tab]);
	const currentConfigured = $derived(configuredMap[tab]);
	const isPending = $derived(currentSession?.status === "pending");
	const remaining = $derived(remainingSecs(currentSession));
</script>

<section class="rounded-lg border border-surface-300-700 bg-surface-100-900 p-4">
	<header class="mb-3 flex items-center justify-between">
		<h3 class="text-sm font-semibold">Twitchアカウントの認証</h3>
		<button
			type="button"
			class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-0.5 text-[10px] hover:bg-surface-200-800"
			onclick={() => void Promise.all([refreshAuthorization(tab), refreshStatus(tab)])}
		>
			↻ 再取得
		</button>
	</header>

	{#if deleteReview}
		<div class="mb-3 rounded border border-warning-500 p-3 text-sm" role="region" aria-label="Twitch認証情報の削除確認">
			<p>{deleteReview.account==='broadcaster' ? '配信者' : 'チャット投稿用'}の保存済み認証情報を削除する？</p>
			<p class="mt-2 text-xs">同じ認証情報を共有するVACにも影響し、次回の接続には再認証が必要になる。起動中のTwitch接続は、そのまま続く場合がある。</p>
			<div class="mt-3 flex flex-wrap gap-3">
				<button bind:this={keepTokenButton} type="button" class="rounded border border-surface-300-700 px-3 py-2" onclick={()=>deleteReview=null}>認証情報を残して戻る</button>
				<button type="button" class="rounded border border-error-500 px-3 py-2" disabled={busy[deleteReview.account]} onclick={()=>void confirmDeleteTokens()}>この役割の認証情報を削除</button>
			</div>
		</div>
	{/if}
	<!-- タブ -->
	<div class="mb-3 flex gap-1 border-b border-surface-300-700">
		{#each ACCOUNTS as acc (acc)}
			<button
				type="button"
				class="rounded-t px-3 py-1 text-xs font-semibold capitalize {tab === acc
					? 'bg-surface-50-950 text-primary-500'
					: 'opacity-60 hover:opacity-90'}"
				disabled={deleteReview !== null}
				onclick={() => (tab = acc)}
			>
				{acc === "broadcaster" ? "配信者" : "チャット投稿用"}
				{#if sessions[acc]}
					<span class="ml-1 inline-block rounded px-1 text-[10px] {statusTone(sessions[acc]!.status)}">
						{sessions[acc]!.status}
					</span>
				{:else if authorizedMap[acc]}
					<span class="ml-1 inline-block rounded bg-success-200-800 px-1 text-[10px] text-success-900-100"> authorized </span>
				{/if}
			</button>
		{/each}
	</div>

	{#if currentError}
		<div class="mb-2 rounded bg-error-200-800 px-2 py-1 text-xs text-error-900-100">
			ERROR: {currentError}
		</div>
	{/if}

	{#if !currentSession}
		<div class="space-y-2 text-xs">
			{#if currentAuthorized}
				<div class="rounded-lg border border-success-500 bg-success-200-800/60 p-3 text-success-900-100">
					<div class="mb-1 font-semibold">✓ 既に認可済み</div>
					<p class="text-[11px] opacity-90">
						<code class="font-mono">{tab}</code> アカウントの有効なアクセストークンが保存されています。 Device Code Flow は不要です（VAC
						は起動時に保存済みトークンを読み込みます）。
					</p>
				</div>
			{:else if currentNotConfiguredReason}
				<p class="opacity-70">
					{currentNotConfiguredReason}
				</p>
			{:else}
				<p class="opacity-70">
					{tab} アカウントの OAuth セッションはまだ開始されていません。
				</p>
			{/if}

			<!-- アクションボタン群。broadcaster / moderator で同一レイアウト。 -->
			<div class="flex flex-wrap items-center gap-2">
				<button
					type="button"
					class="rounded bg-primary-500 px-3 py-1 font-semibold text-primary-950 hover:bg-primary-400 disabled:opacity-50"
					onclick={() => void start(tab)}
					disabled={deleteReview !== null || currentBusy || !currentConfigured}
				>
					{currentBusy ? "開始中…" : currentAuthorized ? "それでも再試行" : "Twitchで認証する"}
				</button>
				{#if currentAuthorized}
					<button
						type="button"
						class="rounded border border-error-500 bg-error-500/20 px-3 py-1 font-semibold text-error-900-100 hover:bg-error-500/40 disabled:opacity-50"
						onclick={() => requestDeleteTokens(tab)}
						disabled={deleteReview !== null || currentBusy || !currentConfigured}
						title="保存済みトークンキャッシュファイルを削除します（再認可が必要になります）"
					>
						⚠ トークンキャッシュを削除
					</button>
				{/if}
			</div>
			{#if currentAuthorized}
				<p class="text-[10px] opacity-60">
					※ 削除しても既に起動中の Twitch 接続は継続します。完全なリセットが必要なら VAC を再起動してください。
				</p>
			{/if}
		</div>
	{:else}
		{@const v = currentSession}
		<div class="space-y-3 text-xs">
			<!-- status ヘッダ -->
			<div class="flex flex-wrap items-center gap-2">
				<span class="rounded px-2 py-0.5 font-mono font-semibold uppercase {statusTone(v.status)}">
					{v.status}
				</span>
				<span class="opacity-70">
					started: {new Date(v.started_at).toLocaleTimeString("ja-JP", { hour12: false })}
				</span>
				{#if isPending}
					<span class="font-mono opacity-80">
						認可待ち ・ 残り {formatCountdown(remaining)}
					</span>
				{/if}
				{#if v.last_error}
					<span class="rounded bg-error-200-800 px-2 py-0.5 text-[10px] text-error-900-100">
						last_error: {v.last_error}
					</span>
				{/if}
			</div>

			<!-- user_code / verification_uri 提示（pending のときに最も重要） -->
			{#if isPending}
				<div class="rounded-lg border border-primary-500 bg-primary-50-950 p-3">
					<p class="mb-2 text-[11px] text-primary-200">
						ブラウザーで認可を完了すると自動で切り替わります。Twitchへの確認間隔により、反映まで最大 {v.interval_secs} 秒かかります。
					</p>
					<div class="mb-2 text-[10px] opacity-70">1. 以下の URL をブラウザで開き、2. コードを入力して認証してください。</div>

					<!-- user_code -->
					<div class="mb-2 flex items-center gap-2">
						<span class="w-20 shrink-0 opacity-60">user code</span>
						<code class="flex-1 rounded bg-surface-50-950 px-2 py-1 text-center text-2xl font-bold tracking-widest">
							{v.user_code}
						</code>
						<button
							type="button"
							class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 hover:bg-surface-200-800"
							onclick={() => void copyText(v.user_code)}
							title="user_code をクリップボードにコピー"
						>
							Copy
						</button>
					</div>

					<!-- verification_uri -->
					<div class="flex items-center gap-2">
						<span class="w-20 shrink-0 opacity-60">verify URL</span>
						<code class="flex-1 truncate rounded bg-surface-50-950 px-2 py-1 font-mono" title={v.verification_uri}>
							{v.verification_uri}
						</code>
						<button
							type="button"
							disabled={deleteReview !== null || currentBusy}
							onclick={() => void openBrowser(tab)}
							class="rounded bg-primary-500 px-2 py-1 font-semibold text-primary-950 hover:bg-primary-400"
						>
							ブラウザーで開く
						</button>
						<button
							type="button"
							class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 hover:bg-surface-200-800"
							onclick={() => void copyText(v.verification_uri)}
						>
							Copy
						</button>
					</div>
				</div>
			{/if}

			<!-- アクション -->
			<div class="flex items-center gap-2">
				{#if isPending}
					<button
						type="button"
						class="rounded bg-error-500 px-3 py-1 font-semibold text-error-950 hover:bg-error-400 disabled:opacity-50"
						onclick={() => void cancel(tab)}
						disabled={deleteReview !== null || currentBusy}
					>
						{currentBusy ? "…" : "Cancel"}
					</button>
				{:else}
					<button
						type="button"
						class="rounded bg-primary-500 px-3 py-1 font-semibold text-primary-950 hover:bg-primary-400 disabled:opacity-50"
						onclick={() => void start(tab)}
						disabled={deleteReview !== null || currentBusy}
					>
						{currentBusy ? "開始中…" : "Twitchで認証し直す"}
					</button>
				{/if}
				<span class="opacity-60">
					account: <code class="font-mono">{tab}</code>
				</span>
			</div>
		</div>
	{/if}
</section>

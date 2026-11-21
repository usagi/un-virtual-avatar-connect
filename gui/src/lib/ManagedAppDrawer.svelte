<script lang="ts">
	/**
	 * Phase VI-γ-2b: Managed App 持続ドロワー。
	 *
	 * - ヘッダ右肩の「連携アプリ」ボタンで開閉。
	 * - 右側からスライドインし、アプリ一覧 + 各行の start/stop/minimize ボタンを置く。
	 * - WebSocket `managed_app_state` を購読してリアルタイム反映。
	 * - 操作ごとの軽量 toast を出す（api.ts 共通の error toast は request() 側で拾わないので、ここで投げる）。
	 *
	 * デザイン方針:
	 *   - shell は普通のアプリ風（見慣れた drawer）を優先。アクション失敗は目立つ警告色で見せる。
	 *   - 各行のボタンは「start = primary」「stop = error」「minimize = surface（Windows のみ）」の 3 つ。
	 *   - `supports_status=false` の entry は「状態不明」として扱い、start のみ許可。
	 */
	import { SvelteSet } from "svelte/reactivity";
	import { tick } from "svelte";
	import { api } from "./api";
	import { eventsStore } from "./events.svelte";
	import { toastStore } from "./toasts.svelte";
	import {
		ControlApiError,
		type ControlEvent,
		type ManagedAppView,
		type ManagedAppsResponse,
		type UnAppApiCommandHistoryItem,
	} from "./types";

	interface Props {
		open: boolean;
		targetId?: string | null;
	}
	let { open = $bindable(), targetId = null }: Props = $props();
	let drawer = $state<HTMLElement>();
	$effect(() => {
		if (!open || loading || !targetId) return;
		const id = targetId;
		let cancelled = false;
		void tick().then(() => {
			if (cancelled) return;
			const row = Array.from(drawer?.querySelectorAll<HTMLElement>("[data-managed-target]") ?? []).find(
				(item) => item.dataset.managedTarget === id
			);
			row?.scrollIntoView({ block: "center" });
			row?.focus({ preventScroll: true });
		});
		return () => {
			cancelled = true;
		};
	});

	let loading = $state(true);
	let error = $state<string | null>(null);
	let resp = $state<ManagedAppsResponse | null>(null);
	let commandHistory = $state<UnAppApiCommandHistoryItem[]>([]);
	const commandStaleMs = 10_000;
	/** 操作中の app id セット。ボタンの spinner 表示に使う。SvelteSet で細粒度リアクティブ。 */
	const busyIds = new SvelteSet<string>();
	const expandedIds = new SvelteSet<string>();
	let actionReview = $state<{ action: "stop" | "restart"; id: string; label: string; command: string; signature: string } | null>(null);
	let reviewing = $state(false);

	function operationSignature(entry: ManagedAppView): string {
		return JSON.stringify([entry.id, entry.command, entry.working_dir, entry.process_marker,
			entry.run_as_admin, entry.supports_status, entry.status.running, [...entry.status.pids].sort((a,b)=>a-b)]);
	}

	function requestOperation(action: "stop" | "restart", entry: ManagedAppView) {
		if (reviewing || busyIds.has(entry.id) || !entry.supports_status) return;
		actionReview = { action, id: entry.id, label: entry.label, command: entry.command, signature: operationSignature(entry) };
	}

	async function confirmOperation() {
		const reviewed = actionReview;
		if (!reviewed || reviewing || busyIds.has(reviewed.id)) return;
		reviewing = true;
		try {
			const latest = await api.managedApps();
			if (!open || actionReview !== reviewed) return;
			resp = latest;
			const entry = latest.entries.find(item => item.id === reviewed.id);
			if (!entry || operationSignature(entry) !== reviewed.signature) {
				actionReview = null;
				toastStore.warn("対象の設定または稼働状態が変わったため、操作を中止した", "最新の対象を確認して操作し直してください。");
				return;
			}
			actionReview = null;
			if (reviewed.action === "stop") await actStop(entry);
			else await actRestart(entry);
		} catch (e) {
			toastStore.error("操作対象を確認できませんでした", formatErr(e));
		} finally { reviewing = false; }
	}

	const unaaZCommandButtons = [
		{ command: "focus", label: "Focus" },
		{ command: "open_gui", label: "Open GUI" },
		{ command: "reload.preview", label: "Reload preview" },
		{ command: "profile.switch.preview", label: "Profile preview" },
	] as const;

	async function load() {
		loading = true;
		error = null;
		try {
			const [nextResp, history] = await Promise.all([api.managedApps(), api.unAppApiCommandHistory(30)]);
			resp = nextResp;
			commandHistory = history.commands;
		} catch (e) {
			error = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		// open になったときに取得。閉じている間は購読だけ効かせておき、開く都度リフレッシュ。
		if (open) {
			void load();
		} else { actionReview = null; }
	});

	// managed_app_state を受けたら対象 entry の status を上書き。全件再フェッチはしない（負荷軽減）。
	$effect(() => {
		const off = eventsStore.subscribe((ts) => {
			const ev: ControlEvent = ts.event;
			if (ev.kind !== "managed_app_state") return;
			if (!resp) return;
			const idx = resp.entries.findIndex((e) => e.id === ev.id);
			if (idx < 0) return;
			// 新しい status で entry を差し替え（配列自体も新規化してリアクティブに拾わせる）
			const next = resp.entries.slice();
			next[idx] = {
				...next[idx],
				status: {
					id: ev.id,
					running: ev.running,
					pids: ev.pids,
					checked_at: ev.checked_at,
				},
			};
			resp = { ...resp, entries: next };
		});
		return off;
	});

	function setBusy(id: string, busy: boolean) {
		if (busy) busyIds.add(id);
		else busyIds.delete(id);
	}

	function toggleDetails(id: string) {
		if (expandedIds.has(id)) expandedIds.delete(id);
		else expandedIds.add(id);
	}

	function allowsUnaaZCommand(entry: ManagedAppView, command: string) {
		return entry.unaa_z_status.allowed_commands.includes(command);
	}

	function latestCommandFor(entry: ManagedAppView): UnAppApiCommandHistoryItem | null {
		return (
			commandHistory
				.toReversed()
				.find((item) => item.app_id === entry.unaa_z_status.app_id && item.instance_id === entry.unaa_z_status.instance_id) ?? null
		);
	}

	function commandStatusClass(item: UnAppApiCommandHistoryItem): string {
		if (commandIsStale(item)) return "text-surface-500";
		if (item.status === "response_observed") return "text-success-500";
		if (item.status === "publish_failed") return "text-error-500";
		return "text-warning-500";
	}

	function commandIsStale(item: UnAppApiCommandHistoryItem): boolean {
		const t = Date.parse(item.at);
		return Number.isFinite(t) && Date.now() - t > commandStaleMs;
	}

	function formatAge(ms: number | null): string {
		if (ms == null || !Number.isFinite(ms)) return "age=unknown";
		if (ms < 1000) return `${Math.round(ms)}ms`;
		if (ms < 60_000) return `${Math.round(ms / 1000)}s`;
		return `${Math.round(ms / 60_000)}m`;
	}

	function commandSummary(entry: ManagedAppView): string {
		const allowed = entry.unaa_z_status.allowed_commands;
		if (allowed.length > 0) return allowed.join(", ");
		const advertised = entry.unaa_z_status.commands;
		return advertised.length > 0 ? `advertised: ${advertised.join(", ")}` : "none";
	}

	function capabilitySummary(entry: ManagedAppView): string {
		const caps = entry.unaa_z_status.capabilities;
		return caps.length > 0 ? caps.join(", ") : "none";
	}

	async function actStart(entry: ManagedAppView) {
		setBusy(entry.id, true);
		try {
			const r = await api.managedAppStart(entry.id);
			if (r.was_running) {
				toastStore.warn(`${entry.label} は既に起動中です`);
			} else {
				toastStore.success(`${entry.label} を起動しました`);
			}
		} catch (e) {
			const msg = formatErr(e);
			if (e instanceof ControlApiError && e.status === 409) {
				toastStore.warn(`${entry.label} は既に起動中です`, msg);
			} else {
				toastStore.error(`${entry.label} の起動に失敗`, msg);
			}
		} finally {
			setBusy(entry.id, false);
		}
	}

	async function actStop(entry: ManagedAppView) {

		setBusy(entry.id, true);
		try {
			const r = await api.managedAppStop(entry.id, { grace_ms: 3000 });
			if (r.terminated_pids > 0) {
				toastStore.warn(`${entry.label} を強制終了しました`, `closed=${r.closed_windows}, terminated=${r.terminated_pids}`);
			} else {
				toastStore.success(`${entry.label} にクローズ要求を送りました`, `closed_windows=${r.closed_windows}`);
			}
		} catch (e) {
			toastStore.error(`${entry.label} の停止に失敗`, formatErr(e));
		} finally {
			setBusy(entry.id, false);
		}
	}

	async function actRestart(entry: ManagedAppView) {

		setBusy(entry.id, true);
		try {
			const r = await api.managedAppRestart(entry.id, { grace_ms: 3000 });
			const stoppedPart = r.was_running
				? `stopped (closed=${r.closed_windows}, terminated=${r.terminated_pids})`
				: "起動していませんでした";
			toastStore.success(`${entry.label} を再起動しました`, stoppedPart);
		} catch (e) {
			toastStore.error(`${entry.label} の再起動に失敗`, formatErr(e));
		} finally {
			setBusy(entry.id, false);
		}
	}

	async function actMinimize(entry: ManagedAppView) {
		setBusy(entry.id, true);
		try {
			const r = await api.managedAppMinimize(entry.id);
			toastStore.info(`${entry.label} を最小化キューに入れました`, `pids=${r.scheduled_pids}`);
		} catch (e) {
			toastStore.error(`${entry.label} の最小化に失敗`, formatErr(e));
		} finally {
			setBusy(entry.id, false);
		}
	}

	async function actUnaaZCommand(entry: ManagedAppView, command: string) {
		const busyKey = `${entry.id}:unaa_z:${command}`;
		setBusy(busyKey, true);
		try {
			const r = await api.unAppApiCommand({
				managed_app_id: entry.id,
				command,
				args: {},
				timeout_ms: 800,
				wait_response_ms: 800,
			});
			if (r.response_observed) {
				toastStore.success(`${entry.label}: ${command} の応答を受信しました`, r.response?.key_expr ?? r.response_key_expr);
			} else {
				toastStore.info(`${entry.label}: ${command} を送信しました`, `response wait timeout / ${r.request_id}`);
			}
			const history = await api.unAppApiCommandHistory(30);
			commandHistory = history.commands;
		} catch (e) {
			toastStore.error(`${entry.label}: ${command} 送信に失敗`, formatErr(e));
			try {
				const history = await api.unAppApiCommandHistory(30);
				commandHistory = history.commands;
			} catch {
				// command failure toast is already enough; history refresh is best-effort.
			}
		} finally {
			setBusy(busyKey, false);
		}
	}

	function formatErr(e: unknown): string {
		if (e instanceof ControlApiError) return `${e.status} ${e.statusText}`;
		if (e instanceof Error) return e.message;
		return String(e);
	}

	function statusLabel(entry: ManagedAppView): string {
		if (!entry.supports_status) return "状態不明";
		return entry.status.running ? "起動中" : "停止";
	}
	function statusClass(entry: ManagedAppView): string {
		if (!entry.supports_status) return "bg-surface-300-700 text-surface-900-100";
		return entry.status.running ? "bg-success-300-700 text-success-900-100" : "bg-error-300-700/60 text-error-900-100";
	}

	function unaaZLabel(entry: ManagedAppView): string {
		if (entry.unaa_z_status.discovery_stale) return "UNAA/Z stale";
		if (entry.unaa_z_status.zenoh_seen) return entry.unaa_z_status.ready ? "UNAA/Z ready" : "UNAA/Z not ready";
		return "UNAA/Z registry";
	}

	function unaaZClass(entry: ManagedAppView): string {
		if (entry.unaa_z_status.discovery_stale) {
			return "bg-surface-300-700 text-surface-900-100";
		}
		if (entry.unaa_z_status.zenoh_seen && entry.unaa_z_status.ready) {
			return "bg-success-300-700 text-success-900-100";
		}
		if (entry.unaa_z_status.zenoh_seen) {
			return "bg-warning-300-700 text-warning-900-100";
		}
		return "bg-surface-300-700 text-surface-900-100";
	}

	function close() {
		actionReview = null;
		open = false;
	}

	function onBackdropKey(e: KeyboardEvent) {
		if (e.key === "Escape") close();
	}
</script>

{#if open}
	<!-- backdrop: 半透明オーバーレイ。クリックで閉じる -->
	<div
		class="fixed inset-0 z-40 bg-surface-950/40 backdrop-blur-sm"
		role="button"
		tabindex="-1"
		aria-label="連携アプリドロワーを閉じる"
		onclick={close}
		onkeydown={onBackdropKey}
	></div>
	<!-- drawer 本体 -->
	<aside
		bind:this={drawer}
		class="fixed right-0 top-0 z-50 flex h-screen w-full max-w-md flex-col border-l border-surface-200-800 bg-surface-50-950 text-surface-950-50 shadow-xl"
		aria-label="Managed app drawer"
	>
		<header class="flex items-center justify-between border-b border-surface-200-800 px-4 py-3">
			<div>
				<h2 class="text-sm font-semibold">連携アプリ</h2>
				<p class="text-xs opacity-60">run_with で管理するアプリの監視と操作</p>
			</div>
			<div class="flex items-center gap-2">
				<button
					type="button"
					class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-100-900"
					onclick={() => void load()}
					disabled={loading}
				>
					再読込
				</button>
				<button
					type="button"
					class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-100-900"
					aria-label="閉じる"
					onclick={close}
				>
					×
				</button>
			</div>
		</header>

		<div class="flex-1 overflow-y-auto p-3">
			{#if actionReview}
				<section aria-label="連携アプリ操作の確認" class="mb-3 rounded border border-warning-500 bg-warning-500/10 p-3 text-sm">
					<p class="font-semibold">{actionReview.label} を{actionReview.action === "stop" ? "停止" : "再起動"}しますか？</p>
					<p class="break-all text-xs">{actionReview.command}</p>
					<p class="mt-2 text-xs">終了を要求し、3秒以内に終了しなければ強制終了します。{actionReview.action === "restart" ? "その後、再び起動します。" : ""}未保存の内容は失われる場合があります。</p>
					<div class="mt-2 flex gap-2">
						<button class="rounded border px-2 py-1" disabled={reviewing} onclick={() => void confirmOperation()}>{reviewing ? "確認・実行中…" : actionReview.action === "stop" ? "停止を実行" : "再起動を実行"}</button>
						<button class="rounded border px-2 py-1" onclick={() => actionReview = null}>キャンセル</button>
					</div>
				</section>
			{/if}

			{#if loading}
				<p class="text-xs opacity-70">読み込み中…</p>
			{:else if error}
				<p class="rounded border border-error-500/40 bg-error-500/10 p-2 text-xs">{error}</p>
			{:else if !resp || resp.entries.length === 0}
				<p class="rounded border border-warning-500/40 bg-warning-500/10 p-2 text-xs">
					登録済みの Managed App がありません。<br />
					<code>conf.toml</code> の <code>[[run_with]]</code> エントリに
					<code>if_not_running = "プロセス名"</code> を書いてください。
				</p>
			{:else}
				<ul class="space-y-2">
					{#each resp.entries as entry (entry.id)}
						{@const busy = busyIds.has(entry.id) || reviewing || actionReview !== null}
						{@const unaaZBusy =
							busyIds.has(`${entry.id}:unaa_z:focus`) ||
							busyIds.has(`${entry.id}:unaa_z:open_gui`) ||
							busyIds.has(`${entry.id}:unaa_z:reload.preview`) ||
							busyIds.has(`${entry.id}:unaa_z:profile.switch.preview`)}
						{@const canStop = entry.supports_status && entry.status.running}
						{@const canMinimize = entry.supports_status && entry.status.running}
						{@const canUnaaZCommand =
							entry.unaa_z_status.zenoh_seen && entry.unaa_z_status.ready && entry.unaa_z_status.allowed_command_count > 0}
						{@const latestCommand = latestCommandFor(entry)}
						{@const detailsOpen = expandedIds.has(entry.id)}
						<li
							data-managed-target={entry.id}
							tabindex="-1"
							class="rounded border border-surface-200-800 bg-surface-100-900 p-3 focus:outline-2 focus:outline-primary-500"
							class:ring-2={targetId === entry.id}
						>
							<div class="flex items-start justify-between gap-2">
								<div class="min-w-0 flex-1">
									<div class="flex items-center gap-2">
										<span class="truncate text-sm font-semibold">{entry.label}</span>
										<span class="rounded px-1.5 py-0 text-[10px] font-semibold uppercase {statusClass(entry)}">
											{statusLabel(entry)}
										</span>
										<span class="rounded px-1.5 py-0 text-[10px] font-semibold uppercase {unaaZClass(entry)}">
											{unaaZLabel(entry)}
										</span>
									</div>
									<p class="mt-0.5 truncate font-mono text-[11px] opacity-60" title={entry.command}>
										{entry.command}
									</p>
									<div class="mt-1 flex flex-wrap gap-1 text-[10px] opacity-70">
										<code>id={entry.id}</code>
										{#if entry.process_marker}
											<code>marker={entry.process_marker}</code>
										{/if}
										{#if entry.minimized}
											<code>minimized</code>
										{/if}
										{#if entry.run_as_admin}
											<code class="text-warning-500">admin</code>
										{/if}
										<code>unaa_z={entry.unaa_z_status.app_id}/{entry.unaa_z_status.instance_id}</code>
										<code title={entry.unaa_z_status.discovery_source_key ?? entry.unaa_z_status.discovery_source}>
											via={entry.unaa_z_status.discovery_payload_kind}
										</code>
										<code>caps={entry.unaa_z_status.capability_count}</code>
										<code>commands={entry.unaa_z_status.allowed_command_count}/{entry.unaa_z_status.command_count}</code
										>
										{#if latestCommand}
											<code class={commandStatusClass(latestCommand)}>
												last={latestCommand.command}:{latestCommand.status}{commandIsStale(latestCommand)
													? ":stale"
													: ""}
											</code>
										{/if}
										<code>endpoints={entry.unaa_z_status.endpoint_count}</code>
										{#if entry.supports_status && entry.status.running && entry.status.pids.length > 0}
											<code>pids=[{entry.status.pids.join(",")}]</code>
										{/if}
									</div>
								</div>
							</div>

							<div class="mt-2 flex flex-wrap gap-1.5">
								<button
									type="button"
									class="rounded bg-primary-500 px-2 py-1 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
									onclick={() => void actStart(entry)}
									disabled={busy || (entry.supports_status && entry.status.running)}
								>
									起動
								</button>
								<button
									type="button"
									class="rounded border border-error-500 px-2 py-1 text-xs font-semibold text-error-500 hover:bg-error-500 hover:text-white disabled:opacity-30"
									onclick={() => requestOperation("stop", entry)}
									disabled={busy || !canStop}
								>
									停止
								</button>
								<button
									type="button"
									class="rounded border border-warning-500 px-2 py-1 text-xs font-semibold text-warning-500 hover:bg-warning-500 hover:text-white disabled:opacity-30"
									onclick={() => requestOperation("restart", entry)}
									disabled={busy || !entry.supports_status}
									title="停止 → 起動 の連続操作"
								>
									再起動
								</button>
								<button
									type="button"
									class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800 disabled:opacity-30"
									onclick={() => void actMinimize(entry)}
									disabled={busy || !canMinimize}
									title="Windows 専用"
								>
									最小化
								</button>
								<button
									type="button"
									class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800"
									aria-expanded={detailsOpen}
									aria-controls={`managed-app-details-${entry.id}`}
									onclick={() => toggleDetails(entry.id)}
								>
									詳細
								</button>
							</div>

							{#if canUnaaZCommand}
								<div class="mt-2 flex flex-wrap gap-1.5 border-t border-surface-200-800 pt-2">
									{#each unaaZCommandButtons as button (button.command)}
										{#if allowsUnaaZCommand(entry, button.command)}
											<button
												type="button"
												class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800 disabled:opacity-30"
												onclick={() => void actUnaaZCommand(entry, button.command)}
												disabled={unaaZBusy || reviewing || actionReview !== null}
											>
												{button.label}
											</button>
										{/if}
									{/each}
								</div>
							{/if}

							{#if detailsOpen}
								<div
									id={`managed-app-details-${entry.id}`}
									class="mt-2 border-t border-surface-200-800 pt-2 text-[11px]"
									data-testid={`managed-app-details-${entry.id}`}
								>
									<dl class="grid gap-x-3 gap-y-1 sm:grid-cols-[7rem_minmax(0,1fr)]">
										<dt class="font-semibold opacity-60">working dir</dt>
										<dd class="min-w-0 break-all font-mono">{entry.working_dir ?? "(default)"}</dd>
										<dt class="font-semibold opacity-60">process</dt>
										<dd class="min-w-0 break-all font-mono">
											marker={entry.process_marker ?? "(none)"} / checked={entry.status.checked_at}
										</dd>
										<dt class="font-semibold opacity-60">UNAA/Z app</dt>
										<dd class="min-w-0 break-all font-mono">
											{entry.unaa_z_status.app_id}/{entry.unaa_z_status.instance_id}
										</dd>
										<dt class="font-semibold opacity-60">discovery</dt>
										<dd class="min-w-0 break-all font-mono">
											{entry.unaa_z_status.discovery_source} / {entry.unaa_z_status.discovery_payload_kind} /
											{formatAge(entry.unaa_z_status.discovery_age_ms)}
										</dd>
										<dt class="font-semibold opacity-60">source key</dt>
										<dd class="min-w-0 break-all font-mono">{entry.unaa_z_status.discovery_source_key ?? "(none)"}</dd>
										<dt class="font-semibold opacity-60">capabilities</dt>
										<dd class="min-w-0 break-words">{capabilitySummary(entry)}</dd>
										<dt class="font-semibold opacity-60">commands</dt>
										<dd class="min-w-0 break-words">{commandSummary(entry)}</dd>
										<dt class="font-semibold opacity-60">endpoints</dt>
										<dd>{entry.unaa_z_status.endpoint_count}</dd>
										{#if latestCommand}
											<dt class="font-semibold opacity-60">latest command</dt>
											<dd class="min-w-0 break-all font-mono {commandStatusClass(latestCommand)}">
												{latestCommand.command} / {latestCommand.status} / {latestCommand.request_id}
												{#if latestCommand.error}
													/ {latestCommand.error}
												{/if}
											</dd>
										{/if}
									</dl>
								</div>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
		</div>
	</aside>
{/if}

<script lang="ts">
	/**
	 * Phase VI-γ-1: アプリケーションシェル。
	 *
	 * - 上部ヘッダ（ロゴ / 接続インジケータ / 再起動ボタン）
	 * - 左ナビゲーション（Now / Modes / Flowgraph Studio / Resources / Observability / Settings）
	 * - 下部ステータスバー（常駐）
	 * - 右下トーストレイヤ
	 * - 再起動・プロファイル切替モーダル
	 *
	 * 各タブの中身はコンテナコンポーネントに切り出し、本ファイルは薄い shell に徹する。
	 *
	 * δ-9 D.5: 旧 V1 `Pipeline` タブを廃止。Flowgraph タブに統合済み。
	 */
	import { linkedAppDraft } from "./lib/linkedAppDraft.svelte";
	import { tick } from "svelte";
	import ConnectionBadge from "./lib/ConnectionBadge.svelte";
	import TabNav from "./lib/TabNav.svelte";
	import PreviewFeatureNav from "./lib/PreviewFeatureNav.svelte";
	import StatusBar from "./lib/StatusBar.svelte";
	import ToastLayer from "./lib/ToastLayer.svelte";
	import ToastBridge from "./lib/ToastBridge.svelte";
	import RestartDialog from "./lib/RestartDialog.svelte";
	import ShutdownDialog from "./lib/ShutdownDialog.svelte";
	import DiscardDraftDialog from "./lib/DiscardDraftDialog.svelte";
	import ManagedAppDrawer from "./lib/ManagedAppDrawer.svelte";
	import RuntimeTargets from "./lib/RuntimeTargets.svelte";
	import LocalVacInstances from "./lib/LocalVacInstances.svelte";
	import ConnectionAccess from "./lib/ConnectionAccess.svelte";
	import SetupExperience from "./lib/SetupExperience.svelte";
	import "./lib/theme.svelte";

	import NowTab from "./lib/tabs/NowTab.svelte";
	import LiveTab from "./lib/tabs/LiveTab.svelte";
	import ModesTab from "./lib/tabs/ModesTab.svelte";
	import ResourcesTab from "./lib/tabs/ResourcesTab.svelte";
	import SettingsTab from "./lib/tabs/SettingsTab.svelte";
	import FlowgraphTab from "./lib/tabs/FlowgraphTab.svelte";
	import LogsTab from "./lib/tabs/LogsTab.svelte";
	import AiPartnersTab from "./lib/tabs/AiPartnersTab.svelte";
	import DictionaryTab from "./lib/tabs/DictionaryTab.svelte";

	import { tabNavStore } from "./lib/tabs.svelte";
	import { flowgraphStore } from "./lib/flowgraphStore.svelte";
	import { api } from "./lib/api";
	import { toastStore } from "./lib/toasts.svelte";
	import type { DesktopProfilesResponse } from "./lib/types";

	// Drafts outlive the editor tab; protect them while another page is visible too.
	function protectFlowgraphDraft(event: BeforeUnloadEvent) {
		if (!flowgraphStore.hasUnsavedDrafts && !linkedAppDraft.value) return;
		event.preventDefault();
		event.returnValue = "";
	}
	$effect(() => {
		const blocked = () => toastStore.error("終了を保留しました", "未保存の編集や保存中の操作を確認してから、再度終了してください。");
		window.addEventListener("beforeunload", protectFlowgraphDraft);
		window.addEventListener("vac-close-blocked", blocked);
		document.documentElement.dataset.unCloseGuardReady = "true";
		return () => {
			delete document.documentElement.dataset.unCloseGuardReady;
			window.removeEventListener("beforeunload", protectFlowgraphDraft);
			window.removeEventListener("vac-close-blocked", blocked);
		};
	});

	function downloadHeldDraft(root: string, fq: string) {
		try {
			const text = flowgraphStore.workspaceDraftText(root, fq);
			if (text === null) throw new Error("保持中の下書きが見つかりません。");
			const recovery = text.startsWith('{\n  "format": "unvac-property-draft-recovery-v1"');
			const url = URL.createObjectURL(new Blob([text], { type: recovery ? "application/json;charset=utf-8" : "application/toml;charset=utf-8" }));
			const link = document.createElement("a");
			link.href = url;
			link.download = `${fq.replace(/[^a-zA-Z0-9_-]/g, "_")}.${recovery ? "draft-recovery.json" : "draft.flowgraph.toml"}`;
			link.click();
			setTimeout(() => URL.revokeObjectURL(url), 1000);
		} catch (error) {
			toastStore.error("下書きの書出に失敗しました", String(error));
		}
	}
	let discardTarget = $state<{ root: string; fq: string; text: string } | null>(null);
	let discardError = $state<string | null>(null);
	function discardHeldDraft(root: string, fq: string) {
		if (flowgraphStore.mutating) return;
		try {
			const text = flowgraphStore.workspaceDraftText(root, fq);
			if (text === null) throw new Error("保持中の下書きが見つかりません。");
			discardError = null;
			discardTarget = { root, fq, text };
		} catch (error) {
			toastStore.error("下書きを確認できません", String(error));
		}
	}
	function confirmDiscardHeldDraft() {
		const target = discardTarget;
		if (!target) return;
		try {
			if (flowgraphStore.workspaceDraftText(target.root, target.fq) !== target.text) {
				discardError = "確認中に下書きが変更されたか、編集画面へ移動しました。キャンセルして内容を確認してください。";
				return;
			}
			if (!flowgraphStore.discardWorkspaceDraft(target.root, target.fq)) {
				discardError = "下書きを破棄できません。保存処理の完了を待つか、キャンセルして内容を確認してください。";
				return;
			}
			discardTarget = null;
			discardError = null;
		} catch (error) {
			discardError = String(error);
		}
	}

	let restartDialogOpen = $state(false);
	let localVacStatus = $state({ count: 0, failed: false });
	let connectionDetails = $state<HTMLDetailsElement>();
	async function showLocalVacs() {
		if (!connectionDetails) return;
		connectionDetails.open = true;
		await tick();
		const list = connectionDetails.querySelector<HTMLDetailsElement>("[data-local-vac-list]");
		if (list) list.open = true;
		const summary = list?.querySelector("summary");
		summary?.focus();
		summary?.scrollIntoView({ block: "nearest" });
	}
	// Normal builds use the approved functional index; preview builds retain the baseline comparison.
	let previewOutline = $state(import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1" || (typeof window !== "undefined" && new URL(window.location.href).searchParams.get("navigation") === "outline"));
	$effect(() => {
		if (import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1") return;
		const change = (event: Event) => { previewOutline = (event as CustomEvent).detail === "outline"; };
		window.addEventListener("un-preview-navigation", change);
		return () => window.removeEventListener("un-preview-navigation", change);
	});
	let managedAppDrawerOpen = $state(false);
	let managedAppTarget = $state<string | null>(null);
	let shutdownInFlight = $state(false);
	let shutdownDialogOpen = $state(false);
	let shutdownError: string | null = $state(null);
	let desktopProfiles = $state<DesktopProfilesResponse | null>(null);
	let profileCheckComplete = $state(false);
	let profileLookupFailed = $state(false);
	let profileLookupRetry = $state(0);
	const connectionLabel = typeof window === "undefined" ? "" : window.location.host;
	const activeProfile = $derived(desktopProfiles?.profiles.find((profile) => profile.id === desktopProfiles?.current_profile_id) ?? null);
	const activeProfileLabel = $derived(
		activeProfile === null
			? desktopProfiles?.current_conf_path
				? "外部設定ファイル"
				: "実行対象を確認できません"
			: activeProfile.label.trim() || activeProfile.id
	);

	$effect(() => {
		// User retry restarts only the read-only identity lookup, not the app.
		void profileLookupRetry;
		let disposed = false;
		let pending = false;
		async function refreshIdentity() {
			if (pending) return;
			pending = true;
			try {
				const response = await api.desktopProfiles();
				if (!disposed) {
					desktopProfiles = response;
					profileLookupFailed = false;
				}
			} catch {
				// A transient lookup failure must not unmount editors or onboarding.
				if (!disposed) profileLookupFailed = true;
			} finally {
				pending = false;
				if (!disposed) profileCheckComplete = true;
			}
		}
		void refreshIdentity();
		const timer = setInterval(() => void refreshIdentity(), 5000);
		return () => {
			disposed = true;
			clearInterval(timer);
		};
	});

	/**
	 * Phase ε-1: VAC プロセスそのものを穏やかに終了させる。
	 *
	 * 停止は `POST /api/v1/control/shutdown`（= `ShutdownBroker` を trigger）に 1 本化している。
	 * 成功レスポンス後はサーバが ManagedApp 停止 → actix graceful stop を順に実行するため、
	 * WS は自然に切れる。GUI 側は停止画面を出さず、toast だけ出して静かに待機する。
	 */
	function canShutdown() {
		if (shutdownInFlight) return false;
		const review = new Event("beforeunload", { cancelable: true });
		window.dispatchEvent(review);
		if (review.defaultPrevented) {
			window.dispatchEvent(new Event("vac-close-blocked"));
			return false;
		}
		return true;
	}

	function handleShutdownClick() {
		if (canShutdown()) { shutdownError = null; shutdownDialogOpen = true; }
	}

	async function confirmShutdown() {
		if (shutdownInFlight) return;
		// Recheck drafts: the confirmation may have been open while a save finished.
		if (!canShutdown()) { shutdownDialogOpen = false; return; }
		shutdownError = null;
		shutdownInFlight = true;
		const wasInert = document.documentElement.inert;
		document.documentElement.inert = true;
		try {
			await api.shutdown();
			toastStore.info("アプリを終了しています");
		} catch (e) {
			shutdownInFlight = false;
			shutdownError = String(e);
			document.documentElement.inert = wasInert;
			toastStore.error("アプリ停止の要求に失敗しました", String(e));
		}
	}
</script>

{#if !profileCheckComplete}
	<main class="flex min-h-screen items-center justify-center bg-surface-50-950 text-sm text-surface-950-50 opacity-65">起動中…</main>
{:else if desktopProfiles?.managed_mode && !desktopProfiles.onboarding_completed}
	<SetupExperience store={desktopProfiles} connectionUnconfirmed={profileLookupFailed} />
{:else}
	<div class="vac-shell flex h-dvh min-h-0 flex-col overflow-hidden text-surface-950-50">
		<header class="vac-app-header sticky top-0 z-20 shrink-0 border-b backdrop-blur">
			<div class="flex flex-wrap items-center justify-between gap-2 px-4 py-2 lg:flex-nowrap lg:px-6">
				<div class="min-w-0">
					<h1 class="truncate text-base font-bold lg:text-lg">Virtual Avatar Connect</h1>
					{#if profileLookupFailed}
						<p class="text-xs text-warning-700-300" role="status">操作対象の最新状態を確認できません。表示を保持して再接続を待っています。</p>
						<button type="button" class="mt-1 rounded border border-surface-300-700 px-2 py-1 text-xs" onclick={() => (profileLookupRetry += 1)}>操作対象を再確認</button>
					{/if}
					{#if activeProfileLabel}
						<p class="truncate text-xs opacity-55" title={desktopProfiles?.current_conf_path ?? ""}>
							操作対象: {activeProfileLabel}{activeProfile ? ` · ${activeProfile.id}` : ""}
						</p>
						<p class="text-[10px] opacity-50">接続先: {connectionLabel}</p>
					{/if}
				</div>
				<div
					class="flex w-full min-w-0 flex-wrap items-center justify-start gap-2 sm:w-auto sm:flex-1 sm:justify-end lg:flex-none"
				>
					<ConnectionBadge />
					{#if localVacStatus.count > 1}
						<button type="button" class="shrink-0 rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-100-900"
							aria-controls="vac-connection-details" onclick={showLocalVacs}>
							{localVacStatus.failed ? "VAC一覧・更新未確認" : `起動中のVAC ${localVacStatus.count}`}
						</button>
					{/if}
					<button
						type="button"
						class="shrink-0 rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-100-900"
						title="連携アプリ（run_with）の起動/停止を管理"
						onclick={() => (managedAppDrawerOpen = true)}
					>
						<span class="sm:hidden">アプリ</span><span class="hidden sm:inline">連携アプリ…</span>
					</button>
					<button
						type="button"
						class="shrink-0 rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-100-900"
						title="VAC を再起動 / プロファイル切替"
						onclick={() => (restartDialogOpen = true)}
					>
						再起動
					</button>
					<button
						type="button"
						class="shrink-0 rounded border border-error-500/60 px-3 py-1 text-xs text-error-500 hover:bg-error-500/10 disabled:opacity-50"
						title="VAC アプリを終了する（連携アプリの停止も試みる）"
						disabled={shutdownInFlight}
						onclick={handleShutdownClick}
					>
						{shutdownInFlight ? "停止中…" : "終了"}
					</button>
				</div>
			</div>
		</header>

		<div class="flex flex-col flex-1 min-h-0 overflow-y-auto lg:grid lg:grid-cols-[240px_minmax(0,1fr)] lg:overflow-hidden">
			<aside class="vac-side-rail shrink-0 min-w-0 max-w-full overflow-hidden border-b lg:overflow-y-auto lg:border-b-0 lg:border-r">
				{#snippet navigationContent()}
				{#if linkedAppDraft.value}
					<div class="mx-3 mt-3 rounded border border-primary-500/40 p-3 text-xs" aria-label="連携アプリの未保存編集">
						<p class="font-semibold">連携アプリ設定を編集中</p>
						<p class="mt-1 break-all opacity-70">{linkedAppDraft.value.sourcePath}</p>
						<button type="button" class="mt-2 underline" onclick={() => tabNavStore.openFeature("resources", "run-with")}>編集に戻る</button>
					</div>
				{/if}
				{#if flowgraphStore.isDirty && flowgraphStore.currentFq}
					<div class="mx-3 mt-3 rounded border border-primary-500/40 p-3 text-xs" aria-label="保持中の編集">
						<p class="font-semibold">未保存の編集</p>
						<p class="mt-1 break-all">{flowgraphStore.currentFq}</p>
						<details class="mt-1 opacity-70"><summary class="cursor-pointer">保存先</summary><p class="break-all">{flowgraphStore.currentFile?.root_dir ?? "保存先を確認できません"}</p></details>
						{#if tabNavStore.active !== "flowgraph"}
							<button type="button" class="mt-2 underline underline-offset-2" onclick={() => tabNavStore.openFeature("flowgraph", "flowgraph-studio")}>編集に戻る</button>
						{/if}
					</div>
				{/if}
				{#if flowgraphStore.workspaceDrafts.length}
					<div class="mx-3 mt-2 text-xs" aria-label="ほかの未保存ファイル">
						<p class="font-semibold">ほかの未保存ファイル</p>
						{#each flowgraphStore.workspaceDrafts as draft}
							<div class="mt-2 break-all rounded border border-primary-500/30 p-2">
								<p>{draft.fq}</p>
								<details class="opacity-70"><summary>保存先</summary>{draft.root}</details>
								<button type="button" class="mt-1 underline" disabled={flowgraphStore.mutating} onclick={async () => {
									if (!tabNavStore.setActive("flowgraph")) return;
									await flowgraphStore.openFile(draft.fq!, draft.root);
								}}>編集を開く</button>
								<details class="mt-1">
									<summary class="cursor-pointer">下書きの書出・破棄</summary>
									<p class="mt-1 opacity-70">元ファイルを開けない場合も、編集中の内容をTOMLで取り出せます。依存ファイルは含みません。</p>
									<button type="button" class="mt-2 underline" onclick={() => downloadHeldDraft(draft.root!, draft.fq!)}>下書きを書き出す</button>
									<button type="button" class="ml-3 mt-2 underline" disabled={flowgraphStore.mutating} onclick={() => discardHeldDraft(draft.root!, draft.fq!)}>編集を破棄</button>
								</details>
							</div>
						{/each}
					</div>
				{/if}
				{#if previewOutline}<PreviewFeatureNav />{:else}<TabNav />{/if}
				{/snippet}
				{#if import.meta.env.VITE_UN_DESIGN_PREVIEW === "1"}
					<div class="vac-navigation-stack">{@render navigationContent()}</div>
				{:else}
					{@render navigationContent()}
				{/if}
				<details id="vac-connection-details" bind:this={connectionDetails} class="mx-3 my-3 border-t border-surface-200-800 pt-3"><summary class="cursor-pointer text-xs font-semibold">接続・起動中のVAC・連携先</summary>
				<LocalVacInstances onstatus={(status) => { localVacStatus = status; }} />
				<ConnectionAccess />
				<RuntimeTargets
					onselect={(id) => {
						managedAppTarget = id;
						managedAppDrawerOpen = true;
					}}
				/>
				</details>
			</aside>

			<main class="vac-main-surface min-w-0 shrink-0 px-4 py-4 lg:overflow-y-auto lg:px-6">
				{#if tabNavStore.active === "now"}
					<NowTab />
				{:else if tabNavStore.active === "live"}
					<LiveTab />
				{:else if tabNavStore.active === "ai"}
					<AiPartnersTab />
				{:else if tabNavStore.active === "dictionary"}
					<DictionaryTab />
				{:else if tabNavStore.active === "modes"}
					<ModesTab />
				{:else if tabNavStore.active === "flowgraph"}
					<FlowgraphTab />
				{:else if tabNavStore.active === "resources"}
					<ResourcesTab />
				{:else if tabNavStore.active === "observability"}
					<LogsTab />
				{:else if tabNavStore.active === "settings"}
					<SettingsTab />
				{/if}
			</main>
		</div>

		<ToastLayer />
		<StatusBar />
	</div>

	<ToastBridge />
	<RestartDialog bind:open={restartDialogOpen} />
	<ShutdownDialog bind:open={shutdownDialogOpen} busy={shutdownInFlight} error={shutdownError} target={activeProfileLabel} connection={connectionLabel} onconfirm={confirmShutdown} />
	{#if discardTarget}
		<DiscardDraftDialog root={discardTarget.root} fq={discardTarget.fq} error={discardError} busy={flowgraphStore.mutating} oncancel={() => { discardTarget = null; discardError = null; }} onconfirm={confirmDiscardHeldDraft} />
	{/if}
	<ManagedAppDrawer bind:open={managedAppDrawerOpen} targetId={managedAppTarget} />
{/if}

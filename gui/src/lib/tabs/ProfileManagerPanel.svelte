<script lang="ts">
	import { linkedAppDraft } from "../linkedAppDraft.svelte";
	import { onMount, tick } from "svelte";
	import { tabNavStore } from "../tabs.svelte";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	/**
	 * Phase VI-γ-4b: プロファイル（conf ファイル）管理パネル。
	 *
	 * - `api.profiles()` で一覧を取得し、複製・リネーム・削除・編集・切替起動を GUI から実行できる。
	 * - 編集は本ファイル内で `ProfileEditorModal` を開いて `<textarea>` で TOML を直接いじる。
	 *   より高機能なエディタ（Monaco 等）への差し替えは後続フェーズで。
	 * - 現 conf（`is_current`）のリネーム・削除は disable。編集は「再起動するまで反映されない」旨を警告する。
	 */
	import { api } from "../api";
	import { ControlApiError } from "../types";
	import type { ProfileEntry } from "../types";
	import { toastStore } from "../toasts.svelte";
	import ProfileEditorModal from "./ProfileEditorModal.svelte";
	import ProfileBackupsPanel from "./ProfileBackupsPanel.svelte";

	let loading = $state(false);
	let loadRequest = 0;
	let entries = $state<ProfileEntry[]>([]);
	let currentPath = $state<string | null>(null);
	let directory = $state<string | null>(null);
	let error = $state<string | null>(null);

	// インライン入力系の UI 状態: 「複製中の行」「リネーム中の行」を filename で持つ。
	let cloneFor = $state<string | null>(null);
	let cloneName = $state("");
	let renameFor = $state<string | null>(null);
	let renameName = $state("");
	let busyName = $state<string | null>(null);
	const hasNameDraft = $derived(cloneFor !== null || (renameFor !== null && renameName !== renameFor));
	type Review = { kind: "delete" | "switch" | "discard"; filename: string; path: string; directory: string | null; current: string | null; draft: string };
	let confirmation = $state<Review | null>(null);
	let keepButton = $state<HTMLButtonElement>();
	$effect(() => { if (confirmation) void tick().then(() => keepButton?.focus()); });
	function draftKey() { return JSON.stringify([cloneFor, cloneName, renameFor, renameName]); }
	function mayLeave() {
		if (busyName || confirmation) return false;
		if (hasNameDraft) { error = "複製・名前変更の入力が残っています。実行するか、入力欄の「取消」から破棄してください。"; return false; }
		return true;
	}
	function requestReview(kind: Review["kind"], entry?: ProfileEntry) {
		if (busyName || loading || confirmation || editingFilename) return;
		if (kind !== "discard" && (!entry || entry.is_current || !mayLeave())) return;
		confirmation = { kind, filename: entry?.filename ?? cloneFor ?? renameFor ?? "", path: entry?.path ?? "", directory, current: currentPath, draft: draftKey() };
	}
	async function confirmOperation() {
		if (busyName || loading || !confirmation) return;
		const review = confirmation;
		confirmation = null;
		if (review.directory !== directory || review.current !== currentPath || review.draft !== draftKey()) {
			error = "確認中に対象または入力が変わりました。内容を確認し直してください。"; return;
		}
		if (review.kind === "discard") { cloneFor = null; cloneName = ""; renameFor = null; renameName = ""; error = null; return; }
		const entry = entries.find(e => e.filename === review.filename && e.path === review.path);
		if (!entry || entry.is_current) { error = "対象が変わりました。一覧を確認し直してください。"; return; }
		if (!tabNavStore.canLeaveCurrent()) return;
		if (review.kind === "delete") await doDelete(entry);
		else await switchTo(entry);
	}
	onMount(() => tabNavStore.guardLeaving(mayLeave));

	// 編集モーダル
	let editingFilename = $state<string | null>(null);

	async function load() {
		const request = ++loadRequest;
		loading = true;
		error = null;
		try {
			const res = await api.profiles();
			if (request !== loadRequest) return;
			entries = res.entries;
			currentPath = res.current;
			directory = res.directory;
		} catch (e) {
			if (request !== loadRequest) return;
			error = extractMessage(e);
		} finally {
			if (request === loadRequest) loading = false;
		}
	}

	$effect(() => {
		void load();
	});

	function extractMessage(e: unknown): string {
		if (e instanceof ControlApiError) {
			const body = e.body as { detail?: string; error?: string } | null;
			return body?.detail ?? body?.error ?? `${e.status} ${e.statusText}`;
		}
		if (e instanceof Error) return e.message;
		return String(e);
	}

	function fmtBytes(n: number): string {
		if (n < 1024) return `${n} B`;
		if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
		return `${(n / 1024 / 1024).toFixed(1)} MiB`;
	}

	function fmtModified(iso: string | null): string {
		if (!iso) return "—";
		const d = new Date(iso);
		if (Number.isNaN(d.valueOf())) return iso;
		return d.toLocaleString();
	}

	function startClone(entry: ProfileEntry) {
		if (!mayLeave()) return;
		cloneFor = entry.filename;
		renameFor = null;
		// `foo.toml` → `foo.copy.toml` のような初期値
		const base = entry.filename.replace(/\.toml$/i, "");
		cloneName = `${base}.copy.toml`;
	}

	function cancelClone() {
		if (busyName || confirmation) return;
		requestReview("discard");
	}

	async function submitClone(source: string) {
		if (busyName || confirmation) return;
		const trimmed = cloneName.trim();
		if (!trimmed) return;
		busyName = source;
		try {
			const res = await api.profileClone({ source, new_filename: trimmed });
			toastStore.success("プロファイルを複製しました", res.filename);
			cloneFor = null;
			cloneName = "";
			await load();
		} catch (e) {
			toastStore.error("複製に失敗しました", extractMessage(e));
		} finally {
			busyName = null;
		}
	}

	function startRename(entry: ProfileEntry) {
		if (entry.is_current) return;
		if (!mayLeave()) return;
		renameFor = entry.filename;
		cloneFor = null;
		renameName = entry.filename;
	}

	function cancelRename() {
		if (busyName || confirmation) return;
		if (!hasNameDraft) { renameFor = null; renameName = ""; return; }
		requestReview("discard");
	}

	async function submitRename(original: string) {
		if (busyName || confirmation) return;
		const trimmed = renameName.trim();
		if (!trimmed || trimmed === original) {
			cancelRename();
			return;
		}
		busyName = original;
		try {
			const res = await api.profileRename(original, { new_filename: trimmed });
			toastStore.success("プロファイルをリネームしました", `${original} → ${res.filename}`);
			renameFor = null;
			renameName = "";
			await load();
		} catch (e) {
			toastStore.error("リネームに失敗しました", extractMessage(e));
		} finally {
			busyName = null;
		}
	}

	async function doDelete(entry: ProfileEntry) {
		if (busyName || confirmation) return;
		if (entry.is_current) return;
		busyName = entry.filename;
		try {
			const res = await api.profileDelete(entry.filename);
			toastStore.success("プロファイルを削除しました", `バックアップ: ${res.backup ?? "(作成されませんでした)"}`);
			await load();
		} catch (e) {
			toastStore.error("削除に失敗しました", extractMessage(e));
		} finally {
			busyName = null;
		}
	}

	function openEditor(entry: ProfileEntry) {
		if (!mayLeave()) return;
		editingFilename = entry.filename;
	}
	function onEditorClosed(didSave: boolean) {
		editingFilename = null;
		if (didSave) {
			void load();
		}
	}

	async function switchTo(entry: ProfileEntry) {
		if (busyName || confirmation) return;
		if (entry.is_current) return;
		if (linkedAppDraft.value || flowgraphStore.hasUnsavedDrafts || flowgraphStore.mutating) {
			toastStore.error("再起動できません", "連携アプリまたはFlowgraphの編集中・保存中です。保存・破棄を済ませてください。");
			return;
		}
		if (!tabNavStore.canLeaveCurrent()) return;
		busyName = entry.filename;
		try {
			const res = await api.restart({ conf: entry.path, graceful_ms: 800 });
			toastStore.info("再起動します…", `new pid=${res.new_pid}`);
		} catch (e) {
			toastStore.error("再起動に失敗しました", extractMessage(e));
		} finally {
			busyName = null;
		}
	}
</script>

<section class="rounded border border-surface-200-800 bg-surface-50-950">
	<header class="flex items-center justify-between border-b border-surface-200-800 px-4 py-2">
		<div>
			<h3 class="text-sm font-semibold">プロファイル管理</h3>
			<p class="text-xs opacity-60">
				{directory ?? "読み込み中…"}
			</p>
		</div>
		<div class="flex items-center gap-2">
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-100-900"
				onclick={() => void load()}
				disabled={loading || busyName !== null || confirmation !== null || hasNameDraft}
			>
				{loading ? "読込中…" : "更新"}
			</button>
		</div>
	</header>

	{#if confirmation}
		<section aria-label="プロファイル操作の確認" class="m-4 rounded border border-warning-500 p-3 text-sm">
			<p class="font-semibold">{confirmation.kind === "delete" ? "設定ファイルを削除" : confirmation.kind === "switch" ? "設定ファイルを切り替えて再起動" : "未完了の入力を破棄"}</p>
			<p class="mt-2 break-all">対象：{confirmation.filename}</p>
			<p class="mt-1 break-all text-xs opacity-70">保存先：{confirmation.directory}</p>
			<p class="mt-2">{confirmation.kind === "delete" ? "削除前の内容はバックアップへ退避します。稼働中の設定は変わりません。" : confirmation.kind === "switch" ? "このVACを再起動して選択した設定を読み込みます。接続は一時的に切断されます。" : "複製・名前変更の入力だけを取り消します。保存済みファイルは変わりません。"}</p>
			<div class="mt-3 flex flex-wrap gap-3">
				<button bind:this={keepButton} class="rounded border px-3 py-2" onclick={() => confirmation = null}>変更せず戻る</button>
				<button class="rounded border border-warning-500 px-3 py-2" onclick={() => void confirmOperation()}>{confirmation.kind === "delete" ? "削除する" : confirmation.kind === "switch" ? "切り替えて再起動する" : "入力を破棄する"}</button>
			</div>
		</section>
	{/if}

	{#if error}
		<p class="border-b border-error-500/40 bg-error-500/10 px-4 py-2 text-xs text-error-900-100">
			{error}
		</p>
	{/if}

	{#if entries.length === 0 && !loading}
		<p class="p-4 text-xs opacity-60">プロファイル候補なし。</p>
	{:else}
		<ul class="divide-y divide-surface-200-800">
			{#each entries as entry (entry.path)}
				{@const isBusy = busyName !== null || loading || confirmation !== null}
				<li class="px-4 py-3 text-sm">
					<div class="flex flex-wrap items-start gap-2">
						<div class="min-w-0 flex-1">
							<div class="flex items-center gap-2">
								<span class="truncate font-medium">{entry.label}</span>
								{#if entry.is_current}
									<span class="rounded bg-primary-500/20 px-1.5 py-0.5 text-xs text-primary-500">current</span>
								{/if}
							</div>
							<div class="truncate text-xs opacity-60">
								{entry.filename} · {fmtBytes(entry.size)} · {fmtModified(entry.modified)}
							</div>
						</div>
						<div class="flex flex-wrap items-center gap-1">
							<button
								type="button"
								class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-100-900 disabled:opacity-50"
								onclick={() => openEditor(entry)}
								disabled={isBusy}
							>
								編集
							</button>
							<button
								type="button"
								class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-100-900 disabled:opacity-50"
								onclick={() => startClone(entry)}
								disabled={isBusy}
							>
								複製
							</button>
							<button
								type="button"
								class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-100-900 disabled:opacity-50"
								onclick={() => startRename(entry)}
								disabled={isBusy || entry.is_current}
								title={entry.is_current ? "現在読み込み中のプロファイルはリネームできません" : ""}
							>
								リネーム
							</button>
							<button
								type="button"
								class="rounded border border-error-500/40 px-2 py-0.5 text-xs text-error-500 hover:bg-error-500/10 disabled:opacity-50"
								onclick={() => requestReview("delete", entry)}
								disabled={isBusy || entry.is_current}
								title={entry.is_current ? "現在読み込み中のプロファイルは削除できません" : ""}
							>
								削除
							</button>
							<button
								type="button"
								class="rounded bg-primary-500 px-2 py-0.5 text-xs font-medium text-white hover:bg-primary-600 disabled:opacity-50"
								onclick={() => requestReview("switch", entry)}
								disabled={isBusy || entry.is_current}
							>
								切替起動
							</button>
						</div>
					</div>

					{#if cloneFor === entry.filename}
						<div class="mt-2 flex flex-wrap items-center gap-2 rounded border border-surface-300-700 bg-surface-100-900 p-2">
							<span class="text-xs opacity-70">複製先ファイル名:</span>
							<input
								type="text"
								bind:value={cloneName}
								disabled={isBusy}
								class="flex-1 min-w-0 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs"
								placeholder="conf.backup.toml"
							/>
							<button
								type="button"
								class="rounded bg-primary-500 px-2 py-0.5 text-xs font-medium text-white hover:bg-primary-600 disabled:opacity-50"
								onclick={() => void submitClone(entry.filename)}
								disabled={isBusy || !cloneName.trim()}
							>
								実行
							</button>
							<button
								type="button"
								class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-50-950"
								onclick={cancelClone}
								disabled={isBusy}
							>
								取消
							</button>
						</div>
					{/if}

					{#if renameFor === entry.filename}
						<div class="mt-2 flex flex-wrap items-center gap-2 rounded border border-surface-300-700 bg-surface-100-900 p-2">
							<span class="text-xs opacity-70">新しいファイル名:</span>
							<input
								type="text"
								bind:value={renameName}
								disabled={isBusy}
								class="flex-1 min-w-0 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs"
							/>
							<button
								type="button"
								class="rounded bg-primary-500 px-2 py-0.5 text-xs font-medium text-white hover:bg-primary-600 disabled:opacity-50"
								onclick={() => void submitRename(entry.filename)}
								disabled={isBusy || !renameName.trim()}
							>
								実行
							</button>
							<button
								type="button"
								class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-50-950"
								onclick={cancelRename}
								disabled={isBusy}
							>
								取消
							</button>
						</div>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}

	<ProfileBackupsPanel onRestored={() => void load()} />
	{#if currentPath}
		<footer class="border-t border-surface-200-800 px-4 py-2 text-xs opacity-60">
			current: {currentPath}
		</footer>
	{/if}
</section>

{#if editingFilename}
	<ProfileEditorModal filename={editingFilename} onClose={onEditorClosed} />
{/if}

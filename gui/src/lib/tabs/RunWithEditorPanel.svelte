<script lang="ts">
	/**
	 * `run_with` 編集パネル（Managed App の追加・編集・削除）。
	 *
	 * - `api.runWithList()` で現状を取得。
	 * - 新規追加フォーム: command / if_not_running / id / label / minimized / run_as_admin / working_dir。
	 * - 各行の「削除」で `api.runWithDelete(index)`。conf.toml の該当エントリが除去される。
	 * - 書き込みはサーバ側で妥当性検証 + バックアップされる（安全）。
	 * - 起動済みプロセスへの影響はなし（= 削除しても走っているプロセスは止まらない）。
	 * - run_with 形式が `[[run_with]]` テーブル形式で書かれている場合は API が 501 を返すので、
	 *   UI はそのメッセージをそのまま表示する。
	 */
	import { api } from "../api";
	import { ControlApiError } from "../types";
	import type { RunWithDto, RunWithTableDto, RunWithView } from "../types";
	import { toastStore } from "../toasts.svelte";
	import { onMount, onDestroy, tick, untrack } from "svelte";
	import { linkedAppDraft, batchFromDraft, additionsFromDraft, formsFromDraft } from "../linkedAppDraft.svelte";
	import { tabNavStore } from "../tabs.svelte";

	let loading = $state(true);
	let listRequest = 0;
	let sourceChanged = $state(false);
	let error = $state<string | null>(null);
	let entries = $state<RunWithView[]>([]);
	let revision = $state<string | null>(null);
	let sourcePath = $state<string | null>(null);
	let submitting = $state(false);
	let busyIndex = $state<number | null>(null);
	let editingIndex = $state<number | null>(null);
	let editingRevision = $state<string | null>(null);
	let formModeActions = $state<Record<string, string>>({});
	let availableModes = $state<{ id: string; label: string; action: string }[]>([]);
	const formModes = $derived(entries.find(e => e.index === editingIndex)?.modes ?? availableModes);
	let preservedShutdown = $state<RunWithTableDto["shutdown"]>({ action: "close_only" });

	// 新規追加フォーム
	let formCommand = $state("");
	let formIfNotRunning = $state("");
	let formId = $state("");
	let formLabel = $state("");
	let formUnaaZAppId = $state("");
	let formUnaaZInstanceId = $state("");
	let formWorkingDir = $state("");
	let formMinimized = $state(false);
	let formRunAsAdmin = $state(false);
	let formStartOnLaunch = $state(false);
	function formState() { return JSON.stringify([formCommand, formIfNotRunning, formId, formLabel, formUnaaZAppId, formUnaaZInstanceId, formWorkingDir, formMinimized, formRunAsAdmin, formStartOnLaunch, formModeActions, preservedShutdown]); }
	let baseline = $state(JSON.stringify(["", "", "", "", "", "", "", false, false, false, {}, { action: "close_only" }]));
	const dirty = $derived(formState() !== baseline);
 const sharedDirty = $derived(Object.keys(linkedAppDraft.value?.otherForms ?? {}).length > 0 || Object.keys(linkedAppDraft.value?.modeEdits ?? {}).length > 0);
	let review = $state<{ kind: "discard" | "delete" | "save"; targetIndex?: number | null; editRevision?: string | null; draft: string; revision: string | null; path: string | null; entry?: RunWithView; signature?: string; sharedSignature?: string } | null>(null);
	function canDiscard() {
		if (submitting || busyIndex !== null || review) return false;
		if (!dirty && !Object.keys(linkedAppDraft.value?.otherForms ?? {}).length && !Object.keys(linkedAppDraft.value?.modeEdits ?? {}).length) return true;
		error = "未保存の編集があります。保存するか、フォームのクリア操作で破棄を確認してください。";
		return false;
	}
	function requestReset() {
		if (submitting || busyIndex !== null || review) return;
		if (!dirty && !sharedDirty) { resetForm(); return; }
		review = { kind: "discard", draft: formState(), revision, path: sourcePath };
	}
	function requestDelete(entry: RunWithView) {
		if (loading || !canDiscard()) return;
		review = { kind: "delete", draft: formState(), revision, path: sourcePath, entry, signature: JSON.stringify(entry) };
	}
	function requestSave() {
		if (loading || sourceChanged || submitting || busyIndex !== null || review || (!formCommand.trim() && (dirty || !sharedDirty))) return;
  retainDraft();
		review = {kind: "save", draft: formState(), revision, path: sourcePath, targetIndex: editingIndex, editRevision: editingRevision, sharedSignature: JSON.stringify(linkedAppDraft.value)};
		void tick().then(() => document.getElementById("run-with-review")?.focus());
	}
	function describeChange(index: number, value: unknown): string {
		if (typeof value === "boolean") return value ? "はい" : "いいえ";
		if (index === 10) return Object.entries((value ?? {}) as Record<string,string>).map(([id, action]) => `${formModes.find(m => m.id === id)?.label ?? id}: ${{inherit:"指定なし",start:"起動",stop:"終了要求",minimize:"最小化",leave:"状態を維持"}[action] ?? action}`).join(" / ") || "指定なし";
		if (index === 11) return Object.entries((value ?? {}) as Record<string,unknown>).map(([key,v]) => `${({scope:"終了する範囲",action:"個別の終了要求",on_vac_exit:"VAC終了時",method:"通知方法",grace_ms:"待機時間(ms)",app_specific:"アプリ固有処理"} as Record<string,string>)[key] ?? key}: ${({process_tree:"子プロセスも含む",matched_processes:"登録アプリだけ",close_only:"終了要求・強制終了なし",close_and_wait:"待機後に強制終了",leave_running:"残す"} as Record<string,string>)[String(v)] ?? v ?? "既定"}`).join(" / ") || "既定";
		return String(value || "未指定");
	}
	function saveChanges() {
		const labels=["起動コマンド","確認するプロセス名","連携ID","表示名","連携先アプリID","連携先個体ID","作業フォルダー","最小化で起動","管理者権限","VAC起動時に開始","モード別の操作","終了方針"];
		const before=JSON.parse(baseline),after=JSON.parse(formState());
		return labels.flatMap((label,index) => JSON.stringify(before[index]) === JSON.stringify(after[index]) ? [] : [{label,before:describeChange(index,before[index]),after:describeChange(index,after[index])}]);
	}

	async function confirmReview() {
		const pending = review;
		if (!pending || submitting || busyIndex !== null || loading) return;
		if (pending.kind === "save" && sourceChanged) { review = null; return; }
		if (pending.draft !== formState() || pending.revision !== revision || pending.path !== sourcePath) {
			review = null; error = "確認後に編集内容または保存先が変わりました。内容を確認し直してください。"; return;
		}
		if (pending.kind === "save") {
			if (pending.sharedSignature !== JSON.stringify(linkedAppDraft.value)) { review=null; error="確認後に共有下書きが変わりました。内容を確認し直してください。"; return; }
			if (pending.targetIndex !== editingIndex || pending.editRevision !== editingRevision) { review=null; error="編集対象が変わりました。保存内容を確認し直してください。"; return; }
			review=null; await addEntry(); return;
		}
		if (pending.kind === "discard") { review = null; linkedAppDraft.value = null; resetForm(); error = null; return; }
		const entry = entries.find(item => item.index === pending.entry?.index);
		if (!entry || JSON.stringify(entry) !== pending.signature) { review = null; error = "削除対象が変わりました。確認し直してください。"; return; }
		review = null;
		await deleteEntry(entry);
		// Deletion may renumber entries; leave no clean editor pointing at its old index.
		if (revision !== pending.revision && !dirty) resetForm();
	}

	function retainDraft() {
		const modeEdits = untrack(() => linkedAppDraft.value?.modeEdits);
		const otherForms = untrack(() => linkedAppDraft.value?.otherForms);
		if (dirty || Object.keys(modeEdits ?? {}).length || Object.keys(otherForms ?? {}).length) linkedAppDraft.value = { otherForms, modeEdits, form: formState(), baseline, editingIndex, editingRevision, revision, sourcePath, entries: $state.snapshot(entries), modes: $state.snapshot(availableModes) };
		else linkedAppDraft.value = null;
	}
	function restoreDraft() {
	const held = linkedAppDraft.value;
	if (held) {
		[formCommand, formIfNotRunning, formId, formLabel, formUnaaZAppId, formUnaaZInstanceId, formWorkingDir, formMinimized, formRunAsAdmin, formStartOnLaunch, formModeActions, preservedShutdown] = JSON.parse(held.form);
		baseline = held.baseline; editingIndex = held.editingIndex; editingRevision = held.editingRevision;
		revision = held.revision; sourcePath = held.sourcePath; entries = held.entries; availableModes = held.modes;
	}
	}
	restoreDraft();
	$effect(() => { retainDraft(); });
	onMount(() => tabNavStore.guardLeaving(() => {
		if (submitting || busyIndex !== null || review) return false;
		retainDraft();
		return true;
	}));
	onDestroy(() => { ++listRequest; });

	async function load() {
		if (submitting || busyIndex !== null || review) return;
		const request = ++listRequest;
		loading = true;
		error = null;
		try {
			const res = await api.runWithList();
			if (request !== listRequest) return;
			sourceChanged = (dirty || (Object.keys(linkedAppDraft.value?.modeEdits ?? {}).length + Object.keys(linkedAppDraft.value?.otherForms ?? {}).length) > 0) && sourcePath !== res.source_path;
			if (dirty || (Object.keys(linkedAppDraft.value?.modeEdits ?? {}).length + Object.keys(linkedAppDraft.value?.otherForms ?? {}).length) > 0) {
				if (sourcePath !== res.source_path || revision !== (res.revision ?? null)) {
					error = "保存先または保存内容が変わりました。下書きは保持しています。必要な内容を控え、クリアで破棄してから最新の内容を読み直してください。";
				}
				return;
			}
			entries = res.entries;
			availableModes = res.modes ?? res.entries[0]?.modes ?? [];
			revision = res.revision ?? null;
			sourcePath = res.source_path;
		} catch (e) {
			if (request === listRequest) error = extractMessage(e);
		} finally {
			if (request === listRequest) loading = false;
		}
	}

	onMount(() => {
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

	function resetForm() {
		formCommand = "";
		formIfNotRunning = "";
		formId = "";
		formLabel = "";
		formUnaaZAppId = "";
		formUnaaZInstanceId = "";
		formWorkingDir = "";
		formMinimized = false;
		formRunAsAdmin = false;
		formStartOnLaunch = false;
		preservedShutdown = { action: "close_only" };
		formModeActions = {};
		editingIndex = null;
		editingRevision = null;
		baseline = formState();
	}

	function buildDto(): RunWithDto | null {
		const cmd = formCommand.trim();
		if (!cmd) return null;
		// 何も追加オプションがない場合は純粋な文字列形式にできるが、
		// Managed App として扱うためには `if_not_running` を推奨するため、常に table 形式で送る。
		const table: RunWithTableDto = { command: cmd };
		if (formIfNotRunning.trim()) table.if_not_running = formIfNotRunning.trim();
		if (formId.trim()) table.id = formId.trim();
		if (formLabel.trim()) table.label = formLabel.trim();
		if (formUnaaZAppId.trim()) table.unaa_z_app_id = formUnaaZAppId.trim();
		if (formUnaaZInstanceId.trim()) table.unaa_z_instance_id = formUnaaZInstanceId.trim();
		if (formWorkingDir.trim()) table.working_dir = formWorkingDir.trim();
		if (formMinimized) table.minimized = true;
		if (formRunAsAdmin) table.run_as_admin = true;
		if (!formStartOnLaunch) table.start_on_launch = false;
		if (preservedShutdown) table.shutdown = preservedShutdown;
		table.mode_actions = formModeActions;
		return table;
	}

	function stashActive() {
		retainDraft();
		if (linkedAppDraft.value && dirty) linkedAppDraft.value={...linkedAppDraft.value,otherForms:formsFromDraft(linkedAppDraft.value)};
	}
	function beginNew() {
		if (submitting || busyIndex !== null || review || loading) return;
		stashActive(); resetForm();
		const held=linkedAppDraft.value?.otherForms?.new;
		if(held) restoreForm(held.form,held.baseline);
		if(linkedAppDraft.value){const otherForms={...linkedAppDraft.value.otherForms};delete otherForms.new;linkedAppDraft.value={...linkedAppDraft.value,otherForms};}
	}
	function restoreForm(form:string,original:string) {
		[formCommand,formIfNotRunning,formId,formLabel,formUnaaZAppId,formUnaaZInstanceId,formWorkingDir,formMinimized,formRunAsAdmin,formStartOnLaunch,formModeActions,preservedShutdown]=JSON.parse(form);
		baseline=original;
	}
	function beginEdit(entry: RunWithView) {
		if (submitting || busyIndex !== null || review || loading) return;
		stashActive();
		const value: RunWithTableDto = isTableEntry(entry.entry) ? entry.entry : { command: entry.entry };
		editingIndex = entry.index;
		editingRevision = revision;
		formCommand = value.command;
		formIfNotRunning = value.if_not_running ?? "";
		formId = value.id ?? "";
		formLabel = value.label ?? "";
		formUnaaZAppId = value.unaa_z_app_id ?? "";
		formUnaaZInstanceId = value.unaa_z_instance_id ?? "";
		formWorkingDir = value.working_dir ?? "";
		formMinimized = value.minimized ?? false;
		formRunAsAdmin = value.run_as_admin ?? false;
		formStartOnLaunch = value.start_on_launch ?? true;
		preservedShutdown = value.shutdown ?? null;
		formModeActions = Object.fromEntries((entry.modes ?? []).map(m => [m.id, m.action]));
		baseline = formState();
		const heldForm=linkedAppDraft.value?.otherForms?.[entry.index];
		if(heldForm) restoreForm(heldForm.form,heldForm.baseline);
		if(linkedAppDraft.value){const otherForms={...linkedAppDraft.value.otherForms};delete otherForms[entry.index];linkedAppDraft.value={...linkedAppDraft.value,otherForms};}
		const heldModes = linkedAppDraft.value?.modeEdits?.[entry.index];
		if (heldModes) {
			formModeActions = {...formModeActions, ...heldModes};
			const modeEdits = {...linkedAppDraft.value?.modeEdits}; delete modeEdits[entry.index];
			if (linkedAppDraft.value) linkedAppDraft.value = {...linkedAppDraft.value, modeEdits};
		}
	}

	async function addEntry() {
		if (submitting || busyIndex !== null || review) return;
		const dto = buildDto();
		if (!dto && (dirty || !sharedDirty)) {
			toastStore.warn("起動コマンドを入力してください");
			return;
		}
		submitting = true;
		++listRequest;
		loading = false;
		error = null;
		try {
			retainDraft();
			const shared = linkedAppDraft.value;
			const hasOtherModes = Object.keys(shared?.modeEdits ?? {}).length > 0 || Object.keys(shared?.otherForms ?? {}).length > 0;
			const res = hasOtherModes && shared ? await api.runWithBatch(batchFromDraft(shared), revision!, additionsFromDraft(shared)) : editingIndex === null ? await api.runWithAdd(dto!, revision) : await api.runWithUpdate(editingIndex, dto!, editingRevision);
			linkedAppDraft.value = null;
			entries = res.entries;
			availableModes = res.modes ?? res.entries[0]?.modes ?? [];
			revision = res.revision ?? null;
			sourcePath = res.source_path;
			toastStore.success(
				hasOtherModes ? "連携アプリの変更をまとめて保存しました" : editingIndex === null ? "連携アプリを追加しました" : "連携アプリを更新しました",
				res.backup ? `バックアップ: ${res.backup}` : undefined
			);
			resetForm();
		} catch (e) {
			toastStore.error(editingIndex === null ? "追加に失敗しました" : "更新に失敗しました", extractMessage(e));
		} finally {
			submitting = false;
		}
	}

	async function deleteEntry(entry: RunWithView) {
		if (submitting || busyIndex !== null || review) return;

		busyIndex = entry.index;
		++listRequest;
		loading = false;
		error = null;
		try {
			const res = await api.runWithDelete(entry.index, revision);
			entries = res.entries;
			availableModes = res.modes ?? res.entries[0]?.modes ?? [];
			revision = res.revision ?? null;
			sourcePath = res.source_path;
			toastStore.success("連携アプリを削除しました", res.backup ? `バックアップ: ${res.backup}` : undefined);
		} catch (e) {
			toastStore.error("削除に失敗しました", extractMessage(e));
		} finally {
			busyIndex = null;
		}
	}

	function isTableEntry(dto: RunWithDto): dto is RunWithTableDto {
		return typeof dto !== "string";
	}

	function entrySummary(view: RunWithView): string {
		if (isTableEntry(view.entry)) {
			return view.entry.command;
		}
		return view.entry;
	}
</script>

<svelte:window onbeforeunload={(event) => { if (dirty || sharedDirty || submitting || busyIndex !== null) { event.preventDefault(); event.returnValue = ""; } }} />

<section class="rounded border border-surface-200-800 bg-surface-50-950">
 <button class="m-4 rounded border px-3 py-2 text-sm" disabled={loading||submitting||busyIndex!==null||!!review} onclick={beginNew}>新規登録を編集</button>
	<header class="flex items-center justify-between border-b border-surface-200-800 px-4 py-2">
		<div>
			<h3 class="text-sm font-semibold">連携アプリ設定（run_with）</h3>
			<p class="text-xs opacity-60">設定を保存し、このVACの連携一覧へ反映します。保存だけではアプリを起動・停止しません。起動などの操作は上部の「連携アプリ」から行えます。</p>
		</div>
		<button
			type="button"
			class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-100-900"
			onclick={() => void load()}
			disabled={loading || submitting || busyIndex !== null || review !== null}
		>
			{loading ? "読込中…" : "更新"}
		</button>
	</header>

	{#if review}
		<section id="run-with-review" tabindex="-1" aria-label="連携設定変更の確認" class="m-4 rounded border border-warning-500/50 bg-warning-500/10 p-3 text-sm">
			<p class="font-semibold">{review.kind === "save" ? "変更内容を確認して保存" : review.kind === "discard" ? "未保存の編集をすべて破棄しますか？" : `${review.entry?.display_label} を連携設定から削除しますか？`}</p>
			<p class="break-all text-xs">保存先: {review.path ?? "未取得"}</p>
			{#if review.kind === "save"}
				<p class="mt-2 text-sm">対象: {editingIndex === null ? "新規登録" : entries.find(e => e.index === editingIndex)?.display_label ?? `#${editingIndex}`}</p>
				<dl class="mt-3 grid gap-3">{#each saveChanges() as change}<div class="min-w-0 break-words"><dt class="font-semibold">{change.label}</dt><dd class="text-xs opacity-70">保存前: {change.before}</dd><dd class="text-sm">保存後: {change.after}</dd></div>{/each}</dl>
				{#each Object.entries(linkedAppDraft.value?.modeEdits ?? {}) as [index, actions]}
					<p class="mt-2 text-xs">{entries.find(e=>e.index===Number(index))?.display_label}: {Object.entries(actions).map(([id, action])=>`${availableModes.find(m=>m.id===id)?.label ?? id}: ${action}`).join(" / ")}</p>
				{/each}
				{#each Object.entries(linkedAppDraft.value?.otherForms ?? {}) as [key, held]}
					<div class="mt-3 text-sm"><strong>{key==='new'?'新規登録':entries.find(e=>e.index===Number(key))?.display_label}</strong>
					{#each JSON.parse(held.form) as value, index}{#if JSON.stringify(value)!==JSON.stringify(JSON.parse(held.baseline)[index])}<p class="break-all">{["起動コマンド","確認するプロセス名","連携ID","表示名","連携先アプリID","連携先個体ID","作業フォルダー","最小化で起動","管理者権限","VAC起動時に開始","モード別の操作","終了方針"][index]}: {describeChange(index,JSON.parse(held.baseline)[index])} → {describeChange(index,value)}</p>{/if}{/each}
					</div>
				{/each}
				<p class="mt-3 text-xs">保存だけではアプリの起動・終了やモード切替を実行しません。</p>
			{/if}
			{#if review.kind === "delete"}<p class="text-xs">起動済みのアプリは停止しません。この登録に対するモードの操作指定も解除します。</p>
				<ul class="mt-2 text-xs">{#each review.entry?.modes?.filter(m => m.action !== "inherit") ?? [] as mode}<li>{mode.label}: {mode.action}</li>{/each}</ul>
				<p class="mt-2 text-xs opacity-70">Flowgraph内で直接指定したIDなど、モード以外の参照は別途確認してください。</p>{/if}
			<div class="mt-2 flex gap-2">
				<button class="rounded border px-2 py-1" onclick={() => void confirmReview()} disabled={submitting || busyIndex !== null}>{review.kind === "save" ? "確認して保存" : review.kind === "discard" ? "編集を破棄" : "削除を実行"}</button>
				<button class="rounded border px-2 py-1" onclick={() => review = null}>キャンセル</button>
			</div>
		</section>
	{/if}

	{#if error}
		<p class="border-b border-error-500/40 bg-error-500/10 px-4 py-2 text-xs text-error-900-100">{error}</p>
	{/if}

	<div class="divide-y divide-surface-200-800">
		{#if entries.length === 0 && !loading && !error}
			<p class="px-4 py-3 text-xs opacity-60">登録済みの連携アプリはありません。下のフォームから追加してください。</p>
		{:else}
			{#each entries as entry (entry.index)}
				<div class="px-4 py-3 text-sm">
					<div class="flex flex-wrap items-start gap-2">
						<div class="min-w-0 flex-1">
							<div class="flex items-center gap-2">
								<span class="truncate font-medium">{entry.display_label}</span>
								<span class="rounded bg-surface-200-800 px-1.5 py-0 text-[10px] opacity-70">#{entry.index}</span>
								{#if entry.supports_status}
									<span class="rounded bg-primary-500/20 px-1.5 py-0.5 text-xs text-primary-500">状態を確認可能</span>
								{:else}
									<span
										class="rounded bg-warning-500/20 px-1.5 py-0.5 text-xs text-warning-500"
										title="確認するプロセス名が未指定のため、起動状態を確認できません"
									>
										起動状態は未確認
									</span>
								{/if}
							</div>
							<p class="mt-0.5 truncate font-mono text-[11px] opacity-60" title={entrySummary(entry)}>
								{entrySummary(entry)}
							</p>
							<div class="mt-1 flex flex-wrap gap-1 text-[10px] opacity-70">
								<code>id={entry.effective_id}</code>
								{#if isTableEntry(entry.entry)}
									{#if entry.entry.if_not_running}
										<code>if_not_running={entry.entry.if_not_running}</code>
									{/if}
									{#if entry.entry.minimized}
										<code>minimized</code>
									{/if}
									<code>{entry.entry.start_on_launch === false ? "起動時は開始しない" : "起動時に開始"}</code>
									{#if entry.entry.run_as_admin}
										<code class="text-warning-500">admin</code>
									{/if}
									{#if entry.entry.working_dir}
										<code title={entry.entry.working_dir}>working_dir=…</code>
									{/if}
									{#if entry.entry.unaa_z_app_id}
										<code>unaa_z_app_id={entry.entry.unaa_z_app_id}</code>
									{/if}
									{#if entry.entry.unaa_z_instance_id}
										<code>unaa_z_instance_id={entry.entry.unaa_z_instance_id}</code>
									{/if}
								{/if}
							</div>
						</div>
						<button
							type="button"
							class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-100-900 disabled:opacity-50"
							onclick={() => beginEdit(entry)}
							disabled={busyIndex !== null || submitting || review !== null}
						>
							編集
						</button>
						<button
							type="button"
							class="rounded border border-error-500/40 px-2 py-0.5 text-xs text-error-500 hover:bg-error-500/10 disabled:opacity-50"
							onclick={() => requestDelete(entry)}
							disabled={busyIndex !== null || submitting || review !== null}
						>
							削除
						</button>
					</div>
				</div>
			{/each}
		{/if}
	</div>

	<div class="border-t border-surface-200-800 bg-surface-100-900 p-4">
		<h4 class="mb-2 text-xs font-semibold opacity-80">{editingIndex === null ? "新規追加" : `#${editingIndex} を編集`}</h4>
		<fieldset class="mb-4 grid gap-3 rounded border border-surface-300-700 p-3">
			<legend class="text-sm font-semibold">終了の扱い</legend>
			<label class="grid gap-1 text-sm">VAC終了時
				<select class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1" value={preservedShutdown?.on_vac_exit ?? "inherit"} disabled={submitting || review !== null} onchange={e => preservedShutdown = {...preservedShutdown, on_vac_exit: e.currentTarget.value === "inherit" ? null : e.currentTarget.value as NonNullable<RunWithTableDto["shutdown"]>["on_vac_exit"]}}>
					<option value="inherit">個別の終了要求と同じ（従来の設定）</option><option value="leave_running">アプリを残す</option><option value="close_only">終了を要求・強制終了しない</option><option value="close_and_wait">終了を要求・待機後に強制終了</option>
				</select>
			</label>
			<label class="grid gap-1 text-sm">手動・モード切替で終了を要求した時
				<select class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1" value={preservedShutdown?.action ?? "inherit"} disabled={submitting || review !== null} onchange={e => preservedShutdown = {...preservedShutdown, action: e.currentTarget.value === "inherit" ? null : e.currentTarget.value as NonNullable<RunWithTableDto["shutdown"]>["action"]}}>
					<option value="inherit">既定（待機後に強制終了）</option><option value="close_only">終了を要求・強制終了しない</option><option value="close_and_wait">終了を要求・待機後に強制終了</option><option value="leave_running">終了要求を行わない（既存互換）</option>
				</select>
			</label>
            <label class="grid gap-1 text-sm">終了する範囲
                <select class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1" aria-label="終了する範囲" value={preservedShutdown?.scope ?? "inherit"} disabled={submitting || review !== null} onchange={e => preservedShutdown = {...preservedShutdown, scope: e.currentTarget.value === "inherit" ? null : e.currentTarget.value as NonNullable<RunWithTableDto["shutdown"]>["scope"]}}>
                    <option value="inherit">従来どおり（子プロセスも含む）</option>
                    <option value="matched_processes">登録アプリだけ（子の終了はアプリに任せる）</option>
                    <option value="process_tree">子プロセスも含めて終了する</option>
                </select>
                <span class="text-xs opacity-70">UNA・UNMの終了時設定を尊重する場合は「登録アプリだけ」を選びます。待機後の強制終了を選んだ場合も、この範囲が対象です。</span>
            </label>
			<p class="text-xs opacity-70">VAC終了時の指定は、個別操作の終了方針を変更しません。待機時間・通知方法・アプリ固有の終了処理は既存値を保持します。</p>
		</fieldset>

		{#if formModes.length}
			<fieldset class="mb-4 grid gap-2 rounded border border-surface-300-700 p-3">
				<legend class="text-sm font-semibold">モードへ切り替えた時の操作</legend>
				{#each formModes as mode (mode.id)}
					<label class="flex flex-wrap items-center justify-between gap-2 text-sm"><span class="min-w-0 break-words">{mode.label}</span>
						<select class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1" value={formModeActions[mode.id] ?? "inherit"} onchange={e => formModeActions = {...formModeActions, [mode.id]: e.currentTarget.value}} disabled={submitting || review !== null}>
							<option value="inherit">指定しない（既定に従う）</option><option value="start">起動する</option><option value="stop">終了を要求する</option><option value="minimize">最小化する</option><option value="leave">状態を維持する</option>
						</select>
					</label>
				{/each}
				<p class="text-xs opacity-70">アプリ登録と一緒に保存。保存だけでは切替を実行せず、次にこのモードへ切り替えた時に使用します。</p>
			</fieldset>
		{/if}

		<div class="grid grid-cols-1 gap-2 md:grid-cols-2">
			<label class="block text-xs">
				<span class="opacity-70">起動コマンド（必須）</span>
				<input
					type="text"
					bind:value={formCommand}
					class="mt-0.5 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs font-mono"
					placeholder="C:\path\to\app.exe --args"
					disabled={submitting || review !== null}
				/>
			</label>
			<label class="block text-xs">
				<span class="opacity-70">起動中の確認に使うプロセス名（任意）</span>
				<input
					type="text"
					bind:value={formIfNotRunning}
					class="mt-0.5 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs font-mono"
					placeholder="app.exe"
					aria-describedby="run-with-process-help"
					disabled={submitting || review !== null}
				/>
			</label>
			<label class="block text-xs">
				<span class="opacity-70">連携設定のID（任意）</span>
				<input
					type="text"
					bind:value={formId}
					class="mt-0.5 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs"
					placeholder="my-app"
					aria-describedby="run-with-id-help"
					disabled={submitting || review !== null}
				/>
			</label>
			<label class="block text-xs">
				<span class="opacity-70">表示名（任意）</span>
				<input
					type="text"
					bind:value={formLabel}
					class="mt-0.5 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs"
					disabled={submitting || review !== null}
				/>
			</label>
			<label class="block text-xs">
				<span class="opacity-70">連携先のアプリID（UNAA/Z・任意）</span>
				<input
					type="text"
					bind:value={formUnaaZAppId}
					class="mt-0.5 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs font-mono"
					placeholder="un-avatar"
					disabled={submitting || review !== null}
				/>
			</label>
			<label class="block text-xs">
				<span class="opacity-70">連携先の個体ID（UNAA/Z・任意）</span>
				<input
					type="text"
					bind:value={formUnaaZInstanceId}
					class="mt-0.5 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs font-mono"
					placeholder="main"
					disabled={submitting || review !== null}
				/>
			</label>
			<label class="col-span-full block text-xs">
				<span class="opacity-70">起動時の作業フォルダー（任意）</span>
				<input
					type="text"
					bind:value={formWorkingDir}
					class="mt-0.5 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs font-mono"
					disabled={submitting || review !== null}
				/>
			</label>
			<label class="flex items-center gap-2 text-xs">
				<input type="checkbox" bind:checked={formMinimized} disabled={submitting || review !== null} />
				<span>最小化で起動 (minimized)</span>
			</label>
			<label class="flex items-center gap-2 text-xs">
				<input type="checkbox" bind:checked={formRunAsAdmin} disabled={submitting || review !== null} />
				<span>管理者権限で起動 (run_as_admin)</span>
			</label>
			<label class="flex items-center gap-2 text-xs">
				<input type="checkbox" bind:checked={formStartOnLaunch} disabled={submitting || review !== null} />
				<span>UNVAC起動時に開始</span>
			</label>
		</div>
		<div class="mt-2 space-y-1 text-xs opacity-65">
			<p id="run-with-process-help">プロセス名は部分一致で確認します。一致するアプリが起動中なら、重ねて起動しません。同じアプリを複数起動するときは、この名前だけでは個体を区別できません。</p>
			<p id="run-with-id-help">連携設定のIDはFlowgraphやAPIから操作するための名前です。省略すると並び順から決まります。UNAA/ZのアプリID・個体IDは、連携先が公開している識別情報に合わせます。</p>
		</div>
		<div class="mt-3 flex items-center justify-end gap-2">
			<button
				type="button"
				class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-50-950"
			onclick={requestReset}
				disabled={submitting || review !== null}
			>
				クリア
			</button>
			<button
				type="button"
				class="rounded bg-primary-500 px-3 py-1 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
				onclick={requestSave}
				disabled={submitting || review !== null || (!formCommand.trim() && (dirty || !sharedDirty))}
			>
				{submitting ? "保存中…" : sharedDirty ? "まとめて保存" : editingIndex === null ? "追加" : "保存"}
			</button>
		</div>
	</div>

	{#if sourcePath}
		<footer class="border-t border-surface-200-800 px-4 py-2 text-xs opacity-60">
			書き込み先: {sourcePath}
		</footer>
	{/if}
</section>

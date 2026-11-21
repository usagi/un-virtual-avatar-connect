<script lang="ts">
	/**
	 * Phase φ-3b / GRN: Glossary Editor Pane のルート。
	 *
	 * - `/api/v1/control/tables` を初回に取得し、`role == "glossary"` なエントリのみ tab 化。
	 * - タブ切替で `/api/v1/control/table/{key}` を読み直して `DictionaryTable.svelte` に流し込む。
	 * - 追加 / 編集 / 削除のクリックは φ-3c の `DictionaryEntryForm.svelte` / φ-3d の
	 *   `DictionaryConflictDialog.svelte` で拾う。φ-3b 時点では placeholder として toast を出す。
	 * - ControlEvent.reloaded でファイル変更が他クライアントから通知されたら自動再読込する予定
	 *   （実装は φ-3e の LiveTab 組み込み時に WebSocket と接続）。
	 */

	import { onMount, tick } from "svelte";
	import { api } from "../../api";
	import { ControlApiError } from "../../types";
	import { dictionaryEditorStore, extractControlApiMessage } from "./dictionaryEditorStore.svelte";
	import { toastStore } from "../../toasts.svelte";
	import DictionaryTable from "./DictionaryTable.svelte";
	import DictionaryEntryForm, { type EntryFormMode, type EntryFormConflictContext } from "./DictionaryEntryForm.svelte";
	import DictionaryConflictDialog from "./DictionaryConflictDialog.svelte";
	import type { TableEntryDto } from "../../types";
	import { findConflictRow } from "./conflictRow";

	onMount(() => {
		void dictionaryEditorStore.loadCatalog();
	});

	// φ-3c: 新規/編集ダイアログの open 状態と mode。
	let formOpen = $state(false);
	let formMode = $state<EntryFormMode>({ kind: "new" });

	// φ-3d: 衝突解決ダイアログの状態。edit 用のみ表示する。
	let conflictOpen = $state(false);
	let conflictBaseRow = $state<TableEntryDto | null>(null);
	let conflictMyValues = $state<Record<string, unknown>>({});
	let conflictServerRow = $state<TableEntryDto | null>(null);

	let deleteReview = $state<{key:string;path:string;hash:string;index:number;label:string}|null>(null);
	let deleting = $state(false);
	let deleteError = $state('');
	let keepRowButton = $state<HTMLButtonElement>();
	$effect(()=>{if(deleteReview)void tick().then(()=>keepRowButton?.focus());});
	async function selectTab(key: string): Promise<void> {
		if (deleting || deleteReview || dictionaryEditorStore.currentKey === key) return;
		await dictionaryEditorStore.selectTable(key);
	}

	async function onReload(): Promise<void> {
		if (deleting || deleteReview) return;
		await dictionaryEditorStore.reloadCurrent();
		toastStore.info("再読込しました", dictionaryEditorStore.currentKey ?? undefined);
	}

	function onAdd(): void {
		if (deleting || deleteReview) return;
		formMode = { kind: "new" };
		formOpen = true;
	}

	function onEdit(row: TableEntryDto): void {
		if (deleting || deleteReview) return;
		formMode = { kind: "edit", row };
		formOpen = true;
	}

	function onDelete(row: TableEntryDto): void {
		if (deleting || deleteReview || formOpen || conflictOpen || dictionaryEditorStore.phase.kind!=="ready") return;
		const key=dictionaryEditorStore.currentKey, table=dictionaryEditorStore.currentTable;
		if (!key || !table?.content_hash) return;
		deleteError='';
		deleteReview={key,path:table.path,hash:table.content_hash,index:row.row_index,label:typeof row.values.surface==='string'?row.values.surface:`行 ${row.row_index}`};
	}
	async function confirmDelete(): Promise<void> {
		if (deleting || !deleteReview) return;
		const target=deleteReview, table=dictionaryEditorStore.currentTable;
		if (dictionaryEditorStore.phase.kind!=="ready" || dictionaryEditorStore.currentKey!==target.key || table?.path!==target.path || table?.content_hash!==target.hash) {
			deleteReview=null;deleteError="確認中に辞書や内容が変わりました。対象行を確認し直してください。";return;
		}
		deleting=true;deleteError='';
		try {
			await api.deleteControlTableEntry(target.key,target.index,target.hash);
			deleteReview=null;
			toastStore.success("辞書行を削除しました",target.label);
			if (dictionaryEditorStore.currentKey===target.key) await dictionaryEditorStore.reloadCurrent();
		} catch(e) {
			deleteReview=null;
			if (e instanceof ControlApiError && e.status===409) {
				deleteError="他のクライアントが先に更新しました。再読込後の行を確認してからやり直してください。";
				if (dictionaryEditorStore.currentKey===target.key) await dictionaryEditorStore.reloadCurrent();
			} else if (e instanceof ControlApiError && e.status===403) {
				deleteError="ロックされている行は削除できません。";
			} else { deleteError=`削除に失敗しました：${extractControlApiMessage(e)}`; }
		} finally { deleting=false; }
	}

	async function onSaved(): Promise<void> {
		// mutation response だけでは Table 全体を再構成しづらいので単純に再読込。
		await dictionaryEditorStore.reloadCurrent();
	}

	async function onConflict(ctx: EntryFormConflictContext): Promise<boolean> {
		if (ctx.mode.kind === "new") return false; // The entry form retains new drafts.
		const target = dictionaryEditorStore.currentTable;
		if (!target) return false;
		// Do not unmount the entry form while fetching conflict data. Failure
		// must leave its input available for retry instead of destroying it.
		const serverTable = await api.getControlTable(target.key);
		if (serverTable.key !== target.key || serverTable.path !== target.path || dictionaryEditorStore.currentKey !== target.key || dictionaryEditorStore.currentTable !== target) {
			throw new Error("確認中に辞書が変わりました。対象を確認してください。");
		}
		dictionaryEditorStore.applyTableOverride(serverTable);

		const baseRow = ctx.mode.row;
		conflictBaseRow = baseRow;
		conflictMyValues = ctx.myValues;
		conflictServerRow = findConflictRow(baseRow, serverTable.rows);
		conflictOpen = true;
		return true;
	}

	async function onConflictResolved(): Promise<void> {
		conflictOpen = false;
		await dictionaryEditorStore.reloadCurrent();
	}
</script>

<section class="flex flex-col gap-3">
	<header class="flex flex-wrap items-center gap-2">
		<h3 class="text-sm font-semibold">辞書の編集</h3>
		<span class="text-xs opacity-60">
			conf.toml の
			<code class="rounded bg-surface-100-900 px-1">[[control_api.tables]]</code>
			で <code>role = "glossary"</code> を指定した TSV が対象です。
		</span>
		<button
			type="button"
			class="ms-auto rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-100-900"
			onclick={() => void dictionaryEditorStore.loadCatalog()}
			disabled={deleting || deleteReview !== null || dictionaryEditorStore.phase.kind === "loading-catalog"}
		>
			↻ カタログ再取得
		</button>
	</header>

	{#if deleteReview}
		<div class="rounded border border-warning-500 p-3 text-sm" role="region" aria-label="辞書行の削除確認">
			<p>「{deleteReview.label}」を辞書から削除する？</p>
			<p class="mt-1 break-all text-xs">対象：{deleteReview.key} · {deleteReview.path} · 行 {deleteReview.index}</p>
			<div class="mt-3 flex flex-wrap gap-3">
				<button bind:this={keepRowButton} type="button" class="rounded border border-surface-300-700 px-3 py-2" disabled={deleting} onclick={()=>deleteReview=null}>行を残して戻る</button>
				<button type="button" class="rounded border border-error-500 px-3 py-2" disabled={deleting} onclick={()=>void confirmDelete()}>{deleting?'削除中…':'この行を削除する'}</button>
			</div>
		</div>
	{/if}
	{#if deleteError}<p role="alert" class="text-sm text-error-500">{deleteError}</p>{/if}

	{#if dictionaryEditorStore.phase.kind === "loading-catalog"}
		<div class="text-xs opacity-70">カタログをロード中…</div>
	{:else if dictionaryEditorStore.phase.kind === "error"}
		<div class="rounded bg-error-100-900 px-2 py-1 text-xs text-error-700-300">
			{dictionaryEditorStore.phase.message}
		</div>
	{:else if dictionaryEditorStore.dictionaryTables.length === 0}
		<div class="rounded bg-surface-100-900 px-3 py-2 text-xs opacity-80">
			<code>[[control_api.tables]]</code> に <code>role = "glossary"</code> の エントリが登録されていません。<br />
			例:
			<pre class="mt-1 rounded bg-surface-950-50 p-2 text-surface-50-950 text-[0.65rem] leading-snug">{`[[control_api.tables]]
key       = "chat_dict"
path      = "dictionary.chat.dict.tsv"
label     = "Chat 用語集"
role      = "glossary"
editable  = true`}</pre>
			設定を反映するには VAC の再起動が必要です。
		</div>
	{:else}
		<!-- タブ一覧 -->
		<nav class="flex flex-wrap gap-1 border-b border-surface-300-700">
			{#each dictionaryEditorStore.dictionaryTables as t (t.key)}
				{@const active = dictionaryEditorStore.currentKey === t.key}
				<button
					type="button"
					class="rounded-t border-b-2 px-3 py-1 text-xs"
					class:border-primary-500={active}
					class:font-semibold={active}
					class:border-transparent={!active}
					disabled={deleting || deleteReview !== null}
					onclick={() => void selectTab(t.key)}
					title={t.path}
				>
					{t.label ?? t.key}
					{#if !t.exists}
						<span class="ms-1 text-warning-700-300" title="ファイル未作成（空 Table 扱い）">⚠</span>
					{/if}
					{#if !t.editable}
						<span class="ms-1 opacity-60" title="read-only">🔒</span>
					{/if}
				</button>
			{/each}
		</nav>

		<!-- 現在選択中の Table -->
		{#if dictionaryEditorStore.phase.kind === "loading-table"}
			<div class="text-xs opacity-70">
				{dictionaryEditorStore.phase.key} を読み込み中…
			</div>
		{:else if dictionaryEditorStore.currentTable}
			<div class="flex flex-col gap-2">
				<div class="text-[0.7rem] opacity-60">
					<span class="font-mono">{dictionaryEditorStore.currentTable.path}</span>
					<span class="mx-2 opacity-40">|</span>
					hash:
					<span class="font-mono">
						b3:{dictionaryEditorStore.currentTable.content_hash.slice(0, 12)}…
					</span>
					<span class="mx-2 opacity-40">|</span>
					columns: {dictionaryEditorStore.currentTable.columns.length}
				</div>

				<fieldset class="min-w-0 border-0 p-0" disabled={deleting || deleteReview !== null} aria-label="辞書の行操作">
				<DictionaryTable
					table={dictionaryEditorStore.currentTable}
					{onEdit}
					onDelete={(r) => void onDelete(r)}
					{onAdd}
					onReload={() => void onReload()}
				/>
				</fieldset>
			</div>

			<DictionaryEntryForm
				bind:open={formOpen}
				table={dictionaryEditorStore.currentTable}
				mode={formMode}
				onSaved={() => void onSaved()}
				onConflict={onConflict}
			/>

			{#if conflictBaseRow}
				<DictionaryConflictDialog
					bind:open={conflictOpen}
					serverTable={dictionaryEditorStore.currentTable}
					serverRow={conflictServerRow}
					baseRow={conflictBaseRow}
					myValues={conflictMyValues}
					onResolved={() => void onConflictResolved()}
					onCancelled={() => (conflictOpen = false)}
					onRefresh={(values) => onConflict({body: null, mode: {kind: "edit", row: conflictServerRow ?? conflictBaseRow!}, myValues: values, priorHash: dictionaryEditorStore.currentTable!.content_hash})}
				/>
			{/if}
		{/if}
	{/if}
</section>

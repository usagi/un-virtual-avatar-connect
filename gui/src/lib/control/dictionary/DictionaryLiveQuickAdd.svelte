<script lang="ts">
	/**
	 * Phase φ-4: V2 Dictionary Live Quick-Add Widget。
	 *
	 * V1 の `DictionaryQuickAddWidget.svelte`（modify processor 時代）を Flowgraph Trigger
	 * ベースで作り直した後継。`[[control_api.tables]].quick_add = {...}` を持つ Table の
	 * 数だけ tab 化し、1 行フォーム (reading + surface + [Learn]) で
	 * `POST /api/v1/control/flowgraph/{inst}/trigger/{quick_add.node_id}` を発火する。
	 *
	 * UI:
	 *   - 上段: 対象テーブル tab + 種別 + reading / surface + [Learn]
	 *   - 下段: 直近 10 件の履歴（per-key）、各行に [Undo]（forget_node_id 設定時のみ）
	 *
	 * 永続化:
	 *   - 設定ファイル別の選択と履歴。旧来の対象未識別履歴は変更せず、操作対象には使わない。
	 *
	 * 再読込:
	 *   - learn 成功時に `dictionaryEditorStore.reloadCurrent()` を呼び、同じ key の
	 *     Editor Pane が開いていれば即時反映させる（flowgraph が table.write_tsv まで
	 *     配線されていることが前提。未配線なら reload しても何も変わらないが害はない）。
	 *
	 * 参考:
	 *   - spec: docs/roadmap/phase-phi-control-api-dictionary-editor.md §7
	 *   - 依存: api.triggerFlowgraphNode / dictionaryEditorStore.catalog
	 */

	import { onMount } from "svelte";
	import { api } from "../../api";
	import { ControlApiError } from "../../types";
	import type { TableCatalogItem } from "../../types";
	import { dictionaryEditorStore, extractControlApiMessage } from "./dictionaryEditorStore.svelte";
	import { toastStore } from "../../toasts.svelte";
	import { tabNavStore } from "../../tabs.svelte";
	import { dictionaryRequestOutcome } from "./quickAddReceipt";

	const LS_TARGET = "vac.dictQuickAdd.target_key";
	const LS_HISTORY_PREFIX = "vac.dictQuickAdd.history.";
	const HISTORY_MAX = 10;

	type HistoryItem = {
		seq: number;
		target_key: string;
		table_path?: string;
		node_id: string;
		forget_node_id: string | null;
		request_format?: "direct" | "control_meta";
		reading: string;
		surface: string;
		part_of_speech: string;
		at: string;
		/** undo 済み（forget trigger 成功）ならフラグを立てて一覧から gray-out。 */
		undone?: boolean;
	};

	type LegacyHistoryItem = Partial<HistoryItem> & {
		source?: unknown;
		replacement?: unknown;
		kind?: unknown;
	};

	let storageScope = $state<string | null>(null);
	let historyReady = $state(false);
	let targetKey = $state<string>("");
	let inputReading = $state("");
	let inputSurface = $state("");
	let kindOverride = $state<"literal" | "regex" | null>(null);
	let submitting = $state(false);
	let undoing = $state<number | null>(null);
	let historyOpen = $state(false);

	// 履歴は per-key で localStorage に保存、in-memory は Map で保持。
	let historyByKey = $state<Record<string, HistoryItem[]>>({});
	let nextSeq = 1;

	// Editor Pane と同じ store のカタログを使う。
	const quickAddTables: TableCatalogItem[] = $derived(dictionaryEditorStore.catalog.filter((t) => t.quick_add != null));

	const selectedTable: TableCatalogItem | null = $derived(quickAddTables.find((t) => t.key === targetKey) ?? null);

	const effectiveKind = $derived.by((): "literal" | "regex" => {
		if (kindOverride) return kindOverride;
		const k = (selectedTable?.quick_add?.kind ?? "literal").toLowerCase();
		return k === "regex" ? "regex" : "literal";
	});

	const history: HistoryItem[] = $derived(targetKey ? (historyByKey[targetKey] ?? []) : []);

	const canLearn = $derived(historyReady && selectedTable != null && !!inputReading.trim() && !!inputSurface.trim() && !submitting);

	onMount(() => {
		let disposed = false;
		void (async () => {
			try {
				const info = await api.desktopProfiles();
				if (disposed) return;
				if (info.current_conf_path) {
					storageScope = encodeURIComponent(info.current_conf_path);
					try { targetKey = localStorage.getItem(`${LS_TARGET}.${storageScope}`) ?? ""; } catch { /* session-only */ }
				}
			} catch { /* Identity unavailable: do not load another profile's persisted history. */ }
			if (disposed) return;
			historyReady = true;
			if (dictionaryEditorStore.catalog.length === 0) void dictionaryEditorStore.loadCatalog();
		})();
		return () => { disposed = true; };
	});

	// カタログが変化したら、選択 key の妥当性と履歴のロードを再評価する。
	$effect(() => {
		if (quickAddTables.length === 0) return;
		if (!targetKey || !quickAddTables.some((t) => t.key === targetKey)) {
			targetKey = quickAddTables[0].key;
		}
	});

	$effect(() => {
		if (!historyReady || !targetKey) return;
		try { if (storageScope) localStorage.setItem(`${LS_TARGET}.${storageScope}`, targetKey); } catch { /* session-only */ }
		if (!(targetKey in historyByKey)) {
			historyByKey = { ...historyByKey, [targetKey]: loadHistory(targetKey) };
		}
	});

	function loadHistory(key: string): HistoryItem[] {
		try {
			if (!storageScope) return [];
			const raw = localStorage.getItem(`${LS_HISTORY_PREFIX}${storageScope}.${encodeURIComponent(key)}`);
			if (!raw) return [];
			const arr = JSON.parse(raw) as unknown[];
			if (!Array.isArray(arr)) return [];
			const migrated = arr.flatMap((value): HistoryItem[] => {
				if (typeof value !== "object" || value === null) return [];
				const item = value as LegacyHistoryItem;
				const reading = typeof item.reading === "string" ? item.reading : item.replacement;
				const surface = typeof item.surface === "string" ? item.surface : item.source;
				if (
					typeof item.seq !== "number" ||
					typeof item.target_key !== "string" ||
					typeof item.node_id !== "string" ||
					(item.forget_node_id !== null && typeof item.forget_node_id !== "string") ||
					(item.request_format !== undefined && item.request_format !== "direct" && item.request_format !== "control_meta") ||
					typeof reading !== "string" ||
					typeof surface !== "string" ||
					typeof item.at !== "string"
				) {
					return [];
				}
				const partOfSpeech =
					typeof item.part_of_speech === "string" ? item.part_of_speech : item.kind === "regex" ? "正規表現" : "名詞";
				return [
					{
						seq: item.seq,
						target_key: item.target_key,
						table_path: typeof item.table_path === "string" ? item.table_path : undefined,
						node_id: item.node_id,
						forget_node_id: item.forget_node_id,
						...(item.request_format ? { request_format: item.request_format } : {}),
						reading,
						surface,
						part_of_speech: partOfSpeech,
						at: item.at,
						undone: item.undone,
					},
				];
			});
			// seq の最大値を nextSeq に反映（複数 key をまたいでも衝突しないよう +1 後に Math.max）。
			for (const h of migrated) if (h.seq >= nextSeq) nextSeq = h.seq + 1;
			persistHistory(key, migrated);
			return migrated;
		} catch {
			return [];
		}
	}

	function persistHistory(key: string, items: HistoryItem[]): void {
		try {
			if (storageScope) localStorage.setItem(`${LS_HISTORY_PREFIX}${storageScope}.${encodeURIComponent(key)}`, JSON.stringify(items));
		} catch {
			// localStorage 満杯等は UX を壊さないように無視。
		}
	}

	function pushHistory(item: HistoryItem): void {
		const prev = historyByKey[item.target_key] ?? [];
		const next = [item, ...prev].slice(0, HISTORY_MAX);
		historyByKey = { ...historyByKey, [item.target_key]: next };
		persistHistory(item.target_key, next);
	}

	function updateHistoryItem(target_key: string, seq: number, patch: Partial<HistoryItem>): void {
		const prev = historyByKey[target_key] ?? [];
		const next = prev.map((h) => (h.seq === seq ? { ...h, ...patch } : h));
		historyByKey = { ...historyByKey, [target_key]: next };
		persistHistory(target_key, next);
	}

	async function onSubmit(): Promise<void> {
		if (!canLearn || !selectedTable || !selectedTable.quick_add) return;
		const submittedTable = selectedTable;
		const submittedReading = inputReading;
		const submittedSurface = inputSurface;
		const reading = inputReading.trim();
		const surface = inputSurface.trim();
		const kind = effectiveKind;
		const partOfSpeech = kind === "regex" ? "正規表現" : "名詞";
		const nodeId = selectedTable.quick_add.node_id;
		const forgetNodeId = selectedTable.quick_add.forget_node_id ?? null;
		const requestFormat = selectedTable.quick_add.request_format ?? "direct";
		const observations = selectedTable.quick_add.learn_observe ?? [];
		submitting = true;
		try {
			const response = await api.triggerFlowgraphNode(nodeId, {
				...(observations.length ? { observe: observations.map(rule => [rule.node_id, rule.port] as [string, string]), wait_ms: 2000 } : {}),
				inputs: requestFormat === "control_meta" ? {
					source_actor: "gui:quick_add",
					meta: { reading, surface, part_of_speech: partOfSpeech },
				} : {
					reading,
					surface,
					part_of_speech: partOfSpeech,
					by: "gui:quick_add",
				},
			});
			const outcome = dictionaryRequestOutcome(response, observations);
			if (!["saved", "unchanged", "accepted"].includes(outcome)) {
				toastStore.error(outcome === "conflict" ? "別の変更と競合しました" : outcome === "failed" ? "辞書の更新処理に失敗しました" : "保存結果を確認できませんでした", "入力を保持しています。辞書の現在の内容を確認してから操作してください。");
				return;
			}
			toastStore.success(outcome === "saved" ? "辞書を保存しました" : outcome === "unchanged" ? "既に登録されています" : "Learn トリガを送信しました", `${surface} → ${reading}`);
			// A duplicate was not created by this request: do not offer an undo
			// that could delete a pre-existing row.
			if (outcome !== "unchanged") {
			pushHistory({
				seq: nextSeq++,
				target_key: submittedTable.key,
				table_path: submittedTable.path,
				node_id: nodeId,
				forget_node_id: forgetNodeId,
				request_format: requestFormat,
				reading,
				surface,
				part_of_speech: partOfSpeech,
				at: new Date().toISOString(),
			});
			}
			if (selectedTable === submittedTable && inputReading === submittedReading && inputSurface === submittedSurface) {
				inputReading = "";
				inputSurface = "";
			}
			// Editor Pane と同じ key を開いていれば自動で反映されるよう再読込。
			if (selectedTable === submittedTable && dictionaryEditorStore.currentKey === submittedTable.key) {
				// flowgraph 側が write_tsv まで配線されていない場合は変化しないが、その時は何もしない。
				void dictionaryEditorStore.reloadCurrent();
			}
		} catch (e) {
			handleTriggerError(e, "Learn トリガに失敗しました");
		} finally {
			submitting = false;
		}
	}

	function canUndo(item: HistoryItem): boolean {
		return !!selectedTable && item.target_key === selectedTable.key && item.table_path === selectedTable.path
			&& item.node_id === selectedTable.quick_add?.node_id
			&& (item.request_format ?? "direct") === (selectedTable.quick_add?.request_format ?? "direct")
			&& !!item.forget_node_id && item.forget_node_id === selectedTable.quick_add?.forget_node_id
			&& !item.undone && undoing == null;
	}

	async function onUndo(item: HistoryItem): Promise<void> {
		if (!canUndo(item) || !item.forget_node_id) return;
		const observations = selectedTable?.quick_add?.forget_observe ?? [];
		undoing = item.seq;
		try {
			const response = await api.triggerFlowgraphNode(item.forget_node_id, {
				...(observations.length ? { observe: observations.map(rule => [rule.node_id, rule.port] as [string, string]), wait_ms: 2000 } : {}),
				inputs: item.request_format === "control_meta" ? {
					source_actor: "gui:quick_add",
					meta: { surface: item.surface, reading: item.reading },
				} : {
					surface: item.surface,
					reading: item.reading,
					mode: "latest",
				},
			});
			const outcome = dictionaryRequestOutcome(response, observations);
			if (!["saved", "unchanged", "accepted"].includes(outcome)) {
				toastStore.error(outcome === "conflict" ? "取消が別の変更と競合しました" : "取消の完了を確認できませんでした", "履歴を保持しています。辞書の現在の内容を確認してください。");
				return;
			}
			updateHistoryItem(item.target_key, item.seq, { undone: true });
			toastStore.success(outcome === "saved" ? "取消を保存しました" : outcome === "unchanged" ? "対象の単語はありません" : "Forget トリガを送信しました", `${item.surface} → ${item.reading}`);
			if (dictionaryEditorStore.currentKey === item.target_key) {
				void dictionaryEditorStore.reloadCurrent();
			}
		} catch (e) {
			handleTriggerError(e, "Forget トリガに失敗しました");
		} finally {
			undoing = null;
		}
	}

	function handleTriggerError(e: unknown, title: string): void {
		if (e instanceof ControlApiError) {
			// 400: control_triggerable=false、404: instance/node unknown、422: 型不一致 など。
			toastStore.error(title, extractControlApiMessage(e));
		} else if (e instanceof Error) {
			toastStore.error(title, e.message);
		} else {
			toastStore.error(title, String(e));
		}
	}

	function onKeyDown(e: KeyboardEvent): void {
		if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
			void onSubmit();
		}
	}

	let historyReview = $state<{ key: string; label: string; content: string } | null>(null);
	let historyReviewError = $state<string | null>(null);
	function clearHistory(): void {
		if (!targetKey || submitting || undoing !== null) return;
		historyReview = { key: targetKey, label: selectedTable?.label ?? targetKey, content: JSON.stringify(historyByKey[targetKey] ?? []) };
		historyReviewError = null;
	}
	function confirmClearHistory(): void {
		const reviewed = historyReview;
		if (!reviewed || submitting || undoing !== null) return;
		if (targetKey !== reviewed.key || JSON.stringify(historyByKey[reviewed.key] ?? []) !== reviewed.content) {
			historyReviewError = "対象または履歴が変わりました。キャンセルして確認し直してください。";
			return;
		}
		historyByKey = { ...historyByKey, [reviewed.key]: [] };
		persistHistory(reviewed.key, []);
		historyReview = null;
	}

</script>

<section class="rounded border border-surface-200-800 bg-surface-50-950 p-2 text-xs">
	<header class="mb-2 flex flex-wrap items-center gap-2">
		<h3 class="text-xs font-semibold">クイック追加</h3>
		<span class="opacity-60">単語と読みを、選択した辞書の追加処理へ送ります。</span>
		{#if quickAddTables.length > 1}
			<select class="ms-auto rounded border border-surface-300-700 bg-surface-50-950 px-1 py-0.5 text-xs" bind:value={targetKey}>
				{#each quickAddTables as t (t.key)}
					<option value={t.key}>{t.label ?? t.key}</option>
				{/each}
			</select>
		{/if}
	</header>

	{#if historyReview}
		<section aria-label="クイック追加履歴の削除確認" class="mb-2 rounded border border-warning-500/50 p-2">
			<p>{historyReview.label} の追加履歴を消しますか？</p>
			<p>この画面の履歴だけを削除します。学習済みの辞書行は残ります。</p>
			{#if historyReviewError}<p role="alert" class="text-error-500">{historyReviewError}</p>{/if}
			<div class="mt-2 flex gap-2"><button class="rounded border px-2 py-1" disabled={submitting || undoing !== null} onclick={confirmClearHistory}>履歴を削除</button><button class="rounded border px-2 py-1" onclick={() => historyReview = null}>キャンセル</button></div>
		</section>
	{/if}

	{#if dictionaryEditorStore.phase.kind === "loading-catalog" && quickAddTables.length === 0}
		<p class="opacity-70" role="status">登録済みの辞書を確認中…</p>
	{:else if dictionaryEditorStore.phase.kind === "error" && quickAddTables.length === 0}
		<p role="alert">辞書の設定を取得できませんでした。未設定かどうかは確認できていません。</p>
		<p class="mt-1 opacity-70">{dictionaryEditorStore.phase.message}</p>
		<button type="button" class="mt-2 rounded border px-3 py-1" onclick={() => void dictionaryEditorStore.loadCatalog()}>辞書を再取得</button>
	{:else if quickAddTables.length === 0}
		<p>すぐ追加できる辞書は、まだ登録されていません。</p>
		<p class="mt-1 opacity-70">保存辞書がある場合は、下の「辞書の編集」から単語を追加できます。ここから追加するには、辞書とFlowgraphの追加処理を設定します。</p>
		<div class="mt-2 flex flex-wrap gap-2">
			<button type="button" class="rounded border px-3 py-1" onclick={() => tabNavStore.openFeature("dictionary", "dictionary-edit")}>保存辞書を確認</button>
			<button type="button" class="rounded border px-3 py-1" onclick={() => tabNavStore.openFeature("settings", "profiles")}>プロファイルの設定へ</button>
			<button type="button" class="rounded border px-3 py-1" onclick={() => tabNavStore.openFeature("flowgraph", "flowgraph-studio")}>追加処理を確認</button>
		</div>
		<details class="mt-2 opacity-70">
			<summary>初回設定の手順・詳細</summary>
			<p class="mt-1">現在は設定内容の編集が必要です。「プロファイルの設定へ」で操作中のプロファイルの「内容編集」（設定ファイル形式ではファイルの編集）を開き、<code>[[control_api.tables]]</code> に辞書を登録します。<code>quick_add.node_id</code> に追加ノード、取消を使う場合は <code>forget_node_id</code> を指定し、保存後に対象VACを再起動します。</p>
			<p class="mt-1">追加結果をファイルへ残すには、Flowgraphで辞書の読込・追加・書戻しを接続します。詳しい構成例は同梱マニュアルの「辞書エディタとクイック追加」を参照してください。</p>
		</details>
	{:else if !selectedTable}
		<p class="opacity-70">テーブルを選択してください。</p>
	{:else}
		<div class="flex flex-wrap items-center gap-2">
			<span class="opacity-60 text-[0.7rem]">{selectedTable.label ?? selectedTable.key}</span>
			<label class="inline-flex items-center gap-1 text-[0.7rem]">
				種別
				<select
					class="rounded border border-surface-300-700 bg-surface-50-950 px-1 py-0.5 text-xs"
					value={kindOverride ?? ""}
					onchange={(e) => {
						const v = (e.currentTarget as HTMLSelectElement).value;
						kindOverride = v === "literal" || v === "regex" ? v : null;
					}}
				>
					<option value="">
						既定（{selectedTable.quick_add?.kind ?? "literal"}）
					</option>
					<option value="literal">literal</option>
					<option value="regex">regex</option>
				</select>
			</label>
			<input
				type="text"
				class="min-w-[8rem] flex-1 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 font-mono text-xs"
				placeholder="reading（読み）"
				bind:value={inputReading}
				onkeydown={onKeyDown}
				disabled={submitting}
			/>
			<span aria-hidden="true" class="opacity-60">⇐</span>
			<input
				type="text"
				class="min-w-[8rem] flex-1 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs"
				placeholder="surface（単語）"
				bind:value={inputSurface}
				onkeydown={onKeyDown}
				disabled={submitting}
			/>
			<button
				type="button"
				class="rounded bg-primary-500 px-3 py-1 text-xs text-white hover:bg-primary-600 disabled:opacity-50"
				onclick={() => void onSubmit()}
				disabled={!canLearn}
				title="Ctrl+Enter でも送信できます"
			>
				{submitting ? "送信中…" : "Learn"}
			</button>
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-100-900"
				onclick={() => (historyOpen = !historyOpen)}
				title="直近 {HISTORY_MAX} 件"
			>
				履歴 ({history.filter((h) => !h.undone).length})
				<span class="ms-1 opacity-60">{historyOpen ? "▲" : "▼"}</span>
			</button>
		</div>

		{#if !selectedTable.quick_add?.forget_node_id}
			<p class="mt-1 text-[0.65rem] opacity-50">
				<code>forget_node_id</code> 未設定のため [Undo] は無効です。 conf.toml の <code>quick_add</code> に追加してください。
			</p>
		{/if}

		{#if historyOpen}
			<div class="mt-2 rounded border border-surface-300-700">
				<div
					class="flex items-center border-b border-surface-300-700 bg-surface-100-900 px-2 py-1 text-[0.65rem] uppercase opacity-70"
				>
					<span>履歴（this table）</span>
					<button type="button" class="ms-auto rounded px-1 py-0.5 hover:bg-surface-50-950" onclick={clearHistory}>
						clear
					</button>
				</div>
				{#if history.length === 0}
					<p class="px-2 py-2 opacity-60">履歴はまだありません</p>
				{:else}
					<ul class="max-h-48 overflow-y-auto text-[0.7rem]">
						{#each history as h (h.seq)}
							<li
								class="flex items-center gap-2 border-b border-surface-200-800 px-2 py-1 last:border-b-0"
								class:opacity-50={h.undone}
							>
								<span class="rounded bg-surface-200-800 px-1 text-[0.6rem] uppercase">
									{h.part_of_speech}
								</span>
								<span>{h.surface}</span>
								<span class="opacity-60">→</span>
								<span class="font-mono">{h.reading}</span>
								<span class="ms-auto font-mono text-[0.65rem] opacity-50">
									{new Date(h.at).toLocaleTimeString()}
								</span>
								<button
									type="button"
									class="rounded border border-surface-300-700 px-1.5 py-0.5 text-[0.65rem] hover:bg-surface-100-900 disabled:opacity-40"
									onclick={() => void onUndo(h)}
									disabled={!canUndo(h)}
									title={h.undone ? "Undo 済み" : h.forget_node_id ? "forget トリガを送る" : "forget_node_id 未設定"}
								>
									{undoing === h.seq ? "…" : h.undone ? "Undone" : "Undo"}
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			</div>
		{/if}
	{/if}
</section>

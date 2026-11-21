<script lang="ts">
	/**
	 * Phase δ-6e: 診断パネル（下部）。
	 *
	 * - `flowgraphStore.diagnostics?.diagnostics` を severity 別に色分けして表示。
	 * - 行クリックで該当ファイルを `store.openFile` で開き、該当ノードを選択する（可能なら）。
	 */
	import FeatureAnchor from "../FeatureAnchor.svelte";
	import { api } from "../api";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	import { toastStore } from "../toasts.svelte";
	import { ControlApiError } from "../types";
	import type { FlowgraphDiagnostic, FlowgraphSeverity } from "../types";
	import { capabilityLabel } from "./effectMetadata";

	let savingStateSnapshot = $state(false);
	let savingLiveStateSnapshot = $state(false);
	let restoringStateSnapshot = $state(false);
	let reloadingPreservingState = $state(false);

	async function onJump(d: FlowgraphDiagnostic) {
		if (!d.file) return;
		// d.file はルートからの相対パス or 絶対パス (OS 依存)。末尾の `.flowgraph.toml` を剥がして fq にする。
		// tree の既知ファイルの中から末尾マッチするものを選ぶのがいちばん頑健。
		const all = flowgraphStore.tree?.files ?? [];
		const fileNorm = String(d.file).replace(/\\/g, "/");
		const match = all.find((f) => fileNorm.endsWith(f.path));
		const fq = match?.fq ?? fileNorm.replace(/\.flowgraph\.toml$/, "").replace(/^.*\//, "");
		if (!fq) return;
		if (!(await flowgraphStore.openFile(fq))) return;
		if (d.node) flowgraphStore.selectedNodeId = d.node;
	}

	function toneClass(s: FlowgraphSeverity): string {
		if (s === "error") return "text-error-500";
		if (s === "warning") return "text-warning-700-300";
		return "text-surface-700-300";
	}

	function badge(s: FlowgraphSeverity): string {
		if (s === "error") return "bg-error-500 text-white";
		if (s === "warning") return "bg-warning-500 text-black";
		return "bg-surface-400 text-white";
	}

	function formatJson(value: unknown): string {
		return JSON.stringify(value);
	}

	function describeError(error: unknown): string {
		if (error instanceof ControlApiError) {
			return `${error.status} ${error.statusText}`;
		}
		if (error instanceof Error) return error.message;
		return String(error);
	}

	async function onSaveLoadedStateSnapshot() {
		if (flowgraphStore.mutating || restoringStateSnapshot || savingStateSnapshot || savingLiveStateSnapshot || reloadingPreservingState) return;
		savingStateSnapshot = true;
		flowgraphStore.mutating = true;
		try {
			const resp = await api.flowgraphSaveLoadedStateSnapshot();
			toastStore.success("State snapshot を保存しました", `${resp.snapshot_node_count} nodes -> ${resp.path}`);
			await flowgraphStore.refreshDiagnostics();
		} catch (error) {
			toastStore.error("State snapshot 保存失敗", describeError(error));
		} finally {
			savingStateSnapshot = false;
			flowgraphStore.mutating = false;
		}
	}

	async function onSaveLiveStateSnapshot() {
		if (flowgraphStore.mutating || restoringStateSnapshot || savingLiveStateSnapshot || savingStateSnapshot || reloadingPreservingState) return;
		savingLiveStateSnapshot = true;
		flowgraphStore.mutating = true;
		try {
			const resp = await api.flowgraphSaveLiveStateSnapshot();
			toastStore.success("Live state snapshot を保存しました", `${resp.snapshot_node_count} nodes -> ${resp.path}`);
			await flowgraphStore.refreshDiagnostics();
		} catch (error) {
			toastStore.error("Live state snapshot 保存失敗", describeError(error));
		} finally {
			savingLiveStateSnapshot = false;
			flowgraphStore.mutating = false;
		}
	}

	async function onReloadPreservingState() {
		if (reloadingPreservingState || restoringStateSnapshot || savingStateSnapshot || savingLiveStateSnapshot) return;
		reloadingPreservingState = true;
		try {
			await flowgraphStore.reloadPreservingState();
		} catch (error) {
			toastStore.error("State-preserving reload 失敗", describeError(error));
		} finally {
			reloadingPreservingState = false;
		}
	}

	async function onRestoreProfileLocalStateSnapshot() {
		if (restoringStateSnapshot || savingStateSnapshot || savingLiveStateSnapshot || reloadingPreservingState) return;
		restoringStateSnapshot = true;
		try { await flowgraphStore.restoreProfileLocalStateSnapshot(); }
		finally { restoringStateSnapshot = false; }
	}

	const diags: FlowgraphDiagnostic[] = $derived([
  ...(flowgraphStore.diagnostics?.diagnostics ?? []),
  ...(flowgraphStore.diagnostics?.runtime_issues ?? []).filter(issue => issue.action === "flowgraph").map(issue => ({
   severity: issue.severity, code: "runtime" as const, message: issue.message,
   node: issue.node ?? undefined, hint: issue.impact,
  })),
 ]);
	const capabilitySummary = $derived(flowgraphStore.diagnostics?.capability_summary);
	const graphSignature = $derived(flowgraphStore.diagnostics?.graph_signature);
	const capabilityCounts = $derived(
		Object.entries(capabilitySummary?.capability_counts ?? {}).map(([capability, count]) => ({
			capability,
			count,
			label: capabilityLabel(capability),
		}))
	);
	const capabilityNodes = $derived(capabilitySummary?.nodes ?? []);
	const stateNodes = $derived(capabilitySummary?.state_nodes ?? []);
	const loadedStateSummary = $derived(flowgraphStore.diagnostics?.loaded_state_summary);
	const loadedStateSnapshot = $derived(flowgraphStore.diagnostics?.loaded_state_snapshot);
	const loadedStateRestoreReport = $derived(flowgraphStore.diagnostics?.loaded_state_restore_report);
	const stateSnapshotFilePath = $derived(flowgraphStore.diagnostics?.state_snapshot_file_path);
	const stateSnapshotFileExists = $derived(flowgraphStore.diagnostics?.state_snapshot_file_exists);
	const loadedStateNodes = $derived(loadedStateSummary?.nodes ?? []);
	const loadedSnapshotNodes = $derived(loadedStateSnapshot?.nodes ?? []);
</script>

{#if import.meta.env.VITE_UN_DESIGN_PREVIEW === '1'}
 <details class="m-2 rounded border border-surface-300-700 p-2 text-xs" aria-label="Flowgraphのログ出力">
  <summary class="cursor-pointer">ログノードの出力</summary>
  <p class="my-2 opacity-65">Logノードに渡した内容です。取得時点の記録で、再読み込みすると消去されます。</p>
  <button class="underline" disabled={flowgraphStore.diagnosticsState==='loading'} onclick={()=>void flowgraphStore.refreshDiagnostics()}>ログを再取得</button>
  {#if flowgraphStore.diagnosticsState !== 'ok'}<p>現在のログを確認できません。再取得してください。</p>
  {:else}
   <ol class="mt-2 max-h-56 overflow-auto space-y-2">
    {#each (flowgraphStore.diagnostics?.runtime_trace??[]).filter(event=>event.phase==='log') as event}
     <li><p class="break-all opacity-60">{event.runtime_node_id} · #{event.generation}</p><pre class="whitespace-pre-wrap break-all">{event.value_summary}</pre></li>
    {:else}<li>ログ出力はまだありません。</li>{/each}
   </ol>
  {/if}
 </details>
{/if}

<div class="h-full">
	{#if flowgraphStore.diagnosticsState !== "ok"}
		<div class="border-b border-surface-300-700 p-2 text-xs" role="status">
			{flowgraphStore.diagnosticsState === "loading" ? "診断を取得中です。" : flowgraphStore.diagnosticsState === "error" ? "診断を取得できません。現在の状態は未確認です。" : "診断はまだ取得していません。"}
			{#if flowgraphStore.diagnostics}前回取得した結果を表示しています。{/if}
			{#if flowgraphStore.diagnosticsError}<p class="mt-1 break-words opacity-70">{flowgraphStore.diagnosticsError}</p>{/if}
			<button type="button" class="mt-2 rounded border border-surface-300-700 px-2 py-1" disabled={flowgraphStore.diagnosticsState === "loading"} onclick={() => void flowgraphStore.refreshDiagnostics()}>診断を再取得</button>
		</div>
	{/if}
	{#if capabilitySummary}
		<div class="border-b border-surface-300-700 px-2 py-1.5 text-xs">
			<div class="flex flex-wrap items-center gap-2">
				<span class="font-mono text-[0.7rem] opacity-70">
					nodes {capabilitySummary.node_count} / effects {capabilitySummary.effectful_node_count}
					/ state {capabilitySummary.stateful_node_count} / snapshots {capabilitySummary.snapshot_supported_state_node_count}
					/ restores {capabilitySummary.restore_supported_state_node_count}
				</span>
				{#if graphSignature}
					<span class="font-mono text-[0.65rem] opacity-55">
						signature files {graphSignature.file_count} / edges {graphSignature.edge_count}
						/ triggers {graphSignature.external_triggers.length} / boundary {graphSignature.boundary_inputs
							.length}:{graphSignature.boundary_outputs.length}
					</span>
				{/if}
				{#if capabilityCounts.length > 0}
					<span class="opacity-50">capabilities</span>
					{#each capabilityCounts as item (item.capability)}
						<span class="rounded border border-surface-300-700 px-1.5 py-0.5 text-[0.65rem]">
							{item.label}
							{item.count}
						</span>
					{/each}
				{:else}
					<span class="opacity-50">capabilities none</span>
				{/if}
			</div>
			{#if capabilityNodes.length > 0}
				<details class="mt-1">
					<summary class="cursor-pointer select-none text-[0.65rem] opacity-60">
						capability nodes {capabilityNodes.length}
					</summary>
					<div class="mt-1 grid gap-1">
						{#each capabilityNodes as node (node.node)}
							<div class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(6rem,auto)] items-center gap-2 text-[0.65rem]">
								<span class="truncate font-mono" title={node.node}>{node.node}</span>
								<span class="truncate font-mono opacity-70" title={node.feature}>{node.feature}</span>
								<span
									class="truncate text-right opacity-70"
									title={node.capabilities.map((capability) => capabilityLabel(capability)).join(", ") ||
										node.effect_class}
								>
									{node.capabilities.map((capability) => capabilityLabel(capability)).join(", ") || node.effect_class}
								</span>
							</div>
						{/each}
					</div>
				</details>
			{/if}
			{#if stateNodes.length > 0}
				<details class="mt-1">
					<summary class="cursor-pointer select-none text-[0.65rem] opacity-60">
						state nodes {stateNodes.length} / volatile {capabilitySummary.volatile_state_node_count}
					</summary>
					<div class="mt-1 grid gap-1">
						{#each stateNodes as node (node.node)}
							<div
								class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(8rem,auto)_minmax(8rem,auto)] items-center gap-2 text-[0.65rem]"
							>
								<span class="truncate font-mono" title={node.node}>{node.node}</span>
								<span class="truncate font-mono opacity-70" title={node.feature}>{node.feature}</span>
								<span class="truncate text-right opacity-70" title={`${node.scope} / ${node.storage} / ${node.lifetime}`}>
									{node.storage} / {node.lifetime}
								</span>
								<span
									class="truncate text-right opacity-70"
									title={`snapshot: ${node.snapshot_policy} / ${node.snapshot_format}, restore: ${node.restore_policy}, migration: ${node.migration_policy}, persistence: ${node.persistence_policy}`}
								>
									snapshot {node.snapshot_policy} / restore {node.restore_policy}
								</span>
							</div>
						{/each}
					</div>
				</details>
			{/if}
			{#if loadedStateSummary && (loadedStateNodes.length > 0 || loadedSnapshotNodes.length > 0)}
				<details class="mt-1">
					<summary class="cursor-pointer select-none text-[0.65rem] opacity-60">
						loaded state {loadedStateSummary.stateful_node_count} / snapshot-capable
						{loadedStateSummary.snapshot_supported_node_count}
						/ restore-capable {loadedStateSummary.restore_supported_node_count} / snapshots {loadedStateSnapshot?.snapshot_node_count ??
							0}
						{#if loadedStateRestoreReport}
							/ restored {loadedStateRestoreReport.restored_node_count}
						{/if}
					</summary>
					<div class="mt-1 grid gap-1">
						{#each loadedStateNodes as node (node.node)}
							<div
								class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(4rem,auto)_minmax(8rem,auto)] items-center gap-2 text-[0.65rem]"
							>
								<span class="truncate font-mono" title={node.node}>{node.node}</span>
								<span class="truncate font-mono opacity-70" title={node.feature}>{node.feature}</span>
								<span class="truncate text-right font-mono opacity-70">v{node.version}</span>
								<span
									class="truncate text-right opacity-70"
									title={`snapshot: ${node.state_model.snapshot_policy} / ${node.state_model.snapshot_format}, restore: ${node.state_model.restore_policy}`}
								>
									{node.state_model.snapshot_format} / {node.state_model.restore_policy}
								</span>
							</div>
						{/each}
						{#each loadedSnapshotNodes as node (node.node)}
							<div
								class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(4rem,auto)_minmax(10rem,auto)] items-center gap-2 text-[0.65rem]"
							>
								<span class="truncate font-mono opacity-70" title={node.node}>{node.node}</span>
								<span class="truncate font-mono opacity-60" title={node.feature}>{node.feature}</span>
								<span class="truncate text-right font-mono opacity-60">v{node.version}</span>
								<span class="truncate text-right font-mono opacity-70" title={formatJson(node.value)}>
									{formatJson(node.value)}
								</span>
							</div>
						{/each}
					</div>
				</details>
			{/if}
			<FeatureAnchor id="flowgraph-state">
			{#if stateSnapshotFilePath}
				<div class="mt-1 flex min-w-0 flex-wrap items-center gap-2">
					<button
						type="button"
						class="shrink-0 rounded border border-surface-300-700 px-1.5 py-0.5 text-[0.65rem] hover:bg-surface-100-900 disabled:opacity-50"
						title="loaded state snapshot を profile-local file に保存"
						disabled={flowgraphStore.mutating || savingStateSnapshot || savingLiveStateSnapshot || restoringStateSnapshot || reloadingPreservingState}
						onclick={onSaveLoadedStateSnapshot}
					>
						{savingStateSnapshot ? "保存中…" : "読込時点の状態を保存"}
					</button>
					<button
						type="button"
						class="shrink-0 rounded border border-surface-300-700 px-1.5 py-0.5 text-[0.65rem] hover:bg-surface-100-900 disabled:opacity-50"
						title="live worker state snapshot を profile-local file に保存"
						disabled={flowgraphStore.mutating || savingStateSnapshot || savingLiveStateSnapshot || restoringStateSnapshot || reloadingPreservingState}
						onclick={onSaveLiveStateSnapshot}
					>
						{savingLiveStateSnapshot ? "保存中…" : "現在の状態を保存"}
					</button>
					<button
						type="button"
						class="shrink-0 rounded border border-surface-300-700 px-1.5 py-0.5 text-[0.65rem] hover:bg-surface-100-900 disabled:opacity-50"
						title="live state snapshot を保存してから reload / restore"
						disabled={flowgraphStore.mutating || savingStateSnapshot || savingLiveStateSnapshot || restoringStateSnapshot || reloadingPreservingState}
						onclick={onReloadPreservingState}
					>
						{reloadingPreservingState ? "再読込中…" : "状態を保持して再読込"}
					</button>
					<button
						type="button"
						class="shrink-0 rounded border border-surface-300-700 px-1.5 py-0.5 text-[0.65rem] hover:bg-surface-100-900 disabled:opacity-50"
						title={stateSnapshotFileExists === false
							? "profile-local snapshot file がまだありません"
							: "profile-local snapshot file から明示 restore して reload"}
						disabled={flowgraphStore.mutating || savingStateSnapshot ||
							savingLiveStateSnapshot ||
							restoringStateSnapshot ||
							reloadingPreservingState ||
							stateSnapshotFileExists === false}
						onclick={onRestoreProfileLocalStateSnapshot}
					>
						{restoringStateSnapshot ? "復元中…" : "保存した状態を復元"}
					</button>
					<div class="min-w-0 truncate font-mono text-[0.65rem] opacity-60" title={stateSnapshotFilePath}>
						state snapshot file {stateSnapshotFilePath}
						{#if stateSnapshotFileExists === false}
							<span class="opacity-70">(missing)</span>
						{/if}
					</div>
				</div>
			{:else}
				<p class="mt-2 opacity-70">処理状態の保存先が未設定のため、状態の保存・復元は利用できません。</p>
			{/if}
			</FeatureAnchor>
		</div>
	{:else}
		<FeatureAnchor id="flowgraph-state"><p class="p-2 text-xs opacity-70">処理状態の保存・復元には、Flowgraphの現在状態を取得する必要があります。</p></FeatureAnchor>
	{/if}
	{#if diags.length === 0}
		{#if flowgraphStore.diagnosticsState === "ok"}<div class="p-2 text-xs opacity-60">診断なし</div>{/if}
	{:else}
		<table class="w-full text-left text-xs">
			<thead class="sticky top-0 bg-surface-100-900">
				<tr class="text-[0.65rem] uppercase opacity-60">
					<th class="px-2 py-1">Sev</th>
					<th class="px-2 py-1">Code</th>
					<th class="px-2 py-1">Message</th>
					<th class="px-2 py-1">File</th>
					<th class="px-2 py-1">Node</th>
				</tr>
			</thead>
			<tbody>
				{#each diags as d, i (i)}
					<tr class="cursor-pointer hover:bg-surface-100-900" onclick={() => onJump(d)}>
						<td class="px-2 py-1">
							<span class={`rounded px-1.5 py-0.5 text-[0.65rem] ${badge(d.severity)}`}>
								{d.severity}
							</span>
						</td>
						<td class="px-2 py-1 font-mono text-[0.7rem]">{d.code}</td>
						<td class={`px-2 py-1 ${toneClass(d.severity)}`}>{d.message}</td>
						<td class="px-2 py-1 font-mono text-[0.7rem] opacity-70">{d.file ?? ""}</td>
						<td class="px-2 py-1 font-mono text-[0.7rem]">{d.node ?? ""}</td>
					</tr>
				{/each}
			</tbody>
		</table>
	{/if}
</div>

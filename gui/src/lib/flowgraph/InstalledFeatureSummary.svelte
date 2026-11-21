<script lang="ts">
	import { onMount } from "svelte";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	import { tabNavStore } from "../tabs.svelte";
	import type { FlowgraphDiagnosticsResponse } from "../types";
	import { featureRuntimeStatus } from "./featureRuntimeStatus";
	let { diagnostics = null }: { diagnostics?: FlowgraphDiagnosticsResponse | null } = $props();
	const files = $derived((flowgraphStore.tree?.files ?? []).filter((file) => file.fq.startsWith("features/")));
	const allFiles = $derived(flowgraphStore.tree?.files ?? []);
    const allStatuses = $derived(allFiles.map(file=>({file,status:featureRuntimeStatus(file,flowgraphStore.tree?.root_dir??'',diagnostics)})));
    const statusCounts = $derived.by(()=>{
        const counts=new Map<string,{label:string;count:number;attention:boolean}>();
        for(const {status} of allStatuses){const entry=counts.get(status.label);if(entry)entry.count++;else counts.set(status.label,{label:status.label,count:1,attention:status.attention});}
        return [...counts.values()];
    });
	const inventoryVariant = import.meta.env.VITE_UN_DESIGN_PREVIEW === '1' && typeof window !== 'undefined' ? new URLSearchParams(window.location.search).get('inventory') : null;
	onMount(() => { void flowgraphStore.refreshTree(); });
	function manage() { tabNavStore.openFeature("flowgraph", inventoryVariant === 'linked' || inventoryVariant === 'inline' ? 'feature-inventory' : 'feature-installed'); }
	function manageAll() { tabNavStore.openFeature("flowgraph", "feature-inventory"); }
	async function edit(fq: string) {
		if (flowgraphStore.isDirty || flowgraphStore.mutating || flowgraphStore.treeError) return;
		if (!tabNavStore.setActive("flowgraph")) return;
		await flowgraphStore.openFile(fq);
	}
</script>

<section class="vac-panel" aria-label="ホームの導入済み機能">
	<header class="vac-panel-header flex flex-wrap items-center justify-between gap-3 px-4 py-3">
		<div><h3 class="text-sm font-semibold">{inventoryVariant === 'linked' || inventoryVariant === 'inline' ? '保存された構成' : '追加した機能'}</h3><p class="mt-1 text-xs opacity-65">このVACに保存された構成。保存済みでも、稼働中とは限りません。</p></div>
		<button class="text-xs text-primary-500" onclick={manage}>追加・共有・削除を管理</button>
	</header>
	{#if flowgraphStore.treeError}
		<p role="alert" class="px-4 py-3 text-sm text-warning-500">一覧を更新できません。前回の一覧から編集せず、接続を確認してください。</p>
		<button class="mx-4 mb-3 text-sm text-primary-500" onclick={()=>void flowgraphStore.refreshTree()}>保存一覧を再取得</button>
	{:else if !flowgraphStore.tree}
		<p class="px-4 py-3 text-sm">保存された機能を確認中…</p>
	{:else if inventoryVariant === 'linked' || inventoryVariant === 'inline'}
		{#if allFiles.length === 0}<p class="px-4 py-3 text-sm">保存された構成はありません。</p>{/if}
	{:else if files.length === 0}
		<div class="px-4 py-3 text-sm"><p>追加先に機能はありません。</p><p class="mt-1 text-xs opacity-65">別フォルダーの既存構成はFlowgraph Studioから利用できます。</p></div>
	{:else}
		<ul class="divide-y divide-surface-200-800">
			{#each files as file (file.fq)}
				{@const status = featureRuntimeStatus(file, flowgraphStore.tree.root_dir, diagnostics)}
				<li class="flex flex-wrap items-center gap-3 px-4 py-3">
					<div class="min-w-0 flex-1"><strong class="text-sm">{file.title ?? file.fq}</strong><p class="break-all text-xs opacity-60">{file.fq}</p></div>
					<details class="max-w-sm text-xs" class:text-warning-500={status.attention}>
						<summary class="cursor-pointer">{status.label}</summary>
						<p class="mt-1 break-words opacity-75">{status.detail}</p>
					</details>
					<button class="rounded border border-surface-300-700 px-3 py-2 text-xs disabled:opacity-50" disabled={flowgraphStore.isDirty || flowgraphStore.mutating || !!flowgraphStore.treeError} onclick={() => void edit(file.fq)}>編集: {file.title ?? file.fq}</button>
				</li>
			{/each}
		</ul>
	{/if}
	{#if flowgraphStore.isDirty}<p class="px-4 py-2 text-xs text-warning-500">Studioに未保存の変更があります。先に保存するか、Studioで確認してください。</p>{/if}
	{#if (inventoryVariant === 'linked' || inventoryVariant === 'inline') && flowgraphStore.tree && !flowgraphStore.treeError}
		<div class="border-t border-surface-300-700 px-4 py-3">
			<p class="text-sm">保存された全構成: {allFiles.length}ファイル</p>
			<p class="mt-1 text-xs opacity-65">別フォルダーへの取込や自作の構成も含みます。ファイル数は用途数・稼働数とは異なります。</p>
            {#if statusCounts.length}
                <ul class="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs" aria-label="保存された全構成の読込状態">
                    {#each statusCounts as status}<li class:text-warning-500={status.attention}>{status.label}: {status.count}件</li>{/each}
                </ul>
                <p class="mt-1 text-xs opacity-65">取得時点の保存内容と実行構成の比較です。接続先やイベント処理の成功件数ではありません。</p>
            {/if}
			{#if inventoryVariant === 'inline'}
				<details class="mt-2"><summary class="cursor-pointer text-sm">全構成をここで確認</summary>
					<ul class="mt-2 max-h-80 overflow-auto divide-y divide-surface-200-800">
						{#each allStatuses as {file,status} (file.fq)}
							<li class="flex flex-wrap items-center gap-3 py-2"><div class="min-w-0 flex-1"><strong class="text-sm">{file.title ?? file.fq}</strong><p class="break-all text-xs opacity-60">{file.fq}</p></div><details class="max-w-sm text-xs" class:text-warning-500={status.attention}><summary>{status.label}</summary><p class="mt-1 break-words">{status.detail}</p></details><button class="text-xs disabled:opacity-50" disabled={flowgraphStore.isDirty || flowgraphStore.mutating || !!flowgraphStore.treeError} onclick={()=>void edit(file.fq)}>編集: {file.fq}</button></li>
						{/each}
					</ul>
				</details>
			{/if}
			<button class="mt-2 text-sm text-primary-500" onclick={manageAll}>全構成の編集・共有・削除を管理</button>
		</div>
	{/if}
</section>

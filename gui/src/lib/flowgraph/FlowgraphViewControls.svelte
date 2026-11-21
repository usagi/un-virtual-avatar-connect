<script lang="ts">
	import { Panel, useSvelteFlow } from "@xyflow/svelte";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	let { miniMapVisible = $bindable(true) }: { miniMapVisible?: boolean } = $props();
	const { getNodes, fitView, setZoom } = useSvelteFlow();
	const selectedIds = $derived(flowgraphStore.selectedNodeIds.length ? flowgraphStore.selectedNodeIds : flowgraphStore.selectedNodeId ? [flowgraphStore.selectedNodeId] : []);
	function focusSelection() {
		const nodes = getNodes(selectedIds);
		if (!nodes.length) return;
		void fitView({ nodes, padding: 0.15, maxZoom: 1, duration: 200 });
	}
</script>

<Panel position="top-right">
	<div class="flex flex-wrap justify-end gap-1" role="group" aria-label="キャンバスの表示">
		<button type="button" class="rounded border border-surface-300-700 bg-surface-100-900 px-2 py-1.5 text-xs" title="全ノードが収まるように表示位置と倍率を調整" onclick={() => void fitView({ padding: 0.12, duration: 200 })}>全体を収める</button>
		<button type="button" class="rounded border border-surface-300-700 bg-surface-100-900 px-2 py-1.5 text-xs disabled:opacity-50" disabled={!selectedIds.length} title={selectedIds.length ? "選択中のノード全体が収まるように移動（最大等倍）" : "先にノードを選択してください"} onclick={focusSelection}>選択へ寄る</button>
		<button type="button" class="rounded border border-surface-300-700 bg-surface-100-900 px-2 py-1.5 text-xs" onclick={() => void setZoom(1)}>等倍</button>
		<button type="button" class="rounded border border-surface-300-700 bg-surface-100-900 px-2 py-1.5 text-xs" title="全体の位置関係を示す小さな地図を表示・非表示" aria-pressed={miniMapVisible} onclick={() => (miniMapVisible = !miniMapVisible)}>ミニマップ</button>
	</div>
</Panel>

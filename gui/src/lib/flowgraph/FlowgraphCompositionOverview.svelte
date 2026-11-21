<script lang="ts">
	import { Background, Controls, MiniMap, SvelteFlow, type Edge, type Node } from "@xyflow/svelte";
	import "@xyflow/svelte/dist/style.css";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	let { onopen }: { onopen?: () => void } = $props();

	const composition = $derived(flowgraphStore.diagnostics?.composition);
	const nodes = $derived.by<Node[]>(() => {
		const graphNodes = (composition?.graphs ?? []).map((graph, index) => ({
			id: `graph:${graph.fq}`,
			position: { x: (index % 4) * 280, y: Math.floor(index / 4) * 220 },
			data: { label: `${graph.kind === "entry" ? "Entry" : "Module"}\n${graph.fq}`, graphFq: graph.fq },
			style: "white-space: pre-line; min-width: 180px; border: 2px solid rgb(14 165 233);",
		}));
		const instanceNodes = (composition?.instances ?? []).map((instance, index) => ({
			id: `instance:${instance.runtime_fq}`,
			position: { x: (index % 4) * 280 + 110, y: 110 + Math.floor(index / 4) * 220 },
			data: { label: `@${instance.id}\n${instance.runtime_fq}`, graphFq: instance.graph_fq },
			style: `white-space: pre-line; min-width: 180px; border: 2px solid ${instance.active ? "rgb(34 197 94)" : "rgb(148 163 184)"}; opacity: ${instance.active ? 1 : 0.5};`,
		}));
		return [...graphNodes, ...instanceNodes];
	});
	const edges = $derived.by<Edge[]>(() => {
		const ownerNode = (owner: string) =>
			(composition?.instances ?? []).some((instance) => instance.runtime_fq === owner) ? `instance:${owner}` : `graph:${owner}`;
		const instanceEdges = (composition?.instances ?? []).flatMap<Edge>((instance, index) => [
			{
				id: `contains:${index}`,
				source: ownerNode(instance.owner_fq),
				target: `instance:${instance.runtime_fq}`,
				label: "contains",
				animated: instance.active,
			},
			{
				id: `instantiates:${index}`,
				source: `instance:${instance.runtime_fq}`,
				target: `graph:${instance.graph_fq}`,
				label: "instantiates",
				animated: instance.active,
			},
		]);
		const portalEdges = (composition?.portals ?? []).map<Edge>((portal, index) => ({
			id: `portal:${index}`,
			source: ownerNode(portal.from_fq),
			target: ownerNode(portal.to_fq),
			label: portal.active ? "portal · active" : "portal · inactive",
			animated: portal.active,
			style: portal.active
				? "stroke: rgb(34 197 94); stroke-width: 3;"
				: "stroke: rgb(148 163 184); stroke-width: 2; stroke-dasharray: 5 4;",
		}));
		return [...instanceEdges, ...portalEdges];
	});

	async function onNodeClick(event: { node: Node; event: MouseEvent | TouchEvent }) {
		const graphFq = event.node.data?.graphFq;
		if (typeof graphFq === "string") {
			if (await flowgraphStore.openFile(graphFq)) onopen?.();
		}
	}
</script>

<div class="h-full min-h-[28rem]" data-testid="flowgraph-composition-overview">
	<SvelteFlow {nodes} {edges} nodeTypes={{}} fitView onnodeclick={onNodeClick}>
		<Background />
		<Controls />
		<MiniMap pannable zoomable />
	</SvelteFlow>
</div>

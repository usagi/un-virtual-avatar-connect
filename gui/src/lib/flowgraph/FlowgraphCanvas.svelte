<script lang="ts">
	/**
	 * Phase δ-6e: キャンバス（Svelte Flow で flowgraph の DAG を表示・編集）。
	 *
	 * 今フェーズの機能:
	 *   - ノードの表示（title / feature / id / ポートを持つ簡易カード）
	 *   - ドラッグで位置を編集 → `flowgraphStore.updateNodePosition` へ反映
	 *   - エッジの表示（Bezier）
	 *   - 新規エッジ接続（同一型 / 同一 is_exec のポート間に限定）
	 *   - ノード選択 → `flowgraphStore.selectedNodeId` に反映（プロパティエディタが読む）
	 *
	 * 編集支援:
	 *   - Undo/Redo、マルチ選択、minimap、接続関係を使うauto-layout
	 *
	 * 設計メモ:
	 *   - Svelte Flow の `nodes` / `edges` は $state な配列を要求するので、store からの変換は
	 *     `$derived` ではなく `$effect` で store → local に push する（Svelte Flow 側でドラッグ
	 *     されるとフィールドが直接書き換えられるので、変換結果は mutation-friendly にする必要がある）。
	 *   - ドラッグ完了時（onnodedragstop）に store へ x/y を push。
	 *   - 接続確立時（onconnect）に store.addEdge。
	 *   - 削除は onnodesdelete / onedgesdelete でハンドル。
	 */
	import { Background, Controls, MiniMap, SvelteFlow, SvelteFlowProvider, type Connection, type Edge, type Node } from "@xyflow/svelte";
	import "@xyflow/svelte/dist/style.css";
	import { untrack } from "svelte";
	import { flowgraphStore, parsePortRef } from "../flowgraphStore.svelte";
	import { toastStore } from "../toasts.svelte";
	import type { FlowgraphNodeSpec, FlowgraphSocketTypeExpr } from "../types";
	import FlowgraphNodeCard from "./FlowgraphNodeCard.svelte";
	import FlowgraphGroupFrame from "./FlowgraphGroupFrame.svelte";
	import FlowgraphAutoFit from "./FlowgraphAutoFit.svelte";
	import FlowgraphPaneDropBridge from "./FlowgraphPaneDropBridge.svelte";
	import { flowgraphGroupBounds } from "./flowgraphLayout";
	import FlowgraphViewControls from "./FlowgraphViewControls.svelte";
	import FlowgraphResizeCenter from "./FlowgraphResizeCenter.svelte";
	const canvasPreview = import.meta.env.VITE_UN_DESIGN_PREVIEW === "1";
	const dockedOverview = !canvasPreview || new URLSearchParams(window.location.search).get("overview") === "docked";
	let miniMapVisible = $state(true);
	let { onselectfile }: { onselectfile?: () => void } = $props();

	function socketTypeExprCompatible(outExpr: FlowgraphSocketTypeExpr, inExpr: FlowgraphSocketTypeExpr): boolean {
		if (outExpr.display === inExpr.display) return true;
		if (outExpr.name === "float" && inExpr.name === "quantity") return true;
		if (outExpr.name === "quantity" && inExpr.name === "float") return true;
		if (outExpr.name === "quantity" && inExpr.name === "string") return true;
		if (outExpr.name === "string" && inExpr.name === "datetime") return true;
		if (outExpr.name === "datetime" && inExpr.name === "string") return true;
		if (outExpr.name === "json" && inExpr.name === "motion_frame") return true;
		if (outExpr.name === "motion_frame" && inExpr.name === "json") return true;
		if (outExpr.name === "json" && inExpr.name === "unmf") return true;
		if (outExpr.name === "unmf" && inExpr.name === "json") return true;
		if (outExpr.kind !== "generic" || inExpr.kind !== "generic") return false;
		if (outExpr.name !== inExpr.name) return false;
		const outArgs = outExpr.args ?? [];
		const inArgs = inExpr.args ?? [];
		return outArgs.length === inArgs.length && outArgs.every((arg, index) => socketTypeExprCompatible(arg, inArgs[index]));
	}

	/** engine の `SocketType::compatible_with` + Phase λ `closed_string_variants` に整合。 */
	function portsWireCompatible(
		outTy: string,
		inTy: string,
		outClosed?: string[] | null,
		inClosed?: string[] | null,
		outExpr?: FlowgraphSocketTypeExpr,
		inExpr?: FlowgraphSocketTypeExpr
	): boolean {
		let ok = outExpr && inExpr ? socketTypeExprCompatible(outExpr, inExpr) : false;
		if (!outExpr || !inExpr) {
			if (outTy === inTy) ok = true;
			else if (outTy === "json" || inTy === "json") ok = true;
			else if ((outTy === "float" && inTy === "quantity") || (outTy === "quantity" && inTy === "float")) ok = true;
			else if (outTy === "quantity" && inTy === "string") ok = true;
			else if ((outTy === "string" && inTy === "datetime") || (outTy === "datetime" && inTy === "string")) ok = true;
		}
		if (!ok) return false;
		if (outTy === "string" && inTy === "string" && inClosed?.length && outClosed?.length) {
			if (!outClosed.every((v) => inClosed.includes(v))) return false;
		}
		return true;
	}

	// Seed selection before SvelteFlow's first selection-change effect. An empty
	// initial canvas would publish "nothing selected" over the retained editor.
	let nodes = $state<Node[]>(toFlowNodes());
	let visibleNodes = $state<Node[]>([...toGroupFrameNodes(), ...toFlowNodes()]);
	let edges = $state<Edge[]>([]);
	let edgeSelectionScope = "";
	function edgeSelectionKey(edge: Edge): string {
		const original = (edge.data as { original?: { from: string; to: string } } | undefined)?.original;
		return original ? JSON.stringify([original.from, original.to]) : edge.id;
	}

	function latestTrace(predicate: (event: NonNullable<typeof flowgraphStore.diagnostics>["runtime_trace"][number]) => boolean) {
		return [...(flowgraphStore.diagnostics?.runtime_trace ?? [])].reverse().find(predicate);
	}

	function traceLocalNode(runtimeNodeId: string): string | undefined {
		return runtimeNodeId.split("::").at(-1);
	}

	function nodeRuntimeActivity(nodeId: string): boolean {
		const current = flowgraphStore.currentFq;
		return !!latestTrace((event) => event.source_fq === current && traceLocalNode(event.runtime_node_id) === nodeId && event.active);
	}

	function nodeRuntimeActive(nodeId: string): boolean {
		const current = flowgraphStore.currentFq;
		if (!current) return true;
		return !(flowgraphStore.diagnostics?.inactive_exec_nodes ?? []).includes(`${current}::${nodeId}`);
	}

	type PortalVisual = {
		id: string;
		direction: "incoming" | "outgoing";
		remoteFq: string;
		remoteNodeId: string;
		localNodeId: string;
		localPort: string;
		remotePort: string;
		active: boolean;
	};

	function portalVisuals(): PortalVisual[] {
		const current = flowgraphStore.currentFq;
		if (!current) return [];
		return (flowgraphStore.diagnostics?.composition.portals ?? []).flatMap<PortalVisual>((portal, index) => {
			if (portal.from_fq.includes("/@") || portal.to_fq.includes("/@")) return [];
			const from = parsePortRef(portal.from);
			const to = parsePortRef(portal.to);
			if (portal.from_fq === current && portal.to_fq !== current) {
				return [
					{
						id: `__portal:out:${index}`,
						direction: "outgoing" as const,
						remoteFq: portal.to_fq,
						remoteNodeId: to.nodeId,
						localNodeId: from.nodeId,
						localPort: from.port,
						remotePort: to.port,
						active: portal.active,
					},
				];
			}
			if (portal.to_fq === current && portal.from_fq !== current) {
				return [
					{
						id: `__portal:in:${index}`,
						direction: "incoming" as const,
						remoteFq: portal.from_fq,
						remoteNodeId: from.nodeId,
						localNodeId: to.nodeId,
						localPort: to.port,
						remotePort: from.port,
						active: portal.active,
					},
				];
			}
			return [];
		});
	}

	/** store の draft → Svelte Flow 用 Node[] への変換。 */
	function toFlowNodes(): Node[] {
		const draft = flowgraphStore.draftNodes ?? [];
		const automaticPositions = flowgraphStore.previewAutoLayoutPositions();
		const regular = draft.map((n, i) => {
			const spec = flowgraphStore.findSpec(n.feature);
			return {
				id: n.id,
				type: "flowgraph",
				position: n.position
					? { x: n.position[0], y: n.position[1] }
					: {
							x: automaticPositions[n.id]?.[0] ?? 50 + (i % 6) * 340,
							y: automaticPositions[n.id]?.[1] ?? 50 + Math.floor(i / 6) * 180,
						},
				data: {
					nodeId: n.id,
					feature: n.feature,
					spec,
					groupBadges: flowgraphStore.groupBadgesForNode(n.id),
					active: nodeRuntimeActive(n.id),
					activity: nodeRuntimeActivity(n.id),
				},
				zIndex: flowgraphStore.selectedNodeIds.includes(n.id) || flowgraphStore.selectedNodeId === n.id ? 3 : 2,
				selected: flowgraphStore.selectedNodeIds.includes(n.id) || flowgraphStore.selectedNodeId === n.id,
			};
		});
		const instances = (flowgraphStore.draftInstances ?? []).map((instance, i) => {
			const placed = flowgraphStore.diagnostics?.composition.instances.find(
				(summary) => summary.owner_fq === flowgraphStore.currentFq && summary.id === instance.id
			);
			const definition = flowgraphStore.diagnostics?.composition.graphs.find((graph) => graph.fq === placed?.graph_fq);
			const spec = definition ? graphInstanceSpec(instance.id, definition) : undefined;
			const activity = placed
				? latestTrace((event) => event.runtime_node_id.startsWith(`${placed.runtime_fq}::`) && event.active)
				: undefined;
			return {
				id: instance.id,
				type: "flowgraph",
				position: instance.position
					? { x: instance.position[0], y: instance.position[1] }
					: {
							x: automaticPositions[instance.id]?.[0] ?? 80 + (i % 4) * 340,
							y: automaticPositions[instance.id]?.[1] ?? 520 + Math.floor(i / 4) * 180,
						},
				data: {
					nodeId: instance.id,
					feature: "graph.instance",
					spec,
					graphFq: placed?.graph_fq,
					graphNodeId: activity ? traceLocalNode(activity.runtime_node_id) : undefined,
					active: placed?.active ?? true,
					activity: !!activity,
				},
				zIndex: flowgraphStore.selectedNodeIds.includes(instance.id) || flowgraphStore.selectedNodeId === instance.id ? 3 : 2,
				selected: flowgraphStore.selectedNodeIds.includes(instance.id) || flowgraphStore.selectedNodeId === instance.id,
			};
		});
		const portals = portalVisuals().map((portal, i) => {
			const local = findPortSpec(portal.localNodeId, portal.localPort, portal.direction === "incoming" ? "input" : "output");
			const port = {
				name: portal.remotePort,
				label: portal.remotePort,
				ty: (local?.ty ?? "json") as never,
				direction: portal.direction === "incoming" ? ("output" as const) : ("input" as const),
				is_exec: local?.is_exec ?? false,
				optional: false,
				multi: false,
			};
			const spec: FlowgraphNodeSpec = {
				feature: "graph.portal",
				title: `${portal.direction === "incoming" ? "←" : "→"} ${portal.remoteFq}`,
				category: "composition",
				description: `Cross-file ${portal.direction} portal`,
				inputs: portal.direction === "outgoing" ? [port] : [],
				outputs: portal.direction === "incoming" ? [port] : [],
				properties: [],
			};
			return {
				id: portal.id,
				type: "flowgraph",
				position: portal.direction === "incoming" ? { x: -260, y: 80 + i * 150 } : { x: 1100, y: 80 + i * 150 },
				data: {
					nodeId: portal.id,
					feature: "graph.portal",
					spec,
					graphFq: portal.remoteFq,
					graphNodeId: portal.remoteNodeId,
					active: portal.active,
					activity: false,
				},
				zIndex: 2,
				draggable: false,
				deletable: false,
			};
		});
		return [...regular, ...instances, ...portals];
	}

	function graphInstanceSpec(
		instanceId: string,
		definition: NonNullable<typeof flowgraphStore.diagnostics>["composition"]["graphs"][number]
	): FlowgraphNodeSpec {
		const port = (name: string, ty: string, direction: "input" | "output") => ({
			name,
			label: name,
			ty: ty as never,
			direction,
			is_exec: ty === "exec",
			optional: false,
			multi: false,
		});
		return {
			feature: "graph.instance",
			title: `${instanceId} · ${definition.fq}`,
			category: "composition",
			description: `Module graph instance: ${definition.fq}`,
			inputs: definition.inputs.map((input) => port(input.name, input.ty, "input")),
			outputs: definition.outputs.map((output) => port(output.name, output.ty, "output")),
			properties: [],
			effect_class: "stateful",
			capabilities: [],
		};
	}

	type GroupFrame = {
		id: string;
		label: string;
		nodeIds: string[];
		color: string | null;
		x: number;
		y: number;
		width: number;
		height: number;
	};

	function toGroupFrameNodes(): Node[] {
		const groups = flowgraphStore.draftGroups ?? [];
		const layoutNodes = flowgraphStore.layoutNodesForCurrentDraft();
		if (layoutNodes.length === 0 || groups.length === 0) return [];
		const automaticPositions = flowgraphStore.previewAutoLayoutPositions();
		const positions: Record<string, [number, number]> = {};
		for (const node of flowgraphStore.draftNodes ?? []) {
			positions[node.id] = node.position ?? automaticPositions[node.id] ?? [50, 50];
		}
		for (const instance of flowgraphStore.draftInstances ?? []) {
			positions[instance.id] = instance.position ?? automaticPositions[instance.id] ?? [50, 50];
		}
		const groupById = new Map(groups.map((group) => [group.id, group]));
		const frames: GroupFrame[] = flowgraphGroupBounds(layoutNodes, positions, flowgraphStore.layoutGroupsForCurrentDraft()).flatMap(
			(bounds) => {
				const group = groupById.get(bounds.id);
				return group
					? [
							{
								...bounds,
								label: group.label || group.id,
								nodeIds: group.node_ids,
								color: group.color,
							},
						]
					: [];
			}
		);
		return frames.map((g) => ({
			id: `__group:${g.id}`,
			type: "flowgraphGroup",
			position: { x: g.x, y: g.y },
			data: {
				groupId: g.id,
				label: g.label,
				nodeCount: g.nodeIds.length,
				color: g.color,
				selected: flowgraphStore.selectedGroupId === g.id,
				width: g.width,
				height: g.height,
			},
			selectable: true,
			draggable: true,
			deletable: false,
			focusable: false,
			zIndex: 0,
		}));
	}

	/** store の draft → Svelte Flow 用 Edge[]。 */
	function toFlowEdges(): Edge[] {
		const draftEdges = flowgraphStore.draftEdges ?? [];
		const localEdges = draftEdges.filter((edge) => !parsePortRef(edge.from).path && !parsePortRef(edge.to).path);
		const regular = localEdges.map((e, i) => {
			const from = parsePortRef(e.from);
			const to = parsePortRef(e.to);
			const fromSpec = findPortSpec(from.nodeId, from.port, "output");
			const toSpec = findPortSpec(to.nodeId, to.port, "input");
			const isExec = fromSpec?.is_exec ?? false;
			const typeMismatch =
				fromSpec &&
				toSpec &&
				!isExec &&
				!portsWireCompatible(
					fromSpec.ty,
					toSpec.ty,
					fromSpec.closed_string_variants,
					toSpec.closed_string_variants,
					fromSpec.type_expr,
					toSpec.type_expr
				);
			const activity = latestTrace(
				(event) =>
					event.source_fq === flowgraphStore.currentFq &&
					((traceLocalNode(event.runtime_node_id) === from.nodeId && event.port === from.port) ||
						(traceLocalNode(event.runtime_node_id) === to.nodeId && event.port === to.port)) &&
					event.active
			);
			let style: string | undefined;
			if (activity) style = "stroke: rgb(34 197 94); stroke-width: 4;";
			else if (isExec) style = "stroke: rgb(249 115 22); stroke-width: 2;";
			else if (typeMismatch) style = "stroke: rgb(239 68 68); stroke-width: 2; stroke-dasharray: 5 4;";
			return {
				id: `e-${i}-${e.from}->${e.to}`,
				zIndex: 1,
				source: from.nodeId,
				sourceHandle: from.port,
				target: to.nodeId,
				targetHandle: to.port,
				animated: isExec || !!activity,
				label: activity ? "live" : undefined,
				data: { original: e },
				style,
			};
		});
		const portalEdges = portalVisuals().map((portal, i) => {
			const local = findPortSpec(portal.localNodeId, portal.localPort, portal.direction === "incoming" ? "input" : "output");
			return {
				id: `portal-edge-${i}-${portal.id}`,
				zIndex: 1,
				source: portal.direction === "incoming" ? portal.id : portal.localNodeId,
				sourceHandle: portal.direction === "incoming" ? portal.remotePort : portal.localPort,
				target: portal.direction === "incoming" ? portal.localNodeId : portal.id,
				targetHandle: portal.direction === "incoming" ? portal.localPort : portal.remotePort,
				animated: local?.is_exec ?? false,
				style: local?.is_exec
					? "stroke: rgb(249 115 22); stroke-width: 2;"
					: "stroke: rgb(14 165 233); stroke-width: 2; stroke-dasharray: 7 4;",
				data: { portal: true },
			};
		});
		return [...regular, ...portalEdges];
	}

	function findPortSpec(
		nodeId: string,
		portName: string,
		dir: "input" | "output"
	):
		| {
				is_exec: boolean;
				multi: boolean;
				ty: string;
				type_expr?: FlowgraphSocketTypeExpr;
				closed_string_variants?: string[] | null;
		  }
		| undefined {
		const node = flowgraphStore.draftNodes?.find((n) => n.id === nodeId);
		const instance = flowgraphStore.draftInstances?.find((item) => item.id === nodeId);
		const placed = instance
			? flowgraphStore.diagnostics?.composition.instances.find(
					(summary) => summary.owner_fq === flowgraphStore.currentFq && summary.id === instance.id
				)
			: undefined;
		const definition = placed
			? flowgraphStore.diagnostics?.composition.graphs.find((graph) => graph.fq === placed.graph_fq)
			: undefined;
		if (!node && !definition) return undefined;
		const spec: FlowgraphNodeSpec | undefined = node ? flowgraphStore.findSpec(node.feature) : graphInstanceSpec(nodeId, definition!);
		if (!spec) return undefined;
		const ports = dir === "input" ? spec.inputs : spec.outputs;
		const p = ports.find((pp) => pp.name === portName);
		return p
			? {
					is_exec: p.is_exec,
					multi: p.multi,
					ty: p.ty,
					type_expr: p.type_expr,
					closed_string_variants: p.closed_string_variants,
				}
			: undefined;
	}

	// store -> local sync
	$effect.pre(() => {
		// Hydrate before the newly mounted canvas emits selection. Do not track
		// selection here: pre-empting pointer selection would cancel Ctrl-click.
		const scope = [flowgraphStore.currentFile?.root_dir, flowgraphStore.currentFq, flowgraphStore.currentState];
		if (scope[2] !== "ok") return;
		untrack(() => {
			nodes = toFlowNodes();
			visibleNodes = [...toGroupFrameNodes(), ...nodes];
		});
	});
	$effect(() => {
		const nextNodes = toFlowNodes();
		nodes = nextNodes;
		visibleNodes = [...toGroupFrameNodes(), ...nextNodes];
	});
	// Node selection updates must not rebuild edges and discard their selection
	// before the user's delete command reaches SvelteFlow.
	$effect(() => {
		const scope = JSON.stringify([flowgraphStore.currentFile?.root_dir, flowgraphStore.currentFq]);
		const next = toFlowEdges();
		const selected = untrack(() => scope === edgeSelectionScope
			? new Set(edges.filter(edge => edge.selected).map(edgeSelectionKey))
			: new Set<string>());
		edgeSelectionScope = scope;
		edges = next.map(edge => ({ ...edge, selected: selected.has(edgeSelectionKey(edge)) }));
	});

	function onNodeDragStop(params: { targetNode: Node | null; nodes: Node[]; event: MouseEvent | TouchEvent }) {
		const n = params.targetNode;
		if (!n) return;
		if (isGroupFrameNode(n.id)) {
			const currentFrame = toGroupFrameNodes().find((frame) => frame.id === n.id);
			if (!currentFrame) return;
			const dx = n.position.x - currentFrame.position.x;
			const dy = n.position.y - currentFrame.position.y;
			flowgraphStore.moveGroupBy(groupIdFromFrameNode(n.id), dx, dy);
			return;
		}
		if (flowgraphStore.draftInstances?.some((instance) => instance.id === n.id)) {
			flowgraphStore.updateGraphInstancePosition(n.id, n.position.x, n.position.y);
			return;
		}
		flowgraphStore.updateNodePosition(n.id, n.position.x, n.position.y);
	}

	function onConnect(c: Connection) {
		// SvelteFlow adds an optimistic local edge before invoking this callback.
		// Reconcile with the draft so a rejected connection cannot remain visible.
		edges = toFlowEdges();
		if (!c.source || !c.target) return;
		// 型チェック: 同じ is_exec 同士・同じ型同士のみ接続を許す。
		const outSpec = findPortSpec(c.source, c.sourceHandle ?? "", "output");
		const inSpec = findPortSpec(c.target, c.targetHandle ?? "", "input");
		if (!outSpec || !inSpec) return;
		if (outSpec.is_exec !== inSpec.is_exec) return;
		if (
			!outSpec.is_exec &&
			!portsWireCompatible(
				outSpec.ty,
				inSpec.ty,
				outSpec.closed_string_variants,
				inSpec.closed_string_variants,
				outSpec.type_expr,
				inSpec.type_expr
			)
		)
			return;
		const from = `${c.source}:${c.sourceHandle ?? ""}`;
		const to = `${c.target}:${c.targetHandle ?? ""}`;
		if (!inSpec.is_exec && !inSpec.multi && flowgraphStore.draftEdges?.some(edge => edge.to === to && edge.from !== from)) {
			toastStore.warn("接続済みの入力", "この入力に接続できるデータ出力は1つです。既存の線を選択して削除してから接続してください。");
			return;
		}
		flowgraphStore.addEdge(from, to);
	}

	function onDelete(params: { nodes: Node[]; edges: Edge[] }) {
		// γ-4a.0: まとめて削除し、直前スナップショットを store に保持して toast から undo できるようにする。
		const nodeIds = params.nodes.map((n) => n.id).filter((id) => !isGroupFrameNode(id) && !id.startsWith("__portal:"));
		const edgePairs = params.edges
			.map((e) => (e.data as { original?: { from: string; to: string } } | undefined)?.original)
			.filter((x): x is { from: string; to: string } => !!x);
		const { removedNodes, removedEdges, undoDelete } = flowgraphStore.removeSelection(nodeIds, edgePairs);
		if (removedNodes === 0 && removedEdges === 0) return;
		const parts: string[] = [];
		if (removedNodes > 0) parts.push(`ノード ${removedNodes}`);
		if (removedEdges > 0) parts.push(`エッジ ${removedEdges}`);
		toastStore.info("削除しました", parts.join(" / "), {
			label: "元に戻す",
			onAction: () => {
				if (!undoDelete?.()) throw new Error("編集対象または履歴が変わりました。対象のグラフで編集履歴を確認してください。");
			},
		});
	}

	function onSelectionChange(params: { nodes: Node[]; edges: Edge[] }) {
		// Loading temporarily unmounts SvelteFlow; its empty selection is not a
		// user action and must not overwrite the draft about to be retained.
		if (flowgraphStore.currentState !== "ok") return;
		const regularNodes = params.nodes.filter((n) => !isGroupFrameNode(n.id));
		const groupNode = params.nodes.find((n) => isGroupFrameNode(n.id));
		if (regularNodes.length > 0) {
			const firstNode = regularNodes[0];
			flowgraphStore.selectedNodeIds = regularNodes.map((n) => n.id);
			flowgraphStore.selectedNodeId = firstNode ? firstNode.id : null;
			return;
		}
		flowgraphStore.selectedNodeIds = [];
		flowgraphStore.selectedNodeId = null;
		if (groupNode) {
			flowgraphStore.selectGroup(groupIdFromFrameNode(groupNode.id));
		}
	}

	function onNodeClick(params: { node: Node; event: MouseEvent | TouchEvent }) {
		const id = params.node.id;
		if (isGroupFrameNode(id)) {
			flowgraphStore.selectGroup(groupIdFromFrameNode(id));
			return;
		}
		const event = params.event;
		const additive = event instanceof MouseEvent && (event.ctrlKey || event.metaKey || event.shiftKey);
		if (!additive) {
			flowgraphStore.selectedNodeIds = [id];
			flowgraphStore.selectedNodeId = id;
			return;
		}
		// SvelteFlow already updates additive selection on pointer down/click.
		// onSelectionChange mirrors that result; toggling again cancels the selection.
	}

	function isGroupFrameNode(id: string): boolean {
		return id.startsWith("__group:");
	}

	function groupIdFromFrameNode(id: string): string {
		return id.replace(/^__group:/, "");
	}

	const nodeTypes = {
		flowgraph: FlowgraphNodeCard,
		flowgraphGroup: FlowgraphGroupFrame,
	};
</script>

<div class="relative h-full w-full">
	{#if !flowgraphStore.currentFq}
		<div class="flex h-full flex-col items-center justify-center gap-3 text-sm">
			<p class="opacity-60">編集するファイルを選択してください。</p>
			{#if onselectfile}
				<button type="button" class="rounded border border-surface-300-700 px-3 py-2 hover:bg-surface-200-800" onclick={onselectfile}>ファイル一覧を開く</button>
			{/if}
		</div>
	{:else if flowgraphStore.currentState === "loading"}
		<div class="flex h-full items-center justify-center text-sm opacity-60">Loading…</div>
	{:else if flowgraphStore.currentState === "error"}
		<div class="flex h-full items-center justify-center text-sm text-error-500">
			{flowgraphStore.currentError}
		</div>
	{:else if !flowgraphStore.currentFile?.parsed}
		<div class="flex h-full items-center justify-center px-4 text-center text-sm opacity-70">
			<div>
				<p class="mb-2 font-semibold text-error-500">TOML パース失敗</p>
				<p class="text-xs">外部エディタでファイルを修正してください。上部の「ファイル操作」から開けます。</p>
			</div>
		</div>
	{:else}
		<SvelteFlowProvider>
		<div class="flowgraph-editor-space">
		<div class="flowgraph-editing-viewport">
		<div class="flowgraph-surface">
		<SvelteFlow
			bind:nodes={visibleNodes}
			bind:edges
			{nodeTypes}
			zIndexMode="manual"
			fitView
			minZoom={canvasPreview ? 0.1 : 0.5}
			colorMode="system"
			proOptions={{ hideAttribution: dockedOverview }}
			multiSelectionKey={["Control", "Shift", "Meta"]}
			deleteKey={["Delete", "Backspace"]}
			onnodeclick={onNodeClick}
			onconnect={onConnect}
			onnodedragstop={onNodeDragStop}
			ondelete={onDelete}
			onselectionchange={onSelectionChange}
		>
			<FlowgraphPaneDropBridge />
			<Background />
			<Controls>
				{#if !canvasPreview}
					<button type="button" class="svelte-flow__controls-button" aria-label="ミニマップ" aria-pressed={miniMapVisible} title={miniMapVisible ? "ミニマップを隠す" : "ミニマップを表示"} onclick={() => (miniMapVisible = !miniMapVisible)}>
                        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 5l6-2 6 2 6-2v16l-6 2-6-2-6 2V5zm6-2v16m6-14v16" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" /></svg>
                    </button>
				{/if}
			</Controls>
			{#if canvasPreview}
				<FlowgraphResizeCenter />
				<FlowgraphViewControls bind:miniMapVisible />
				{#if miniMapVisible && !dockedOverview}<MiniMap pannable zoomable width={140} height={90} />{/if}
			{:else}
				{#if miniMapVisible && !dockedOverview}<MiniMap pannable zoomable />{/if}
			{/if}
			<!-- ファイル切替 or ノード数変化のたびに 2 フレーム待ち fitView を発火する。
        `<SvelteFlow fitView />` の既定は初回マウント時のみ fit するため、
        $effect で後から nodes を差し込む我々の構成では明示的な再 fit が必須。 -->
			<FlowgraphAutoFit triggerKey={`${flowgraphStore.currentFq ?? ""}::${nodes.length}::${flowgraphStore.layoutRevision}`} />
		</SvelteFlow>
		</div>
		{#if dockedOverview}
			<div class="flowgraph-credit"><a href="https://svelteflow.dev" target="_blank" rel="noopener noreferrer" aria-label="Svelte Flow attribution">Svelte Flow</a></div>
		{/if}
		</div>
		{#if dockedOverview && miniMapVisible}
			<aside class="flowgraph-overview-dock" aria-label="編集領域と分離した全体図">
				<span class="text-xs opacity-70">全体図</span>
				<MiniMap pannable zoomable width={140} height={90}
					bgColor="var(--color-surface-100-900)" nodeColor="var(--color-primary-400)"
					maskColor="color-mix(in srgb, var(--color-surface-100-900) 75%, transparent)"
					maskStrokeColor="var(--color-primary-400)" maskStrokeWidth={1}
					style="position:relative;margin:0;inset:auto" />
			</aside>
		{/if}
		</div>
		</SvelteFlowProvider>
		<!-- γ-4a.0: 削除操作の発見性確保。編集中キャンバスの左下にキーボードヒントを固定表示。 -->
		<div class="flowgraph-keyhint pointer-events-none absolute bottom-1 left-1 select-none">
			<code>Delete</code> / <code>Backspace</code> : 選択削除 ・
			<code>Ctrl+D</code>
			: 複製 ・ <code>Ctrl+Click</code> : 複数選択
		</div>
	{/if}
</div>

<style>
	.flowgraph-editor-space { height: 100%; display: flex; min-width: 0; }
	.flowgraph-editing-viewport { flex: 1; min-width: 0; height: 100%; display:flex; flex-direction:column; }
	.flowgraph-surface { flex:1; min-height:0; }
	.flowgraph-credit { flex:none; text-align:right; padding:0 4px; font-size:10px; line-height:16px; }
	.flowgraph-overview-dock { width: 156px; flex-shrink: 0; padding: 8px; border-left: 1px solid var(--color-surface-500); display: flex; flex-direction: column; justify-content: flex-end; gap: 8px; }
	/* Svelte Flow のキャンバスは親 (tab) の dark/light 背景をそのまま使う。
    colorMode="system" を SvelteFlow に渡しているので xyflow 側の内部テーマは
    OS の prefers-color-scheme に連動する。ここで background: transparent
    にしておくと skeleton 側の surface 色が透けて、上位タブと一体感が出る。 */
	:global(.svelte-flow) {
		background: transparent;
	}
	/* OS dark モードでは xyflow の pane / viewport の背景色が設定されていても
    全体としては skeleton の dark 面に乗せるのが見やすい。`.svelte-flow__pane`
    は xyflow 内部クラスだが、style.css が上書きしてくる場合に備えて透明化。 */
	:global(.dark .svelte-flow__pane),
	:global([data-color-mode="dark"] .svelte-flow__pane) {
		background: transparent;
	}
	/* γ-4a.0: キーボードヒント。控えめな配色で、操作の邪魔にならないサイズ感。 */
	.flowgraph-keyhint {
		font-size: 10.5px;
		opacity: 0.55;
		background: rgba(0, 0, 0, 0.04);
		border-radius: 4px;
		padding: 2px 6px;
	}
	.flowgraph-keyhint code {
		font-family: inherit;
		font-weight: 600;
		padding: 0 3px;
		border: 1px solid currentColor;
		border-radius: 3px;
		opacity: 0.9;
	}
	@media (prefers-color-scheme: dark) {
		.flowgraph-keyhint {
			background: rgba(255, 255, 255, 0.06);
		}
	}
</style>

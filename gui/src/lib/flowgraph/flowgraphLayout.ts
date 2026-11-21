export type FlowgraphLayoutNode = {
	id: string;
	width: number;
	height: number;
};

export type FlowgraphLayoutEdge = {
	from: string;
	to: string;
};

export type FlowgraphLayoutGroup = {
	id: string;
	nodeIds: string[];
};

export type FlowgraphGroupBounds = {
	id: string;
	x: number;
	y: number;
	width: number;
	height: number;
};

export type FlowgraphLayoutOptions = {
	originX?: number;
	originY?: number;
	columnGap?: number;
	rowGap?: number;
};

/** 新規nodeの矩形が既存nodeと衝突しない、preferred近傍の決定的位置を返す。 */
export function findOpenFlowgraphPosition(
	nodes: FlowgraphLayoutNode[],
	positions: Partial<Record<string, [number, number]>>,
	candidate: Pick<FlowgraphLayoutNode, "width" | "height">,
	preferred: [number, number],
	clearance = 32
): [number, number] {
	const collides = (x: number, y: number): boolean =>
		nodes.some((node) => {
			const position = positions[node.id];
			if (!position) return false;
			return !(
				x + candidate.width + clearance <= position[0] ||
				position[0] + node.width + clearance <= x ||
				y + candidate.height + clearance <= position[1] ||
				position[1] + node.height + clearance <= y
			);
		});
	if (!collides(preferred[0], preferred[1])) return preferred;

	const stepX = candidate.width + clearance + 64;
	const stepY = candidate.height + clearance + 48;
	for (let ring = 1; ring <= 12; ring += 1) {
		const candidates: Array<[number, number]> = [
			[preferred[0], preferred[1] - stepY * ring],
			[preferred[0] + stepX * ring, preferred[1] - stepY * ring],
			[preferred[0] + stepX * ring, preferred[1]],
			[preferred[0] + stepX * ring, preferred[1] + stepY * ring],
			[preferred[0], preferred[1] + stepY * ring],
			[preferred[0] - stepX * ring, preferred[1] + stepY * ring],
			[preferred[0] - stepX * ring, preferred[1]],
			[preferred[0] - stepX * ring, preferred[1] - stepY * ring],
		];
		const open = candidates.find(([x, y]) => !collides(x, y));
		if (open) return open;
	}
	return [preferred[0] + stepX * 13, preferred[1]];
}

/**
 * 接続方向を左から右へ並べる決定的レイアウト。
 *
 * 循環は strongly-connected component として同じ列へまとめ、各ノードの推定寸法を
 * 使って縦方向へ積む。固定セル格子と違い、ポート数が多いノードも互いに重ならない。
 */
export function layoutFlowgraph(
	nodes: FlowgraphLayoutNode[],
	edges: FlowgraphLayoutEdge[],
	options: FlowgraphLayoutOptions = {}
): Record<string, [number, number]> {
	if (nodes.length === 0) return {};

	const originX = options.originX ?? 80;
	const originY = options.originY ?? 80;
	const columnGap = options.columnGap ?? 120;
	const rowGap = options.rowGap ?? 56;
	const nodeById = new Map(nodes.map((node) => [node.id, node]));
	const order = new Map(nodes.map((node, index) => [node.id, index]));
	const adjacency = new Map(nodes.map((node) => [node.id, new Set<string>()]));
	const predecessors = new Map(nodes.map((node) => [node.id, new Set<string>()]));
	for (const edge of edges) {
		if (edge.from === edge.to || !nodeById.has(edge.from) || !nodeById.has(edge.to)) continue;
		adjacency.get(edge.from)!.add(edge.to);
		predecessors.get(edge.to)!.add(edge.from);
	}

	const components = stronglyConnectedComponents(
		nodes.map((node) => node.id),
		adjacency
	);
	const componentOf = new Map<string, number>();
	components.forEach((component, componentIndex) => {
		for (const id of component) componentOf.set(id, componentIndex);
	});
	const componentEdges = components.map(() => new Set<number>());
	const indegree = components.map(() => 0);
	for (const edge of edges) {
		const from = componentOf.get(edge.from);
		const to = componentOf.get(edge.to);
		if (from === undefined || to === undefined || from === to || componentEdges[from].has(to)) continue;
		componentEdges[from].add(to);
		indegree[to] += 1;
	}
	const componentOrder = components.map((component) => Math.min(...component.map((id) => order.get(id) ?? 0)));
	const ready = indegree
		.map((value, index) => ({ value, index }))
		.filter(({ value }) => value === 0)
		.map(({ index }) => index)
		.sort((a, b) => componentOrder[a] - componentOrder[b]);
	const rank = components.map(() => 0);
	while (ready.length > 0) {
		const current = ready.shift()!;
		for (const next of componentEdges[current]) {
			rank[next] = Math.max(rank[next], rank[current] + 1);
			indegree[next] -= 1;
			if (indegree[next] === 0) {
				ready.push(next);
				ready.sort((a, b) => componentOrder[a] - componentOrder[b]);
			}
		}
	}

	const nodeRank = new Map<string, number>();
	components.forEach((component, componentIndex) => {
		for (const id of component) nodeRank.set(id, rank[componentIndex]);
	});
	const maxRank = Math.max(...rank);
	const columns = Array.from({ length: maxRank + 1 }, () => [] as FlowgraphLayoutNode[]);
	for (const node of nodes) columns[nodeRank.get(node.id) ?? 0].push(node);

	const placedOrder = new Map<string, number>();
	for (let columnIndex = 0; columnIndex < columns.length; columnIndex += 1) {
		columns[columnIndex].sort((a, b) => {
			const aPred = [...(predecessors.get(a.id) ?? [])].map((id) => placedOrder.get(id)).filter((v): v is number => v !== undefined);
			const bPred = [...(predecessors.get(b.id) ?? [])].map((id) => placedOrder.get(id)).filter((v): v is number => v !== undefined);
			const aCenter = aPred.length > 0 ? aPred.reduce((sum, value) => sum + value, 0) / aPred.length : Number.POSITIVE_INFINITY;
			const bCenter = bPred.length > 0 ? bPred.reduce((sum, value) => sum + value, 0) / bPred.length : Number.POSITIVE_INFINITY;
			return aCenter - bCenter || (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0);
		});
		columns[columnIndex].forEach((node, index) => placedOrder.set(node.id, index));
	}

	const columnX: number[] = [];
	let nextX = originX;
	for (const column of columns) {
		columnX.push(nextX);
		nextX += Math.max(0, ...column.map((node) => node.width)) + columnGap;
	}
	const positions: Record<string, [number, number]> = {};
	columns.forEach((column, columnIndex) => {
		let y = originY;
		for (const node of column) {
			positions[node.id] = [Math.round(columnX[columnIndex]), Math.round(y)];
			y += node.height + rowGap;
		}
	});
	return positions;
}

/**
 * groupを視覚clusterとして扱う自動配置。
 *
 * 排他的なgroupは別clusterへ分離する。複数groupが同じnodeを共有する場合だけは
 * 集合の重なりに意味があるため同じclusterへ統合し、重なりを強制解消しない。
 */
export function layoutFlowgraphWithGroups(
	nodes: FlowgraphLayoutNode[],
	edges: FlowgraphLayoutEdge[],
	groups: FlowgraphLayoutGroup[],
	options: FlowgraphLayoutOptions = {}
): Record<string, [number, number]> {
	if (nodes.length === 0 || groups.length === 0) return layoutFlowgraph(nodes, edges, options);
	const nodeById = new Map(nodes.map((node) => [node.id, node]));
	const parent = new Map(nodes.map((node) => [node.id, node.id]));
	const find = (id: string): string => {
		const current = parent.get(id) ?? id;
		if (current === id) return id;
		const root = find(current);
		parent.set(id, root);
		return root;
	};
	const union = (a: string, b: string) => {
		const aRoot = find(a);
		const bRoot = find(b);
		if (aRoot !== bRoot) parent.set(bRoot, aRoot);
	};
	const grouped = new Set<string>();
	for (const group of groups) {
		const members = group.nodeIds.filter((id) => nodeById.has(id));
		members.forEach((id) => grouped.add(id));
		for (let index = 1; index < members.length; index += 1) union(members[0], members[index]);
	}

	type Cluster = {
		id: string;
		nodes: FlowgraphLayoutNode[];
		grouped: boolean;
		localPositions: Record<string, [number, number]>;
		width: number;
		height: number;
	};
	const membersByCluster = new Map<string, FlowgraphLayoutNode[]>();
	for (const node of nodes) {
		const clusterId = grouped.has(node.id) ? `group:${find(node.id)}` : `node:${node.id}`;
		const members = membersByCluster.get(clusterId) ?? [];
		members.push(node);
		membersByCluster.set(clusterId, members);
	}
	const clusterOf = new Map<string, string>();
	const groupPadding = 40;
	const clusters: Cluster[] = [...membersByCluster.entries()].map(([id, members]) => {
		members.forEach((node) => clusterOf.set(node.id, id));
		const isGrouped = id.startsWith("group:");
		const memberIds = new Set(members.map((node) => node.id));
		const internalEdges = edges.filter((edge) => memberIds.has(edge.from) && memberIds.has(edge.to));
		const localPositions = isGrouped
			? layoutFlowgraph(members, internalEdges, {
					originX: groupPadding,
					originY: groupPadding,
					columnGap: 96,
					rowGap: 48,
				})
			: { [members[0].id]: [0, 0] as [number, number] };
		const width = Math.max(...members.map((node) => localPositions[node.id][0] + node.width)) + (isGrouped ? groupPadding : 0);
		const height = Math.max(...members.map((node) => localPositions[node.id][1] + node.height)) + (isGrouped ? groupPadding : 0);
		return { id, nodes: members, grouped: isGrouped, localPositions, width, height };
	});
	const clusterEdges: FlowgraphLayoutEdge[] = [];
	const seenClusterEdges = new Set<string>();
	for (const edge of edges) {
		const from = clusterOf.get(edge.from);
		const to = clusterOf.get(edge.to);
		if (!from || !to || from === to) continue;
		const key = `${from}->${to}`;
		if (seenClusterEdges.has(key)) continue;
		seenClusterEdges.add(key);
		clusterEdges.push({ from, to });
	}
	const clusterPositions = layoutFlowgraph(
		clusters.map((cluster) => ({ id: cluster.id, width: cluster.width, height: cluster.height })),
		clusterEdges,
		{
			...options,
			columnGap: options.columnGap ?? 160,
			rowGap: options.rowGap ?? 88,
		}
	);
	const positions: Record<string, [number, number]> = {};
	for (const cluster of clusters) {
		const [clusterX, clusterY] = clusterPositions[cluster.id];
		for (const node of cluster.nodes) {
			const [localX, localY] = cluster.localPositions[node.id];
			positions[node.id] = [Math.round(clusterX + localX), Math.round(clusterY + localY)];
		}
	}
	return positions;
}

/** 可変寸法node/instanceを含むgroup frameの外接矩形。 */
export function flowgraphGroupBounds(
	nodes: FlowgraphLayoutNode[],
	positions: Partial<Record<string, [number, number]>>,
	groups: FlowgraphLayoutGroup[],
	padding = 40
): FlowgraphGroupBounds[] {
	const nodeById = new Map(nodes.map((node) => [node.id, node]));
	return groups.flatMap((group) => {
		const members = group.nodeIds.flatMap((id) => {
			const node = nodeById.get(id);
			const position = positions[id];
			return node && position ? [{ node, position }] : [];
		});
		if (members.length === 0) return [];
		const minX = Math.min(...members.map(({ position }) => position[0]));
		const minY = Math.min(...members.map(({ position }) => position[1]));
		const maxX = Math.max(...members.map(({ node, position }) => position[0] + node.width));
		const maxY = Math.max(...members.map(({ node, position }) => position[1] + node.height));
		return [
			{ id: group.id, x: minX - padding, y: minY - padding, width: maxX - minX + padding * 2, height: maxY - minY + padding * 2 },
		];
	});
}

/** 排他的group frame同士、またはgroup frameと未所属nodeの視覚衝突を検出する。 */
export function groupLayoutNeedsRepair(
	nodes: FlowgraphLayoutNode[],
	positions: Partial<Record<string, [number, number]>>,
	groups: FlowgraphLayoutGroup[],
	clearance = 16
): boolean {
	if (groups.length === 0) return false;
	const bounds = flowgraphGroupBounds(nodes, positions, groups);
	const boundsById = new Map(bounds.map((bound) => [bound.id, bound]));
	const membersByGroup = new Map(groups.map((group) => [group.id, new Set(group.nodeIds)]));
	const overlaps = (
		a: { x: number; y: number; width: number; height: number },
		b: { x: number; y: number; width: number; height: number }
	) =>
		!(
			a.x + a.width + clearance <= b.x ||
			b.x + b.width + clearance <= a.x ||
			a.y + a.height + clearance <= b.y ||
			b.y + b.height + clearance <= a.y
		);
	for (let index = 0; index < groups.length; index += 1) {
		const a = groups[index];
		const aBounds = boundsById.get(a.id);
		if (!aBounds) continue;
		const aMembers = membersByGroup.get(a.id)!;
		for (let otherIndex = index + 1; otherIndex < groups.length; otherIndex += 1) {
			const b = groups[otherIndex];
			if (b.nodeIds.some((id) => aMembers.has(id))) continue;
			const bBounds = boundsById.get(b.id);
			if (bBounds && overlaps(aBounds, bBounds)) return true;
		}
	}
	const groupedNodes = new Set(groups.flatMap((group) => group.nodeIds));
	for (const node of nodes) {
		if (groupedNodes.has(node.id)) continue;
		const position = positions[node.id];
		if (!position) continue;
		const nodeBounds = { x: position[0], y: position[1], width: node.width, height: node.height };
		if (bounds.some((bound) => overlaps(bound, nodeBounds))) return true;
	}
	return false;
}

/** 保存座標に欠落または矩形衝突があるかを判定する。clearance は配線を読むための余白。 */
export function layoutNeedsRepair(
	nodes: FlowgraphLayoutNode[],
	positions: Partial<Record<string, [number, number]>>,
	clearance = 24
): boolean {
	if (nodes.some((node) => !positions[node.id])) return true;
	for (let i = 0; i < nodes.length; i += 1) {
		const a = nodes[i];
		const [ax, ay] = positions[a.id]!;
		for (let j = i + 1; j < nodes.length; j += 1) {
			const b = nodes[j];
			const [bx, by] = positions[b.id]!;
			const separated =
				ax + a.width + clearance <= bx ||
				bx + b.width + clearance <= ax ||
				ay + a.height + clearance <= by ||
				by + b.height + clearance <= ay;
			if (!separated) return true;
		}
	}
	return false;
}

function stronglyConnectedComponents(ids: string[], adjacency: Map<string, Set<string>>): string[][] {
	let nextIndex = 0;
	const index = new Map<string, number>();
	const lowLink = new Map<string, number>();
	const stack: string[] = [];
	const onStack = new Set<string>();
	const components: string[][] = [];

	const visit = (id: string) => {
		index.set(id, nextIndex);
		lowLink.set(id, nextIndex);
		nextIndex += 1;
		stack.push(id);
		onStack.add(id);
		for (const next of adjacency.get(id) ?? []) {
			if (!index.has(next)) {
				visit(next);
				lowLink.set(id, Math.min(lowLink.get(id)!, lowLink.get(next)!));
			} else if (onStack.has(next)) {
				lowLink.set(id, Math.min(lowLink.get(id)!, index.get(next)!));
			}
		}
		if (lowLink.get(id) !== index.get(id)) return;
		const component: string[] = [];
		while (stack.length > 0) {
			const member = stack.pop()!;
			onStack.delete(member);
			component.push(member);
			if (member === id) break;
		}
		components.push(component);
	};

	for (const id of ids) if (!index.has(id)) visit(id);
	return components;
}

import assert from "node:assert/strict";
import test from "node:test";

import {
	findOpenFlowgraphPosition,
	flowgraphGroupBounds,
	groupLayoutNeedsRepair,
	layoutFlowgraph,
	layoutFlowgraphWithGroups,
	layoutNeedsRepair,
	type FlowgraphLayoutNode,
} from "../src/lib/flowgraph/flowgraphLayout.ts";

test("新規nodeは実寸矩形と余白が既存nodeへ重ならない位置を選ぶ", () => {
	const existing = [
		{ id: "wide", width: 300, height: 240 },
		{ id: "tall", width: 300, height: 420 },
	];
	const positions: Record<string, [number, number]> = { wide: [80, 80], tall: [500, 80] };
	const result = findOpenFlowgraphPosition(existing, positions, { width: 300, height: 180 }, [300, 180], 32);
	assert.equal(layoutNeedsRepair([...existing, { id: "new", width: 300, height: 180 }], { ...positions, new: result }, 32), false);
});

test("接続順に左から右へ配置する", () => {
	const nodes = ["source", "transform", "sink"].map((id) => ({ id, width: 240, height: 100 }));
	const positions = layoutFlowgraph(nodes, [
		{ from: "source", to: "transform" },
		{ from: "transform", to: "sink" },
	]);
	assert.ok(positions.source[0] < positions.transform[0]);
	assert.ok(positions.transform[0] < positions.sink[0]);
});

test("同じ列の可変高ノードを重ねない", () => {
	const nodes: FlowgraphLayoutNode[] = [
		{ id: "short", width: 240, height: 96 },
		{ id: "tall", width: 300, height: 430 },
		{ id: "other", width: 220, height: 150 },
	];
	const positions = layoutFlowgraph(nodes, [], { rowGap: 48 });
	for (let i = 0; i < nodes.length; i += 1) {
		for (let j = i + 1; j < nodes.length; j += 1) {
			const a = nodes[i];
			const b = nodes[j];
			assert.ok(positions[a.id][1] + a.height + 48 <= positions[b.id][1]);
		}
	}
});

test("循環ノードを同じ列へまとめ、後続を右へ送る", () => {
	const nodes = ["a", "b", "out"].map((id) => ({ id, width: 220, height: 100 }));
	const positions = layoutFlowgraph(nodes, [
		{ from: "a", to: "b" },
		{ from: "b", to: "a" },
		{ from: "b", to: "out" },
	]);
	assert.equal(positions.a[0], positions.b[0]);
	assert.ok(positions.b[0] < positions.out[0]);
	assert.notEqual(positions.a[1], positions.b[1]);
});

test("保存済み座標の重なりと欠落を検出する", () => {
	const nodes: FlowgraphLayoutNode[] = [
		{ id: "english", width: 300, height: 180 },
		{ id: "tts", width: 300, height: 430 },
	];
	assert.equal(layoutNeedsRepair(nodes, { english: [900, 200], tts: [1120, 200] }), true);
	assert.equal(layoutNeedsRepair(nodes, { english: [80, 80] }), true);
	assert.equal(layoutNeedsRepair(nodes, { english: [80, 80], tts: [520, 80] }), false);
});

test("group frameは可変node寸法とinstance寸法を外接矩形へ含める", () => {
	const nodes: FlowgraphLayoutNode[] = [
		{ id: "source", width: 220, height: 110 },
		{ id: "translate_instance", width: 300, height: 180 },
		{ id: "tts", width: 300, height: 430 },
	];
	const [bounds] = flowgraphGroupBounds(
		nodes,
		{ source: [100, 120], translate_instance: [480, 180], tts: [900, 80] },
		[{ id: "speech", nodeIds: ["source", "translate_instance", "tts"] }],
		40
	);
	assert.deepEqual(bounds, { id: "speech", x: 60, y: 40, width: 1180, height: 510 });
});

test("排他的groupをclusterとして重ならない領域へ配置する", () => {
	const nodes: FlowgraphLayoutNode[] = [
		{ id: "voice", width: 300, height: 150 },
		{ id: "observe", width: 300, height: 180 },
		{ id: "speech", width: 300, height: 430 },
		{ id: "log", width: 240, height: 110 },
	];
	const groups = [
		{ id: "observation", nodeIds: ["voice", "observe"] },
		{ id: "output", nodeIds: ["speech", "log"] },
	];
	const positions = layoutFlowgraphWithGroups(
		nodes,
		[
			{ from: "voice", to: "observe" },
			{ from: "voice", to: "speech" },
			{ from: "speech", to: "log" },
		],
		groups
	);
	assert.equal(layoutNeedsRepair(nodes, positions, 32), false);
	assert.equal(groupLayoutNeedsRepair(nodes, positions, groups), false);
});

test("排他的groupの重なりは修復対象、共有nodeを持つgroupの重なりは許容する", () => {
	const nodes: FlowgraphLayoutNode[] = [
		{ id: "a", width: 220, height: 110 },
		{ id: "shared", width: 220, height: 110 },
		{ id: "b", width: 220, height: 110 },
	];
	const positions = { a: [80, 80] as [number, number], shared: [340, 80] as [number, number], b: [600, 80] as [number, number] };
	assert.equal(
		groupLayoutNeedsRepair(nodes, positions, [
			{ id: "left", nodeIds: ["a", "shared"] },
			{ id: "right", nodeIds: ["shared", "b"] },
		]),
		false
	);
	assert.equal(
		groupLayoutNeedsRepair(nodes, positions, [
			{ id: "left", nodeIds: ["a"] },
			{ id: "right", nodeIds: ["shared"] },
		]),
		true
	);
});

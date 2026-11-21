import assert from "node:assert/strict";
import test from "node:test";
import { filterPalette } from "../src/lib/flowgraph/paletteFilter.ts";
import type { FlowgraphNodeSpec } from "../src/lib/types.ts";

const groups = [
	{ category: "state", specs: [{ feature: "state.counter", title: "Counter", category: "state", description: "回数を数える" } as FlowgraphNodeSpec] },
	{ category: "twitch", specs: [{ feature: "twitch.chat", title: "Chat", category: "twitch", description: "チャット投稿" } as FlowgraphNodeSpec] },
];

test("hidden nodes remain searchable by feature, title, category and description", () => {
	const hidden = new Set(["state", "twitch"]);
	for (const query of ["state.counter", "COUNTER", "state", "回数"]) {
		const result = filterPalette(groups, query, hidden);
		assert.equal(result.length, 1);
		assert.equal(result[0].hidden, false);
		assert.equal(result[0].specs[0], groups[0].specs[0]);
	}
	assert.deepEqual([...hidden], ["state", "twitch"]);
	assert.deepEqual(filterPalette(groups, " ", hidden).map(group => group.hidden), [true, true]);
});

test("unmatched hidden categories do not disguise empty search results", () => {
	assert.deepEqual(filterPalette(groups, "no-such-node", new Set(["state"])), []);
});

test("unfiltered browse retains every node and category in order", () => {
	const result = filterPalette(groups, "", new Set(["state"]));
	assert.deepEqual(result.flatMap(group => group.specs), groups.flatMap(group => group.specs));
	assert.deepEqual(result.map(group => group.hidden), [true, false]);
});

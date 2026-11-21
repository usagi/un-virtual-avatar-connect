import assert from "node:assert/strict";
import test from "node:test";

import { flowgraphPathForDisplay } from "../src/lib/flowgraph/flowgraphPathDisplay.ts";

test("Windows verbatim path の内部接頭辞を表示しない", () => {
	assert.equal(flowgraphPathForDisplay("//?/C:/Users/example/profile/flowgraph.local"), "C:\\Users\\example\\profile\\flowgraph.local");
	assert.equal(flowgraphPathForDisplay("\\\\?\\C:\\Users\\example\\profile"), "C:\\Users\\example\\profile");
});

test("UNC verbatim path は通常の UNC 表示へ戻す", () => {
	assert.equal(flowgraphPathForDisplay("//?/UNC/server/share/flowgraph.local"), "\\\\server\\share\\flowgraph.local");
	assert.equal(flowgraphPathForDisplay("\\\\?\\UNC\\server\\share"), "\\\\server\\share");
});

test("相対パスや POSIX path は意味を変えない", () => {
	assert.equal(flowgraphPathForDisplay("fixture"), "fixture");
	assert.equal(flowgraphPathForDisplay("/srv/vac/flowgraph.local"), "/srv/vac/flowgraph.local");
	assert.equal(flowgraphPathForDisplay(undefined), "読み込み中…");
});

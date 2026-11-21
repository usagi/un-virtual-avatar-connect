import assert from "node:assert/strict";
import { featureRuntimeStatus } from "../src/lib/flowgraph/featureRuntimeStatus.ts";

const file = { fq: "features/alerts/main", parse_error: null };
const runtime = { root_dir: "C:/preview/first", ok: true, composition: { graphs: [{ fq: file.fq, kind: "entry" as const, inputs: [], outputs: [] }], instances: [], portals: [] } };
assert.equal(featureRuntimeStatus(file, "C:\\preview\\first\\", runtime).label, "実行構成に読込済み");
// The same graph name in a different profile must not imply activation here.
assert.equal(featureRuntimeStatus(file, "C:/preview/second", runtime).label, "別の保存先を実行中");
assert.equal(featureRuntimeStatus(file, runtime.root_dir, null).label, "実行構成を確認できません");
assert.equal(featureRuntimeStatus(file, runtime.root_dir, { ...runtime, ok: false }).label, "実行構成にエラー");
const unavailable = { ...runtime, ok: false, diagnostics: [], runtime_issues: [] };
assert.equal(featureRuntimeStatus(file, runtime.root_dir, unavailable).label, "実行構成を確認できません");
assert.equal(featureRuntimeStatus(file, runtime.root_dir, { ...unavailable, diagnostics: [{severity:"error",code:"toml-parse",message:"Invalid graph"}] }).label, "実行構成にエラー");
assert.equal(featureRuntimeStatus(file, runtime.root_dir, { ...runtime, composition: { ...runtime.composition, graphs: [] } }).label, "実行構成に未反映");
// Invalid saved content must stay visible even if an older runtime loaded this name.
assert.equal(featureRuntimeStatus({ ...file, parse_error: "invalid TOML" }, runtime.root_dir, runtime).label, "保存内容にエラー");
const revisionRuntime = { ...runtime, source_digests: { [file.fq]: "b3:old" } };
assert.equal(featureRuntimeStatus({ ...file, source_digest: "b3:old" }, runtime.root_dir, revisionRuntime).label, "保存内容と読込内容が一致");
assert.equal(featureRuntimeStatus({ ...file, source_digest: "b3:new" }, runtime.root_dir, revisionRuntime).label, "保存後の変更が未反映");
assert.equal(featureRuntimeStatus(file, runtime.root_dir, revisionRuntime).label, "実行構成に読込済み");
console.log("PASS: runtime lookup, distinct profile roots, unavailable evidence, load failure, unapplied graph, invalid saved content");

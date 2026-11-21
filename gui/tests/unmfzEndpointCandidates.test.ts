import { strict as assert } from "node:assert";
import { readFileSync } from "node:fs";
import { collectUnmfzEndpointCandidates, endpointCandidateFromRecord } from "../src/lib/flowgraph/unmfzEndpointCandidates.ts";

const fixturePath = new URL("../../tests/fixtures/unaa_z/discovery_snapshot_mixed.json", import.meta.url);
const fixture = JSON.parse(readFileSync(fixturePath, "utf8")) as {
	apps: Array<Record<string, unknown> & { endpoints?: Record<string, unknown>[] }>;
	endpoints: Record<string, unknown>[];
};

const rendererApp = fixture.apps.find((app) => app.app_id === "un-avatar");
assert(rendererApp, "fixture must contain un-avatar app");
const rendererEndpoint = rendererApp.endpoints?.[0];
assert(rendererEndpoint, "fixture must contain renderer endpoint");

const rendererCandidate = endpointCandidateFromRecord({
	...rendererEndpoint,
	app_id: rendererApp.app_id,
	instance_id: rendererApp.instance_id,
	source_app_id: rendererApp.app_id,
	source_instance_id: rendererApp.instance_id,
	source_app_display_name: rendererApp.display_name,
	discovery_source: "zenoh",
	discovery_source_key: "un/app/**/snapshot",
	discovery_payload_kind: "query",
});

assert(rendererCandidate, "renderer endpoint should parse");
assert.equal(rendererCandidate.appId, "un-avatar");
assert.equal(rendererCandidate.instanceId, "renderer-main");
assert.equal(rendererCandidate.appDisplayName, "UN Avatar Renderer");
assert.equal(rendererCandidate.endpointId, "motion-in");
assert.equal(rendererCandidate.direction, "input");
assert.equal(rendererCandidate.baseKeyExpr, "un-motion/frame/renderer-main");
assert.equal(rendererCandidate.payloadKind, "query");
assert.deepEqual(rendererCandidate.validationWarnings, []);

const invalidCandidate = endpointCandidateFromRecord({
	protocol: "unmfz",
	endpoint_id: "motion-out",
	direction: "sideways",
	validation_warnings: [
		{ code: "missing_base_key_expr", message: "missing base key" },
		{ code: "invalid_direction", message: "bad direction" },
	],
});

assert(invalidCandidate, "invalid endpoint should still parse with fallback");
assert.equal(invalidCandidate.direction, "both");
assert.equal(invalidCandidate.baseKeyExpr, "un-motion/frame");
assert.deepEqual(invalidCandidate.validationWarnings, ["missing_base_key_expr: missing base key", "invalid_direction: bad direction"]);

const diagnosticsCandidates = collectUnmfzEndpointCandidates({
	unaa_z_endpoint_candidates: [
		{
			...rendererCandidate.endpoint,
			candidate_id: "renderer-motion",
		},
		{
			...rendererCandidate.endpoint,
			candidate_id: "renderer-motion",
			label: "duplicate should be ignored",
		},
		invalidCandidate.endpoint,
	],
});

assert.equal(diagnosticsCandidates.length, 2);
assert.deepEqual(
	diagnosticsCandidates.map((candidate) => candidate.id),
	[":motion-out:both:un-motion/frame", "renderer-motion"]
);

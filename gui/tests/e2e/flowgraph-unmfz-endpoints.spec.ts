import { expect, test } from "@playwright/test";
import { readFileSync } from "node:fs";
import { authHeader, openFlowgraphStudio, tokenQuery } from "./fixtures";

const FQ = "sample";
const FILE_PATH = `/api/v1/control/flowgraph/file/${FQ}`;

type DiscoveryFixture = {
	apps?: Array<Record<string, unknown> & { endpoints?: Record<string, unknown>[] }>;
	endpoints?: Record<string, unknown>[];
};

function discoveryFixtureCandidates(): Record<string, unknown>[] {
	const fixtureUrl = new URL("../../../tests/fixtures/unaa_z/discovery_snapshot_mixed.json", import.meta.url);
	const fixture = JSON.parse(readFileSync(fixtureUrl, "utf8")) as DiscoveryFixture;
	const candidates: Record<string, unknown>[] = [];
	for (const app of fixture.apps ?? []) {
		for (const endpoint of app.endpoints ?? []) {
			const candidate: Record<string, unknown> = {
				...endpoint,
				source_app_id: app.app_id,
				source_instance_id: app.instance_id,
				source_app_display_name: app.display_name,
				discovery_source: "zenoh",
				discovery_source_key: `un/app/${app.app_id}/${app.instance_id}/snapshot`,
				discovery_payload_kind: "query",
			};
			if (endpoint.endpoint_id === "motion-out") {
				candidate.validation_warnings = [
					{
						code: "invalid_direction",
						field: "direction",
						message: "direction must be input, output, or both",
					},
					{
						code: "missing_base_key_expr",
						field: "base_key_expr",
						message: "base_key_expr is missing",
					},
				];
			}
			candidates.push(candidate);
		}
	}
	candidates.push(...(fixture.endpoints ?? []));
	return candidates;
}

function diagnosticsResponse(): Record<string, unknown> {
	return {
		diagnostics: [],
		node_meta: {},
		capability_summary: {
			node_count: 0,
			effectful_node_count: 0,
			stateful_node_count: 0,
			snapshot_supported_state_node_count: 0,
			restore_supported_state_node_count: 0,
			capability_counts: {},
			nodes: [],
			state_nodes: [],
		},
		graph_signature: {
			file_count: 0,
			node_count: 0,
			edge_count: 0,
			external_triggers: [],
			boundary_inputs: [],
			boundary_outputs: [],
			files: [],
			type_schemas: [],
			type_schema_graph_summary: {
				schema_count: 0,
				edge_count: 0,
				cycle_count: 0,
				cycles: [],
			},
		},
		package_manifests: [],
		package_lock_preview: [],
		type_schemas: [],
		type_schema_graph_summary: {
			schema_count: 0,
			edge_count: 0,
			cycle_count: 0,
			cycles: [],
		},
		loaded_state_summary: { nodes: [] },
		loaded_state_snapshot: { nodes: [] },
		loaded_state_restore_report: null,
		file_activation: {},
		unaa_z_endpoint_candidates: discoveryFixtureCandidates(),
		unaa_z_endpoint_candidate_diagnostics: [],
	};
}

test.describe("UNMF/Z endpoint candidates", () => {
	test("palette warning and generated group provenance are visible", async ({ page, request }) => {
		const snapRes = await request.get(FILE_PATH, { headers: authHeader() });
		expect(snapRes.status(), await snapRes.text()).toBe(200);

		await page.route("**/api/v1/control/flowgraph/diagnostics", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				json: diagnosticsResponse(),
			});
		});

		await page.goto(`/gui/${tokenQuery()}`);
		await page
			.getByRole("navigation", { name: "Main tabs" })
			.getByRole("button", { name: /flowgraph/i })
			.click();
		await openFlowgraphStudio(page);

		const sampleFile = page.getByRole("button", { name: /^sample\b/ });
		await expect(sampleFile).toBeVisible({ timeout: 15_000 });
		await sampleFile.click();

		const rendererEndpoint = page.getByTestId("unmfz-endpoint-motion-in");
		await expect(rendererEndpoint).toContainText("Renderer Motion In");
		await expect(rendererEndpoint).toContainText("UN Avatar Renderer");
		await expect(rendererEndpoint).toContainText("via query");

		const invalidEndpoint = page.getByTestId("unmfz-endpoint-motion-out");
		await expect(invalidEndpoint).toContainText("Tracker Motion Out");
		await expect(page.getByTestId("unmfz-endpoint-warning-motion-out")).toHaveAttribute("title", /invalid_direction/);

		await rendererEndpoint.getByRole("button", { name: "Publish" }).click();

		const provenance = page.getByTestId("flowgraph-group-provenance");
		await expect(provenance).toBeVisible({ timeout: 10_000 });
		await expect(provenance).toContainText("UNAA/Z provenance");
		await expect(provenance).toContainText("un-avatar");
		await expect(provenance).toContainText("renderer-main");
		await expect(provenance).toContainText("motion-in");
		await expect(provenance).toContainText("un-motion/frame/renderer-main");
		await expect(provenance).toContainText("query");
		await expect(provenance).toContainText("un/app/un-avatar/renderer-main/snapshot");

		await invalidEndpoint.getByRole("button", { name: "Ingress" }).click();
		await expect(provenance).toBeVisible({ timeout: 10_000 });
		await expect(provenance).toContainText("un-motion");
		await expect(provenance).toContainText("motion-out");
		await expect(provenance).toContainText("un-motion/frame");
	});
});

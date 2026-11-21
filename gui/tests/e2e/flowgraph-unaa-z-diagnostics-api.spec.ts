import { expect, test } from "@playwright/test";
import { authHeader } from "./fixtures";

test.describe("UNAA/Z diagnostics API", () => {
	test("flowgraph diagnostics returns UNAA/Z discovery contract without crashing server", async ({ request }) => {
		const diagnostics = await request.get("/api/v1/control/flowgraph/diagnostics", {
			headers: authHeader(),
		});
		expect(diagnostics.status(), await diagnostics.text()).toBe(200);
		const body = (await diagnostics.json()) as Record<string, unknown>;
		expect(body.unaa_z).toEqual(expect.any(Object));
		expect(body.unaa_z_health).toEqual(expect.any(Object));
		expect(body.unaa_z_endpoint_candidates).toEqual(expect.any(Array));
		expect(body.unaa_z_endpoint_candidate_diagnostics).toEqual(expect.any(Array));

		const unaaZ = body.unaa_z as Record<string, unknown>;
		expect(unaaZ.schema_version).toBe(1);
		expect(unaaZ.source).toEqual(expect.any(String));
		expect(unaaZ.apps).toEqual(expect.any(Array));
		expect(unaaZ.endpoints).toEqual(expect.any(Array));
		expect(unaaZ.discovery_errors).toEqual(expect.any(Array));
		const health = body.unaa_z_health as Record<string, unknown>;
		expect(health.source).toEqual(expect.any(String));
		expect(health.app_count).toEqual(expect.any(Number));
		expect(health.endpoint_candidate_count).toEqual(expect.any(Number));
		expect(health.endpoint_warning_count).toEqual(expect.any(Number));
		expect(health.discovery_error_count).toEqual(expect.any(Number));

		const ping = await request.get("/api/v1/control/ping", { headers: authHeader() });
		expect(ping.status(), await ping.text()).toBe(200);
	});
});

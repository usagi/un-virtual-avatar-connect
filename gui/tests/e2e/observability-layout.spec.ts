import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

function diagnosticsWithUnaaZIssues(): Record<string, unknown> {
	return {
		root_dir: "e2e",
		ok: true,
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
		package_dependency_order: [],
		package_lock_preview: [],
		type_schemas: [],
		type_schema_dependency_order: [],
		type_schema_graph_summary: {
			schema_count: 0,
			edge_count: 0,
			cycle_count: 0,
			cycles: [],
		},
		package_lock_preview_digest: null,
		loaded_state_summary: { nodes: [] },
		loaded_state_snapshot: { nodes: [] },
		loaded_state_restore_report: null,
		state_snapshot_file_path: null,
		state_snapshot_file_exists: null,
		file_activation: {},
		inactive_exec_nodes: [],
		unaa_z: {
			schema_version: 1,
			source: "managed_app_registry+zenoh",
			generated_at: "2026-05-30T00:00:00Z",
			apps: [],
			endpoints: [],
			discovery_errors: ["zenoh discovery failed: synthetic e2e timeout"],
		},
		unaa_z_health: {
			source: "managed_app_registry+zenoh",
			app_count: 2,
			managed_registry_app_count: 1,
			zenoh_app_count: 1,
			stale_app_count: 1,
			root_endpoint_count: 1,
			endpoint_candidate_count: 2,
			endpoint_warning_count: 2,
			endpoint_warning_app_count: 1,
			discovery_error_count: 1,
			query_key_count: 2,
			announcement_key_count: 2,
		},
		unaa_z_endpoint_candidates: [],
		unaa_z_endpoint_candidate_diagnostics: [
			{
				severity: "warning",
				code: "missing_base_key_expr",
				message: "base_key_expr is missing",
				field: "base_key_expr",
				candidate_id: "un-motion:motion-out",
				app_id: "un-motion",
				endpoint_id: "motion-out",
				source_key: "un/app/un-motion/main/snapshot",
			},
			{
				severity: "warning",
				code: "invalid_direction",
				message: "direction must be input, output, or both",
				field: "direction",
				candidate_id: "un-motion:motion-out",
				app_id: "un-motion",
				endpoint_id: "motion-out",
				source_key: "un/app/un-motion/main/snapshot",
			},
		],
	};
}

function runtimeVoiceIssue(): Record<string, unknown> {
	return {
		key: "voice:voice-to-tts/main::voice_in",
		severity: "error",
		component: "voice",
		code: "voice_worker_start_failed",
		message: "音声認識ワーカーを開始できない。",
		impact: "この音声入力から Flowgraph へ発話を渡せない。",
		resolution: "Model PathにVoskモデルIDを設定する。",
		action: "flowgraph",
		node: "voice-to-tts/main::voice_in",
		updated_at: "2026-07-14T00:00:00Z",
	};
}

test.describe("GUI redesign: Observability layout", () => {
	test("診断は健康状態を先に示し、証跡を必要なときだけ展開する", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}#observability`);

		await expect(page.getByRole("heading", { name: "診断" })).toBeVisible({
			timeout: 15_000,
		});

		const main = page.getByRole("main");
		await expect(main.getByText("動作状態を確認し、問題がある箇所の記録を調べる。")).toBeVisible();
		await expect(main.getByText("ランタイム", { exact: true })).toBeVisible();
		await expect(main.locator("article").filter({ hasText: "Flowgraph" }).first()).toBeVisible();
		await expect(main.getByText("UNアプリ連携", { exact: true })).toBeVisible();
		await expect(main.getByRole("heading", { name: "ライブイベント" })).toBeHidden();

		await main.locator("summary").filter({ hasText: "ライブイベント" }).click();
		await expect(main.getByRole("button", { name: "channel_datum" })).toBeVisible();
		await expect(main.getByRole("button", { name: "flowgraph_reloaded" })).toBeVisible();

		await main.locator("summary").filter({ hasText: "UNAA/Zの検出とコマンド履歴" }).click();
		await expect(main.getByText("UNAA/Z discovery health")).toBeVisible();
		await expect(main.getByTestId("unaa-z-health-summary")).toContainText("Candidates");

		await main.locator("summary").filter({ hasText: "イベント履歴" }).last().click();
		await expect(main.getByRole("heading", { name: "イベント履歴" })).toBeVisible();
		await expect(main.getByText("運用確認用の server-side ring buffer。")).toBeVisible();
	});

	test("Observability exposes UNAA/Z discovery error and endpoint warning details", async ({ page }) => {
		await page.route("**/api/v1/control/flowgraph/diagnostics", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				json: diagnosticsWithUnaaZIssues(),
			});
		});

		await page.goto(`/gui/${tokenQuery()}#observability`);

		const main = page.getByRole("main");
		await main.locator("summary").filter({ hasText: "UNAA/Zの検出とコマンド履歴" }).click();
		const health = main.getByTestId("unaa-z-health-summary");
		await expect(health).toContainText("errors 1");
		await expect(health).toContainText("warnings 2");
		await expect(health.getByRole("link", { name: "errors 1" })).toHaveAttribute("href", "#unaa-z-discovery-errors");
		await expect(health.getByRole("link", { name: "warnings 2" })).toHaveAttribute("href", "#unaa-z-endpoint-diagnostics");

		await expect(main.locator("#unaa-z-discovery-errors")).toContainText("synthetic e2e timeout");
		await expect(main.locator("#unaa-z-endpoint-diagnostics")).toContainText("missing_base_key_expr");
		await expect(main.locator("#unaa-z-endpoint-diagnostics")).toContainText("invalid_direction");
		await page.unroute("**/api/v1/control/flowgraph/diagnostics");
	});

	test("診断はランタイム問題の影響とnodeを表示する", async ({ page }) => {
		await page.route("**/api/v1/control/flowgraph/diagnostics", async (route) => {
			const response = await route.fetch();
			const diagnostics = (await response.json()) as Record<string, unknown>;
			await route.fulfill({ response, json: { ...diagnostics, runtime_issues: [runtimeVoiceIssue()] } });
		});

		await page.goto(`/gui/${tokenQuery()}#observability`);
		const main = page.getByRole("main");
		await expect(main.getByText("修正が必要")).toBeVisible();
		await expect(main.getByText("ランタイム問題 (1)")).toBeVisible();
		await expect(main.getByText("影響: この音声入力から Flowgraph へ発話を渡せない。")).toBeVisible();
		await expect(main.getByText("voice-to-tts/main::voice_in")).toBeVisible();
	});

	test("診断概要は狭幅でも横へ溢れない", async ({ page }, testInfo) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(`/gui/${tokenQuery()}#observability`);
		await expect(page.getByRole("heading", { name: "診断" })).toBeVisible({ timeout: 15_000 });
		await expect(page.getByRole("button", { name: "状態を更新" })).toBeVisible();
		await expect(page.locator("article").filter({ hasText: "ランタイム" })).toContainText("エラー");
		await expect(page.locator("article").filter({ hasText: "Flowgraph" })).not.toContainText(/確認中|不明/);
		await expect(page.locator("article").filter({ hasText: "UNアプリ連携" })).not.toContainText(/確認中|不明/);
		const dimensions = await page.evaluate(() => ({
			clientWidth: document.documentElement.clientWidth,
			scrollWidth: document.documentElement.scrollWidth,
		}));
		expect(dimensions.scrollWidth).toBeLessThanOrEqual(dimensions.clientWidth);
		await page.screenshot({ path: testInfo.outputPath("diagnostics-mobile.png") });
	});
});

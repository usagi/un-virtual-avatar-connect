import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

const snapshot = {
	schema: 3,
	app_version: "e2e",
	now: "2026-07-14T00:00:00Z",
	runtime: { session_id: "e2e", root: "C:\\vac", session_dir: "C:\\vac\\sessions\\e2e", inline_max_bytes: 4096 },
	processors: [{ index: 0, feature: "flowgraph", id: "main", paused: false, last_invoked_at: null }],
	ai_personas: [
		{
			index: 0,
			id: "companion",
			paused: false,
			usage: {
				completed_reactions: 4,
				failed_reactions: 0,
				api_requests: 12,
				tool_calls: 3,
				input_tokens: 1000,
				cached_tokens: 200,
				cache_write_tokens: 0,
				output_tokens: 300,
				reasoning_tokens: 20,
				last_model: "gpt-5.6-luna",
				last_latency_ms: 840,
				last_updated_at: "2026-07-14T00:00:00Z",
			},
		},
	],
	voice_input: { muted: false, worker_count: 1 },
	bridges: {
		voice_nodes: 1,
		voice_workers: 1,
		voice_ingresses: [
			{
				node_id: "voice/main::ingress",
				label: "配信用マイク",
				source: "CABLE Output",
				engine: "whisper",
				active_backend: "vulkan",
				running: true,
			},
		],
		twitch_irc_nodes: 0,
		twitch_irc_workers: 0,
		twitch_irc_ready_workers: 0,
		twitch_irc_joined_channels: 0,
		twitch_irc_expected_channels: 0,
		twitch_eventsub_nodes: 0,
		twitch_eventsub_workers: 0,
	},
	readiness: { status: "ready", mode: "daily", checks: [] },
	twitch: null,
};

test("LiveはAI操作を主面に置き、管理機能を必要時だけ開く", async ({ page }, testInfo) => {
	let pauseRequest: unknown = null;
	await page.route("**/api/v1/control/desktop/profiles", (route) =>
		route.fulfill({ json: { root: "C:\\vac", managed_mode: false, onboarding_completed: true, active_profile_id: null, profiles: [] } })
	);
	await page.route("**/api/v1/control/snapshot", (route) => route.fulfill({ json: snapshot }));
	await page.route("**/api/v1/control/pause", async (route) => {
		pauseRequest = route.request().postDataJSON();
		snapshot.ai_personas[0].paused = true;
		await route.fulfill({ json: { processors_affected: [], ais_affected: [0], paused: true } });
	});
	await page.route("**/api/v1/control/bos", (route) => route.fulfill({ json: { root: "C:\\vac\\browser_output", entries: [] } }));
	await page.route("**/api/v1/control/quick_effects/presets", (route) => route.fulfill({ json: { presets: [] } }));
	await page.route("**/api/v1/control/quick_effects?*", (route) => route.fulfill({ json: { effects: [], limit: 12, capacity: 128 } }));

	await page.goto(`/gui/${tokenQuery()}#live`);
	await expect(page.getByRole("heading", { name: "ライブ" })).toBeVisible();
	const aiPanel = page.getByRole("region", { name: "AI稼働制御" });
	await expect(aiPanel.getByText("companion")).toBeVisible();
	await expect(aiPanel.getByText("12 requests / 最終 840 ms")).toBeVisible();
	await expect(page.getByRole("region", { name: "音声入力" })).toBeVisible();
	await expect(page.getByRole("region", { name: "演出" })).toBeVisible();
	await expect(page.getByRole("region", { name: "配信前チェック" })).toBeHidden();
	const screenshot = await page.screenshot({ path: testInfo.outputPath("live-operation-layout.png") });
	expect(screenshot.byteLength).toBeGreaterThan(15_000);

	await aiPanel.getByRole("button", { name: "一時停止" }).click();
	await expect.poll(() => pauseRequest).toEqual({ target: "ai", index: 0 });
	await expect(aiPanel.getByText("一時停止中")).toBeVisible();
	await page.getByText("配信準備", { exact: true }).click();
	await expect(page.getByRole("region", { name: "配信前チェック" })).toBeVisible();

	await expect(page.getByRole("heading", { name: "OBS 出力管理" })).toBeHidden();
	await page.getByText("補助操作", { exact: true }).click();
	await page.getByText("出力管理", { exact: true }).click();
	await expect(page.getByRole("heading", { name: "OBS 出力管理" })).toBeVisible();
	await expect(page.getByLabel("label")).toBeHidden();
});

test("Liveの主操作はモバイル幅でも横にはみ出さない", async ({ page }, testInfo) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await page.route("**/api/v1/control/desktop/profiles", (route) =>
		route.fulfill({ json: { root: "C:\\vac", managed_mode: false, onboarding_completed: true, active_profile_id: null, profiles: [] } })
	);
	await page.route("**/api/v1/control/snapshot", (route) => route.fulfill({ json: snapshot }));
	await page.route("**/api/v1/control/bos", (route) => route.fulfill({ json: { root: "C:\\vac\\browser_output", entries: [] } }));
	await page.route("**/api/v1/control/quick_effects/presets", (route) => route.fulfill({ json: { presets: [] } }));
	await page.route("**/api/v1/control/quick_effects?*", (route) => route.fulfill({ json: { effects: [], limit: 12, capacity: 128 } }));

	await page.goto(`/gui/${tokenQuery()}#live`);
	await expect(page.getByRole("heading", { name: "ライブ" })).toBeVisible();
	const viewport = await page.evaluate(() => ({ scrollWidth: document.documentElement.scrollWidth, innerWidth: window.innerWidth }));
	expect(viewport.scrollWidth).toBeLessThanOrEqual(viewport.innerWidth);
	const screenshot = await page.screenshot({ fullPage: true, path: testInfo.outputPath("live-operation-layout-mobile.png") });
	expect(screenshot.byteLength).toBeGreaterThan(15_000);
});

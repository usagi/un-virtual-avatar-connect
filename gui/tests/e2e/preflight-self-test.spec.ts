import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

const snapshot = {
	schema: 3,
	app_version: "e2e",
	now: "2026-08-01T00:00:00Z",
	runtime: { session_id: "e2e", root: "C:\\vac", session_dir: "C:\\vac\\sessions\\e2e", inline_max_bytes: 4096 },
	ai_personas: [],
	voice_input: { muted: false, worker_count: 1 },
	bridges: {
		voice_nodes: 1,
		voice_workers: 1,
		twitch_irc_nodes: 0,
		twitch_irc_workers: 0,
		twitch_irc_ready_workers: 0,
		twitch_irc_joined_channels: 0,
		twitch_irc_expected_channels: 0,
		twitch_eventsub_nodes: 0,
		twitch_eventsub_workers: 0,
	},
	readiness: {
		status: "attention",
		mode: "daily",
		checks: [
			{
				id: "tts",
				area: "conversation",
				status: "attention",
				title: "音声合成",
				detail: "起動後の実再生は未確認。",
				required: true,
				action: "live",
			},
			{
				id: "subtitle",
				area: "stream",
				status: "attention",
				title: "字幕出力",
				detail: "起動後のデータ到達は未確認。",
				required: true,
				action: "live",
			},
		],
	},
	twitch: null,
};

async function routeLiveDependencies(page: import("@playwright/test").Page): Promise<void> {
	await page.route("**/api/v1/control/desktop/profiles", (route) =>
		route.fulfill({ json: { root: "C:\\vac", managed_mode: false, onboarding_completed: true, active_profile_id: null, profiles: [] } })
	);
	await page.route("**/api/v1/control/snapshot", (route) => route.fulfill({ json: snapshot }));
	await page.route("**/api/v1/control/bos", (route) => route.fulfill({ json: { root: "C:\\vac\\browser_output", entries: [] } }));
	await page.route("**/api/v1/control/quick_effects/presets", (route) => route.fulfill({ json: { presets: [] } }));
	await page.route("**/api/v1/control/quick_effects?*", (route) => route.fulfill({ json: { effects: [], limit: 12, capacity: 128 } }));
}

test("配信前セルフテストは字幕、個別TTS、コメント、アバター経路を明示的に分ける", async ({ page }, testInfo) => {
	let subtitleRequest: unknown = null;
	let ttsRequest: unknown = null;
	let chatRequest: unknown = null;
	let avatarRequested = false;
	let ttsTested = false;
	let failAiTts = false;
	const requestOrder: string[] = [];
	await routeLiveDependencies(page);
	await page.route("**/api/v1/control/preflight/self-test", (route) =>
		route.fulfill({
			json: {
				streaming_contract: {
					status: "ready",
					checks: [{ id: "assistant_subtitle", status: "ready", detail: "ai全文はai-synth字幕だけへ送られる。" }],
				},
				tts_nodes: [
					{
						id: "solo/ai-tts::speak",
						file: "solo/ai-tts.flowgraph.toml",
						active: true,
						evidence: "untested",
						elapsed_ms: null,
						error: null,
					},
					{
						id: "solo/chat-tts::speak",
						file: "solo/chat-tts.flowgraph.toml",
						active: true,
						evidence: ttsTested ? "ready" : "untested",
						elapsed_ms: ttsTested ? 184 : null,
						error: null,
					},
				],
				default_subtitle_channels: ["user", "ai-synth"],
				chat_output_routes: [{ subscription_node_id: "solo/chat-tts::sub", tts_node_id: "solo/chat-tts::speak" }],
				avatar_targets: [{ target_link_id: "kaltsit_ai", base_key_expr: "un-motion/frame/kaltsit-ai" }],
			},
		})
	);
	await page.route("**/api/v1/control/preflight/self-test/subtitles", async (route) => {
		subtitleRequest = route.request().postDataJSON();
		requestOrder.push("subtitles");
		await route.fulfill({ json: { channels: ["ai-synth", "user"], datum_ids: [1, 2], hold_ms: 2000 } });
	});
	await page.route("**/api/v1/control/preflight/self-test/tts", async (route) => {
		const request = route.request().postDataJSON() as { node_id: string };
		ttsRequest = request;
		requestOrder.push(`tts:${request.node_id}`);
		ttsTested = true;
		const failed = failAiTts && request.node_id === "solo/ai-tts::speak";
		await route.fulfill({
			json: {
				node_id: request.node_id,
				completed: !failed,
				played: !failed,
				elapsed_ms: 184,
				error: failed ? "COEIROINKへ接続できない" : null,
			},
		});
	});
	await page.route("**/api/v1/control/preflight/self-test/chat-output", async (route) => {
		chatRequest = route.request().postDataJSON();
		requestOrder.push("chat-output");
		await route.fulfill({
			json: {
				subscription_node_id: "solo/chat-tts::sub",
				tts_node_id: "solo/chat-tts::speak",
				subtitle_reached: true,
				completed: true,
				played: true,
				elapsed_ms: 212,
				error: null,
			},
		});
	});
	await page.route("**/api/v1/control/preflight/self-test/avatar-motion", async (route) => {
		avatarRequested = true;
		requestOrder.push("avatar-motion");
		await route.fulfill({
			json: {
				checked_at: "2026-08-01T00:00:00Z",
				expected_base_keys: ["un-motion/frame/kaltsit-ai"],
				renderers: [
					{
						pid: 52216,
						profile_hint: "kalt-sit.toml",
						runtime_bus_key: "un-avatar/runtime/renderer/2",
						connected: true,
						unmotion_zenoh_enabled: true,
						unmotion_zenoh_key: "un-motion/frame/kaltsit-ai",
						key_matches: true,
						received_before: 100,
						received_after: 123,
						received_delta: 23,
						applied_before: 90,
						applied_after: 104,
						applied_delta: 14,
						error: null,
					},
				],
			},
		});
	});

	await page.goto(`/gui/${tokenQuery()}#live`);
	const panel = page.getByRole("region", { name: "配信前セルフテスト" });
	await expect(panel).toBeVisible();
	await expect(panel.getByText("配信経路", { exact: true })).toBeVisible();
	await expect(panel.getByText("正常", { exact: true })).toBeVisible();
	await panel.getByRole("button", { name: "字幕を2秒表示" }).click();
	await expect.poll(() => subtitleRequest).toEqual({ channels: ["user", "ai-synth"], hold_ms: 2000 });
	await expect(panel.getByText("字幕確認完了: ai-synth, user")).toBeVisible();

	await panel.getByRole("combobox").selectOption("solo/chat-tts::speak");
	await panel.getByRole("button", { name: "テスト音声を再生" }).click();
	await expect.poll(() => ttsRequest).toEqual({ node_id: "solo/chat-tts::speak" });
	await expect(panel.getByText("音声確認完了: 184ms")).toBeVisible();
	await expect(panel.getByRole("option", { name: /solo\/chat-tts::speak · 確認済み/ })).toBeAttached();

	await panel.getByRole("button", { name: "字幕と音声を確認" }).click();
	await expect.poll(() => chatRequest).toEqual({});
	await expect(panel.getByText("コメント経路確認完了: 212ms")).toBeVisible();

	await panel.getByRole("button", { name: "動作を確認" }).click();
	await expect.poll(() => avatarRequested).toBe(true);
	await expect(panel.getByText("アバター動作確認完了: 受信 +23 / 適用 +14 frame")).toBeVisible();

	requestOrder.length = 0;
	await panel.getByRole("button", { name: "音声を含む一括確認" }).click();
	await expect(panel.getByText("一括確認完了: 4項目", { exact: true })).toBeVisible();
	await expect(panel.getByText("音声合成 solo/ai-tts::speak", { exact: true })).toBeVisible();
	await expect(panel.getByText("確認済み", { exact: true })).toHaveCount(4);
	expect(requestOrder).toEqual(["subtitles", "tts:solo/ai-tts::speak", "chat-output", "avatar-motion"]);

	failAiTts = true;
	requestOrder.length = 0;
	await panel.getByRole("button", { name: "音声を含む一括確認" }).click();
	await expect(panel.getByText("一括確認: 1件の失敗を検出", { exact: true })).toBeVisible();
	await expect(panel.getByText("音声合成 solo/ai-tts::speak: COEIROINKへ接続できない", { exact: true })).toBeVisible();
	expect(requestOrder).toEqual(["subtitles", "tts:solo/ai-tts::speak", "chat-output", "avatar-motion"]);

	const screenshot = await page.screenshot({ fullPage: true, path: testInfo.outputPath("preflight-self-test.png") });
	expect(screenshot.byteLength).toBeGreaterThan(15_000);
});

test("配信前セルフテストはモバイル幅で横にはみ出さない", async ({ page }, testInfo) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await routeLiveDependencies(page);
	await page.route("**/api/v1/control/preflight/self-test", (route) =>
		route.fulfill({
			json: {
				streaming_contract: { status: "ready", checks: [] },
				tts_nodes: [
					{
						id: "solo/ai-tts::speak",
						file: "solo/ai-tts.flowgraph.toml",
						active: true,
						evidence: "untested",
						elapsed_ms: null,
						error: null,
					},
				],
				default_subtitle_channels: ["user", "ai-synth"],
				chat_output_routes: [],
				avatar_targets: [{ target_link_id: "kaltsit_ai", base_key_expr: "un-motion/frame/kaltsit-ai" }],
			},
		})
	);

	await page.goto(`/gui/${tokenQuery()}#live`);
	const panel = page.getByRole("region", { name: "配信前セルフテスト" });
	await expect(panel).toBeVisible();
	const viewport = await page.evaluate(() => ({ scrollWidth: document.documentElement.scrollWidth, innerWidth: window.innerWidth }));
	expect(viewport.scrollWidth).toBeLessThanOrEqual(viewport.innerWidth);
	const selectBox = await panel.getByRole("combobox").boundingBox();
	const buttonBox = await panel.getByRole("button", { name: "テスト音声を再生" }).boundingBox();
	expect(selectBox).not.toBeNull();
	expect(buttonBox).not.toBeNull();
	expect(selectBox!.y + selectBox!.height).toBeLessThanOrEqual(buttonBox!.y);
	const screenshot = await page.screenshot({ fullPage: true, path: testInfo.outputPath("preflight-self-test-mobile.png") });
	expect(screenshot.byteLength).toBeGreaterThan(15_000);
});

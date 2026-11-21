import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

test.describe("GUI: ホーム", () => {
	test("ホームを最初に表示し、利用者の役割単位で状態を示す", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}`);

		await expect(page.getByRole("heading", { name: "ホーム" })).toBeVisible({
			timeout: 15_000,
		});
		const main = page.getByRole("main");
		const readiness = main.getByRole("region", { name: "利用状態" });
		await expect(readiness).toBeVisible();
		const readinessList = readiness.getByRole("list");
		const readinessItems = readinessList.getByRole("listitem");
		await expect(readinessItems).toHaveCount(3);
		const [listBox, lastItemBox] = await Promise.all([readinessList.boundingBox(), readinessItems.last().boundingBox()]);
		expect(lastItemBox?.width).toBeCloseTo(listBox?.width ?? 0, 0);
		await expect(main.getByText("AIパートナー", { exact: true })).toBeVisible();
		await expect(main.getByText("Twitchチャット入力", { exact: true })).toBeVisible();
		await expect(main.getByText("連携アプリ", { exact: true })).toBeVisible();
		await expect(main.getByText("現在のモード", { exact: true })).toBeVisible();
		await expect(main.getByRole("heading", { name: "音声入力" })).toBeVisible();
		await expect(main.getByText(/worker/i)).toHaveCount(0);
		await expect(main.getByText(/Renderer PID/i)).toHaveCount(0);
		await expect(main.getByText(/EventSub/i)).toHaveCount(0);
		await expect(main.getByText("Session", { exact: true })).toHaveCount(0);
		await expect(main.getByText("最近のイベント")).toHaveCount(0);
	});

	for (const viewport of [
		{ name: "desktop", width: 1366, height: 768 },
		{ name: "mobile", width: 390, height: 844 },
	]) {
		test(`ホームが${viewport.name}で横にはみ出さない`, async ({ page }, testInfo) => {
			await page.setViewportSize({ width: viewport.width, height: viewport.height });
			await page.goto(`/gui/${tokenQuery()}`);
			await expect(page.getByRole("heading", { name: "ホーム" })).toBeVisible({ timeout: 15_000 });
			const metrics = await page.evaluate(() => ({
				viewport: document.documentElement.clientWidth,
				content: document.documentElement.scrollWidth,
			}));
			expect(metrics.content).toBeLessThanOrEqual(metrics.viewport);
			const screenshot = await page.screenshot({ path: testInfo.outputPath(`home-${viewport.name}.png`), fullPage: false });
			expect(screenshot.byteLength).toBeGreaterThan(15_000);
		});
	}

	test("ホームから対応する操作面へ移動できる", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}`);
		const main = page.getByRole("main");

		await main.getByRole("button", { name: "変更する" }).click();
		await expect(page.getByRole("heading", { name: "動作モード" })).toBeVisible({ timeout: 15_000 });

		await page
			.getByRole("navigation", { name: "Main tabs" })
			.getByRole("button", { name: /ホーム/ })
			.click();
		const aiAction = main.getByRole("button", { name: /会話と操作|設定する/ });
		const aiIsConfigured = (await aiAction.textContent())?.trim() === "会話と操作";
		await aiAction.click();
		await expect(page.getByRole("heading", { name: aiIsConfigured ? "ライブ" : "連携", exact: true })).toBeVisible({
			timeout: 15_000,
		});

		await page
			.getByRole("navigation", { name: "Main tabs" })
			.getByRole("button", { name: /ホーム/ })
			.click();
		await main.getByRole("button", { name: "認証を管理" }).click();
		await expect(page.getByRole("heading", { name: "連携", exact: true })).toBeVisible({
			timeout: 15_000,
		});
	});

	test("複数のAIパートナーと入力実体をFlowgraph構成どおり表示する", async ({ page }) => {
		await page.route("**/api/v1/control/snapshot", async (route) => {
			const response = await route.fetch();
			const snapshot = (await response.json()) as Record<string, unknown> & { bridges?: Record<string, unknown> };
			await route.fulfill({
				response,
				json: {
					...snapshot,
					ai_personas: [
						{ index: 0, id: "kaltsit", display_name: "Kalt'sit Pseudo", paused: false, usage: { last_model: "gpt-5.6-luna" } },
						{ index: 1, id: "operator", display_name: "Operator", paused: true, usage: { last_model: null } },
					],
					voice_input: { muted: false, worker_count: 2 },
					bridges: {
						...(snapshot.bridges ?? {}),
						voice_ingresses: [
							{
								node_id: "voice/main::host",
								label: "配信用マイク",
								source: "CABLE Output",
								engine: "whisper",
								active_backend: "vulkan",
								running: true,
							},
							{
								node_id: "voice/main::guest",
								label: "ゲスト音声",
								source: "USB Audio",
								engine: "vosk",
								active_backend: "vosk",
								running: true,
							},
						],
						twitch_irc_ingresses: [
							{
								node_id: "twitch/main::primary",
								label: "配信コメント",
								credential_key: "broadcaster",
								configured_channels: ["usaginetwork"],
								joined_channels: ["usaginetwork"],
								ready: true,
							},
							{
								node_id: "twitch/main::collab",
								label: "コラボ先",
								credential_key: null,
								configured_channels: ["partner_channel"],
								joined_channels: ["partner_channel"],
								ready: true,
							},
						],
					},
				},
			});
		});

		await page.goto(`/gui/${tokenQuery()}`);
		const main = page.getByRole("main");
		await expect(main.getByText("Kalt'sit Pseudo", { exact: true })).toBeVisible();
		await expect(main.getByText("Operator", { exact: true })).toBeVisible();
		await expect(main.getByText("#partner_channel", { exact: false })).toBeVisible();
		await expect(main.getByText("配信用マイク", { exact: true })).toBeVisible();
		await expect(main.getByText("ゲスト音声", { exact: true })).toBeVisible();
		await expect(main.getByRole("switch", { name: "すべての音声入力を一時停止" })).toBeVisible();
	});

	test("ランタイム問題は影響と修正先を示す", async ({ page }) => {
		await page.route("**/api/v1/control/flowgraph/diagnostics", async (route) => {
			const response = await route.fetch();
			const diagnostics = (await response.json()) as Record<string, unknown>;
			await route.fulfill({
				response,
				json: {
					...diagnostics,
					runtime_issues: [
						{
							key: "voice:sample::in",
							severity: "error",
							component: "voice",
							code: "voice_worker_start_failed",
							message: "音声認識ワーカーを開始できない。",
							impact: "この音声入力から Flowgraph へ発話を渡せない。",
							resolution: "Model Pathに`vosk-model-small-ja-0.22`を設定し、Save、Reloadの順に実行する。",
							action: "flowgraph",
							node: "sample::in",
							updated_at: "2026-07-14T00:00:00Z",
						},
					],
				},
			});
		});

		await page.goto(`/gui/${tokenQuery()}`);
		const main = page.getByRole("main");
		await expect(main.getByText("音声認識ワーカーを開始できない。")).toBeVisible();
		await expect(main.getByText("影響: この音声入力から Flowgraph へ発話を渡せない。")).toBeVisible();
		await expect(main.getByText("現在、対応が必要な問題はない。")).toHaveCount(0);
		await page.unrouteAll({ behavior: "wait" });
		await main.getByRole("button", { name: "確認する" }).click();
		await expect(page.getByRole("heading", { name: "Flowgraph Studio" })).toBeVisible();
		await expect(main.getByRole("region", { name: "修正する問題" })).toContainText("音声認識ワーカーを開始できない。");
		await expect(main.getByRole("region", { name: "修正する問題" })).toContainText("vosk-model-small-ja-0.22");
		await expect(main.getByText("対象: sample::in")).toBeVisible();
		await expect(main.getByText("sample", { exact: true }).first()).toBeVisible();
		await expect(main.getByText("ノードが選択されていません。")).toHaveCount(0);
	});
});

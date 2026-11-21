import { expect, test } from "@playwright/test";

import { authHeader, tokenQuery } from "./fixtures";

test.describe("Live: BOS preview", () => {
	test("conversation template merges fresh channels and hides preflight output", async ({ page }) => {
		const now = new Date();
		const fresh = now.toISOString();
		const old = new Date(now.getTime() - 120_000).toISOString();
		await page.route("**/output", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					channel_data: {
						user: [
							{ id: 1, datetime: old, channel: "user", content: "期限切れ", source_actor: null, flags: [] },
							{ id: 2, datetime: fresh, channel: "user", content: "配信者の発話", source_actor: null, flags: ["is_final"] },
						],
						"ai-synth": [
							{
								id: 3,
								datetime: fresh,
								channel: "ai-synth",
								content: "セルフテスト",
								source_actor: "control.preflight:test",
								flags: ["is_final"],
							},
							{
								id: 4,
								datetime: fresh,
								channel: "ai-synth",
								content: "AIの発話",
								source_actor: "Kal'tsit Pseudo",
								flags: ["is_final"],
							},
						],
					},
				}),
			});
		});

		await page.goto("/obs-template/conversation?channels=user%2Cai-synth&poll_ms=100&limit=4&ttl_ms=60000");
		await expect(page.getByText("配信者の発話")).toBeVisible();
		await expect(page.getByText("AIの発話")).toBeVisible();
		await expect(page.getByText("Kal'tsit Pseudo")).toBeVisible();
		await expect(page.getByText("期限切れ")).toHaveCount(0);
		await expect(page.getByText("セルフテスト")).toHaveCount(0);
	});

	test("subtitle template applies speaker and layout controls", async ({ page }) => {
		await page.route("**/output", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					channel_data: {
						user: [
							{
								id: 1,
								datetime: new Date().toISOString(),
								channel: "user",
								content: "字幕レイアウト確認",
								source_actor: "ignored actor",
								flags: ["is_final"],
							},
						],
					},
				}),
			});
		});

		await page.goto("/obs-template/subtitle?channel=user&poll_ms=100&align=right&label=Dr.USAGI&font_scale=115&width_pct=65");

		await expect(page.getByText("字幕レイアウト確認")).toBeVisible();
		await expect(page.getByText("Dr.USAGI")).toBeVisible();
		await expect(page.getByText("ignored actor")).toHaveCount(0);
		await expect(page.locator("#root")).toHaveCSS("justify-items", "end");
		await expect(page.locator(".line")).toHaveCSS("text-align", "right");
		expect(await page.locator("html").evaluate((element) => getComputedStyle(element).getPropertyValue("--font-scale").trim())).toBe(
			"1.15"
		);
		expect(await page.locator("html").evaluate((element) => getComputedStyle(element).getPropertyValue("--content-width").trim())).toBe(
			"65%"
		);
	});

	test("effect template ignores ordinary chat and renders visual flags", async ({ page }) => {
		const now = new Date().toISOString();
		await page.route("**/output", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					channel_data: {
						chat: [
							{ id: 1, datetime: now, channel: "chat", content: "通常コメント", flags: ["is_final"] },
							{
								id: 2,
								datetime: now,
								channel: "chat",
								content: "注目コメント",
								flags: ["is_final", "obs.chat.highlight"],
								quick_effect_label: "チャット強調",
								quick_effect_duration_ms: 700,
							},
						],
					},
				}),
			});
		});

		await page.goto("/obs-template/effect?channels=effect%2Cchat&poll_ms=100&ttl_ms=15000");
		await expect(page.getByText("チャット強調")).toBeVisible();
		await expect(page.getByText("注目コメント")).toBeVisible();
		await expect(page.getByText("通常コメント")).toHaveCount(0);
		await expect(page.locator("#root")).toHaveClass(/effect-chat/);
		await expect(page.getByText("注目コメント")).toBeHidden({ timeout: 2500 });
	});

	test("BOS API exposes built-in OBS templates without document_root", async ({ request }) => {
		const res = await request.get("/api/v1/control/bos", { headers: authHeader() });
		expect(res.status(), await res.text()).toBe(200);
		const body = (await res.json()) as {
			document_root: string | null;
			entries: Array<{
				id: string;
				title: string;
				category: string;
				url: string;
				path: string;
				supports_channel_param: boolean;
				user_defined: boolean;
			}>;
		};

		expect(body.document_root).toBeNull();
		expect(body.entries).toEqual(
			expect.arrayContaining([
				expect.objectContaining({
					id: "obs-template-subtitle",
					category: "subtitles",
					url: "/obs-template/subtitle",
					path: "builtin:/obs-template/subtitle",
					supports_channel_param: true,
					user_defined: false,
				}),
				expect.objectContaining({
					id: "obs-template-conversation",
					category: "subtitles",
					url: "/obs-template/conversation",
					supports_channel_param: true,
				}),
				expect.objectContaining({
					id: "obs-template-chat",
					url: "/obs-template/chat",
					supports_channel_param: true,
				}),
				expect.objectContaining({
					id: "obs-template-raid",
					category: "effects",
					url: "/obs-template/raid",
					supports_channel_param: true,
				}),
				expect.objectContaining({
					id: "obs-template-effect",
					category: "effects",
					url: "/obs-template/effect",
					supports_channel_param: true,
				}),
			])
		);
	});

	test("browser source preview exposes OBS aspect presets", async ({ page }) => {
		let obsSetInputSettings: Record<string, unknown> | null = null;
		let obsCreateInput: Record<string, unknown> | null = null;
		await page.routeWebSocket("ws://127.0.0.1:4455", (socket) => {
			socket.send(JSON.stringify({ op: 0, d: { rpcVersion: 1 } }));
			socket.onMessage((message) => {
				const request = JSON.parse(String(message)) as {
					op: number;
					d: { requestType?: string; requestId?: string; requestData?: Record<string, unknown> };
				};
				if (request.op === 1) {
					socket.send(JSON.stringify({ op: 2, d: { negotiatedRpcVersion: 1 } }));
					return;
				}
				if (request.op !== 6 || !request.d.requestId || !request.d.requestType) return;
				let responseData: Record<string, unknown> = {};
				if (request.d.requestType === "GetInputList") {
					responseData = {
						inputs: [
							{ inputName: "subtitle-usagi", inputKind: "browser_source", unversionedInputKind: "browser_source" },
							{ inputName: "mic", inputKind: "wasapi_input_capture", unversionedInputKind: "wasapi_input_capture" },
						],
					};
				} else if (request.d.requestType === "GetSceneList") {
					responseData = {
						currentProgramSceneName: "Game",
						scenes: [
							{ sceneName: "Game", sceneIndex: 0 },
							{ sceneName: "Overlays", sceneIndex: 1 },
						],
					};
				} else if (request.d.requestType === "GetVideoSettings") {
					responseData = { baseWidth: 3840, baseHeight: 2160 };
				} else if (request.d.requestType === "GetInputSettings") {
					responseData = { inputKind: "browser_source", inputSettings: { url: "https://old.invalid" } };
				} else if (request.d.requestType === "SetInputSettings") {
					obsSetInputSettings = request.d.requestData ?? null;
				} else if (request.d.requestType === "CreateInput") {
					obsCreateInput = request.d.requestData ?? null;
				}
				socket.send(
					JSON.stringify({
						op: 7,
						d: {
							requestType: request.d.requestType,
							requestId: request.d.requestId,
							requestStatus: { result: true, code: 100 },
							responseData,
						},
					})
				);
			});
		});
		await page.route("**/api/v1/control/bos", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					document_root: "C:\\vac\\browser-output",
					url_prefix: "/browser-output",
					entries: [
						{
							id: "subtitles-vertical",
							title: "Subtitles Vertical",
							category: "subtitles",
							url: "/browser-output/subtitles-vertical/",
							path: "C:\\vac\\browser-output\\subtitles-vertical",
							supports_channel_param: true,
							user_defined: false,
						},
						{
							id: "obs-template-subtitle",
							title: "OBS Template Subtitle",
							category: "subtitles",
							url: "/obs-template/subtitle",
							path: "builtin:/obs-template/subtitle",
							supports_channel_param: true,
							user_defined: false,
						},
						{
							id: "obs-template-chat",
							title: "OBS Template Chat",
							category: "other",
							url: "/obs-template/chat",
							path: "builtin:/obs-template/chat",
							supports_channel_param: true,
							user_defined: false,
						},
						{
							id: "obs-template-raid",
							title: "OBS Template Raid",
							category: "effects",
							url: "/obs-template/raid",
							path: "builtin:/obs-template/raid",
							supports_channel_param: true,
							user_defined: false,
						},
						{
							id: "obs-template-effect",
							title: "OBS Template Effects",
							category: "effects",
							url: "/obs-template/effect",
							path: "builtin:/obs-template/effect",
							supports_channel_param: true,
							user_defined: false,
						},
					],
				}),
			});
		});
		let quickEffectRequest: unknown = null;
		let quickEffectMoveRequest: unknown = null;
		let quickEffectPresetsPutRequest: unknown = null;
		await page.route("**/api/v1/control/quick_effects/presets", async (route) => {
			if (route.request().method() === "GET") {
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify({
						presets: [
							{
								id: "raid",
								label: "RAID",
								channel: "effect",
								content: "RAID incoming",
								flags: ["obs.raid"],
								duration_ms: 8000,
							},
							{
								id: "scene_flash",
								label: "画面効果",
								channel: "effect",
								content: "scene flash",
								flags: ["obs.effect.flash"],
								duration_ms: 2500,
							},
						],
						source_path: "C:\\vac\\conf.toml",
					}),
				});
				return;
			}
			quickEffectPresetsPutRequest = route.request().postDataJSON();
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					presets: route.request().postDataJSON().presets,
					source_path: "C:\\vac\\conf.toml",
					backup: "conf.toml.bak-20260531000000",
					warning: "restart required",
				}),
			});
		});
		await page.route("**/api/v1/control/quick_effects?*", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					effects: [
						{
							id: "qe-existing",
							label: "scheduled",
							channel: "effect",
							content: "soon",
							flags: ["obs.effect.flash"],
							created_at: "2026-05-31T00:00:00Z",
							created_at_ms: 1770000000000,
							fire_at_ms: 1770000005000,
							fired_at_ms: null,
							duration_ms: 2500,
							expires_at_ms: 1770000007500,
							status: "scheduled",
							datum_id: 41,
							meta: { preset: "scene_flash" },
						},
					],
					limit: 12,
					capacity: 128,
				}),
			});
		});
		await page.route("**/api/v1/control/quick_effects/*/move", async (route) => {
			quickEffectMoveRequest = route.request().postDataJSON();
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					effects: [
						{
							id: "qe-existing",
							label: "scheduled",
							channel: "effect",
							content: "soon",
							flags: ["obs.effect.flash"],
							created_at: "2026-05-31T00:00:00Z",
							created_at_ms: 1770000000000,
							fire_at_ms: 1770000005000,
							fired_at_ms: null,
							duration_ms: 2500,
							expires_at_ms: 1770000007500,
							status: "scheduled",
							datum_id: 41,
							meta: { preset: "scene_flash" },
						},
					],
					limit: 1,
					capacity: 128,
				}),
			});
		});
		await page.route("**/api/v1/control/quick_effects", async (route) => {
			quickEffectRequest = route.request().postDataJSON();
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					id: "qe-42",
					label: "RAID",
					channel: "effect",
					content: "RAID incoming",
					flags: ["obs.raid"],
					created_at: "2026-05-31T00:00:00Z",
					created_at_ms: 1770000000000,
					fire_at_ms: null,
					fired_at_ms: 1770000000000,
					duration_ms: 8000,
					expires_at_ms: 1770000008000,
					status: "active",
					datum_id: 42,
					meta: { preset: "raid" },
				}),
			});
		});

		await page.goto(`/gui/${tokenQuery()}#live`);

		await expect(page.getByRole("heading", { name: /BOS \/ OBS 出力プレビュー/ })).toBeVisible({
			timeout: 15_000,
		});
		await page.locator("summary").filter({ hasText: "出力チャンネル管理" }).click();
		await expect(page.getByRole("heading", { name: "OBS 出力管理" })).toBeVisible();
		const categoryFilter = page.getByRole("group", { name: "BOS category filter" });
		await expect(categoryFilter.getByRole("button", { name: "全部 5" })).toHaveAttribute("aria-pressed", "true");
		await expect(page.getByRole("heading", { name: "OBS Template Subtitle" })).toBeVisible();
		await categoryFilter.getByRole("button", { name: "字幕 2" }).click();
		await expect(categoryFilter.getByRole("button", { name: "字幕 2" })).toHaveAttribute("aria-pressed", "true");
		await expect(page.getByRole("link", { name: "開く" }).first()).toHaveAttribute("href", /channel=user%2Cai-synth$/);
		await expect(page.getByLabel("Browser source URL")).toHaveValue(/\/browser-output\/subtitles-vertical\/\?channel=user%2Cai-synth$/);
		await expect(page.getByText("OBS 1920 x 1080")).toBeVisible();

		const group = page.getByRole("group", { name: "Preview aspect ratio" });
		await expect(group.getByRole("button", { name: "16:9" })).toHaveAttribute("aria-pressed", "true");

		await group.getByRole("button", { name: "9:16" }).click();
		await expect(group.getByRole("button", { name: "9:16" })).toHaveAttribute("aria-pressed", "true");
		await expect(page.getByText("OBS 1080 x 1920")).toBeVisible();
		await expect(page.getByText("subtitles-vertical", { exact: true }).first()).toBeVisible();

		await page.getByLabel(/ソース/).selectOption("obs-template-subtitle");
		await expect(page.getByLabel("Browser source URL")).toHaveValue(
			/\/obs-template\/subtitle\?channels=user%2Cai-synth&theme=default&poll_ms=500&limit=20&ttl_ms=30000&align=center&font_scale=100&width_pct=100$/
		);
		const quickSettings = page.getByRole("group", { name: "OBS template quick settings" });
		await expect(quickSettings.getByRole("button", { name: "字幕" })).toBeVisible();
		await quickSettings.getByRole("button", { name: "最小" }).click();
		await expect(page.getByLabel("Browser source URL")).toHaveValue(
			/\/obs-template\/subtitle\?channel=user&theme=minimal&poll_ms=500&limit=20&ttl_ms=30000&align=center&font_scale=100&width_pct=80$/
		);
		await quickSettings.getByRole("button", { name: "チャット" }).click();
		await expect(page.getByLabel("Browser source URL")).toHaveValue(
			/\/obs-template\/chat\?channel=chat&theme=minimal&poll_ms=300&limit=12&ttl_ms=60000&align=left&font_scale=80&width_pct=72$/
		);
		await expect(page.getByText("OBS 1080 x 1920")).toBeVisible();
		await quickSettings.getByRole("button", { name: "RAID" }).click();
		await expect(page.getByLabel("Browser source URL")).toHaveValue(
			/\/obs-template\/raid\?channel=effect&theme=default&poll_ms=250&limit=1&ttl_ms=15000&align=center&font_scale=120&width_pct=90$/
		);
		await expect(page.getByText("OBS 1920 x 1080")).toBeVisible();
		await quickSettings.getByRole("button", { name: "演出" }).click();
		await expect(page.getByLabel("Browser source URL")).toHaveValue(
			/\/obs-template\/effect\?channels=effect%2Cchat&theme=default&poll_ms=100&limit=20&ttl_ms=15000&align=center&font_scale=100&width_pct=100$/
		);
		await quickSettings.getByRole("button", { name: "字幕" }).click();
		await page.getByLabel("外観").selectOption("minimal");
		await page.getByLabel("更新間隔 (ms)").fill("1000");
		await page.getByLabel("表示件数").fill("4");
		await page.getByLabel("表示時間 (ms)").fill("45000");
		await page.getByRole("group", { name: "字幕の配置" }).getByRole("button", { name: "右" }).click();
		await page.getByLabel("話者名").fill("Kal'tsit Pseudo");
		await page.getByLabel("文字倍率 (%)").fill("115");
		await page.getByLabel("最大幅 (%)").fill("65");
		await expect(page.getByLabel("Browser source URL")).toHaveValue(
			/\/obs-template\/subtitle\?channel=user&theme=minimal&poll_ms=1000&limit=4&ttl_ms=45000&align=right&label=Kal%27tsit\+Pseudo&font_scale=115&width_pct=65$/
		);
		await page.getByRole("button", { name: "OBSから取得" }).click();
		await expect(page.getByLabel("OBS Browser Source")).toHaveValue("subtitle-usagi");
		await page.getByRole("button", { name: "OBSへ反映" }).click();
		await expect.poll(() => obsSetInputSettings).not.toBeNull();
		expect(obsSetInputSettings).toEqual({
			inputName: "subtitle-usagi",
			inputSettings: {
				url: expect.stringMatching(
					/\/obs-template\/subtitle\?channel=user&theme=minimal&poll_ms=1000&limit=4&ttl_ms=45000&align=right&label=Kal%27tsit\+Pseudo&font_scale=115&width_pct=65$/
				),
			},
			overlay: true,
		});
		await quickSettings.getByRole("button", { name: "演出" }).click();
		await page.getByLabel("OBSシーン").selectOption("Overlays");
		await page.getByLabel("新規OBSソース名").fill("unvac-effects");
		await page.getByRole("button", { name: "新規作成 (3840 x 2160)" }).click();
		await expect.poll(() => obsCreateInput).not.toBeNull();
		expect(obsCreateInput).toEqual({
			sceneName: "Overlays",
			inputName: "unvac-effects",
			inputKind: "browser_source",
			inputSettings: {
				url: expect.stringMatching(
					/\/obs-template\/effect\?channels=effect%2Cchat&theme=default&poll_ms=100&limit=20&ttl_ms=15000&align=center&font_scale=100&width_pct=100$/
				),
				width: 3840,
				height: 2160,
			},
			sceneItemEnabled: true,
		});
		await expect(page.getByLabel("OBS Browser Source")).toHaveValue("unvac-effects");

		await expect(page.getByRole("heading", { name: "BGM / 画面効果キュー" })).toBeVisible();
		const quickEffectsPanel = page.getByRole("region", { name: "Quick effects panel" });
		await expect(page.getByText("scheduled").first()).toBeVisible();
		await page.getByTitle("move down").first().click();
		expect(quickEffectMoveRequest).toEqual({ direction: "down" });
		await quickEffectsPanel.locator("summary").filter({ hasText: "カスタム演出" }).click();
		await quickEffectsPanel.getByText("delay_ms").locator("..").getByRole("spinbutton").fill("1500");
		await quickEffectsPanel.getByRole("group", { name: "Quick effect presets" }).getByRole("button", { name: "RAID" }).click();
		expect(quickEffectRequest).toEqual(
			expect.objectContaining({
				label: "RAID",
				channel: "effect",
				content: "RAID incoming",
				flags: ["obs.raid"],
				duration_ms: 8000,
				fire_at_ms: expect.any(Number),
			})
		);
		await quickEffectsPanel.getByText("label").locator("..").getByRole("textbox").fill("Stinger");
		await quickEffectsPanel.getByText("content").locator("..").getByRole("textbox").fill("sting");
		await quickEffectsPanel.getByText("flags").locator("..").getByRole("textbox").fill("obs.stinger, audio.hit");
		await quickEffectsPanel.getByRole("button", { name: "Preset 保存" }).click();
		await expect.poll(() => quickEffectPresetsPutRequest).not.toBeNull();
		expect(quickEffectPresetsPutRequest).toEqual({
			presets: expect.arrayContaining([
				expect.objectContaining({
					id: "stinger",
					label: "Stinger",
					channel: "effect",
					content: "sting",
					flags: ["obs.stinger", "audio.hit"],
					duration_ms: 5000,
				}),
			]),
		});
		await expect(page.getByText("active")).toBeVisible();
	});
});

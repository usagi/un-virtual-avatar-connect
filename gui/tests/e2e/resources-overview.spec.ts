import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

test.describe("GUI redesign: リソース概要", () => {
	test("Twitch EventSubの認可不足を接続操作の前に示す", async ({ page }) => {
		await page.route("**/api/v1/control/flowgraph/diagnostics", (route) =>
			route.fulfill({
				json: {
					runtime_issues: [
						{
							key: "twitch_eventsub_auth",
							severity: "warning",
							message: "Twitch EventSubを利用するには下のOAuth認可を完了する必要がある。",
							impact: "",
							action: "resources",
						},
					],
				},
			})
		);
		await page.route("**/api/v1/control/snapshot", async (route) => {
			await route.fulfill({
				json: {
					schema: 3,
					app_version: "e2e",
					now: "2026-07-14T00:00:00Z",
					runtime: { session_id: "e2e", root: "C:\\vac", session_dir: "C:\\vac\\sessions\\e2e", inline_max_bytes: 4096 },
					ai_personas: [],
					twitch: {
						username: "doctor",
						channel_to: "doctor",
						reads: null,
						eventsub_enabled: true,
						moderator_enabled: false,
						moderator_login: null,
						ignore_logins: [],
						broadcaster_authorized: false,
						moderator_authorized: null,
					},
				},
			});
		});

		await page.goto(`/gui/${tokenQuery()}#resources`);
		const notice = page.getByRole("region", { name: "連携の確認事項" });
		await expect(notice.getByRole("heading", { name: "接続に確認が必要" })).toBeVisible();
		await expect(notice).toContainText("Twitch EventSubを利用するには下のOAuth認可を完了する必要がある。");
	});

	test("AIパートナーの詳細設定と連携診断へ到達できる", async ({ page }) => {
		let pauseRequest: unknown = null;
		await page.route("**/api/v1/control/snapshot", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					schema: 1,
					app_version: "e2e",
					now: "2026-05-31T00:00:00Z",
					runtime: {
						session_id: "e2e",
						root: "C:\\vac",
						session_dir: "C:\\vac\\sessions\\e2e",
						inline_max_bytes: 4096,
					},
					processors: [],
					ai_personas: [
						{
							index: 0,
							id: "kaltsit",
							paused: false,
							usage: {
								completed_reactions: 0,
								failed_reactions: 0,
								api_requests: 0,
								tool_calls: 0,
								input_tokens: 0,
								cached_tokens: 0,
								cache_write_tokens: 0,
								output_tokens: 0,
								reasoning_tokens: 0,
								last_model: null,
								last_latency_ms: null,
								last_updated_at: null,
							},
						},
						{
							index: 1,
							id: "operator",
							paused: true,
							usage: {
								completed_reactions: 0,
								failed_reactions: 0,
								api_requests: 0,
								tool_calls: 0,
								input_tokens: 0,
								cached_tokens: 0,
								cache_write_tokens: 0,
								output_tokens: 0,
								reasoning_tokens: 0,
								last_model: null,
								last_latency_ms: null,
								last_updated_at: null,
							},
						},
					],
					twitch: null,
				}),
			});
		});
		await page.route("**/api/v1/control/pause", async (route) => {
			pauseRequest = route.request().postDataJSON();
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({ processors_affected: [], ais_affected: [0], paused: true }),
			});
		});
		let reloadRequest: unknown = null;
		let aiConfigPutRequest: unknown = null;
		await page.route("**/api/v1/control/reload", async (route) => {
			reloadRequest = route.request().postDataJSON();
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					target: "ai_persona",
					ai_report: {
						custom_instructions_changed: false,
						system_instructions_extra_changed: false,
						heartbeat_enabled_changed: true,
						decision_threshold_changed: true,
						rebuilt_request_template: true,
						rebuilt_decision_spec: true,
						warnings: [],
					},
				}),
			});
		});
		await page.route("**/api/v1/control/ai_personas/0/config", async (route) => {
			if (route.request().method() === "GET") {
				await route.fulfill({
					status: 200,
					contentType: "application/json",
					body: JSON.stringify({
						index: 0,
						id: "kaltsit",
						source_path: "C:\\vac\\conf.toml",
						revision: "fixture-ai-revision",
						config: {
							id: "kaltsit",
							custom_instructions: "doctor",
							heartbeat: { enabled: false, interval_secs: 60 },
							decision: { threshold: 0.5, base_score: 0, jitter: 0, modulators: [] },
						},
					}),
				});
				return;
			}
			aiConfigPutRequest = route.request().postDataJSON();
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					index: 0,
					source_path: "C:\\vac\\conf.toml",
					backup: "conf.toml.bak-20260531000000",
					warning: "restart required",
				}),
			});
		});

		await page.goto(`/gui/${tokenQuery()}#ai`);

		await expect(page.getByRole("heading", { name: "AIパートナー", exact: true })).toBeVisible({
			timeout: 15_000,
		});
		await page.locator("summary").filter({ hasText: "詳細設定と稼働状態" }).click();
		const aiPanel = page.getByRole("region", { name: "AI management panel" });
		await expect(aiPanel.getByRole("heading", { name: "AI 管理" })).toBeVisible();
		await expect(aiPanel.getByText("1 active")).toBeVisible();
		await expect(aiPanel.getByText("1 paused")).toBeVisible();
		await expect(aiPanel.getByRole("heading", { name: "kaltsit" })).toBeVisible();
		const kaltsitRow = aiPanel.getByRole("article", { name: "AI persona kaltsit" });
		await kaltsitRow.getByRole("button", { name: "詳細" }).click();
		await kaltsitRow.getByRole("radio", { name: "ON" }).check();
		await kaltsitRow.getByRole("checkbox", { name: "decision_threshold" }).check();
		await kaltsitRow.getByRole("spinbutton").fill("0.72");
		await kaltsitRow.getByRole("button", { name: "現在の動作に適用" }).click();
		expect(reloadRequest).toEqual(
			expect.objectContaining({
				target: "ai_persona",
				id: "kaltsit",
				heartbeat_enabled: true,
				decision_threshold: 0.72,
			})
		);
		await kaltsitRow.getByRole("button", { name: "次回起動用に保存" }).click();
		await expect.poll(() => aiConfigPutRequest).not.toBeNull();
		await expect(kaltsitRow.getByText("restart required", { exact: true })).toBeVisible();
		expect(aiConfigPutRequest).toEqual({
			expected_revision: "fixture-ai-revision",
			config: expect.objectContaining({
				id: "kaltsit",
				custom_instructions: "doctor",
				heartbeat: expect.objectContaining({ enabled: true, interval_secs: 60 }),
				decision: expect.objectContaining({ threshold: 0.72 }),
			}),
		});
		await kaltsitRow.getByRole("button", { name: "Pause" }).click();
		expect(pauseRequest).toEqual({ target: "ai", index: 0 });

		await page.goto(`/gui/${tokenQuery()}#resources`);
		await expect(page.getByRole("heading", { name: "連携", exact: true })).toBeVisible();
		await expect(page.getByText("現在の run_with 登録から Managed App の状態を確認します。")).not.toBeVisible();
		await page.locator("summary").filter({ hasText: "接続診断" }).click();
		await expect(page.getByText("現在の run_with 登録から Managed App の状態を確認します。")).toBeVisible();
		await expect(page.getByText("Apps", { exact: true })).toBeVisible();
		await expect(page.getByText("起動中", { exact: true })).toBeVisible();
		await expect(page.getByText("追跡対象", { exact: true })).toBeVisible();
		await expect(page.getByText("PIDs", { exact: true })).toBeVisible();
		await expect(page.getByRole("heading", { name: "VMC passthrough" })).toBeVisible();
		await expect(page.getByText("motion 層の UDP 転送状態と packet 統計を確認します。")).toBeVisible();
		await expect(page.getByText("Routes", { exact: true })).toBeVisible();
		await expect(page.getByText("Packets", { exact: true })).toBeVisible();
		await expect(page.getByRole("button", { name: "route 追加" })).toBeVisible();
	});
});

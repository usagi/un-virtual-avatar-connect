import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

const emptySnapshot = {
	schema: 1,
	app_version: "e2e",
	now: "2026-07-14T00:00:00Z",
	runtime: { session_id: "e2e", root: "C:\\vac", session_dir: "C:\\vac\\sessions\\e2e", inline_max_bytes: 4096 },
	processors: [],
	ai_personas: [],
	twitch: null,
};

test("managed profileへAI設定を保存して再起動する", async ({ page }, testInfo) => {
	let setupRequest: unknown = null;
	let restartRequest: unknown = null;
	await page.route("**/api/v1/control/desktop/profiles", async (route) => {
		await route.fulfill({
			json: {
				root: "C:\\Users\\test\\AppData\\Roaming\\UN Virtual Avatar Connect",
				managed_mode: true,
				onboarding_completed: true,
				active_profile_id: "streaming",
				profiles: [
					{
						id: "streaming",
						label: "配信AI",
						conf_path: "C:\\Users\\test\\AppData\\Roaming\\UN Virtual Avatar Connect\\profiles\\streaming\\conf.toml",
						is_active: true,
						template_id: "streaming-companion",
					},
				],
			},
		});
	});
	await page.route("**/api/v1/control/desktop/active-profile/ai", async (route) => {
		if (route.request().method() === "GET") {
			await route.fulfill({
				json: {
					configured: false,
					enabled: false,
					persona_id: null,
					display_name: null,
					model: null,
					custom_instructions: null,
					observe_triggers: [],
					avatar_actions: false,
					avatar_link_id: null,
					avatar_base_key: null,
					avatar_publish: false,
					credential_source: "missing",
					template_id: "streaming-companion",
				},
			});
			return;
		}
		setupRequest = route.request().postDataJSON();
		await route.fulfill({
			json: {
				status: {
					configured: true,
					enabled: true,
					persona_id: "companion",
					display_name: "Kalt'sit Pseudo",
					model: "gpt-5.6-luna",
					custom_instructions: "配信を支援する。",
					observe_triggers: ["user", "twitch.chat"],
					avatar_actions: true,
					avatar_link_id: "companion_avatar",
					avatar_base_key: "un-motion/frame/companion-avatar",
					avatar_publish: true,
					credential_source: "profile_file",
					template_id: "streaming-companion",
				},
				restart_required: true,
			},
		});
	});
	await page.route("**/api/v1/control/snapshot", async (route) => route.fulfill({ json: emptySnapshot }));
	await page.route("**/api/v1/control/restart", async (route) => {
		restartRequest = route.request().postDataJSON();
		await route.fulfill({ json: { new_pid: 4321 } });
	});

	await page.goto(`/gui/${tokenQuery()}#ai`);
	const panel = page.getByRole("region", { name: "AIセットアップ" });
	await expect(panel.getByRole("heading", { name: "AIアシスタント" })).toBeVisible();
	await panel.getByLabel("表示名").fill("Kalt'sit Pseudo");
	await panel.getByLabel("OpenAI API key").fill("sk-test-secret");
	await panel.getByLabel("Twitchコメント").check();
	await panel.getByLabel("AvatarLink ID").fill("companion_avatar");
	await panel.getByLabel("UNMF/Z base key").fill("un-motion/frame/companion-avatar");
	await panel.getByLabel("接続先へ実際に出力する").check();
	await panel.getByRole("button", { name: "保存して再起動" }).click();

	await expect.poll(() => setupRequest).not.toBeNull();
	expect(setupRequest).toEqual(
		expect.objectContaining({
			persona_id: "companion",
			display_name: "Kalt'sit Pseudo",
			model: "gpt-5.6-luna",
			api_key: "sk-test-secret",
			observe_triggers: ["user", "twitch.chat"],
			avatar_actions: true,
			avatar_link_id: "companion_avatar",
			avatar_base_key: "un-motion/frame/companion-avatar",
			avatar_publish: true,
		})
	);
	await expect.poll(() => restartRequest).toEqual({ graceful_ms: 800 });
	const screenshot = await page.screenshot({ fullPage: true, path: testInfo.outputPath("desktop-ai-setup.png") });
	expect(screenshot.byteLength).toBeGreaterThan(15_000);
});

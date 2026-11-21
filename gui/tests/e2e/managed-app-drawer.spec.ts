import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

test.describe("GUI shell: 連携アプリドロワー", () => {
	test("shell action からドロワーを開ける", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}`);

		await page.getByRole("button", { name: "連携アプリ…" }).click();

		const drawer = page.getByRole("complementary", { name: "Managed app drawer" });
		await expect(drawer.getByRole("heading", { name: "連携アプリ" })).toBeVisible({
			timeout: 15_000,
		});
		await expect(drawer.getByText("run_with で管理するアプリの監視と操作")).toBeVisible();
		await expect(drawer.getByRole("button", { name: "再読込" })).toBeVisible();
		await expect(drawer.getByRole("button", { name: "閉じる" })).toBeVisible();
	});

	test("UNAA/Z 詳細を折りたたみで確認できる", async ({ page }) => {
		await page.route("**/api/v1/control/managed_apps", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				json: {
					entries: [
						{
							id: "renderer",
							label: "UN Avatar Renderer",
							command: "un-avatar-renderer.exe --profile main",
							process_marker: "un-avatar-renderer.exe",
							minimized: false,
							run_as_admin: false,
							working_dir: "C:\\UN\\renderer",
							supports_status: true,
							unaa_z_app_id: "un-avatar",
							unaa_z_instance_id: "renderer-main",
							status: {
								id: "renderer",
								running: true,
								pids: [1234],
								checked_at: "2026-05-31T00:00:00Z",
							},
							unaa_z_status: {
								app_id: "un-avatar",
								instance_id: "renderer-main",
								discovery_source: "managed_app_registry+zenoh",
								discovery_source_key: "un/app/un-avatar/renderer-main/snapshot",
								discovery_payload_kind: "query",
								discovery_observed_at: "2026-05-31T00:00:01Z",
								discovery_reported_at: "2026-05-31T00:00:00Z",
								discovery_age_ms: 1200,
								discovery_stale: false,
								zenoh_seen: true,
								running: true,
								ready: true,
								capability_count: 2,
								capabilities: ["avatar.render", "preview.reload"],
								command_count: 2,
								commands: ["focus", "reload.preview"],
								allowed_command_count: 2,
								allowed_commands: ["focus", "reload.preview"],
								endpoint_count: 1,
							},
						},
					],
					unaa_z_source: "managed_app_registry+zenoh",
					unaa_z_discovery_errors: [],
				},
			});
		});
		await page.route("**/api/v1/control/un_app_api/commands/history?*", async (route) => {
			await route.fulfill({
				status: 200,
				contentType: "application/json",
				json: {
					commands: [
						{
							at: new Date().toISOString(),
							app_id: "un-avatar",
							instance_id: "renderer-main",
							command: "focus",
							request_id: "req-1",
							key_expr: "un/app/un-avatar/renderer-main/command",
							response_key_expr: "un/app/un-avatar/renderer-main/response/req-1",
							event_key_expr: "un/app/un-avatar/renderer-main/event",
							status: "response_observed",
							response_observed: true,
							response: {
								key_expr: "un/app/un-avatar/renderer-main/response/req-1",
								source: "response",
								payload: { ok: true },
							},
							error: null,
						},
					],
				},
			});
		});

		await page.goto(`/gui/${tokenQuery()}`);
		await page.getByRole("button", { name: "連携アプリ…" }).click();

		const drawer = page.getByRole("complementary", { name: "Managed app drawer" });
		await expect(drawer.getByText("UN Avatar Renderer")).toBeVisible({ timeout: 15_000 });
		await drawer.getByRole("button", { name: "詳細" }).click();

		const details = page.getByTestId("managed-app-details-renderer");
		await expect(details).toContainText("C:\\UN\\renderer");
		await expect(details).toContainText("un-avatar/renderer-main");
		await expect(details).toContainText("un/app/un-avatar/renderer-main/snapshot");
		await expect(details).toContainText("avatar.render, preview.reload");
		await expect(details).toContainText("focus, reload.preview");
		await expect(details).toContainText("focus / response_observed / req-1");
	});
});

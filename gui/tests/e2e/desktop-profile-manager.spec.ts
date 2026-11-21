import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

const templates = [
	{ id: "streaming-companion", label: "配信AI", description: "配信向け", recommended: true },
	{ id: "desktop-assistant", label: "デスクトップAI", description: "デスクトップ向け", recommended: false },
];

test("Settingsでprofileを作成し、選択したprofileへ再起動する", async ({ page }, testInfo) => {
	let createdBody: unknown = null;
	let activatedId: string | null = null;
	let restartBody: unknown = null;
	const profiles = [
		{
			id: "streaming",
			label: "配信AI",
			conf_path: "C:\\vac\\profiles\\streaming\\conf.toml",
			is_active: true,
			template_id: "streaming-companion",
		},
		{
			id: "desktop",
			label: "デスクトップ",
			conf_path: "C:\\vac\\profiles\\desktop\\conf.toml",
			is_active: false,
			template_id: "desktop-assistant",
		},
	];

	await page.route("**/api/v1/control/desktop/profiles", async (route) => {
		if (route.request().method() === "POST") {
			createdBody = route.request().postDataJSON();
			const created = {
				id: "desk-work",
				label: "Desk Work",
				conf_path: "C:\\vac\\profiles\\desk-work\\conf.toml",
				is_active: false,
				template_id: "desktop-assistant",
			};
			profiles.push(created);
			await route.fulfill({ status: 201, json: { profile: created } });
			return;
		}
		await route.fulfill({
			json: {
				root: "C:\\vac",
				managed_mode: true,
				onboarding_completed: true,
				active_profile_id: "streaming",
				profiles,
			},
		});
	});
	await page.route("**/api/v1/control/desktop/profile-templates", (route) => route.fulfill({ json: templates }));
	await page.route("**/api/v1/control/desktop/profiles/*/activate", async (route) => {
		activatedId = new URL(route.request().url()).pathname.split("/").at(-2) ?? null;
		await route.fulfill({ json: { profile: profiles.find((profile) => profile.id === activatedId) } });
	});
	await page.route("**/api/v1/control/restart", async (route) => {
		restartBody = route.request().postDataJSON();
		await route.fulfill({ json: { new_pid: 4321 } });
	});

	await page.goto(`/gui/${tokenQuery()}#settings`);
	await expect(page.getByRole("heading", { name: "設定", exact: true })).toBeVisible();
	await expect(page.getByRole("heading", { name: "プロファイル", exact: true })).toBeVisible();
	await page.getByLabel("用途").selectOption("desktop-assistant");
	await page.getByLabel("プロファイル名").fill("Desk Work");
	await page.getByRole("button", { name: "作成", exact: true }).click();
	await expect.poll(() => createdBody).toEqual({ id: "desk-work", label: "Desk Work", template_id: "desktop-assistant" });
	await expect(page.getByRole("main").getByText("Desk Work", { exact: true })).toBeVisible();

	await page.getByRole("button", { name: "デスクトップへ切替" }).click();
	await expect.poll(() => activatedId).toBe("desktop");
	await expect.poll(() => restartBody).toEqual({ graceful_ms: 800 });

	const screenshot = await page.screenshot({ fullPage: true, path: testInfo.outputPath("desktop-profile-settings.png") });
	expect(screenshot.byteLength).toBeGreaterThan(15_000);
});

test("Settingsから関連ファイルを含むprofile bundleを取り込む", async ({ page }) => {
	let importUrl = "";
	let importBytes = 0;
	await page.route("**/api/v1/control/desktop/profiles", (route) =>
		route.fulfill({
			json: {
				root: "C:\\vac",
				managed_mode: true,
				onboarding_completed: true,
				active_profile_id: "streaming",
				profiles: [
					{
						id: "streaming",
						label: "配信AI",
						conf_path: "C:\\vac\\profiles\\streaming\\conf.toml",
						is_active: true,
						template_id: "streaming-companion",
					},
				],
			},
		})
	);
	await page.route("**/api/v1/control/desktop/profile-templates", (route) => route.fulfill({ json: templates }));
	await page.route("**/api/v1/control/desktop/profiles/import-bundle?*", async (route) => {
		importUrl = route.request().url();
		importBytes = route.request().postDataBuffer()?.byteLength ?? 0;
		await route.fulfill({
			status: 201,
			json: {
				profile: {
					id: "my-bundle",
					label: "My Bundle",
					conf_path: "C:\\vac\\profiles\\my-bundle\\conf.toml",
					is_active: false,
					template_id: null,
				},
			},
		});
	});

	await page.goto(`/gui/${tokenQuery()}#settings`);
	await page.getByLabel("プロファイルファイル").setInputFiles({
		name: "My Bundle.zip",
		mimeType: "application/zip",
		buffer: Buffer.from("PK bundle fixture"),
	});
	await expect(page.getByText("My Bundle.zip", { exact: true })).toBeVisible();
	await page.getByRole("button", { name: "取り込む", exact: true }).click();
	await expect.poll(() => importBytes).toBeGreaterThan(0);
	const url = new URL(importUrl);
	expect(url.searchParams.get("id")).toBe("my-bundle");
	expect(url.searchParams.get("label")).toBe("My Bundle");
});

test("Settingsから秘密情報を除いたprofile bundleを書き出す", async ({ page }) => {
	let exportedId: string | null = null;
	await page.route("**/api/v1/control/desktop/profiles", (route) =>
		route.fulfill({
			json: {
				root: "C:\\vac",
				managed_mode: true,
				onboarding_completed: true,
				active_profile_id: "streaming",
				profiles: [
					{
						id: "streaming",
						label: "配信AI",
						conf_path: "C:\\vac\\profiles\\streaming\\conf.toml",
						is_active: true,
						template_id: "streaming-companion",
					},
				],
			},
		})
	);
	await page.route("**/api/v1/control/desktop/profile-templates", (route) => route.fulfill({ json: templates }));
	await page.route("**/api/v1/control/desktop/profiles/*/export-bundle", async (route) => {
		exportedId = new URL(route.request().url()).pathname.split("/").at(-2) ?? null;
		await route.fulfill({
			status: 200,
			contentType: "application/zip",
			body: Buffer.from("PK exported bundle"),
		});
	});

	await page.goto(`/gui/${tokenQuery()}#settings`);
	const downloadPromise = page.waitForEvent("download");
	await page.getByRole("button", { name: "配信AIを書き出す" }).click();
	const download = await downloadPromise;
	await expect.poll(() => exportedId).toBe("streaming");
	expect(download.suggestedFilename()).toBe("streaming.unvac-profile.zip");
});

import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

test.describe("GUI redesign: main navigation", () => {
	test("new top-level surfaces are reachable from the main navigation", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}`);

		const tabs = page.getByRole("navigation", { name: "Main tabs" });
		await expect(tabs.getByRole("button", { name: "ホーム" })).toBeVisible({
			timeout: 15_000,
		});

		await tabs.getByRole("button", { name: "動作モード" }).click();
		await expect(page.getByRole("heading", { name: "動作モード" })).toBeVisible();
		await expect(page.getByText("配信や作業に合わせて、AIと連携アプリの動作をまとめて切り替える。")).toBeVisible();

		await tabs.getByRole("button", { name: "AIパートナー" }).click();
		await expect(page.getByRole("heading", { name: "AIパートナー" })).toBeVisible({ timeout: 15_000 });

		await tabs.getByRole("button", { name: "Flowgraph Studio" }).click();
		await expect(page.getByRole("heading", { name: "Flowgraph Studio" })).toBeVisible({ timeout: 15_000 });

		await tabs.getByRole("button", { name: "辞書" }).click();
		await expect(page.getByRole("heading", { name: "辞書", exact: true })).toBeVisible({ timeout: 15_000 });

		await tabs.getByRole("button", { name: "連携" }).click();
		await expect(page.getByRole("heading", { name: "連携", exact: true })).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByRole("heading", { name: "OAuth (Twitch Device Code Flow)" })).toBeVisible();
		await expect(page.getByRole("heading", { name: "連携アプリ設定（run_with）" })).toBeHidden();

		await tabs.getByRole("button", { name: "診断" }).click();
		await expect(page.getByRole("heading", { name: "診断" })).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByRole("heading", { name: "ライブイベント" })).toBeHidden();

		await tabs.getByRole("button", { name: "設定" }).click();
		await expect(page.getByRole("heading", { name: "設定", exact: true })).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByRole("heading", { name: "接続情報" })).toBeHidden();
		await page.locator("summary").filter({ hasText: "ランタイム詳細" }).click();
		await expect(page.getByRole("heading", { name: "接続情報" })).toBeVisible();
	});

	test("hash routes fall back to the new information architecture", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}#live`);
		await expect(page.getByRole("heading", { name: "ライブ" })).toBeVisible({
			timeout: 15_000,
		});

		await page.goto(`/gui/${tokenQuery()}#setup`);
		await expect(page.getByRole("heading", { name: "連携", exact: true })).toBeVisible({
			timeout: 15_000,
		});

		await page.goto(`/gui/${tokenQuery()}#logs`);
		await expect(page.getByRole("heading", { name: "診断" })).toBeVisible({
			timeout: 15_000,
		});

		await page.goto(`/gui/${tokenQuery()}#tools`);
		await expect(page.getByRole("heading", { name: "設定", exact: true })).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByRole("heading", { name: "接続情報" })).toBeHidden();

		await page.goto(`/gui/${tokenQuery()}#unknown-legacy-tab`);
		await expect(page.getByRole("heading", { name: "ホーム" })).toBeVisible({
			timeout: 15_000,
		});
	});
});

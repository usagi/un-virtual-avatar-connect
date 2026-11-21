import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

test.describe("GUI redesign: Modes planning surface", () => {
	test("動作モードを選び、必要なときだけ切替内容を確認できる", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}#modes`);

		const main = page.getByRole("main");
		await expect(page.getByRole("heading", { name: "動作モード" })).toBeVisible({
			timeout: 15_000,
		});
		await expect(main.getByText("選択中のモード", { exact: true })).toBeVisible();
		await expect(main.getByText("現在のモード:")).toBeVisible();
		await expect(main.getByText("利用可能:")).toBeVisible();
		await expect(main.getByText("Flowgraph", { exact: true })).toBeHidden();
		await expect(main.getByRole("button", { name: /作業/ })).toHaveCount(0);

		await main.locator("button.mode-button").filter({ hasText: "Streaming" }).click();
		await main.getByText("切替内容の詳細", { exact: true }).click();
		await expect(main.getByText("切替計画", { exact: true })).toBeVisible();
		await expect(main.getByText("Flowgraph", { exact: true })).toBeVisible();
		await expect(main.getByText("連携アプリ", { exact: true })).toBeVisible();
		await expect(main.getByText("Managed App の変更はありません。")).toBeVisible();
		await expect(main.getByText("daily").first()).toBeVisible();
		await expect(main.getByText("streaming").last()).toBeVisible();
		await expect(main.locator("button.mode-button").filter({ hasText: "Streaming" })).toHaveAttribute("aria-pressed", "true");
		await expect(main.getByText("遷移進捗")).toBeVisible();
		await expect(main.getByText(/completed|applying|firing|suppressing/)).toBeVisible();
		await expect(main.getByText("現在のモード:")).toBeVisible();
		await expect(main.locator("button.mode-button").filter({ hasText: "Streaming" })).toBeDisabled();
	});

	test("動作モードは狭幅でも横へ溢れない", async ({ page }, testInfo) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(`/gui/${tokenQuery()}#modes`);
		await expect(page.getByRole("heading", { name: "動作モード" })).toBeVisible({ timeout: 15_000 });
		const dimensions = await page.evaluate(() => ({
			clientWidth: document.documentElement.clientWidth,
			scrollWidth: document.documentElement.scrollWidth,
		}));
		expect(dimensions.scrollWidth).toBeLessThanOrEqual(dimensions.clientWidth);
		await page.screenshot({ path: testInfo.outputPath("modes-mobile.png") });
	});
});

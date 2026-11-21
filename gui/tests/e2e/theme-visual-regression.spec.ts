import { expect, test } from "@playwright/test";

import { tokenQuery } from "./fixtures";

const THEMES = ["standard", "dark-crimson", "light-silver", "soft-cute"] as const;

test.describe("GUI visual regression", () => {
	for (const theme of THEMES) {
		test(`settings surface baseline: ${theme}`, async ({ page, browserName }) => {
			await page.setViewportSize({ width: 1366, height: 768 });
			await page.addInitScript(
				({ storageKey, themeId }) => {
					window.localStorage.setItem(storageKey, themeId);
				},
				{ storageKey: "vac.gui.theme", themeId: theme }
			);

			await page.goto(`/gui/${tokenQuery()}#settings`);
			await expect(page.locator("html")).toHaveAttribute("data-vac-theme", theme);
			await expect(page.getByRole("heading", { name: "テーマ" })).toBeVisible({ timeout: 15_000 });

			const choices = page.getByRole("radiogroup", { name: "GUI テーマ" });
			await expect(choices.getByRole("radio", { checked: true })).toHaveCount(1);
			// Screenshot baselines are recorded on Windows Chromium.
			if (process.platform !== "win32" || browserName !== "chromium") return;
			await expect(choices).toHaveScreenshot(`settings-${theme}.png`, {
				animations: "disabled",
				caret: "hide",
				maxDiffPixelRatio: 0.01,
			});
		});
	}
});

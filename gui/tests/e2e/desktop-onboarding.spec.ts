import { expect, test, type Page } from "@playwright/test";
import { strFromU8, unzipSync } from "fflate";

import { tokenQuery } from "./fixtures";

const profileResponse = {
	root: "C:\\Users\\test\\AppData\\Roaming\\UN Virtual Avatar Connect",
	managed_mode: true,
	onboarding_completed: false,
	active_profile_id: "default",
	profiles: [
		{
			id: "default",
			label: "Default",
			conf_path: "C:\\Users\\test\\AppData\\Roaming\\UN Virtual Avatar Connect\\profiles\\default\\conf.toml",
			is_active: true,
			template_id: "blank",
		},
	],
};

const templateResponse = [
	{
		id: "streaming-companion",
		label: "配信AIと始める",
		description: "AI音声とアバター反応を配信へ接続する構成",
		recommended: true,
	},
	{
		id: "desktop-assistant",
		label: "デスクトップAI",
		description: "会話と音声出力を常駐アシスタントへ育てる構成",
		recommended: false,
	},
	{
		id: "automation",
		label: "データ連携・自動化",
		description: "入力、状態、外部処理をFlowgraphで組み立てる構成",
		recommended: false,
	},
	{
		id: "blank",
		label: "空から作る",
		description: "構成を追加せず新しいworkspaceだけを作成",
		recommended: false,
	},
];

async function mockOnboarding(page: Page, onCreate?: (body: unknown) => void) {
	await page.route("**/api/v1/control/desktop/profile-templates", async (route) => {
		await route.fulfill({ json: templateResponse });
	});
	await page.route("**/api/v1/control/desktop/profiles", async (route) => {
		if (route.request().method() === "GET") {
			await route.fulfill({ json: profileResponse });
			return;
		}
		onCreate?.(route.request().postDataJSON());
		await route.fulfill({ status: 201, json: { profile: profileResponse.profiles[0] } });
	});
	await page.route("**/api/v1/control/desktop/profiles/*/activate", async (route) => {
		await route.fulfill({ json: { profile: profileResponse.profiles[0] } });
	});
	await page.route("**/api/v1/control/restart", async (route) => {
		await route.fulfill({ json: { new_pid: 1234 } });
	});
}

for (const viewport of [
	{ name: "desktop", width: 1366, height: 768 },
	{ name: "mobile", width: 390, height: 844 },
]) {
	test(`desktop onboarding renders without overflow on ${viewport.name}`, async ({ page }, testInfo) => {
		await mockOnboarding(page);
		await page.setViewportSize({ width: viewport.width, height: viewport.height });
		await page.goto(`/gui/${tokenQuery()}`);

		await expect(page.getByRole("heading", { name: "Virtual Avatar Connect" })).toBeVisible();
		await expect(page.getByRole("heading", { name: "新しく始める" })).toBeVisible();
		await expect(page.getByRole("heading", { name: "既存の設定を使う" })).toBeVisible();
		await expect(page.getByRole("radio", { name: /配信AIと始める/ })).toBeChecked();
		await expect(page.getByRole("radio", { name: /デスクトップAI/ })).toBeVisible();
		await expect(page.getByRole("radio", { name: /データ連携・自動化/ })).toBeVisible();
		await expect(page.getByText("Profile ID")).toHaveCount(0);
		await expect(page.getByText("Choose File")).toHaveCount(0);
		await expect(page.getByRole("button", { name: "作成して開始" })).toBeEnabled();
		await expect(page.getByRole("button", { name: "開く" })).toBeVisible();

		const metrics = await page.evaluate(() => ({
			viewport: document.documentElement.clientWidth,
			content: document.documentElement.scrollWidth,
		}));
		expect(metrics.content).toBeLessThanOrEqual(metrics.viewport);
		const screenshot = await page.screenshot({ fullPage: false, path: testInfo.outputPath(`onboarding-${viewport.name}.png`) });
		expect(screenshot.byteLength).toBeGreaterThan(15_000);
	});
}

test("recommended template is used when creating a workspace", async ({ page }) => {
	let createBody: unknown = null;
	await mockOnboarding(page, (body) => (createBody = body));
	await page.goto(`/gui/${tokenQuery()}`);
	await page.getByRole("button", { name: "作成して開始" }).click();
	await expect
		.poll(() => createBody)
		.toEqual({
			id: "my-unvac",
			label: "My UNVAC",
			template_id: "streaming-companion",
		});
	await expect(page.getByRole("status")).toContainText("再起動を要求しました");
	await expect(page.getByRole("button", { name: "再起動待ち…" })).toBeDisabled();
});

test("ZIP workspace is sent to the profile bundle endpoint", async ({ page }) => {
	await mockOnboarding(page);
	let requestUrl = "";
	let requestBytes = 0;
	await page.route("**/api/v1/control/desktop/profiles/import-bundle?*", async (route) => {
		requestUrl = route.request().url();
		requestBytes = route.request().postDataBuffer()?.byteLength ?? 0;
		await route.fulfill({ status: 201, json: { profile: profileResponse.profiles[0] } });
	});
	await page.goto(`/gui/${tokenQuery()}`);
	await page.getByLabel("プロファイルファイル").setInputFiles({
		name: "My Stream.zip",
		mimeType: "application/zip",
		buffer: Buffer.from("PK fixture"),
	});
	await page.getByRole("button", { name: "取り込んで開始" }).click();
	await expect.poll(() => requestBytes).toBeGreaterThan(0);
	const url = new URL(requestUrl);
	expect(url.searchParams.get("id")).toBe("my-stream");
	expect(url.searchParams.get("label")).toBe("My Stream");
});

test("workspaceフォルダーから選択confと関連ファイルをbundle化する", async ({ page }) => {
	await mockOnboarding(page);
	let requestBody: Buffer | null = null;
	let importUrl = "";
	await page.route("**/api/v1/control/desktop/profiles/import-bundle?*", async (route) => {
		requestBody = route.request().postDataBuffer();
		importUrl = route.request().url();
		await route.fulfill({ status: 201, json: { profile: profileResponse.profiles[0] } });
	});
	await page.goto(`/gui/${tokenQuery()}`);

	const files = [
		{ path: "conf.toml", text: 'workers = 1\nflowgraph_dir = "unused-flowgraph"\n' },
		{ path: "conf.local.solo.toml", text: 'workers = 2\nflowgraph_dir = "flowgraph.local"\n' },
		{
			path: "flowgraph.local/main.flowgraph.toml",
			text: '[meta]\ntitle = "Imported"\n[nodes.dictionary.properties]\npath = "dictionary.local.tsv"\n',
		},
		{ path: "flowgraph.local/main.flowgraph.toml.bak-20260714-120000", text: "old" },
		{ path: "dictionary.local.tsv", text: "source\treplacement\nfoo\tbar\n" },
		{ path: "unrelated.txt", text: "do not import" },
		{ path: "credentials/openai-api-key.txt", text: "secret" },
	];
	await page.getByLabel("ワークスペースフォルダー").evaluate((element, entries) => {
		const transfer = new DataTransfer();
		for (const entry of entries) {
			const file = new File([entry.text], entry.path.split("/").at(-1) ?? entry.path, { type: "text/plain" });
			Object.defineProperty(file, "webkitRelativePath", { value: `legacy/${entry.path}` });
			transfer.items.add(file);
		}
		Object.defineProperty(element, "files", { configurable: true, value: transfer.files });
		element.dispatchEvent(new Event("change", { bubbles: true }));
	}, files);

	await page.getByLabel("使用する設定").selectOption("conf.local.solo.toml");
	await expect(page.getByText("legacy / conf.local.solo.toml", { exact: true })).toBeVisible();
	await page.getByRole("button", { name: "取り込んで開始" }).click();
	await expect.poll(() => requestBody?.byteLength ?? 0).toBeGreaterThan(0);

	const archive = unzipSync(new Uint8Array(requestBody!));
	expect(strFromU8(archive["conf.toml"])).toContain("workers = 2");
	expect(strFromU8(archive["flowgraph.local/main.flowgraph.toml"])).toContain('title = "Imported"');
	expect(strFromU8(archive["dictionary.local.tsv"])).toContain("foo\tbar");
	expect(archive["unrelated.txt"]).toBeUndefined();
	expect(archive["credentials/openai-api-key.txt"]).toBeUndefined();
	expect(archive["flowgraph.local/main.flowgraph.toml.bak-20260714-120000"]).toBeUndefined();
	const url = new URL(importUrl);
	expect(url.searchParams.get("id")).toBe("legacy");
	expect(url.searchParams.get("label")).toBe("legacy");
});

test("saved workspace restarts through the active profile store", async ({ page }) => {
	await mockOnboarding(page);
	let restartBody: unknown = null;
	await page.route("**/api/v1/control/restart", async (route) => {
		restartBody = route.request().postDataJSON();
		await route.fulfill({ json: { new_pid: 1234 } });
	});
	await page.goto(`/gui/${tokenQuery()}`);
	await page.getByRole("button", { name: "開く" }).click();
	await expect.poll(() => restartBody).toEqual({ conf: profileResponse.profiles[0].conf_path, graceful_ms: 800 });
	await expect(page.getByRole("status")).toContainText("Default");
});

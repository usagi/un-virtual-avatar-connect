import { expect, test } from "@playwright/test";

import { openFlowgraphStudio, tokenQuery } from "./fixtures";

test.describe("GUI redesign: Flowgraph Studio layout", () => {
	test("Flowgraph Studioをナビゲーションから直接開く", async ({ page }, testInfo) => {
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);

		await openFlowgraphStudio(page);
		await expect(page.getByRole("button", { name: "概要へ戻る" })).toHaveCount(0);
		const dimensions = await page.evaluate(() => ({
			clientWidth: document.documentElement.clientWidth,
			scrollWidth: document.documentElement.scrollWidth,
		}));
		expect(dimensions.scrollWidth).toBeLessThanOrEqual(dimensions.clientWidth);
		await page.screenshot({ path: testInfo.outputPath("flowgraph-studio-desktop.png") });
	});

	test("Flowgraph Studioは狭幅でもページ全体を横へ溢れさせない", async ({ page }, testInfo) => {
		await page.setViewportSize({ width: 390, height: 844 });
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);

		await openFlowgraphStudio(page);
		const dimensions = await page.evaluate(() => ({
			clientWidth: document.documentElement.clientWidth,
			scrollWidth: document.documentElement.scrollWidth,
		}));
		expect(dimensions.scrollWidth).toBeLessThanOrEqual(dimensions.clientWidth);
		await expect(page.getByRole("button", { name: "保存", exact: true })).toBeVisible();
		await expect(page.getByText("ファイル操作", { exact: false })).toBeVisible();
		await expect(page.getByRole("button", { name: "再読み込み", exact: true })).toBeHidden();
		await page.screenshot({ path: testInfo.outputPath("flowgraph-studio-mobile.png") });
	});

	test("Windows内部パスを利用者向けの編集フォルダー表示へ変換する", async ({ page }) => {
		await page.route("**/api/v1/control/flowgraph/tree", async (route) => {
			const response = await route.fetch();
			const body = (await response.json()) as Record<string, unknown>;
			body.root_dir = "//?/C:/Users/example/AppData/Roaming/UN Virtual Avatar Connect/profiles/main/flowgraph.local";
			await route.fulfill({ response, json: body });
		});
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);

		const path = page.getByTestId("flowgraph-root-path");
		await expect(path).toHaveText("C:\\Users\\example\\AppData\\Roaming\\UN Virtual Avatar Connect\\profiles\\main\\flowgraph.local");
		await expect(path).not.toContainText("//?/");
	});

	test("Problems表示中も低いデスクトップ画面でCanvasを切らず、折り畳むと作業領域が広がる", async ({ page }) => {
		await page.setViewportSize({ width: 1365, height: 768 });
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);

		const root = page.getByTestId("flowgraph-studio-root");
		const canvas = page.getByTestId("flowgraph-canvas-pane");
		const problems = page.getByTestId("flowgraph-problems-pane");
		const [rootBox, canvasBox, problemsBox] = await Promise.all([root.boundingBox(), canvas.boundingBox(), problems.boundingBox()]);
		expect(rootBox).not.toBeNull();
		expect(canvasBox).not.toBeNull();
		expect(problemsBox).not.toBeNull();
		expect(rootBox!.y + rootBox!.height).toBeLessThanOrEqual(768);
		expect(canvasBox!.y + canvasBox!.height).toBeLessThanOrEqual(problemsBox!.y);

		await problems.getByRole("button", { name: "折り畳む" }).click();
		await expect(problems.getByRole("button", { name: "展開する" })).toBeVisible();
		await expect.poll(async () => (await canvas.boundingBox())?.height ?? 0).toBeGreaterThan(canvasBox!.height);
	});

	test("保存座標が重なるファイルは初回表示で自動再配置する", async ({ page }) => {
		await page.route("**/api/v1/control/flowgraph/file/sample", async (route) => {
			const response = await route.fetch();
			const body = (await response.json()) as { parsed?: { nodes?: Array<Record<string, unknown>> } };
			if (body.parsed?.nodes) {
				body.parsed.nodes = body.parsed.nodes.map((node) => ({ ...node, position: [100, 100] }));
			}
			await route.fulfill({ response, json: body });
		});
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);
		await page.getByRole("button", { name: /^sample\b/ }).click();
		await expect(page.getByText("ノード配置を自動補正")).toBeVisible();

		const nodeBoxes = await page.locator('[data-testid^="flowgraph-node-"]').evaluateAll((elements) =>
			elements.map((element) => {
				const rect = element.getBoundingClientRect();
				return { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom };
			})
		);
		for (let i = 0; i < nodeBoxes.length; i += 1) {
			for (let j = i + 1; j < nodeBoxes.length; j += 1) {
				const a = nodeBoxes[i];
				const b = nodeBoxes[j];
				const overlaps = a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
				expect(overlaps, `node ${i} overlaps node ${j}`).toBe(false);
			}
		}
		await expect(page.getByText("未保存の変更", { exact: true })).toBeVisible();
		await expect(page.getByRole("button", { name: "保存", exact: true })).toBeVisible();
	});

	test("Flowgraph Studio exposes IDE-style workspace regions", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);

		await expect(page.getByRole("heading", { name: "Flowgraph Studio" })).toBeVisible({
			timeout: 15_000,
		});

		const main = page.getByRole("main");
		await expect(main.getByText("配信やAIの処理を、ノードの接続として編集・確認します。")).toBeVisible();
		await expect(main.getByText("ファイル", { exact: true }).first()).toBeVisible();
		await expect(main.getByText("キャンバス", { exact: true })).toBeVisible();
		await expect(main.getByText("ノード一覧", { exact: true })).toBeVisible();
		await expect(main.getByText("設定", { exact: true })).toBeVisible();
		await expect(main.getByText("問題", { exact: true })).toBeVisible();
		await expect(main.getByText("ノード", { exact: true })).toBeVisible();
		await expect(main.getByText("接続", { exact: true })).toBeVisible();
	});

	test("Flowgraph Studio opens a command palette for editor operations", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);

		const main = page.getByRole("main");
		await main.getByRole("button", { name: "コマンド" }).click();

		const dialog = page.getByRole("dialog", { name: "コマンド" });
		await expect(dialog).toBeVisible();
		await expect(dialog.getByRole("button", { name: /ディスクから再読み込み/ })).toBeVisible();
		await expect(dialog.getByRole("button", { name: /状態を保持して再読み込み/ })).toBeEnabled();
		await expect(dialog.getByRole("button", { name: /元に戻す/ })).toBeVisible();
		await expect(dialog.getByRole("button", { name: /選択ノードを複製/ })).toBeVisible();
		await expect(dialog.getByRole("button", { name: /現在のファイルを保存/ })).toBeVisible();
		await expect(dialog.getByRole("button", { name: /ZIPから読み込む/ })).toBeVisible();
		await dialog.getByPlaceholder("操作名またはノード名を検索").fill("literal");
		await expect(dialog.getByRole("button", { name: /Insert/ }).first()).toBeVisible();
		await dialog.getByRole("button", { name: "閉じる" }).click();
		await expect(dialog).toBeHidden();
	});

	test("Flowgraph Studio exposes explicit state-preserving reload action", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);

		const main = page.getByRole("main");
		await main.getByTestId("flowgraph-file-operations").locator("summary").click();
		await expect(main.getByRole("button", { name: "状態を保持して再読み込み" })).toBeEnabled({
			timeout: 15_000,
		});

		await main.getByRole("button", { name: "コマンド" }).click();
		const dialog = page.getByRole("dialog", { name: "コマンド" });
		await dialog.getByPlaceholder("操作名またはノード名を検索").fill("状態");
		await expect(dialog.getByRole("button", { name: /状態を保持して再読み込み/ })).toBeEnabled();
	});

	test("Flowgraph Studio shows node effect metadata in the palette", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);
		await page.evaluate(() => localStorage.removeItem("vac-flowgraph-palette-hidden-categories"));

		const paletteSearch = page.getByPlaceholder(/検索（feature \/ title）/);
		await expect(paletteSearch).toBeVisible({ timeout: 15_000 });
		await paletteSearch.fill("util.log");

		const logEntry = page.getByTestId("palette-entry-flowgraph_util_log");
		await expect(logEntry).toBeVisible();
		await expect(logEntry.getByText("Effect", { exact: true })).toBeVisible();
		await expect(logEntry.getByText("Trace", { exact: true })).toBeVisible();

		await paletteSearch.fill("audio.play");
		const audioPlayEntry = page.getByTestId("palette-entry-flowgraph_audio_play");
		await expect(audioPlayEntry).toBeVisible();
		await expect(audioPlayEntry.getByText("Effect", { exact: true })).toBeVisible();
		await expect(audioPlayEntry).toContainText("Audio / File read");

		await paletteSearch.fill("audio.envelope");
		const envelopeEntry = page.getByTestId("palette-entry-flowgraph_audio_envelope");
		await expect(envelopeEntry).toBeVisible();
		await expect(envelopeEntry.getByText("Pure", { exact: true })).toBeVisible();
	});

	test("親Canvasからmodule instanceとcross-file portalを辿れる", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);

		await page.getByRole("button", { name: /^composition-parent\b/ }).click();
		const child = page.getByTestId("flowgraph-node-child");
		await expect(child).toBeVisible({ timeout: 15_000 });
		await expect(child).toContainText("composition-module");
		const portal = page.locator('[data-testid^="flowgraph-node-__portal:out:"]').first();
		await expect(portal).toBeVisible();
		await expect(portal).toContainText("composition-target");

		await child.getByRole("button", { name: "子Flowgraphを開く" }).click();
		await expect(page.getByTestId("flowgraph-node-log")).toBeVisible();

		await page.getByRole("button", { name: /^composition-parent\b/ }).click();
		const reopenedPortal = page.locator('[data-testid^="flowgraph-node-__portal:out:"]').first();
		await reopenedPortal.getByRole("button", { name: "子Flowgraphを開く" }).click();
		await expect(page.getByTestId("flowgraph-node-sink")).toBeVisible();
	});

	test("profile graphで全file portalとinstanceを一枚に表示する", async ({ page }) => {
		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);
		await page.getByRole("button", { name: "構成全体" }).click();

		const overview = page.getByTestId("flowgraph-composition-overview");
		await expect(overview).toBeVisible();
		await expect(overview).toContainText("composition-parent");
		await expect(overview).toContainText("composition-module");
		await expect(overview).toContainText("@child");
		await expect(overview).toContainText(/portal · active/);
	});

	test("型不一致とinactive境界を表示し、instance activityから子edgeへdrill-downする", async ({ page }) => {
		await page.route("**/api/v1/control/flowgraph/diagnostics", async (route) => {
			const response = await route.fetch();
			const body = (await response.json()) as {
				diagnostics: Array<Record<string, unknown>>;
				inactive_exec_nodes: string[];
				composition: { instances: Array<Record<string, unknown>>; portals: Array<Record<string, unknown>> };
				runtime_trace: Array<Record<string, unknown>>;
			};
			body.diagnostics = [
				...(body.diagnostics ?? []),
				{
					severity: "error",
					code: "engine-build",
					message: "型不一致: string -> int",
					file: "composition-parent.flowgraph.toml",
					node: "in",
				},
				{
					severity: "warning",
					code: "activation-boundary",
					message: "cross-file portalのactivation境界が不一致",
				},
			];
			body.inactive_exec_nodes = ["composition-parent::in"];
			body.composition.instances = body.composition.instances.map((instance: Record<string, unknown>) => ({
				...instance,
				active: instance.id === "child" ? false : instance.active,
			}));
			body.composition.portals = body.composition.portals.map((portal: Record<string, unknown>) => ({
				...portal,
				from_active: true,
				to_active: false,
				active: false,
			}));
			body.runtime_trace = [
				{
					generation: 7,
					runtime_node_id: "composition-parent/@child::gate",
					source_fq: "composition-module",
					instance_path: "child",
					port: "exec_out",
					phase: "exec_output",
					value_summary: "exec",
					active: true,
				},
			];
			await route.fulfill({ response, json: body });
		});

		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);
		await page.getByRole("button", { name: /^composition-parent\b/ }).click();
		await expect(page.getByTestId("flowgraph-node-in")).toContainText("inactive");
		await expect(page.getByTestId("flowgraph-node-child")).toContainText("live activity");
		await expect(page.getByText("型不一致: string -> int")).toBeVisible();
		await expect(page.getByText("cross-file portalのactivation境界が不一致")).toBeVisible();

		await page.getByTestId("flowgraph-node-child").getByRole("button", { name: "子Flowgraphを開く" }).click();
		await expect(page.getByTestId("flowgraph-node-gate")).toBeVisible();
		await expect(page.getByText("live", { exact: true })).toBeVisible();
	});

	test("module instanceを保存・reloadし、ZIP export/importを往復できる", async ({ page }) => {
		let savedToml = "";
		let reloadCount = 0;
		let exportCount = 0;
		const importDryRuns: boolean[] = [];
		await page.route("**/api/v1/control/flowgraph/file/composition-parent", async (route) => {
			if (route.request().method() !== "PUT") return route.continue();
			savedToml = (route.request().postDataJSON() as { content: string }).content;
			await route.fulfill({
				contentType: "application/json",
				body: JSON.stringify({
					fq: "composition-parent",
					path: "composition-parent.flowgraph.toml",
					diagnostics: [],
					ok: true,
					backup: null,
				}),
			});
		});
		await page.route("**/api/v1/control/flowgraph/reload", async (route) => {
			reloadCount += 1;
			await route.fulfill({
				contentType: "application/json",
				body: JSON.stringify({ root_dir: "fixture", ok: true, diagnostics: [], node_count: 3 }),
			});
		});
		await page.route("**/api/v1/control/flowgraph/export/zip", async (route) => {
			exportCount += 1;
			await route.fulfill({ contentType: "application/zip", body: Buffer.from("PK fixture") });
		});
		await page.route("**/api/v1/control/flowgraph/import/zip?*", async (route) => {
			const dryRun = new URL(route.request().url()).searchParams.get("dry_run") === "true";
			importDryRuns.push(dryRun);
			const manifest = { schema: "unvac.flowgraph.fragment.v1", exported_at: "2026-08-03T00:00:00Z", scope: "file" };
			const body = dryRun
				? { kind: "preview", manifest, file_count: 1, entries: [], danglings: [], target_prefix: "", conflicts: [], companions: [] }
				: {
						kind: "report",
						manifest,
						written_files: ["copy.flowgraph.toml"],
						written_companions: [],
						danglings_unresolved: [],
						target_prefix: "",
					};
			await route.fulfill({ contentType: "application/json", body: JSON.stringify(body) });
		});

		await page.goto(`/gui/${tokenQuery()}#flowgraph`);
		await openFlowgraphStudio(page);
		await page.getByRole("button", { name: /^composition-parent\b/ }).click();
		const child = page.getByTestId("flowgraph-node-child");
		const box = await child.boundingBox();
		expect(box).not.toBeNull();
		await page.mouse.move(box!.x + 30, box!.y + 20);
		await page.mouse.down();
		await page.mouse.move(box!.x + 90, box!.y + 70, { steps: 5 });
		await page.mouse.up();
		await page.getByRole("button", { name: "保存", exact: true }).click();
		await expect.poll(() => savedToml).toContain("[[instances]]");
		expect(savedToml).toContain('graph = "composition-module"');

		await page.getByRole("button", { name: "コマンド" }).click();
		const commandDialog = page.getByRole("dialog", { name: "コマンド" });
		await commandDialog.getByPlaceholder("操作名またはノード名を検索").fill("ディスクから再読み込み");
		await commandDialog.getByRole("button", { name: /ディスクから再読み込み/ }).click();
		await expect.poll(() => reloadCount).toBe(1);
		await expect(page.getByTestId("flowgraph-node-child")).toBeVisible();

		await page.getByTestId("flowgraph-file-operations").locator("summary").click();
		await page.getByRole("button", { name: /ZIP書き出し/ }).click();
		await expect.poll(() => exportCount).toBe(1);
		await page.getByRole("button", { name: "ZIP読み込み…" }).click();
		const zipDialog = page.getByRole("dialog");
		await zipDialog
			.locator('input[type="file"]')
			.setInputFiles({ name: "composition.zip", mimeType: "application/zip", buffer: Buffer.from("PK") });
		await expect(zipDialog.getByText("Preview", { exact: true })).toBeVisible();
		await zipDialog.getByRole("button", { name: "Import", exact: true }).click();
		await expect(zipDialog).toBeHidden();
		expect(importDryRuns).toEqual([true, false]);
	});
});

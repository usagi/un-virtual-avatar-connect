import { expect, test } from "@playwright/test";
import { authHeader, tokenQuery } from "./fixtures";

/**
 * §3.4 live-quick-add-learn-undo
 *
 * conf.fixture.e2e.toml の `[[control_api.tables]]` + fixture flowgraph の
 * `glossary.learn` / `glossary.forget` ノードを使い、Live タブの
 * 辞書タブのクイック追加ウィジェットからの一連の UX を検証する。
 *
 *   1. Quick-Add カタログに sample_dict が出てくる
 *   2. reading / surface を入力して [Learn] を押すと
 *      POST /api/v1/control/flowgraph/default/trigger/sample::learn が
 *      期待 body で飛ぶ
 *   3. 履歴カウンタが 1 になり、[履歴] 展開で 1 行出る
 *   4. [Undo] を押すと
 *      POST /api/v1/control/flowgraph/default/trigger/sample::forget が飛び、
 *      行が "Undone" に遷移する
 *
 * 設計メモ:
 *   E2E ではファイル永続化 (table.write_tsv) までは配線しない。
 *   Quick-Add の関心事は「GUI → Control API → node 発火まで届いたか」であり、
 *   テーブル永続化は §3.3 Editor 側の責務として分離する。
 */

const TRIGGER_PREFIX = "/api/v1/control/flowgraph/default/trigger/";

test.describe("§3.4 live-quick-add-learn-undo", () => {
	test("Learn fires trigger POST and Undo fires forget trigger POST", async ({ page, request }) => {
		await page.goto(`/gui/${tokenQuery()}`);
		await page.getByRole("button", { name: "辞書" }).click();

		// 辞書タブと Quick-Add カタログ読み込みを待つ。
		// API 経由でも catalog を別途 verify しておくと、UI が見つからないときに
		// conf 側の問題か UI 側の問題かを切り分けやすい。
		const catalog = await request.get("/api/v1/control/tables", {
			headers: authHeader(),
		});
		expect(catalog.status(), await catalog.text()).toBe(200);
		const catalogBody = (await catalog.json()) as {
			tables: Array<{ key: string; quick_add: unknown }>;
		};
		expect(catalogBody.tables.some((t) => t.key === "sample_dict" && t.quick_add != null)).toBe(true);

		// クイック追加 widget のスコープを <section> レベルに確定させる。
		// DictionaryLiveQuickAdd.svelte は <section> > <header> > <h3>クイック追加</h3> の構造。
		const quickAddHeading = page.getByRole("heading", { name: "クイック追加" });
		await expect(quickAddHeading).toBeVisible({ timeout: 20_000 });
		const quickAdd = quickAddHeading.locator("xpath=ancestor::section[1]");

		// placeholder ベースで input を探す（文言は DictionaryLiveQuickAdd.svelte の real text）。
		const readingInput = quickAdd.getByPlaceholder(/reading（読み）/);
		const surfaceInput = quickAdd.getByPlaceholder(/surface（単語）/);
		await expect(readingInput).toBeVisible();
		await expect(surfaceInput).toBeVisible();

		const nonce = `nu24-${Date.now()}`;
		const reading = `いーつーいー ${nonce}`;
		const surface = `e2e_surface_${nonce}`;

		await readingInput.fill(reading);
		await surfaceInput.fill(surface);

		// Learn トリガ POST を捕まえる。node_id の URL encode 形は "sample%3A%3Alearn"。
		const learnRequestPromise = page.waitForRequest(
			(req) =>
				req.method() === "POST" && req.url().includes(TRIGGER_PREFIX) && decodeURIComponent(req.url()).includes("sample::learn"),
			{ timeout: 15_000 }
		);

		await quickAdd.getByRole("button", { name: "Learn" }).click();

		const learnRequest = await learnRequestPromise;
		const learnBody = learnRequest.postDataJSON() as {
			inputs: { reading: string; surface: string; part_of_speech: string; by?: string };
		};
		expect(learnBody.inputs.reading).toBe(reading);
		expect(learnBody.inputs.surface).toBe(surface);
		expect(learnBody.inputs.part_of_speech).toBe("名詞");
		expect(learnBody.inputs.by).toBe("gui:quick_add");

		// 202 Accepted が返る（server 側実装: HttpResponse::Accepted）。
		const learnResponse = await learnRequest.response();
		expect(learnResponse?.status()).toBe(202);

		// 履歴カウンタが 1 になるはず（button label に "(1)" を含む）。
		const historyToggle = quickAdd.getByRole("button", { name: /^履歴 \(/ });
		await expect(historyToggle).toContainText("(1)", { timeout: 5_000 });

		// 展開して undo ボタンを出す。
		await historyToggle.click();
		const undoButton = quickAdd.getByRole("button", { name: "Undo" });
		await expect(undoButton).toBeVisible();

		// Forget トリガ POST を捕まえる。
		const forgetRequestPromise = page.waitForRequest(
			(req) =>
				req.method() === "POST" && req.url().includes(TRIGGER_PREFIX) && decodeURIComponent(req.url()).includes("sample::forget"),
			{ timeout: 15_000 }
		);
		await undoButton.click();
		const forgetRequest = await forgetRequestPromise;
		const forgetBody = forgetRequest.postDataJSON() as {
			inputs: { surface: string; reading: string; mode?: string };
		};
		expect(forgetBody.inputs.surface).toBe(surface);
		expect(forgetBody.inputs.reading).toBe(reading);
		expect(forgetBody.inputs.mode).toBe("latest");

		const forgetResponse = await forgetRequest.response();
		expect(forgetResponse?.status()).toBe(202);

		// Undo 済み表示に遷移する。
		await expect(quickAdd.getByRole("button", { name: "Undone" })).toBeVisible({ timeout: 5_000 });
	});
});

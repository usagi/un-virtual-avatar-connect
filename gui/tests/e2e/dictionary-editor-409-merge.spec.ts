import { expect, test } from "@playwright/test";
import { authHeader, tokenQuery } from "./fixtures";

/**
 * §3.3 dictionary-editor-409-merge (ν-β-1)
 *
 * Glossary Editor の楽観ロック衝突経路を、実 409 を踏ませて検証する。
 *
 *   1. GUI を開き、辞書タブの編集セクションを待つ
 *   2. `sample_dict` の `Dr.USAGI`（unlocked な row）を検索で絞り込み、[編集] を押す
 *   3. 編集ダイアログで `reading` を書き換える
 *   4. ★ 書き換えて「更新する」を押す前に、別クライアント (Playwright `request`) が
 *      同じ row を PATCH で先行更新 → server 側の content_hash が進む
 *   5. GUI で「更新する」を押す → GUI が保持していた古い If-Match で 409
 *   6. 3-way 衝突ダイアログ `DictionaryConflictDialog` が開き、mine / server の差分が
 *      実際に mine / server 値として並んでいること
 *   7. 「自分の編集を強制」ボタン → 最新 content_hash で PATCH → 閉じる
 *   8. API 直叩きで最終 row.reading が mine の値に確定していることを確認
 *   9. Cleanup: row 1 の reading を元の値に戻す
 *
 * 設計メモ:
 *   - 現行の作業単位IAでは、編集とクイック追加は独立した辞書タブに置く。
 *   - `revision` 番号は API 上存在しない。`If-Match: b3:<content_hash>` で楽観ロックする。
 *   - 409 レスポンス body は `{ error: "optimistic_lock_failed", detail: ... }` のみで
 *     diff 情報は含まない。GUI は衝突後に再 GET して 3-way を構築する。
 *   - data-testid は増やさない（§5.3）。`<label>` wrap で input が入れ子になっている
 *     ため `getByLabel(/^reading/)` で一意に引けることを確認済み。
 *   - ファイル汚染防止のため、test 末尾で必ず row 1 を元の値に PATCH して戻す
 *     （`sample.dict.tsv` は repo 管理下のためテスト後も差分ゼロが望ましい）。
 */

const TABLE_KEY = "sample_dict";
const TARGET_SURFACE = "Dr.USAGI";
const ORIGINAL_READING = "どくたーうさぎ";

type TableRow = {
	row_index: number;
	values: Record<string, unknown>;
};

type TableDto = {
	key: string;
	content_hash: string;
	columns: string[];
	rows: TableRow[];
};

test.describe("§3.3 dictionary-editor-409-merge", () => {
	test('PATCH 409 opens conflict dialog and "force mine" writes with fresh hash', async ({ page, request }) => {
		// --- preflight: 前回の test run で mine の値が残っていたら戻す ---------------
		// （local 再実行や前回 SIGINT で中断した場合の自己修復）
		{
			const r = await request.get(`/api/v1/control/table/${TABLE_KEY}`, {
				headers: authHeader(),
			});
			expect(r.status(), await r.text()).toBe(200);
			const body = (await r.json()) as TableDto;
			const target = body.rows.find((row) => row.values.surface === TARGET_SURFACE);
			expect(target, `fixture must contain row with surface=${TARGET_SURFACE}`).toBeDefined();
			if (target && target.values.reading !== ORIGINAL_READING) {
				const restore = await request.patch(`/api/v1/control/table/${TABLE_KEY}/entry/${target.row_index}`, {
					headers: { ...authHeader(), "If-Match": `b3:${body.content_hash}` },
					data: { values: { ...target.values, reading: ORIGINAL_READING } },
				});
				expect(restore.status(), await restore.text()).toBe(200);
			}
		}

		// --- 1. GUI を開く ---------------------------------------------------------
		await page.goto(`/gui/${tokenQuery()}`);
		await page.getByRole("button", { name: "辞書" }).click();

		// --- 2. 辞書編集セクションが見える ----------------------------------------
		// DictionaryTab.svelte が外側 <section> でラップ → DictionaryEditorPane.svelte の
		// 内側 <section> の 2 段入れ子になるので、見出しから最近接の <section> を拾う。
		const dictHeading = page.getByRole("heading", {
			name: "辞書の編集",
			level: 3,
		});
		await expect(dictHeading).toBeVisible({ timeout: 20_000 });
		const dictSection = dictHeading.locator("xpath=ancestor::section[1]");

		// sample_dict タブがロード完了するまで、テーブル行が 2 行出揃うのを待つ。
		// fixture TSV は header + 2 行 (にんげん / Dr.USAGI)。
		const rows = dictSection.locator("tbody tr");
		await expect(rows).toHaveCount(2, { timeout: 15_000 });

		// --- 3. 検索で Dr.USAGI 行に絞り、[編集] を開く -----------------------------
		const filter = dictSection.getByPlaceholder(/読み \/ 単語 \/ 品詞 \/ コメント/);
		await filter.fill(TARGET_SURFACE);
		await expect(rows).toHaveCount(1);
		await rows.first().getByRole("button", { name: "編集" }).click();

		// 編集ダイアログ（DictionaryEntryForm）。dialog aria-labelledby は row_index を含む。
		const editForm = page.getByRole("dialog", { name: /辞書エントリを編集/ });
		await expect(editForm).toBeVisible();

		// --- 4. GUI 側で reading を書き換える（まだ保存はしない） ------------------
		const nonce = `nb1-${Date.now()}`;
		const myReplacement = `MINE-${nonce}`;
		const theirReplacement = `SERVER-${nonce}`;

		const readingInput = editForm.getByLabel(/^reading/);
		await expect(readingInput).toBeVisible();
		await readingInput.fill(myReplacement);

		// --- 5. 別クライアントで同じ row を先行 PATCH（server 側 hash を進める） -----
		// GUI が hold している priorHash を stale にするのが目的。
		const preRes = await request.get(`/api/v1/control/table/${TABLE_KEY}`, {
			headers: authHeader(),
		});
		expect(preRes.status()).toBe(200);
		const preBody = (await preRes.json()) as TableDto;
		const target = preBody.rows.find((row) => row.values.surface === TARGET_SURFACE);
		expect(target).toBeDefined();
		const targetRowIndex = target!.row_index;
		const targetValues = { ...target!.values, reading: theirReplacement };

		const extPatch = await request.patch(`/api/v1/control/table/${TABLE_KEY}/entry/${targetRowIndex}`, {
			headers: { ...authHeader(), "If-Match": `b3:${preBody.content_hash}` },
			data: { values: targetValues },
		});
		expect(extPatch.status(), await extPatch.text()).toBe(200);

		// --- 6. GUI の「更新する」で 409 → ConflictDialog が開く --------------------
		// 409 レスポンスをネットワーク経路で確認したい（UI 依存だけだと debug が難しいので）。
		const conflictResPromise = page.waitForResponse(
			(res) =>
				res.status() === 409 &&
				res.request().method() === "PATCH" &&
				res.url().includes(`/api/v1/control/table/${TABLE_KEY}/entry/${targetRowIndex}`),
			{ timeout: 10_000 }
		);
		await editForm.getByRole("button", { name: "更新する" }).click();
		const conflictRes = await conflictResPromise;
		const conflictBody = (await conflictRes.json()) as { error: string };
		expect(conflictBody.error).toBe("optimistic_lock_failed");

		// 編集フォームは閉じる（DictionaryEntryForm.submit() 側で open=false）。
		await expect(editForm).not.toBeVisible({ timeout: 5_000 });

		const conflictDialog = page.getByRole("dialog", { name: /辞書編集で競合/ });
		await expect(conflictDialog).toBeVisible({ timeout: 10_000 });

		// 3-way テーブルに mine / server 両方の値が並ぶ（どちらも reading 行）。
		await expect(conflictDialog).toContainText(myReplacement);
		await expect(conflictDialog).toContainText(theirReplacement);
		// `reading` カラム名も行頭に出ているはず。
		await expect(conflictDialog.locator("tbody tr")).toContainText(["reading"]);

		// --- 7. 「自分の編集を強制」→ fresh hash で PATCH ---------------------------
		const resolvePatchPromise = page.waitForResponse(
			(res) =>
				res.status() === 200 &&
				res.request().method() === "PATCH" &&
				res.url().includes(`/api/v1/control/table/${TABLE_KEY}/entry/${targetRowIndex}`),
			{ timeout: 10_000 }
		);
		await conflictDialog.getByRole("button", { name: "自分の編集を強制" }).click();
		await resolvePatchPromise;
		await expect(conflictDialog).not.toBeVisible({ timeout: 10_000 });

		// --- 8. API 直叩きで mine が勝った最終状態を確認 ----------------------------
		const finalRes = await request.get(`/api/v1/control/table/${TABLE_KEY}`, {
			headers: authHeader(),
		});
		expect(finalRes.status()).toBe(200);
		const finalBody = (await finalRes.json()) as TableDto;
		const finalRow = finalBody.rows.find((row) => row.row_index === targetRowIndex);
		expect(finalRow?.values.reading).toBe(myReplacement);

		// --- 9. cleanup: reading を元の値に戻して repo の TSV 差分を作らない ---------
		const cleanupPatch = await request.patch(`/api/v1/control/table/${TABLE_KEY}/entry/${targetRowIndex}`, {
			headers: { ...authHeader(), "If-Match": `b3:${finalBody.content_hash}` },
			data: {
				values: {
					...finalRow!.values,
					reading: ORIGINAL_READING,
				},
			},
		});
		expect(cleanupPatch.status(), await cleanupPatch.text()).toBe(200);
	});
});

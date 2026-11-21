/** Keep actionable server explanations separate from transport diagnostics.
 * @param {unknown} error
 * @param {"read" | "save"} operation
 */
export function perceptionError(error, operation) {
	const fallback = operation === 'read'
		? '画面設定を読み込めませんでした。接続状態を確認して、もう一度読み込んでください。'
		: '保存結果を確認できませんでした。入力を残したまま、接続状態を確認してください。';
	const diagnostic = String(error);
	if (error && typeof error === 'object' && 'status' in error && 'body' in error) {
		const body = error.body;
		if (body && typeof body === 'object') {
			for (const key of ['detail', 'reason', 'message']) {
				const value = /** @type {Record<string, unknown>} */ (body)[key];
				if (typeof value === 'string' && value.trim()) {
					return {message: value, diagnostic};
				}
			}
		}
		return {message: fallback, diagnostic};
	}
	// Locally generated validation/conflict explanations are already readable.
	if (error instanceof Error && !(error instanceof TypeError) && error.message.trim()) {
		return {message: error.message, diagnostic};
	}
	return {message: fallback, diagnostic};
}

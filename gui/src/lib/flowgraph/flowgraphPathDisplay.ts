/**
 * Flowgraph API が返す実パスを、人が読むための Windows 表示へ整える。
 *
 * API 上の値はファイル操作に使うため変更せず、Windows の verbatim path
 * (`\\?\` / `//?/`) だけを UI 境界で隠す。
 */
export function flowgraphPathForDisplay(path: string | null | undefined): string {
	if (!path) return "読み込み中…";

	let display = path;
	if (/^(?:\\\\\?\\|\/\/\?\/)UNC[\\/]/i.test(display)) {
		display = `\\\\${display.replace(/^(?:\\\\\?\\|\/\/\?\/)UNC[\\/]/i, "")}`;
	} else {
		display = display.replace(/^(?:\\\\\?\\|\/\/\?\/)/, "");
	}

	// ドライブレターで始まる Windows path だけを OS 標準の区切りへ戻す。
	if (/^[A-Za-z]:[\\/]/.test(display) || display.startsWith("\\\\")) return display.replace(/\//g, "\\");
	return display;
}

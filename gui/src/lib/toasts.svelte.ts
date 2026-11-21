/**
 * トースト通知のモジュールシングルトンストア。
 *
 * `lagged` / `pause_state` / `reloaded` / `oauth_status` / `restarting` など、
 * 画面主操作を止めない粒度の通知を右下に数秒間表示する。
 *
 * 方針:
 *   - Svelte 5 runes（`$state`）で外部公開、UI は配列を購読するだけ
 *   - 時限自動消去（tone ごとに既定時間、user 操作で dismiss 可）
 *   - 非同期を追わずとも、`push()` → `cleanup setTimeout` で完結する単純モデル
 */

export type ToastTone = "info" | "success" | "warn" | "error";

export type ToastAction = {
	/** ボタンラベル。例: "元に戻す" */
	label: string;
	/** クリック時のハンドラ。成功後のみトーストを閉じる。 */
	onAction: () => void | Promise<void>;
};

export type ToastMessage = {
	id: number;
	tone: ToastTone;
	/** 短い見出し。例: "Reloaded"、"Restart pending"。 */
	title: string;
	/** 補足説明。省略可。 */
	detail?: string;
	/** 自動消去までの ms。0 なら手動でのみ消せる。 */
	timeout_ms: number;
	/** 作成時刻（Date.now()）。 */
	created_at: number;
	/** γ-4a.0: Undo 等のクリック可能なアクション。省略可。 */
	action?: ToastAction;
	action_pending?: boolean;
	action_error?: string;
};

const DEFAULT_TIMEOUT: Record<ToastTone, number> = {
	info: 3500,
	success: 3500,
	warn: 6000,
	error: 0,
};

class ToastStore {
	items: ToastMessage[] = $state([]);

	#nextId = 1;
	#timers = new Map<number, ReturnType<typeof setTimeout>>();

	push(opts: { tone: ToastTone; title: string; detail?: string; timeout_ms?: number; action?: ToastAction }): number {
		const id = this.#nextId++;
		const timeout_ms = opts.timeout_ms ?? DEFAULT_TIMEOUT[opts.tone];
		const msg: ToastMessage = {
			id,
			tone: opts.tone,
			title: opts.title,
			detail: opts.detail,
			timeout_ms,
			created_at: Date.now(),
			action: opts.action,
		};
		this.items = [...this.items, msg];
		if (timeout_ms > 0) {
			const t = setTimeout(() => this.dismiss(id), timeout_ms);
			this.#timers.set(id, t);
		}
		return id;
	}

	dismiss(id: number): void {
		const t = this.#timers.get(id);
		if (t) {
			clearTimeout(t);
			this.#timers.delete(id);
		}
		this.items = this.items.filter((m) => m.id !== id);
	}

	clear(): void {
		for (const t of this.#timers.values()) clearTimeout(t);
		this.#timers.clear();
		this.items = [];
	}

	// 便利ショートカット
	info(title: string, detail?: string, action?: ToastAction): number {
		return this.push({ tone: "info", title, detail, action });
	}
	success(title: string, detail?: string, action?: ToastAction): number {
		return this.push({ tone: "success", title, detail, action });
	}
	warn(title: string, detail?: string, action?: ToastAction): number {
		return this.push({ tone: "warn", title, detail, action });
	}
	error(title: string, detail?: string, action?: ToastAction): number {
		return this.push({ tone: "error", title, detail, action });
	}

	/** Keep failed actions available and prevent duplicate execution while pending. */
	async triggerAction(id: number): Promise<void> {
		const msg = this.items.find((m) => m.id === id);
		if (!msg?.action || msg.action_pending) return;
		const timer = this.#timers.get(id);
		if (timer) clearTimeout(timer);
		this.#timers.delete(id);
		this.items = this.items.map(m => m.id === id ? {...m, action_pending: true, action_error: undefined} : m);
		try {
			await msg.action.onAction();
			this.dismiss(id);
		} catch (error) {
			this.items = this.items.map(m => m.id === id ? {
				...m, action_pending: false,
				action_error: `操作を完了できませんでした: ${error instanceof Error ? error.message : String(error)}`,
			} : m);
		}
	}
}

export const toastStore = new ToastStore();

/**
 * Phase VI-γ-1: トップレベルのタブ構成。
 *
 * - v2 GUI: 利用 / 構成 / 接続と管理の作業単位で分類する。
 * - URL hash (`#live` 等) で永続化。ブラウザ戻る/進むと同期。
 * - シングルトンストアで全コンポーネントに共有。
 *
 * γ-2 以降で各タブの中身を詰めていくが、γ-1 ではタブ自体の骨組みを確定させて
 * 既存パネル群は Tools タブに仮配置し、表示経路を壊さないようにする。
 *
 * δ-9 D.5: V1 `Pipeline` タブは廃止。旧 `#pipeline` URL は `flowgraph` に fallback する。
 * GUI redesign: 旧 `#setup` / `#logs` / `#tools` は新 IA の対応タブへ fallback する。
 */

export const TABS = [
	{ id: "now", label: "ホーム", group: "利用" },
	{ id: "live", label: "ライブ", group: "利用" },
	{ id: "ai", label: "AIパートナー", group: "構成" },
	{ id: "flowgraph", label: "Flowgraph Studio", group: "構成" },
	{ id: "dictionary", label: "辞書", group: "構成" },
	{ id: "modes", label: "動作モード", group: "構成" },
	{ id: "resources", label: "連携", group: "接続と管理" },
	{ id: "observability", label: "診断", group: "接続と管理" },
	{ id: "settings", label: "設定", group: "接続と管理" },
] as const;

export type TabId = (typeof TABS)[number]["id"];

export type RepairFocus = {
	key: string;
	severity: "error" | "warning" | "info";
	message: string;
	impact?: string;
	resolution?: string;
	component?: string;
	code?: string;
	file?: string;
	node?: string | null;
};

const VALID_IDS: readonly TabId[] = TABS.map((t) => t.id);

function fromHash(): TabId {
	if (typeof window === "undefined") return "now";
	const h = window.location.hash.replace(/^#/, "").trim().toLowerCase();
	if (h === "setup") return "resources";
	if (h === "logs") return "observability";
	if (h === "tools") return "settings";
	if (h === "pipeline") return "flowgraph";
	return (VALID_IDS as readonly string[]).includes(h) ? (h as TabId) : "now";
}

class TabNavStore {
	private leaveGuards = new Set<() => boolean>();
	guardLeaving(guard: () => boolean): () => void {
		this.leaveGuards.add(guard);
		return () => this.leaveGuards.delete(guard);
	}
	active: TabId = $state(fromHash());
	canLeaveCurrent(): boolean {
		return [...this.leaveGuards].every((guard) => guard());
	}
	repairFocus: RepairFocus | null = $state(null);
	featureFocus: { id: string } | null = $state(null);

	constructor() {
		if (typeof window !== "undefined") {
			window.addEventListener("hashchange", () => {
				const next = fromHash();
				if (next !== this.active && [...this.leaveGuards].some((guard) => !guard())) {
					history.replaceState(null, "", `#${this.active}`);
					return;
				}
				this.active = next;
				this.repairFocus = null;
			});
		}
	}

	setActive(id: TabId, preserveRepairFocus = false): boolean {
		if (id !== this.active && [...this.leaveGuards].some((guard) => !guard())) return false;
		this.featureFocus = null;
		this.active = id;
		if (!preserveRepairFocus) this.repairFocus = null;
		if (typeof window !== "undefined") {
			// hash 更新は hashchange を自発的に再発火しないケースもあるのでストアは直接更新済み。
			const next = `#${id}`;
			if (window.location.hash !== next) {
				history.replaceState(null, "", next);
			}
		}
		return true;
	}

	openRepair(id: TabId, focus: RepairFocus): void {
		if (!this.setActive(id, true)) return;
		// ConfirmationItem は NowTab の derived 値なので、タブ破棄後も保持できる plain object にする。
		this.repairFocus = { ...focus };
	}

	openFeature(tab: TabId, id: string): void {
		if (!this.setActive(tab)) return;
		this.featureFocus = { id };
	}

	clearRepairFocus(): void {
		this.repairFocus = null;
	}
}

export const tabNavStore = new TabNavStore();

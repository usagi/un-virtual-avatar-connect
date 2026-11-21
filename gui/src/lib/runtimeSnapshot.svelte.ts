import { api } from "./api";
import { eventsStore } from "./events.svelte";
import type { StateSnapshot } from "./types";

const AUTO_REFRESH_MS = 10_000;

class RuntimeSnapshotStore {
	snapshot: StateSnapshot | null = $state(null);
	loading = $state(true);
	error: string | null = $state(null);

	#started = false;
	#inFlight: Promise<StateSnapshot> | null = null;

	start(): void {
		if (this.#started) return;
		this.#started = true;
		void this.refresh().catch(() => undefined);
		setInterval(() => {
			if (!document.hidden) void this.refresh().catch(() => undefined);
		}, AUTO_REFRESH_MS);
		document.addEventListener("visibilitychange", () => {
			if (!document.hidden) void this.refresh().catch(() => undefined);
		});
		eventsStore.subscribe((event) => {
			if (
				event.event.kind === "pause_state" ||
				event.event.kind === "voice_input_state" ||
				event.event.kind === "reloaded" ||
				event.event.kind === "oauth_status" ||
				event.event.kind === "managed_app_state" ||
				event.event.kind === "flowgraph_reloaded" ||
				event.event.kind === "runtime_mode_changed"
			) {
				void this.refresh().catch(() => undefined);
			}
		});
	}

	refresh(): Promise<StateSnapshot> {
		if (this.#inFlight !== null) return this.#inFlight;
		if (this.snapshot === null) this.loading = true;
		this.#inFlight = api
			.snapshot()
			.then((snapshot) => {
				this.snapshot = snapshot;
				this.error = null;
				return snapshot;
			})
			.catch((value: unknown) => {
				this.error = value instanceof Error ? value.message : String(value);
				throw value;
			})
			.finally(() => {
				this.loading = false;
				this.#inFlight = null;
			});
		return this.#inFlight;
	}
}

export const runtimeSnapshotStore = new RuntimeSnapshotStore();

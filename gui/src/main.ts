import { mount } from "svelte";
import "./app.css";
import "./styles/un-series.css";
import App from "./App.svelte";
import { eventsStore } from "./lib/events.svelte";
import { runtimeSnapshotStore } from "./lib/runtimeSnapshot.svelte";

document.documentElement.dataset.unProduct = "vac";
document.documentElement.dataset.unDesign ??= "hybrid";

// Control API の WebSocket を起動時に 1 本張る。
// 以降、各コンポーネントは eventsStore.recent / eventsStore.connection を参照するだけで良い。
// 失敗しても自動再接続が走るので、VAC 未起動でも GUI 自体は動く。
// Design fixtures do not run a VAC event stream. Never simulate a live connection.
if (import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1") eventsStore.connect();
runtimeSnapshotStore.start();

const app = mount(App, {
	target: document.getElementById("app")!,
});

export default app;

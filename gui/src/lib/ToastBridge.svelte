<script lang="ts">
	/**
	 * Control WebSocket のイベントを購読してトーストへ反映するブリッジコンポーネント。
	 *
	 * 別コンポーネントとして切り出す理由:
	 *   - トースト表示（`ToastLayer`）と通知発火（本ファイル）を独立に差し替えたい
	 *   - テスト時に `ToastLayer` だけマウントしても副作用が出ないようにする
	 *
	 * 反映するイベント（γ-1 時点）:
	 *   - `lagged`           → warn トースト（件数付き）
	 *   - `pause_state`      → info トースト（一括/部分）
	 *   - `reloaded`         → success トースト
	 *   - `oauth_status`     → tone を status に応じて切替
	 *   - `restarting`       → warn トースト（新 conf, 新 pid 表示）
	 *
	 * それ以外は EventStream パネル側で見ればよいので、ここでは通知しない。
	 */
	import { eventsStore } from "./events.svelte";
	import { toastStore } from "./toasts.svelte";
	import { confSyncStore } from "./confSync.svelte";

	$effect(() => {
		const off = eventsStore.subscribe((ts) => {
			const ev = ts.event;
			switch (ev.kind) {
				case "lagged": {
					toastStore.warn("WS が配信に追いついていません", `dropped=${ev.dropped}`);
					break;
				}
				case "pause_state": {
					const label = ev.paused ? "Paused" : "Resumed";
					toastStore.info(
						`${label} (${ev.target})`,
						`processors=[${ev.processors_affected.join(",")}] ais=[${ev.ais_affected.join(",")}]`
					);
					break;
				}
				case "voice_input_state": {
					toastStore.info(
						ev.worker_count === 0 ? "入力開始後の音声受付を変更した" : ev.muted ? "音声認識結果の受付を停止した" : "音声認識結果の受付を再開した",
						ev.worker_count === 0 ? `受付は${ev.muted ? "OFF" : "ON"}。マイクや音声認識は起動していない。` : `${ev.worker_count}件の音声入力が対象。マイク自体のミュートではない。`
					);
					break;
				}
				case "reloaded": {
					toastStore.success("Reloaded", `target=${ev.target}${ev.id ? ` id=${ev.id}` : ""}`);
					break;
				}
				case "oauth_status": {
					const tone = ev.status === "authorized" ? "success" : ev.status === "pending" ? "info" : "warn";
					toastStore.push({
						tone,
						title: `Twitch ${ev.account}: ${ev.status}`,
						detail: ev.view.last_error,
					});
					break;
				}
				case "restarting": {
					confSyncStore.requestRestart();
					toastStore.warn("VAC を再起動しています…", `new pid=${ev.new_pid} conf=${ev.new_conf_path}`);
					break;
				}
				case "heartbeat": {
					// 再起動後の最初の heartbeat で状態を synced に戻す
					if (confSyncStore.phase === "restart_in_flight") {
						confSyncStore.markSynced();
						toastStore.success("再起動完了", "新プロセスと接続しました。");
					}
					break;
				}
				default:
					break;
			}
		});
		return off;
	});
</script>

<!-- no DOM -->

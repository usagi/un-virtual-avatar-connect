<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "./api";
	import { runtimeSnapshotStore } from "./runtimeSnapshot.svelte";
	import { toastStore } from "./toasts.svelte";
	import { ControlApiError, type PreflightSelfTestStatus } from "./types";

	type RehearsalResult = {
		label: string;
		status: "ready" | "failed" | "skipped";
		detail?: string;
	};

	let status = $state<PreflightSelfTestStatus | null>(null);
	let selectedNodeId = $state("");
	let loading = $state(true);
	let ttsBusy = $state(false);
	let subtitleBusy = $state(false);
	let chatBusy = $state(false);
	let avatarBusy = $state(false);
	let rehearsalBusy = $state(false);
	let rehearsalStep = $state<string | null>(null);
	let rehearsalResults = $state<RehearsalResult[]>([]);
	let resultText = $state<string | null>(null);

	const activeNodes = $derived(status?.tts_nodes.filter((node) => node.active) ?? []);
	const subtitleChannels = $derived(status?.default_subtitle_channels ?? ["user", "ai-synth"]);
	const contract = $derived(status?.streaming_contract ?? null);
	const contractIssues = $derived(contract?.checks.filter((check) => check.status !== "ready") ?? []);
	const chatRoute = $derived(status?.chat_output_routes.length === 1 ? status.chat_output_routes[0] : null);
	const avatarTargets = $derived(status?.avatar_targets ?? []);

	function evidenceLabel(evidence: "untested" | "pending" | "ready" | "failed"): string {
		return { untested: "未確認", pending: "処理中", ready: "確認済み", failed: "失敗" }[evidence];
	}

	function contractLabel(value: "ready" | "attention" | "blocked" | "inactive"): string {
		return { ready: "正常", attention: "要確認", blocked: "修正が必要", inactive: "対象外" }[value];
	}

	function errorDetail(value: unknown): string {
		if (value instanceof ControlApiError) {
			if (typeof value.body === "object" && value.body !== null) {
				const body = value.body as Record<string, unknown>;
				const detail = body.reason ?? body.error;
				if (typeof detail === "string") return detail;
			}
			return `${value.status} ${value.statusText}`;
		}
		return value instanceof Error ? value.message : String(value);
	}

	async function load(): Promise<void> {
		loading = true;
		try {
			status = await api.preflightSelfTestStatus();
			if (!status.tts_nodes.some((node) => node.id === selectedNodeId && node.active)) {
				selectedNodeId = status.tts_nodes.find((node) => node.active)?.id ?? "";
			}
		} catch (value) {
			toastStore.error("セルフテストを読み込めなかった", errorDetail(value));
		} finally {
			loading = false;
		}
	}

	async function testSubtitles(): Promise<void> {
		if (subtitleBusy || rehearsalBusy) return;
		subtitleBusy = true;
		rehearsalResults = [];
		resultText = null;
		try {
			const response = await api.preflightSubtitleSelfTest({ channels: subtitleChannels, hold_ms: 2000 });
			resultText = `字幕確認完了: ${response.channels.join(", ")}`;
			toastStore.success("字幕を出力した", `${response.hold_ms / 1000}秒後に自動で消去`);
			await runtimeSnapshotStore.refresh();
		} catch (value) {
			const detail = errorDetail(value);
			resultText = `字幕確認失敗: ${detail}`;
			toastStore.error("字幕を確認できなかった", detail);
		} finally {
			subtitleBusy = false;
		}
	}

	async function testTts(): Promise<void> {
		if (ttsBusy || rehearsalBusy || selectedNodeId === "") return;
		ttsBusy = true;
		rehearsalResults = [];
		resultText = null;
		try {
			const response = await api.preflightTtsSelfTest({ node_id: selectedNodeId });
			await runtimeSnapshotStore.refresh();
			await load();
			if (!response.completed || !response.played) {
				throw new Error(response.error ?? "音声再生の完了を確認できなかった");
			}
			resultText = `音声確認完了: ${response.elapsed_ms}ms`;
			toastStore.success("テスト音声を再生した", `${response.elapsed_ms}ms`);
		} catch (value) {
			const detail = errorDetail(value);
			resultText = `音声確認失敗: ${detail}`;
			toastStore.error("音声を確認できなかった", detail);
			await runtimeSnapshotStore.refresh().catch(() => undefined);
			await load().catch(() => undefined);
		} finally {
			ttsBusy = false;
		}
	}

	async function testChatOutput(): Promise<void> {
		if (chatBusy || rehearsalBusy || chatRoute === null) return;
		chatBusy = true;
		rehearsalResults = [];
		resultText = null;
		try {
			const response = await api.preflightChatOutputSelfTest();
			if (!response.subtitle_reached || !response.completed || !response.played) {
				throw new Error(response.error ?? "字幕と音声の両方へ到達しなかった");
			}
			resultText = `コメント経路確認完了: ${response.elapsed_ms}ms`;
			toastStore.success("コメント読み上げを確認した", "字幕と音声の両方へ到達");
			await runtimeSnapshotStore.refresh();
		} catch (value) {
			const detail = errorDetail(value);
			resultText = `コメント経路確認失敗: ${detail}`;
			toastStore.error("コメント読み上げを確認できなかった", detail);
		} finally {
			chatBusy = false;
		}
	}

	async function testAvatarMotion(): Promise<void> {
		if (avatarBusy || rehearsalBusy || avatarTargets.length === 0) return;
		avatarBusy = true;
		rehearsalResults = [];
		resultText = null;
		try {
			const response = await api.preflightAvatarMotionSelfTest();
			const ready = response.renderers.find(
				(renderer) =>
					renderer.connected &&
					renderer.unmotion_zenoh_enabled &&
					renderer.key_matches &&
					renderer.received_delta > 0 &&
					renderer.applied_delta > 0
			);
			if (!ready) {
				const error = response.renderers.find((renderer) => renderer.error)?.error;
				throw new Error(error ?? "RendererでUNMF/Zの受信・反映を確認できなかった");
			}
			resultText = `アバター動作確認完了: 受信 +${ready.received_delta} / 適用 +${ready.applied_delta} frame`;
			toastStore.success("アバター動作を確認した", ready.unmotion_zenoh_key ?? `PID ${ready.pid}`);
			await runtimeSnapshotStore.refresh();
		} catch (value) {
			const detail = errorDetail(value);
			resultText = `アバター動作確認失敗: ${detail}`;
			toastStore.error("アバター動作を確認できなかった", detail);
		} finally {
			avatarBusy = false;
		}
	}

	async function runRehearsal(): Promise<void> {
		if (rehearsalBusy || loading || status === null) return;
		rehearsalBusy = true;
		resultText = null;
		rehearsalResults = [];
		const current = status;
		const completed: string[] = [];
		const failures: string[] = [];
		const runStep = async (label: string, operation: () => Promise<void>): Promise<void> => {
			rehearsalStep = label;
			try {
				await operation();
				completed.push(label);
				rehearsalResults = [...rehearsalResults, { label, status: "ready" }];
			} catch (value) {
				const detail = errorDetail(value);
				failures.push(`${label}: ${detail}`);
				rehearsalResults = [...rehearsalResults, { label, status: "failed", detail }];
			}
		};

		try {
			await runStep("字幕", async () => {
				await api.preflightSubtitleSelfTest({ channels: current.default_subtitle_channels, hold_ms: 2000 });
			});

			const chatTtsNodeId = current.chat_output_routes.length === 1 ? current.chat_output_routes[0].tts_node_id : null;
			const directTtsNodes = current.tts_nodes.filter((candidate) => candidate.active && candidate.id !== chatTtsNodeId);
			for (const node of directTtsNodes) {
				await runStep(`音声合成 ${node.id}`, async () => {
					const response = await api.preflightTtsSelfTest({ node_id: node.id });
					if (!response.completed || !response.played) {
						throw new Error(response.error ?? "音声再生の完了を確認できなかった");
					}
				});
			}
			if (directTtsNodes.length === 0) {
				rehearsalResults = [...rehearsalResults, { label: "個別の音声合成", status: "skipped", detail: "有効な経路なし" }];
			}

			if (current.chat_output_routes.length === 1) {
				await runStep("コメント読み上げ", async () => {
					const response = await api.preflightChatOutputSelfTest();
					if (!response.subtitle_reached || !response.completed || !response.played) {
						throw new Error(response.error ?? "字幕と音声の両方へ到達しなかった");
					}
				});
			} else if (current.chat_output_routes.length > 1) {
				const detail = `有効な経路が${current.chat_output_routes.length}本ある`;
				failures.push(`コメント読み上げ: ${detail}`);
				rehearsalResults = [...rehearsalResults, { label: "コメント読み上げ", status: "failed", detail }];
			} else {
				rehearsalResults = [...rehearsalResults, { label: "コメント読み上げ", status: "skipped", detail: "未構成" }];
			}

			if (current.avatar_targets.length > 0) {
				await runStep("AIアバター", async () => {
					const response = await api.preflightAvatarMotionSelfTest();
					const ready = response.renderers.some(
						(renderer) =>
							renderer.connected &&
							renderer.unmotion_zenoh_enabled &&
							renderer.key_matches &&
							renderer.received_delta > 0 &&
							renderer.applied_delta > 0
					);
					if (!ready) {
						const error = response.renderers.find((renderer) => renderer.error)?.error;
						throw new Error(error ?? "RendererでUNMF/Zの受信・反映を確認できなかった");
					}
				});
			} else {
				rehearsalResults = [...rehearsalResults, { label: "AIアバター", status: "skipped", detail: "未構成" }];
			}

			if (failures.length === 0) {
				resultText = `一括確認完了: ${completed.length}項目`;
				toastStore.success("配信経路を一括確認した", `${completed.length}項目が完了`);
			} else {
				resultText = `一括確認: ${failures.length}件の失敗を検出`;
				toastStore.error("一括確認で問題を検出した", failures[0]);
			}
		} finally {
			rehearsalStep = null;
			await runtimeSnapshotStore.refresh().catch(() => undefined);
			await load().catch(() => undefined);
			rehearsalBusy = false;
		}
	}

	onMount(() => {
		void load();
	});
</script>

<section class="vac-panel" aria-label="配信前セルフテスト">
	<header class="vac-panel-header flex items-center justify-between gap-3 px-4 py-2.5">
		<h3 class="text-sm font-semibold">配信前セルフテスト</h3>
		<div class="flex items-center gap-2">
			<button
				type="button"
				class="rounded border border-primary-500 px-2.5 py-1 text-xs text-primary-600-400 hover:bg-primary-500/10 disabled:opacity-50"
				disabled={loading || ttsBusy || subtitleBusy || chatBusy || avatarBusy || rehearsalBusy || status === null}
				onclick={() => void runRehearsal()}
			>
				{rehearsalBusy ? "一括確認中…" : "音声を含む一括確認"}
			</button>
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2.5 py-1 text-xs hover:bg-surface-100-900 disabled:opacity-50"
				disabled={loading || ttsBusy || subtitleBusy || chatBusy || avatarBusy || rehearsalBusy}
				onclick={() => void load()}
			>
				{loading ? "確認中…" : "再読込"}
			</button>
		</div>
	</header>

	{#if contract && contract.status !== "inactive"}
		<div class="flex flex-wrap items-start justify-between gap-2 border-b border-surface-200-800 px-4 py-2.5 text-xs">
			<div>
				<span class="font-medium">配信経路</span>
				<span
					class:text-success-600-400={contract.status === "ready"}
					class:text-warning-600-400={contract.status === "attention"}
					class:text-error-600-400={contract.status === "blocked"}
					class="ml-2"
				>
					{contractLabel(contract.status)}
				</span>
			</div>
			{#if contractIssues.length > 0}
				<div class="min-w-0 max-w-2xl text-right opacity-70">{contractIssues.map((check) => check.detail).join(" / ")}</div>
			{/if}
		</div>
	{/if}

	<div class="grid divide-y divide-surface-200-800 xl:grid-cols-4 xl:divide-x xl:divide-y-0">
		<div class="flex min-h-20 flex-wrap items-center gap-3 px-4 py-3">
			<div class="min-w-0 flex-1">
				<p class="text-sm font-medium">字幕出力</p>
				<p class="mt-0.5 truncate text-xs opacity-60">{subtitleChannels.join(" / ")}</p>
			</div>
			<button
				type="button"
				class="rounded border border-primary-500 px-3 py-1.5 text-xs text-primary-600-400 hover:bg-primary-500/10 disabled:opacity-50"
				disabled={loading || subtitleBusy || rehearsalBusy}
				onclick={() => void testSubtitles()}
			>
				{subtitleBusy ? "表示中…" : "字幕を2秒表示"}
			</button>
		</div>

		<div class="grid min-h-20 gap-3 px-4 py-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
			<label class="min-w-0">
				<span class="block text-sm font-medium">音声合成</span>
				<select
					class="mt-1 w-full min-w-0 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs disabled:opacity-50"
					bind:value={selectedNodeId}
					disabled={loading || ttsBusy || rehearsalBusy || activeNodes.length === 0}
				>
					{#if activeNodes.length === 0}
						<option value="">有効な音声経路なし</option>
					{:else}
						{#each activeNodes as node (node.id)}
							<option value={node.id}>{node.id} · {evidenceLabel(node.evidence)}</option>
						{/each}
					{/if}
				</select>
			</label>
			<button
				type="button"
				class="justify-self-start rounded border border-primary-500 px-3 py-1.5 text-xs text-primary-600-400 hover:bg-primary-500/10 disabled:opacity-50 sm:justify-self-auto"
				disabled={loading || ttsBusy || rehearsalBusy || selectedNodeId === ""}
				onclick={() => void testTts()}
			>
				{ttsBusy ? "再生確認中…" : "テスト音声を再生"}
			</button>
		</div>

		<div class="flex min-h-20 flex-wrap items-center gap-3 px-4 py-3">
			<div class="min-w-0 flex-1">
				<p class="text-sm font-medium">コメント読み上げ</p>
				<p class="mt-0.5 truncate text-xs opacity-60">
					{chatRoute ? "chat → 字幕・音声" : "有効な経路が1本必要"}
				</p>
			</div>
			<button
				type="button"
				class="rounded border border-primary-500 px-3 py-1.5 text-xs text-primary-600-400 hover:bg-primary-500/10 disabled:opacity-50"
				disabled={loading || chatBusy || rehearsalBusy || chatRoute === null}
				onclick={() => void testChatOutput()}
			>
				{chatBusy ? "経路確認中…" : "字幕と音声を確認"}
			</button>
		</div>

		<div class="flex min-h-20 flex-wrap items-center gap-3 px-4 py-3">
			<div class="min-w-0 flex-1">
				<p class="text-sm font-medium">AIアバター</p>
				<p class="mt-0.5 truncate text-xs opacity-60">
					{avatarTargets.length > 0 ? avatarTargets.map((target) => target.target_link_id).join(" / ") : "動作経路なし"}
				</p>
			</div>
			<button
				type="button"
				class="rounded border border-primary-500 px-3 py-1.5 text-xs text-primary-600-400 hover:bg-primary-500/10 disabled:opacity-50"
				disabled={loading || avatarBusy || rehearsalBusy || avatarTargets.length === 0}
				onclick={() => void testAvatarMotion()}
			>
				{avatarBusy ? "受信確認中…" : "動作を確認"}
			</button>
		</div>
	</div>

	{#if rehearsalBusy && rehearsalStep}
		<p class="border-t border-surface-200-800 px-4 py-2 text-xs" aria-live="polite">確認中: {rehearsalStep}</p>
	{:else if resultText}
		<p class="border-t border-surface-200-800 px-4 py-2 text-xs font-medium" aria-live="polite">{resultText}</p>
	{/if}
	{#if rehearsalResults.length > 0}
		<ul class="divide-y divide-surface-200-800 border-t border-surface-200-800 text-xs">
			{#each rehearsalResults as result (result.label)}
				<li class="grid grid-cols-[4.5rem_minmax(0,1fr)] gap-2 px-4 py-2">
					<span
						class:text-success-600-400={result.status === "ready"}
						class:text-error-600-400={result.status === "failed"}
						class:opacity-60={result.status === "skipped"}
					>
						{{ ready: "確認済み", failed: "失敗", skipped: "対象外" }[result.status]}
					</span>
					<span class="min-w-0 break-words">
						{result.label}{result.detail ? `: ${result.detail}` : ""}
					</span>
				</li>
			{/each}
		</ul>
	{/if}
</section>

<script lang="ts">
	/**
	 * BGM / screen effect quick queue.
	 *
	 * Sends structured ChannelDatum events and tracks them in the Control API
	 * quick-effect queue for cancel / duration visibility.
	 */
	import { onDestroy, onMount } from "svelte";
	import { api } from "./api";
	import { ControlApiError, type QuickEffectEnqueueRequest, type QuickEffectItem, type QuickEffectPreset } from "./types";
	import { toastStore } from "./toasts.svelte";

	const fallbackPresets: QuickEffectPreset[] = [
		{ id: "raid", label: "RAID", channel: "effect", content: "RAID incoming", flags: ["obs.raid"], duration_ms: 8000 },
		{
			id: "chat_highlight",
			label: "チャット強調",
			channel: "chat",
			content: "highlight",
			flags: ["obs.chat.highlight"],
			duration_ms: 5000,
		},
		{ id: "scene_flash", label: "画面効果", channel: "effect", content: "scene flash", flags: ["obs.effect.flash"], duration_ms: 2500 },
		{ id: "bgm_fade", label: "BGM fade", channel: "bgm", content: "fade", flags: ["audio.bgm.fade"], duration_ms: 12000 },
	];

	let presets = $state<QuickEffectPreset[]>(fallbackPresets);
	let presetError = $state<string | null>(null);
	let presetsReady = $state(false);
	let presetRevision = $state<string | null>(null);
	let presetsLoading = $state(false);
	let customLabel = $state("custom");
	let channel = $state("effect");
	let content = $state("");
	let flagsText = $state("control.quick_effect");
	let durationMs = $state(5000);
	let delayMs = $state(0);
	let busy = $state<string | null>(null);
	let queue = $state<QuickEffectItem[]>([]);
	let queueError = $state<string | null>(null);
	let timer: ReturnType<typeof setInterval> | null = null;
	let queueRequest = 0;

	onMount(() => {
		void refreshQueue();
		void refreshPresets();
		timer = setInterval(() => void refreshQueue(), 5000);
	});

	onDestroy(() => {
		if (timer !== null) clearInterval(timer);
	});

	async function refreshQueue() {
		const request = ++queueRequest;
		const queueAtRequest = queue;
		try {
			const res = await api.quickEffects(12);
			if (request !== queueRequest || queue !== queueAtRequest) return;
			queue = res.effects;
			queueError = null;
		} catch (e) {
			if (request !== queueRequest || queue !== queueAtRequest) return;
			queueError = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
		}
	}

	async function refreshPresets() {
		if (presetsLoading || busy !== null) return;
		presetsLoading = true;
		presetsReady = false;
		try {
			const res = await api.quickEffectPresets();
			presets = res.presets.length > 0 ? res.presets : fallbackPresets;
			presetError = null;
			presetRevision = res.revision ?? null;
			presetsReady = presetRevision !== null;
			if (!presetsReady) presetError = "保存先の世代を確認できません。接続先とバージョンを確認してください";
		} catch (e) {
			presetError = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
			presets = fallbackPresets;
		} finally {
			presetsLoading = false;
		}
	}

	async function sendPreset(preset: QuickEffectPreset) {
		if (busy !== null) return;
		busy = preset.id;
		await send({
			label: preset.label,
			req: {
				label: preset.label,
				channel: preset.channel,
				content: preset.content,
				flags: preset.flags,
				duration_ms: preset.duration_ms,
				fire_at_ms: scheduledFireAtMs(),
				meta: { preset: preset.id },
			},
		});
		busy = null;
	}

	async function sendCustom() {
		const ch = channel.trim();
		if (!ch) {
			toastStore.error("送信できません", "channel が空です");
			return;
		}
		busy = "custom";
		await send({
			label: customLabel.trim() || "custom",
			req: {
				label: customLabel.trim() || "custom",
				channel: ch,
				content,
				flags: parseFlags(flagsText),
				duration_ms: durationMs,
				fire_at_ms: scheduledFireAtMs(),
				meta: { custom: true },
			},
		});
		busy = null;
	}

	async function saveCustomPreset() {
		if (busy !== null) return;
		if (!presetsReady || !presetRevision || presetsLoading) {
			toastStore.error("保存できません", "既存のプリセットを読み込んでから保存してください。入力内容は保持しています。");
			return;
		}
		const label = customLabel.trim() || "custom";
		const ch = channel.trim();
		if (!ch) {
			toastStore.error("保存できません", "channel が空です");
			return;
		}
		const preset: QuickEffectPreset = {
			id: presetIdFromLabel(label),
			label,
			channel: ch,
			content,
			flags: parseFlags(flagsText),
			duration_ms: Number(durationMs) > 0 ? Number(durationMs) : null,
		};
		const next = [...presets.filter((p) => p.id !== preset.id), preset];
		busy = "save_preset";
		try {
			const res = await api.putQuickEffectPresets({ presets: next, expected_revision: presetRevision });
			presets = res.presets;
			presetRevision = res.revision ?? null;
			presetsReady = presetRevision !== null;
			presetError = presetsReady ? null : "保存先の世代を確認できません。再読み込みしてください";
			toastStore.success("Quick Effects preset を保存しました", res.backup ? `backup: ${res.backup}` : label);
		} catch (e) {
			const message = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
			if (e instanceof ControlApiError && e.status === 409) {
				presetsReady = false;
				const locked = typeof e.body === "object" && e.body !== null && "error" in e.body && e.body.error === "profile_busy";
				presetError = locked
					? "別の保存処理が進行中です。入力内容は保持しています。処理が終わってから再読み込みし、保存内容を確認してください"
					: "別の操作で設定が変更されました。再読み込み後に保存内容を確認してください";
			}
			toastStore.error("preset 保存に失敗しました", message);
		} finally {
			busy = null;
		}
	}

	async function send({ label, req }: { label: string; req: QuickEffectEnqueueRequest }) {
		try {
			const res = await api.enqueueQuickEffect(req);
			queue = [res, ...queue.filter((item) => item.id !== res.id)].slice(0, 12);
			toastStore.success("演出イベントを送信しました", `${label} -> ${res.channel}`);
			if (label === "custom" && content === req.content) content = "";
		} catch (e) {
			const message = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
			toastStore.error("演出イベント送信に失敗しました", message);
		}
	}

	async function cancelEffect(item: QuickEffectItem) {
		busy = item.id;
		try {
			const cancelled = await api.cancelQuickEffect(item.id);
			queue = queue.map((it) => (it.id === cancelled.id ? cancelled : it));
		} catch (e) {
			const message = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
			toastStore.error("演出キャンセルに失敗しました", message);
		} finally {
			busy = null;
		}
	}

	async function moveEffect(item: QuickEffectItem, direction: "up" | "down") {
		busy = `${item.id}:${direction}`;
		try {
			const res = await api.moveQuickEffect(item.id, { direction });
			queue = res.effects.slice(-12);
		} catch (e) {
			const message = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
			toastStore.error("演出キューの並べ替えに失敗しました", message);
		} finally {
			busy = null;
		}
	}

	function statusClass(item: QuickEffectItem): string {
		switch (item.status) {
			case "scheduled":
				return "text-primary-500";
			case "active":
				return "text-success-500";
			case "expired":
				return "text-surface-500";
			case "cancelled":
				return "text-warning-500";
		}
	}

	function scheduledFireAtMs(): number | null {
		const delay = Number(delayMs);
		if (!Number.isFinite(delay) || delay <= 0) return null;
		return Date.now() + Math.round(delay);
	}

	function parseFlags(raw: string): string[] {
		return raw
			.split(",")
			.map((flag) => flag.trim())
			.filter(Boolean);
	}

	function presetIdFromLabel(label: string): string {
		return (
			label
				.trim()
				.toLowerCase()
				.replace(/[^a-z0-9_-]+/g, "_")
				.replace(/^_+|_+$/g, "") || "custom"
		);
	}

	function itemTimeLabel(item: QuickEffectItem): string {
		const ms = item.status === "scheduled" && item.fire_at_ms ? item.fire_at_ms : Date.parse(item.created_at);
		return new Date(ms).toLocaleTimeString("ja-JP", { hour12: false });
	}
</script>

<section class="vac-panel p-4" aria-label="演出">
	<header class="mb-3 flex flex-wrap items-start justify-between gap-3">
		<h3 class="text-sm font-semibold">演出</h3>
		<div class="flex items-center gap-2">
			<span class="rounded bg-surface-200-800 px-2 py-1 text-xs">待機 {queue.length}</span>
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800"
				onclick={() => void refreshQueue()}
			>
				更新
			</button>
		</div>
	</header>

	<div class="grid grid-cols-2 gap-2 sm:grid-cols-4" role="group" aria-label="Quick effect presets">
		{#each presets as preset (preset.id)}
			<button
				type="button"
				class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-2 text-xs font-semibold hover:bg-surface-200-800 disabled:opacity-50"
				onclick={() => void sendPreset(preset)}
				disabled={busy !== null}
				title={`${preset.channel}: ${preset.flags.join(", ")}`}
			>
				{busy === preset.id ? "…" : preset.label}
			</button>
		{/each}
	</div>
	{#if presetError}
		<div class="mt-2 rounded border border-warning-500/40 bg-warning-500/10 p-2 text-xs">
			<p>プリセットを読み込めませんでした: {presetError}。保存は読み込み成功後に再開できます。入力内容は保持しています。</p>
			<button type="button" class="mt-2 rounded border px-2 py-1 disabled:opacity-50" disabled={presetsLoading || busy !== null} onclick={() => void refreshPresets()}>{presetsLoading ? "読み込み中…" : "再読み込み"}</button>
		</div>
	{/if}

	<details class="mt-3 border-t border-surface-200-800 pt-2">
		<summary class="cursor-pointer text-xs font-semibold opacity-70">カスタム演出</summary>
		<div class="mt-3 grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
			<label class="flex flex-col text-xs">
				<span class="opacity-70">label</span>
				<input
					type="text"
					class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
					bind:value={customLabel}
				/>
			</label>
			<label class="flex flex-col text-xs">
				<span class="opacity-70">channel</span>
				<input
					type="text"
					class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 font-mono text-sm"
					bind:value={channel}
				/>
			</label>
			<label class="flex flex-col text-xs">
				<span class="opacity-70">duration_ms</span>
				<input
					type="number"
					min="0"
					step="500"
					class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 font-mono text-sm"
					bind:value={durationMs}
				/>
			</label>
			<label class="flex flex-col text-xs">
				<span class="opacity-70">delay_ms</span>
				<input
					type="number"
					min="0"
					step="500"
					class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 font-mono text-sm"
					bind:value={delayMs}
				/>
			</label>
			<label class="flex flex-col text-xs">
				<span class="opacity-70">content</span>
				<input
					type="text"
					class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
					bind:value={content}
					placeholder="custom effect payload"
				/>
			</label>
			<label class="flex flex-col text-xs">
				<span class="opacity-70">flags</span>
				<input
					type="text"
					class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 font-mono text-sm"
					bind:value={flagsText}
					placeholder="comma,separated"
				/>
			</label>
			<div class="flex items-end gap-2 sm:col-span-2 xl:col-span-4">
				<button
					type="button"
					class="rounded bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
					onclick={() => void sendCustom()}
					disabled={busy !== null}
				>
					{busy === "custom" ? "送信中…" : "送信"}
				</button>
				<button
					type="button"
					class="rounded border border-primary-500/60 px-3 py-1.5 text-xs font-semibold hover:bg-primary-500/10 disabled:opacity-50"
					onclick={() => void saveCustomPreset()}
					disabled={busy !== null || !presetsReady || presetsLoading}
				>
					{busy === "save_preset" ? "保存中…" : "Preset 保存"}
				</button>
			</div>
		</div>
	</details>

	{#if queueError}
		<p class="mt-3 rounded border border-error-500/40 bg-error-500/10 p-2 text-xs">queue 取得失敗: {queueError}</p>
	{/if}

	{#if queue.length > 0}
		<ul class="mt-3 space-y-1 text-[11px]">
			{#each queue as item (item.id)}
				<li class="flex items-center gap-2 rounded bg-surface-50-950 px-2 py-1">
					<span class="font-mono opacity-60">{itemTimeLabel(item)}</span>
					<span class="rounded bg-surface-200-800 px-1.5 py-0.5 font-semibold">{item.label}</span>
					<span class="font-mono opacity-70">{item.channel}</span>
					<span class="font-mono opacity-70">{item.duration_ms ?? 0}ms</span>
					<span class="ml-auto font-mono {statusClass(item)}">{item.status}</span>
					<button
						type="button"
						class="rounded border border-surface-300-700 px-1.5 py-0.5 hover:bg-surface-200-800 disabled:opacity-50"
						title="move up"
						onclick={() => void moveEffect(item, "up")}
						disabled={busy !== null}
					>
						↑
					</button>
					<button
						type="button"
						class="rounded border border-surface-300-700 px-1.5 py-0.5 hover:bg-surface-200-800 disabled:opacity-50"
						title="move down"
						onclick={() => void moveEffect(item, "down")}
						disabled={busy !== null}
					>
						↓
					</button>
					{#if item.status === "active" || item.status === "scheduled"}
						<button
							type="button"
							class="rounded border border-surface-300-700 px-1.5 py-0.5 hover:bg-surface-200-800 disabled:opacity-50"
							onclick={() => void cancelEffect(item)}
							disabled={busy !== null}
						>
							{busy === item.id ? "…" : "cancel"}
						</button>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</section>

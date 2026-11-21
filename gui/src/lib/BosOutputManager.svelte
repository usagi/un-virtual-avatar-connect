<script lang="ts">
	/**
	 * OBS output management surface.
	 *
	 * BOS Preview is focused on one selected source. This panel keeps the whole
	 * browser-source set visible so stream setup can verify URL inventory quickly.
	 */
	import { SvelteMap, SvelteURLSearchParams } from "svelte/reactivity";
	import { api } from "./api";
	import { ControlApiError, type BosCategory, type BosEntry, type BosResponse } from "./types";
	import { toastStore } from "./toasts.svelte";

	const categoryOrder: BosCategory[] = ["subtitles", "effects", "bgm", "other"];

	let loading = $state(true);
	let error = $state<string | null>(null);
	let resp = $state<BosResponse | null>(null);
	let channel = $state("user,ai-synth");
	let activeCategory = $state<BosCategory | "all">("all");

	const origin = typeof window !== "undefined" ? window.location.origin : "";

	async function load() {
		loading = true;
		error = null;
		try {
			resp = await api.bos();
		} catch (e) {
			error = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void load();
	});

	const entries = $derived(resp?.entries ?? []);
	const categoryCounts = $derived.by(() => {
		const counts = new SvelteMap<BosCategory, number>();
		for (const category of categoryOrder) counts.set(category, 0);
		for (const entry of entries) counts.set(entry.category, (counts.get(entry.category) ?? 0) + 1);
		return counts;
	});
	const filteredEntries = $derived(activeCategory === "all" ? entries : entries.filter((entry) => entry.category === activeCategory));

	function categoryLabel(category: BosCategory | "all"): string {
		switch (category) {
			case "all":
				return "全部";
			case "subtitles":
				return "字幕";
			case "effects":
				return "演出";
			case "bgm":
				return "BGM";
			case "other":
				return "その他";
		}
	}

	function sourceKind(entry: BosEntry): string {
		if (entry.url.startsWith("/obs-template/")) return "template";
		return entry.user_defined ? "user" : "bundled";
	}

	function composedUrl(entry: BosEntry): string {
		const params = new SvelteURLSearchParams();
		if (entry.supports_channel_param) {
			const ch = channel.trim();
			if (ch !== "") params.set(entry.url.startsWith("/obs-template/") && ch.includes(",") ? "channels" : "channel", ch);
		}
		const qs = params.toString();
		if (qs === "") return entry.url;
		return `${entry.url}${entry.url.includes("?") ? "&" : "?"}${qs}`;
	}

	function absoluteUrl(entry: BosEntry): string {
		return `${origin}${composedUrl(entry)}`;
	}

	async function copyUrl(entry: BosEntry) {
		const url = absoluteUrl(entry);
		try {
			if (navigator.clipboard?.writeText) {
				await navigator.clipboard.writeText(url);
			} else {
				const ta = document.createElement("textarea");
				ta.value = url;
				ta.style.position = "fixed";
				ta.style.opacity = "0";
				document.body.appendChild(ta);
				ta.select();
				document.execCommand("copy");
				document.body.removeChild(ta);
			}
			toastStore.success("URL をコピーしました", entry.title);
		} catch (e) {
			toastStore.error("コピーに失敗しました", e instanceof Error ? e.message : String(e));
		}
	}
</script>

<section class="rounded-lg border border-surface-200-800 bg-surface-100-900 p-4">
	<header class="mb-3 flex flex-wrap items-start justify-between gap-3">
		<div>
			<h2 class="text-sm font-semibold">OBS 出力管理</h2>
			<p class="text-xs opacity-60">OBSのブラウザーソースに設定するURLを確認・コピーします。</p>
		</div>
		<div class="flex items-center gap-2">
			<span class="rounded bg-surface-200-800 px-2 py-1 text-xs">{entries.length} 件</span>
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800"
				onclick={() => void load()}
				disabled={loading}
			>
				更新
			</button>
		</div>
	</header>

	<div class="mb-3 flex flex-wrap items-end gap-3">
		<label class="flex flex-col">
			<span class="text-xs opacity-70">表示する出力チャンネル</span>
			<input
				type="text"
				class="w-32 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
				bind:value={channel}
				placeholder="user,ai-synth"
				aria-describedby="bos-output-channel-help"
			/>
		</label>
		<div class="flex flex-wrap gap-1" role="group" aria-label="BOS category filter">
			<button
				type="button"
				class="rounded border px-2 py-1 text-xs {activeCategory === 'all'
					? 'border-primary-500 bg-primary-500 text-white'
					: 'border-surface-300-700 hover:bg-surface-200-800'}"
				aria-pressed={activeCategory === "all"}
				onclick={() => (activeCategory = "all")}
			>
				{categoryLabel("all")}
				{entries.length}
			</button>
			{#each categoryOrder as category (category)}
				<button
					type="button"
					class="rounded border px-2 py-1 text-xs {activeCategory === category
						? 'border-primary-500 bg-primary-500 text-white'
						: 'border-surface-300-700 hover:bg-surface-200-800'}"
					aria-pressed={activeCategory === category}
					onclick={() => (activeCategory = category)}
				>
					{categoryLabel(category)}
					{categoryCounts.get(category) ?? 0}
				</button>
			{/each}
		</div>
	</div>
	<p id="bos-output-channel-help" class="mb-3 text-xs opacity-70">Twitchの配信者名ではなく、Flowgraphの画面出力先を指定します。通知・クリップの雛形は <code>stream-alerts</code>。この欄へ入力し、「OBS Template Effects」のURLをOBSへコピーしてください。入力で変わるのは下のURLだけで、Flowgraphや既存のOBS設定は変更しません。</p>

	{#if loading}
		<p class="text-xs opacity-70">読み込み中…</p>
	{:else if error}
		<p class="rounded border border-error-500/40 bg-error-500/10 p-2 text-xs">
			BOS 一覧の取得に失敗しました: {error}
		</p>
	{:else if filteredEntries.length === 0}
		<p class="rounded border border-warning-500/40 bg-warning-500/10 p-2 text-xs">表示対象の OBS 出力はありません。</p>
	{:else}
		<div class="grid gap-2">
			{#each filteredEntries as entry (entry.id)}
				<article class="rounded border border-surface-300-700 bg-surface-50-950 p-2">
					<div class="flex flex-wrap items-start justify-between gap-2">
						<div class="min-w-0">
							<div class="flex flex-wrap items-center gap-1">
								<h3 class="truncate text-sm font-semibold">{entry.title}</h3>
								<span class="rounded bg-surface-200-800 px-1.5 py-0.5 text-[10px] uppercase"
									>{categoryLabel(entry.category)}</span
								>
								<span class="rounded bg-surface-200-800 px-1.5 py-0.5 text-[10px] uppercase">{sourceKind(entry)}</span>
							</div>
							<p class="mt-1 break-all font-mono text-[11px] opacity-70">{composedUrl(entry)}</p>
						</div>
						<div class="flex shrink-0 gap-1">
							<button
								type="button"
								class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800"
								onclick={() => void copyUrl(entry)}
							>
								URL
							</button>
							<a
								class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800"
								href={absoluteUrl(entry)}
								target="_blank"
								rel="noopener noreferrer"
							>
								開く
							</a>
						</div>
					</div>
					<dl class="mt-2 grid gap-x-3 gap-y-1 text-[11px] opacity-65 sm:grid-cols-[auto_minmax(0,1fr)_auto_minmax(0,1fr)]">
						<dt class="font-semibold">ID</dt>
						<dd class="min-w-0 break-all font-mono">{entry.id}</dd>
						<dt class="font-semibold">Path</dt>
						<dd class="min-w-0 break-all font-mono">{entry.path}</dd>
					</dl>
				</article>
			{/each}
		</div>
	{/if}
</section>

<script lang="ts">
	/**
	 * Phase δ-6e: ノードパレット（右上）。
	 *
	 * - `flowgraphStore.catalog` をカテゴリ別にグルーピングして表示。
	 * - 検索ボックスで title / feature / category に対するインクリメンタル絞り込み。
	 * - クリックで「現在のキャンバス中央付近」に new node を追加（`flowgraphStore.addCatalogNodeAt`）。
	 * - Phase ο-6: カテゴリ単位の表示/非表示（localStorage 永続化）、HTML5 DnD で feature を dataTransfer に載せる。
	 */
	import { onMount } from "svelte";
	import { SvelteSet } from "svelte/reactivity";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	import type { FlowgraphNodeSpec } from "../types";
	import { capabilitySummary, effectClassLabel, effectTooltip } from "./effectMetadata";
	import { filterPalette } from "./paletteFilter";

	const LS_HIDDEN = "vac-flowgraph-palette-hidden-categories";

	function shortFeature(feature: string): string {
		const parts = feature.split(".");
		if (parts.length <= 1) return feature;
		return parts[parts.length - 1];
	}

	function effectBadgeClass(effectClass: FlowgraphNodeSpec["effect_class"]): string {
		const base = "rounded border px-1 py-px text-[0.58rem] font-semibold uppercase leading-none";
		switch (effectClass) {
			case "pure":
				return `${base} border-emerald-500/40 bg-emerald-500/10 text-emerald-700`;
			case "stateful":
				return `${base} border-sky-500/40 bg-sky-500/10 text-sky-700`;
			case "effectful":
				return `${base} border-amber-500/50 bg-amber-500/10 text-amber-700`;
			default:
				return `${base} border-surface-300-700 bg-surface-100-900 text-surface-600-400`;
		}
	}

	let search = $state("");
	const searchLower = $derived(search.trim().toLowerCase());
	const userEnumPaletteExtra = $derived(flowgraphStore.currentFile?.parsed?.enums?.reduce((s, e) => s + e.variants.length, 0) ?? 0);

	/** 非表示カテゴリ（`spec.category` 文字列キー）。 */
	const hiddenCategories = new SvelteSet<string>();

	onMount(() => {
		try {
			const raw = localStorage.getItem(LS_HIDDEN);
			if (!raw) return;
			const arr = JSON.parse(raw) as unknown;
			if (Array.isArray(arr)) {
				hiddenCategories.clear();
				for (const x of arr) {
					if (typeof x === "string") hiddenCategories.add(x);
				}
			}
		} catch {
			/* ignore */
		}
	});

	function persistHidden(): void {
		try {
			localStorage.setItem(LS_HIDDEN, JSON.stringify([...hiddenCategories]));
		} catch {
			/* ignore */
		}
	}

	function toggleCategoryVisibility(category: string) {
		if (hiddenCategories.has(category)) hiddenCategories.delete(category);
		else hiddenCategories.add(category);
		persistHidden();
	}

	const groupedForDisplay = $derived(filterPalette(flowgraphStore.groupedCatalog(), search, hiddenCategories));
	const unmfzEndpointCandidates = $derived(flowgraphStore.unmfzEndpointCandidates());

	function onAdd(spec: FlowgraphNodeSpec) {
		flowgraphStore.addCatalogNodeAt(spec, null);
	}

	function onPaletteDragStart(ev: DragEvent, spec: FlowgraphNodeSpec) {
		ev.dataTransfer?.setData("application/x-vac-flowgraph-feature", spec.feature);
		ev.dataTransfer?.setData("text/plain", spec.feature);
		if (ev.dataTransfer) ev.dataTransfer.effectAllowed = "copy";
	}

	function canPublish(direction: string): boolean {
		return direction === "input" || direction === "both";
	}

	function canSubscribe(direction: string): boolean {
		return direction === "output" || direction === "both";
	}

	function endpointSourceLabel(candidate: ReturnType<typeof flowgraphStore.unmfzEndpointCandidates>[number]): string {
		const app = candidate.appDisplayName || candidate.appId || "unknown app";
		const via = candidate.payloadKind || candidate.discoverySource || "metadata";
		return `${app} · via ${via}${candidate.discoveryStale ? " · stale" : ""}`;
	}

	function endpointSourceTitle(candidate: ReturnType<typeof flowgraphStore.unmfzEndpointCandidates>[number]): string {
		const parts = [
			candidate.appId ? `app=${candidate.appId}` : null,
			candidate.instanceId ? `instance=${candidate.instanceId}` : null,
			candidate.discoverySource ? `source=${candidate.discoverySource}` : null,
			candidate.payloadKind ? `kind=${candidate.payloadKind}` : null,
			candidate.sourceKey ? `key=${candidate.sourceKey}` : null,
			candidate.discoveryAgeMs != null ? `age=${Math.trunc(candidate.discoveryAgeMs)}ms` : null,
			candidate.validationWarnings.length ? `warnings=${candidate.validationWarnings.join(" | ")}` : null,
		];
		return parts.filter(Boolean).join("\n");
	}

	function testIdSlug(value: string): string {
		return value.replace(/[^a-zA-Z0-9_-]/g, "_");
	}
</script>

<div class="flex h-full flex-col">
	<div class="border-b border-surface-200-800 p-2">
		<div class="mb-1 flex items-center justify-between">
			<span class="text-xs font-semibold uppercase tracking-wider opacity-60">Palette</span>
			<span class="text-xs opacity-50">
				{(flowgraphStore.catalog?.count ?? 0) + userEnumPaletteExtra} ノード
			</span>
		</div>
		<input
			type="text"
			class="w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs"
			placeholder="検索（feature / title）…"
			aria-label="全ノードを検索"
			bind:value={search}
		/>
		{#if searchLower}
			<p class="mt-1 text-xs opacity-60">非表示カテゴリを含む全ノードから検索中</p>
		{/if}
	</div>
	<div class="flex-1 overflow-y-auto text-xs">
		{#if unmfzEndpointCandidates.length > 0}
			<div class="border-b border-surface-200-800 p-2">
				<div class="mb-1 text-xs font-semibold uppercase tracking-wider opacity-60">UNMF/Z endpoints</div>
				<div class="grid gap-1">
					{#each unmfzEndpointCandidates as candidate (candidate.id)}
						<div
							class="rounded border border-surface-200-800 bg-surface-50-950 p-2"
							data-testid={`unmfz-endpoint-${testIdSlug(candidate.endpointId)}`}
						>
							<div class="flex items-baseline justify-between gap-2">
								<span class="truncate font-medium">{candidate.label}</span>
								<div class="flex shrink-0 items-center gap-1">
									{#if candidate.validationWarnings.length > 0}
										<span
											class="rounded border border-warning-500/50 bg-warning-500/10 px-1 text-[0.55rem] text-warning-800-100"
											title={candidate.validationWarnings.join("\n")}
											data-testid={`unmfz-endpoint-warning-${testIdSlug(candidate.endpointId)}`}
										>
											warn
										</span>
									{/if}
									<span class="text-[0.6rem] opacity-55">{candidate.direction}</span>
								</div>
							</div>
							<div class="mt-0.5 truncate text-[0.6rem] opacity-60" title={endpointSourceTitle(candidate)}>
								{endpointSourceLabel(candidate)}
							</div>
							<div class="mt-0.5 truncate text-[0.6rem] opacity-55">{candidate.baseKeyExpr} · {candidate.topicMode}</div>
							<div class="mt-2 flex gap-1">
								<button
									type="button"
									class="rounded border border-surface-300-700 px-2 py-0.5 text-[0.65rem] hover:bg-surface-200-800 disabled:opacity-40"
									disabled={!canPublish(candidate.direction)}
									title="UNVAC からこの endpoint へ UNMF/Z を publish するノード群を追加"
									onclick={() => flowgraphStore.addUnmfzEndpointGraph(candidate, "publish", null)}
								>
									Publish
								</button>
								<button
									type="button"
									class="rounded border border-surface-300-700 px-2 py-0.5 text-[0.65rem] hover:bg-surface-200-800 disabled:opacity-40"
									disabled={!canSubscribe(candidate.direction)}
									title="この endpoint から UNMF/Z を常時受信する ingress を追加"
									onclick={() => flowgraphStore.addUnmfzEndpointGraph(candidate, "subscribe", null)}
								>
									Ingress
								</button>
							</div>
						</div>
					{/each}
				</div>
			</div>
		{/if}
		{#if flowgraphStore.catalogState === "loading"}
			<div class="p-3 opacity-60">Loading…</div>
		{:else if groupedForDisplay.length === 0}
			<div class="p-3 opacity-60">該当なし</div>
		{:else}
			{#each groupedForDisplay as group (group.category)}
				<div class="flex items-center justify-between gap-1 px-2 py-1 font-semibold uppercase tracking-wider opacity-60">
					<span>{group.category}{group.hidden ? "（非表示）" : ""}</span>
					<button
						type="button"
						class="shrink-0 rounded border border-surface-300-700 px-1.5 py-0.5 text-[0.6rem] font-normal normal-case hover:bg-surface-200-800"
						title={group.hidden ? "カテゴリを再表示" : "カテゴリをパレットから隠す"}
						data-testid="palette-category-toggle"
						data-category={group.category}
						disabled={!!searchLower}
						onclick={() => toggleCategoryVisibility(group.category)}
					>
						{group.hidden ? "表示" : "隠す"}
					</button>
				</div>
				{#if !group.hidden}
					<ul class="mb-1">
						{#each group.specs as spec (spec.palette_key ?? `${spec.feature}\t${spec.title}`)}
							<li>
								<button
									type="button"
									class="w-full cursor-pointer rounded px-2 py-1 text-left hover:bg-surface-200-800"
									title={[spec.feature, spec.description, effectTooltip(spec)].filter(Boolean).join("\n")}
									draggable="true"
									data-testid={`palette-entry-${spec.feature.replace(/[^a-zA-Z0-9_-]/g, "_")}`}
									onclick={() => onAdd(spec)}
									ondragstart={(e) => onPaletteDragStart(e, spec)}
								>
									<div class="flex items-baseline justify-between gap-1">
										<span class="truncate font-medium">{spec.title}</span>
										<span class="truncate text-[0.65rem] opacity-50">{shortFeature(spec.feature)}</span>
									</div>
									<div class="mt-0.5 flex min-w-0 items-center gap-1">
										<span class={effectBadgeClass(spec.effect_class)}>
											{effectClassLabel(spec.effect_class)}
										</span>
										{#if capabilitySummary(spec)}
											<span class="truncate text-[0.58rem] opacity-55">{capabilitySummary(spec)}</span>
										{/if}
									</div>
								</button>
							</li>
						{/each}
					</ul>
				{:else}
					<div class="mb-1 px-2 py-0.5 text-[0.65rem] opacity-50">このカテゴリのノード一覧は非表示です。</div>
				{/if}
			{/each}
		{/if}
	</div>
</div>

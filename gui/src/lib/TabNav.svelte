<script lang="ts">
	/**
	 * タブナビゲーションの表示。現在タブは URL hash (`#now`, `#flowgraph`, ...) で保持し、
	 * ブラウザ戻る/進むや deep-link に対応する。
	 *
	 * GUI redesign では desktop を左レール、mobile を横スクロールにする。
	 */
	import { tabNavStore, type TabId, TABS } from "./tabs.svelte";
	import { FEATURES } from "./featureCatalog";
	let search = $state("");
	let showFeatures = $state(false);
	const query = $derived(search.normalize("NFKC").trim().toLocaleLowerCase());
	const matches = $derived(FEATURES.filter((item) => `${item.label} ${item.keywords}`.toLocaleLowerCase().includes(query)));

	function onClick(id: TabId) {
		tabNavStore.setActive(id);
	}
</script>

<div class="px-3 pt-3">
	<label class="block text-xs font-semibold" for="feature-search">機能を探す</label>
	<input
		id="feature-search"
		type="search"
		bind:value={search}
		placeholder="例：チャネポ、辞書"
		class="mt-2 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-2 text-xs"
	/>
	<button type="button" class="mt-2 text-xs text-primary-500" aria-expanded={showFeatures} onclick={() => (showFeatures = !showFeatures)}
		>機能目次を{showFeatures ? "閉じる" : "開く"}</button
	>
</div>

<nav
	class="flex max-w-full items-center gap-1 overflow-x-auto px-4 py-2 lg:flex-col lg:items-stretch lg:overflow-visible lg:px-3 lg:py-4"
	aria-label="Main tabs"
>
	{#each TABS as tab, index (tab.id)}
		{@const active = tabNavStore.active === tab.id}
		{#if index === 0 || TABS[index - 1].group !== tab.group}
			<div class="mt-3 hidden px-4 text-[10px] font-semibold text-surface-500 first:mt-0 lg:block">{tab.group}</div>
		{/if}
		<button
			type="button"
			class="relative shrink-0 whitespace-nowrap rounded px-4 py-2.5 text-left text-sm transition-colors lg:w-full"
			class:font-semibold={active}
			class:text-primary-500={active}
			class:text-surface-700-300={!active}
			class:bg-surface-50-950={active}
			class:hover:text-surface-950-50={!active}
			class:hover:bg-surface-50-950={!active}
			aria-current={active ? "page" : undefined}
			onclick={() => onClick(tab.id)}
		>
			<span>{tab.label}</span>
			{#if active}
				<span class="absolute inset-x-2 bottom-0 h-0.5 rounded-full bg-primary-500 lg:inset-x-0 lg:inset-y-2 lg:h-auto lg:w-0.5"
				></span>
			{/if}
		</button>
		{#if showFeatures && !query}
			{#each FEATURES.filter((item) => item.tab === tab.id) as feature (feature.id)}
				<button
					type="button"
					data-feature-link={feature.id}
					class="shrink-0 rounded px-4 py-1.5 text-left text-xs hover:bg-surface-50-950 lg:whitespace-normal"
					onclick={() => tabNavStore.openFeature(feature.tab, feature.id)}>{feature.label}</button
				>
			{/each}
		{/if}
	{/each}
</nav>

{#if query}
	<div class="px-3 pb-3" aria-label="機能の検索結果">
		<p class="mb-2 text-xs opacity-60">{matches.length}件</p>
		{#each matches as feature (feature.id)}
			<button
				type="button"
				class="block w-full rounded px-2 py-2 text-left text-xs hover:bg-surface-50-950"
				onclick={() => tabNavStore.openFeature(feature.tab, feature.id)}
			>
				{feature.label}<span class="mt-1 block opacity-50">{TABS.find((tab) => tab.id === feature.tab)?.label}</span>
			</button>
		{/each}
		{#if matches.length === 0}<p class="text-xs opacity-65">別の言葉で検索するか、機能目次を確認してください。</p>{/if}
	</div>
{/if}

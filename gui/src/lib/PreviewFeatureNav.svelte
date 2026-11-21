<script lang="ts">
	import { FEATURES } from "./featureCatalog";
	import { TABS, tabNavStore } from "./tabs.svelte";
	let query = $state("");
	let expanded = $state<string[]>([]);
	let all = $state(false);
	const normalized = $derived(query.normalize("NFKC").trim().toLocaleLowerCase());
	const matching = $derived(FEATURES.filter((feature) => `${feature.label} ${feature.keywords}`.normalize("NFKC").toLocaleLowerCase().includes(normalized)));
	function toggle(id: string) {
		expanded = expanded.includes(id) ? expanded.filter((value) => value !== id) : [...expanded, id];
	}
</script>

<nav class="feature-outline" aria-label="機能から操作する">
	<div class="outline-tools">
		<label for="outline-search">機能を探す</label>
		<input id="outline-search" type="search" bind:value={query} placeholder="例：音声、プロファイル" />
		<button type="button" aria-pressed={all} onclick={() => (all = !all)}>{all ? "作業中の項目を中心に表示" : "全項目を展開"}</button>
	</div>
	{#each TABS as tab, index (tab.id)}
		{@const children = matching.filter((feature) => feature.tab === tab.id)}
		{@const active = tabNavStore.active === tab.id}
		{@const open = all || !!normalized || active || expanded.includes(tab.id)}
		{#if !normalized || children.length > 0}
			{#if !normalized && (index === 0 || TABS[index - 1].group !== tab.group)}
				<p class="outline-group">{tab.group}</p>
			{/if}
			<div class="outline-section" class:active>
				<div class="outline-heading">
					<button class="section-link" type="button" aria-current={active ? "page" : undefined} onclick={() => tabNavStore.setActive(tab.id)}>{tab.label}</button>
					{#if children.length && !all && !normalized && !active}
						<button class="section-expand" type="button" aria-label={`${tab.label}の項目`} aria-expanded={open} onclick={() => toggle(tab.id)}>{open ? "−" : "+"}</button>
					{/if}
				</div>
				{#if open && children.length}
					<ul>
						{#each children as feature (feature.id)}
							<li><button type="button" data-feature-link={feature.id} class:selected={tabNavStore.featureFocus?.id === feature.id} onclick={() => tabNavStore.openFeature(feature.tab, feature.id)}>{feature.label}</button></li>
						{/each}
					</ul>
				{/if}
			</div>
		{/if}
	{/each}
	{#if normalized && !matching.length}<p class="outline-empty">該当する項目がありません。検索を消すと全区分を確認できます。</p>{/if}
</nav>

<style>
	.feature-outline { padding: .75rem; font-size: .82rem; }
	.outline-tools { display: grid; gap: .5rem; }
	.outline-tools label { font-weight: 650; }
	.outline-tools input { width: 100%; min-width: 0; border: 1px solid color-mix(in srgb, currentColor 25%, transparent); border-radius: .45rem; padding: .55rem; background: transparent; }
	.outline-tools > button { text-align: left; font-size: .75rem; text-decoration: underline; text-underline-offset: .2rem; }
	.outline-group { margin: 1rem .55rem .35rem; opacity: .65; font-size: .7rem; }
	.outline-section { border-left: 2px solid transparent; }
	.outline-section.active { border-left-color: var(--color-primary-500); }
	.outline-heading { display: flex; align-items: stretch; }
	.section-link { flex: 1; text-align: left; padding: .6rem .55rem; font-weight: 650; }
	.section-expand { width: 2rem; flex-shrink: 0; }
	ul { list-style: none; padding: 0 0 .4rem .55rem; margin: 0; }
	li button { width: 100%; padding: .4rem .55rem; text-align: left; line-height: 1.5; border-radius: .35rem; }
	button:hover, .selected { background: color-mix(in srgb, currentColor 9%, transparent); }
	button:focus-visible, input:focus-visible { outline: 2px solid var(--color-primary-500); outline-offset: 2px; }
	.outline-empty { padding: .6rem; line-height: 1.6; }
</style>

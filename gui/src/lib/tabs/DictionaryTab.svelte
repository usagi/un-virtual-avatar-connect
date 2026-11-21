<script lang="ts">
	import FeatureAnchor from "../FeatureAnchor.svelte";
	import DictionaryEditorPane from "../control/dictionary/DictionaryEditorPane.svelte";
	import DictionaryLiveQuickAdd from "../control/dictionary/DictionaryLiveQuickAdd.svelte";
	import DictionarySetupReviewPane from "../control/dictionary/DictionarySetupReviewPane.svelte";
	const setupReview = typeof window !== 'undefined' && new URLSearchParams(window.location.search).get('dictionarySetupReview') === '1';
</script>

<section class="grid gap-5">
	<header>
		<h2 class="text-xl font-semibold">辞書</h2>
	</header>

	{#if setupReview}<DictionarySetupReviewPane />
	{:else}
		<details class="rounded-xl border border-surface-400 p-4">
			<summary class="cursor-pointer font-semibold">辞書を追加・読み上げに接続</summary>
			<div class="mt-4"><DictionarySetupReviewPane comparison={false} /></div>
		</details>
	{/if}
	<FeatureAnchor id="dictionary-live"><DictionaryLiveQuickAdd /></FeatureAnchor>
	<FeatureAnchor id="dictionary-edit"><DictionaryEditorPane /></FeatureAnchor>
</section>

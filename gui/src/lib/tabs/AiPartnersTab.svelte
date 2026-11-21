<script lang="ts">
	import FeatureAnchor from "../FeatureAnchor.svelte";
	import AiManagementPanel from "../AiManagementPanel.svelte";
	import PausePanel from "../PausePanel.svelte";
	import DesktopAiSetupPanel from "../resources/DesktopAiSetupPanel.svelte";
	import PerceptionSetupPanel from "../resources/PerceptionSetupPanel.svelte";
	let aiSaved = $state<{instance:string;profile:string} | null>(null);
	const perceptionPreview = import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" && new URLSearchParams(window.location.search).get("perceptionReview") === "1"
		? import("../resources/PerceptionSetupPreview.svelte") : null;
</script>

<section class="grid gap-5">
	<header>
		<h2 class="text-xl font-semibold">AIパートナー</h2>
	</header>

	{#if perceptionPreview}
		{#await perceptionPreview then module}<module.default />{/await}
	{/if}
	<FeatureAnchor id="ai-setup"><DesktopAiSetupPanel onsaved={profile => aiSaved = {instance:window.location.origin,profile}} /></FeatureAnchor>
	{#if !perceptionPreview}<PerceptionSetupPanel externalSave={aiSaved} />{/if}

	<details class="border-y border-surface-200-800">
		<summary class="cursor-pointer py-3 text-sm font-semibold">詳細設定と稼働状態</summary>
		<div class="grid gap-4 pb-5">
			<FeatureAnchor id="ai-detail"><AiManagementPanel /></FeatureAnchor>
			<FeatureAnchor id="ai-status"><PausePanel /></FeatureAnchor>
		</div>
	</details>
</section>

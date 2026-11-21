<script lang="ts">
	import { tick, type Snippet } from "svelte";
	import { tabNavStore } from "./tabs.svelte";
	let { id, children }: { id: string; children: Snippet } = $props();
	let element: HTMLDivElement;
	$effect(() => {
		const request = tabNavStore.featureFocus;
		if (request?.id !== id) return;
		let cancelled = false;
		void tick().then(() => {
			if (cancelled || !element) return;
			for (let parent = element.parentElement; parent; parent = parent.parentElement) {
				if (parent instanceof HTMLDetailsElement) parent.open = true;
			}
			element.scrollIntoView({ block: "center" });
			element.focus({ preventScroll: true });
		});
		return () => {
			cancelled = true;
		};
	});
</script>

<div bind:this={element} data-feature={id} tabindex="-1" class="min-w-0 scroll-mt-24 rounded focus:outline-2 focus:outline-primary-500">
	{@render children()}
</div>

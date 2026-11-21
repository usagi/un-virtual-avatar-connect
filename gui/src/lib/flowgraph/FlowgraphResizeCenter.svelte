<script lang="ts">
	import { onMount } from "svelte";
	import { useSvelteFlow } from "@xyflow/svelte";
	const { getViewport, setViewport } = useSvelteFlow();
	let marker: HTMLSpanElement;
	onMount(() => {
		const pane = marker.closest<HTMLElement>(".svelte-flow");
		if (!pane) return;
		let previous: { width: number; height: number } | undefined;
		const observer = new ResizeObserver(() => {
			const { width, height } = pane.getBoundingClientRect();
			// Keep the last usable dimensions while a mounted editor is hidden.
			if (width <= 0 || height <= 0) return;
			if (previous && (width !== previous.width || height !== previous.height)) {
				const view = getViewport();
				void setViewport({ ...view, x: view.x + (width - previous.width) / 2, y: view.y + (height - previous.height) / 2 }, { duration: 0 });
			}
			previous = { width, height };
		});
		observer.observe(pane);
		return () => observer.disconnect();
	});
</script>

<span bind:this={marker} hidden aria-hidden="true"></span>

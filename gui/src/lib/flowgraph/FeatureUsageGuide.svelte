<script lang="ts">
    import { FEATURES } from '../featureCatalog';
    import { tabNavStore } from '../tabs.svelte';
    import { FEATURE_USAGE } from './featureUsage';
    import { flowgraphStore } from '../flowgraphStore.svelte';
    let { packId }: { packId: string } = $props();
    const usage = $derived(FEATURE_USAGE[packId]);
    const destinations = $derived(usage?.destinations.map(id => FEATURES.find(item => item.id === id)).filter(item => item !== undefined) ?? []);
</script>

{#if usage}
    <details class="mt-3 rounded border border-surface-300-700 p-3 text-xs" aria-label="テンプレートの使い方">
        <summary class="cursor-pointer font-semibold">設定して試す</summary>
        <ol class="my-3 list-decimal space-y-2 pl-5">
            {#each usage.steps as step}<li>{step}</li>{/each}
        </ol>
        <div class="flex flex-wrap gap-2">
            {#each destinations as feature (feature.id)}
                <button type="button" disabled={flowgraphStore.mutating} class="rounded border border-surface-300-700 px-2 py-1.5 disabled:opacity-50" onclick={() => tabNavStore.openFeature(feature.tab, feature.id)}>{feature.label}</button>
            {/each}
        </div>
    </details>
{/if}

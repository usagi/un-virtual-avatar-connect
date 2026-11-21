<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "./api";
	import type { ManagedAppView } from "./types";
	let { onselect }: { onselect: (id: string) => void } = $props();
	let entries = $state<ManagedAppView[]>([]);
	let error = $state(false);
	let loaded = $state(false);
	let updated = $state("");
	onMount(() => {
		let disposed = false;
		let pending = false;
		async function refresh() {
			if (pending) return;
			pending = true;
			try {
				const result = await api.managedApps();
				if (!disposed) {
					entries = result.entries;
					error = false;
					loaded = true;
					updated = new Date().toLocaleTimeString();
				}
			} catch {
				if (!disposed) error = true;
			} finally {
				pending = false;
			}
		}
		void refresh();
		const timer = setInterval(() => void refresh(), 5000);
		return () => {
			disposed = true;
			clearInterval(timer);
		};
	});
</script>

<details class="border-b border-surface-200-800 px-3 py-3" open>
	<summary class="cursor-pointer text-xs font-semibold">このVACの連携対象 <span class="opacity-60">{entries.length}</span></summary>
	<p class="mt-2 text-[11px] opacity-60">対象を選ぶと、そのアプリの操作を開きます。</p>
	{#if error}<p role="status" class="mt-2 text-xs text-warning-500">状態を更新できません。表示は前回取得時のものです。</p>{/if}
	{#if !loaded && !error}<p class="mt-2 text-xs opacity-60">確認中…</p>{/if}
	<div class="max-h-56 overflow-y-auto">
		{#each entries as entry (entry.id)}
			<button
				type="button"
				class="mt-2 block w-full rounded border border-surface-200-800 px-2 py-2 text-left hover:bg-surface-50-950"
				onclick={() => onselect(entry.id)}
			>
				<span class="block truncate text-xs font-semibold">{entry.label}</span>
				<span class="block text-[11px] opacity-65"
					>{error ? "更新未確認" : !entry.supports_status ? "状態不明" : entry.status.running ? "稼働中" : "停止中"} · {entry.id}</span
				>
				{#if entry.unaa_z_status.zenoh_seen}<span class="block break-all text-[10px] opacity-55"
						>{entry.unaa_z_status.app_id} / {entry.unaa_z_status.instance_id}</span
					>{/if}
			</button>
		{/each}
	</div>
	{#if loaded && entries.length === 0}<p class="mt-2 text-xs opacity-60">登録した連携アプリはありません。</p>{/if}
	{#if updated}<p class="mt-2 text-[10px] opacity-50">取得 {updated}</p>{/if}
</details>

<script lang="ts">
	import { api } from "./api";
	import { ControlApiError, type TwitchRewardsResponse } from "./types";
	import { toastStore } from "./toasts.svelte";

	let result = $state<TwitchRewardsResponse | null>(null);
	let loading = $state(false);
	let error = $state<string | null>(null);

	async function copyRewardId(id: string): Promise<void> {
		try {
			if (navigator.clipboard?.writeText) {
				await navigator.clipboard.writeText(id);
			} else {
				const input = document.createElement("textarea");
				input.value = id;
				input.style.position = "fixed";
				input.style.opacity = "0";
				document.body.appendChild(input);
				input.select();
				const copied = document.execCommand("copy");
				document.body.removeChild(input);
				if (!copied) throw new Error("clipboard unavailable");
			}
			toastStore.success("報酬IDをコピーしました", id);
		} catch {
			toastStore.error("報酬IDをコピーできませんでした", "IDを選択してコピーしてくれ。");
		}
	}

	async function load(): Promise<void> {
		if (loading) return;
		loading = true;
		error = null;
		try {
			result = await api.twitchRewards();
		} catch (cause) {
			if (cause instanceof ControlApiError) {
				const reason =
					typeof cause.body === "object" && cause.body !== null && "reason" in cause.body
						? String(cause.body.reason)
						: cause.statusText;
				error = cause.status === 401 ? "BroadcasterのTwitch認可が必要だ。" : `報酬を取得できない: ${reason}`;
			} else {
				error = cause instanceof Error ? cause.message : String(cause);
			}
		} finally {
			loading = false;
		}
	}
</script>

<section class="border border-surface-200-800 bg-surface-50-950 p-3" aria-labelledby="twitch-rewards-heading">
	<header class="flex flex-wrap items-center justify-between gap-2">
		<div>
			<h4 id="twitch-rewards-heading" class="text-sm font-semibold">チャンネルポイント報酬</h4>
			<p class="text-xs opacity-60">EventSub連携に使う報酬を確認</p>
		</div>
		<button class="btn btn-sm variant-soft-primary" type="button" onclick={load} disabled={loading}>
			{loading ? "取得中" : result ? "更新" : "報酬を取得"}
		</button>
	</header>

	{#if error}
		<p class="mt-3 border-l-2 border-error-500 pl-2 text-sm text-error-500">{error}</p>
	{:else if result?.available === false}
  <p class="mt-3 text-sm opacity-75">{result.reason}</p>
 {:else if result}
		<div class="mt-3 flex items-baseline justify-between gap-3 text-xs opacity-60">
			<span>{result.broadcaster_login}</span>
			<span>{result.rewards.length}件</span>
		</div>
		{#if result.rewards.length === 0}
			<p class="mt-2 text-sm opacity-60">カスタム報酬はない。</p>
		{:else}
			<ul class="mt-2 divide-y divide-surface-200-800 border-y border-surface-200-800">
				{#each result.rewards as reward (reward.id)}
					<li class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 py-2">
						<div class="min-w-0">
							<div class="flex items-center gap-2">
								<span
									class="size-3 shrink-0 border border-surface-300-700"
									style:background-color={reward.background_color || "transparent"}
								></span>
								<span class="truncate text-sm font-medium">{reward.title}</span>
							</div>
							<div class="mt-0.5 flex min-w-0 items-center gap-2">
								<p class="truncate font-mono text-[11px] opacity-45">{reward.id}</p>
								<button
									type="button"
									class="shrink-0 border border-surface-300-700 px-1.5 py-0.5 text-[10px] hover:bg-surface-200-800"
									onclick={() => void copyRewardId(reward.id)}>IDをコピー</button
								>
							</div>
						</div>
						<div class="text-right">
							<p class="text-sm tabular-nums">{reward.cost.toLocaleString()} pt</p>
							<p class="text-[11px] opacity-50">
								{reward.is_enabled && !reward.is_paused && reward.is_in_stock ? "利用可能" : "停止中"}
							</p>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	{/if}
</section>

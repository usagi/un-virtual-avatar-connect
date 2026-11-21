<script lang="ts">
	import FeatureAnchor from "../FeatureAnchor.svelte";
	import ConnectionSetupGuide from "../resources/ConnectionSetupGuide.svelte";
	import TwitchSetupPanel from "../resources/TwitchSetupPanel.svelte";
	/**
	 * Runtime resources.
	 *
	 * External services, credentials, and child processes live here. Profile
	 * management and developer diagnostics belong to Settings.
	 */
	import { onMount } from "svelte";
	import { api } from "../api";
	import type { RuntimeIssue } from "../types";
	import GoogleSheetsOAuthPanel from "../GoogleSheetsOAuthPanel.svelte";
	import OAuthPanel from "../OAuthPanel.svelte";
	import TwitchRewardsPanel from "../TwitchRewardsPanel.svelte";
	import ResourceOverviewPanel from "../resources/ResourceOverviewPanel.svelte";
	import VmcStatusPanel from "../resources/VmcStatusPanel.svelte";
	import RunWithEditorPanel from "./RunWithEditorPanel.svelte";

	let runtimeIssues = $state<RuntimeIssue[]>([]);
	let twitchAuthorizationEpoch = $state(0);
	let twitchSetupPending = $state(true);

	onMount(() => {
		void api
			.flowgraphDiagnostics()
			.then((diagnostics) => {
				runtimeIssues = (diagnostics.runtime_issues ?? []).filter((item) => item.action === "resources");
			})
			.catch(() => {});
	});
</script>

<section class="grid gap-5">
	<header>
		<h2 class="text-xl font-semibold">連携</h2>
		<p class="text-sm opacity-60">AI、配信サービス、外部アプリ</p>
	</header>
	<FeatureAnchor id="connection-setup"><ConnectionSetupGuide /></FeatureAnchor>

	{#if runtimeIssues.length > 0}
		<section class="rounded border border-warning-500/50 bg-warning-500/10 px-3 py-2 text-sm" aria-label="連携の確認事項">
			<h3 class="font-semibold">接続に確認が必要</h3>
			{#each runtimeIssues as item (item.key)}
				<p class="mt-1">{item.message} {item.impact}</p>
			{/each}
		</section>
	{/if}

	<section aria-labelledby="stream-service-heading">
		<h3 id="stream-service-heading" class="mb-2 text-sm font-semibold">配信サービス</h3>
		<div class="grid gap-3">
			<FeatureAnchor id="twitch">
				<TwitchSetupPanel onpending={(pending) => twitchSetupPending = pending} onapplied={() => twitchAuthorizationEpoch++} />
				<fieldset disabled={twitchSetupPending} class="min-w-0">
					<legend class="my-2 text-sm font-semibold">2. Twitch接続・認証</legend>
					{#if twitchSetupPending}<p class="mb-2 text-xs opacity-65">基本設定の保存・適用を完了してから認証できます。</p>{/if}
					{#key twitchAuthorizationEpoch}<OAuthPanel />{/key}
				</fieldset>
			</FeatureAnchor>
			<FeatureAnchor id="rewards"><TwitchRewardsPanel /></FeatureAnchor>
		</div>
	</section>

	<div class="border-t border-surface-200-800 pt-2">
		<h3 class="mb-1 text-xs font-semibold uppercase opacity-50">追加の接続と詳細</h3>

		<details class="group border-b border-surface-200-800">
			<summary class="cursor-pointer list-none py-3 text-sm font-semibold">
				連携アプリ・アバター
				<span class="ml-2 text-xs font-normal opacity-50">UNAvatar、UNMotion、外部アプリ</span>
			</summary>
			<div class="pb-4"><FeatureAnchor id="run-with"><RunWithEditorPanel /></FeatureAnchor></div>
		</details>

		<details class="group border-b border-surface-200-800">
			<summary class="cursor-pointer list-none py-3 text-sm font-semibold">
				Google Sheets
				<span class="ml-2 text-xs font-normal opacity-50">OAuth接続</span>
			</summary>
			<div class="pb-4"><FeatureAnchor id="sheets"><GoogleSheetsOAuthPanel /></FeatureAnchor></div>
		</details>

		<details class="group border-b border-surface-200-800">
			<summary class="cursor-pointer list-none py-3 text-sm font-semibold">
				接続診断
				<span class="ml-2 text-xs font-normal opacity-50">UNAA/Z、process、VMC</span>
			</summary>
			<div class="grid gap-4 pb-4 xl:grid-cols-2">
				<FeatureAnchor id="connections"><ResourceOverviewPanel /></FeatureAnchor>
				<FeatureAnchor id="vmc"><VmcStatusPanel /></FeatureAnchor>
			</div>
		</details>
	</div>
</section>

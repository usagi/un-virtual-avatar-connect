<script lang="ts">
	import SettingsScopeGuide from "../SettingsScopeGuide.svelte";
	import FeatureAnchor from "../FeatureAnchor.svelte";
	/** 永続profileと外観を主面に置き、runtime操作は必要時だけ開示する。 */
	import PingStatus from "../PingStatus.svelte";
	import ReloadPanel from "../ReloadPanel.svelte";
	import { VAC_THEMES, vacThemeStore, type VacThemeId } from "../theme.svelte";
	import DesktopProfileManagerPanel from "./DesktopProfileManagerPanel.svelte";
	import ProfileManagerPanel from "./ProfileManagerPanel.svelte";
	import { api } from "../api";
	import { tabNavStore } from "../tabs.svelte";

	let managedProfiles = $state<boolean | null>(null);
	let profileView = $state<"managed" | "files" | null>(null);
	$effect(() => {
		api.desktopProfiles()
			.then((response) => (managedProfiles = response.managed_mode))
			.catch(() => (managedProfiles = false));
	});

	function setTheme(themeId: VacThemeId) {
		vacThemeStore.set(themeId);
	}
	function selectProfileView(next: "managed" | "files") {
		if (next === (profileView ?? (managedProfiles ? "managed" : "files"))) return;
		if (tabNavStore.canLeaveCurrent()) profileView = next;
	}
</script>

<section class="grid gap-5">
	<header>
		<h2 class="text-xl font-semibold">設定</h2>
		<p class="text-sm opacity-60">プロファイルとアプリの表示を管理する。</p>
	</header>

	<SettingsScopeGuide />

	<FeatureAnchor id="profiles">
		<div class="mb-3 flex flex-wrap gap-2" role="group" aria-label="プロファイルの保存形式">
			<button
				type="button"
				class="rounded border border-surface-300-700 px-3 py-2 text-xs"
				aria-pressed={(profileView ?? (managedProfiles ? "managed" : "files")) === "managed"}
				onclick={() => selectProfileView("managed")}>管理プロファイル</button
			>
			<button
				type="button"
				class="rounded border border-surface-300-700 px-3 py-2 text-xs"
				aria-pressed={(profileView ?? (managedProfiles ? "managed" : "files")) === "files"}
				onclick={() => selectProfileView("files")}>設定ファイル</button
			>
		</div>
		{#if profileView === "managed" || (profileView === null && managedProfiles)}
			<DesktopProfileManagerPanel />
		{:else if profileView === "files" || managedProfiles === false}
			<p class="mb-3 text-xs opacity-65">接続中のVACが参照する設定フォルダー。ファイルの編集・複製・改名・削除を行えます。</p>
			<ProfileManagerPanel />
		{:else}
			<div class="vac-panel px-4 py-5 text-sm opacity-60">プロファイルを読み込み中…</div>
		{/if}
	</FeatureAnchor>

	<FeatureAnchor id="appearance">
		<section class="vac-panel" aria-labelledby="appearance-heading">
			<header class="vac-panel-header px-4 py-3">
				<h3 id="appearance-heading" class="text-sm font-semibold">テーマ</h3>
				<p class="mt-0.5 text-xs opacity-55">画面の配色を選択する。</p>
			</header>
			<div class="grid gap-2 p-4 sm:grid-cols-2" role="radiogroup" aria-label="GUI テーマ">
				{#each VAC_THEMES as theme (theme.id)}
					{@const active = vacThemeStore.current === theme.id}
					<button
						type="button"
						role="radio"
						class="vac-theme-choice rounded border p-3 text-left transition-colors"
						class:is-active={active}
						aria-checked={active}
						onclick={() => setTheme(theme.id)}
					>
						<span class="flex min-w-0 items-center gap-3">
							<span class="vac-theme-swatch" data-theme-swatch={theme.id} aria-hidden="true"></span>
							<span class="min-w-0 flex-1">
								<span class="block truncate text-sm font-semibold">{theme.label}</span>
								<span class="block truncate text-xs opacity-60">{theme.tone}</span>
							</span>
							{#if active}<span class="vac-theme-active-badge shrink-0">選択中</span>{/if}
						</span>
					</button>
				{/each}
			</div>
		</section>
	</FeatureAnchor>

	<details class="group border-t border-surface-200-800">
		<summary class="cursor-pointer list-none py-3 text-sm font-semibold">
			ランタイム詳細
			<span class="ml-2 text-xs font-normal opacity-50">一時reload、接続情報</span>
		</summary>
		<div class="grid gap-4 pb-4 xl:grid-cols-[minmax(0,1.2fr)_minmax(18rem,0.8fr)]">
			<FeatureAnchor id="reload"><ReloadPanel /></FeatureAnchor>
			<section class="vac-panel p-4">
				<h3 class="mb-2 text-sm font-semibold">接続情報</h3>
				<FeatureAnchor id="connection-info"><PingStatus /></FeatureAnchor>
			</section>
		</div>
	</details>
</section>

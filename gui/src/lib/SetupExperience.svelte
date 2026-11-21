<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "./api";
	import ProfileImportPicker from "./ProfileImportPicker.svelte";
	import { prepareProfileImport, profileImportBaseName, type ProfileImportSource } from "./profileWorkspaceBundle";
	import type { DesktopProfileEntry, DesktopProfilesResponse, DesktopProfileTemplateEntry } from "./types";

	let { store, connectionUnconfirmed = false } = $props<{ store: DesktopProfilesResponse; connectionUnconfirmed?: boolean }>();
	let label = $state("My UNVAC");
	let templates = $state<DesktopProfileTemplateEntry[]>([]);
	let templatesLoading = $state(false);
	let templatesError = $state<string | null>(null);
	let selectedTemplateId = $state<string | null>(null);
	let importSource = $state<ProfileImportSource | null>(null);
	let busy = $state(false);
	let restartRequested = $state<{ label: string; confPath: string } | null>(null);
	let error = $state<string | null>(null);
	let createdProfiles = $state<DesktopProfileEntry[]>([]);
	let savedProfiles = $derived([
		...store.profiles.filter((profile: DesktopProfileEntry) => !createdProfiles.some((created) => created.id === profile.id)),
		...createdProfiles,
	]);

	onMount(() => {
		void loadTemplates();
	});

	async function loadTemplates() {
		if (templatesLoading || busy) return;
		templatesLoading = true;
		templatesError = null;
		try {
			const entries = await api.desktopProfileTemplates();
			templates = entries;
			if (!entries.some((entry) => entry.id === selectedTemplateId)) {
				selectedTemplateId = entries.find((entry) => entry.recommended)?.id ?? entries[0]?.id ?? null;
			}
			if (!entries.length) templatesError = "開始する構成が見つかりません。再取得するか、既存の設定を取り込んでください。";
		} catch (value) {
			templatesError = `開始する構成を取得できません。${message(value)}`;
		} finally {
			templatesLoading = false;
		}
	}

	function slugify(value: string): string {
		return value
			.normalize("NFKC")
			.toLowerCase()
			.replace(/[^a-z0-9_-]+/g, "-")
			.replace(/^-+|-+$/g, "")
			.slice(0, 56);
	}

	function uniqueProfileId(value: string): string {
		const base = slugify(value) || "workspace";
		const used = new Set(savedProfiles.map((profile: DesktopProfileEntry) => profile.id));
		if (!used.has(base)) return base;
		for (let suffix = 2; suffix < 10_000; suffix += 1) {
			const candidate = `${base}-${suffix}`.slice(0, 64);
			if (!used.has(candidate)) return candidate;
		}
		return `${base}-${Date.now().toString(36)}`.slice(0, 64);
	}

	function message(value: unknown): string {
		if (value instanceof Error) return value.message;
		return String(value);
	}

	async function activate(profile: DesktopProfileEntry) {
		let selected = false;
		try {
			const activated = await api.activateDesktopProfile(profile.id);
			selected = true;
			await api.restart({ conf: activated.profile.conf_path, graceful_ms: 800 });
			restartRequested = { label: profile.label, confPath: activated.profile.conf_path };
		} catch (value) {
			throw new Error(
				`「${profile.label}」は保存済みです。` +
					(selected
						? "次回起動先の変更は完了しましたが、再起動の完了を確認できません。稼働状態を確認してください。"
						: "起動先の変更を確認できず、再起動は要求していません。保存済みワークスペースの「開く」から再試行できます。") +
					` ${message(value)}`,
			);
		}
	}

	function rememberCreated(profile: DesktopProfileEntry) {
		createdProfiles = [...createdProfiles.filter((entry) => entry.id !== profile.id), profile];
	}

	async function createAndStart() {
		if (busy || !selectedTemplateId) return;
		busy = true;
		error = null;
		try {
			const profileLabel = label.trim() || "My UNVAC";
			const created = await api.createDesktopProfile({
				id: uniqueProfileId(profileLabel),
				label: profileLabel,
				template_id: selectedTemplateId,
			});
			rememberCreated(created.profile);
			await activate(created.profile);
		} catch (value) {
			error = message(value);
			busy = false;
		}
	}

	async function importAndStart() {
		if (!importSource || busy) return;
		busy = true;
		error = null;
		try {
			const base = profileImportBaseName(importSource);
			const id = uniqueProfileId(base);
			const prepared = await prepareProfileImport(importSource);
			const created = prepared.bundle
				? await api.importDesktopProfileBundle(id, base, prepared.body)
				: await api.createDesktopProfile({ id, label: base, conf: await prepared.body.text() });
			rememberCreated(created.profile);
			await activate(created.profile);
		} catch (value) {
			error = message(value);
			busy = false;
		}
	}

	async function useExisting(profile: DesktopProfileEntry) {
		if (busy) return;
		busy = true;
		error = null;
		try {
			await activate(profile);
		} catch (value) {
			error = message(value);
			busy = false;
		}
	}
</script>

<main class="min-h-screen bg-surface-50-950 text-surface-950-50">
	<header class="border-b border-surface-200-800 px-5 py-4 lg:px-10">
		<h1 class="text-xl font-bold">Virtual Avatar Connect</h1>
		<p class="text-sm opacity-60">新しいワークスペース</p>
		{#if restartRequested}
			<div class="mt-3 rounded border border-primary-500/40 bg-primary-500/5 px-3 py-3 text-sm" role="status">
				<p>「{restartRequested.label}」の設定は保存済みです。再起動を要求しました。</p>
				<p class="mt-1">再起動後のVACウィンドウで続きを設定してください。この画面が残る場合は、接続先が変わっている可能性があります。</p>
				<details class="mt-2">
					<summary class="cursor-pointer">切替先を確認</summary>
					<p class="mt-1 break-all">{restartRequested.confPath}</p>
				</details>
			</div>
		{:else if connectionUnconfirmed}
			<p class="mt-2 text-sm text-warning-700-300" role="status">接続を確認できません。入力を保持して再接続を待っています。</p>
		{/if}
	</header>

	<div class="mx-auto grid max-w-6xl gap-8 px-5 py-8 lg:grid-cols-[minmax(0,1.2fr)_minmax(320px,0.8fr)] lg:px-10 lg:py-10">
		<section class="min-w-0">
			<h2 class="text-lg font-semibold">新しく始める</h2>
			{#if templatesLoading}
				<p class="mt-3 text-sm opacity-60" role="status">開始する構成を読み込み中…</p>
			{/if}
			{#if templatesError}
				<div class="mt-3 rounded border border-error-500 px-3 py-3 text-sm" role="alert">
					<p>{templatesError}</p>
					<button type="button" class="mt-2 rounded border px-3 py-1.5 disabled:opacity-50" disabled={busy || templatesLoading} onclick={() => void loadTemplates()}>構成一覧を再取得</button>
				</div>
			{/if}
			<div class="mt-4 grid gap-2" role="radiogroup" aria-label="開始する構成">
				{#each templates as template (template.id)}
					<label
						class={`flex cursor-pointer items-start gap-3 rounded border px-3 py-3 ${
							selectedTemplateId === template.id ? "border-primary-500 bg-primary-500/5" : "border-surface-300-700"
						}`}
					>
						<input type="radio" name="profile-template" value={template.id} bind:group={selectedTemplateId} disabled={busy} class="mt-1" />
						<span class="min-w-0">
							<span class="block text-sm font-semibold">{template.label}</span>
							<span class="mt-0.5 block text-xs opacity-60">{template.description}</span>
						</span>
					</label>
				{/each}
			</div>

			<label class="mt-5 block text-sm font-medium" for="profile-label">名前</label>
			<input
				id="profile-label"
				type="text"
				bind:value={label}
				disabled={busy}
				class="mt-2 w-full rounded border border-surface-300-700 bg-surface-50-950 px-3 py-2"
			/>
			<button
				type="button"
				class="mt-5 rounded bg-primary-500 px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
				disabled={busy || !selectedTemplateId}
				onclick={() => void createAndStart()}
			>
				{restartRequested ? "再起動待ち…" : busy ? "準備中…" : "作成して開始"}
			</button>
		</section>

		<div class="min-w-0 space-y-8 border-t border-surface-200-800 pt-8 lg:border-t-0 lg:border-l lg:pl-8 lg:pt-0">
			<section>
				<h2 class="text-base font-semibold">既存の設定を使う</h2>
				<p class="mt-1 text-xs opacity-60">自分で作成したもの、または信頼できる提供元の設定だけを使用</p>
				<ProfileImportPicker bind:source={importSource} disabled={busy} />
				<button
					type="button"
					class="mt-3 rounded border border-surface-300-700 px-3 py-2 text-sm font-medium disabled:opacity-50"
					disabled={busy || !importSource}
					onclick={() => void importAndStart()}
				>
					取り込んで開始
				</button>
			</section>

			{#if savedProfiles.length > 0}
				<section>
					<h2 class="text-base font-semibold">保存済みワークスペース</h2>
					<div class="mt-3 divide-y divide-surface-200-800 border-y border-surface-200-800">
						{#each savedProfiles as profile (profile.id)}
							<div class="flex items-center justify-between gap-3 py-3">
								<div class="min-w-0">
									<div class="truncate text-sm font-medium">{profile.label}</div>
									<div class="truncate text-xs opacity-55">
										{templates.find((template) => template.id === profile.template_id)?.label ?? "既存設定"}
									</div>
								</div>
								<button
									type="button"
									class="shrink-0 rounded border border-surface-300-700 px-3 py-1.5 text-sm disabled:opacity-50"
									disabled={busy}
									onclick={() => void useExisting(profile)}
								>
									開く
								</button>
							</div>
						{/each}
					</div>
				</section>
			{/if}
		</div>
	</div>

	{#if error}
		<div
			class="fixed inset-x-4 bottom-4 mx-auto max-w-3xl rounded border border-error-500 bg-error-500/10 px-4 py-3 text-sm text-error-900-100"
		>
			{error}
		</div>
	{/if}
</main>

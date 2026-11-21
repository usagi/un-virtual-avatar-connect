<script lang="ts">
	import { onMount, tick } from "svelte";
	import { api } from "../api";
	import { ControlApiError, type DesktopAiSetupStatus, type DesktopProfileEntry } from "../types";
	import { toastStore } from "../toasts.svelte";
	import { tabNavStore } from "../tabs.svelte";
	import { confSyncStore } from "../confSync.svelte";
	import RestartDialog from "../RestartDialog.svelte";
	let { onsaved }: { onsaved?: (profileId: string) => void } = $props();
	let savedPending = $state(false);
	let restartOpen = $state(false);
	$effect(() => { if (savedPending && confSyncStore.phase === "synced") savedPending = false; });

	let available = $state(false);
	let currentProfile = $state<DesktopProfileEntry | null>(null);
	let loading = $state(true);
	let editing = $state(false);
	let busy = $state(false);
	let error = $state<string | null>(null);
	let loadError = $state<string | null>(null);
	let status = $state<DesktopAiSetupStatus | null>(null);
	const defaultInstructions = "配信者と視聴者を支援するAI共演者として、状況を観察し、必要なときだけ簡潔に応答する。";
	let displayName = $state("AIパートナー");
	let model = $state("gpt-5.6-luna");
	let apiKey = $state("");
	let instructions = $state(defaultInstructions);
	let observeUser = $state(true);
	let observeTwitchChat = $state(false);
	let observeChannelPoints = $state(false);
	let observeRuntime = $state(false);
	let extraTriggers = $state("");
	let avatarActions = $state(false);
	let avatarLinkId = $state("ai_avatar");
	let avatarBaseKey = $state("un-motion/frame/ai-avatar");
	let avatarPublish = $state(false);
	let baseline = $state("");
	function draft(key = apiKey) { return JSON.stringify([displayName, model, key, instructions, observeUser, observeTwitchChat, observeChannelPoints, observeRuntime, extraTriggers, avatarActions, avatarLinkId, avatarBaseKey, avatarPublish]); }
	const dirty = $derived(editing && baseline !== "" && draft() !== baseline);
	let discardReview = $state<{action: "cancel" | "reload"; content: string; revision: string | undefined} | null>(null);
	let keepInputButton = $state<HTMLButtonElement>();
	$effect(() => { if (discardReview) void tick().then(() => keepInputButton?.focus()); });
	function canDiscard() {
		if (busy || loading || discardReview) return false;
		if (dirty) { error = "AI設定に未保存の入力があります。保存するか、キャンセルで破棄を確認してください。"; return false; }
		return true;
	}
	function requestDiscard(action: "cancel" | "reload") {
		if (busy || loading || discardReview) return;
		if (dirty) { discardReview = {action, content: draft(), revision: status?.revision}; return; }
		void finishDiscard(action);
	}
	async function finishDiscard(action: "cancel" | "reload") {
		if (action === "reload") { await load(); return; }
		if (status) { applyStatus(status); apiKey = ""; baseline = draft(); editing = false; }
	}
	async function confirmDiscard() {
		if (busy || loading || !discardReview) return;
		if (discardReview.content !== draft() || discardReview.revision !== status?.revision) {
			discardReview = null;
			error = "確認中に入力または保存設定が変わりました。もう一度操作してください。";
			return;
		}
		const action = discardReview.action;
		discardReview = null;
		await finishDiscard(action);
	}

	onMount(() => {
		void load();
		return tabNavStore.guardLeaving(canDiscard);
	});

	async function load() {
		if (busy) return;
		loading = true;
		error = null;
		loadError = null;
		try {
			const profiles = await api.desktopProfiles();
			available = profiles.managed_mode;
			currentProfile = profiles.current_profile_id
				? profiles.profiles.find(profile => profile.id === profiles.current_profile_id) ?? null
				: null;
			if (!available) return;
			status = await api.desktopAiSetup();
			applyStatus(status);
			apiKey = "";
			baseline = draft();
			editing = !status.configured || !status.enabled || status.credential_source === "missing";
		} catch (value) {
			loadError = message(value);
		} finally {
			loading = false;
		}
	}

	function applyStatus(next: DesktopAiSetupStatus) {
		displayName = next.display_name ?? next.persona_id ?? "AIパートナー";
		model = next.model ?? "gpt-5.6-luna";
		instructions = next.custom_instructions ?? defaultInstructions;
		observeUser = next.observe_triggers.includes("user") || next.observe_triggers.length === 0;
		observeTwitchChat = next.observe_triggers.includes("twitch.chat");
		observeChannelPoints = next.observe_triggers.includes("twitch.channel_point");
		observeRuntime = next.observe_triggers.includes("runtime.event");
		extraTriggers = next.observe_triggers
			.filter((channel) => !["user", "twitch.chat", "twitch.channel_point", "runtime.event"].includes(channel))
			.join(", ");
		avatarActions = next.avatar_actions || next.template_id === "streaming-companion";
		avatarLinkId = next.avatar_link_id ?? "ai_avatar";
		avatarBaseKey = next.avatar_base_key ?? "un-motion/frame/ai-avatar";
		avatarPublish = next.avatar_publish;
	}

	function triggers(): string[] {
		const values = [
			observeUser ? "user" : null,
			observeTwitchChat ? "twitch.chat" : null,
			observeChannelPoints ? "twitch.channel_point" : null,
			observeRuntime ? "runtime.event" : null,
			...extraTriggers.split(/[,\s]+/),
		]
			.filter((value): value is string => Boolean(value?.trim()))
			.map((value) => value.trim());
		return [...new Set(values)];
	}

	async function saveAndRestart(restart = true) {
		if (busy || loading || loadError || !status || discardReview) return;
		const observeTriggers = triggers();
		if (!displayName.trim() || !model.trim() || !instructions.trim() || observeTriggers.length === 0) {
			error = "表示名、model、AIの役割、観測対象を指定してください";
			return;
		}
		if (avatarActions && (!avatarLinkId.trim() || !avatarBaseKey.trim())) {
			error = "AvatarLink IDとUNMF/Z base keyを指定してください";
			return;
		}
		const submittedDraft = draft();
		const submittedBaseline = draft("");
		const submittedApiKey = apiKey;
		busy = true;
		error = null;
		try {
			const response = await api.configureDesktopAi({
				expected_revision: status?.revision,
				persona_id: status?.persona_id ?? "companion",
				display_name: displayName.trim(),
				model: model.trim(),
				api_key: apiKey.trim() || undefined,
				custom_instructions: instructions.trim(),
				observe_triggers: observeTriggers,
				avatar_actions: avatarActions,
				avatar_link_id: avatarActions ? avatarLinkId.trim() : undefined,
				avatar_base_key: avatarActions ? avatarBaseKey.trim() : undefined,
				avatar_publish: avatarActions && avatarPublish,
			});
			status = response.status;
			if (currentProfile) onsaved?.(currentProfile.id);
			const unchangedSinceSubmit = draft() === submittedDraft;
			if (apiKey === submittedApiKey) apiKey = "";
			baseline = submittedBaseline;
			savedPending = true;
			confSyncStore.markSavedRequiringRestart();
			toastStore.success("AI設定を保存しました", unchangedSinceSubmit
				? "適用にはこのVACの再起動が必要です"
				: "送信後の入力は未保存のまま保持しています。再起動前に保存または破棄してください");
			if (restart && unchangedSinceSubmit) restartOpen = true;
			busy = false;
		} catch (value) {
			error = message(value);
			busy = false;
		}
	}

	function credentialLabel(source: string): string {
		switch (source) {
			case "environment":
				return "環境変数";
			case "profile_file":
				return "プロファイル内credential";
			case "conf":
				return "既存conf";
			default:
				return "未設定";
		}
	}

	function message(value: unknown): string {
		if (value instanceof ControlApiError) {
			const body = value.body as { detail?: string; error?: string } | null;
			return body?.detail ?? body?.error ?? `${value.status} ${value.statusText}`;
		}
		return value instanceof Error ? value.message : String(value);
	}
</script>

<svelte:window onbeforeunload={(event) => { if (dirty || busy) { event.preventDefault(); event.returnValue = ""; } }} />
<RestartDialog bind:open={restartOpen} currentOnly />

{#if available}
	<section class="vac-panel" aria-label="AIセットアップ">
		<header class="vac-panel-header flex flex-wrap items-center justify-between gap-3 px-4 py-3">
			<div>
				<h3 class="text-sm font-semibold">AIアシスタント</h3>
				<p class="mt-0.5 text-xs opacity-60">役割と観測対象を設定し、常駐AIを開始</p>
			</div>
			{#if status?.configured && !editing}
				<button type="button" class="rounded border border-surface-300-700 px-3 py-1.5 text-xs" onclick={() => (editing = true)}>
					設定を変更
				</button>
			{/if}
		</header>
		{#if !loading}
			<div class="px-4 py-3 text-sm" aria-label="AI基本設定の保存対象">
				<p>この欄の保存対象：起動中のプロファイル{#if currentProfile}「{currentProfile.label}」 · {currentProfile.id}{/if}</p>
				<p class="mt-1 text-xs opacity-70">下の画面設定でプロファイルを選び直しても、この欄の保存先は変わりません。</p>
				{#if currentProfile}<details class="mt-2"><summary class="cursor-pointer text-xs">保存先</summary><p class="mt-1 break-all text-xs">{currentProfile.conf_path}</p></details>{/if}
			</div>
		{/if}
		{#if savedPending}<div class="px-4 py-3"><p class="text-sm" role="status">保存済み。適用にはこのVACの再起動が必要です。</p><button type="button" class="text-sm underline" disabled={busy || dirty} onclick={() => restartOpen = true}>再起動の確認へ…</button></div>{/if}

		{#if discardReview}
			<div class="m-4 rounded border border-warning-500 p-3" role="region" aria-label="AI設定の入力を破棄する確認">
				<p>未保存のAI設定を破棄して、{discardReview.action === "reload" ? "保存内容を読み直す" : "編集を閉じる"}？</p>
				<div class="mt-3 flex flex-wrap gap-3">
					<button bind:this={keepInputButton} class="rounded border border-surface-300-700 px-3 py-2 text-sm focus-visible:outline-2 focus-visible:outline-offset-2" type="button" onclick={() => discardReview = null}>入力を残して戻る</button>
					<button class="rounded border border-warning-500 px-3 py-2 text-sm focus-visible:outline-2 focus-visible:outline-offset-2" type="button" disabled={busy || loading} onclick={() => void confirmDiscard()}>入力を破棄して続ける</button>
				</div>
			</div>
		{/if}
		{#if loading}
			<p class="px-4 py-5 text-sm opacity-60">AI設定を確認中…</p>
		{:else if loadError}
			<div class="px-4 py-4">
				<p role="alert" class="text-sm text-warning-500">AI設定を読み込めませんでした。保存内容を確認できるまで編集・保存はできません。</p>
				<p class="mt-2 text-sm">{loadError}</p>
				<button type="button" class="mt-3 text-sm underline" disabled={busy} onclick={() => requestDiscard("reload")}>もう一度読み込む</button>
			</div>
		{:else if !editing && status}
			<div class="grid gap-3 px-4 py-4 sm:grid-cols-3">
				<div>
					<div class="text-xs opacity-55">状態</div>
					<div class="mt-1 text-sm font-medium">保存設定あり</div>
				</div>
				<div>
					<div class="text-xs opacity-55">モデル</div>
					<div class="mt-1 truncate text-sm font-medium">{status.model}</div>
				</div>
				<div>
					<div class="text-xs opacity-55">Credential</div>
					<div class="mt-1 text-sm font-medium">{credentialLabel(status.credential_source)}</div>
				</div>
			</div>
		{:else}
			<form
				class="grid gap-4 p-4"
				onsubmit={(event) => {
					event.preventDefault();
					void saveAndRestart();
				}}
			>
				<label class="grid gap-1 text-sm">
					<span class="font-medium">表示名</span>
					<input bind:value={displayName} class="rounded border border-surface-300-700 bg-surface-50-950 px-3 py-2" />
				</label>

				<label class="grid gap-1 text-sm">
					<span class="font-medium">AIの役割</span>
					<textarea bind:value={instructions} rows="4" class="rounded border border-surface-300-700 bg-surface-50-950 px-3 py-2"
					></textarea>
				</label>

				<div class="grid gap-4 md:grid-cols-2">
					<label class="grid gap-1 text-sm">
						<span class="font-medium">OpenAI model</span>
						<input bind:value={model} list="desktop-ai-models" class="rounded border border-surface-300-700 bg-surface-50-950 px-3 py-2" />
						<datalist id="desktop-ai-models">
							<option value="gpt-5.6-luna"></option>
							<option value="gpt-6-luna"></option>
							<option value="gpt-6.1-terra"></option>
							<option value="gpt-6.1-sol"></option>
							<option value="gpt-6.2-sol"></option>
							<option value="gpt-6.2-astra"></option>
							<option value="gpt-6-astra"></option>
						</datalist>
					</label>
					<label class="grid gap-1 text-sm">
						<span class="font-medium">OpenAI API key</span>
						<input
							type="password"
							bind:value={apiKey}
							autocomplete="off"
							placeholder={status?.credential_source !== "missing" ? "現在のcredentialを維持" : "sk-..."}
							class="rounded border border-surface-300-700 bg-surface-50-950 px-3 py-2"
						/>
						<span class="text-xs opacity-55">入力値はconfやworkspace bundleへ含めず、profileのcredential fileへ保存</span>
					</label>
				</div>

				<fieldset class="grid gap-2">
					<legend class="text-sm font-medium">観測するもの</legend>
					<div class="flex flex-wrap gap-x-5 gap-y-2 text-sm">
						<label class="flex items-center gap-2"><input type="checkbox" bind:checked={observeUser} />手動入力・音声</label>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={observeTwitchChat} />Twitchコメント</label
						>
						<label class="flex items-center gap-2"
							><input type="checkbox" bind:checked={observeChannelPoints} />チャンネルポイント</label
						>
						<label class="flex items-center gap-2"><input type="checkbox" bind:checked={observeRuntime} />配信イベント</label>
					</div>
				</fieldset>

				<label class="flex items-start gap-2 text-sm">
					<input type="checkbox" bind:checked={avatarActions} class="mt-1" />
					<span
						><span class="font-medium">アバターへ身体反応を送る</span><span class="block text-xs opacity-55"
							>初期状態はUNMF/Z dry-run。接続確認後に実出力へ切替</span
						></span
					>
				</label>
				{#if avatarActions}
					<div class="grid gap-3 border-l-2 border-primary-500/50 pl-4 md:grid-cols-2">
						<label class="grid gap-1 text-sm">
							<span class="font-medium">AvatarLink ID</span>
							<input bind:value={avatarLinkId} class="rounded border border-surface-300-700 bg-surface-50-950 px-3 py-2" />
						</label>
						<label class="grid gap-1 text-sm">
							<span class="font-medium">UNMF/Z base key</span>
							<input bind:value={avatarBaseKey} class="rounded border border-surface-300-700 bg-surface-50-950 px-3 py-2" />
						</label>
						<label class="flex items-center gap-2 text-sm md:col-span-2">
							<input type="checkbox" bind:checked={avatarPublish} />接続先へ実際に出力する
						</label>
					</div>
				{/if}

				<details>
					<summary class="cursor-pointer text-xs opacity-65">高度な観測channel</summary>
					<input
						bind:value={extraTriggers}
						placeholder="game.event, custom.channel"
						class="mt-2 w-full rounded border border-surface-300-700 bg-surface-50-950 px-3 py-2 text-sm"
					/>
				</details>

				{#if error}<p class="rounded border border-error-500/40 bg-error-500/10 px-3 py-2 text-sm text-error-600-400">
						{error}
					</p>{/if}
				<div class="flex flex-wrap gap-2">
					<button type="button" disabled={busy} class="rounded border border-surface-300-700 px-4 py-2 text-sm" onclick={() => void saveAndRestart(false)}>保存のみ</button>
					<button
						type="submit"
						disabled={busy}
						class="rounded bg-primary-500 px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
					>
						{busy ? "保存中…" : "保存して適用へ…"}
					</button>
					{#if status?.configured}
						<button
							type="button"
							disabled={busy}
							class="rounded border border-surface-300-700 px-4 py-2 text-sm"
							onclick={() => requestDiscard("cancel")}>キャンセル</button
						>
					{/if}
				</div>
				<button type="button" class="text-xs underline" disabled={busy} onclick={() => requestDiscard("reload")}>保存内容を読み直す</button>
			</form>
		{/if}
	</section>
{:else}
	<section class="vac-panel p-4 text-sm">
		<h3 class="font-semibold">AIの導入・基本設定</h3>
		{#if loading}
			<p class="mt-2 opacity-65">設定形式を確認中…</p>
		{:else if loadError}
			<p role="alert" class="mt-2 text-warning-500">設定形式を確認できません: {loadError}</p>
			<button type="button" class="mt-3 underline" disabled={busy} onclick={() => requestDiscard("reload")}>もう一度読み込む</button>
		{:else}
			<p class="mt-2 opacity-65">外部設定ファイルで起動しています。AIの編集は「AI詳細設定」から行えます。</p>
		{/if}
	</section>
{/if}

<script lang="ts">
	import { linkedAppDraft } from "../linkedAppDraft.svelte";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	import { onMount, onDestroy, tick } from "svelte";
	import { tabNavStore } from "../tabs.svelte";
	import { api } from "../api";
	import ManagedProfileEditor from "./ManagedProfileEditor.svelte";
	import ProfileImportPicker from "../ProfileImportPicker.svelte";
	import { prepareProfileImport, profileImportBaseName, type ProfileImportSource } from "../profileWorkspaceBundle";
	import type { DesktopProfilesResponse, DesktopProfileTemplateEntry } from "../types";
	import { toastStore } from "../toasts.svelte";

	let store = $state<DesktopProfilesResponse | null>(null);
	const previewRuntimeScope = import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1" || (typeof window !== "undefined" && new URL(window.location.href).searchParams.get("profileScope") === "integrated");
	let instances = $state<Awaited<ReturnType<typeof api.localInstances>> | null>(null);
	let instancesFailed = $state(false);
	let instancesCheckedAt = $state("");
	let templates = $state<DesktopProfileTemplateEntry[]>([]);
	let selectedTemplateId = $state("blank");
	let busy = $state(false);
	let error = $state<string | null>(null);
	let label = $state("");
	let importSource = $state<ProfileImportSource | null>(null);
	let editingProfile = $state<string | null>(null);
	let editOperation = $state<"rename" | "copy">("rename");
	let editLabel = $state("");
	let initialEditLabel = $state("");
	const nameDraftChanged = $derived(editingProfile !== null && editLabel !== initialEditLabel);
	const hasDraft = $derived(!!label.trim() || importSource !== null || nameDraftChanged);
	let confirmation = $state<{kind: "discard"; snapshot: string; source: ProfileImportSource | null} | {kind: "archive"; id: string; label: string; root: string; path: string} | null>(null);
	let keepButton = $state<HTMLButtonElement>();
	$effect(() => { if (confirmation) void tick().then(() => keepButton?.focus()); });
	function draftIdentity() { return JSON.stringify([store?.root,label,editingProfile,editOperation,editLabel,initialEditLabel]); }
	function mayLeave() {
		if (busy || confirmation) return false;
		if (hasDraft) { error = "未保存の入力があります。操作を完了するか、「未保存の入力を破棄」から確認してください。"; return false; }
		return true;
	}
	onMount(() => tabNavStore.guardLeaving(mayLeave));
	function beginNameEdit(id: string, operation: "rename" | "copy", name: string) {
		if (busy || confirmation) return;
		if (nameDraftChanged) { error = "編集中の名前を保持しています。保存またはキャンセルしてから別の操作を選んでください。"; return; }
		editingProfile = id;
		editOperation = operation;
		editLabel = initialEditLabel = name;
	}
	function requestDiscard() {
		if (busy || confirmation || !hasDraft) return;
		confirmation = {kind:"discard",snapshot:draftIdentity(),source:importSource};
	}
	function requestArchive(id: string, name: string) {
		if (!mayLeave() || !store) return;
		const profile=store.profiles.find(p=>p.id===id);
		if (!profile || profile.is_active || id===store.current_profile_id) return;
		confirmation={kind:"archive",id,label:name,root:store.root,path:profile.conf_path};
	}
	async function confirmOperation() {
		if (busy || !confirmation) return;
		const review=confirmation;
		confirmation=null;
		if (review.kind==='discard') {
			if (review.snapshot!==draftIdentity() || review.source!==importSource) { error="確認中に入力や対象が変わりました。もう一度確認してください。"; return; }
			label="";importSource=null;editingProfile=null;editLabel="";initialEditLabel="";error=null;
			return;
		}
		const profile=store?.profiles.find(p=>p.id===review.id);
		if (!profile || store?.root!==review.root || profile.conf_path!==review.path || profile.label!==review.label || profile.is_active || review.id===store.current_profile_id || hasDraft) {
			error="対象・稼働状態・入力が変わったため削除していません。一覧を確認してください。";return;
		}
		await archiveProfile(review.id);
	}
	let contentTarget = $state<{ id: string; label: string } | null>(null);
	let deleted = $state<Awaited<ReturnType<typeof api.deletedDesktopProfiles>>>([]);
	let archiveError = $state("");
	async function setNextProfile(id: string) {
		if (busy || confirmation) return;
		busy = true; error = null;
		try { await api.activateDesktopProfile(id); await load(); toastStore.info("次回起動先を変更しました", "実行中のVACは切り替えていません"); }
		catch (value) { error = message(value); }
		finally { busy = false; }
	}
	async function archiveProfile(id: string) {
		if (busy || confirmation) return;
		busy = true;
		error = null;
		try {
			await api.archiveDesktopProfile(id);
			await load();
		} catch (value) {
			error = message(value);
		} finally {
			busy = false;
		}
	}
	async function restoreProfile(id: string) {
		if (busy || confirmation) return;
		busy = true;
		error = null;
		try {
			await api.restoreDesktopProfile(id);
			await load();
		} catch (value) {
			error = message(value);
		} finally {
			busy = false;
		}
	}

	async function saveProfileOperation() {
		if (!editingProfile || busy || confirmation || !editLabel.trim()) return;
		const target = editingProfile;
		const operation = editOperation;
		const requestedLabel = editLabel.trim();
		const copyId = operation === "copy" ? uniqueProfileId(requestedLabel) : null;
		busy = true;
		error = null;
		try {
			if (operation === "rename") {
				await api.renameDesktopProfile(target, requestedLabel);
			} else {
				const bundle = await api.exportDesktopProfileBundle(target);
				await api.importDesktopProfileBundle(copyId!, requestedLabel, bundle);
			}
			editingProfile = null;
			await load();
		} catch (value) {
			error = message(value);
		} finally {
			busy = false;
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
		const used = new Set(store?.profiles.map((profile) => profile.id) ?? []);
		if (!used.has(base)) return base;
		for (let suffix = 2; suffix < 10_000; suffix += 1) {
			const candidate = `${base}-${suffix}`.slice(0, 64);
			if (!used.has(candidate)) return candidate;
		}
		return `${base}-${Date.now().toString(36)}`.slice(0, 64);
	}

	function message(value: unknown): string {
		return value instanceof Error ? value.message : String(value);
	}

	let loadGeneration = 0;
	let disposed = false;
	onDestroy(() => { disposed = true; loadGeneration++; });
	async function load() {
		if (confirmation) return;
		const generation = ++loadGeneration;
		const current = () => !disposed && generation === loadGeneration;
		try {
			const [nextStore, nextTemplates] = await Promise.all([api.desktopProfiles(), api.desktopProfileTemplates()]);
			if (!current()) return;
			store = nextStore;
			templates = nextTemplates;
			if (!nextTemplates.some((template) => template.id === selectedTemplateId)) {
				selectedTemplateId = nextTemplates.find((template) => template.recommended)?.id ?? nextTemplates[0]?.id ?? "blank";
			}
			error = null;
			try {
				const nextDeleted = await api.deletedDesktopProfiles();
				if (!current()) return;
				deleted = nextDeleted;
				archiveError = "";
			} catch {
				if (!current()) return;
				archiveError = "削除済みプロファイルの一覧を取得できません。";
			}
		} catch (value) {
			if (!current()) return;
			error = message(value);
		}
		if (previewRuntimeScope) {
			try {
				const nextInstances = await api.localInstances();
				if (!current()) return;
				instances = nextInstances;
				instancesFailed = false;
				instancesCheckedAt = new Date().toLocaleTimeString();
			} catch {
				if (!current()) return;
				instances = null;
				instancesFailed = true;
			}
		}
	}

	onMount(() => {
		void load();
	});

	async function switchProfile(profileId: string) {
		if (!tabNavStore.canLeaveCurrent()) return;
		if (linkedAppDraft.value || flowgraphStore.hasUnsavedDrafts || flowgraphStore.mutating) {
			error = "連携アプリまたはFlowgraphの編集中・保存中です。編集画面で保存・破棄を済ませてから再起動してください。次回起動先は変更していません。";
			return;
		}
		if (busy || confirmation) return;
		busy = true;
		error = null;
		let nextStartChanged = false;
		try {
			const activated = await api.activateDesktopProfile(profileId);
			nextStartChanged = true;
			const response = await api.restart({ conf: activated.profile.conf_path, graceful_ms: 800 });
			toastStore.info("プロファイルを切り替えます", `new pid=${response.new_pid}`);
		} catch (value) {
			await load();
			const refreshError = error;
			error = `${nextStartChanged ? "次回起動先の変更は完了しましたが、再起動の完了を確認できません。このPCのVAC一覧で稼働対象を確認してください。次回起動先は一覧から変更できます。 " : "次回起動先の変更を確認できなかったため、再起動は要求していません。 "}${message(value)}${refreshError ? ` 一覧の再取得にも失敗しました: ${refreshError}` : ""}`;
			busy = false;
		}
	}

	async function exportProfile(profileId: string, profileLabel: string) {
		if (busy || confirmation) return;
		busy = true;
		error = null;
		try {
			const bundle = await api.exportDesktopProfileBundle(profileId);
			const url = URL.createObjectURL(bundle);
			const anchor = document.createElement("a");
			anchor.href = url;
			anchor.download = `${profileId}.unvac-profile.zip`;
			anchor.click();
			URL.revokeObjectURL(url);
			toastStore.success("プロファイルを書き出しました", profileLabel);
		} catch (value) {
			error = message(value);
		} finally {
			busy = false;
		}
	}

	async function createProfile() {
		if (busy || confirmation) return;
		const profileLabel = label.trim();
		if (!profileLabel) {
			error = "名前を入力してください";
			return;
		}
		busy = true;
		error = null;
		try {
			await api.createDesktopProfile({
				id: uniqueProfileId(profileLabel),
				label: profileLabel,
				template_id: selectedTemplateId,
			});
			label = "";
			await load();
			toastStore.success("プロファイルを作成しました", profileLabel);
		} catch (value) {
			error = message(value);
		} finally {
			busy = false;
		}
	}

	async function importProfile() {
		if (!importSource || busy || confirmation) return;
		busy = true;
		error = null;
		try {
			const base = profileImportBaseName(importSource);
			const id = uniqueProfileId(base);
			const prepared = await prepareProfileImport(importSource);
			if (prepared.bundle) {
				await api.importDesktopProfileBundle(id, base, prepared.body);
			} else {
				await api.createDesktopProfile({ id, label: base, conf: await prepared.body.text() });
			}
			importSource = null;
			await load();
			toastStore.success("プロファイルを取り込みました", base);
		} catch (value) {
			error = message(value);
		} finally {
			busy = false;
		}
	}

	function templateLabel(templateId: string | null): string {
		return templates.find((template) => template.id === templateId)?.label ?? "既存設定";
	}
</script>

<svelte:window onbeforeunload={(event) => { if (busy || hasDraft) { event.preventDefault(); event.returnValue = ""; } }} />

<section class="vac-panel">
	<header class="vac-panel-header flex items-center justify-between gap-3 px-4 py-3">
		<div class="min-w-0">
			<h2 class="text-sm font-semibold">プロファイル</h2>
			<p class="truncate text-xs opacity-55">{store?.root ?? "読み込み中…"}</p>
		</div>
		<button type="button" class="rounded border border-surface-300-700 px-2 py-1 text-xs" onclick={() => void load()} disabled={busy || confirmation !== null}>
			更新
		</button>
	</header>
	<p class="px-4 py-3 text-xs opacity-65">
		この画面の操作は接続中のVACに適用されます。「切替して再起動」は同時起動を追加する操作ではありません。
	</p>

	{#if hasDraft && !confirmation}
		<button class="mx-4 my-2 text-sm underline" disabled={busy} onclick={requestDiscard}>未保存の入力を破棄</button>
	{/if}
	{#if confirmation}
		<div class="m-4 rounded border border-warning-500 p-3 text-sm" role="region" aria-label="プロファイル操作の確認">
			{#if confirmation.kind==='archive'}
				<p>「{confirmation.label}」を削除済みに移す？ 関連ファイルは保管し、後から復元できる。</p>
				<p class="mt-1 break-all text-xs">対象：{confirmation.id} · {confirmation.path}</p>
			{:else}<p>作成・名前変更・取込の未保存入力を破棄する？ 保存済みプロファイルは変更しない。</p>{/if}
			<div class="mt-3 flex flex-wrap gap-3">
				<button bind:this={keepButton} class="rounded border border-surface-300-700 px-3 py-2" onclick={() => confirmation=null}>戻る</button>
				<button class="rounded border border-warning-500 px-3 py-2" disabled={busy} onclick={() => void confirmOperation()}>{confirmation.kind==='archive' ? '削除済みに移す' : '未保存入力を破棄する'}</button>
			</div>
		</div>
	{/if}

	{#if error}
		<p class="border-b border-error-500/40 bg-error-500/10 px-4 py-2 text-xs text-error-900-100">{error}</p>
	{/if}

	<div class="divide-y divide-surface-200-800">
		{#each store?.profiles ?? [] as profile (profile.id)}
			{@const running = instances?.instances.filter(instance => instance.profile?.id === profile.id) ?? []}
			<div class="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
				<div class="min-w-0">
					<div class="flex items-center gap-2 text-sm font-medium">
						<span class="truncate">{profile.label}</span>
						{#if profile.id === store?.current_profile_id}<span class="text-xs text-primary-500">このVACで使用中</span>{/if}
						{#if profile.is_active}<span class="text-xs opacity-65">次回起動</span>{/if}
					</div>
					<div class="truncate text-xs opacity-55">{templateLabel(profile.template_id)}</div>
					{#if previewRuntimeScope}
						<div class="mt-2 text-xs" aria-label={`${profile.label}の稼働先`}>
							{#if (store?.profiles.filter(item => item.label === profile.label).length ?? 0) > 1}<p>プロファイルID: {profile.id}</p>{/if}
							{#if instancesFailed || instances?.profile_lookup_ok !== true}
								<p>稼働先との対応は未確認</p>
							{:else if running.length === 0}
								<p class="opacity-65">登録済みVAC一覧に稼働先なし</p>
							{:else}
								{#each running as instance (instance.pid)}
									<p>{instance.pid === instances?.current_pid ? "この画面で操作中" : "別のVACで使用中"} · PID {instance.pid}</p>
								{/each}
							{/if}
							<details class="mt-1"><summary class="cursor-pointer opacity-65">保存先</summary><p class="break-all">{profile.conf_path}</p></details>
						</div>
					{/if}
				</div>
				<div class="flex flex-wrap items-center gap-2">
					<button type="button" class="rounded border border-surface-300-700 px-3 py-1.5 text-xs disabled:opacity-50" disabled={busy || confirmation !== null || profile.is_active} aria-label={`${profile.label}を次回起動先に設定`} onclick={() => void setNextProfile(profile.id)}>次回起動に設定</button>
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1.5 text-xs"
						disabled={busy || confirmation !== null}
						aria-label={`${profile.label}の内容を編集`}
						onclick={() => (contentTarget = { id: profile.id, label: profile.label })}>内容編集</button
					>
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1.5 text-xs disabled:opacity-50"
						disabled={busy || confirmation !== null || profile.is_active || profile.id === store?.current_profile_id}
						aria-label={`${profile.label}を削除`}
						title={profile.is_active
							? "次回起動先を別のプロファイルに変更してから削除できます"
							: "使用中のプロファイルは削除できません"}
						onclick={() => requestArchive(profile.id, profile.label)}>削除</button
					>
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1.5 text-xs"
						disabled={busy || confirmation !== null}
						aria-label={`${profile.label}の名前を変更`}
						onclick={() => beginNameEdit(profile.id, "rename", profile.label)}>名前変更</button
					>
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1.5 text-xs"
						disabled={busy || confirmation !== null}
						aria-label={`${profile.label}の設定をコピー`}
						onclick={() => beginNameEdit(profile.id, "copy", `${profile.label} コピー`)}>設定をコピー</button
					>
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1.5 text-xs disabled:opacity-50"
						aria-label={`${profile.label}を書き出す`}
						disabled={busy || confirmation !== null}
						onclick={() => void exportProfile(profile.id, profile.label)}
					>
						書き出す
					</button>
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1.5 text-xs disabled:opacity-50"
						aria-label={`${profile.label}へ切替して再起動`}
						disabled={busy || confirmation !== null || !store?.managed_mode || profile.id === store?.current_profile_id}
						onclick={() => void switchProfile(profile.id)}
					>
						切替して再起動
					</button>
				</div>
			</div>
			{#if editingProfile === profile.id}
				<form
					class="bg-surface-100-900 p-4"
					onsubmit={(event) => {
						event.preventDefault();
						void saveProfileOperation();
					}}
				>
					<label class="text-xs"
						>{editOperation === "rename" ? "新しい表示名" : "コピーの名前"}<input
							class="ml-2 rounded border border-surface-300-700 bg-surface-50-950 p-2"
							bind:value={editLabel}
							disabled={busy || confirmation !== null}
							maxlength="80"
						/></label
					>
					{#if editOperation === "copy"}<p class="my-2 text-xs opacity-65">
							共有用ZIPと同じ範囲の設定・関連ファイルをコピーします。認証情報はコピーせず、新しいプロファイルで設定します。起動はしません。
						</p>{/if}
					<button
						type="submit"
						class="ml-2 rounded bg-primary-500 px-3 py-2 text-xs text-white"
						disabled={busy || confirmation !== null || !editLabel.trim()}>保存</button
					>
					<button type="button" class="ml-2 text-xs" disabled={busy || confirmation !== null} onclick={() => (editingProfile = null)}>キャンセル</button>
				</form>
			{/if}
		{/each}
	</div>
	{#if previewRuntimeScope}
		<p class="px-4 py-2 text-xs opacity-65">稼働先は{instancesCheckedAt || "未取得"}時点の登録情報。「更新」で再取得します。同じ保存先を使うVACが複数あっても、編集・次回起動先の変更だけでは実行中の設定は切り替わりません。</p>
	{/if}

	<div class="grid gap-5 border-t border-surface-200-800 p-4 md:grid-cols-2">
		<div>
			<h3 class="text-sm font-medium">新規作成</h3>
			<label class="mt-2 block text-xs">
				<span class="opacity-60">用途</span>
				<select
					bind:value={selectedTemplateId}
					disabled={busy || confirmation !== null}
					class="mt-1 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1.5 text-sm"
				>
					{#each templates as template (template.id)}
						<option value={template.id}>{template.label}</option>
					{/each}
				</select>
			</label>
			<p class="mt-2 text-xs opacity-65">{templates.find((template) => template.id === selectedTemplateId)?.description}</p>
			<p class="mt-1 text-xs opacity-65">作成だけでは起動・切替しません。既存のプロファイルはそのまま残ります。</p>
			<input
				type="text"
				bind:value={label}
				disabled={busy || confirmation !== null}
				placeholder="名前"
				aria-label="プロファイル名"
				class="mt-2 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1.5 text-sm"
			/>
			<button
				type="button"
				class="mt-2 rounded bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white disabled:opacity-50"
				disabled={busy || confirmation !== null || !label.trim() || templates.length === 0}
				onclick={() => void createProfile()}
			>
				作成
			</button>
		</div>
		<div>
			<h3 class="text-sm font-medium">既存設定を取り込む</h3>
			<p class="mt-1 text-xs opacity-55">信頼できる設定のみ使用</p>
			<ProfileImportPicker bind:source={importSource} disabled={busy || confirmation !== null} />
			<button
				type="button"
				class="mt-2 rounded border border-surface-300-700 px-3 py-1.5 text-xs font-medium disabled:opacity-50"
				disabled={busy || confirmation !== null || !importSource}
				onclick={() => void importProfile()}
			>
				取り込む
			</button>
		</div>
	</div>
	<details class="border-t border-surface-200-800 p-4">
		<summary class="cursor-pointer text-sm font-semibold">削除済みプロファイル（{deleted.length}）</summary>
		<p class="mt-2 text-xs opacity-65">元のIDに復元します。同じIDが存在する場合は上書きしません。復元だけでは起動しません。</p>
		{#if archiveError}<p class="mt-2 text-xs text-warning-500">{archiveError}</p>{/if}
		{#each deleted as item (item.archive_id)}
			<div class="mt-3 flex flex-wrap items-center justify-between gap-2">
				<span class="text-xs">{item.label} · {item.profile_id}<span class="block break-all text-[10px] opacity-60">保管ID: {item.archive_id}</span></span><button
					type="button"
					class="rounded border px-3 py-1.5 text-xs"
					disabled={busy || confirmation !== null}
					aria-label={`${item.label}を復元`}
					onclick={() => void restoreProfile(item.archive_id)}>復元</button
				>
			</div>
		{/each}
	</details>
</section>

{#if contentTarget}
	<ManagedProfileEditor
		id={contentTarget.id}
		label={contentTarget.label}
		current={contentTarget.id === store?.current_profile_id}
		onclose={() => (contentTarget = null)}
	/>
{/if}

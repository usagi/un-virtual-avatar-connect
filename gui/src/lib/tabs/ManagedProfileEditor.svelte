<script lang="ts">
	import { linkedAppDraft } from "../linkedAppDraft.svelte";
	import { onMount, tick } from "svelte";
	import { api } from "../api";
	import { tabNavStore } from "../tabs.svelte";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	import { toastStore } from "../toasts.svelte";
	let { id, label, current, onclose }: { id: string; label: string; current: boolean; onclose: () => void } = $props();
	let content = $state("");
	let original = $state("");
	let revision = $state("");
	let busy = $state(false);
	let loaded = $state(false);
	let error = $state("");
	let saved = $state(false);
	let confirmation = $state<{ kind: "discard" | "restart"; id: string; content: string; revision: string } | null>(null);
	let confirmationCancel = $state<HTMLButtonElement>();
	let panel = $state<HTMLDivElement>();
	$effect(() => {
		if (confirmation) void tick().then(() => confirmationCancel?.focus());
	});
	function cancelConfirmation() {
		if (busy) return;
		confirmation = null;
		void tick().then(() => panel?.focus());
	}
	function onKeyDown(event: KeyboardEvent) {
		if (event.key === "Escape") {
			event.preventDefault();
			if (confirmation && !busy) cancelConfirmation();
			else requestClose();
		}
		if (event.key === "Tab") {
			const items = Array.from(panel?.querySelectorAll<HTMLElement>("button:not(:disabled), textarea:not(:disabled)") ?? []);
			const next = event.shiftKey ? items.at(-1) : items[0];
			if (document.activeElement === (event.shiftKey ? items[0] : items.at(-1)) || document.activeElement === panel) {
				event.preventDefault();
				next?.focus();
			}
		}
	}
	const dirty = $derived(content !== original);
	function mayLeave() {
		if (busy) return false;
		if (dirty) {
			error = "未保存の編集があります。保存するか、「閉じる」から破棄を確認してください。";
			return false;
		}
		return confirmation === null;
	}
	function requestClose() {
		if (busy) return;
		if (!dirty) { onclose(); return; }
		confirmation = { kind: "discard", id, content, revision };
		error = "";
	}
	function confirmDiscard() {
		if (busy || confirmation?.kind !== "discard") return;
		if (confirmation.id !== id || confirmation.content !== content || confirmation.revision !== revision) {
			error = "確認後に編集内容が変わりました。閉じる操作から確認し直してください。";
			confirmation = null;
			return;
		}
		confirmation = null;
		onclose();
	}
	onMount(() => {
		const previous = document.activeElement as HTMLElement | null;
		void tick().then(() => panel?.focus());
		void load();
		const unguard = tabNavStore.guardLeaving(mayLeave);
		return () => {
			unguard();
			if (previous?.isConnected) previous.focus();
		};
	});
	function beforeUnload(event: BeforeUnloadEvent) {
		if (dirty || busy) {
			event.preventDefault();
			event.returnValue = "";
		}
	}
	function describeError(value: unknown): string {
		if (typeof value === "object" && value !== null && "body" in value) {
			const body = value.body;
			if (typeof body === "object" && body !== null && "detail" in body && typeof body.detail === "string" && body.detail.trim()) {
				return body.detail;
			}
		}
		return value instanceof Error ? value.message : String(value);
	}
	async function load() {
		if (busy || loaded) return;
		busy = true;
		error = "";
		try {
			const result = await api.desktopProfileContent(id);
			if (!result.revision) throw new Error("設定の版情報を取得できませんでした。再読込してください。");
			content = original = result.content;
			revision = result.revision;
			loaded = true;
		} catch (value) {
			error = describeError(value);
		} finally {
			busy = false;
		}
	}
	async function save() {
		if (busy || confirmation || !loaded || !revision || !dirty) return;
		busy = true;
		error = "";
		try {
			const result = await api.saveDesktopProfileContent(id, content, revision);
			original = result.content;
			revision = result.revision;
			saved = true;
		} catch (value) {
			error = describeError(value);
		} finally {
			busy = false;
		}
	}
	async function apply() {
		if (busy || !loaded || !revision || dirty || !current) return;
		if (linkedAppDraft.value || flowgraphStore.hasUnsavedDrafts || flowgraphStore.mutating) {
			error = "連携アプリまたはFlowgraphの編集中・保存中です。保存・破棄を済ませてから再起動してください。";
			return;
		}
		if (!tabNavStore.canLeaveCurrent()) return;
		confirmation = { kind: "restart", id, content, revision };
	}
	async function confirmApply() {
		if (busy || confirmation?.kind !== "restart") return;
		const reviewed = confirmation;
		confirmation = null;
		if (!loaded || dirty || !current || reviewed.id !== id || reviewed.revision !== revision || reviewed.content !== content) {
			error = "確認後に対象や設定が変わりました。適用操作から確認し直してください。";
			return;
		}
		if (linkedAppDraft.value || flowgraphStore.hasUnsavedDrafts || flowgraphStore.mutating || !tabNavStore.canLeaveCurrent()) {
			error = "編集中または保存中の内容があります。完了してから適用してください。";
			return;
		}
		busy = true;
		error = "";
		try {
			const profiles = await api.desktopProfiles();
			const profile = profiles.profiles.find((item) => item.id === id);
			if (!profile || profiles.current_profile_id !== id) throw new Error("操作対象が変わっています。画面を再確認してください。");
			const latest = await api.desktopProfileContent(id);
			if (latest.revision !== revision) throw new Error("保存後に設定が変更されています。内容を再確認してから適用してください。");
			if (linkedAppDraft.value || flowgraphStore.hasUnsavedDrafts || flowgraphStore.mutating || reviewed.id !== id || reviewed.revision !== revision || dirty || !current) throw new Error("確認中に対象や編集状態が変わりました。適用操作から確認し直してください。");
			await api.restart({ conf: profile.conf_path, graceful_ms: 800 });
			toastStore.info("保存した設定で再起動しています", label);
			onclose();
		} catch (value) {
			error = describeError(value);
			busy = false;
		}
	}
</script>

<svelte:window onbeforeunload={beforeUnload} />
<div class="fixed inset-0 z-40 flex items-center justify-center bg-black/50 p-4" role="presentation">
	<div
		bind:this={panel}
		tabindex="-1"
		onkeydown={onKeyDown}
		role="dialog"
		aria-modal="true"
		aria-label={`プロファイル内容: ${label}`}
		class="flex max-h-[90dvh] w-full max-w-4xl flex-col gap-3 overflow-y-auto rounded border border-surface-300-700 bg-surface-50-950 p-5"
	>
		<h2 class="font-semibold">{label} の内容編集 <span class="text-xs opacity-60">{id}</span></h2>
		<p class="text-xs opacity-65">保存先はこのプロファイルのconf.toml。保存だけでは実行中のVACへ適用されません。</p>
		{#if error}<p role="alert" class="text-xs text-warning-500">{error}</p>{/if}
		{#if !loaded && !busy}<button type="button" class="self-start rounded border px-3 py-2 text-xs" onclick={() => void load()}>設定内容を再読込</button>{/if}
		{#if saved && !dirty}<p role="status" class="text-xs text-primary-500">保存済み。実行中への適用には再起動が必要です。</p>{/if}
		{#if confirmation}
			<section aria-label="プロファイル操作の確認" class="rounded border border-warning-500 p-3 text-sm">
				<p class="font-semibold">{label} · {confirmation.id}</p>
				<p class="my-2">{confirmation.kind === "discard" ? "このプロファイルの未保存の編集を破棄して閉じます。保存済みの設定は変更しません。" : "このプロファイルの保存内容で、このVACを再起動します。実行中の処理は引き継がれません。"}</p>
				<button bind:this={confirmationCancel} type="button" class="rounded border px-3 py-2" disabled={busy} onclick={cancelConfirmation}>編集画面に戻る</button>
				<button type="button" class="ml-2 rounded border border-warning-500 px-3 py-2" disabled={busy} onclick={() => confirmation?.kind === "discard" ? confirmDiscard() : void confirmApply()}>{confirmation.kind === "discard" ? "編集を破棄して閉じる" : "保存した設定で再起動する"}</button>
			</section>
		{/if}
		<textarea
			aria-label="プロファイルの設定内容"
			bind:value={content}
			disabled={busy || !revision || !!confirmation}
			spellcheck="false"
			class="min-h-72 w-full rounded border border-surface-300-700 bg-surface-100-900 p-3 font-mono text-xs"
		></textarea>
		<div class="flex flex-wrap justify-end gap-2">
			<button
				type="button"
				class="rounded border px-3 py-2 text-xs"
				disabled={busy || !!confirmation}
				onclick={requestClose}>閉じる</button
			>
			<button
				type="button"
				class="rounded bg-primary-500 px-3 py-2 text-xs text-white"
				disabled={busy || !dirty || !revision || !!confirmation}
				onclick={() => void save()}>保存</button
			>
			{#if current}<button
					type="button"
					class="rounded border px-3 py-2 text-xs"
					disabled={busy || dirty || !revision || !!confirmation}
					onclick={() => void apply()}>保存内容を適用（再起動）</button
				>{/if}
		</div>
	</div>
</div>

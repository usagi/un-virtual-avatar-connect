<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "./api";
	let { onstatus }: { onstatus?: (status: { count: number; failed: boolean }) => void } = $props();
	$effect(() => { onstatus?.({ count: result?.instances.length ?? 0, failed }); });
	let result = $state<Awaited<ReturnType<typeof api.localInstances>> | null>(null);
	let failed = $state(false);
	let namedIdentity = $state(import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1" || new URL(window.location.href).searchParams.get("identity") === "names");
	let compactIdentity = $state(import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1" || new URL(window.location.href).searchParams.get("targetDetails") === "compact");
	const currentInstance = $derived(result?.instances.find((instance) => instance.pid === result?.current_pid));
	const currentLabel = $derived(result?.profile_lookup_ok === false ? null : currentInstance?.profile?.label);
	$effect(() => {
		if (import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1") return;
		const change = (event: Event) => { namedIdentity = (event as CustomEvent).detail === "names"; };
		window.addEventListener("un-preview-identity", change);
		const detailChange = (event: Event) => { compactIdentity = (event as CustomEvent).detail === "compact"; };
		window.addEventListener("un-preview-target-details", detailChange);
		return () => { window.removeEventListener("un-preview-identity", change); window.removeEventListener("un-preview-target-details", detailChange); };
	});
	function destination(value: string): string | null {
		try {
			const url = new URL(value);
			if (url.protocol !== "http:" || url.username || url.password || url.search || url.hash || url.pathname !== "/gui/") return null;
			return url.href;
		} catch {
			return null;
		}
	}
	onMount(() => {
		let disposed = false,
			pending = false;
		async function refresh() {
			if (pending) return;
			pending = true;
			try {
				const next = await api.localInstances();
				if (!disposed) {
					result = next;
					failed = false;
				}
			} catch {
				if (!disposed) failed = true;
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

<details data-local-vac-list class="border-b border-surface-200-800 px-3 py-3" open>
	<summary class="cursor-pointer text-xs font-semibold"
		>このPCのVAC <span class="opacity-60">{result?.instances.length ?? "—"}</span>
		{#if namedIdentity}
			<span class="mt-1 block break-words font-normal opacity-80">
				{#if failed}一覧の更新を確認できません{#if currentInstance} · 前回の操作対象: {currentLabel || `PID ${currentInstance.pid}`}{/if}
				{:else if !result}操作対象を確認中…
				{:else if currentInstance}操作中: {currentLabel || `PID ${currentInstance.pid}`}
				{:else}この画面に対応するVACを確認できません{/if}
			</span>
		{/if}
	</summary
	>
	{#if failed}<p role="status" class="mt-2 text-xs text-warning-500">{result ? "一覧を更新できません。前回取得した対象を表示しています。現在の稼働状態は未確認です。" : "一覧を取得できません。接続の回復後に再確認します。"}</p>{/if}
	{#if result}
		<div class="max-h-48 overflow-y-auto">
			{#each result.instances as instance (`${instance.pid}:${instance.started_at}`)}
				{@const url = destination(instance.gui_url)}
				<div data-vac-instance={`${instance.pid}:${instance.started_at}`} class="mt-2 rounded border border-surface-200-800 p-2 text-xs">
					{#if namedIdentity}
						{#if instance.profile && result.profile_lookup_ok !== false}
							<p class="font-semibold">{instance.profile.label}</p>
							{#if !compactIdentity}<p class="mt-1 break-all opacity-70">構成ID: {instance.profile.id}</p>{/if}
							{#if instance.profile.next_start}<p class="mt-1 opacity-70">この構成を次回起動に使用</p>{/if}
						{:else}
							<p class="font-semibold">{result.profile_lookup_ok === false ? "保存プロファイル名を取得できません" : "保存プロファイルとの対応不明"}</p>
						{/if}
					{/if}
					<p class="font-semibold">{instance.pid === result.current_pid ? "操作中のVAC" : "別のVAC"}{#if !(namedIdentity && compactIdentity)} · PID {instance.pid}{/if}</p>
					{#if namedIdentity && compactIdentity}
						<details class="mt-1 text-[10px]"><summary class="cursor-pointer">対象の詳細</summary>
							{#if instance.profile && result.profile_lookup_ok !== false}<p>構成ID: {instance.profile.id}</p>{/if}
							<p>PID {instance.pid}</p><p class="break-all">{instance.conf_path}</p>
						</details>
					{:else}<p class="mt-1 break-all text-[10px] opacity-60">{instance.conf_path}</p>{/if}
					{#if url && instance.pid !== result.current_pid}<a
							href={url}
							target="_blank"
							rel="noopener noreferrer"
							class="mt-2 block text-primary-500">専用画面を開く ↗</a
						>{/if}
				</div>
			{/each}
		</div>
		<p class="mt-2 text-[10px] opacity-55">同じ設定保管先の対応版VACを表示。{failed ? "前回の取得時点の一覧です。" : "プロセスの存在を確認しています。"}通信状態と認証は各専用画面で確認します。</p>
	{:else if !failed}<p class="mt-2 text-xs opacity-60">確認中…</p>{/if}
</details>

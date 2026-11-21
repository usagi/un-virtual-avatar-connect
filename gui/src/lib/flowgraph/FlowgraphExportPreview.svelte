<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "../api";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	import type { ZipExportRequest, FlowgraphCompanionInventory, FlowgraphSharePlan } from "../types";
	let { request, filename, root, onclose, assetCandidates = [] }: { request: ZipExportRequest; filename: string; root: string; onclose: () => void; assetCandidates?: NonNullable<FlowgraphSharePlan['asset_candidates']> } = $props();
	let dialog: HTMLDialogElement;
	let inventory = $state<FlowgraphCompanionInventory | null>(null);
	let selected = $state<string[]>([]);
	let error = $state("");
 let errorDetail = $state("");
	let busy = $state(false);
	let query = $state("");
	const visible = $derived(inventory?.files.filter(file => file.path.toLowerCase().includes(query.toLowerCase())) ?? []);
	const total = $derived(inventory?.files.filter(file => selected.includes(file.path)).reduce((sum, file) => sum + file.bytes, 0) ?? 0);
	onMount(() => {
		let active = true;
		dialog.showModal();
		void api.flowgraphCompanions().then(value => {
			if (!active) return;
			if (value.root_dir !== root) { error = "保存先が変わりました。閉じて対象を確認し直してください。"; return; }
			inventory = value;
		}).catch(reason => { if (active) { error = "素材一覧を取得できませんでした。閉じてから、もう一度書出しを開いてください。"; errorDetail = String(reason); } });
		return () => { active = false; };
	});
	async function download() {
		if (!inventory || busy) return;
		busy = true;
		error = "";
		const blob = await flowgraphStore.exportZip({ ...$state.snapshot(request), companions: [...selected], expected_root: root });
		busy = false;
		if (!blob) { error = "書出しに失敗しました。選択は保持しています。保存先や素材の状態を確認してから再試行してください。"; return; }
		const url = URL.createObjectURL(blob);
		try {
			const link = document.createElement("a");
			link.href = url; link.download = filename; document.body.append(link); link.click(); link.remove();
		} finally { URL.revokeObjectURL(url); }
		onclose();
	}
</script>

<dialog bind:this={dialog!} onclose={onclose} oncancel={(event) => { if (busy) event.preventDefault(); }} aria-labelledby="export-preview-title">
	<header><div><small>共有するファイルと素材</small><h2 id="export-preview-title">素材と一緒に書き出す</h2></div><button disabled={busy} onclick={onclose} aria-label="書出し画面を閉じる">閉じる</button></header>
	<p><strong>{filename}</strong> · 保存済みの構成を書き出します。</p>
	<details><summary>対象と保存先</summary><code>{root}</code><pre>{JSON.stringify(request.targets, null, 2)}</pre></details>
	<p>同梱したいファイルだけを選択してください。選択しなければ、従来どおり構成だけを書き出します。</p>
	{#if assetCandidates.length}
		<details open><summary>構成に記載された素材 · {assetCandidates.length}件</summary>
			<p>実際に使用されるかは処理条件によります。必要なものを下の一覧から選択してください。</p>
			<ul>{#each assetCandidates as candidate}<li><code>{candidate.target ?? candidate.raw}</code> — {candidate.error ? '見つからないか、同梱できない参照です' : !inventory ? '一覧を確認中' : !inventory.files.some(file => file.path === candidate.target) ? '素材一覧にありません。この画面では選択できません' : selected.includes(candidate.target!) ? '選択済み' : '未選択'}<br /><small>{candidate.from_file} · {candidate.field}</small></li>{/each}</ul>
		</details>
	{/if}
	{#if error}<p role="alert">{error}</p>{#if errorDetail}<details><summary>エラーの詳細</summary><pre>{errorDetail}</pre></details>{/if}{/if}
	{#if !inventory && !error}<p role="status">素材を確認中…</p>
	{:else if inventory}
		{#if inventory.truncated}<p role="status">件数が多いため一覧の一部を省略しています。この一覧だけでは素材の網羅を確認できません。</p>{/if}
		<label>素材を検索<input type="search" bind:value={query} /></label>
		<div class="assets">
			{#each visible as file (file.path)}<label class="asset"><input type="checkbox" bind:group={selected} value={file.path} disabled={busy} /><span>{file.path}</span><small>{file.bytes.toLocaleString()} B</small></label>{/each}
			{#if visible.length === 0}<p>該当する素材はありません。</p>{/if}
		</div>
		<p>{selected.length}件選択 · 素材合計 {total.toLocaleString()} B</p>
		<p class="note">動的に組み立てるパスや従来形式のパスは、素材候補に含まれません。必要な素材が揃っているか確認してください。</p>
	{/if}
	<footer><button disabled={!inventory || busy} onclick={download}>{busy ? "ZIPを生成中…" : "選択した内容を書き出す"}</button></footer>
</dialog>

<style>
	dialog{margin:auto;inset:0;width:min(720px,92vw);max-height:85vh;overflow:auto;border:1px solid #607b87;border-radius:12px;padding:24px;background:#17222c;color:#edf4f5}dialog::backdrop{background:#080e16b3}header,footer,.asset{display:flex;align-items:center;gap:12px}header{justify-content:space-between}h2{font-size:22px;margin:4px 0}p{margin:16px 0}button,input{font:inherit}button{padding:8px 14px;border:1px solid #91bdc2;border-radius:6px;background:#24434b;color:inherit}button:disabled{opacity:.5}input[type=search]{display:block;width:100%;padding:8px;margin:8px 0;background:#101820;color:inherit;border:1px solid #607b87}.assets{max-height:30vh;overflow:auto}.asset{padding:10px 0;border-bottom:1px solid #344450}.asset span{flex:1;overflow-wrap:anywhere}small,.note{color:#b8cbd1}.note{font-size:13px}pre,code{font-size:12px;overflow-wrap:anywhere;white-space:pre-wrap}footer{justify-content:flex-end}button:focus-visible,input:focus-visible{outline:2px solid #9ef0d9;outline-offset:3px}
</style>


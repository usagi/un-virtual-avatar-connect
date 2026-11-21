<script lang="ts">
	/**
	 * Phase δ-6e: ファイルツリー（左サイドバー）。
	 *
	 * - `flowgraphStore.tree.files` を平坦リストのまま fq の `/` 区切りで階層化して表示。
	 * - 選択中ファイル（`store.currentFq`）はハイライト。
	 * - 右クリック相当の UI は今フェーズでは実装せず、ヘッダにボタンを並べて賄う:
	 *    "+ New"（fq を入力して新規）/ 選択中に対する Rename（δ-7）/ Delete / Open external。
	 *    Rename は API として未提供なので今フェーズでは隠し、削除→新規作成で代替する方針（spec §8.6）。
	 * - エラー（parse_error）はファイル行に赤マークで表示。
	 */
	import { onMount } from "svelte";
	import { tabNavStore } from "../tabs.svelte";
	import { flowgraphStore } from "../flowgraphStore.svelte";
	import type { FlowgraphTreeFileEntry } from "../types";
	let { onopened }: { onopened?: () => void } = $props();

	type TreeNode =
		| { kind: "dir"; name: string; fullPath: string; children: TreeNode[] }
		| { kind: "file"; name: string; fullPath: string; entry: FlowgraphTreeFileEntry };

	function buildTree(files: FlowgraphTreeFileEntry[]): TreeNode[] {
		const root: TreeNode[] = [];
		for (const f of files) {
			const parts = f.fq.split("/").filter(Boolean);
			if (parts.length === 0) continue;
			let here = root;
			let acc = "";
			for (let i = 0; i < parts.length; i++) {
				const name = parts[i];
				acc = acc ? `${acc}/${name}` : name;
				const isLast = i === parts.length - 1;
				if (isLast) {
					here.push({ kind: "file", name, fullPath: acc, entry: f });
				} else {
					let dir = here.find((c): c is Extract<TreeNode, { kind: "dir" }> => c.kind === "dir" && c.name === name);
					if (!dir) {
						dir = { kind: "dir", name, fullPath: acc, children: [] };
						here.push(dir);
					}
					here = dir.children;
				}
			}
		}
		sortRecursive(root);
		return root;
	}

	function sortRecursive(nodes: TreeNode[]) {
		nodes.sort((a, b) => {
			if (a.kind !== b.kind) return a.kind === "dir" ? -1 : 1;
			return a.name.localeCompare(b.name);
		});
		for (const n of nodes) {
			if (n.kind === "dir") sortRecursive(n.children);
		}
	}

	const tree = $derived(buildTree(flowgraphStore.tree?.files ?? []));

	async function onSelect(fq: string) {
		if (await flowgraphStore.openFile(fq)) onopened?.();
	}

	let operation = $state<"create" | "delete" | null>(null);
	let nameDraft = $state("");
	let operationRoot = $state<string | null>(null);
	let deleteTarget = $state<{ fq: string; root: string; content: string } | null>(null);
	let operationError = $state<string | null>(null);
	let pending = $state(false);
	onMount(() => tabNavStore.guardLeaving(() => {
		if (pending || operation) { operationError = "作成・削除の確認を完了するか、キャンセルしてください。"; return false; }
		return true;
	}));
	function onCreate() {
		if (pending || flowgraphStore.mutating || operation) return;
		operationRoot = flowgraphStore.tree?.root_dir ?? null;
		nameDraft = ""; operationError = null; operation = "create";
	}
	function onDelete() {
		if (pending || flowgraphStore.mutating || operation || !flowgraphStore.currentFq || !flowgraphStore.currentFile) return;
		const file = flowgraphStore.currentFile;
		deleteTarget = { fq: flowgraphStore.currentFq, root: file.root_dir ?? "", content: file.raw_toml };
		operationError = null; operation = "delete";
	}
	function cancelOperation() {
		if (pending) return;
		operation = null; nameDraft = ""; deleteTarget = null; operationError = null;
	}
	async function confirmOperation() {
		if (!operation || pending || flowgraphStore.mutating) return;
		operationError = null;
		if (operation === "create") {
			const fq = nameDraft.trim().replace(/\.flowgraph\.toml$/, "");
			if (!fq) { operationError = "ファイル名を入力してください。"; return; }
			if (!operationRoot || flowgraphStore.tree?.root_dir !== operationRoot) { operationError = "保存先が変わりました。キャンセルして確認し直してください。"; return; }
			pending = true;
			try { if (await flowgraphStore.createFile(fq)) { operation = null; nameDraft = ""; onopened?.(); } else operationError = "作成できませんでした。入力は保持しています。"; }
			finally { pending = false; }
		} else {
			const target = deleteTarget;
			if (!target?.root || flowgraphStore.currentFq !== target.fq || flowgraphStore.currentFile?.root_dir !== target.root || flowgraphStore.currentFile?.raw_toml !== target.content) { operationError = "削除対象が変わりました。キャンセルして確認し直してください。"; return; }
			pending = true;
			try { if (await flowgraphStore.deleteFile(target.fq, target.content, target.root)) { operation = null; deleteTarget = null; } else operationError = "削除できませんでした。未保存の編集や診断を確認してください。"; }
			finally { pending = false; }
		}
	}

</script>

<div class="flex h-full flex-col">
	<div class="flex items-center justify-between border-b border-surface-200-800 p-2">
		<span class="text-xs font-semibold uppercase tracking-wider opacity-60">Files</span>
		<div class="flex gap-1">
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-200-800"
				title="新規 flowgraph ファイル"
				onclick={onCreate}
				disabled={pending || operation !== null || flowgraphStore.mutating}
			>
				+
			</button>
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-200-800 disabled:opacity-30"
				disabled={!flowgraphStore.currentFq || pending || operation !== null || flowgraphStore.mutating}
				title="選択中ファイルを削除"
				onclick={onDelete}
			>
				×
			</button>
		</div>
	</div>
	{#if operation}
		<section aria-label="Flowgraphファイル操作" class="m-2 rounded border border-surface-300-700 p-2 text-xs">
			<p class="font-semibold">{operation === "create" ? "ファイルを作成" : `${deleteTarget?.fq} を削除しますか？`}</p>
			<p class="my-2 break-all">保存先: {operation === "create" ? operationRoot : deleteTarget?.root}</p>
			{#if operation === "create"}
				<label>ファイル名<input class="my-2 w-full rounded border p-1" bind:value={nameDraft} placeholder="chat-echo/main" disabled={pending} /></label>
			{:else}<p>バックアップを残して削除します。未保存の編集がある場合は削除できません。</p>{/if}
			{#if operationError}<p role="alert" class="my-2 text-error-500">{operationError}</p>{/if}
			<div class="mt-2 flex gap-2"><button class="rounded border px-2 py-1" disabled={pending} onclick={() => void confirmOperation()}>{pending ? "処理中…" : operation === "create" ? "作成を実行" : "削除を実行"}</button><button class="rounded border px-2 py-1" disabled={pending} onclick={cancelOperation}>キャンセル</button></div>
		</section>
	{/if}
	{#if flowgraphStore.treeState === "loading"}
		<div class="p-3 text-xs opacity-60">Loading…</div>
	{:else if flowgraphStore.treeState === "error"}
		<div class="p-3 text-xs text-error-500">{flowgraphStore.treeError}</div>
	{:else if flowgraphStore.tree && !flowgraphStore.tree.exists}
		<div class="p-3 text-xs opacity-70">
			<p class="mb-1">
				<code>{flowgraphStore.tree.root_dir}</code> が存在しません。
			</p>
			<p>新規ファイルを作成するとディレクトリごと作成されます。</p>
		</div>
	{:else if tree.length === 0}
		<div class="p-3 text-xs opacity-60">ファイルがありません。</div>
	{:else}
		<ul class="flex-1 overflow-y-auto p-1 text-sm">
			{#each tree as node (node.fullPath)}
				{@render treeItem(node, 0)}
			{/each}
		</ul>
	{/if}
</div>

{#snippet treeItem(node: TreeNode, depth: number)}
	{#if node.kind === "dir"}
		<li style="padding-left: {depth * 0.75}rem">
			<div class="px-1 py-0.5 text-xs font-semibold opacity-70">{node.name}/</div>
			<ul>
				{#each node.children as child (child.fullPath)}
					{@render treeItem(child, depth + 1)}
				{/each}
			</ul>
		</li>
	{:else}
		{@const active = flowgraphStore.currentFq === node.entry.fq}
		{@const error = node.entry.parse_error}
		<li style="padding-left: {depth * 0.75}rem">
			<button
				type="button"
				class="flex w-full items-center justify-between gap-1 rounded px-1 py-0.5 text-left text-xs hover:bg-surface-200-800"
				class:bg-primary-500={active}
				class:text-white={active}
				onclick={() => onSelect(node.entry.fq)}
				title={node.entry.path}
			>
				<span class="truncate">{node.name}</span>
				<span class="flex items-center gap-1">
					{#if error}
						<span class="rounded bg-error-500 px-1 text-[0.65rem] text-white" title={error}>!</span>
					{:else if node.entry.node_count !== null}
						<span class="text-[0.65rem] opacity-60">{node.entry.node_count}</span>
					{/if}
				</span>
			</button>
		</li>
	{/if}
{/snippet}

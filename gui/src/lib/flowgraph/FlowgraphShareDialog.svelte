<script lang="ts">
	/**
	 * Phase δ-7e: Fragment paste / ZIP import のための汎用ダイアログ。
	 *
	 * 4 モード:
	 *   - `paste`      : fragment TOML を貼り付けて指定ファイル/フォルダに paste
	 *   - `import_zip` : ZIP ファイルを選んで dry_run → 本番 import の 2 段階
	 *
	 * 親は `mode` と `open` を bind する。閉じた時点でフォームはリセットされる。
	 */
	import { flowgraphStore } from "../flowgraphStore.svelte";
	import type { FragmentOnConflict, FragmentOnConflictFile, ZipImportPreview } from "../types";

	type Props = {
		open: boolean;
		mode: "paste" | "import_zip";
	};

	let { open = $bindable(), mode = $bindable() }: Props = $props();

	// --- Paste state ---
	let pasteToml = $state("");
	let pasteTargetKind = $state<"file" | "folder">("file");
	let pasteTargetFq = $state("");
	let pasteTargetFolder = $state("");
	let onConflictNode = $state<FragmentOnConflict>("suffix");
	let onConflictFile = $state<FragmentOnConflictFile>("rename");
	let offsetX = $state(0);
	let offsetY = $state(0);

	// --- ZIP state ---
	let zipFile = $state<File | null>(null);
	let zipTargetPrefix = $state("imported");
	let zipOnConflictNode = $state<FragmentOnConflict>("suffix");
	let zipOnConflictFile = $state<FragmentOnConflictFile>("rename");
	let zipPreview = $state<ZipImportPreview | null>(null);
	const companionConflict = $derived(zipPreview?.companions.some(entry => zipPreview?.conflicts.includes(entry.dest_path)) ?? false);
	let zipBusy = $state(false);
	let zipRoot = $state<string | undefined>();
	let requestGeneration = 0;

	function reset() {
		requestGeneration++;
		pasteToml = "";
		pasteTargetKind = "file";
		pasteTargetFq = flowgraphStore.currentFq ?? "";
		pasteTargetFolder = "";
		onConflictNode = "suffix";
		onConflictFile = "rename";
		offsetX = 0;
		offsetY = 0;
		zipFile = null;
		zipRoot = undefined;
		zipTargetPrefix = "imported";
		zipOnConflictNode = "suffix";
		zipOnConflictFile = "rename";
		zipPreview = null;
		zipBusy = false;
	}

	function close() {
		open = false;
		reset();
	}

	function pasteDraftIdentity(): string {
		return JSON.stringify([pasteToml, pasteTargetKind, pasteTargetFq, pasteTargetFolder, onConflictNode, onConflictFile, offsetX, offsetY]);
	}

	async function onPaste() {
		if (!pasteToml.trim()) return;
		const generation = ++requestGeneration;
		const draft = pasteDraftIdentity();
		const target =
			pasteTargetKind === "file"
				? ({ kind: "file", fq: pasteTargetFq } as const)
				: ({ kind: "folder", path: pasteTargetFolder } as const);
		const offset: [number, number] | null = offsetX !== 0 || offsetY !== 0 ? [offsetX, offsetY] : null;
		const resp = await flowgraphStore.fragmentPaste({
			fragment_toml: pasteToml,
			target,
			options: {
				on_conflict_node: onConflictNode,
				on_conflict_file: onConflictFile,
				position_offset: offset,
			},
		});
		if (resp && generation === requestGeneration && draft === pasteDraftIdentity()) close();
	}

	async function onFilePicked(ev: Event) {
		const input = ev.target as HTMLInputElement;
		zipFile = input.files?.[0] ?? null;
		zipPreview = null;
		await refreshZipPreview();
	}

	async function refreshZipPreview() {
		if (!zipFile) return;
		const generation = ++requestGeneration;
		zipPreview = null;
		zipRoot = flowgraphStore.tree?.root_dir;
		zipBusy = true;
		const outcome = await flowgraphStore.importZip(zipFile, {
			dry_run: true,
			expected_root: zipRoot,
			target_prefix: zipTargetPrefix,
			on_conflict_node: zipOnConflictNode,
			on_conflict_file: zipOnConflictFile,
		});
		if (generation !== requestGeneration) return;
		zipBusy = false;
		if (outcome && outcome.kind === "preview") zipPreview = outcome;
	}

	async function onImportApply() {
		if (!zipFile || !zipPreview || !zipRoot || zipBusy || companionConflict) return;
		const generation = ++requestGeneration;
		// The request may save some files even when its completion fails.
		// Keep the input, but require a fresh conflict review before retrying.
		zipPreview = null;
		zipBusy = true;
		const outcome = await flowgraphStore.importZip(zipFile, {
			dry_run: false,
			expected_root: zipRoot,
			target_prefix: zipTargetPrefix,
			on_conflict_node: zipOnConflictNode,
			on_conflict_file: zipOnConflictFile,
		});
		if (generation !== requestGeneration) return;
		zipBusy = false;
		if (outcome && outcome.kind === "report") close();
	}

	$effect(() => {
		if (open && pasteTargetFq === "" && flowgraphStore.currentFq) {
			pasteTargetFq = flowgraphStore.currentFq;
		}
	});
</script>

{#if open}
	<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" role="dialog" aria-modal="true">
		<div class="flex max-h-[90vh] w-[min(640px,95vw)] flex-col gap-3 overflow-y-auto rounded-lg bg-surface-50-950 p-4 shadow-xl">
			<div class="flex items-center justify-between">
				<h2 class="text-lg font-semibold">
					{#if mode === "paste"}構成を貼り付ける{:else}ZIPから構成を取り込む{/if}
				</h2>
				<button type="button" class="rounded px-2 py-1 text-sm hover:bg-surface-200-800" aria-label="閉じる" onclick={close}>✕</button>
			</div>

			{#if mode === "paste"}
				<label class="flex flex-col gap-1 text-xs">
					<span>コピーした構成（TOML）</span>
					<textarea
						class="min-h-[180px] resize-y rounded border border-surface-300-700 bg-surface-100-900 p-2 font-mono text-xs"
						placeholder="クリップボードの fragment TOML を貼り付け…"
						bind:value={pasteToml}
					></textarea>
				</label>

				<div class="flex items-center gap-2 text-xs">
					<label class="flex items-center gap-1">
						<input type="radio" bind:group={pasteTargetKind} value="file" />
						ファイルへ追加
					</label>
					<label class="flex items-center gap-1">
						<input type="radio" bind:group={pasteTargetKind} value="folder" />
						フォルダーへ追加
					</label>
				</div>

				{#if pasteTargetKind === "file"}
					<label class="flex flex-col gap-1 text-xs">
						<span>追加先の構成ファイル（例: <code>project/main</code>）</span>
						<input
							type="text"
							class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5 font-mono text-xs"
							bind:value={pasteTargetFq}
						/>
					</label>
				{:else}
					<label class="flex flex-col gap-1 text-xs">
						<span>追加先フォルダー（空欄で構成フォルダー直下）</span>
						<input
							type="text"
							class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5 font-mono text-xs"
							bind:value={pasteTargetFolder}
						/>
					</label>
				{/if}

				<div class="grid grid-cols-2 gap-2 text-xs">
					<label class="flex flex-col gap-1">
						<span>同じノードIDがある場合</span>
						<select class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5" bind:value={onConflictNode}>
							<option value="suffix">IDを変えて追加</option>
							<option value="skip">既存を残して追加しない</option>
							<option value="overwrite">既存を上書き</option>
						</select>
					</label>
					<label class="flex flex-col gap-1">
						<span>同じファイルがある場合</span>
						<select class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5" bind:value={onConflictFile}>
							<option value="rename">名前を変えて追加</option>
							<option value="skip">既存を残して追加しない</option>
							<option value="overwrite">既存を上書き</option>
							<option value="merge">既存ファイルへ統合</option>
						</select>
					</label>
					<label class="flex flex-col gap-1">
						<span>配置をずらす量（横）</span>
						<input type="number" class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5" bind:value={offsetX} />
					</label>
					<label class="flex flex-col gap-1">
						<span>配置をずらす量（縦）</span>
						<input type="number" class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5" bind:value={offsetY} />
					</label>
				</div>

				<div class="flex justify-end gap-2 pt-2">
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800"
						onclick={close}>閉じる</button
					>
					<button
						type="button"
						class="rounded bg-primary-500 px-3 py-1 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-40"
						disabled={!pasteToml.trim() || flowgraphStore.mutating || (pasteTargetKind === "file" && !pasteTargetFq.trim())}
						onclick={onPaste}
					>
						貼り付けて保存・適用
					</button>
				</div>
			{:else}
				<label class="flex flex-col gap-1 text-xs">
					<span>取り込むZIPファイル</span>
					<input
						type="file"
                        disabled={zipBusy}
						accept=".zip,application/zip"
						class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5 text-xs"
						onchange={onFilePicked}
					/>
				</label>

				<label class="flex flex-col gap-1 text-xs">
					<span>追加先フォルダー（空欄で構成フォルダー直下）</span>
					<input
						type="text"
						class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5 font-mono text-xs"
						bind:value={zipTargetPrefix}
                        disabled={zipBusy}
                        oninput={() => (zipPreview = null)}
					/>
				</label>

				<div class="grid grid-cols-2 gap-2 text-xs">
					<label class="flex flex-col gap-1">
						<span>同じノードIDがある場合</span>
						<select class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5" bind:value={zipOnConflictNode} disabled={zipBusy} onchange={() => (zipPreview = null)}>
							<option value="suffix">IDを変えて追加</option>
							<option value="skip">既存を残して追加しない</option>
							<option value="overwrite">既存を上書き</option>
						</select>
					</label>
					<label class="flex flex-col gap-1">
						<span>同じファイルがある場合</span>
						<select class="rounded border border-surface-300-700 bg-surface-100-900 p-1.5" bind:value={zipOnConflictFile} disabled={zipBusy} onchange={() => (zipPreview = null)}>
							<option value="rename">名前を変えて追加</option>
							<option value="skip">既存を残して追加しない</option>
							<option value="overwrite">既存を上書き</option>
							<option value="merge">既存ファイルへ統合</option>
						</select>
					</label>
				</div>

				{#if zipPreview}
					<div class="rounded border border-surface-300-700 p-2 text-xs">
						<div class="mb-1 font-semibold">追加内容の確認</div>
						<div>追加先: <code class="break-all">{zipPreview.target_prefix || "構成フォルダー直下"}</code></div>
						<div>
							ファイル {zipPreview.file_count} 件 · 同名の項目 {zipPreview.conflicts.length} 件 · 接続先未解決
							{zipPreview.danglings.length}
						</div>
						{#if companionConflict}<p role="alert">同名の素材が保存先にあります。既存素材を残すため、別の取込先フォルダーを指定してください。</p>{/if}
						{#if zipPreview.conflicts.length > 0}
							<details class="mt-1">
								<summary class="cursor-pointer text-warning-700-300">同名の項目</summary>
								<ul class="ml-4 list-disc">
									{#each zipPreview.conflicts as c (c)}
										<li><code class="break-all">{c}</code></li>
									{/each}
								</ul>
							</details>
						{/if}
						{#if zipPreview.entries.length > 0}
							<details class="mt-1">
								<summary class="cursor-pointer">取込元と保存先（{zipPreview.entries.length} 件）</summary>
								<ul class="ml-4 list-disc">
									{#each zipPreview.entries as e (e.zip_path)}
										<li>
											<code class="break-all">{e.zip_path}</code> → <code class="break-all">{e.dest_path}</code>
											<span class="opacity-60">({e.size}B)</span>
										</li>
									{/each}
								</ul>
							</details>
						{/if}
					</div>
				{/if}

				<div class="flex justify-end gap-2 pt-2">
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800"
						onclick={close}>閉じる</button
					>
                    <button type="button" class="rounded border border-surface-300-700 px-3 py-1 text-xs disabled:opacity-40"
                        disabled={!zipFile || zipBusy || flowgraphStore.mutating} onclick={refreshZipPreview}>内容を確認</button>
					<button
						type="button"
						class="rounded bg-primary-500 px-3 py-1 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-40"
						disabled={!zipFile || !zipPreview || zipBusy || companionConflict || flowgraphStore.mutating}
						onclick={onImportApply}
					>
						{zipBusy ? "処理中…" : "取り込んで保存・適用"}
					</button>
				</div>
			{/if}
		</div>
	</div>
{/if}

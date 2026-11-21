<script lang="ts">
	import { profileImportDisplayName, type ProfileImportSource, workspaceConfigCandidates } from "./profileWorkspaceBundle";

	let { source = $bindable<ProfileImportSource | null>(null), disabled = false } = $props<{
		source?: ProfileImportSource | null;
		disabled?: boolean;
	}>();
	let fileInput = $state<HTMLInputElement | null>(null);
	let folderInput = $state<HTMLInputElement | null>(null);
	let workspaceFiles = $state<File[]>([]);
	let configCandidates = $state<string[]>([]);
	let selectedConfig = $state("");
	let localError = $state<string | null>(null);

	function selectFile(file: File | null): void {
		workspaceFiles = [];
		configCandidates = [];
		selectedConfig = "";
		localError = null;
		source = file ? { kind: "file", file } : null;
	}

	function selectFolder(files: File[]): void {
		localError = null;
		workspaceFiles = files;
		try {
			configCandidates = workspaceConfigCandidates(files);
			selectedConfig = configCandidates[0] ?? "";
			source = selectedConfig ? { kind: "workspace", files, configPath: selectedConfig } : null;
			if (!selectedConfig) localError = "フォルダー直下に conf.toml または conf.*.toml がありません";
		} catch (error) {
			configCandidates = [];
			selectedConfig = "";
			source = null;
			localError = error instanceof Error ? error.message : String(error);
		}
	}

	function changeConfig(path: string): void {
		selectedConfig = path;
		source = path ? { kind: "workspace", files: workspaceFiles, configPath: path } : null;
	}
</script>

<input
	bind:this={fileInput}
	type="file"
	accept=".toml,.zip,application/zip,text/plain"
	aria-label="プロファイルファイル"
	class="sr-only"
	{disabled}
	onchange={(event) => selectFile(event.currentTarget.files?.[0] ?? null)}
/>
<input
	bind:this={folderInput}
	type="file"
	multiple
	webkitdirectory={true}
	aria-label="ワークスペースフォルダー"
	class="sr-only"
	{disabled}
	onchange={(event) => selectFolder(Array.from(event.currentTarget.files ?? []))}
/>

<div class="mt-2 flex min-w-0 flex-wrap items-center gap-2">
	<button
		type="button"
		class="rounded border border-surface-300-700 px-3 py-1.5 text-xs hover:bg-surface-100-900 disabled:opacity-50"
		{disabled}
		onclick={() => fileInput?.click()}
	>
		TOML / ZIP
	</button>
	<button
		type="button"
		class="rounded border border-surface-300-700 px-3 py-1.5 text-xs hover:bg-surface-100-900 disabled:opacity-50"
		{disabled}
		onclick={() => folderInput?.click()}
	>
		フォルダー
	</button>
	<span class="min-w-0 flex-1 truncate text-xs opacity-60" title={profileImportDisplayName(source)}>
		{profileImportDisplayName(source)}
	</span>
</div>

{#if configCandidates.length > 1}
	<label class="mt-2 block text-xs">
		<span class="opacity-60">使用する設定</span>
		<select
			class="mt-1 w-full rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1.5 text-xs"
			value={selectedConfig}
			{disabled}
			onchange={(event) => changeConfig(event.currentTarget.value)}
		>
			{#each configCandidates as candidate (candidate)}
				<option value={candidate}>{candidate}</option>
			{/each}
		</select>
	</label>
{/if}

{#if source?.kind === "workspace"}
	<p class="mt-2 text-xs opacity-60">設定から参照されるFlowgraphと関連ファイルを取り込む。</p>
{:else}
	<p class="mt-2 text-xs opacity-60">関連ファイルがある既存設定はフォルダーを選択。</p>
{/if}
{#if localError}<p class="mt-2 text-xs text-error-500">{localError}</p>{/if}

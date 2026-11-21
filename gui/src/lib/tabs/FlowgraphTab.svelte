<script lang="ts">
	/**
	 * Phase δ-6e: Flowgraph タブのトップレベル。
	 *
	 * レイアウト:
	 *   ├── 上部: ツールバー（reload / save / open-external）
	 *   ├── 中央:
	 *   │    ├── 左サイドバー（ファイルツリー）
	 *   │    ├── キャンバス（Svelte Flow）
	 *   │    └── 右サイドバー（上: パレット / 下: プロパティエディタ）
	 *   └── 下部: 診断パネル
	 *
	 * 責務:
	 *   - マウント時に `flowgraphStore.refreshAll()` + WS 購読を開始
	 *   - アンマウント時に WS 購読を解除
	 *
	 * 子コンポーネントとの通信は `flowgraphStore` 経由。props は最小に保つ。
	 */
	import { onMount } from "svelte";
	import { flowgraphStore, summarizeDiagnostics } from "../flowgraphStore.svelte";
	import FeatureLibrary from "../flowgraph/FeatureLibrary.svelte";
	import FlowgraphRecoveryPreview from "../flowgraph/FlowgraphRecoveryPreview.svelte";
	import FlowgraphExportPreview from "../flowgraph/FlowgraphExportPreview.svelte";
	import type { ZipExportRequest } from "../types";
	let exportPreview = $state<{ request: ZipExportRequest; filename: string; root: string } | null>(null);
	import FeatureAnchor from "../FeatureAnchor.svelte";
	import FlowgraphTree from "../flowgraph/FlowgraphTree.svelte";
	import FlowgraphCanvas from "../flowgraph/FlowgraphCanvas.svelte";
	import FlowgraphCompositionOverview from "../flowgraph/FlowgraphCompositionOverview.svelte";
	import FlowgraphPalette from "../flowgraph/FlowgraphPalette.svelte";
	import FlowgraphPropertyEditor from "../flowgraph/FlowgraphPropertyEditor.svelte";
	import FeatureSetupGuide from "../flowgraph/FeatureSetupGuide.svelte";
	import FlowgraphDiagnostics from "../flowgraph/FlowgraphDiagnostics.svelte";
	import FlowgraphShareDialog from "../flowgraph/FlowgraphShareDialog.svelte";
	import { flowgraphPathForDisplay } from "../flowgraph/flowgraphPathDisplay";
	import { tabNavStore, type RepairFocus } from "../tabs.svelte";
	import { toastStore } from "../toasts.svelte";

	let dialogOpen = $state(false);
	let dialogMode = $state<"paste" | "import_zip">("paste");
	let commandPaletteOpen = $state(false);
	let commandQuery = $state("");
	let filePaneWidth = $state(260);
	let inspectorPaneWidth = $state(320);
	let problemsHeight = $state(176);
	let problemsOpen = $state(!(import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" && typeof window !== "undefined" && new URL(window.location.href).searchParams.get("diagnostics") === "summary"));
	let compositionOverview = $state(false);
	const focusedWorkspace = import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1" || (typeof window !== "undefined" && new URL(window.location.href).searchParams.get("workspace") === "focused");
	// Comparison links must open the surface being compared, not the shared Studio.
	const purposeComparison = import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" && typeof window !== "undefined" && new URL(window.location.href).searchParams.get("purposes") === "preview";
	let workspaceView = $state<"studio" | "library">(purposeComparison ? "library" : "studio");
	const recoveryPreview = import.meta.env.VITE_UN_DESIGN_PREVIEW === "1";
	let recoveryPlacement = $state<"side" | "drawer">("drawer");
	let expandedStudio = $state(false);
	const panelLayoutPreview = import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1" || (typeof window !== "undefined" && new URL(window.location.href).searchParams.get("studio") === "panels");
	let compactHeader = $state(panelLayoutPreview && new URL(window.location.href).searchParams.get("header") === "compact");
	let adaptivePanes = $state(import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1" || (panelLayoutPreview && new URL(window.location.href).searchParams.get("panes") === "adaptive"));
	let workspaceWidth = $state(0);
	let compactPane = $state<"canvas" | "files" | "inspector">("canvas");
	const compactPanes = $derived(panelLayoutPreview && adaptivePanes && workspaceWidth > 0 && workspaceWidth < 1100);
	let filesVisible = $state(true);
	let inspectorVisible = $state(true);
	const viewportLayoutPreview = panelLayoutPreview && new URL(window.location.href).searchParams.get("height") === "viewport";
	let editorTop = $state(0);
	let editorBottom = $state(0);
	onMount(() => {
		if (!viewportLayoutPreview) return;
		const header = document.querySelector(".vac-app-header");
		const footer = document.querySelector(".vac-status-bar");
		const measure = () => {
			editorTop = header?.getBoundingClientRect().bottom ?? 0;
			editorBottom = Math.max(0, window.innerHeight - (footer?.getBoundingClientRect().top ?? window.innerHeight));
		};
		const observer = new ResizeObserver(measure);
		for (const element of [header, footer, document.documentElement]) if (element) observer.observe(element);
		window.addEventListener("resize", measure);
		measure();
		return () => { observer.disconnect(); window.removeEventListener("resize", measure); };
	});
	let fileOperationsOpen = $state(false);
	let viewSettingsOpen = $state(false);
	$effect(() => {
		if (tabNavStore.featureFocus?.id === "flowgraph-state") {
			workspaceView = "studio";
			problemsOpen = true;
		}
		if (!focusedWorkspace) return;
		const id = tabNavStore.featureFocus?.id;
		if (id === "feature-library" || id === "feature-installed" || id === "feature-inventory") workspaceView = "library";
		else if (id === "flowgraph-studio") workspaceView = "studio";
	});
	let inspectorTabs = $state(import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1" || (typeof window !== "undefined" && new URL(window.location.href).searchParams.get("inspector") === "tabs"));
	let inspectorView = $state<"nodes" | "properties">("nodes");
	$effect(() => {
		if (import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1") return;
		const change = (event: Event) => { inspectorTabs = (event as CustomEvent).detail === "tabs"; };
		window.addEventListener("un-preview-inspector", change);
		return () => window.removeEventListener("un-preview-inspector", change);
	});
	$effect(() => {
		if (flowgraphStore.selectedNodeId) inspectorView = "properties";
	});

	type StudioCommand = {
		id: string;
		label: string;
		description: string;
		disabled: boolean;
		run: () => void | Promise<void>;
	};

	// The store retains drafts across tab unmounts in normal builds as well.
	const retainNavigationDraft = true;
	function canLeaveWorkspace(): boolean {
		// The store survives tab navigation; pending writes must settle first.
		return !flowgraphStore.mutating;
	}

	onMount(() => {
		const unguard = tabNavStore.guardLeaving(canLeaveWorkspace);
		void prepareFlowgraph();
		flowgraphStore.attachWsSubscriber();
		// γ-4a: Ctrl+S / Cmd+S で現在ファイルを保存。フォーカスが input 系でも有効にするため window に付ける。
		const onKeyDown = (ev: KeyboardEvent) => {
			const t = ev.target as HTMLElement | null;
			if (t?.closest('input, textarea, select, [contenteditable="true"]')) return;

			const isSave = (ev.ctrlKey || ev.metaKey) && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "s";
			if (isSave) {
				if (!flowgraphStore.currentFq || flowgraphStore.mutating) return;
				ev.preventDefault();
				void flowgraphStore.saveCurrent();
				return;
			}

			const isUndo = (ev.ctrlKey || ev.metaKey) && !ev.altKey && ev.key.toLowerCase() === "z";
			if (isUndo) {
				ev.preventDefault();
				if (ev.shiftKey) onRedo();
				else onUndo();
				return;
			}

			const isRedo = (ev.ctrlKey || ev.metaKey) && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "y";
			if (isRedo) {
				ev.preventDefault();
				onRedo();
				return;
			}

			const isCommandPalette = (ev.ctrlKey || ev.metaKey) && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "k";
			if (isCommandPalette) {
				ev.preventDefault();
				commandPaletteOpen = true;
				return;
			}

			const isDup = (ev.ctrlKey || ev.metaKey) && !ev.shiftKey && !ev.altKey && ev.key.toLowerCase() === "d";
			if (isDup) {
				if (!flowgraphStore.currentFq || !flowgraphStore.selectedNodeId) return;
				ev.preventDefault();
				const ok = flowgraphStore.duplicateSelectedNode();
				if (ok) {
					toastStore.success("複製しました", flowgraphStore.selectedNodeId ?? "");
				}
				return;
			}
		};
		window.addEventListener("keydown", onKeyDown);

		return () => {
			unguard();
			flowgraphStore.detachWsSubscriber();
			window.removeEventListener("keydown", onKeyDown);
		};
	});

	async function prepareFlowgraph() {
		await flowgraphStore.refreshAll();
		const focus = tabNavStore.repairFocus;
		if (focus) await openRepairTarget(focus);
	}

	function repairTarget(focus: RepairFocus): { fq: string; node?: string } | null {
		if (focus.node?.includes("::")) {
			const separator = focus.node.lastIndexOf("::");
			return { fq: focus.node.slice(0, separator), node: focus.node.slice(separator + 2) };
		}
		if (focus.file) {
			const normalized = focus.file.replace(/\\/g, "/");
			const match = (flowgraphStore.tree?.files ?? []).find((file) => normalized.endsWith(file.path));
			const fq = match?.fq ?? normalized.replace(/\.flowgraph\.toml$/, "").replace(/^.*\//, "");
			if (fq) return { fq, node: focus.node ?? undefined };
		}
		return null;
	}

	async function openRepairTarget(focus: RepairFocus) {
		const target = repairTarget(focus);
		if (!target) return;
		const sameDraft = retainNavigationDraft
			&& flowgraphStore.currentFq === target.fq
			&& flowgraphStore.currentState === "ok"
			&& !!flowgraphStore.currentFile?.root_dir
			&& flowgraphStore.currentFile.root_dir === flowgraphStore.tree?.root_dir;
		if (!sameDraft && !(await openStudioFile(target.fq))) return;
		if (flowgraphStore.currentState !== "ok" || flowgraphStore.currentFq !== target.fq) return;
		if (target.node) {
			flowgraphStore.selectedNodeId = target.node;
			flowgraphStore.selectedNodeIds = [target.node];
		}
	}

	function returnHome() {
		tabNavStore.setActive("now");
	}

	function repairInstruction(focus: RepairFocus): string {
		if (focus.resolution) return focus.resolution;
		if (focus.code === "voice_worker_start_failed") {
			return "右側の設定で音声入力と認識エンジンを確認する。保存すると、このVACのFlowgraph全体へ適用される。";
		}
		if (repairTarget(focus)) {
			return "右側の設定で対象ノードを確認する。保存すると、このVACのFlowgraph全体へ適用される。";
		}
		return "下の診断内容を確認し、該当するFlowgraphの設定を修正する。";
	}

	const isDirty = $derived(flowgraphStore.isDirty);
	const saveLabel = $derived(flowgraphStore.mutating ? "保存中…" : "保存");
	const saveStatusLabel = $derived(isDirty ? "未保存の変更" : "保存済み");
	const flowgraphRootDisplay = $derived(flowgraphPathForDisplay(flowgraphStore.tree?.root_dir));

	const summary = $derived(summarizeDiagnostics(flowgraphStore.diagnostics?.diagnostics));
	const hasErrors = $derived(summary.error > 0);
	const hasWarnings = $derived(summary.warning > 0);
	const fileCount = $derived(flowgraphStore.tree?.files.length ?? 0);
	const instanceCount = $derived(flowgraphStore.draftInstances?.length ?? 0);
	const nodeCount = $derived((flowgraphStore.draftNodes?.length ?? 0) + instanceCount);
	const edgeCount = $derived(flowgraphStore.draftEdges?.length ?? 0);
	const canReloadPreservingState = $derived(Boolean(flowgraphStore.diagnostics?.state_snapshot_file_path) && !flowgraphStore.mutating);
	const workspaceGridStyle = $derived(`--file-pane-width: ${filePaneWidth}px; --inspector-pane-width: ${inspectorPaneWidth}px;` + (compactPanes ? "grid-template-columns:minmax(0,1fr);" : panelLayoutPreview ? `grid-template-columns:${filesVisible ? "minmax(150px,min(var(--file-pane-width),30%)) " : ""}minmax(0,1fr)${inspectorVisible ? " minmax(240px,min(var(--inspector-pane-width),36%))" : ""};` : ""));
	const problemsStyle = $derived(problemsOpen ? `height: min(${problemsHeight}px, 32vh);` : "height: 2.25rem;");
	const studioCommands: StudioCommand[] = $derived([
		{
			id: "undo",
			label: "元に戻す",
			description: "直前のキャンバス編集を取り消します。",
			disabled: !flowgraphStore.canUndo(),
			run: onUndo,
		},
		{
			id: "redo",
			label: "やり直す",
			description: "取り消したキャンバス編集をやり直します。",
			disabled: !flowgraphStore.canRedo(),
			run: onRedo,
		},
		{
			id: "duplicate",
			label: "選択ノードを複製",
			description: "選択中のノードを複製します。",
			disabled: !flowgraphStore.currentFq || flowgraphStore.selectedNodeIds.length === 0,
			run: onDuplicateSelection,
		},
		{
			id: "align_horizontal",
			label: "横方向に整列",
			description: "選択ノードの縦位置を揃えます。",
			disabled: flowgraphStore.selectedNodeIds.length < 2,
			run: () => flowgraphStore.alignSelectedNodes("y"),
		},
		{
			id: "align_vertical",
			label: "縦方向に整列",
			description: "選択ノードの横位置を揃えます。",
			disabled: flowgraphStore.selectedNodeIds.length < 2,
			run: () => flowgraphStore.alignSelectedNodes("x"),
		},
		{
			id: "distribute_horizontal",
			label: "横方向へ均等配置",
			description: "選択ノードを横方向へ等間隔に配置します。",
			disabled: flowgraphStore.selectedNodeIds.length < 3,
			run: () => flowgraphStore.distributeSelectedNodes("x"),
		},
		{
			id: "distribute_vertical",
			label: "縦方向へ均等配置",
			description: "選択ノードを縦方向へ等間隔に配置します。",
			disabled: flowgraphStore.selectedNodeIds.length < 3,
			run: () => flowgraphStore.distributeSelectedNodes("y"),
		},
		{
			id: "auto_layout",
			label: "全体を自動配置",
			description: "接続順に従い、ノードが重ならないよう左から右へ配置します。",
			disabled: (flowgraphStore.draftNodes?.length ?? 0) + (flowgraphStore.draftInstances?.length ?? 0) < 2,
			run: onAutoLayout,
		},
		{
			id: "group_layout",
			label: "グループを作成",
			description: "選択ノードを表示グループにまとめて配置します。",
			disabled: flowgraphStore.selectedNodeIds.length < 2,
			run: () => flowgraphStore.groupSelectedNodes(),
		},
		{
			id: "select_group_members",
			label: "グループ内をすべて選択",
			description: "現在の表示グループに属するノードをすべて選択します。",
			disabled: !flowgraphStore.selectedGroupId,
			run: () => {
				if (flowgraphStore.selectedGroupId) flowgraphStore.selectGroupNodes(flowgraphStore.selectedGroupId);
			},
		},
		{
			id: "remove_group",
			label: "選択グループを解除",
			description: "ノードを削除せず、表示グループだけを解除します。",
			disabled: !flowgraphStore.selectedGroupId,
			run: () => {
				if (flowgraphStore.selectedGroupId) flowgraphStore.removeGroup(flowgraphStore.selectedGroupId);
			},
		},
		{
			id: "reload",
			label: "ディスクから再読み込み",
			description: "編集フォルダーからファイルと診断結果を読み直します。",
			disabled: flowgraphStore.mutating,
			run: onReload,
		},
		{
			id: "reload_preserve_state",
			label: "状態を保持して再読み込み",
			description: "現在の状態を一時保存し、再読み込み後に復元します。",
			disabled: !canReloadPreservingState,
			run: onReloadPreservingState,
		},
		{
			id: "save",
			label: "現在のファイルを保存",
			description: "編集中のFlowgraphファイルをディスクへ保存します。",
			disabled: !flowgraphStore.currentFq || flowgraphStore.mutating,
			run: onSave,
		},
		{
			id: "open_external",
			label: "外部エディタで開く",
			description: "選択中のFlowgraphファイルを外部エディタで開きます。",
			disabled: !flowgraphStore.currentFq,
			run: onOpenExternal,
		},
		{
			id: "copy_fragment",
			label: "選択範囲をコピー",
			description: "選択ノードまたはグループをコピーします。未選択時は現在のファイルが対象です。",
			disabled: !flowgraphStore.currentFq,
			run: onCopy,
		},
		{
			id: "paste_fragment",
			label: "貼り付け",
			description: "Flowgraph断片の貼り付け画面を開きます。",
			disabled: false,
			run: onPaste,
		},
		{
			id: "export_zip",
			label: "ZIPへ書き出す",
			description: "選択範囲または現在のファイルをZIP断片として書き出します。",
			disabled: !flowgraphStore.currentFq,
			run: onExportZip,
		},
		{
			id: "import_zip",
			label: "ZIPから読み込む",
			description: "ZIPの内容を確認して読み込む画面を開きます。",
			disabled: false,
			run: onImportZip,
		},
		...nodeInsertCommands(),
	]);

	const filteredStudioCommands = $derived.by(() => {
		const q = commandQuery.trim().toLowerCase();
		if (!q) return studioCommands;
		return studioCommands.filter((command) => `${command.label} ${command.description}`.toLowerCase().includes(q));
	});

	function nodeInsertCommands(): StudioCommand[] {
		const specs = flowgraphStore.catalog?.specs ?? [];
		const catalogCommands = specs.slice(0, 250).map((spec) => ({
			id: `insert:${spec.feature}`,
			label: `Insert ${spec.title}`,
			description: `${spec.category} · ${spec.feature}`,
			disabled: !flowgraphStore.currentFq,
			run: () => {
				flowgraphStore.addCatalogNodeAt(spec, null);
			},
		}));
		const endpointCommands = flowgraphStore.unmfzEndpointCandidates().flatMap((candidate) => {
			const out: StudioCommand[] = [];
			if (candidate.direction === "input" || candidate.direction === "both") {
				out.push({
					id: `unmfz:publish:${candidate.id}`,
					label: `Insert UNMF/Z publish: ${candidate.label}`,
					description: `${candidate.baseKeyExpr} · ${candidate.topicMode}`,
					disabled: !flowgraphStore.currentFq,
					run: () => {
						flowgraphStore.addUnmfzEndpointGraph(candidate, "publish", null);
					},
				});
			}
			if (candidate.direction === "output" || candidate.direction === "both") {
				out.push({
					id: `unmfz:subscribe:${candidate.id}`,
					label: `Insert UNMF/Z ingress: ${candidate.label}`,
					description: `${candidate.baseKeyExpr} · ${candidate.topicMode}`,
					disabled: !flowgraphStore.currentFq,
					run: () => {
						flowgraphStore.addUnmfzEndpointGraph(candidate, "subscribe", null);
					},
				});
			}
			return out;
		});
		return [...endpointCommands, ...catalogCommands];
	}

	function onUndo() {
		if (flowgraphStore.undo()) toastStore.info("Undo", flowgraphStore.undoStack.at(-1)?.label ?? "");
	}

	function onRedo() {
		if (flowgraphStore.redo()) toastStore.info("Redo", flowgraphStore.redoStack.at(-1)?.label ?? "");
	}

	function onDuplicateSelection() {
		const ok = flowgraphStore.duplicateSelectedNode();
		if (ok) toastStore.success("Duplicated", `${flowgraphStore.selectedNodeIds.length} node(s)`);
	}

	function onAutoLayout() {
		if (flowgraphStore.autoLayoutCurrentGraph()) {
			toastStore.success("Auto layout", "接続順とノード寸法に基づいて再配置しました。保存するまで元に戻せます。");
		}
	}

	async function onReload() {
		await flowgraphStore.reloadFromDisk();
	}

	async function onReloadPreservingState() {
		await flowgraphStore.reloadPreservingState();
	}

	async function onSave() {
		await flowgraphStore.saveCurrent();
	}

	async function onOpenExternal() {
		if (flowgraphStore.currentFq) await flowgraphStore.openExternal(flowgraphStore.currentFq);
	}

	async function onCopy() {
		await flowgraphStore.fragmentCopySelectedNodesOrCurrentFile();
	}

	function onPaste() {
		dialogMode = "paste";
		dialogOpen = true;
	}

	async function onExportZip() {
		const exportingFq = flowgraphStore.currentFq;
		if (!exportingFq) return;
		const exportingRoot = flowgraphStore.currentFile?.root_dir;
		if (!exportingRoot) {
			toastStore.warn("書出し対象を再確認してください", "開いているファイルの保存先を確認できません。対象を開き直してください。");
			return;
		}
		const req = flowgraphStore.fragmentCopyRequestForSelection(true);
		if (!req) return;
		const scopeSuffix = req.scope === "nodes" ? "_selection" : "";
		const safeName = `${exportingFq}${scopeSuffix}`.replace(/[\\/]/g, "_");
		if (import.meta.env.VITE_UN_DESIGN_PREVIEW === "1") {
			exportPreview = { request: structuredClone(req), filename: `${safeName}.zip`, root: exportingRoot };
			return;
		}
		const blob = await flowgraphStore.exportZip({ ...req, expected_root: exportingRoot });
		if (!blob) return;
		const url = URL.createObjectURL(blob);
		const a = document.createElement("a");
		a.href = url;
		a.download = `${safeName}.zip`;
		document.body.appendChild(a);
		a.click();
		a.remove();
		URL.revokeObjectURL(url);
	}

	function onImportZip() {
		dialogMode = "import_zip";
		dialogOpen = true;
	}

	async function runStudioCommand(command: StudioCommand) {
		if (command.disabled) return;
		commandPaletteOpen = false;
		await command.run();
	}

	async function openStudioFile(fq: string): Promise<boolean> {
		return await flowgraphStore.openFile(fq);
	}
</script>

{#snippet repairGuide()}
	{#if tabNavStore.repairFocus}
		{@const focus = tabNavStore.repairFocus}
		<section class="border-l-2 border-error-500 bg-error-500/10 px-4 py-3" aria-label="修正する問題">
			<div class="flex flex-wrap items-start justify-between gap-3">
				<div class="min-w-0">
					<div class="text-xs font-semibold text-error-500">{focus.severity === "error" ? "エラーの修正" : "警告の確認"}</div>
					<p class="mt-1 text-sm font-semibold">{focus.message}</p>
					{#if focus.impact}<p class="mt-1 text-xs opacity-70">影響: {focus.impact}</p>{/if}
					<p class="mt-2 text-xs"><span class="font-semibold">修正手順:</span> {repairInstruction(focus)}</p>
					{#if focus.node}<p class="mt-1 truncate font-mono text-xs opacity-60" title={focus.node}>対象: {focus.node}</p>{/if}
				</div>
				<div class="flex shrink-0 items-center gap-2">
					{#if repairTarget(focus)}
						<button
							type="button"
							class="rounded bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white hover:bg-primary-600"
							onclick={() => void openRepairTarget(focus)}
						>
							対象を開く
						</button>
					{/if}
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1.5 text-xs hover:bg-surface-100-900"
						onclick={returnHome}
					>
						ホームへ戻る
					</button>
					<button
						type="button"
						class="h-7 w-7 text-lg opacity-60 hover:opacity-100"
						title="案内を閉じる"
						aria-label="案内を閉じる"
						onclick={() => tabNavStore.clearRepairFocus()}>×</button
					>
				</div>
			</div>
		</section>
	{/if}
{/snippet}

{#snippet studioWorkspace()}
	<div class="studio-root flex h-[calc(100dvh-9rem)] min-h-[28rem] flex-col gap-3" class:studio-standard={import.meta.env.VITE_UN_DESIGN_PREVIEW !== "1"} class:studio-expanded={import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" && expandedStudio && (!focusedWorkspace || workspaceView === "studio")} class:studio-viewport={viewportLayoutPreview && expandedStudio && (!focusedWorkspace || workspaceView === "studio")} style={`--editor-top:${editorTop}px;--editor-bottom:${editorBottom}px;`} data-testid="flowgraph-studio-root">
		{@render repairGuide()}
		<svelte:element this={compactHeader ? "details" : "div"} class="studio-introduction">
			{#if compactHeader}<summary class="cursor-pointer text-xs font-semibold">編集の案内・構成情報</summary>{/if}
		<div class="flex flex-wrap items-start justify-between gap-3">
			<div class="min-w-0">
				<h2 class="text-xl font-semibold">Flowgraph Studio</h2>
				<p class="text-sm opacity-65">配信やAIの処理を、ノードの接続として編集・確認します。</p>
			</div>
			<div class="flex items-start gap-2">

				<div class="vac-panel grid grid-cols-3 overflow-hidden text-center text-xs">
					<div class="border-r border-surface-200-800 px-3 py-2">
						<div class="font-semibold">{fileCount}</div>
						<div class="opacity-60">ファイル</div>
					</div>
					<div class="border-r border-surface-200-800 px-3 py-2">
						<div class="font-semibold">{nodeCount}</div>
						<div class="opacity-60">{instanceCount ? "ノード・部品" : "ノード"}</div>
					</div>
					<div class="px-3 py-2">
						<div class="font-semibold">{edgeCount}</div>
						<div class="opacity-60">接続</div>
					</div>
				</div>
			</div>
		</div>

			{#if compactHeader}<p class="text-xs opacity-65">保存すると接続中のVACのFlowgraph全体を再読み込みします。一時的な処理状態は初期化される場合があります。プロファイル本体の保存とは適用方法が異なります。</p>{/if}
		</svelte:element>

		<!-- Toolbar: context, daily actions, and infrequent operations are intentionally separated. -->
		<section class="vac-panel-muted shrink-0 overflow-hidden" data-testid="flowgraph-studio-toolbar" aria-label="Flowgraphの操作">
			{#if import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" && (expandedStudio || compactHeader)}
				<div class="flex min-w-0 flex-wrap items-baseline gap-x-3 gap-y-1 border-b border-surface-200-800 px-3 py-2" aria-label="編集中のFlowgraph">
					<span class="shrink-0 text-xs font-semibold opacity-70">編集中・保存対象</span>
					<code class="min-w-0 break-all text-sm" data-testid="expanded-graph-target">{flowgraphStore.currentFq ?? "ファイル未選択"}</code>
				</div>
			{/if}
			<div class="flex min-w-0 items-center gap-2 border-b border-surface-200-800 px-3 py-2">
				<span class="shrink-0 text-xs font-semibold opacity-70">編集フォルダー</span>
				<code class="min-w-0 flex-1 truncate text-xs opacity-70" title={flowgraphRootDisplay} data-testid="flowgraph-root-path"
					>{flowgraphRootDisplay}</code
				>
				{#if flowgraphStore.tree && !flowgraphStore.tree.exists}
					<span class="shrink-0 rounded bg-warning-500/30 px-2 py-0.5 text-xs">見つかりません</span>
				{/if}
			</div>

			<div class="flex flex-wrap items-center gap-2 px-3 py-2">
                {#if import.meta.env.VITE_UN_DESIGN_PREVIEW === "1"}
                    <button type="button" class="hidden rounded border border-surface-300-700 px-3 py-1 text-xs lg:inline-block" aria-pressed={expandedStudio} title="対象一覧と機能目次を一時的に閉じます。操作対象は画面上部、保存先はこの操作欄に残ります。" onclick={() => (expandedStudio = !expandedStudio)}>{expandedStudio ? "対象一覧・機能目次に戻す" : "編集領域を広げる"}</button>
                {/if}
				{#if focusedWorkspace}
					<button
						type="button"
						class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800"
						aria-pressed={fileOperationsOpen && viewSettingsOpen}
						onclick={() => {
							const expand = !(fileOperationsOpen && viewSettingsOpen);
							fileOperationsOpen = expand;
							viewSettingsOpen = expand;
						}}
						>ファイル操作・表示調整を展開</button
					>
				{/if}
				<button
					type="button"
					class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800"
					class:bg-primary-500={compositionOverview}
					aria-pressed={compositionOverview}
					onclick={() => (compositionOverview = !compositionOverview)}
				>
					構成全体
				</button>
				<button
					type="button"
					class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800"
					title="コマンドを検索（Ctrl+K）"
					onclick={() => (commandPaletteOpen = true)}
				>
					コマンド
				</button>
				<button
					type="button"
					class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800 disabled:opacity-40"
					disabled={(flowgraphStore.draftNodes?.length ?? 0) + (flowgraphStore.draftInstances?.length ?? 0) < 2}
					title="接続順とノード寸法に基づいて全体を再配置"
					onclick={onAutoLayout}
				>
					自動配置
				</button>
				{#if flowgraphStore.diagnosticsState === "ok"}
				<span
					class="rounded px-2 py-0.5 text-xs"
					class:bg-error-500={hasErrors}
					class:text-white={hasErrors}
					class:bg-warning-500={!hasErrors && hasWarnings}
					class:bg-success-500={!hasErrors && !hasWarnings}
					title={`情報 ${summary.info}件`}
				>
					エラー {summary.error} / 警告 {summary.warning}
				</span>
				{:else}
					<span class="rounded border border-surface-300-700 px-2 py-0.5 text-xs" title={flowgraphStore.diagnosticsError ?? undefined}>
						{flowgraphStore.diagnosticsState === "loading" ? "診断を取得中" : flowgraphStore.diagnosticsState === "error" ? "診断を取得できません" : "診断は未取得"}
						{flowgraphStore.diagnostics ? " · 前回の結果を表示" : ""}
					</span>
				{/if}
				<div class="min-w-2 flex-1"></div>
				<span class="shrink-0 text-xs" class:text-warning-500={isDirty} class:opacity-60={!isDirty}>{saveStatusLabel}</span>
				<button
					type="button"
					class="rounded px-3 py-1 text-xs font-semibold text-white disabled:opacity-40"
					class:bg-primary-500={!isDirty}
					class:hover:bg-primary-600={!isDirty}
					class:bg-warning-500={isDirty}
					class:hover:bg-warning-600={isDirty}
					disabled={!flowgraphStore.currentFq || flowgraphStore.mutating}
					title="現在のファイルを保存（Ctrl+S）"
					onclick={onSave}
				>
					{saveLabel}
				</button>
			</div>

			<div class="grid border-t border-surface-200-800 lg:grid-cols-2">
				<details
					bind:open={fileOperationsOpen}
					class="border-b border-surface-200-800 px-3 py-2 lg:border-r lg:border-b-0"
					data-testid="flowgraph-file-operations"
				>
					<summary class="cursor-pointer select-none text-xs font-semibold"
						>ファイル操作 <span class="ml-1 font-normal opacity-55">再読み込み・共有</span></summary
					>
					<div class="mt-2 flex flex-wrap items-center gap-2">
						<button
							type="button"
							class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800"
							title="保存済みのFlowgraph全体を稼働中のVACへ再適用します。未保存の編集は保持します。"
							onclick={onReload}>再読み込み</button
						>
						<button
							type="button"
							class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800 disabled:opacity-40"
							title={flowgraphStore.diagnostics?.state_snapshot_file_path
								? "実行中の状態を保存してから、保存済みのFlowgraph全体を再読み込みします。未保存の編集は保持します。"
								: "状態の保存先情報がまだありません"}
							disabled={!canReloadPreservingState}
							onclick={onReloadPreservingState}>状態を保持して再読み込み</button
						>
						<button
							type="button"
							class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800 disabled:opacity-40"
							disabled={!flowgraphStore.currentFq}
							onclick={onOpenExternal}>外部で開く</button
						>
						<button
							type="button"
							class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800 disabled:opacity-40"
							disabled={!flowgraphStore.currentFq}
							onclick={onCopy}>コピー（{flowgraphStore.fragmentSelectionSummary()}）</button
						>
						<button
							type="button"
							class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800"
							onclick={onPaste}>貼り付け…</button
						>
						<button
							type="button"
							class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800 disabled:opacity-40"
							disabled={!flowgraphStore.currentFq}
							onclick={onExportZip}>ZIP書き出し（{flowgraphStore.fragmentSelectionSummary()}）</button
						>
						<button
							type="button"
							class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800"
							onclick={onImportZip}>ZIP読み込み…</button
						>
					</div>
				</details>
				<details bind:open={viewSettingsOpen} class="px-3 py-2" data-testid="flowgraph-view-settings">
					<summary class="cursor-pointer select-none text-xs font-semibold"
						>表示調整 <span class="ml-1 font-normal opacity-55">各領域の幅と高さ</span></summary
					>
					<div class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-2">
						<label class="flex items-center gap-2 text-xs"
							><span class="w-20">ファイル欄</span><input
								class="w-28"
								type="range"
								min="200"
								max="420"
								step="10"
								bind:value={filePaneWidth}
							/></label
						>
						<label class="flex items-center gap-2 text-xs"
							><span class="w-20">右側パネル</span><input
								class="w-28"
								type="range"
								min="260"
								max="520"
								step="10"
								bind:value={inspectorPaneWidth}
							/></label
						>
						<label class="flex items-center gap-2 text-xs"
							><span class="w-20">問題パネル</span><input
								class="w-28"
								type="range"
								min="120"
								max="360"
								step="8"
								bind:value={problemsHeight}
								disabled={!problemsOpen}
							/></label
						>
					</div>
				</details>
			</div>
		</section>

		<FlowgraphShareDialog bind:open={dialogOpen} bind:mode={dialogMode} />
		{#if exportPreview}<FlowgraphExportPreview {...exportPreview} onclose={() => { exportPreview = null; }} />{/if}

		{#if commandPaletteOpen}
			<div
				class="fixed inset-0 z-50 flex items-start justify-center bg-black/35 px-4 py-20"
				role="presentation"
				onclick={() => (commandPaletteOpen = false)}
			>
				<div
					class="vac-panel w-full max-w-xl overflow-hidden shadow-xl"
					role="dialog"
					aria-modal="true"
					aria-labelledby="flowgraph-command-palette-title"
					tabindex="-1"
					onkeydown={(ev) => {
						if (ev.key === "Escape") commandPaletteOpen = false;
					}}
					onclick={(ev) => ev.stopPropagation()}
				>
					<div class="vac-panel-header flex items-center justify-between px-4 py-3">
						<div>
							<h3 id="flowgraph-command-palette-title" class="text-sm font-semibold">コマンド</h3>
							<p class="text-xs opacity-60">操作や追加するノードを検索します。</p>
						</div>
						<button
							type="button"
							class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800"
							onclick={() => (commandPaletteOpen = false)}
						>
							閉じる
						</button>
					</div>
					<div class="vac-panel-header px-3 py-2">
						<input
							class="w-full rounded border border-surface-300-700 bg-surface-50-950 px-3 py-2 text-sm"
							placeholder="操作名またはノード名を検索"
							bind:value={commandQuery}
						/>
					</div>
					<div class="grid gap-1 p-2">
						{#each filteredStudioCommands as command (command.id)}
							<button
								type="button"
								class="rounded px-3 py-2 text-left hover:bg-surface-100-900 disabled:opacity-40 disabled:hover:bg-transparent"
								disabled={command.disabled}
								onclick={() => void runStudioCommand(command)}
							>
								<div class="text-sm font-semibold">{command.label}</div>
								<div class="mt-0.5 text-xs opacity-60">{command.description}</div>
							</button>
						{:else}
							<div class="px-3 py-8 text-center text-sm opacity-60">一致する操作はありません。</div>
						{/each}
					</div>
				</div>
			</div>
		{/if}

		<!-- Studio workspace -->
		{#if panelLayoutPreview}
			<div class="studio-pane-switch" role="group" aria-label="編集領域の表示">
				<label><input type="checkbox" bind:checked={compactHeader} />説明を折り畳む</label>
				<label>狭い編集領域 <select aria-label="狭い編集領域" bind:value={adaptivePanes}><option value={false}>並列のまま</option><option value={true}>欄を切り替える</option></select></label>
				{#if compactPanes}<button type="button" aria-pressed={compactPane === "canvas"} onclick={() => compactPane = "canvas"}>キャンバス</button>{/if}
				<button type="button" aria-pressed={compactPanes ? compactPane === "files" : filesVisible} onclick={() => { if (compactPanes) compactPane = "files"; else filesVisible = !filesVisible; }}>ファイル一覧</button>
				<button type="button" aria-pressed={compactPanes ? compactPane === "inspector" : inspectorVisible} onclick={() => { if (compactPanes) compactPane = "inspector"; else inspectorVisible = !inspectorVisible; }}>ノード一覧・設定</button>
				<span title={flowgraphStore.selectedNodeIds.join(" · ")}>選択: {flowgraphStore.selectedNodeIds.length > 1 ? `${flowgraphStore.selectedNodeIds.length}ノード` : (flowgraphStore.selectedNodeId ?? "なし")}</span>
			</div>
		{/if}
		<div class="studio-workspace grid min-h-0 flex-1 gap-2" style={workspaceGridStyle} bind:clientWidth={workspaceWidth} data-testid="flowgraph-studio-workspace">
			<section hidden={compactPanes ? compactPane !== "files" : panelLayoutPreview && !filesVisible} class="studio-pane vac-panel flex min-h-[16rem] flex-col overflow-hidden" data-testid="flowgraph-files-pane">
				<div class="vac-panel-header px-3 py-2 text-xs font-semibold tracking-wider opacity-60">ファイル</div>
				<div class="min-h-0 flex-1 overflow-y-auto">
					<FlowgraphTree onopened={() => { if (compactPanes && compactPane === "files") compactPane = "canvas"; }} />
				</div>
			</section>

			<section hidden={compactPanes && compactPane !== "canvas"} class="studio-pane vac-panel flex min-h-[28rem] flex-col overflow-hidden" data-testid="flowgraph-canvas-pane">
				<div class="vac-panel-header flex items-center justify-between gap-2 px-3 py-2">
					<span class="text-xs font-semibold tracking-wider opacity-60">キャンバス</span>
					<span class="truncate text-xs opacity-60"
						>{compositionOverview ? "プロファイル全体" : (flowgraphStore.currentFq ?? "ファイル未選択")}</span
					>
				</div>
				<div class="min-h-0 flex-1 overflow-hidden">
					{#if compositionOverview}
						<FlowgraphCompositionOverview onopen={() => (compositionOverview = false)} />
					{:else}
						<FlowgraphCanvas onselectfile={() => { filesVisible = true; if (compactPanes) compactPane = "files"; }} />
					{/if}
				</div>
			</section>

			<div hidden={compactPanes ? compactPane !== "inspector" : panelLayoutPreview && !inspectorVisible} class="studio-side-stack grid min-h-[28rem] grid-rows-[minmax(12rem,1fr)_minmax(12rem,1fr)] gap-2" class:inspector-tabs={inspectorTabs}>
				{#if inspectorTabs}
					<div class="inspector-switch" role="group" aria-label="編集パネルの表示">
						<button type="button" aria-pressed={inspectorView === "nodes"} onclick={() => (inspectorView = "nodes")}>ノードを追加</button>
						<button type="button" aria-pressed={inspectorView === "properties"} title="用途の設定案内・試験入力と、選択したノードの設定を開きます" onclick={() => (inspectorView = "properties")}>設定・用途の案内</button>
					</div>
				{/if}
				<section class="vac-panel flex min-h-0 flex-col overflow-hidden" hidden={inspectorTabs && inspectorView !== "nodes"}>
					<div class="vac-panel-header px-3 py-2 text-xs font-semibold tracking-wider opacity-60">ノード一覧</div>
					<div class="min-h-0 flex-1 overflow-hidden">
						<FlowgraphPalette />
					</div>
				</section>
				<section class="vac-panel flex min-h-0 flex-col overflow-hidden" hidden={inspectorTabs && inspectorView !== "properties"}>
					<div class="vac-panel-header px-3 py-2 text-xs font-semibold tracking-wider opacity-60">設定</div>
					<div class="min-h-0 flex-1 overflow-y-auto">
						<FeatureSetupGuide />
						<FlowgraphPropertyEditor />
					</div>
				</section>
			</div>
		</div>

		<!-- Problems -->
		<section class="vac-panel flex shrink-0 flex-col overflow-hidden" style={problemsStyle} data-testid="flowgraph-problems-pane">
			<div class="vac-panel-header flex items-center justify-between gap-2 px-3 py-2">
				<span class="text-xs font-semibold tracking-wider opacity-60">問題</span>
				<button
					type="button"
					class="rounded px-2 py-0.5 text-[10px] hover:bg-surface-200-800"
					aria-expanded={problemsOpen}
					onclick={() => (problemsOpen = !problemsOpen)}
				>
					{problemsOpen ? "折り畳む" : "展開する"}
				</button>
			</div>
			{#if problemsOpen}
				<div class="min-h-0 flex-1 overflow-y-auto">
					<FlowgraphDiagnostics />
				</div>
			{/if}
		</section>
	</div>
{/snippet}

{#if flowgraphStore.partialWriteFailure}
	<section class="mb-3 rounded border border-warning-500 p-3 text-sm" role="status" aria-label="取込の途中失敗">
		<p class="font-semibold">{flowgraphStore.partialWriteFailure.operation}の途中失敗の記録</p>
		<p class="mt-1 break-all text-xs">開始時の保存先表示: {flowgraphStore.partialWriteFailure.root}</p>
		<p class="mt-2">一部の保存が完了している場合があります。再試行する前に、保存済みのファイルを確認してください。</p>
		<details open class="mt-2"><summary class="cursor-pointer">保存状況とバックアップ</summary><p class="mt-2 whitespace-pre-wrap break-all text-xs">{flowgraphStore.partialWriteFailure.detail}</p></details>
		<button type="button" class="mt-2 text-xs underline" onclick={() => (flowgraphStore.partialWriteFailure = null)}>記録を確認して閉じる</button>
	</section>
{/if}

{#if focusedWorkspace}
	<nav class="workspace-choice" aria-label="機能の作り方">
		<button type="button" aria-pressed={workspaceView === "studio"} onclick={() => (workspaceView = "studio")}>自由に組み立てる</button>
		<button type="button" aria-pressed={workspaceView === "library"} onclick={() => (workspaceView = "library")}>機能を追加・管理</button>
		<span>{flowgraphStore.currentFq ? `${flowgraphPathForDisplay(flowgraphStore.currentFq)}${flowgraphStore.isDirty ? " · 未保存" : ""}` : "Flowgraph Studio"}</span>
	</nav>
{/if}
<div class="mb-4 grid gap-3 workspace-library" hidden={focusedWorkspace && workspaceView !== "library"}>
	<FeatureAnchor id="feature-library"><FeatureLibrary expanded={focusedWorkspace && workspaceView === "library"} onedit={focusedWorkspace ? () => (workspaceView = "studio") : undefined} /></FeatureAnchor>
</div>
{#if recoveryPreview && (!focusedWorkspace || workspaceView === "studio")}
 <nav class="workspace-choice" aria-label="履歴の配置比較">
  <span>履歴の配置 · 確認用</span>
  <button type="button" aria-pressed={recoveryPlacement==='side'} onclick={()=>recoveryPlacement='side'}>A · 編集の横に置く</button>
  <button type="button" aria-pressed={recoveryPlacement==='drawer'} onclick={()=>recoveryPlacement='drawer'}>B · 必要時に開く</button>
 </nav>
{/if}
<div class:recovery-side={recoveryPreview && recoveryPlacement==='side'}>
<div class="workspace-editor" hidden={focusedWorkspace && workspaceView !== "studio"}>
	{#if !compactHeader}<p class="text-xs opacity-65">保存すると接続中のVACのFlowgraph全体を再読み込みします。一時的な処理状態は初期化される場合があります。プロファイル本体の保存とは適用方法が異なります。</p>{/if}
<FeatureAnchor id="flowgraph-studio">{@render studioWorkspace()}</FeatureAnchor>
</div>
<aside><FeatureAnchor id="flowgraph-recovery"><FlowgraphRecoveryPreview drawer={recoveryPlacement==='drawer'} /></FeatureAnchor></aside>
</div>

<style>
	/* Keep controls operable without the comparison stylesheet, including short windows. */
	.studio-standard { height: max(52rem, calc(100dvh - 9rem)); }
	.studio-workspace > [hidden] { display: none; }
	.studio-pane-switch { display:flex; flex-wrap:wrap; align-items:center; gap:.5rem; }
	.studio-pane-switch button { border:1px solid var(--color-surface-300-700); border-radius:.4rem; padding:.4rem .7rem; font-size:.75rem; }
	.studio-pane-switch button[aria-pressed="true"] { border-color:var(--color-primary-500); }
	.studio-pane-switch span { font-size:.75rem; overflow-wrap:anywhere; }
	.recovery-side {display:grid;grid-template-columns:minmax(0,1fr) minmax(20rem,28rem);gap:1rem;align-items:start}
	@media(max-width:1100px) {.recovery-side {grid-template-columns:minmax(0,1fr)}}
	.workspace-library[hidden], .workspace-editor[hidden] { display: none; }
	.workspace-choice { display: flex; flex-wrap: wrap; align-items: center; gap: .6rem; margin-bottom: .8rem; }
	.workspace-choice button { padding: .65rem .9rem; border-bottom: 2px solid transparent; font-weight: 650; }
	.workspace-choice button[aria-pressed="true"] { border-color: var(--color-primary-500); }
	.workspace-choice span { margin-left: auto; font-size: .8rem; overflow-wrap: anywhere; }
	.inspector-tabs { grid-template-rows: auto minmax(0, 1fr); }
	.inspector-tabs > section[hidden] { display: none; }
	.inspector-switch { display: flex; gap: .4rem; }
	.inspector-switch button { flex: 1; border: 1px solid var(--color-surface-300-700); padding: .5rem; border-radius: .4rem; font-size: .75rem; }
	.inspector-switch button[aria-pressed="true"] { border-color: var(--color-primary-500); background: color-mix(in srgb, var(--color-primary-500) 15%, transparent); }
	.studio-workspace {
		grid-template-columns: minmax(0, 1fr);
		overflow-y: auto;
	}

	@media (min-width: 1280px) {
		.studio-workspace {
			grid-template-columns: var(--file-pane-width) minmax(34rem, 1fr) var(--inspector-pane-width);
			overflow: hidden;
		}

		.studio-pane,
		.studio-side-stack {
			min-height: 0;
		}
	}
</style>

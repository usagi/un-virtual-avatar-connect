/**
 * Phase δ-6e: Flowgraph タブの状態管理ストア。
 *
 * 責務:
 *   - サーバから取得する各種リソース（tree / node catalog / current file / diagnostics）を保持。
 *   - 「選択中ファイル」「選択中ノード」の UI 状態を保持。
 *   - `ControlEvent::FlowgraphReloaded` を購読し、tree / diagnostics / 開いているファイルを自動で再取得。
 *   - 編集 API（POST/PUT/DELETE / open-external）への薄い委譲。呼び出し側（UI）は
 *     `store.saveCurrent(...)` のような明示的なメソッドだけを叩けば済む設計。
 *
 * 非責務:
 *   - TOML のシリアライズ。GUI 側では parsed 構造 ↔ raw text の変換を行なわず、**raw TOML を
 *     single source of truth** として扱う。ノード位置やプロパティの編集は parsed を JSON で更新し、
 *     保存時に再シリアライズする（toml_edit を JS で扱うのは重いので minimal に）。
 *   - 実行トリガ。δ-6e は GUI 側から flowgraph を動かすことはしない（δ-8 で run_forever を接続する予定）。
 */

import { api } from "./api";
import { eventsStore } from "./events.svelte";
import { toastStore } from "./toasts.svelte";
import {
	findOpenFlowgraphPosition,
	groupLayoutNeedsRepair,
	layoutFlowgraph,
	layoutFlowgraphWithGroups,
	layoutNeedsRepair,
	type FlowgraphLayoutGroup,
	type FlowgraphLayoutNode,
} from "./flowgraph/flowgraphLayout";
import { collectUnmfzEndpointCandidates, type UnmfzEndpointCandidate } from "./flowgraph/unmfzEndpointCandidates";
import {
	ControlApiError,
	type FlowgraphDiagnostic,
	type FlowgraphDiagnosticsResponse,
	type FlowgraphEnumDef,
	type FlowgraphFileMeta,
	type FlowgraphFileResponse,
	type FlowgraphGraphDeclaration,
	type FlowgraphGraphInstance,
	type FlowgraphGroupDef,
	type FlowgraphNodeCatalogResponse,
	type FlowgraphNodeSpec,
	type FlowgraphPackageManifest,
	type FlowgraphTreeResponse,
	type FlowgraphTypeDef,
	type FragmentCopyRequest,
	type ZipExportRequest,
	type FlowgraphRecoveryPlan,
	type FragmentCopyResponse,
	type FragmentPasteRequest,
	type FragmentPasteResponse,
	type ZipImportOutcome,
} from "./types";

/** 書き戻し中フラグなど非同期状態をまとめて扱う列挙。 */
export type FlowgraphFetchState = "idle" | "loading" | "ok" | "error";

class FlowgraphStore {
	/** `GET /flowgraph/tree` の結果。初期 null はまだ取得していない（初回ロード前）。 */
	tree: FlowgraphTreeResponse | null = $state(null);
	treeState: FlowgraphFetchState = $state("idle");
	treeError: string | null = $state(null);
	private treeRequest = 0;

	/** `GET /flowgraph/node-catalog` の結果。カタログは更新頻度が低いので一度 fetch したら使い回す。 */
	catalog: FlowgraphNodeCatalogResponse | null = $state(null);
	catalogState: FlowgraphFetchState = $state("idle");

	/** `GET /flowgraph/diagnostics` の結果。`FlowgraphReloaded` WS でも再取得する。 */
	diagnostics: FlowgraphDiagnosticsResponse | null = $state(null);
	diagnosticsState: FlowgraphFetchState = $state("idle");
	diagnosticsError: string | null = $state(null);
	private diagnosticsRequest = 0;

	/** 現在選択中 / 編集中のファイル。null のときは何も開いていない。 */
	currentFq: string | null = $state(null);
	private currentFileRequest = 0;
	currentFile: FlowgraphFileResponse | null = $state(null);
	currentState: FlowgraphFetchState = $state("idle");
	currentError: string | null = $state(null);

	/**
	 * 未保存の変更があるか。`currentFile.raw_toml` と `draftToml` が乖離したら true。
	 * GUI の「Save」ボタン活性化や「閉じる前の確認」に使う。
	 */
	draftToml: string | null = $state(null);
	/** ノード位置やプロパティの GUI 編集を parsed 層で持つ。保存時に raw へ再シリアライズ。 */
	draftNodes: FlowgraphDraftNode[] | null = $state(null);
	draftEdges: FlowgraphDraftEdge[] | null = $state(null);
	draftGroups: FlowgraphDraftGroup[] | null = $state(null);
	draftInstances: FlowgraphGraphInstance[] | null = $state(null);
	propertyJsonDrafts: Array<{ nodeId: string; key: string; text: string; error: string }> = $state([]);

	get invalidPropertyJsonDrafts() {
		return this.propertyJsonDrafts.filter(draft => this.draftNodes?.some(node =>
			node.id === draft.nodeId && Object.prototype.hasOwnProperty.call(node.properties, draft.key)));
	}

	propertyJsonText(nodeIds: string[], key: string): string | undefined {
		const drafts = nodeIds.map(id => this.propertyJsonDrafts.find(draft => draft.nodeId === id && draft.key === key));
		return drafts.length > 0 && drafts.every(draft => draft && draft.text === drafts[0]?.text) ? drafts[0]?.text : undefined;
	}

	discardPropertyJson(nodeId: string, key: string): void {
		this.propertyJsonDrafts = this.propertyJsonDrafts.filter(draft => draft.nodeId !== nodeId || draft.key !== key);
	}

	editPropertyJson(nodeIds: string[], key: string, text: string): void {
		const ids = [...new Set(nodeIds)].filter(id => this.draftNodes?.some(node => node.id === id));
		if (!ids.length) return;
		let value: unknown;
		try { value = JSON.parse(text); tomlValue(value); }
		catch (error) {
			this.propertyJsonDrafts = [
				...this.propertyJsonDrafts.filter(draft => !ids.includes(draft.nodeId) || draft.key !== key),
				...ids.map(nodeId => ({ nodeId, key, text, error: String(error) })),
			];
			return;
		}
		this.updateNodePropertyMany(ids, key, value);
		this.propertyJsonDrafts = this.propertyJsonDrafts.filter(draft => !ids.includes(draft.nodeId) || draft.key !== key);
	}

	editPropertyNumber(nodeIds: string[], key: string, text: string, integer: boolean): void {
		const ids = [...new Set(nodeIds)].filter(id => this.draftNodes?.some(node => node.id === id));
		if (!ids.length) return;
		const value = Number(text);
		if (!text.trim() || !Number.isFinite(value) || (integer && !Number.isSafeInteger(value))) {
			const error = integer ? "整数を入力してください（正確に扱える範囲: ±9007199254740991）。" : "有限の数値を入力してください。";
			this.propertyJsonDrafts = [...this.propertyJsonDrafts.filter(draft => !ids.includes(draft.nodeId) || draft.key !== key),
				...ids.map(nodeId => ({ nodeId, key, text, error }))];
			return;
		}
		this.updateNodePropertyMany(ids, key, value);
		this.propertyJsonDrafts = this.propertyJsonDrafts.filter(draft => !ids.includes(draft.nodeId) || draft.key !== key);
	}

	private rejectInvalidPropertyJson(): boolean {
		if (!this.invalidPropertyJsonDrafts.length) return false;
		toastStore.warn("プロパティの入力を確認してください", "プロパティ欄の入力を修正するか破棄するまで保存できません。入力は保持しています。");
		return true;
	}


	/** いま選択しているキャンバス上のノード ID。プロパティエディタが参照する。 */
	selectedNodeId: string | null = $state(null);
	/** 複数選択中のノード ID。先頭は Inspector が扱う primary selection。 */
	selectedNodeIds: string[] = $state([]);
	/** キャンバス上で選択している視覚 group。node selection とは別の Inspector 対象。 */
	selectedGroupId: string | null = $state(null);
	/** 明示的な全体配置後だけCanvasを再fitさせるための世代番号。通常のdragでは増やさない。 */
	layoutRevision: number = $state(0);
	undoStack: FlowgraphHistoryEntry[] = $state([]);
	redoStack: FlowgraphHistoryEntry[] = $state([]);

	/** 保存中 / 削除中 / 新規作成中のスピナー用。同時に複数走らない前提。 */
	mutating: boolean = $state(false);
	partialWriteFailure: { operation: string; root: string; detail: string } | null = $state(null);

	// Drafts stay in memory in all builds, scoped by the root returned with each file.
	retainFileDrafts = true;
	workspaceDrafts: ReturnType<FlowgraphStore["captureWorkspaceDraft"]>[] = $state([]);
	get hasUnsavedDrafts(): boolean {
		return this.isDirty || this.workspaceDrafts.length > 0;
	}
	hasDraftForFile(fq: string, root?: string): boolean {
		// Unknown destinations remain conservative; reviewed operations carry their root.
		return (this.currentFq === fq && this.isDirty && (!root || !this.currentFile?.root_dir || this.currentFile.root_dir === root))
			|| this.workspaceDrafts.some(item => item.fq === fq && (!root || !item.root || item.root === root));
	}
	get canRetainCurrentDraft(): boolean {
		return this.retainFileDrafts && !!this.currentFile?.root_dir;
	}

	captureWorkspaceDraft() {
		return {
			root: this.currentFile?.root_dir,
			fq: this.currentFq,
			editor: {
				currentFq: this.currentFq, currentFile: this.currentFile,
				draftToml: this.draftToml, draftNodes: this.draftNodes, propertyJsonDrafts: this.propertyJsonDrafts,
				draftEdges: this.draftEdges, draftGroups: this.draftGroups, draftInstances: this.draftInstances,
				selectedNodeId: this.selectedNodeId, selectedNodeIds: this.selectedNodeIds, selectedGroupId: this.selectedGroupId,
				undoStack: this.undoStack, redoStack: this.redoStack,
			},
			lastDeletion: this.#lastDeletion,
		};
	}
	private retainCurrentWorkspaceDraft(): void {
		if (!this.retainFileDrafts || !this.isDirty || !this.currentFq || !this.currentFile?.root_dir) return;
		const draft = this.captureWorkspaceDraft();
		this.workspaceDrafts = [...this.workspaceDrafts.filter(item => item.root !== draft.root || item.fq !== draft.fq), draft];
	}
	private restoreWorkspaceDraft(root: string | undefined, fq: string): boolean {
		if (!this.retainFileDrafts || !root) return false;
		const draft = this.workspaceDrafts.find(item => item.root === root && item.fq === fq);
		if (!draft) return false;
		Object.assign(this, draft.editor);
		this.#lastDeletion = draft.lastDeletion;
		this.workspaceDrafts = this.workspaceDrafts.filter(item => item !== draft);
		this.currentState = "ok";
		this.currentError = null;
		return true;
	}

	workspaceDraftText(root: string, fq: string): string | null {
		const draft = this.workspaceDrafts.find(item => item.root === root && item.fq === fq);
		if (!draft) return null;
		const editor = draft.editor;
		if (editor.propertyJsonDrafts?.some(draft => editor.draftNodes?.some(node => node.id === draft.nodeId && Object.prototype.hasOwnProperty.call(node.properties, draft.key)))) {
			return JSON.stringify({ format: "unvac-property-draft-recovery-v1", root, fq,
				note: "未完のプロパティ入力を含む救出用データです。Flowgraphとして直接実行・取込するファイルではありません。",
				editor }, null, 2);
		}
		if (!editor.draftNodes || !editor.draftEdges) return editor.draftToml;
		const parsed = editor.currentFile?.parsed;
		try {
		return serializeFlowgraph({
			meta: parsed?.meta ?? null, graph: parsed?.graph, package: parsed?.package ?? null,
			nodes: editor.draftNodes, edges: editor.draftEdges,
			enums: parsed?.enums ?? [], types: parsed?.types ?? [],
			groups: editor.draftGroups ?? [], instances: editor.draftInstances ?? [],
		});
		} catch (error) {
			return JSON.stringify({ format: "unvac-property-draft-recovery-v1", root, fq, note: describeError(error), editor }, null, 2);
		}
	}
	discardWorkspaceDraft(root: string, fq: string): boolean {
		if (this.mutating) return false;
		const remaining = this.workspaceDrafts.filter(item => item.root !== root || item.fq !== fq);
		if (remaining.length === this.workspaceDrafts.length) return false;
		this.workspaceDrafts = remaining;
		return true;
	}

	#wsUnsubscribe: (() => void) | null = null;

	// -------------------------------------------------------------------------
	// Reads
	// -------------------------------------------------------------------------

	async refreshAll(): Promise<void> {
		await Promise.all([this.refreshTree(), this.refreshCatalog(), this.refreshDiagnostics()]);
	}

	async refreshTree(): Promise<void> {
		const request = ++this.treeRequest;
		this.treeState = "loading";
		try {
			const tree = await api.flowgraphTree();
			if (request !== this.treeRequest) return;
			this.tree = tree;
			this.treeState = "ok";
			this.treeError = null;
		} catch (e) {
			if (request !== this.treeRequest) return;
			this.treeState = "error";
			this.treeError = describeError(e);
		}
	}

	async refreshCatalog(force = false): Promise<void> {
		if (this.catalog && !force) return;
		this.catalogState = "loading";
		try {
			this.catalog = await api.flowgraphNodeCatalog();
			this.catalogState = "ok";
		} catch (e) {
			this.catalogState = "error";
			toastStore.warn("ノードカタログ取得失敗", describeError(e));
		}
	}

	async refreshDiagnostics(): Promise<void> {
		const request = ++this.diagnosticsRequest;
		this.diagnosticsState = "loading";
		this.diagnosticsError = null;
		try {
			const diagnostics = await api.flowgraphDiagnostics();
			if (request !== this.diagnosticsRequest) return;
			this.diagnostics = diagnostics;
			this.diagnosticsState = "ok";
		} catch (e) {
			if (request !== this.diagnosticsRequest) return;
			// Keep the last result available, but never represent it as a fresh success.
			this.diagnosticsState = "error";
			this.diagnosticsError = describeError(e);
		}
	}

	// -------------------------------------------------------------------------
	// Current file
	// -------------------------------------------------------------------------

	async openFile(fq: string, expectedRoot?: string): Promise<boolean> {
		if (this.mutating) {
			toastStore.warn("変更処理中です", "処理が終わってからファイルを開いてください。編集中の内容は保持しています。");
			return false;
		}
		if (this.isDirty && !(this.retainFileDrafts && this.currentFile?.root_dir)) {
			toastStore.warn("未保存の編集を保持しています", "保存先を確認できないため別ファイルへ移動できません。現在の編集を保存してから開いてください。");
			return false;
		}
		const request = ++this.currentFileRequest;
		this.currentState = "loading";
		this.currentError = null;
		try {
			const resp = await api.flowgraphFile(fq);
			if (request !== this.currentFileRequest) return false;
			if (expectedRoot && resp.root_dir !== expectedRoot) {
				this.currentState = this.currentFile ? "ok" : "idle";
				toastStore.warn("保存先が異なります", "保持中の編集は別の構成に属しています。元の構成の画面で開いてください。下書きは保持しています。");
				return false;
			}
			this.retainCurrentWorkspaceDraft();
			if (this.restoreWorkspaceDraft(resp.root_dir, fq)) return true;
			this.#lastDeletion = null;
			this.currentFq = fq;
			this.selectedNodeId = null;
			this.selectedNodeIds = [];
			this.selectedGroupId = null;
			this.currentFile = resp;
			this.draftToml = resp.raw_toml;
			this.propertyJsonDrafts = [];
			this.draftNodes = resp.parsed
				? resp.parsed.nodes.map((n) => ({
						id: n.id,
						feature: n.feature,
						position: n.position ?? null,
						properties: { ...n.properties },
					}))
				: null;
			this.draftEdges = resp.parsed ? resp.parsed.edges.map((e) => ({ from: e.from, to: e.to })) : null;
			this.draftGroups = resp.parsed
				? (resp.parsed.groups ?? []).map((g) => ({
						id: g.id,
						label: g.label ?? null,
						node_ids: [...g.node_ids],
						color: g.color ?? null,
					}))
				: null;
			this.draftInstances = resp.parsed
				? (resp.parsed.instances ?? []).map((instance) => ({
						...instance,
						position: instance.position ? [instance.position[0], instance.position[1]] : null,
					}))
				: null;
			this.undoStack = [];
			this.redoStack = [];
			const repairedLayout = this.repairOverlappingLayout();
			this.currentState = "ok";
			if (repairedLayout) {
				toastStore.info("ノード配置を自動補正（未保存）", "重複または未配置の座標を接続順に再配置しました。元の配置には「元に戻す」で戻せます。保存するまでファイルは変更しません。");
			}
			return true;
		} catch (e) {
			if (request !== this.currentFileRequest) return false;
			this.currentState = this.currentFile ? "ok" : "error";
			this.currentError = describeError(e);
			toastStore.error("ファイル読み込み失敗", `${fq}: ${describeError(e)}`);
			return false;
		}
	}

	closeFile(): void {
		++this.currentFileRequest;
		this.currentFq = null;
		this.currentFile = null;
		this.draftToml = null;
		this.propertyJsonDrafts = [];
		this.draftNodes = null;
		this.draftEdges = null;
		this.draftGroups = null;
		this.draftInstances = null;
		this.selectedNodeId = null;
		this.selectedNodeIds = [];
		this.selectedGroupId = null;
		this.undoStack = [];
		this.redoStack = [];
		this.currentState = "idle";
		this.currentError = null;
	}

	/** draft 層のノード位置を更新する。Svelte Flow から呼ぶ。 */
	updateNodePosition(id: string, x: number, y: number): void {
		if (!this.draftNodes) return;
		this.#pushHistory("Move node");
		const next = this.draftNodes.map((n) => (n.id === id ? { ...n, position: [x, y] as [number, number] } : n));
		this.draftNodes = next;
	}

	updateGraphInstancePosition(id: string, x: number, y: number): void {
		if (!this.draftInstances) return;
		this.#pushHistory("Move graph instance");
		this.draftInstances = this.draftInstances.map((instance) =>
			instance.id === id ? { ...instance, position: [x, y] as [number, number] } : instance
		);
	}

	moveGroupBy(groupId: string, dx: number, dy: number): boolean {
		if (!this.draftNodes || !this.draftGroups) return false;
		if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) return false;
		const group = this.draftGroups.find((g) => g.id === groupId);
		if (!group) return false;
		const memberIds = new Set(group.node_ids);
		if (![...this.draftNodes, ...(this.draftInstances ?? [])].some((n) => memberIds.has(n.id))) return false;
		this.#pushHistory("Move group");
		this.draftNodes = this.draftNodes.map((n, i) => {
			if (!memberIds.has(n.id)) return n;
			const pos = n.position ?? ([50 + (i % 6) * 220, 50 + Math.floor(i / 6) * 140] as [number, number]);
			return {
				...n,
				position: [Math.round(pos[0] + dx), Math.round(pos[1] + dy)],
			};
		});
		if (this.draftInstances) {
			this.draftInstances = this.draftInstances.map((instance, i) => {
				if (!memberIds.has(instance.id)) return instance;
				const index = (this.draftNodes?.length ?? 0) + i;
				const pos = instance.position ?? [50 + (index % 6) * 220, 50 + Math.floor(index / 6) * 140];
				return { ...instance, position: [Math.round(pos[0] + dx), Math.round(pos[1] + dy)] as [number, number] };
			});
		}
		this.selectedGroupId = groupId;
		return true;
	}

	updateNodeProperty(id: string, key: string, value: unknown): void {
		if (!this.draftNodes) return;
		this.#pushHistory("Edit property");
		const next = this.draftNodes.map((n) => (n.id === id ? { ...n, properties: { ...n.properties, [key]: value } } : n));
		this.draftNodes = next;
	}

	updateNodePropertyMany(ids: string[], key: string, value: unknown): void {
		if (!this.draftNodes || ids.length === 0) return;
		const selected = new Set(ids);
		if (!this.draftNodes.some((n) => selected.has(n.id))) return;
		this.#pushHistory("Edit properties");
		this.draftNodes = this.draftNodes.map((n) => (selected.has(n.id) ? { ...n, properties: { ...n.properties, [key]: value } } : n));
	}

	removeNodeProperty(id: string, key: string): void {
		if (!this.draftNodes) return;
		this.#pushHistory("Remove property");
		const next = this.draftNodes.map((n) => {
			if (n.id !== id) return n;
			const props = { ...n.properties };
			delete props[key];
			return { ...n, properties: props };
		});
		this.draftNodes = next;
	}

	removeNodePropertyMany(ids: string[], key: string): void {
		if (!this.draftNodes || ids.length === 0) return;
		const selected = new Set(ids);
		if (!this.draftNodes.some((n) => selected.has(n.id) && Object.prototype.hasOwnProperty.call(n.properties, key))) return;
		this.#pushHistory("Remove properties");
		this.draftNodes = this.draftNodes.map((n) => {
			if (!selected.has(n.id)) return n;
			const props = { ...n.properties };
			delete props[key];
			return { ...n, properties: props };
		});
	}

	addNode(node: FlowgraphDraftNode): void {
		if (!this.draftNodes) this.draftNodes = [];
		this.#pushHistory("Add node");
		this.draftNodes = [...this.draftNodes, node];
	}

	/**
	 * Phase ο-6: カタログのノードを 1 つ draft に追加（パレット click / canvas drop 共通）。
	 * `position` が null のときは既存ノード重心付近の空き位置へ配置する。
	 */
	addCatalogNodeAt(spec: FlowgraphNodeSpec, position: [number, number] | null): boolean {
		if (!this.currentFq) {
			toastStore.warn("ファイル未選択", "先にファイルを選択してください。");
			return false;
		}
		if (!this.draftNodes) this.draftNodes = [];
		const nodes = this.draftNodes;
		const xy = position ?? this.#pickNewNodePositionNearCentroid(nodes, spec);
		const id = this.#makeUniqueNodeId(spec.feature, nodes);
		const node: FlowgraphDraftNode = {
			id,
			feature: spec.feature,
			position: [Math.round(xy[0]), Math.round(xy[1])],
			properties: this.#defaultPropertiesForSpec(spec),
		};
		this.addNode(node);
		this.selectedNodeId = node.id;
		this.selectedNodeIds = [node.id];
		return true;
	}

	unmfzEndpointCandidates(): UnmfzEndpointCandidate[] {
		return collectUnmfzEndpointCandidates(this.diagnostics);
	}

	addUnmfzEndpointGraph(candidate: UnmfzEndpointCandidate, mode: "publish" | "subscribe", position: [number, number] | null): boolean {
		if (!this.currentFq) {
			toastStore.warn("ファイル未選択", "先にファイルを選択してください。");
			return false;
		}
		if ((mode === "publish" && candidate.direction === "output") || (mode === "subscribe" && candidate.direction === "input")) {
			toastStore.warn("方向が一致しません", `${candidate.label} (${candidate.direction})`);
			return false;
		}
		if (!this.draftNodes) this.draftNodes = [];
		if (!this.draftEdges) this.draftEdges = [];

		const jsonLiteral = this.findSpec("flowgraph.literal.json");
		const stringLiteral = this.findSpec("flowgraph.literal.string");
		const resolver = this.findSpec("flowgraph.unmfz.resolve_endpoint");
		const target = this.findSpec(mode === "publish" ? "flowgraph.unmfz.publish" : "flowgraph.ingress.unmfz");
		if (!jsonLiteral || !target || (mode === "publish" && (!stringLiteral || !resolver))) {
			toastStore.warn("UNMF/Z ノード不足", "literal / resolver / publish / ingress のカタログを確認してください。");
			return false;
		}

		const start = position ?? this.#pickNewNodePositionNearCentroid(this.draftNodes);
		const x = Math.round(start[0]);
		const y = Math.round(start[1]);
		const nextNodes = [...this.draftNodes];
		const nextGroups = [...(this.draftGroups ?? [])];
		const makeNode = (spec: FlowgraphNodeSpec, dx: number, dy: number, properties: Record<string, unknown>): FlowgraphDraftNode => {
			const id = this.#makeUniqueNodeId(spec.feature, nextNodes);
			const node: FlowgraphDraftNode = {
				id,
				feature: spec.feature,
				position: [x + dx, y + dy],
				properties,
			};
			nextNodes.push(node);
			return node;
		};

		const provenance = endpointCandidateProvenance(candidate, mode);
		const endpointPayload = {
			unaa_z_provenance: provenance,
			endpoints: [cloneJsonObject(candidate.endpoint)],
		};
		const endpointsNode = makeNode(jsonLiteral, 0, 0, { value: endpointPayload });
		let targetNode: FlowgraphDraftNode;
		let nodeIds: string[];
		let newEdges: FlowgraphDraftEdge[];

		if (mode === "publish") {
			const direction = candidate.direction === "both" ? "input" : candidate.direction;
			const appNode = makeNode(stringLiteral!, 0, 116, { value: candidate.appId });
			const endpointNode = makeNode(stringLiteral!, 0, 232, {
				value: candidate.endpointId,
			});
			const directionNode = makeNode(stringLiteral!, 0, 348, { value: direction });
			const resolverNode = makeNode(resolver!, 260, 116, {});
			targetNode = makeNode(target, 540, 116, {});
			nodeIds = [endpointsNode.id, appNode.id, endpointNode.id, directionNode.id, resolverNode.id, targetNode.id];
			newEdges = [
				{ from: `${endpointsNode.id}:value`, to: `${resolverNode.id}:endpoints` },
				{ from: `${appNode.id}:value`, to: `${resolverNode.id}:app_id` },
				{ from: `${endpointNode.id}:value`, to: `${resolverNode.id}:endpoint_id` },
				{ from: `${directionNode.id}:value`, to: `${resolverNode.id}:direction` },
				{ from: `${resolverNode.id}:base_key_expr`, to: `${targetNode.id}:base_key_expr` },
				{ from: `${resolverNode.id}:topic_mode`, to: `${targetNode.id}:topic_mode` },
				{ from: `${resolverNode.id}:schema_major`, to: `${targetNode.id}:schema_major` },
				{ from: `${resolverNode.id}:endpoint`, to: `${targetNode.id}:endpoint` },
			];
		} else {
			targetNode = makeNode(target, 300, 116, {
				base_key_expr: candidate.baseKeyExpr,
				topic_mode: candidate.topicMode,
				schema_major: candidate.schemaMajor,
				fixed_channel: candidate.endpointId ? `unmfz:${candidate.endpointId}` : "",
			});
			nodeIds = [endpointsNode.id, targetNode.id];
			newEdges = [];
		}

		this.#pushHistory("Add UNMF/Z endpoint graph");
		nextGroups.push({
			id: this.#makeUniqueGroupId(nextGroups),
			label: endpointCandidateGroupLabel(candidate, mode),
			node_ids: nodeIds,
			color: mode === "publish" ? "#14b8a6" : "#0ea5e9",
		});
		this.draftNodes = nextNodes;
		this.draftEdges = [...this.draftEdges, ...newEdges];
		this.draftGroups = nextGroups;
		this.selectedNodeId = targetNode.id;
		this.selectedNodeIds = nodeIds;
		this.selectedGroupId = nextGroups[nextGroups.length - 1]?.id ?? null;
		toastStore.success("UNMF/Z endpoint graph を追加しました", `${mode}: ${candidate.label}`);
		return true;
	}

	/** Phase ο-6: 選択中ノードを (+24,+24) オフセットで複製（エッジはコピーしない）。 */
	duplicateSelectedNode(): boolean {
		const ids = this.selectedNodeIds.length > 0 ? this.selectedNodeIds : this.selectedNodeId ? [this.selectedNodeId] : [];
		return this.duplicateNodes(ids);
	}

	duplicateNodes(ids: string[]): boolean {
		if (!this.draftNodes || ids.length === 0) return false;
		const sourceIds = new Set(ids);
		const sources = this.draftNodes.filter((n) => sourceIds.has(n.id));
		const instanceSources = (this.draftInstances ?? []).filter((n) => sourceIds.has(n.id));
		if (sources.length + instanceSources.length === 0) return false;
		this.#pushHistory(sources.length + instanceSources.length > 1 ? "Duplicate nodes" : "Duplicate node");
		let nextNodes = [...this.draftNodes];
		const nextInstances = [...(this.draftInstances ?? [])];
		const duplicatedIds: string[] = [];
		for (const src of sources) {
			const id = this.#makeUniqueNodeId(src.feature, nextNodes);
			const basePos = src.position ?? [100, 100];
			const dup: FlowgraphDraftNode = {
				id,
				feature: src.feature,
				position: [Math.round(basePos[0] + 24), Math.round(basePos[1] + 24)],
				properties: { ...src.properties },
			};
			nextNodes = [...nextNodes, dup];
			duplicatedIds.push(id);
		}
		for (const src of instanceSources) {
			const id = this.#makeUniqueNodeId(src.id, [...nextNodes, ...nextInstances]);
			const basePos = src.position ?? [100, 100];
			nextInstances.push({ ...src, id, position: [Math.round(basePos[0] + 24), Math.round(basePos[1] + 24)] });
			duplicatedIds.push(id);
		}
		this.draftNodes = nextNodes;
		this.draftInstances = nextInstances;
		this.selectedNodeIds = duplicatedIds;
		this.selectedNodeId = duplicatedIds[0] ?? null;
		return true;
	}

	alignSelectedNodes(axis: "x" | "y"): boolean {
		if (!this.draftNodes || this.selectedNodeIds.length < 2) return false;
		const selected = new Set(this.selectedNodeIds);
		const selectedItems = [...this.draftNodes, ...(this.draftInstances ?? [])].filter((n) => selected.has(n.id));
		if (selectedItems.length < 2) return false;
		const anchor = selectedItems[0].position ?? [100, 100];
		this.#pushHistory(axis === "x" ? "Align vertical" : "Align horizontal");
		const align = <T extends { id: string; position?: [number, number] | null }>(n: T): T => {
			if (!selected.has(n.id)) return n;
			const pos = n.position ?? [100, 100];
			return {
				...n,
				position: (axis === "x" ? [anchor[0], pos[1]] : [pos[0], anchor[1]]) as [number, number],
			};
		};
		this.draftNodes = this.draftNodes.map(align);
		if (this.draftInstances) this.draftInstances = this.draftInstances.map(align);
		return true;
	}

	distributeSelectedNodes(axis: "x" | "y"): boolean {
		if (!this.draftNodes || this.selectedNodeIds.length < 3) return false;
		const selected = new Set(this.selectedNodeIds);
		const rows = [...this.draftNodes, ...(this.draftInstances ?? [])]
			.filter((n) => selected.has(n.id))
			.map((n) => ({
				id: n.id,
				position: n.position ?? ([100, 100] as [number, number]),
			}))
			.sort((a, b) => (axis === "x" ? a.position[0] - b.position[0] : a.position[1] - b.position[1]));
		if (rows.length < 3) return false;
		this.#pushHistory(axis === "x" ? "Distribute horizontal" : "Distribute vertical");
		const first = rows[0].position;
		const last = rows[rows.length - 1].position;
		const step = (axis === "x" ? last[0] - first[0] : last[1] - first[1]) / (rows.length - 1);
		// Local lookup for one layout command, not reactive UI state.
		// eslint-disable-next-line svelte/prefer-svelte-reactivity
		const positions = new Map<string, [number, number]>();
		rows.forEach((row, i) => {
			positions.set(
				row.id,
				axis === "x" ? [Math.round(first[0] + step * i), row.position[1]] : [row.position[0], Math.round(first[1] + step * i)]
			);
		});
		this.draftNodes = this.draftNodes.map((n) => {
			const pos = positions.get(n.id);
			return pos ? { ...n, position: pos } : n;
		});
		if (this.draftInstances) this.draftInstances = this.draftInstances.map((n) => {
			const pos = positions.get(n.id);
			return pos ? { ...n, position: pos } : n;
		});
		return true;
	}

	/** 接続関係とノードの推定寸法から、現在ファイル全体を重ならない列へ再配置する。 */
	autoLayoutCurrentGraph(): boolean {
		const nodeCount = (this.draftNodes?.length ?? 0) + (this.draftInstances?.length ?? 0);
		if (nodeCount < 2) return false;
		const positions = this.previewAutoLayoutPositions();
		this.#pushHistory("Auto layout graph");
		if (this.draftNodes) {
			this.draftNodes = this.draftNodes.map((node) => ({ ...node, position: positions[node.id] ?? node.position ?? null }));
		}
		if (this.draftInstances) {
			this.draftInstances = this.draftInstances.map((instance) => ({
				...instance,
				position: positions[instance.id] ?? instance.position ?? null,
			}));
		}
		this.layoutRevision += 1;
		return true;
	}

	/** 読み込み時に保存済み座標を検査し、衝突している場合だけdraftを自動補正する。 */
	repairOverlappingLayout(): boolean {
		const layoutNodes = this.layoutNodesForCurrentDraft();
		if (layoutNodes.length < 2) return false;
		const currentPositions: Partial<Record<string, [number, number]>> = {};
		for (const node of this.draftNodes ?? []) {
			if (node.position) currentPositions[node.id] = [node.position[0], node.position[1]];
		}
		for (const instance of this.draftInstances ?? []) {
			if (instance.position) currentPositions[instance.id] = [instance.position[0], instance.position[1]];
		}
		const layoutGroups = this.layoutGroupsForCurrentDraft();
		if (!layoutNeedsRepair(layoutNodes, currentPositions, 32) && !groupLayoutNeedsRepair(layoutNodes, currentPositions, layoutGroups)) {
			return false;
		}
		const positions = this.previewAutoLayoutPositions();
		this.#pushHistory("Repair overlapping layout");
		if (this.draftNodes) {
			this.draftNodes = this.draftNodes.map((node) => ({ ...node, position: positions[node.id] ?? node.position ?? null }));
		}
		if (this.draftInstances) {
			this.draftInstances = this.draftInstances.map((instance) => ({
				...instance,
				position: positions[instance.id] ?? instance.position ?? null,
			}));
		}
		this.layoutRevision += 1;
		return true;
	}

	/** Canvas上だけで使う未保存レイアウト。position欠落時の固定格子fallbackもこれへ揃える。 */
	previewAutoLayoutPositions(): Record<string, [number, number]> {
		const layoutNodes = this.layoutNodesForCurrentDraft();
		const ids = new Set(layoutNodes.map((node) => node.id));
		const layoutEdges = (this.draftEdges ?? []).flatMap((edge) => {
			const from = parsePortRef(edge.from).nodeId;
			const to = parsePortRef(edge.to).nodeId;
			return ids.has(from) && ids.has(to) ? [{ from, to }] : [];
		});
		const layoutGroups = this.layoutGroupsForCurrentDraft();
		return layoutGroups.length > 0
			? layoutFlowgraphWithGroups(layoutNodes, layoutEdges, layoutGroups)
			: layoutFlowgraph(layoutNodes, layoutEdges);
	}

	layoutNodesForCurrentDraft(): FlowgraphLayoutNode[] {
		const layoutNodes: FlowgraphLayoutNode[] = (this.draftNodes ?? []).map((node) => {
			const spec = this.findSpec(node.feature);
			const portRows = Math.max(spec?.inputs.length ?? 0, spec?.outputs.length ?? 0);
			const groupRows = Math.ceil(this.groupBadgesForNode(node.id).length / 2);
			return {
				id: node.id,
				width: 300,
				height: Math.max(110, 78 + portRows * 15 + groupRows * 18),
			};
		});
		for (const instance of this.draftInstances ?? []) {
			const summary = this.diagnostics?.composition.instances.find(
				(candidate) => candidate.owner_fq === this.currentFq && candidate.id === instance.id
			);
			const graph = this.diagnostics?.composition.graphs.find((candidate) => candidate.fq === summary?.graph_fq);
			const portRows = Math.max(graph?.inputs.length ?? 0, graph?.outputs.length ?? 0);
			layoutNodes.push({ id: instance.id, width: 300, height: Math.max(110, 78 + portRows * 15) });
		}
		return layoutNodes;
	}

	layoutGroupsForCurrentDraft(): FlowgraphLayoutGroup[] {
		return (this.draftGroups ?? []).map((group) => ({ id: group.id, nodeIds: [...group.node_ids] }));
	}

	groupSelectedNodes(): boolean {
		if (!this.draftNodes || this.selectedNodeIds.length < 2) return false;
		if (!this.draftGroups) this.draftGroups = [];
		const selectedIds = [...this.selectedNodeIds];
		this.#pushHistory("Create group");
		const id = this.#makeUniqueGroupId(this.draftGroups);
		const index = this.draftGroups.length + 1;
		this.draftGroups = [
			...this.draftGroups,
			{
				id,
				label: `Group ${index}`,
				node_ids: selectedIds,
				color: "#38bdf8",
			},
		];
		this.selectedGroupId = id;
		const groupedPositions = this.previewAutoLayoutPositions();
		this.draftNodes = this.draftNodes.map((node) => ({
			...node,
			position: groupedPositions[node.id] ?? node.position,
		}));
		if (this.draftInstances) {
			this.draftInstances = this.draftInstances.map((instance) => ({
				...instance,
				position: groupedPositions[instance.id] ?? instance.position,
			}));
		}
		this.layoutRevision += 1;
		return true;
	}

	removeGroup(id: string): boolean {
		if (!this.draftGroups) return false;
		if (!this.draftGroups.some((g) => g.id === id)) return false;
		this.#pushHistory("Remove group");
		this.draftGroups = this.draftGroups.filter((g) => g.id !== id);
		if (this.selectedGroupId === id) this.selectedGroupId = null;
		return true;
	}

	selectGroup(id: string): boolean {
		if (!this.draftGroups?.some((g) => g.id === id)) return false;
		this.selectedGroupId = id;
		return true;
	}

	addSelectedNodesToGroup(id: string): boolean {
		if (!this.draftGroups || !this.draftNodes || this.selectedNodeIds.length === 0) return false;
		const group = this.draftGroups.find((g) => g.id === id);
		if (!group) return false;
		const existingNodes = new Set([...this.draftNodes, ...(this.draftInstances ?? [])].map((n) => n.id));
		const addIds = this.selectedNodeIds.filter((nodeId) => existingNodes.has(nodeId) && !group.node_ids.includes(nodeId));
		if (addIds.length === 0) return false;
		this.#pushHistory("Add nodes to group");
		this.draftGroups = this.draftGroups.map((g) => (g.id === id ? { ...g, node_ids: [...g.node_ids, ...addIds] } : g));
		this.selectedGroupId = id;
		return true;
	}

	removeSelectedNodesFromGroup(id: string): boolean {
		if (!this.draftGroups || this.selectedNodeIds.length === 0) return false;
		const selected = new Set(this.selectedNodeIds);
		const group = this.draftGroups.find((g) => g.id === id);
		if (!group || !group.node_ids.some((nodeId) => selected.has(nodeId))) return false;
		this.#pushHistory("Remove nodes from group");
		this.draftGroups = this.draftGroups
			.map((g) =>
				g.id === id
					? {
							...g,
							node_ids: g.node_ids.filter((nodeId) => !selected.has(nodeId)),
						}
					: g
			)
			.filter((g) => g.node_ids.length > 0);
		if (!this.draftGroups.some((g) => g.id === id)) this.selectedGroupId = null;
		return true;
	}

	removeNodeFromGroup(groupId: string, nodeId: string): boolean {
		if (!this.draftGroups) return false;
		const group = this.draftGroups.find((g) => g.id === groupId);
		if (!group || !group.node_ids.includes(nodeId)) return false;
		this.#pushHistory("Remove node from group");
		this.draftGroups = this.draftGroups
			.map((g) => (g.id === groupId ? { ...g, node_ids: g.node_ids.filter((id) => id !== nodeId) } : g))
			.filter((g) => g.node_ids.length > 0);
		if (!this.draftGroups.some((g) => g.id === groupId)) this.selectedGroupId = null;
		return true;
	}

	updateGroupLabel(id: string, label: string): boolean {
		if (!this.draftGroups) return false;
		const nextLabel = label.trim();
		const index = this.draftGroups.findIndex((g) => g.id === id);
		if (index < 0) return false;
		if ((this.draftGroups[index].label ?? "") === nextLabel) return false;
		this.#pushHistory("Edit group label");
		this.draftGroups = this.draftGroups.map((g) => (g.id === id ? { ...g, label: nextLabel || null } : g));
		return true;
	}

	updateGroupColor(id: string, color: string): boolean {
		if (!this.draftGroups) return false;
		const nextColor = color.trim();
		const index = this.draftGroups.findIndex((g) => g.id === id);
		if (index < 0) return false;
		if ((this.draftGroups[index].color ?? "") === nextColor) return false;
		this.#pushHistory("Edit group color");
		this.draftGroups = this.draftGroups.map((g) => (g.id === id ? { ...g, color: nextColor || null } : g));
		return true;
	}

	selectGroupNodes(id: string): boolean {
		if (!this.draftGroups || !this.draftNodes) return false;
		const group = this.draftGroups.find((g) => g.id === id);
		if (!group) return false;
		const existing = new Set([...this.draftNodes, ...(this.draftInstances ?? [])].map((n) => n.id));
		const ids = group.node_ids.filter((nodeId) => existing.has(nodeId));
		if (ids.length === 0) return false;
		this.selectedGroupId = id;
		this.selectedNodeIds = ids;
		this.selectedNodeId = ids[0] ?? null;
		return true;
	}

	groupLabelsForNode(nodeId: string): string[] {
		return this.groupBadgesForNode(nodeId).map((g) => g.label);
	}

	groupBadgesForNode(nodeId: string): Array<{ id: string; label: string; color: string | null }> {
		return (this.draftGroups ?? [])
			.filter((g) => g.node_ids.includes(nodeId))
			.map((g) => ({ id: g.id, label: g.label || g.id, color: g.color }));
	}

	provenanceForGroup(groupId: string): Record<string, unknown> | null {
		const group = this.draftGroups?.find((g) => g.id === groupId);
		if (!group) return null;
		const nodes = this.draftNodes ?? [];
		for (const nodeId of group.node_ids) {
			const node = nodes.find((n) => n.id === nodeId);
			const value = node?.properties.value;
			const provenance = asRecord(value)?.unaa_z_provenance;
			if (isPlainRecord(provenance)) return provenance;
		}
		return null;
	}

	#makeUniqueGroupId(existing: FlowgraphDraftGroup[]): string {
		const used = new Set(existing.map((g) => g.id));
		let i = existing.length + 1;
		let id = `group_${i}`;
		while (used.has(id)) {
			i += 1;
			id = `group_${i}`;
		}
		return id;
	}

	#defaultPropertiesForSpec(spec: FlowgraphNodeSpec): Record<string, unknown> {
		const out: Record<string, unknown> = {};
		for (const p of spec.properties) {
			if (p.required) {
				out[p.name] = p.default;
			}
		}
		return out;
	}

	#makeUniqueNodeId(feature: string, existing: Array<{ id: string }>): string {
		const base = feature.replace(/^.*\./, "").replace(/[^a-zA-Z0-9_]/g, "_");
		let i = 1;
		let id = base;
		const used = new Set([...existing, ...(this.draftInstances ?? [])].map((n) => n.id));
		while (used.has(id)) {
			i += 1;
			id = `${base}_${i}`;
		}
		return id;
	}

	#pickNewNodePositionNearCentroid(nodes: FlowgraphDraftNode[], spec?: FlowgraphNodeSpec): [number, number] {
		let x = 100;
		let y = 100;
		const withPos = nodes.filter((n) => n.position);
		if (withPos.length > 0) {
			const cx = withPos.reduce((s, n) => s + n.position![0], 0) / withPos.length;
			const cy = withPos.reduce((s, n) => s + n.position![1], 0) / withPos.length;
			x = Math.round(cx + 160);
			y = Math.round(cy + 110);

			const portRows = Math.max(spec?.inputs.length ?? 0, spec?.outputs.length ?? 0);
			const candidate = { width: 300, height: Math.max(110, 78 + portRows * 15) };
			const positions = Object.fromEntries(withPos.map((node) => [node.id, node.position!])) as Record<string, [number, number]>;
			return findOpenFlowgraphPosition(this.layoutNodesForCurrentDraft(), positions, candidate, [x, y]);
		}
		return [x, y];
	}

	removeNode(id: string): void {
		if (!this.draftNodes) return;
		this.#pushHistory("Remove node");
		this.draftNodes = this.draftNodes.filter((n) => n.id !== id);
		if (this.draftEdges) {
			this.draftEdges = this.draftEdges.filter((e) => !edgeMentionsNode(e, id));
		}
		if (this.draftGroups) {
			this.draftGroups = pruneGroupsAfterNodeRemoval(this.draftGroups, new Set([id]));
		}
		if (this.selectedGroupId && !this.draftGroups?.some((g) => g.id === this.selectedGroupId)) {
			this.selectedGroupId = null;
		}
		if (this.selectedNodeId === id) this.selectedNodeId = null;
		this.selectedNodeIds = this.selectedNodeIds.filter((selected) => selected !== id);
	}

	/**
	 * γ-4a.0: Undo 可能な「まとめ削除」。複数ノード / エッジを 1 回で消して、
	 * 直前の snapshot をスタック（1 段）に保持する。UI は toast action で `undoLastDelete()` を呼べる。
	 */
	removeSelection(
		nodeIds: string[],
		edgePairs: Array<{ from: string; to: string }>
	): {
		removedNodes: number;
		removedEdges: number;
		undoDelete?: () => boolean;
	} {
		if (!this.draftNodes || !this.draftEdges) return { removedNodes: 0, removedEdges: 0 };
		const nodeSet = new Set(nodeIds);
		const edgeKeys = new Set(edgePairs.map((e) => `${e.from}||${e.to}`));

		const removedNodes = this.draftNodes.filter((n) => nodeSet.has(n.id));
		const removedInstances = (this.draftInstances ?? []).filter((instance) => nodeSet.has(instance.id));
		// ノード消滅で巻き添えになるエッジも含める。
		const removedEdges = this.draftEdges.filter((e) => edgeKeys.has(`${e.from}||${e.to}`) || edgeMentionsAny(e, nodeSet));
		if (removedNodes.length === 0 && removedInstances.length === 0 && removedEdges.length === 0) {
			return { removedNodes: 0, removedEdges: 0 };
		}
		this.#pushHistory("Delete selection");

		this.#lastDeletion = this.undoStack[this.undoStack.length - 1];
		const deletion = this.#lastDeletion;

		this.draftNodes = this.draftNodes.filter((n) => !nodeSet.has(n.id));
		if (this.draftInstances) this.draftInstances = this.draftInstances.filter((instance) => !nodeSet.has(instance.id));
		this.draftEdges = this.draftEdges.filter((e) => !edgeKeys.has(`${e.from}||${e.to}`) && !edgeMentionsAny(e, nodeSet));
		if (this.draftGroups) {
			this.draftGroups = pruneGroupsAfterNodeRemoval(this.draftGroups, nodeSet);
		}
		if (this.selectedGroupId && !this.draftGroups?.some((g) => g.id === this.selectedGroupId)) {
			this.selectedGroupId = null;
		}
		if (this.selectedNodeId && nodeSet.has(this.selectedNodeId)) this.selectedNodeId = null;
		this.selectedNodeIds = this.selectedNodeIds.filter((id) => !nodeSet.has(id));

		return {
			removedNodes: removedNodes.length + removedInstances.length,
			removedEdges: removedEdges.length,
			undoDelete: () => this.undoLastDelete(deletion),
		};
	}

	/** γ-4a.0: 直前の `removeSelection` を取り消す。成功すれば true。 */
	undoLastDelete(expected = this.#lastDeletion): boolean {
		// A stale toast must not undo edits made after the deletion.
		if (!expected || this.#lastDeletion !== expected || this.undoStack[this.undoStack.length - 1] !== expected) return false;
		return this.undo();
	}

	/** γ-4a.0: 直前の削除があるか（UI の undo ボタン表示判定に使う）。 */
	hasPendingUndo(): boolean {
		return this.#lastDeletion !== null;
	}

	canUndo(): boolean {
		return this.undoStack.length > 0;
	}

	canRedo(): boolean {
		return this.redoStack.length > 0;
	}

	undo(): boolean {
		if (this.undoStack.length === 0) return false;
		const current = this.#snapshot("Redo point");
		const entry = this.undoStack[this.undoStack.length - 1];
		this.undoStack = this.undoStack.slice(0, -1);
		this.redoStack = [...this.redoStack, current].slice(-50);
		this.#restoreSnapshot(entry);
		return true;
	}

	redo(): boolean {
		if (this.redoStack.length === 0) return false;
		const current = this.#snapshot("Undo point");
		const entry = this.redoStack[this.redoStack.length - 1];
		this.redoStack = this.redoStack.slice(0, -1);
		this.undoStack = [...this.undoStack, current].slice(-50);
		this.#restoreSnapshot(entry);
		return true;
	}

	/**
	 * γ-4a: 現在の draft と、サーバから返ってきた parsed とで差分があるか。
	 * 初回ロード直後は false、ノード追加/削除/移動/プロパティ変更で true に倒す。
	 * 「何が変わっているか」を JSON 比較で粗く判定する実装。position は整数化してから比較し、
	 * サブピクセル単位のドラッグで dirty になるのを避ける。
	 */
	get isDirty(): boolean {
		if (this.invalidPropertyJsonDrafts.length) return true;
		if (!this.currentFile?.parsed || !this.draftNodes || !this.draftEdges) return false;
		const base = this.currentFile.parsed;
		if (base.nodes.length !== this.draftNodes.length) return true;
		if (base.edges.length !== this.draftEdges.length) return true;
		const normalizeNode = (n: {
			id: string;
			feature: string;
			position?: [number, number] | null;
			properties: Record<string, unknown>;
		}) => ({
			id: n.id,
			feature: n.feature,
			position: n.position ? [Math.round(n.position[0]), Math.round(n.position[1])] : null,
			properties: n.properties,
		});
		const baseNodes = JSON.stringify(base.nodes.map(normalizeNode));
		const draftNodes = JSON.stringify(this.draftNodes.map(normalizeNode));
		if (baseNodes !== draftNodes) return true;
		const baseEdges = JSON.stringify(base.edges.map((e) => ({ from: e.from, to: e.to })));
		const draftEdges = JSON.stringify(this.draftEdges.map((e) => ({ from: e.from, to: e.to })));
		if (baseEdges !== draftEdges) return true;
		const baseGroups = JSON.stringify((base.groups ?? []).map(normalizeGroup));
		const draftGroups = JSON.stringify((this.draftGroups ?? []).map(normalizeGroup));
		if (baseGroups !== draftGroups) return true;
		const normalizeInstance = (instance: FlowgraphGraphInstance) => ({
			id: instance.id,
			graph: instance.graph,
			position: instance.position ? [Math.round(instance.position[0]), Math.round(instance.position[1])] : null,
		});
		return (
			JSON.stringify((base.instances ?? []).map(normalizeInstance)) !==
			JSON.stringify((this.draftInstances ?? []).map(normalizeInstance))
		);
	}

	#lastDeletion: FlowgraphHistoryEntry | null = null;

	#pushHistory(label: string): void {
		this.#lastDeletion = null;
		if (!this.draftNodes || !this.draftEdges) return;
		this.undoStack = [...this.undoStack, this.#snapshot(label)].slice(-50);
		this.redoStack = [];
	}

	#snapshot(label: string): FlowgraphHistoryEntry {
		return {
			label,
			nodes: cloneNodes(this.draftNodes ?? []),
			edges: cloneEdges(this.draftEdges ?? []),
			groups: cloneGroups(this.draftGroups ?? []),
			instances: cloneInstances(this.draftInstances ?? []),
			selectedNodeIds: [...this.selectedNodeIds],
			selectedNodeId: this.selectedNodeId,
			selectedGroupId: this.selectedGroupId,
		};
	}

	#restoreSnapshot(entry: FlowgraphHistoryEntry): void {
		this.draftNodes = cloneNodes(entry.nodes);
		this.draftEdges = cloneEdges(entry.edges);
		this.draftGroups = cloneGroups(entry.groups);
		this.draftInstances = cloneInstances(entry.instances);
		this.selectedNodeIds = [...entry.selectedNodeIds];
		this.selectedNodeId = entry.selectedNodeId;
		this.selectedGroupId = entry.selectedGroupId;
		this.#lastDeletion = null;
	}

	addEdge(from: string, to: string): void {
		if (!this.draftEdges) this.draftEdges = [];
		if (this.draftEdges.some((e) => e.from === from && e.to === to)) return;
		this.#pushHistory("Connect edge");
		this.draftEdges = [...this.draftEdges, { from, to }];
	}

	removeEdge(from: string, to: string): void {
		if (!this.draftEdges) return;
		this.#pushHistory("Remove edge");
		this.draftEdges = this.draftEdges.filter((e) => !(e.from === from && e.to === to));
	}

	/** 現在の draft を TOML に書き戻してディスクへ保存する。 */
	async saveCurrent(): Promise<boolean> {
		if (this.rejectInvalidPropertyJson()) return false;
		if (this.mutating || this.currentState !== "ok" || !this.currentFq || !this.draftNodes || !this.draftEdges) return false;
		const expectedContent = this.currentFile?.raw_toml;
		const expectedRoot = this.currentFile?.root_dir;
		if (typeof expectedContent !== "string") return false;
		const fq = this.currentFq;
		const request = this.currentFileRequest;
		const rawBeforeSave = this.draftToml;
		let nextToml: string;
		try {
		nextToml = serializeFlowgraph({
			meta: this.currentFile?.parsed?.meta ?? null,
			graph: this.currentFile?.parsed?.graph,
			package: this.currentFile?.parsed?.package ?? null,
			nodes: this.draftNodes,
			edges: this.draftEdges,
			enums: this.currentFile?.parsed?.enums ?? [],
			types: this.currentFile?.parsed?.types ?? [],
			groups: this.draftGroups ?? [],
			instances: this.draftInstances ?? [],
		});
		} catch (error) {
			toastStore.error("保存できないプロパティがあります", describeError(error));
			return false;
		}

		this.mutating = true;
		try {
			const resp = await api.flowgraphPutFile(fq, {
				content: nextToml,
				expected_content: expectedContent,
				expected_root: expectedRoot,
			});
			if (resp.restart_required) toastStore.warn("保存済み・再起動が必要です", "Web入力の変更は、このVACの再起動後に反映されます。");
			if (resp.ok && !resp.restart_required) {
				toastStore.success("Flowgraph を保存・適用しました", `fq=${resp.fq}`);
			} else if (!resp.ok) {
				toastStore.warn("保存済み・適用時にエラーがあります", `${resp.diagnostics.length} 件の診断`);
			}
			// 保存済みの比較基準だけ更新する。保存中の追加入力・選択・Undoは維持する。
			if (this.currentFq === fq && this.currentFileRequest === request) {
				try {
					const saved = await api.flowgraphFile(fq);
					if (this.currentFq === fq && this.currentFileRequest === request) {
						if (saved.raw_toml === nextToml && saved.root_dir === expectedRoot) {
							this.currentFile = saved;
							if (this.draftToml === rawBeforeSave) this.draftToml = saved.raw_toml;
						} else {
							toastStore.warn("保存後に別の変更を検出しました", "編集中の内容は保持しています。保存済みの内容を再確認してください。");
						}
					}
				} catch (e) {
					toastStore.warn("保存済み・再取得に失敗しました", `${fq}: ${describeError(e)}。編集中の内容は保持しています。`);
				}
			}
			await this.refreshTree();
			await this.refreshDiagnostics();
			return true;
		} catch (e) {
			toastStore.error("保存失敗", describeError(e));
			return false;
		} finally {
			this.mutating = false;
		}
	}

	async saveRawToml(raw: string): Promise<boolean> {
		if (this.rejectInvalidPropertyJson()) return false;
		if (this.mutating || this.currentState !== "ok" || !this.currentFq || !this.currentFile) return false;
		const fq = this.currentFq;
		const request = this.currentFileRequest;
		const expectedContent = this.currentFile.raw_toml;
		const expectedRoot = this.currentFile.root_dir;
		const draftSnapshot = () => JSON.stringify([this.draftToml, this.draftNodes, this.draftEdges, this.draftGroups, this.draftInstances]);
		const before = draftSnapshot();
		this.mutating = true;
		try {
			const resp = await api.flowgraphPutFile(fq, { content: raw, expected_content: expectedContent, expected_root: expectedRoot });
			if (resp.restart_required) toastStore.warn("保存済み・再起動が必要です", "Web入力の変更は、このVACの再起動後に反映されます。");
			if (resp.ok && !resp.restart_required) toastStore.success("Flowgraph を保存・適用しました", `fq=${resp.fq}`);
			else if (!resp.ok) toastStore.warn("保存済み・適用時にエラーがあります", `${resp.diagnostics.length} 件の診断`);
			if (this.currentFq === fq && this.currentFileRequest === request) {
				try {
					const saved = await api.flowgraphFile(fq);
					if (this.currentFq === fq && this.currentFileRequest === request) {
						if (saved.root_dir !== expectedRoot || saved.raw_toml !== raw) {
							toastStore.warn("保存後に別の変更を検出しました", "編集中の内容は保持しています。");
						} else {
							const unchanged = draftSnapshot() === before;
							this.currentFile = saved;
							if (unchanged) {
								this.draftToml = saved.raw_toml;
								this.draftNodes = saved.parsed?.nodes.map(n => ({ ...n, position: n.position ?? null, properties: { ...n.properties } })) ?? null;
								this.draftEdges = saved.parsed?.edges.map(e => ({ ...e })) ?? null;
								this.draftGroups = saved.parsed ? (saved.parsed.groups ?? []).map(g => ({ ...g, label: g.label ?? null, color: g.color ?? null, node_ids: [...g.node_ids] })) : null;
								this.draftInstances = saved.parsed ? (saved.parsed.instances ?? []).map(i => ({ ...i, position: i.position ? [i.position[0], i.position[1]] : null })) : null;
								this.selectedNodeId = null;
								this.selectedNodeIds = [];
								this.selectedGroupId = null;
								this.undoStack = [];
								this.redoStack = [];
							}
						}
					}
				} catch (e) {
					toastStore.warn("保存済み・再取得に失敗しました", `${fq}: ${describeError(e)}。編集中の内容は保持しています。`);
				}
			}
			await this.refreshTree();
			await this.refreshDiagnostics();
			return true;
		} catch (e) {
			toastStore.error("保存失敗", describeError(e));
			return false;
		} finally {
			this.mutating = false;
		}
	}

	// -------------------------------------------------------------------------
	// File CRUD
	// -------------------------------------------------------------------------

	async createFile(fq: string, initialToml?: string): Promise<boolean> {
		if (this.mutating) return false;
		this.mutating = true;
		try {
			const response = await api.flowgraphCreateFile({ fq, initial_toml: initialToml });
			if (response.restart_required) toastStore.warn("作成済み・再起動が必要です", "Web入力の変更は、このVACの再起動後に反映されます。");
			if (!response.ok) toastStore.warn("作成済み・適用に問題があります", "診断を確認してください。");
			else if (!response.restart_required) toastStore.success("ファイルを作成・適用しました", `fq=${fq}`);
			await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
		} catch (e) {
			toastStore.error("ファイル作成失敗", describeError(e));
			return false;
		} finally {
			this.mutating = false;
		}
		await this.openFile(fq);
		return true;
	}

	async deleteFile(fq: string, expectedContent = this.currentFq === fq ? this.currentFile?.raw_toml : undefined, expectedRoot = this.currentFq === fq ? this.currentFile?.root_dir : this.tree?.root_dir): Promise<Awaited<ReturnType<typeof api.flowgraphDeleteFile>> | null> {
		const editorRequest = this.currentFileRequest;
		if (!expectedRoot) {
			toastStore.warn("削除対象を再確認してください", "保存先を確認できません。ファイル一覧を更新し、削除内容を再確認してください。");
			return null;
		}
		if (this.mutating || this.isDirty || this.hasDraftForFile(fq, expectedRoot)) {
			toastStore.warn("ファイルを削除できません", "変更処理が終わり、編集中の内容を保存してから削除してください。");
			return null;
		}
		if (typeof expectedContent !== "string") {
			toastStore.warn("削除対象を再確認してください", "確認時の内容を取得できません。ファイルを開くか、削除内容を再確認してください。");
			return null;
		}
		this.mutating = true;
		try {
			const resp = await api.flowgraphDeleteFile(fq, expectedContent, expectedRoot);
			if (resp.restart_required) toastStore.warn("削除済み・再起動が必要です", "Web入力の変更は、このVACの再起動後に反映されます。");
			if (resp.ok && !resp.restart_required) toastStore.success("ファイルを削除・適用しました", `backup=${resp.backup ?? "(none)"}`);
			else if (!resp.ok) toastStore.warn("削除済み・適用に問題があります", `バックアップ: ${resp.backup ?? "(none)"}。診断を確認してください。`);
			if (this.currentFq === fq && this.currentFile?.root_dir === expectedRoot && this.currentFileRequest === editorRequest) this.closeFile();
			await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
			return resp;
		} catch (e) {
			toastStore.error("削除失敗", describeError(e));
			return null;
		} finally {
			this.mutating = false;
		}
	}

	async openExternal(fq: string): Promise<void> {
		try {
			const resp = await api.flowgraphOpenExternal(fq);
			if (resp.spawned) toastStore.info("外部エディタで開きました", resp.command);
			else toastStore.warn("外部エディタ起動失敗", resp.command);
		} catch (e) {
			toastStore.error("外部起動失敗", describeError(e));
		}
	}

	async restoreFile(plan: FlowgraphRecoveryPlan): Promise<Awaited<ReturnType<typeof api.flowgraphRestoreFile>> | null> {
		if (this.mutating || this.isDirty || this.hasDraftForFile(plan.fq, plan.root_dir)) {
			toastStore.warn("復元できません", "変更処理を終え、対象の未保存編集を保存または退避してから復元してください。");
			return null;
		}
		const fq = this.currentFq;
		const request = this.currentFileRequest;
		// Freeze the reviewed content across the asynchronous request.
		const expected = JSON.parse(JSON.stringify(plan)) as FlowgraphRecoveryPlan;
		this.mutating = true;
		let response: Awaited<ReturnType<typeof api.flowgraphRestoreFile>>;
		try {
			response = await api.flowgraphRestoreFile(expected.fq, expected);
		} catch (error) {
			toastStore.error("復元失敗", describeError(error));
			this.mutating = false;
			return null;
		}
		try {
			if (response.restart_required) toastStore.warn("復元済み・再起動が必要です", "Web入力の変更は、このVACの再起動後に反映されます。");
			if (response.ok && !response.restart_required) toastStore.success("復元・適用しました", response.path);
			else if (!response.ok) toastStore.warn("復元済み・適用に問題があります", "診断を確認してください。");
			await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
		} catch (error) {
			toastStore.warn("復元済み・表示更新に失敗しました", describeError(error));
		} finally {
			this.mutating = false;
		}
		if (fq === expected.fq && this.currentFile?.root_dir === expected.root_dir && !this.hasDraftForFile(fq, expected.root_dir)) {
			try { await this.refreshEditorAfterReload(fq, request, expected.root_dir); }
			catch (error) { toastStore.warn("復元済み・エディター更新に失敗しました", describeError(error)); }
		}
		return response;
	}

	async reloadFromDisk(): Promise<void> {
		if (this.mutating) return;
		const fq = this.currentFq;
		const request = this.currentFileRequest;
		const editorRoot = this.currentFile?.root_dir;
		this.mutating = true;
		try {
			const resp = await api.flowgraphReload();
			if (resp.restart_required) toastStore.warn("再読込済み・再起動が必要です", "Web入力の変更は、このVACの再起動後に反映されます。");
			if (resp.ok && !resp.restart_required) toastStore.success("構成を再読込しました", `${resp.node_count} ノード`);
			else if (!resp.ok) toastStore.warn("再読込に問題があります", `診断 ${resp.diagnostics.length} 件を確認してください。`);
			await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
		} catch (e) {
			toastStore.error("構成の再読込に失敗しました", describeError(e));
			return;
		} finally {
			this.mutating = false;
		}
		await this.refreshEditorAfterReload(fq, request, editorRoot);
	}

	async reloadPreservingState(): Promise<void> {
		if (this.mutating) return;
		const fq = this.currentFq;
		const request = this.currentFileRequest;
		const editorRoot = this.currentFile?.root_dir;
		this.mutating = true;
		try {
			const resp = await api.flowgraphReloadPreservingState();
			if (!resp.ok) {
				toastStore.error("状態を復元できませんでした", [
					"稼働中の構成と編集中の入力は維持しています。",
					...resp.diagnostics.map((diagnostic) => diagnostic.message),
					resp.path,
				].join("\n"));
				return;
			}
			if (resp.restart_required) toastStore.warn("再読込済み・再起動が必要です", "Web入力の変更は、このVACの再起動後に反映されます。");
			if (resp.ok && !resp.restart_required) {
				toastStore.success(
					"Live state を保持して reload しました",
					`${resp.saved_node_count} saved / ${resp.restored_node_count ?? 0} restored -> ${resp.path}`
				);
			}
			await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
		} catch (e) {
			toastStore.error("State-preserving reload 失敗", describeError(e));
			return;
		} finally {
			this.mutating = false;
		}
		await this.refreshEditorAfterReload(fq, request, editorRoot);
	}

	async restoreProfileLocalStateSnapshot(): Promise<void> {
		if (this.mutating) return;
		const fq = this.currentFq;
		const request = this.currentFileRequest;
		const editorRoot = this.currentFile?.root_dir;
		this.mutating = true;
		try {
			const resp = await api.flowgraphRestoreProfileLocalStateSnapshot();
			if (!resp.ok) {
				toastStore.error("状態を復元できませんでした", [
					"稼働中の構成と編集中の入力は維持しています。",
					...resp.diagnostics.map((diagnostic) => diagnostic.message),
					resp.path,
				].join("\n"));
				return;
			}
			if (resp.restart_required) toastStore.warn("状態復元済み・再起動が必要です", "Web入力の変更は、このVACの再起動後に反映されます。");
			if (resp.ok && !resp.restart_required) {
				toastStore.success(
					"State snapshot を restore しました",
					`${resp.restored_node_count ?? 0} restored / ${resp.node_count} nodes <- ${resp.path}`
				);
			}
			await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
		} catch (error) {
			toastStore.error("State snapshot restore 失敗", describeError(error));
			return;
		} finally {
			this.mutating = false;
		}
		await this.refreshEditorAfterReload(fq, request, editorRoot);
	}

	private async refreshEditorAfterReload(fq: string | null, request: number, expectedRoot?: string): Promise<void> {
		if (!fq || this.currentFq !== fq || this.currentFileRequest !== request) return;
		if (expectedRoot && this.currentFile?.root_dir !== expectedRoot) return;
		if (this.isDirty) {
			toastStore.info("未保存の編集は保持しています", "再読み込みは保存済みの内容に対して実行しました。編集中の変更は保存するまで適用されません。");
			return;
		}
		await this.openFile(fq, expectedRoot);
	}

	// -------------------------------------------------------------------------
	// Fragment share / export / import (δ-7)
	// -------------------------------------------------------------------------

	/** copy API を叩いて fragment を取得する。成功時はクリップボード書き込みも試みる。 */
	async fragmentCopy(req: FragmentCopyRequest): Promise<FragmentCopyResponse | null> {
		try {
			const resp = await api.flowgraphFragmentCopy(req);
			try {
				await navigator.clipboard?.writeText(resp.fragment_toml);
				toastStore.success(
					"Fragment をクリップボードにコピーしました",
					`${resp.file_count} ファイル / dangling ${resp.dangling_count}`
				);
			} catch {
				toastStore.info("Fragment を生成しました（クリップボード書き込み不可）", `${resp.file_count} ファイル`);
			}
			return resp;
		} catch (e) {
			toastStore.error("Copy 失敗", describeError(e));
			return null;
		}
	}

	/** 現在開いているファイルの全体を fragment として copy する（簡易ヘルパ）。 */
	async fragmentCopyCurrentFile(): Promise<FragmentCopyResponse | null> {
		if (!this.currentFq) {
			toastStore.warn("ファイル未選択", "copy するファイルを開いてください");
			return null;
		}
		return this.fragmentCopy({
			scope: "file",
			targets: [{ kind: "file", fq: this.currentFq }],
			origin: this.currentFq,
		});
	}

	fragmentCopyRequestForSelection(fallbackToCurrentFile = true): FragmentCopyRequest | null {
		if (!this.currentFq) return null;
		const nodeIds = this.#selectedNodeIdsForFragment();
		if (nodeIds.length > 0) {
			return {
				scope: "nodes",
				targets: nodeIds.map((nodeId) => ({
					kind: "node",
					fq_file: this.currentFq!,
					node_id: nodeId,
				})),
				origin: this.currentFq,
			};
		}
		if (!fallbackToCurrentFile) return null;
		return {
			scope: "file",
			targets: [{ kind: "file", fq: this.currentFq }],
			origin: this.currentFq,
		};
	}

	fragmentSelectionSummary(): string {
		const ids = this.#selectedNodeIdsForFragment();
		if (ids.length > 0) return `${ids.length}ノード`;
		return this.currentFq ? "現在のファイル" : "対象なし";
	}

	/** 選択ノード集合または選択 group を fragment 化する。未選択なら現在ファイル全体。 */
	async fragmentCopySelectedNodesOrCurrentFile(): Promise<FragmentCopyResponse | null> {
		const req = this.fragmentCopyRequestForSelection(true);
		if (!req) {
			toastStore.warn("ファイル未選択", "copy するファイルを開いてください");
			return null;
		}
		return this.fragmentCopy(req);
	}

	/** 選択ノード 1 個または複数選択/group を fragment 化する。 */
	async fragmentCopySelectedNode(): Promise<FragmentCopyResponse | null> {
		const req = this.fragmentCopyRequestForSelection(false);
		if (!req) {
			toastStore.warn("ノード未選択", "copy するノードを選択してください");
			return null;
		}
		return this.fragmentCopy(req);
	}

	async fragmentPaste(req: FragmentPasteRequest): Promise<FragmentPasteResponse | null> {
		if (this.mutating || this.isDirty) {
			toastStore.warn("貼り付けできません", "変更処理が終わり、編集中の内容を保存してから貼り付けてください。");
			return null;
		}
		const fq = this.currentFq;
		const request = this.currentFileRequest;
		const editorRoot = this.currentFile?.root_dir;
		let response: FragmentPasteResponse;
		const operationRoot = this.tree?.root_dir ?? "保存先を確認できません";
		this.mutating = true;
		try {
			const resp = await api.flowgraphFragmentPaste(req);
			response = resp;
			const r = resp.report;
			const unresolved = r.unresolved_danglings.length;
			const skipped = r.skipped_nodes.length + r.skipped_files.length;
			if (resp.restart_required) toastStore.warn("貼り付け済み・再起動が必要です", "Web入力の変更は、このVACの再起動後に反映されます。");
			if (resp.reload_ok && unresolved === 0 && skipped === 0 && !resp.restart_required) {
				toastStore.success("Paste しました", `${r.imported_nodes.length} ノード / ${r.written_files.length} ファイル`);
			} else if (!resp.reload_ok || unresolved > 0 || skipped > 0) {
				toastStore.warn(
					"Paste 完了（要注意）",
					`imported=${r.imported_nodes.length} skipped=${skipped} unresolved=${unresolved} diag=${resp.diagnostics_count}`
				);
			}
			await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
		} catch (e) {
			if ((e as { body?: { error?: unknown } } | null)?.body?.error === "partial_write") {
				this.partialWriteFailure = { operation: "貼り付け", root: operationRoot, detail: describeError(e) };
				// Refresh observations only; never reload the runtime or discard the editor here.
				await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
			}
			toastStore.error("Paste 失敗", describeError(e));
			return null;
		} finally {
			this.mutating = false;
		}
		try { await this.refreshEditorAfterReload(fq, request, editorRoot); }
		catch (error) { toastStore.warn("貼り付け済み・エディター更新に失敗しました", describeError(error)); }
		return response;
	}

	/** ZIP export を実行して Blob を返す（caller が download 処理する）。 */
	async exportZip(req: ZipExportRequest): Promise<Blob | null> {
		const expectedRoot = req.expected_root ?? this.tree?.root_dir;
		if (!expectedRoot) {
			toastStore.warn("書出し対象を再確認してください", "保存先を確認できません。ファイル一覧を更新してください。");
			return null;
		}
		try {
			const blob = await api.flowgraphExportZip({ ...req, expected_root: expectedRoot });
			toastStore.success("ZIP を生成しました", `${(blob.size / 1024).toFixed(1)} KiB`);
			return blob;
		} catch (e) {
			toastStore.error("Export ZIP 失敗", describeError(e));
			return null;
		}
	}

	async importZip(
		zip: Blob,
		params: {
			dry_run: boolean;
			expected_root?: string;
			target_prefix?: string;
			on_conflict_node?: string;
			on_conflict_file?: string;
		}
	): Promise<ZipImportOutcome | null> {
		if (this.mutating || (!params.dry_run && this.isDirty)) {
			toastStore.warn("構成を追加できません", "変更処理が終わり、編集中の内容を保存してから追加してください。");
			return null;
		}
		const operationRoot = params.expected_root ?? this.tree?.root_dir;
		if (!operationRoot) { toastStore.warn("取込先を再確認してください", "保存先を確認できません。ファイル一覧を更新してください。"); return null; }
		const atDestination = (detail: string) => `追加先: ${operationRoot}\n${detail}`;
		this.mutating = true;
		try {
			const outcome = await api.flowgraphImportZip(zip, { ...params, expected_root: operationRoot });
			if (outcome.kind === "preview") {
				toastStore.info("追加内容の確認", atDestination(`${outcome.file_count} ファイル / 競合 ${outcome.conflicts.length} 件`));
			} else {
				const unresolved = outcome.danglings_unresolved.length;
				if (outcome.reload_ok === false) {
					toastStore.error("保存した構成を読み込めません", atDestination("診断で問題を確認してください。ファイルの追加は保存されています。"));
				} else if (outcome.restart_required) {
					toastStore.warn("保存済み・再起動が必要です", atDestination("Web入力の変更を反映するには、この保存先を使うVACの再起動が必要です。未接続の箇所は診断で確認できます。"));
				} else if (unresolved > 0) {
					toastStore.warn("追加した構成に未接続の箇所があります", atDestination(`${unresolved} 件。接続先を確認してください。`));
				} else if (outcome.reload_ok === true) {
					toastStore.success("構成を追加しました", atDestination(`${outcome.written_files.length} ファイルを保存し、読み込みました。`));
				} else {
					toastStore.info("構成を保存しました", atDestination("読み込み結果は未確認です。診断で状態を確認してください。"));
				}
				await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
			}
			return outcome;
		} catch (e) {
			if ((e as { body?: { error?: unknown } } | null)?.body?.error === "partial_write") {
				this.partialWriteFailure = { operation: "ZIP取込", root: operationRoot, detail: describeError(e) };
				// Refresh observations only; never reload the runtime or discard the editor here.
				await Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
			}
			toastStore.error("ZIP取込の完了を確認できません", atDestination(describeError(e)));
			return null;
		} finally {
			this.mutating = false;
		}
	}

	#selectedNodeIdsForFragment(): string[] {
		const existing = new Set((this.draftNodes ?? []).map((n) => n.id));
		const rawIds =
			this.selectedNodeIds.length > 0
				? this.selectedNodeIds
				: this.selectedGroupId
					? ((this.draftGroups ?? []).find((g) => g.id === this.selectedGroupId)?.node_ids ?? [])
					: this.selectedNodeId
						? [this.selectedNodeId]
						: [];
		return [...new Set(rawIds)].filter((id) => existing.size === 0 || existing.has(id));
	}

	// -------------------------------------------------------------------------
	// Feature lookup helpers
	// -------------------------------------------------------------------------

	findSpec(feature: string): FlowgraphNodeSpec | undefined {
		return this.catalog?.specs.find((s) => s.feature === feature);
	}

	groupedCatalog(): Array<{ category: string; specs: FlowgraphNodeSpec[] }> {
		// 一時バッファは普通のオブジェクトで済む（reactive state ではないので SvelteMap 不要）。
		const groups: Record<string, FlowgraphNodeSpec[]> = Object.create(null);
		for (const spec of this.catalog?.specs ?? []) {
			const key = spec.category || "misc";
			if (!groups[key]) groups[key] = [];
			groups[key].push(spec);
		}
		const userEnumSpecs = this.#userEnumPaletteSpecs();
		if (userEnumSpecs.length > 0) {
			groups.user_defined = userEnumSpecs;
		}
		return Object.entries(groups)
			.map(([category, specs]) => ({
				category,
				specs: specs.slice().sort((a, b) => a.title.localeCompare(b.title)),
			}))
			.sort((a, b) => a.category.localeCompare(b.category));
	}

	/** Phase λ: `[[enums]]` を String Literal プリセットとしてパレットに載せる。 */
	#userEnumPaletteSpecs(): FlowgraphNodeSpec[] {
		const parsed = this.currentFile?.parsed;
		if (!parsed?.enums?.length) return [];
		const base = this.catalog?.specs.find((s) => s.feature === "flowgraph.literal.string");
		if (!base) return [];
		const out: FlowgraphNodeSpec[] = [];
		for (const en of parsed.enums) {
			for (const v of en.variants) {
				const props = base.properties.map((p) =>
					p.name === "value"
						? {
								...p,
								required: true,
								default: v,
								choices: en.variants.length > 0 ? [...en.variants] : p.choices,
							}
						: { ...p }
				);
				out.push({
					...base,
					palette_key: `user_enum:${en.id}:${v}`,
					title: `${en.id}: ${v}`,
					category: "user_defined",
					description: `ユーザ定義 enum「${en.id}」の値 "${v}"（String Literal プリセット）`,
					properties: props,
				});
			}
		}
		return out;
	}

	// -------------------------------------------------------------------------
	// WS subscription
	// -------------------------------------------------------------------------

	/** 一度だけ呼んで WS 購読を張る。`FlowgraphTab` がマウント時に呼ぶ。 */
	attachWsSubscriber(): void {
		if (this.#wsUnsubscribe) return;
		this.#wsUnsubscribe = eventsStore.subscribe((ts) => {
			if (ts.event.kind !== "flowgraph_reloaded") return;
			const ev = ts.event;
			if (ev.ok) {
				toastStore.info("構成を再読込しました", `${ev.node_count} ノード`);
			} else {
				toastStore.warn("再読込に問題があります", `エラー ${ev.error_count} 件・警告 ${ev.warning_count} 件。診断を確認してください。`);
			}
			// diagnostics と tree は必ず引き直す。開いているファイルがあればそれも再取得する。
			void Promise.all([this.refreshTree(), this.refreshDiagnostics()]);
			if (this.currentFq && !this.isDirty && !this.mutating) void this.openFile(this.currentFq);
		});
	}

	detachWsSubscriber(): void {
		if (this.#wsUnsubscribe) {
			this.#wsUnsubscribe();
			this.#wsUnsubscribe = null;
		}
	}
}

// ---------------------------------------------------------------------------
// Draft types & helpers
// ---------------------------------------------------------------------------

export type FlowgraphDraftNode = {
	id: string;
	feature: string;
	position: [number, number] | null;
	properties: Record<string, unknown>;
};

export type FlowgraphDraftEdge = {
	from: string;
	to: string;
};

export type FlowgraphDraftGroup = {
	id: string;
	label: string | null;
	node_ids: string[];
	color: string | null;
};

type FlowgraphHistoryEntry = {
	label: string;
	nodes: FlowgraphDraftNode[];
	edges: FlowgraphDraftEdge[];
	groups: FlowgraphDraftGroup[];
	instances: FlowgraphGraphInstance[];
	selectedNodeIds: string[];
	selectedNodeId: string | null;
	selectedGroupId: string | null;
};

function cloneNodes(nodes: FlowgraphDraftNode[]): FlowgraphDraftNode[] {
	return nodes.map((n) => ({
		id: n.id,
		feature: n.feature,
		position: n.position ? [n.position[0], n.position[1]] : null,
		properties: { ...n.properties },
	}));
}

function cloneEdges(edges: FlowgraphDraftEdge[]): FlowgraphDraftEdge[] {
	return edges.map((e) => ({ from: e.from, to: e.to }));
}

function cloneGroups(groups: FlowgraphDraftGroup[]): FlowgraphDraftGroup[] {
	return groups.map((g) => ({
		id: g.id,
		label: g.label,
		node_ids: [...g.node_ids],
		color: g.color,
	}));
}

function cloneInstances(instances: FlowgraphGraphInstance[]): FlowgraphGraphInstance[] {
	return instances.map((instance) => ({
		...instance,
		position: instance.position ? [instance.position[0], instance.position[1]] : null,
	}));
}

function normalizeGroup(g: FlowgraphGroupDef | FlowgraphDraftGroup): FlowgraphDraftGroup {
	return {
		id: g.id,
		label: g.label ?? null,
		node_ids: [...g.node_ids],
		color: g.color ?? null,
	};
}

function pruneGroupsAfterNodeRemoval(groups: FlowgraphDraftGroup[], removedNodeIds: Set<string>): FlowgraphDraftGroup[] {
	return groups
		.map((g) => ({
			...g,
			node_ids: g.node_ids.filter((id) => !removedNodeIds.has(id)),
		}))
		.filter((g) => g.node_ids.length > 0);
}

function edgeMentionsNode(edge: FlowgraphDraftEdge, nodeId: string): boolean {
	return parsePortRef(edge.from).nodeId === nodeId || parsePortRef(edge.to).nodeId === nodeId;
}

/** γ-4a.0: 複数ノード版。`removeSelection` の巻き添えエッジ計算に使う。 */
function edgeMentionsAny(edge: FlowgraphDraftEdge, nodeIds: Set<string>): boolean {
	return nodeIds.has(parsePortRef(edge.from).nodeId) || nodeIds.has(parsePortRef(edge.to).nodeId);
}

/**
 * `"node:port"` / `"path::node:port"` を decomposition。spec §7 の parse_port_ref を GUI 側で簡易実装。
 * 完全な再現は不要で、ノード ID の抽出だけに使う。
 */
export function parsePortRef(ref: string): {
	path: string;
	nodeId: string;
	port: string;
} {
	const lastColon = findLastSinglePortColon(ref);
	if (lastColon < 0) return { path: "", nodeId: ref, port: "" };
	const head = ref.slice(0, lastColon);
	const port = ref.slice(lastColon + 1);
	const pathSepIdx = head.lastIndexOf("::");
	if (pathSepIdx < 0) return { path: "", nodeId: head, port };
	const path = head.slice(0, pathSepIdx);
	const nodeId = head.slice(pathSepIdx + 2);
	return { path, nodeId, port };
}

/** `::` 以外の単独 `:` の位置を末尾から探す。 */
function findLastSinglePortColon(s: string): number {
	for (let i = s.length - 1; i >= 0; i--) {
		if (s[i] !== ":") continue;
		if (i > 0 && s[i - 1] === ":") {
			i -= 1;
			continue;
		}
		if (i + 1 < s.length && s[i + 1] === ":") continue;
		return i;
	}
	return -1;
}

function asRecord(value: unknown): Record<string, unknown> | null {
	return value !== null && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
	return value !== null && typeof value === "object" && !Array.isArray(value);
}

function cloneJsonObject(value: Record<string, unknown>): Record<string, unknown> {
	return JSON.parse(JSON.stringify(value)) as Record<string, unknown>;
}

function endpointCandidateProvenance(candidate: UnmfzEndpointCandidate, mode: "publish" | "subscribe"): Record<string, unknown> {
	return compactJsonRecord({
		schema_version: 1,
		source: "unaa_z_endpoint_candidate",
		mode,
		label: candidate.label,
		app_id: candidate.appId,
		instance_id: candidate.instanceId,
		app_display_name: candidate.appDisplayName,
		endpoint_id: candidate.endpointId,
		direction: candidate.direction,
		base_key_expr: candidate.baseKeyExpr,
		topic_mode: candidate.topicMode,
		schema_major: candidate.schemaMajor,
		discovery_source: candidate.discoverySource,
		discovery_source_key: candidate.sourceKey,
		discovery_payload_kind: candidate.payloadKind,
		discovery_stale: candidate.discoveryStale,
		discovery_age_ms: candidate.discoveryAgeMs,
	});
}

function endpointCandidateGroupLabel(candidate: UnmfzEndpointCandidate, mode: "publish" | "subscribe"): string {
	const app = candidate.appDisplayName || candidate.appId || "unknown app";
	const via = candidate.payloadKind || candidate.discoverySource || "metadata";
	const stale = candidate.discoveryStale ? " stale" : "";
	return `UNMF/Z ${mode}: ${app} / ${candidate.endpointId} via ${via}${stale}`;
}

function compactJsonRecord(value: Record<string, unknown>): Record<string, unknown> {
	return Object.fromEntries(Object.entries(value).filter(([, v]) => v !== undefined && v !== null && v !== ""));
}

/**
 * draft の nodes / edges を `.flowgraph.toml` 形式に素朴にシリアライズする。
 *
 * 完全な format-preserving edit は行わず、nodes の位置・feature・id・properties と edges の from/to を
 * 機械的に TOML で書き出す。元ファイルの meta はそのまま保持する（title / description / tags）。
 *
 * この方針により、`toml_edit` を JS 側に持ち込まずに済む。コメントなどは失われるが、
 * δ-6e では GUI 編集中心なので許容する（外部エディタで直接書いた場合は PUT を使わず open-external で開く UX）。
 */
export function serializeFlowgraph(doc: {
	meta: FlowgraphFileMeta | null;
	graph?: FlowgraphGraphDeclaration;
	package?: FlowgraphPackageManifest | null;
	nodes: FlowgraphDraftNode[];
	edges: FlowgraphDraftEdge[];
	enums?: FlowgraphEnumDef[];
	types?: FlowgraphTypeDef[];
	groups?: FlowgraphDraftGroup[];
	instances?: FlowgraphGraphInstance[];
}): string {
	const lines: string[] = [];
	const m = doc.meta;
	const hasMeta =
		m &&
		(m.title ||
			m.description ||
			(m.tags && m.tags.length > 0) ||
			m.author ||
			m.name ||
			m.version ||
			m.license ||
			m.repos ||
			(m.mode_groups && m.mode_groups.length > 0) ||
			m.default_enabled === false);
	if (hasMeta && m) {
		lines.push("[meta]");
		if (m.title) lines.push(`title = ${tomlString(m.title)}`);
		if (m.description) lines.push(`description = ${tomlString(m.description)}`);
		if (m.tags && m.tags.length > 0) {
			lines.push(`tags = [${m.tags.map(tomlString).join(", ")}]`);
		}
		if (m.author) lines.push(`author = ${tomlString(m.author)}`);
		if (m.name) lines.push(`name = ${tomlString(m.name)}`);
		if (m.version) lines.push(`version = ${tomlString(m.version)}`);
		if (m.license) lines.push(`license = ${tomlString(m.license)}`);
		if (m.repos) lines.push(`repos = ${tomlString(m.repos)}`);
		if (m.mode_groups && m.mode_groups.length > 0) lines.push(`mode_groups = [${m.mode_groups.map(tomlString).join(", ")}]`);
		if (m.default_enabled === false) lines.push("default_enabled = false");
		lines.push("");
	}

	const graph = doc.graph;
	if (graph && (graph.kind === "module" || graph.inputs.length > 0 || graph.outputs.length > 0)) {
		lines.push("[graph]");
		lines.push(`kind = ${tomlString(graph.kind)}`);
		lines.push("");
		for (const input of graph.inputs) {
			lines.push("[[graph.inputs]]");
			lines.push(`name = ${tomlString(input.name)}`);
			lines.push(`type = ${tomlString(input.type)}`);
			lines.push(`to = ${tomlString(input.to)}`);
			lines.push("");
		}
		for (const output of graph.outputs) {
			lines.push("[[graph.outputs]]");
			lines.push(`name = ${tomlString(output.name)}`);
			lines.push(`type = ${tomlString(output.type)}`);
			lines.push(`from = ${tomlString(output.from)}`);
			lines.push("");
		}
	}

	if (doc.package) {
		lines.push("[package]");
		if (doc.package.id) lines.push(`id = ${tomlString(doc.package.id)}`);
		if (doc.package.version) lines.push(`version = ${tomlString(doc.package.version)}`);
		if (doc.package.exports.length > 0) lines.push(`exports = [${doc.package.exports.map(tomlString).join(", ")}]`);
		lines.push("");
		if (Object.keys(doc.package.dependencies).length > 0) {
			lines.push("[package.dependencies]");
			for (const [id, version] of Object.entries(doc.package.dependencies)) lines.push(`${tomlKey(id)} = ${tomlString(version)}`);
			lines.push("");
		}
	}

	for (const en of doc.enums ?? []) {
		lines.push("[[enums]]");
		lines.push(`id = ${tomlString(en.id)}`);
		if (en.primitive) lines.push(`primitive = ${tomlString(en.primitive)}`);
		lines.push(`variants = [${en.variants.map(tomlString).join(", ")}]`);
		lines.push("");
	}

	for (const ty of doc.types ?? []) {
		lines.push("[[types]]");
		lines.push(`id = ${tomlString(ty.id)}`);
		if (ty.kind) lines.push(`kind = ${tomlString(ty.kind)}`);
		if (ty.description) lines.push(`description = ${tomlString(ty.description)}`);
		if (Object.keys(ty.fields).length > 0) {
			lines.push("[types.fields]");
			for (const [name, fieldType] of Object.entries(ty.fields)) lines.push(`${tomlKey(name)} = ${tomlString(fieldType)}`);
		}
		lines.push("");
	}

	for (const group of doc.groups ?? []) {
		lines.push("[[groups]]");
		lines.push(`id = ${tomlString(group.id)}`);
		if (group.label) lines.push(`label = ${tomlString(group.label)}`);
		lines.push(`node_ids = [${group.node_ids.map(tomlString).join(", ")}]`);
		if (group.color) lines.push(`color = ${tomlString(group.color)}`);
		lines.push("");
	}

	for (const instance of doc.instances ?? []) {
		lines.push("[[instances]]");
		lines.push(`id = ${tomlString(instance.id)}`);
		lines.push(`graph = ${tomlString(instance.graph)}`);
		if (instance.position) lines.push(`position = [${formatNum(instance.position[0])}, ${formatNum(instance.position[1])}]`);
		lines.push("");
	}

	for (const node of doc.nodes) {
		lines.push("[[nodes]]");
		lines.push(`id = ${tomlString(node.id)}`);
		lines.push(`feature = ${tomlString(node.feature)}`);
		if (node.position) {
			lines.push(`position = [${formatNum(node.position[0])}, ${formatNum(node.position[1])}]`);
		}
		const propKeys = Object.keys(node.properties);
		if (propKeys.length > 0) {
			lines.push("[nodes.properties]");
			for (const k of propKeys) {
				lines.push(`${tomlKey(k)} = ${tomlValue(node.properties[k])}`);
			}
		}
		lines.push("");
	}

	for (const edge of doc.edges) {
		lines.push("[[edges]]");
		lines.push(`from = ${tomlString(edge.from)}`);
		lines.push(`to = ${tomlString(edge.to)}`);
		lines.push("");
	}

	return (
		lines
			.join("\n")
			.replace(/\n{3,}$/, "\n\n")
			.trimEnd() + "\n"
	);
}

function tomlString(s: string): string {
	// JSON入力で表現できる制御文字も、TOMLの基本文字列へ可逆に保存する。
	const escapes: Record<string, string> = { '"': '\\"', "\\": "\\\\", "\n": "\\n", "\r": "\\r", "\t": "\\t", "\b": "\\b", "\f": "\\f" };
	return `"${s.replace(/["\\\u0000-\u001f\u007f]/g, (char) => escapes[char] ?? `\\u${char.charCodeAt(0).toString(16).padStart(4, "0")}`)}"`;
}

function tomlKey(k: string): string {
	// bare key に合致しない場合は quoted key にする。
	if (/^[A-Za-z0-9_-]+$/.test(k)) return k;
	return tomlString(k);
}

function tomlValue(v: unknown): string {
	if (v === null || v === undefined) throw new Error("nullはTOMLのプロパティに保存できません。既定値のnullはUnsetで項目を省略し、JSON内のnullは文字列とJSON Parseノードで渡してください。");
	if (typeof v === "string") return tomlString(v);
	if (typeof v === "boolean") return v ? "true" : "false";
	if (typeof v === "number") {
		if (!Number.isFinite(v)) throw new Error("有限でない数値は保存できません。入力の桁数を確認してください。");
		return Number.isInteger(v) ? String(v) : formatNum(v);
	}
	if (Array.isArray(v)) {
		return `[${v.map(tomlValue).join(", ")}]`;
	}
	if (typeof v === "object") {
		// inline table。ネスト深いケースは想定しない（GUI 側プロパティ編集の浅い object 用）。
		const entries = Object.entries(v as Record<string, unknown>);
		return `{ ${entries.map(([k, vv]) => `${tomlKey(k)} = ${tomlValue(vv)}`).join(", ")} }`;
	}
	return tomlString(String(v));
}

function formatNum(n: number): string {
	if (!Number.isFinite(n)) return "0";
	// 極小値や極大値でも TOML 的に読める表現。
	if (Number.isInteger(n)) return `${n}.0`;
	return String(n);
}

// ---------------------------------------------------------------------------
// Diagnostics helpers
// ---------------------------------------------------------------------------

export function summarizeDiagnostics(diags: FlowgraphDiagnostic[] | undefined): {
	error: number;
	warning: number;
	info: number;
} {
	const out = { error: 0, warning: 0, info: 0 };
	for (const d of diags ?? []) out[d.severity] += 1;
	return out;
}

function describeError(e: unknown): string {
	if (e instanceof ControlApiError) {
		const body = typeof e.body === "object" && e.body !== null ? e.body as Record<string, unknown> : {};
		const code = typeof body.error === "string" ? body.error : e.statusText;
		const detail = typeof body.detail === "string" ? body.detail : code;
		const saved = Array.isArray(body.written_files) ? body.written_files.flatMap(file =>
			typeof file === "string" ? [file] : typeof file === "object" && file !== null && typeof file.path === "string" ? [file.path] : []) : [];
		const companions = Array.isArray(body.written_companions) ? body.written_companions.filter((path): path is string => typeof path === "string") : [];
		const backups = Array.isArray(body.backups) ? body.backups.filter((path): path is string => typeof path === "string") : [];
		return [`${detail} (${e.status} ${code})`, saved.length ? `保存済み: ${saved.join("、")}` : "", companions.length ? `保存済み素材: ${companions.join("、")}` : "", backups.length ? `バックアップ: ${backups.join("、")}` : ""].filter(Boolean).join("\n");
	}
	if (e instanceof Error) return e.message;
	return String(e);
}

export const flowgraphStore = new FlowgraphStore();

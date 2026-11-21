<script lang="ts">
	/**
	 * Phase VI-γ-2a: Live タブの BOS プレビュー / OBS 用ブラウザソース URL コピー。
	 *
	 * 機能:
	 *   - `GET /api/v1/control/bos` の結果を取得してセレクターで選ぶ
	 *   - `supports_channel_param` のエントリには `channel` 入力欄を出し、`?channel=xxx` に反映
	 *   - 現在の絶対 URL を表示し、コピーボタン（`navigator.clipboard`、失敗したら execCommand フォールバック）
	 *   - iframe に指定 URL を埋め込み（OBS での見た目をそのまま確認）
	 *   - 16:9 / 9:16 / 1:1 の代表的な OBS キャンバス比率でプレビュー
	 *   - `/obs-template/*` は外観、更新、表示時間、字幕配置も URL に反映
	 *   - よく使う OBS 演出設定をクイックプリセットで即時反映
	 *   - 「別タブで開く」ボタンで単独検証も容易
	 *
	 * 非目標（γ-2a）:
	 *   - VAC 側のイベントをリアルタイムに描画する独自字幕（既存 poll.js で十分）
	 */
	import { untrack } from "svelte";
	import { SvelteURLSearchParams } from "svelte/reactivity";
	import { api } from "./api";
	import { ControlApiError, type BosEntry, type BosResponse } from "./types";
	import { toastStore } from "./toasts.svelte";
	import { ObsWsClient } from "./widgets/obsWsClient";

	interface Props {
		/** 親 (LiveTab) が指定する既定チャンネル。カンマ区切りで複数指定できる。 */
		defaultChannel?: string;
	}
	const { defaultChannel = "user,ai-synth" }: Props = $props();

	type PreviewPreset = {
		id: "landscape" | "portrait" | "square";
		label: string;
		size: string;
		aspect: string;
		maxWidth: string;
	};

	const previewPresets: PreviewPreset[] = [
		{ id: "landscape", label: "16:9", size: "1920 x 1080", aspect: "16 / 9", maxWidth: "100%" },
		{ id: "portrait", label: "9:16", size: "1080 x 1920", aspect: "9 / 16", maxWidth: "22rem" },
		{ id: "square", label: "1:1", size: "1080 x 1080", aspect: "1 / 1", maxWidth: "28rem" },
	];

	type TemplateQuickPreset = {
		id: "subtitle" | "minimal" | "chat" | "raid" | "effects";
		label: string;
		entryId: string;
		channel: string;
		theme: "default" | "minimal";
		pollMs: number;
		limit: number;
		ttlMs: number;
		aspect: PreviewPreset["id"];
		align: "left" | "center" | "right";
		fontScale: number;
		widthPct: number;
	};

	const templateQuickPresets: TemplateQuickPreset[] = [
		{
			id: "subtitle",
			label: "字幕",
			entryId: "obs-template-subtitle",
			channel: "user",
			theme: "default",
			pollMs: 500,
			limit: 20,
			ttlMs: 30000,
			aspect: "landscape",
			align: "center",
			fontScale: 100,
			widthPct: 100,
		},
		{
			id: "minimal",
			label: "最小",
			entryId: "obs-template-subtitle",
			channel: "user",
			theme: "minimal",
			pollMs: 500,
			limit: 20,
			ttlMs: 30000,
			aspect: "landscape",
			align: "center",
			fontScale: 100,
			widthPct: 80,
		},
		{
			id: "chat",
			label: "チャット",
			entryId: "obs-template-chat",
			channel: "chat",
			theme: "minimal",
			pollMs: 300,
			limit: 12,
			ttlMs: 60000,
			aspect: "portrait",
			align: "left",
			fontScale: 80,
			widthPct: 72,
		},
		{
			id: "raid",
			label: "RAID",
			entryId: "obs-template-raid",
			channel: "effect",
			theme: "default",
			pollMs: 250,
			limit: 1,
			ttlMs: 15000,
			aspect: "landscape",
			align: "center",
			fontScale: 120,
			widthPct: 90,
		},
		{
			id: "effects",
			label: "演出",
			entryId: "obs-template-effect",
			channel: "effect,chat",
			theme: "default",
			pollMs: 100,
			limit: 20,
			ttlMs: 15000,
			aspect: "landscape",
			align: "center",
			fontScale: 100,
			widthPct: 100,
		},
	];

	let loading = $state(true);
	let error = $state<string | null>(null);
	let resp = $state<BosResponse | null>(null);
	let selectedId = $state<string | null>(null);
	// 親の defaultChannel はマウント時の初期値のみ反映（変動は想定しない）。
	// untrack で囲むことで state_referenced_locally 警告を抑制しつつ意図を明示する。
	let channel = $state(untrack(() => defaultChannel));
	let templateTheme = $state<"default" | "minimal">("default");
	let templatePollMs = $state(500);
	let templateLimit = $state(20);
	let templateTtlMs = $state(30000);
	let templateAlign = $state<"left" | "center" | "right">("center");
	let templateLabel = $state("");
	let templateFontScale = $state(100);
	let templateWidthPct = $state(100);
	type ObsInput = { inputName: string; inputKind?: string; unversionedInputKind?: string };
	type ObsScene = { sceneName: string; sceneIndex?: number };
	let obsInputs = $state<ObsInput[]>([]);
	let obsScenes = $state<ObsScene[]>([]);
	let obsInputName = $state(localStorage.getItem("vac.obs.browserSourceName") ?? "");
	let obsSceneName = $state(localStorage.getItem("vac.obs.browserSourceScene") ?? "");
	let newObsInputName = $state("unvac-output");
	let obsBaseWidth = $state(1920);
	let obsBaseHeight = $state(1080);
	let obsBusy = $state<"load" | "apply" | "create" | null>(null);

	const origin = typeof window !== "undefined" ? window.location.origin : "";

	async function load() {
		loading = true;
		error = null;
		try {
			resp = await api.bos();
			if (resp.entries.length > 0 && (selectedId == null || !resp.entries.some((e) => e.id === selectedId))) {
				// 字幕カテゴリを優先して初期選択、無ければ先頭
				const firstSub = resp.entries.find((e) => e.category === "subtitles");
				selectedId = firstSub?.id ?? resp.entries[0].id;
			}
		} catch (e) {
			error = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		void load();
	});

	const selectedEntry: BosEntry | null = $derived(resp && selectedId ? (resp.entries.find((e) => e.id === selectedId) ?? null) : null);
	const selectedIsObsTemplate = $derived(Boolean(selectedEntry?.url.startsWith("/obs-template/")));
	const selectedIsSubtitleTemplate = $derived(selectedEntry?.url === "/obs-template/subtitle");

	function clampNumber(value: number, min: number, max: number, fallback: number): number {
		if (!Number.isFinite(value)) return fallback;
		return Math.max(min, Math.min(max, Math.round(value)));
	}

	/** channel パラメータを載せたときの相対 URL（supports_channel_param かつ channel 非空なら付与）。 */
	const composedUrl = $derived.by(() => {
		if (!selectedEntry) return "";
		const base = selectedEntry.url;
		const params = new SvelteURLSearchParams();
		if (selectedEntry.supports_channel_param) {
			const ch = channel.trim();
			if (ch !== "") params.set(selectedIsObsTemplate && ch.includes(",") ? "channels" : "channel", ch);
		}
		if (selectedIsObsTemplate) {
			params.set("theme", templateTheme);
			params.set("poll_ms", String(clampNumber(templatePollMs, 100, 60000, 500)));
			params.set("limit", String(clampNumber(templateLimit, 1, 100, 20)));
			params.set("ttl_ms", String(clampNumber(templateTtlMs, 1000, 300000, 30000)));
			params.set("align", templateAlign);
			const label = templateLabel.trim();
			if (selectedIsSubtitleTemplate && label !== "") params.set("label", label.slice(0, 40));
			params.set("font_scale", String(clampNumber(templateFontScale, 50, 200, 100)));
			params.set("width_pct", String(clampNumber(templateWidthPct, 20, 100, 100)));
		}
		const qs = params.toString();
		if (qs === "") return base;
		const sep = base.includes("?") ? "&" : "?";
		return `${base}${sep}${qs}`;
	});

	const absoluteUrl = $derived(composedUrl ? `${origin}${composedUrl}` : "");

	// γ-2: 縦画面（portrait）では iframe プレビューは邪魔になりやすいので折りたたむ。
	// 既定は横画面（landscape）だと展開、縦画面だと折りたたみ。ユーザ操作が優先される。
	let previewManuallyToggled = $state(false);
	let previewOpenOverride = $state(false);
	let isPortrait = $state(false);
	let previewPresetId = $state<PreviewPreset["id"]>("landscape");
	const previewOpen = $derived(previewManuallyToggled ? previewOpenOverride : !isPortrait);
	const previewPreset = $derived(previewPresets.find((p) => p.id === previewPresetId) ?? previewPresets[0]);

	$effect(() => {
		if (typeof window === "undefined") return;
		const mq = window.matchMedia("(orientation: portrait)");
		const apply = () => (isPortrait = mq.matches);
		apply();
		mq.addEventListener("change", apply);
		return () => mq.removeEventListener("change", apply);
	});

	function togglePreview() {
		previewManuallyToggled = true;
		previewOpenOverride = !previewOpen;
	}

	function applyTemplatePreset(preset: TemplateQuickPreset) {
		if (resp?.entries.some((entry) => entry.id === preset.entryId)) selectedId = preset.entryId;
		channel = preset.channel;
		templateTheme = preset.theme;
		templatePollMs = preset.pollMs;
		templateLimit = preset.limit;
		templateTtlMs = preset.ttlMs;
		templateAlign = preset.align;
		templateFontScale = preset.fontScale;
		templateWidthPct = preset.widthPct;
		previewPresetId = preset.aspect;
	}

	function selectSource(id: string) {
		selectedId = id;
		const entry = resp?.entries.find((candidate) => candidate.id === id);
		switch (entry?.url) {
			case "/obs-template/subtitle":
			case "/obs-template/conversation":
				channel = "user,ai-synth";
				break;
			case "/obs-template/chat":
				channel = "chat";
				break;
			case "/obs-template/raid":
				channel = "effect";
				break;
			case "/obs-template/effect":
				channel = "effect,chat";
				break;
		}
	}

	async function copyUrl() {
		if (!absoluteUrl) return;
		try {
			if (navigator.clipboard?.writeText) {
				await navigator.clipboard.writeText(absoluteUrl);
				toastStore.success("URL をコピーしました", absoluteUrl);
				return;
			}
		} catch {
			// fallthrough
		}
		// フォールバック: 一時 textarea + execCommand
		try {
			const ta = document.createElement("textarea");
			ta.value = absoluteUrl;
			ta.style.position = "fixed";
			ta.style.opacity = "0";
			document.body.appendChild(ta);
			ta.select();
			const ok = document.execCommand("copy");
			document.body.removeChild(ta);
			if (ok) {
				toastStore.success("URL をコピーしました", absoluteUrl);
			} else {
				toastStore.error("コピーに失敗しました", "手動でコピーしてください。");
			}
		} catch (e) {
			toastStore.error("コピーに失敗しました", e instanceof Error ? e.message : String(e));
		}
	}

	function openInNewTab() {
		if (absoluteUrl) window.open(absoluteUrl, "_blank", "noopener");
	}

	async function withObsClient<T>(operation: (client: ObsWsClient) => Promise<T>): Promise<T> {
		const url = localStorage.getItem("vac.obs.url")?.trim() || "ws://127.0.0.1:4455";
		const password = localStorage.getItem("vac.obs.password") || null;
		const client = new ObsWsClient(url, password);
		try {
			await client.connect();
			return await operation(client);
		} finally {
			client.disconnect();
		}
	}

	async function loadObsBrowserSources(): Promise<void> {
		if (obsBusy) return;
		obsBusy = "load";
		try {
			const [inputResponse, sceneResponse, videoResponse] = await withObsClient((client) =>
				Promise.all([
					client.request<{ inputs?: ObsInput[] }>("GetInputList"),
					client.request<{ currentProgramSceneName?: string; scenes?: ObsScene[] }>("GetSceneList"),
					client.request<{ baseWidth?: number; baseHeight?: number }>("GetVideoSettings"),
				])
			);
			obsInputs = (inputResponse.inputs ?? [])
				.filter((input) => input.inputKind === "browser_source" || input.unversionedInputKind === "browser_source")
				.sort((a, b) => a.inputName.localeCompare(b.inputName, "ja"));
			obsScenes = (sceneResponse.scenes ?? []).sort((a, b) => (a.sceneIndex ?? 0) - (b.sceneIndex ?? 0));
			if (!obsInputs.some((input) => input.inputName === obsInputName)) {
				obsInputName = obsInputs[0]?.inputName ?? "";
			}
			if (!obsScenes.some((scene) => scene.sceneName === obsSceneName)) {
				obsSceneName = sceneResponse.currentProgramSceneName ?? obsScenes[0]?.sceneName ?? "";
			}
			obsBaseWidth = clampNumber(videoResponse.baseWidth ?? 1920, 320, 16384, 1920);
			obsBaseHeight = clampNumber(videoResponse.baseHeight ?? 1080, 240, 16384, 1080);
			if (obsInputs.length === 0) {
				toastStore.warn("Browser Sourceがありません", "下の新規作成から追加できる。");
			} else {
				toastStore.success("OBSソースを取得しました", `${obsInputs.length}件`);
			}
		} catch (cause) {
			toastStore.error("OBSソースを取得できません", cause instanceof Error ? cause.message : String(cause));
		} finally {
			obsBusy = null;
		}
	}

	async function createObsBrowserSource(): Promise<void> {
		const inputName = newObsInputName.trim();
		if (obsBusy || !absoluteUrl || !obsSceneName || !inputName) return;
		if (obsInputs.some((input) => input.inputName === inputName)) {
			toastStore.warn("同名のOBSソースがあります", "既存ソースを選んで反映するか、別の名前を指定してくれ。");
			return;
		}
		obsBusy = "create";
		try {
			await withObsClient((client) =>
				client.request("CreateInput", {
					sceneName: obsSceneName,
					inputName,
					inputKind: "browser_source",
					inputSettings: {
						url: absoluteUrl,
						width: obsBaseWidth,
						height: obsBaseHeight,
					},
					sceneItemEnabled: true,
				})
			);
			obsInputs = [...obsInputs, { inputName, inputKind: "browser_source", unversionedInputKind: "browser_source" }].sort((a, b) =>
				a.inputName.localeCompare(b.inputName, "ja")
			);
			obsInputName = inputName;
			localStorage.setItem("vac.obs.browserSourceName", inputName);
			localStorage.setItem("vac.obs.browserSourceScene", obsSceneName);
			toastStore.success("OBSソースを作成しました", `${obsSceneName} / ${inputName}`);
		} catch (cause) {
			toastStore.error("OBSソースを作成できません", cause instanceof Error ? cause.message : String(cause));
		} finally {
			obsBusy = null;
		}
	}

	async function applyUrlToObs(): Promise<void> {
		if (obsBusy || !absoluteUrl || !obsInputName) return;
		obsBusy = "apply";
		try {
			await withObsClient(async (client) => {
				const current = await client.request<{ inputKind?: string; inputSettings?: Record<string, unknown> }>("GetInputSettings", {
					inputName: obsInputName,
				});
				if (current.inputKind !== "browser_source") {
					throw new Error(`${obsInputName}はBrowser Sourceではない。`);
				}
				await client.request("SetInputSettings", {
					inputName: obsInputName,
					inputSettings: { url: absoluteUrl },
					overlay: true,
				});
			});
			localStorage.setItem("vac.obs.browserSourceName", obsInputName);
			toastStore.success("OBSへ反映しました", obsInputName);
		} catch (cause) {
			toastStore.error("OBSへ反映できません", cause instanceof Error ? cause.message : String(cause));
		} finally {
			obsBusy = null;
		}
	}

	function categoryLabel(c: BosEntry["category"]): string {
		switch (c) {
			case "subtitles":
				return "字幕";
			case "bgm":
				return "BGM";
			case "effects":
				return "エフェクト";
			case "other":
				return "その他";
		}
	}
</script>

<section class="rounded-lg border border-surface-200-800 bg-surface-100-900 p-4">
	<header class="mb-2 flex items-baseline justify-between gap-2">
		<div>
			<h2 class="text-sm font-semibold">BOS / OBS 出力プレビュー</h2>
			<p class="text-xs opacity-60">OBS のブラウザソースとして使える URL。選択中のソースは下の iframe でそのまま表示されます。</p>
		</div>
		<button
			type="button"
			class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-200-800"
			onclick={() => void load()}
			disabled={loading}
		>
			再読込
		</button>
	</header>

	{#if loading}
		<p class="text-xs opacity-70">読み込み中…</p>
	{:else if error}
		<p class="rounded border border-error-500/40 bg-error-500/10 p-2 text-xs">
			BOS 一覧の取得に失敗しました: {error}
		</p>
	{:else if !resp || resp.entries.length === 0}
		<p class="rounded border border-warning-500/40 bg-warning-500/10 p-2 text-xs">
			BOS エントリが見つかりませんでした。
			{#if resp?.document_root}
				<br />
				走査対象: <code>{resp.document_root}</code>
			{/if}
			<br />
			<code>resources/browser-output</code> を <code>conf.browser_source.document_root</code> に指定しているか確認してください。
		</p>
	{:else}
		<div class="flex flex-wrap items-end gap-3">
			<label class="flex flex-col">
				<span class="text-xs opacity-70">ソース</span>
				<select
					class="min-w-[16rem] rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
					value={selectedId ?? ""}
					onchange={(event) => selectSource(event.currentTarget.value)}
				>
					{#each resp.entries as e (e.id)}
						<option value={e.id}>
							[{categoryLabel(e.category)}] {e.title}{e.user_defined ? " *" : ""}
						</option>
					{/each}
				</select>
				<span class="text-[10px] opacity-50">* = ユーザー定義（同梱外）</span>
			</label>

			{#if selectedEntry?.supports_channel_param}
				<label class="flex flex-col">
					<span class="text-xs opacity-70">チャンネル</span>
					<input
						type="text"
						class="w-32 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
						bind:value={channel}
						placeholder="user,ai-synth"
					/>
				</label>
			{/if}
		</div>

		{#if selectedIsObsTemplate}
			<div class="mt-3 grid gap-2 rounded border border-surface-300-700 bg-surface-50-950 p-2 sm:grid-cols-4">
				<label class="flex flex-col">
					<span class="text-xs opacity-70">外観</span>
					<select class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm" bind:value={templateTheme}>
						<option value="default">default</option>
						<option value="minimal">minimal</option>
					</select>
				</label>
				<label class="flex flex-col">
					<span class="text-xs opacity-70">更新間隔 (ms)</span>
					<input
						type="number"
						min="100"
						max="60000"
						step="100"
						class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
						bind:value={templatePollMs}
					/>
				</label>
				<label class="flex flex-col">
					<span class="text-xs opacity-70">表示件数</span>
					<input
						type="number"
						min="1"
						max="100"
						step="1"
						class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
						bind:value={templateLimit}
					/>
				</label>
				<label class="flex flex-col">
					<span class="text-xs opacity-70">表示時間 (ms)</span>
					<input
						type="number"
						min="1000"
						max="300000"
						step="1000"
						class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
						bind:value={templateTtlMs}
					/>
				</label>
			</div>
			{#if selectedIsSubtitleTemplate}
				<div
					class="mt-2 grid gap-2 rounded border border-surface-300-700 bg-surface-50-950 p-2 sm:grid-cols-[auto_minmax(8rem,1fr)_minmax(7rem,auto)_minmax(7rem,auto)]"
				>
					<div class="flex flex-col gap-1">
						<span class="text-xs opacity-70">配置</span>
						<div class="flex" role="group" aria-label="字幕の配置">
							{#each [["left", "左"], ["center", "中央"], ["right", "右"]] as option (option[0])}
								<button
									type="button"
									class="border px-2 py-1 text-xs first:rounded-l last:rounded-r {templateAlign === option[0]
										? 'border-primary-500 bg-primary-500 text-white'
										: 'border-surface-300-700 hover:bg-surface-200-800'}"
									aria-pressed={templateAlign === option[0]}
									onclick={() => (templateAlign = option[0] as "left" | "center" | "right")}>{option[1]}</button
								>
							{/each}
						</div>
					</div>
					<label class="flex flex-col">
						<span class="text-xs opacity-70">話者名</span>
						<input
							class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
							maxlength="40"
							bind:value={templateLabel}
							placeholder="省略可"
						/>
					</label>
					<label class="flex flex-col">
						<span class="text-xs opacity-70">文字倍率 (%)</span>
						<input
							type="number"
							min="50"
							max="200"
							step="5"
							class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
							bind:value={templateFontScale}
						/>
					</label>
					<label class="flex flex-col">
						<span class="text-xs opacity-70">最大幅 (%)</span>
						<input
							type="number"
							min="20"
							max="100"
							step="5"
							class="rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
							bind:value={templateWidthPct}
						/>
					</label>
				</div>
			{/if}
			<div class="mt-2 flex flex-wrap items-center gap-1" role="group" aria-label="OBS template quick settings">
				{#each templateQuickPresets as preset (preset.id)}
					<button
						type="button"
						class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800"
						title={`${preset.theme} / poll ${preset.pollMs}ms / ttl ${preset.ttlMs}ms / ${preset.limit}`}
						onclick={() => applyTemplatePreset(preset)}
					>
						{preset.label}
					</button>
				{/each}
			</div>
		{/if}

		<div class="mt-3 flex items-center gap-2">
			<input
				type="text"
				class="flex-1 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 font-mono text-xs"
				value={absoluteUrl}
				readonly
				aria-label="Browser source URL"
			/>
			<button
				type="button"
				class="rounded bg-primary-500 px-3 py-1 text-xs font-semibold text-white hover:bg-primary-600"
				onclick={copyUrl}
				disabled={!absoluteUrl}
			>
				URL コピー
			</button>
			<button
				type="button"
				class="rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800"
				onclick={openInNewTab}
				disabled={!absoluteUrl}
			>
				別タブで開く
			</button>
		</div>

		{#if selectedIsObsTemplate}
			<div class="mt-2 grid gap-2 rounded border border-surface-300-700 bg-surface-50-950 p-2">
				<div class="grid gap-2 sm:grid-cols-[minmax(10rem,1fr)_auto_auto]">
					<label class="flex min-w-0 flex-col">
						<span class="text-xs opacity-70">OBS Browser Source</span>
						<select
							class="min-w-0 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
							bind:value={obsInputName}
							disabled={obsInputs.length === 0 || obsBusy !== null}
						>
							{#if obsInputs.length === 0}<option value="">未取得</option>{/if}
							{#each obsInputs as input (input.inputName)}
								<option value={input.inputName}>{input.inputName}</option>
							{/each}
						</select>
					</label>
					<button
						type="button"
						class="self-end rounded border border-surface-300-700 px-3 py-1 text-xs hover:bg-surface-200-800 disabled:opacity-50"
						disabled={obsBusy !== null}
						onclick={() => void loadObsBrowserSources()}
					>
						{obsBusy === "load" ? "取得中…" : "OBSから取得"}
					</button>
					<button
						type="button"
						class="self-end rounded bg-primary-500 px-3 py-1 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
						disabled={obsBusy !== null || !obsInputName || !absoluteUrl}
						onclick={() => void applyUrlToObs()}
					>
						{obsBusy === "apply" ? "反映中…" : "OBSへ反映"}
					</button>
				</div>
				<div class="grid gap-2 border-t border-surface-300-700 pt-2 sm:grid-cols-[minmax(10rem,1fr)_minmax(10rem,1fr)_auto]">
					<label class="flex min-w-0 flex-col">
						<span class="text-xs opacity-70">OBSシーン</span>
						<select
							class="min-w-0 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
							bind:value={obsSceneName}
							disabled={obsScenes.length === 0 || obsBusy !== null}
						>
							{#if obsScenes.length === 0}<option value="">OBSから取得</option>{/if}
							{#each obsScenes as scene (scene.sceneName)}
								<option value={scene.sceneName}>{scene.sceneName}</option>
							{/each}
						</select>
					</label>
					<label class="flex min-w-0 flex-col">
						<span class="text-xs opacity-70">新規OBSソース名</span>
						<input
							type="text"
							class="min-w-0 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-sm"
							bind:value={newObsInputName}
							disabled={obsBusy !== null}
						/>
					</label>
					<button
						type="button"
						class="self-end rounded bg-primary-500 px-3 py-1 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
						disabled={obsBusy !== null || !obsSceneName || !newObsInputName.trim() || !absoluteUrl}
						onclick={() => void createObsBrowserSource()}
					>
						{obsBusy === "create" ? "作成中…" : `新規作成 (${obsBaseWidth} x ${obsBaseHeight})`}
					</button>
				</div>
			</div>
		{/if}

		{#if selectedEntry}
			<dl class="mt-2 grid gap-x-3 gap-y-1 text-[11px] opacity-70 sm:grid-cols-[auto_minmax(0,1fr)_auto_minmax(0,1fr)]">
				<dt class="font-semibold">ID</dt>
				<dd class="min-w-0 break-all font-mono">{selectedEntry.id}</dd>
				<dt class="font-semibold">種別</dt>
				<dd>{categoryLabel(selectedEntry.category)}{selectedEntry.user_defined ? " / user" : " / bundled"}</dd>
				<dt class="font-semibold">Path</dt>
				<dd class="min-w-0 break-all font-mono sm:col-span-3">{selectedEntry.path}</dd>
			</dl>
		{/if}

		{#if composedUrl}
			<div class="mt-3">
				<button
					type="button"
					class="flex w-full items-center justify-between rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 text-xs hover:bg-surface-200-800"
					onclick={togglePreview}
					aria-expanded={previewOpen}
				>
					<span>プレビュー ({previewOpen ? "閉じる" : "開く"})</span>
					<span class="opacity-60">{isPortrait ? "縦画面" : "横画面"}</span>
				</button>
				{#if previewOpen}
					<div class="mt-2 flex flex-wrap items-center justify-between gap-2">
						<div class="flex items-center gap-1" role="group" aria-label="Preview aspect ratio">
							{#each previewPresets as preset (preset.id)}
								<button
									type="button"
									class="rounded border px-2 py-1 text-xs {previewPresetId === preset.id
										? 'border-primary-500 bg-primary-500 text-white'
										: 'border-surface-300-700 hover:bg-surface-200-800'}"
									aria-pressed={previewPresetId === preset.id}
									title={preset.size}
									onclick={() => (previewPresetId = preset.id)}
								>
									{preset.label}
								</button>
							{/each}
						</div>
						<span class="text-[11px] opacity-60">OBS {previewPreset.size}</span>
					</div>
					<div class="mt-2 overflow-auto rounded border border-surface-300-700 bg-black/90 p-2">
						<div
							class="mx-auto overflow-hidden bg-black"
							style={`aspect-ratio: ${previewPreset.aspect}; max-width: ${previewPreset.maxWidth};`}
						>
							<!-- iframe の src は絶対 URL でも相対 URL でも良い。相対にしておくと origin を切り替えても追従する -->
							<iframe
								title={selectedEntry?.title ?? "Browser source preview"}
								src={composedUrl}
								class="block h-full w-full"
								sandbox="allow-scripts allow-same-origin"
							></iframe>
						</div>
					</div>
				{/if}
			</div>
		{/if}
	{/if}
</section>

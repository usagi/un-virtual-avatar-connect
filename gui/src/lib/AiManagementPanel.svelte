<script lang="ts">
	/**
	 * AI management first slice.
	 *
	 * Focuses on live persona state and individual pause/resume controls. Deep
	 * prompt/reload editing remains in the existing Reload / config surfaces.
	 */
	import { onDestroy, onMount } from "svelte";
	import { api } from "./api";
	import {
		ControlApiError,
		type AiPersonaSummary,
		type AiReloadReport,
		type AiReloadRequest,
		type JsonValue,
		type NodePutConfigResponse,
		type PauseTarget,
		type StateSnapshot,
	} from "./types";
	import { eventsStore } from "./events.svelte";
	import PauseBadge from "./PauseBadge.svelte";
	import { toastStore } from "./toasts.svelte";

	const AUTO_REFRESH_MS = 10_000;

	let snapshot = $state<StateSnapshot | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let busyKey = $state<string | null>(null);
	let lastFetchedAt = $state<number | null>(null);
	let expandedIndex = $state<number | null>(null);
	let reloadBusyKey = $state<string | null>(null);
	let persistBusyKey = $state<string | null>(null);
	let reloadResult = $state<{ label: string; report: AiReloadReport | null } | null>(null);
	let reloadError = $state<string | null>(null);
	let persistResult = $state<{ label: string; response: NodePutConfigResponse } | null>(null);
	let persistError = $state<string | null>(null);

	let systemExtraEnabled = $state(false);
	let systemExtraText = $state("");
	let heartbeatMode = $state<"skip" | "on" | "off">("skip");
	let thresholdEnabled = $state(false);
	let thresholdValue = $state(0.5);
	type ReloadDraft = {
		systemExtraEnabled: boolean; systemExtraText: string;
		heartbeatMode: "skip" | "on" | "off";
		thresholdEnabled: boolean; thresholdValue: number;
	};
	const personaDrafts = new Map<string, ReloadDraft>();
	let draftKey: string | null = null;
	function readDraft(): ReloadDraft {
		return { systemExtraEnabled, systemExtraText, heartbeatMode, thresholdEnabled, thresholdValue };
	}
	function restoreDraft(draft: ReloadDraft) {
		({ systemExtraEnabled, systemExtraText, heartbeatMode, thresholdEnabled, thresholdValue } = draft);
	}
	function personaDraftKey(persona: AiPersonaSummary, state = snapshot): string {
		return JSON.stringify([state?.runtime.session_id, persona.index, persona.id]);
	}
	function acceptSnapshot(next: StateSnapshot) {
		const current = next.ai_personas.find((persona) => persona.index === expandedIndex);
		if (draftKey !== null && (!current || personaDraftKey(current, next) !== draftKey)) {
			personaDrafts.set(draftKey, readDraft());
			draftKey = null;
			expandedIndex = null;
			resetReloadForm();
			toastStore.warn("AIの実行対象が変わりました", "旧対象の入力を新しい対象へ引き継がず、詳細を閉じました。");
		}
		snapshot = next;
	}

	let timer: ReturnType<typeof setInterval> | null = null;
	let unsubscribe: (() => void) | null = null;

	const aiPersonas = $derived(snapshot?.ai_personas ?? []);
	const pausedCount = $derived(aiPersonas.filter((persona) => persona.paused).length);
	const activeCount = $derived(aiPersonas.length - pausedCount);
	const lastFetchedLabel = $derived(
		lastFetchedAt === null ? "-" : new Date(lastFetchedAt).toLocaleTimeString("ja-JP", { hour12: false })
	);

	async function refresh() {
		loading = true;
		try {
			acceptSnapshot(await api.snapshot());
			error = null;
			lastFetchedAt = Date.now();
		} catch (e) {
			error = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	onMount(() => {
		void refresh();
		timer = setInterval(() => void refresh(), AUTO_REFRESH_MS);
		unsubscribe = eventsStore.subscribe((ev) => {
			if (ev.event.kind === "pause_state" || ev.event.kind === "reloaded") {
				void refresh();
			}
		});
	});

	onDestroy(() => {
		if (timer !== null) clearInterval(timer);
		unsubscribe?.();
	});

	function personaLabel(persona: AiPersonaSummary): string {
		return persona.id ?? `#${persona.index}`;
	}

	function tokenCount(value: number): string {
		return new Intl.NumberFormat("ja-JP").format(value);
	}

	function toggleDetails(persona: AiPersonaSummary) {
		if (draftKey !== null) personaDrafts.set(draftKey, readDraft());
		const nextKey = personaDraftKey(persona);
		const closing = expandedIndex === persona.index && draftKey === nextKey;
		expandedIndex = closing ? null : persona.index;
		draftKey = closing ? null : nextKey;
		if (!closing) {
			const previous = personaDrafts.get(nextKey);
			if (previous) restoreDraft(previous);
			else resetReloadForm();
		}
		reloadResult = null;
		reloadError = null;
		persistResult = null;
		persistError = null;
	}

	function resetReloadForm() {
		systemExtraEnabled = false;
		systemExtraText = "";
		heartbeatMode = "skip";
		thresholdEnabled = false;
		thresholdValue = 0.5;
	}

	async function setPaused(persona: AiPersonaSummary, paused: boolean) {
		const key = `ai:${persona.index}`;
		if (busyKey !== null) return;
		busyKey = key;
		const target: PauseTarget = { target: "ai", index: persona.index };
		try {
			if (paused) {
				await api.pause(target);
			} else {
				await api.resume(target);
			}
			toastStore.success(paused ? "AI を一時停止しました" : "AI を再開しました", personaLabel(persona));
			await refresh();
		} catch (e) {
			const message = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
			toastStore.error("AI 状態変更に失敗しました", message);
		} finally {
			busyKey = null;
		}
	}

	async function reloadPersona(persona: AiPersonaSummary) {
		const key = `reload:${persona.index}`;
		if (reloadBusyKey !== null) return;
		const changes: AiReloadRequest = {
			system_instructions_extra: systemExtraEnabled ? systemExtraText : null,
			heartbeat_enabled: heartbeatMode === "skip" ? null : heartbeatMode === "on",
			decision_threshold: thresholdEnabled ? thresholdValue : null,
		};
		if (!systemExtraEnabled && heartbeatMode === "skip" && !thresholdEnabled) {
			reloadError = "送信する変更がありません";
			return;
		}
		reloadBusyKey = key;
		reloadError = null;
		reloadResult = null;
		try {
			const res = await api.reloadAiPersona(persona.id, changes);
			reloadResult = { label: personaLabel(persona), report: res.ai_report ?? null };
			toastStore.success("現在の動作に適用しました", `${personaLabel(persona)} · 次回起動用の設定は変更していません`);
			await refresh();
		} catch (e) {
			reloadError = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
		} finally {
			reloadBusyKey = null;
		}
	}

	function hasReloadChanges(): boolean {
		return systemExtraEnabled || heartbeatMode !== "skip" || thresholdEnabled;
	}

	function objectValue(value: JsonValue | undefined): { [k: string]: JsonValue } {
		if (value !== null && typeof value === "object" && !Array.isArray(value)) {
			return { ...value };
		}
		return {};
	}

	function buildPersistentPersonaConfig(config: JsonValue, draft: ReloadDraft): JsonValue {
		const next = objectValue(config);
		if (draft.systemExtraEnabled) {
			next.system_instructions_extra = draft.systemExtraText;
		}
		if (draft.heartbeatMode !== "skip") {
			next.heartbeat = {
				...objectValue(next.heartbeat),
				enabled: draft.heartbeatMode === "on",
			};
		}
		if (draft.thresholdEnabled) {
			next.decision = {
				...objectValue(next.decision),
				threshold: draft.thresholdValue,
			};
		}
		return next;
	}

	async function persistPersona(persona: AiPersonaSummary) {
		const key = `persist:${persona.index}`;
		if (persistBusyKey !== null) return;
		if (!hasReloadChanges()) {
			persistError = "保存する変更がありません";
			return;
		}
		persistBusyKey = key;
		const draft = readDraft();
		const expectedTarget = personaDraftKey(persona);
		persistError = null;
		persistResult = null;
		reloadError = null;
		reloadResult = null;
		try {
			const current = await api.aiPersonaConfig(persona.index);
			const runtimePersona = snapshot?.ai_personas.find((entry) => entry.index === persona.index);
			if (!runtimePersona || personaDraftKey(runtimePersona) !== expectedTarget || current.id !== persona.id || current.index !== persona.index) {
				throw new Error("保存対象が変わったため保存しませんでした。対象を確認してから再試行してください。");
			}
			if (!current.revision) throw new Error("保存先の版を確認できません。VAC本体を更新してから再試行してください。");
			const res = await api.putAiPersonaConfig(persona.index, {
				config: buildPersistentPersonaConfig(current.config, draft),
				expected_revision: current.revision,
			});
			persistResult = { label: personaLabel(persona), response: res };
			toastStore.success("次回起動用の設定を保存しました", `${personaLabel(persona)} · 現在の動作は変更していません`);
		} catch (e) {
			persistError = e instanceof ControlApiError ? `${e.status} ${e.statusText}` : e instanceof Error ? e.message : String(e);
		} finally {
			persistBusyKey = null;
		}
	}
</script>

<section class="rounded-lg border border-surface-300-700 bg-surface-100-900 p-4" aria-label="AI management panel">
	<header class="mb-3 flex flex-wrap items-start justify-between gap-3">
		<div>
			<h3 class="text-sm font-semibold">AI 管理</h3>
			<p class="text-xs opacity-60">Persona state, pause control, and runtime visibility.</p>
		</div>
		<div class="flex items-center gap-2">
			<span class="rounded bg-success-300-700 px-2 py-1 text-xs">{activeCount} active</span>
			<span class="rounded bg-warning-300-700 px-2 py-1 text-xs">{pausedCount} paused</span>
			<button
				type="button"
				class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800"
				onclick={() => void refresh()}
				disabled={loading}
			>
				更新
			</button>
		</div>
	</header>

	{#if error}
		<p class="rounded border border-error-500/40 bg-error-500/10 p-2 text-xs">
			snapshot 取得に失敗: {error}
		</p>
	{:else if loading && snapshot === null}
		<p class="text-xs opacity-70">読み込み中…</p>
	{:else if aiPersonas.length === 0}
		<p class="rounded border border-surface-300-700 bg-surface-50-950 p-2 text-xs opacity-70">AI persona は登録されていません。</p>
	{:else}
		<div class="grid gap-2">
			{#each aiPersonas as persona (persona.index)}
				{@const key = `ai:${persona.index}`}
				<article
					class="rounded border border-surface-300-700 bg-surface-50-950 p-2"
					aria-label={`AI persona ${personaLabel(persona)}`}
				>
					<div class="flex flex-wrap items-center justify-between gap-2">
						<div class="min-w-0">
							<div class="flex items-center gap-2">
								<span class="font-mono text-xs opacity-60">#{persona.index}</span>
								<h4 class="truncate text-sm font-semibold">{personaLabel(persona)}</h4>
								<PauseBadge paused={persona.paused} compact />
							</div>
							<p class="mt-1 text-[11px] opacity-60">
								{persona.usage.last_model ?? "未実行"} / {persona.usage.last_latency_ms ?? "-"} ms / last snapshot:
								{lastFetchedLabel}
							</p>
						</div>
						<div class="flex gap-1">
							<button
								type="button"
								class="rounded border border-surface-300-700 px-2 py-1 text-xs font-semibold hover:bg-surface-200-800"
								aria-expanded={expandedIndex === persona.index}
								aria-controls={`ai-runtime-editor-${persona.index}`}
								onclick={() => toggleDetails(persona)}
							>
								詳細
							</button>
							<button
								type="button"
								class="rounded border border-warning-500/50 px-2 py-1 text-xs font-semibold hover:bg-warning-500/10 disabled:opacity-45"
								onclick={() => void setPaused(persona, true)}
								disabled={busyKey !== null || persona.paused}
							>
								{busyKey === key ? "…" : "Pause"}
							</button>
							<button
								type="button"
								class="rounded border border-success-500/50 px-2 py-1 text-xs font-semibold hover:bg-success-500/10 disabled:opacity-45"
								onclick={() => void setPaused(persona, false)}
								disabled={busyKey !== null || !persona.paused}
							>
								{busyKey === key ? "…" : "Resume"}
							</button>
						</div>
					</div>
					{#if expandedIndex === persona.index}
						<div
							id={`ai-runtime-editor-${persona.index}`}
							class="mt-3 rounded border border-surface-300-700 bg-surface-100-900 p-2"
						>
							<div class="mb-3 grid grid-cols-2 gap-x-4 gap-y-1 text-xs sm:grid-cols-4">
								<span>判断完了 <strong>{tokenCount(persona.usage.completed_reactions)}</strong></span>
								<span>API <strong>{tokenCount(persona.usage.api_requests)}</strong></span>
								<span>Tool <strong>{tokenCount(persona.usage.tool_calls)}</strong></span>
								<span>失敗 <strong>{tokenCount(persona.usage.failed_reactions)}</strong></span>
								<span>入力 <strong>{tokenCount(persona.usage.input_tokens)}</strong></span>
								<span>Cache read <strong>{tokenCount(persona.usage.cached_tokens)}</strong></span>
								<span>Cache write <strong>{tokenCount(persona.usage.cache_write_tokens)}</strong></span>
								<span>Reasoning <strong>{tokenCount(persona.usage.reasoning_tokens)}</strong></span>
								<span>出力 <strong>{tokenCount(persona.usage.output_tokens)}</strong></span>
							</div>
							<div class="mb-2 flex flex-wrap items-center justify-between gap-2">
								<div class="text-xs font-semibold">Runtime reload</div>
								<button
									type="button"
									class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-200-800"
									onclick={resetReloadForm}
									disabled={reloadBusyKey !== null}
								>
									クリア
								</button>
							</div>
							<div class="grid gap-2 text-xs">
								<fieldset class="rounded border border-surface-300-700 p-2">
									<legend class="px-1">
										<label class="flex items-center gap-1">
											<input type="checkbox" bind:checked={systemExtraEnabled} />
											system_instructions_extra
										</label>
									</legend>
									<textarea
										class="w-full rounded border border-surface-300-700 bg-surface-50-950 p-1 font-mono disabled:opacity-40"
										rows="3"
										bind:value={systemExtraText}
										disabled={!systemExtraEnabled}
										placeholder="空のまま送信すると clear"
									></textarea>
								</fieldset>
								<div class="flex flex-wrap items-center gap-3 rounded border border-surface-300-700 p-2">
									<span class="font-semibold opacity-80">heartbeat</span>
									<label class="flex items-center gap-1"
										><input type="radio" bind:group={heartbeatMode} value="skip" /> skip</label
									>
									<label class="flex items-center gap-1"
										><input type="radio" bind:group={heartbeatMode} value="on" /> ON</label
									>
									<label class="flex items-center gap-1"
										><input type="radio" bind:group={heartbeatMode} value="off" /> OFF</label
									>
								</div>
								<div class="flex flex-wrap items-center gap-2 rounded border border-surface-300-700 p-2">
									<label class="flex items-center gap-1 font-semibold opacity-80">
										<input type="checkbox" bind:checked={thresholdEnabled} />
										decision_threshold
									</label>
									<input
										type="number"
										min="0"
										max="1"
										step="0.01"
										class="w-24 rounded border border-surface-300-700 bg-surface-50-950 px-2 py-1 font-mono disabled:opacity-40"
										bind:value={thresholdValue}
										disabled={!thresholdEnabled}
									/>
								</div>
								<div class="flex items-center gap-2">
									<button
										type="button"
										class="rounded bg-primary-500 px-3 py-1 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
										onclick={() => void reloadPersona(persona)}
										disabled={reloadBusyKey !== null}
									>
										{reloadBusyKey === `reload:${persona.index}` ? "送信中…" : "現在の動作に適用"}
									</button>
									<button
										type="button"
										class="rounded border border-primary-500/60 px-3 py-1 text-xs font-semibold hover:bg-primary-500/10 disabled:opacity-50"
										onclick={() => void persistPersona(persona)}
										disabled={persistBusyKey !== null}
									>
										{persistBusyKey === `persist:${persona.index}` ? "保存中…" : "次回起動用に保存"}
									</button>
								</div>
								{#if reloadError}
									<span class="text-error-500">{reloadError}</span>
								{:else if reloadResult}
									<span class="text-success-500">
										{reloadResult.label}: heartbeat={reloadResult.report?.heartbeat_enabled_changed ?? false},
										threshold={reloadResult.report?.decision_threshold_changed ?? false}
									</span>
								{/if}
								{#if persistError}
									<span class="text-error-500">{persistError}</span>
								{:else if persistResult}
									<span role="status">
										{persistResult.label}: 保存済み（現在の動作は未変更）
										<span class="block text-xs opacity-70">バックアップ: {persistResult.response.backup}</span>
										{#if persistResult.response.warning}
											<span class="block font-medium">{persistResult.response.warning}</span>
										{/if}
									</span>
								{/if}
							</div>
						</div>
					{/if}
				</article>
			{/each}
		</div>
	{/if}
</section>

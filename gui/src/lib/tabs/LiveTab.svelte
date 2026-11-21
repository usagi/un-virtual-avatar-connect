<script lang="ts">
	import FeatureAnchor from "../FeatureAnchor.svelte";
	/** 配信中の操作を主面に置き、構成・互換機能は段階的に開示する。 */
	import BigPauseButtons from "../BigPauseButtons.svelte";
	import BosOutputManager from "../BosOutputManager.svelte";
	import BosPreview from "../BosPreview.svelte";
	import IngressPanel from "../IngressPanel.svelte";
	import QuickEffectsPanel from "../QuickEffectsPanel.svelte";
	import ObsControlWidget from "../widgets/ObsControlWidget.svelte";
	import LiveAiControlPanel from "../live/LiveAiControlPanel.svelte";
	import VoiceInputControl from "../VoiceInputControl.svelte";
	import ReadinessPanel from "../ReadinessPanel.svelte";
	import PreflightSelfTestPanel from "../PreflightSelfTestPanel.svelte";
	import { runtimeSnapshotStore } from "../runtimeSnapshot.svelte";
	import { tabNavStore } from "../tabs.svelte";

	const readiness = $derived(runtimeSnapshotStore.snapshot?.readiness ?? null);
	const preparationNeedsAttention = $derived(readiness?.status === "attention" || readiness?.status === "blocked");
	const preparationLabel = $derived(
		readiness === null
			? "確認中"
			: readiness.status === "blocked"
				? "対応が必要"
				: readiness.status === "attention"
					? "確認が必要"
					: "準備完了"
	);
</script>

<section class="grid gap-4">
	<header>
		<h2 class="text-xl font-semibold">ライブ</h2>
	</header>

	<div class="grid items-start gap-4 xl:grid-cols-2">
		<FeatureAnchor id="live-ai"><LiveAiControlPanel /></FeatureAnchor>
		<FeatureAnchor id="voice"><VoiceInputControl /></FeatureAnchor>
	</div>

	<FeatureAnchor id="effects"><QuickEffectsPanel /></FeatureAnchor>

	<div class="mt-1 border-t border-surface-200-800">
		<details class="group border-b border-surface-200-800" open={preparationNeedsAttention || tabNavStore.featureFocus?.id === "preflight"}>
			<summary class="flex cursor-pointer list-none items-center justify-between gap-3 py-3 text-sm font-semibold">
				<span>配信準備</span>
				<span
					class:text-error-500={readiness?.status === "blocked"}
					class:text-warning-500={readiness?.status === "attention"}
					class:text-success-500={readiness?.status === "ready"}
					class="text-xs font-medium"
				>
					{preparationLabel}
				</span>
			</summary>
			<div class="grid gap-4 pb-5">
				<ReadinessPanel requiredOnly currentTab="live" />
				<FeatureAnchor id="preflight"><PreflightSelfTestPanel /></FeatureAnchor>
			</div>
		</details>

		<details class="group border-b border-surface-200-800">
			<summary class="cursor-pointer list-none py-3 text-sm font-semibold">配信入出力</summary>
			<div class="grid gap-4 pb-5">
				<div class="grid gap-4 xl:grid-cols-[minmax(24rem,0.9fr)_minmax(28rem,1.1fr)]">
					<FeatureAnchor id="ingress"><IngressPanel /></FeatureAnchor>
					<FeatureAnchor id="obs"><ObsControlWidget /></FeatureAnchor>
				</div>
				<section aria-labelledby="live-preview-heading">
					<h3 id="live-preview-heading" class="mb-2 text-sm font-semibold">出力プレビュー</h3>
					<FeatureAnchor id="preview"><BosPreview /></FeatureAnchor>
				</section>
			</div>
		</details>

		<details class="group border-b border-surface-200-800">
			<summary class="cursor-pointer list-none py-3 text-sm font-semibold">補助操作</summary>
			<div class="grid gap-4 pb-4">
				<details>
					<summary class="cursor-pointer text-xs font-semibold opacity-70">一括停止と再開</summary>
					<div class="pt-3"><FeatureAnchor id="pause"><BigPauseButtons /></FeatureAnchor></div>
				</details>
				<details class="border-t border-surface-200-800 pt-3">
					<summary class="cursor-pointer text-xs font-semibold opacity-70">出力管理</summary>
					<div class="pt-3"><FeatureAnchor id="outputs"><BosOutputManager /></FeatureAnchor></div>
				</details>
			</div>
		</details>
	</div>
</section>

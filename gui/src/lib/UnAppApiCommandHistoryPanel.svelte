<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "./api";
	import type { UnAppApiCommandHistoryItem, UnAppApiDiscoveryHealthSummary, UnAppApiEndpointCandidateDiagnostic } from "./types";

	let loading = $state(false);
	let error = $state<string | null>(null);
	let commands = $state<UnAppApiCommandHistoryItem[]>([]);
	let discoveryHealth = $state<UnAppApiDiscoveryHealthSummary | null>(null);
	let discoveryErrors = $state<string[]>([]);
	let endpointDiagnostics = $state<UnAppApiEndpointCandidateDiagnostic[]>([]);

	const hasDiscoveryIssue = $derived(
		(discoveryHealth?.discovery_error_count ?? 0) > 0 || (discoveryHealth?.endpoint_warning_count ?? 0) > 0
	);

	async function load(): Promise<void> {
		loading = true;
		error = null;
		try {
			const [res, diagnostics] = await Promise.all([api.unAppApiCommandHistory(50), api.flowgraphDiagnostics()]);
			commands = res.commands;
			discoveryHealth = diagnostics.unaa_z_health ?? null;
			discoveryErrors = diagnostics.unaa_z.discovery_errors ?? [];
			endpointDiagnostics = diagnostics.unaa_z_endpoint_candidate_diagnostics ?? [];
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			loading = false;
		}
	}

	function statusClass(item: UnAppApiCommandHistoryItem): string {
		if (item.status === "response_observed") return "bg-success-300-700 text-success-900-100";
		if (item.status === "publish_failed") return "bg-error-300-700/70 text-error-900-100";
		return "bg-warning-300-700 text-warning-900-100";
	}

	function summarize(item: UnAppApiCommandHistoryItem): string {
		if (item.error) return item.error;
		if (item.response_observed) return item.response?.key_expr ?? item.response_key_expr;
		return item.response_key_expr;
	}

	function diagnosticKey(item: UnAppApiEndpointCandidateDiagnostic): string {
		return `${item.code}:${item.candidate_id ?? ""}:${item.field}:${item.source_key ?? ""}`;
	}

	function healthValue(value: number | undefined): string {
		return String(value ?? 0);
	}

	onMount(() => {
		void load();
	});
</script>

<section class="rounded border border-surface-200-800 bg-surface-50-950">
	<div class="flex flex-wrap items-center justify-between gap-2 border-b border-surface-200-800 px-3 py-2">
		<div>
			<h3 class="text-sm font-semibold">UNAA/Z command 履歴</h3>
			<p class="text-xs opacity-60">直近の command publish / response wait の記録。</p>
		</div>
		<button
			type="button"
			class="rounded border border-surface-300-700 px-2 py-1 text-xs hover:bg-surface-100-900 disabled:opacity-50"
			disabled={loading}
			onclick={() => void load()}
		>
			{loading ? "読み込み中..." : "更新"}
		</button>
	</div>

	{#if discoveryHealth}
		<div
			class={`border-b px-3 py-2 ${
				hasDiscoveryIssue ? "border-warning-500/30 bg-warning-500/10" : "border-surface-200-800 bg-surface-100-900/35"
			}`}
			data-testid="unaa-z-health-summary"
		>
			<div class="mb-2 flex flex-wrap items-center justify-between gap-2">
				<div>
					<div class="text-xs font-semibold">UNAA/Z discovery health</div>
					<div class="text-[11px] opacity-65">
						source={discoveryHealth.source} / registry={discoveryHealth.managed_registry_app_count} / zenoh={discoveryHealth.zenoh_app_count}
					</div>
				</div>
				{#if hasDiscoveryIssue}
					<div class="flex flex-wrap gap-1 text-[11px]">
						{#if discoveryHealth.discovery_error_count > 0}
							<a
								class="rounded bg-error-300-700/70 px-1.5 py-0.5 font-semibold text-error-900-100 hover:underline"
								href="#unaa-z-discovery-errors"
							>
								errors {discoveryHealth.discovery_error_count}
							</a>
						{/if}
						{#if discoveryHealth.endpoint_warning_count > 0}
							<a
								class="rounded bg-warning-300-700 px-1.5 py-0.5 font-semibold text-warning-900-100 hover:underline"
								href="#unaa-z-endpoint-diagnostics"
							>
								warnings {discoveryHealth.endpoint_warning_count}
							</a>
						{/if}
					</div>
				{/if}
			</div>
			<div class="grid grid-cols-4 overflow-hidden rounded border border-surface-200-800 bg-surface-50-950 text-center text-[11px]">
				<div class="border-r border-surface-200-800 px-2 py-1.5">
					<div class="font-semibold">{healthValue(discoveryHealth.app_count)}</div>
					<div class="opacity-60">Apps</div>
				</div>
				<div class="border-r border-surface-200-800 px-2 py-1.5">
					<div class="font-semibold">{healthValue(discoveryHealth.endpoint_candidate_count)}</div>
					<div class="opacity-60">Candidates</div>
				</div>
				<div class="border-r border-surface-200-800 px-2 py-1.5">
					<div class={`font-semibold ${discoveryHealth.endpoint_warning_count > 0 ? "text-warning-500" : ""}`}>
						{healthValue(discoveryHealth.endpoint_warning_count)}
					</div>
					<div class="opacity-60">Warnings</div>
				</div>
				<div class="px-2 py-1.5">
					<div class={`font-semibold ${discoveryHealth.discovery_error_count > 0 ? "text-error-500" : ""}`}>
						{healthValue(discoveryHealth.discovery_error_count)}
					</div>
					<div class="opacity-60">Errors</div>
				</div>
			</div>
			<div class="mt-1 text-[11px] opacity-65">
				stale={discoveryHealth.stale_app_count} / root_endpoints={discoveryHealth.root_endpoint_count} / query_keys={discoveryHealth.query_key_count}
				/ announce_keys={discoveryHealth.announcement_key_count}
			</div>
		</div>
	{/if}

	{#if discoveryErrors.length > 0}
		<div id="unaa-z-discovery-errors" class="border-b border-error-500/30 bg-error-500/10 px-3 py-2">
			<div class="mb-1 flex items-center justify-between gap-2">
				<div class="text-xs font-semibold text-error-800-100">UNAA/Z discovery errors</div>
				<div class="text-[0.65rem] opacity-70">{discoveryErrors.length} errors</div>
			</div>
			<ol class="max-h-28 overflow-y-auto">
				{#each discoveryErrors as item, i (`${i}:${item}`)}
					<li class="border-t border-error-500/20 py-1 font-mono text-[11px] break-all first:border-t-0">{item}</li>
				{/each}
			</ol>
		</div>
	{/if}

	{#if endpointDiagnostics.length > 0}
		<div id="unaa-z-endpoint-diagnostics" class="border-b border-warning-500/30 bg-warning-500/10 px-3 py-2">
			<div class="mb-1 flex items-center justify-between gap-2">
				<div class="text-xs font-semibold text-warning-800-100">UNAA/Z endpoint diagnostics</div>
				<div class="text-[0.65rem] opacity-70">{endpointDiagnostics.length} warnings</div>
			</div>
			<ol class="max-h-40 overflow-y-auto">
				{#each endpointDiagnostics as item (diagnosticKey(item))}
					<li class="border-t border-warning-500/20 py-1 text-[11px] first:border-t-0">
						<div class="flex flex-wrap items-center gap-2 opacity-75">
							<code>{item.app_id ?? "unknown"}/{item.endpoint_id ?? "unknown"}</code>
							<span class="rounded bg-warning-300-700 px-1.5 py-0 font-semibold text-warning-900-100">{item.code}</span>
							<code>{item.field}</code>
						</div>
						<div class="mt-0.5 break-all font-mono opacity-65">{item.source_key ?? item.candidate_id ?? "-"}</div>
						<div class="mt-0.5 opacity-75">{item.message}</div>
					</li>
				{/each}
			</ol>
		</div>
	{/if}

	{#if error}
		<div class="px-3 py-3 text-sm text-error-500">{error}</div>
	{:else if commands.length === 0}
		<div class="px-3 py-6 text-center text-sm opacity-60">まだ UNAA/Z command 履歴はありません。</div>
	{:else}
		<ol class="max-h-72 overflow-y-auto">
			{#each commands.toReversed() as item (`${item.at}:${item.request_id}`)}
				<li class="border-b border-surface-200-800 px-3 py-2 last:border-b-0">
					<div class="flex flex-wrap items-center gap-2 text-[11px] opacity-70">
						<span>{new Date(item.at).toLocaleString()}</span>
						<code>{item.app_id}/{item.instance_id}</code>
						<code>{item.request_id}</code>
						<span class="rounded px-1.5 py-0 font-semibold uppercase {statusClass(item)}">{item.status}</span>
					</div>
					<div class="mt-1 font-mono text-xs break-all">{item.command} -> {item.key_expr}</div>
					<div class="mt-1 font-mono text-[11px] break-all opacity-65">{summarize(item)}</div>
				</li>
			{/each}
		</ol>
	{/if}
</section>

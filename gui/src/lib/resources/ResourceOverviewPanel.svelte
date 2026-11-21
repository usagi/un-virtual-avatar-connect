<script lang="ts">
	/**
	 * Resources タブ向けのリソース概要。
	 *
	 * ここは読み取り専用。起動・停止・再起動は Managed App ドロワーと
	 * run_with エディタに残し、概要画面では状態確認に集中できるようにする。
	 */
	import { onMount } from "svelte";
	import { api } from "../api";
	import { eventsStore } from "../events.svelte";
	import {
		ControlApiError,
		type ControlEvent,
		type ManagedAppsResponse,
		type UnAppApiCommandHistoryItem,
		type UnAppApiDiscoveryHealthSummary,
		type UnAppApiEndpointCandidateDiagnostic,
		type UnmfzTransportSnapshot,
	} from "../types";

	let loading = $state(true);
	let error = $state<string | null>(null);
	let apps = $state<ManagedAppsResponse | null>(null);
	let commandHistory = $state<UnAppApiCommandHistoryItem[]>([]);
	let discoveryHealth = $state<UnAppApiDiscoveryHealthSummary | null>(null);
	let endpointDiagnostics = $state<UnAppApiEndpointCandidateDiagnostic[]>([]);
	let unmfzTransport = $state<UnmfzTransportSnapshot | null>(null);
	const commandStaleMs = 10_000;

	const entries = $derived(apps?.entries ?? []);
	const total = $derived(entries.length);
	const statusTracked = $derived(entries.filter((entry) => entry.supports_status).length);
	const running = $derived(entries.filter((entry) => entry.supports_status && entry.status.running).length);
	const unknown = $derived(entries.filter((entry) => !entry.supports_status).length);
	const unaaZReady = $derived(entries.filter((entry) => entry.unaa_z_status.ready).length);
	const unaaZZenohSeen = $derived(entries.filter((entry) => entry.unaa_z_status.zenoh_seen).length);
	const unaaZEndpointTotal = $derived(entries.reduce((sum, entry) => sum + entry.unaa_z_status.endpoint_count, 0));
	const unaaZCapabilityTotal = $derived(entries.reduce((sum, entry) => sum + entry.unaa_z_status.capability_count, 0));
	const commandObserved = $derived(commandHistory.filter((item) => item.status === "response_observed").length);
	const commandTimeout = $derived(commandHistory.filter((item) => item.status === "accepted_timeout").length);
	const commandFailed = $derived(commandHistory.filter((item) => item.status === "publish_failed").length);
	const endpointDiagnosticApps = $derived(new Set(endpointDiagnostics.map((item) => item.app_id).filter(Boolean)).size);
	const staleDiscovery = $derived(entries.filter((entry) => entry.unaa_z_status.discovery_stale).length);
	const staleCommands = $derived(commandHistory.filter((item) => commandIsStale(item)).length);
	const processOnly = $derived(
		entries.filter((entry) => entry.supports_status && entry.status.running && !entry.unaa_z_status.zenoh_seen)
	);
	const readyOnly = $derived(
		entries.filter((entry) => !entry.status.running && entry.unaa_z_status.ready && entry.unaa_z_status.zenoh_seen)
	);
	const pidTotal = $derived(entries.reduce((sum, entry) => sum + (entry.supports_status ? entry.status.pids.length : 0), 0));
	const recentRunning = $derived(entries.filter((entry) => entry.supports_status && entry.status.running).slice(0, 4));

	onMount(() => {
		void refresh();
	});

	$effect(() => {
		const off = eventsStore.subscribe((ts) => {
			const ev: ControlEvent = ts.event;
			if (ev.kind !== "managed_app_state" || !apps) return;
			const idx = apps.entries.findIndex((entry) => entry.id === ev.id);
			if (idx < 0) return;
			const next = apps.entries.slice();
			next[idx] = {
				...next[idx],
				status: {
					id: ev.id,
					running: ev.running,
					pids: ev.pids,
					checked_at: ev.checked_at,
				},
			};
			apps = { ...apps, entries: next };
		});
		return off;
	});

	async function refresh(): Promise<void> {
		loading = true;
		error = null;
		try {
			const [nextApps, history, flowgraphDiagnostics] = await Promise.all([
				api.managedApps(),
				api.unAppApiCommandHistory(20),
				api.flowgraphDiagnostics(),
			]);
			apps = nextApps;
			commandHistory = history.commands;
			discoveryHealth = flowgraphDiagnostics.unaa_z_health ?? null;
			endpointDiagnostics = flowgraphDiagnostics.unaa_z_endpoint_candidate_diagnostics ?? [];
			unmfzTransport = flowgraphDiagnostics.unmfz_transport ?? null;
		} catch (e) {
			error = formatError(e);
		} finally {
			loading = false;
		}
	}

	function formatError(e: unknown): string {
		if (e instanceof ControlApiError) return `${e.status} ${e.statusText}`;
		if (e instanceof Error) return e.message;
		return String(e);
	}

	function latestCommandFor(appId: string, instanceId: string): UnAppApiCommandHistoryItem | null {
		return commandHistory.toReversed().find((item) => item.app_id === appId && item.instance_id === instanceId) ?? null;
	}

	function commandStatusClass(item: UnAppApiCommandHistoryItem): string {
		if (commandIsStale(item)) return "text-surface-500";
		if (item.status === "response_observed") return "text-success-500";
		if (item.status === "publish_failed") return "text-error-500";
		return "text-warning-500";
	}

	function commandIsStale(item: UnAppApiCommandHistoryItem): boolean {
		const t = Date.parse(item.at);
		return Number.isFinite(t) && Date.now() - t > commandStaleMs;
	}

	function endpointDiagnosticsFor(appId: string, instanceId: string): UnAppApiEndpointCandidateDiagnostic[] {
		return endpointDiagnostics.filter((item) => item.app_id === appId || item.app_id === instanceId);
	}
</script>

<section class="vac-panel-muted p-4" aria-label="リソース概要">
	<header class="mb-3 flex items-start justify-between gap-3">
		<div>
			<h3 class="text-sm font-semibold opacity-80">リソース概要</h3>
			<p class="mt-0.5 text-xs opacity-60">現在の run_with 登録から Managed App の状態を確認します。</p>
		</div>
		<button
			type="button"
			class="rounded border border-surface-300-700 px-2 py-0.5 text-xs hover:bg-surface-200-800 disabled:opacity-50"
			disabled={loading}
			onclick={() => void refresh()}
		>
			更新
		</button>
	</header>

	{#if error}
		<p class="rounded border border-error-500/40 bg-error-500/10 p-2 text-xs">{error}</p>
	{:else}
		<div class="grid grid-cols-4 overflow-hidden rounded border border-surface-200-800 bg-surface-50-950 text-center text-xs">
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold">{loading ? "-" : total}</div>
				<div class="opacity-60">Apps</div>
			</div>
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold">{loading ? "-" : running}</div>
				<div class="opacity-60">起動中</div>
			</div>
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold">{loading ? "-" : statusTracked}</div>
				<div class="opacity-60">追跡対象</div>
			</div>
			<div class="px-2 py-2">
				<div class="font-semibold">{loading ? "-" : pidTotal}</div>
				<div class="opacity-60">PIDs</div>
			</div>
		</div>

		<div class="mt-2 grid grid-cols-4 overflow-hidden rounded border border-surface-200-800 bg-surface-50-950 text-center text-xs">
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold">{loading ? "-" : unaaZZenohSeen}</div>
				<div class="opacity-60">Zenoh{staleDiscovery > 0 ? ` stale ${staleDiscovery}` : ""}</div>
			</div>
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold">{loading ? "-" : unaaZReady}</div>
				<div class="opacity-60">Ready</div>
			</div>
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class={`font-semibold ${endpointDiagnostics.length > 0 ? "text-warning-500" : ""}`}>
					{loading ? "-" : unaaZEndpointTotal}
				</div>
				<div class="opacity-60">Endpoints{endpointDiagnostics.length > 0 ? ` warn ${endpointDiagnostics.length}` : ""}</div>
			</div>
			<div class="px-2 py-2">
				<div class="font-semibold">{loading ? "-" : unaaZCapabilityTotal}</div>
				<div class="opacity-60">Caps</div>
			</div>
		</div>

		<div class="mt-2 grid grid-cols-4 overflow-hidden rounded border border-surface-200-800 bg-surface-50-950 text-center text-xs">
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold">{loading ? "-" : (discoveryHealth?.zenoh_app_count ?? 0)}</div>
				<div class="opacity-60">UNAA/Z apps</div>
			</div>
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold">{loading ? "-" : (discoveryHealth?.endpoint_candidate_count ?? 0)}</div>
				<div class="opacity-60">Candidates</div>
			</div>
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class={`font-semibold ${(discoveryHealth?.endpoint_warning_count ?? 0) > 0 ? "text-warning-500" : ""}`}>
					{loading ? "-" : (discoveryHealth?.endpoint_warning_count ?? 0)}
				</div>
				<div class="opacity-60">Warnings</div>
			</div>
			<div class="px-2 py-2">
				<div class={`font-semibold ${(discoveryHealth?.discovery_error_count ?? 0) > 0 ? "text-error-500" : ""}`}>
					{loading ? "-" : (discoveryHealth?.discovery_error_count ?? 0)}
				</div>
				<div class="opacity-60">Discovery errors</div>
			</div>
		</div>

		<div class="mt-2 grid grid-cols-4 overflow-hidden rounded border border-surface-200-800 bg-surface-50-950 text-center text-xs">
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold">{loading ? "-" : commandHistory.length}</div>
				<div class="opacity-60">Commands</div>
			</div>
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold text-success-500">{loading ? "-" : commandObserved}</div>
				<div class="opacity-60">Response</div>
			</div>
			<div class="border-r border-surface-200-800 px-2 py-2">
				<div class="font-semibold text-warning-500">{loading ? "-" : commandTimeout}</div>
				<div class="opacity-60">Timeout{staleCommands > 0 ? ` stale ${staleCommands}` : ""}</div>
			</div>
			<div class="px-2 py-2">
				<div class="font-semibold text-error-500">{loading ? "-" : commandFailed}</div>
				<div class="opacity-60">Failed</div>
			</div>
		</div>

		{#if loading}
			<p class="mt-3 text-xs opacity-60">リソース状態を読み込み中...</p>
		{:else if total === 0}
			<p class="mt-3 rounded border border-warning-500/40 bg-warning-500/10 p-2 text-xs">
				Managed App は未登録です。Runtime Mode から外部ツールを協調制御するには run_with に追加します。
			</p>
		{:else}
			<div class="mt-3 space-y-2">
				{#if unknown > 0}
					<p class="vac-panel p-2 text-xs opacity-75">
						{unknown} 件のアプリは起動できますが、process marker が未設定のため起動状態を報告できません。
					</p>
				{/if}
				{#if apps?.unaa_z_discovery_errors.length}
					<p class="rounded border border-warning-500/40 bg-warning-500/10 p-2 text-xs">
						UNAA/Z discovery: {apps.unaa_z_discovery_errors.join(" / ")}
					</p>
				{/if}
				{#if discoveryHealth}
					<p class="vac-panel p-2 text-xs opacity-75">
						UNAA/Z health: source={discoveryHealth.source} / registry={discoveryHealth.managed_registry_app_count}
						/ zenoh={discoveryHealth.zenoh_app_count} / query_keys={discoveryHealth.query_key_count}
						/ announce_keys={discoveryHealth.announcement_key_count}
						{#if discoveryHealth.stale_app_count > 0}
							/ stale={discoveryHealth.stale_app_count}
						{/if}
					</p>
				{/if}
				{#if unmfzTransport}
					<div class="vac-panel p-2 text-xs">
						<div class="font-semibold opacity-75">UNMF/Z transport</div>
						<div class="mt-1 opacity-75">
							mode={unmfzTransport.mode} / auto={unmfzTransport.automatic_discovery ? "on" : "off"}
							{#if unmfzTransport.listen_endpoint}
								/ listen={unmfzTransport.listen_endpoint}{/if}
						</div>
						{#if unmfzTransport.connect_endpoints.length > 0}
							<div class="mt-1 flex flex-wrap gap-1 opacity-75">
								{#each unmfzTransport.connect_endpoints as endpoint (endpoint)}
									<code>connect={endpoint}</code>
								{/each}
							</div>
						{/if}
					</div>
				{/if}
				{#if endpointDiagnostics.length > 0}
					<div class="rounded border border-warning-500/40 bg-warning-500/10 p-2 text-xs">
						<div class="font-semibold opacity-80">UNAA/Z endpoint diagnostics</div>
						<div class="mt-1 opacity-70">{endpointDiagnostics.length} warnings / {endpointDiagnosticApps} apps</div>
						<div class="mt-1 flex flex-wrap gap-1">
							{#each endpointDiagnostics.slice(0, 8) as diag (`${diag.code}:${diag.candidate_id}:${diag.field}`)}
								<code title={diag.message}>{diag.app_id ?? "unknown"}:{diag.field}:{diag.code}</code>
							{/each}
						</div>
					</div>
				{/if}
				{#if processOnly.length > 0 || readyOnly.length > 0}
					<div class="vac-panel p-2 text-xs">
						<div class="font-semibold opacity-75">Process / UNAA/Z 差分</div>
						<div class="mt-1 flex flex-wrap gap-1 opacity-75">
							{#each processOnly as entry (entry.id)}
								<code>{entry.label}: process only</code>
							{/each}
							{#each readyOnly as entry (entry.id)}
								<code>{entry.label}: zenoh ready</code>
							{/each}
						</div>
					</div>
				{/if}
				{#if recentRunning.length > 0}
					<ul class="space-y-1">
						{#each recentRunning as entry (entry.id)}
							{@const latestCommand = latestCommandFor(entry.unaa_z_status.app_id, entry.unaa_z_status.instance_id)}
							{@const endpointWarnings = endpointDiagnosticsFor(entry.unaa_z_status.app_id, entry.unaa_z_status.instance_id)}
							<li class="vac-subtle-row flex items-center justify-between gap-2 px-2 py-1.5 text-xs">
								<span class="min-w-0 truncate font-semibold">{entry.label}</span>
								<span class="shrink-0 font-mono opacity-60">
									pids [{entry.status.pids.join(", ")}] / unaa_z {entry.unaa_z_status.ready ? "ready" : "not-ready"}
									/ via {entry.unaa_z_status.discovery_payload_kind}
									{#if endpointWarnings.length > 0}
										/ endpoint <span class="text-warning-500">warn {endpointWarnings.length}</span>
									{/if}
									{#if entry.unaa_z_status.discovery_stale}
										/ discovery <span class="text-surface-500">stale</span>
									{/if}
									{#if latestCommand}
										/ cmd <span class={commandStatusClass(latestCommand)}
											>{latestCommand.status}{commandIsStale(latestCommand) ? ":stale" : ""}</span
										>
									{/if}
								</span>
							</li>
						{/each}
					</ul>
				{:else}
					<p class="mt-3 text-xs opacity-60">現在起動中の追跡対象 Managed App はありません。</p>
				{/if}
			</div>
		{/if}
	{/if}
</section>

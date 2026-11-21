import type { FlowgraphDiagnosticsResponse } from "../types";

export type UnmfzEndpointCandidate = {
	id: string;
	label: string;
	appId: string;
	instanceId?: string;
	appDisplayName?: string;
	endpointId: string;
	direction: "input" | "output" | "both";
	baseKeyExpr: string;
	topicMode: "frame" | "primary_source" | "stream_id";
	schemaMajor: number;
	discoverySource?: string;
	sourceKey?: string;
	payloadKind?: string;
	discoveryStale?: boolean;
	discoveryAgeMs?: number | null;
	validationWarnings: string[];
	endpoint: Record<string, unknown>;
};

export function collectUnmfzEndpointCandidates(
	diagnostics: FlowgraphDiagnosticsResponse | Record<string, unknown> | null
): UnmfzEndpointCandidate[] {
	if (!diagnostics) return [];
	const root = diagnostics as unknown as Record<string, unknown>;
	const buckets: unknown[] = [
		root.unaa_z_endpoint_candidates,
		root.unaa_z_endpoints,
		root.unaaz_endpoint_candidates,
		asRecord(root.unaa_z)?.endpoints,
		asRecord(root.unaaz)?.endpoints,
	];
	const out: UnmfzEndpointCandidate[] = [];
	const seen = new Set<string>();
	for (const bucket of buckets) {
		for (const endpoint of endpointArray(bucket)) {
			const candidate = endpointCandidateFromRecord(endpoint);
			if (!candidate || seen.has(candidate.id)) continue;
			seen.add(candidate.id);
			out.push(candidate);
		}
	}
	return out.sort((a, b) => a.label.localeCompare(b.label));
}

export function endpointCandidateFromRecord(endpoint: Record<string, unknown>): UnmfzEndpointCandidate | null {
	if (!looksLikeUnmfzEndpoint(endpoint)) return null;
	const unmfz = asRecord(endpoint.unmfz);
	const appId =
		stringField(endpoint, "source_app_id") ??
		stringField(endpoint, "sourceAppId") ??
		stringField(endpoint, "app_id") ??
		stringField(endpoint, "appId") ??
		stringField(endpoint, "app") ??
		stringField(endpoint, "id") ??
		"";
	const instanceId =
		stringField(endpoint, "source_instance_id") ??
		stringField(endpoint, "sourceInstanceId") ??
		stringField(endpoint, "instance_id") ??
		stringField(endpoint, "instanceId") ??
		undefined;
	const appDisplayName =
		stringField(endpoint, "source_app_display_name") ??
		stringField(endpoint, "sourceAppDisplayName") ??
		stringField(endpoint, "app_display_name") ??
		stringField(endpoint, "appDisplayName") ??
		stringField(endpoint, "display_name") ??
		stringField(endpoint, "displayName") ??
		undefined;
	const endpointId =
		stringField(endpoint, "endpoint_id") ??
		stringField(endpoint, "endpointId") ??
		stringField(endpoint, "id") ??
		stringField(endpoint, "name") ??
		"unmfz";
	const direction = normalizeEndpointDirection(stringField(endpoint, "direction") ?? "both");
	const baseKeyExpr =
		stringField(endpoint, "base_key_expr") ??
		stringField(endpoint, "baseKeyExpr") ??
		(unmfz ? stringField(unmfz, "base_key_expr") : null) ??
		(unmfz ? stringField(unmfz, "baseKeyExpr") : null) ??
		"un-motion/frame";
	const topicMode = normalizeUnmfzTopicMode(
		stringField(endpoint, "topic_mode") ??
			stringField(endpoint, "topicMode") ??
			(unmfz ? stringField(unmfz, "topic_mode") : null) ??
			(unmfz ? stringField(unmfz, "topicMode") : null) ??
			"frame"
	);
	const schemaMajor =
		numberField(endpoint, "schema_major") ??
		numberField(endpoint, "schemaMajor") ??
		(unmfz ? numberField(unmfz, "schema_major") : null) ??
		(unmfz ? numberField(unmfz, "schemaMajor") : null) ??
		1;
	const label =
		stringField(endpoint, "label") ??
		stringField(endpoint, "title") ??
		([appId, endpointId].filter(Boolean).join(" / ") || baseKeyExpr);
	return {
		id: stringField(endpoint, "candidate_id") ?? `${appId}:${endpointId}:${direction}:${baseKeyExpr}`,
		label,
		appId,
		instanceId,
		appDisplayName,
		endpointId,
		direction,
		baseKeyExpr,
		topicMode,
		schemaMajor: Math.trunc(schemaMajor),
		discoverySource: stringField(endpoint, "discovery_source") ?? stringField(endpoint, "discoverySource") ?? undefined,
		sourceKey: stringField(endpoint, "discovery_source_key") ?? stringField(endpoint, "discoverySourceKey") ?? undefined,
		payloadKind: stringField(endpoint, "discovery_payload_kind") ?? stringField(endpoint, "discoveryPayloadKind") ?? undefined,
		discoveryStale: boolField(endpoint, "discovery_stale") ?? boolField(endpoint, "discoveryStale") ?? undefined,
		discoveryAgeMs: numberField(endpoint, "discovery_age_ms") ?? numberField(endpoint, "discoveryAgeMs") ?? null,
		validationWarnings: endpointValidationWarnings(endpoint),
		endpoint: cloneJsonObject(endpoint),
	};
}

function endpointArray(value: unknown): Record<string, unknown>[] {
	if (Array.isArray(value)) return value.map(asRecord).filter(isRecord);
	const record = asRecord(value);
	if (!record) return [];
	const nested = record.endpoints;
	if (Array.isArray(nested)) return nested.map(asRecord).filter(isRecord);
	return [record];
}

function looksLikeUnmfzEndpoint(endpoint: Record<string, unknown>): boolean {
	const protocol = stringField(endpoint, "protocol") ?? stringField(endpoint, "kind") ?? stringField(endpoint, "type");
	if (protocol) {
		const p = protocol.toLowerCase();
		if (p === "unmfz" || p === "unmf/z") return true;
	}
	return isRecord(asRecord(endpoint.unmfz));
}

function normalizeEndpointDirection(value: string): UnmfzEndpointCandidate["direction"] {
	const v = value.toLowerCase();
	if (v === "input" || v === "output" || v === "both") return v;
	return "both";
}

function normalizeUnmfzTopicMode(value: string): UnmfzEndpointCandidate["topicMode"] {
	const v = value.toLowerCase();
	if (v === "primary_source" || v === "stream_id") return v;
	return "frame";
}

function stringField(record: Record<string, unknown>, key: string): string | null {
	const value = record[key];
	return typeof value === "string" && value.trim() ? value.trim() : null;
}

function numberField(record: Record<string, unknown>, key: string): number | null {
	const value = record[key];
	if (typeof value === "number" && Number.isFinite(value)) return value;
	if (typeof value === "string") {
		const parsed = Number(value);
		if (Number.isFinite(parsed)) return parsed;
	}
	return null;
}

function boolField(record: Record<string, unknown>, key: string): boolean | null {
	const value = record[key];
	if (typeof value === "boolean") return value;
	if (typeof value === "string") {
		const normalized = value.trim().toLowerCase();
		if (["1", "true", "yes", "on"].includes(normalized)) return true;
		if (["0", "false", "no", "off"].includes(normalized)) return false;
	}
	return null;
}

function endpointValidationWarnings(endpoint: Record<string, unknown>): string[] {
	const raw = endpoint.validation_warnings ?? endpoint.validationWarnings;
	if (!Array.isArray(raw)) return [];
	const warnings: string[] = [];
	for (const item of raw) {
		if (typeof item === "string" && item.trim()) {
			warnings.push(item.trim());
			continue;
		}
		const record = asRecord(item);
		const code = record ? stringField(record, "code") : null;
		const message = record ? stringField(record, "message") : null;
		if (message) warnings.push(code ? `${code}: ${message}` : message);
	}
	return warnings;
}

function asRecord(value: unknown): Record<string, unknown> | null {
	return value !== null && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function isRecord(value: Record<string, unknown> | null): value is Record<string, unknown> {
	return value !== null;
}

function cloneJsonObject(value: Record<string, unknown>): Record<string, unknown> {
	return JSON.parse(JSON.stringify(value)) as Record<string, unknown>;
}

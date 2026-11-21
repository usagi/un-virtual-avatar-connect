import { ControlApiError, type ModeTransitionPlan, type RuntimeModeSummary } from "../types";

export type DisplayMode = {
	id: string;
	label: string;
	description: string;
};

export type ManagedDirectiveRow = {
	label: string;
	values: string[];
};

export type CapabilityPolicyRow = {
	label: string;
	values: string[];
};

export type CapabilityPolicyWarning = {
	kind: "deny" | "allow";
	capability: string;
};

export function buildDisplayModes(configuredModes: RuntimeModeSummary[]): DisplayMode[] {
	return configuredModes.map((mode) => ({
		id: mode.id,
		label: mode.display_name?.trim() || mode.id,
		description: "このプロファイルで定義された動作モード。",
	}));
}

export function managedDirectiveRows(plan: ModeTransitionPlan | null): ManagedDirectiveRow[] {
	if (!plan) return [];
	return [
		{ label: "Start", values: plan.target_managed_apps.start },
		{ label: "Stop", values: plan.target_managed_apps.stop },
		{ label: "Minimize", values: plan.target_managed_apps.minimize },
		{ label: "Leave", values: plan.target_managed_apps.leave },
	].filter((row) => row.values.length > 0);
}

export function capabilityPolicyRows(plan: ModeTransitionPlan | null): CapabilityPolicyRow[] {
	if (!plan) return [];
	return [
		{ label: "Allow", values: plan.target_capability_policy.allow },
		{ label: "Deny", values: plan.target_capability_policy.deny },
	].filter((row) => row.values.length > 0);
}

export function capabilityPolicyWarnings(plan: ModeTransitionPlan | null): CapabilityPolicyWarning[] {
	if (!plan) return [];
	return [
		...plan.capability_denied_by_target.map((capability) => ({
			kind: "deny" as const,
			capability,
		})),
		...plan.capability_unlisted_by_target.map((capability) => ({
			kind: "allow" as const,
			capability,
		})),
	];
}

export function formatControlApiError(e: unknown): string {
	if (e instanceof ControlApiError) return `${e.status} ${e.statusText}`;
	if (e instanceof Error) return e.message;
	return String(e);
}

export function joinList(values: string[]): string {
	return values.length > 0 ? values.join(", ") : "-";
}

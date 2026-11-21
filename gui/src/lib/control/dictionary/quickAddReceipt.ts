import type { TriggerNodeResponse } from "../../types";

export type DictionaryOutputObservation = {
    node_id: string;
    port: string;
    outcome: "saved" | "unchanged" | "conflict" | "failed";
};
export type DictionaryRequestOutcome = "accepted" | "pending" | "unknown" | "saved" | "unchanged" | "conflict" | "failed";

/** A missing/expired observation must never become a successful save. */
export function dictionaryRequestOutcome(
    response: TriggerNodeResponse,
    observations: readonly DictionaryOutputObservation[],
): DictionaryRequestOutcome {
    if (!observations.length) return "accepted";
    const receipt = response.receipt;
    if (!receipt) return "unknown";
    if (receipt.phase === "pending" || receipt.phase === "running") return "pending";
    const matched = new Set(observations.filter(rule =>
        receipt.outputs.some(([node, port]) => node === rule.node_id && port === rule.port),
    ).map(rule => rule.outcome));
    if (matched.has("conflict")) return "conflict";
    if (receipt.phase === "failed" || matched.has("failed")) return "failed";
    // Contradictory configured terminal outcomes cannot certify a save.
    if (matched.has("saved") && matched.has("unchanged")) return "unknown";
    if (matched.has("saved")) return "saved";
    if (matched.has("unchanged")) return "unchanged";
    return "unknown";
}

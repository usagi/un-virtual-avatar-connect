import type { FlowgraphFileDocument } from "../types";

export type SetupField = { label: string; node_id: string; feature: string; property: string };
export type ResolvedSetupField = SetupField & { value: unknown };

/** Planned address only: routes are fixed at process startup, not on graph reload. */
export function plannedWebInput(fq: string, properties: Record<string, unknown>) {
    const raw = typeof properties.path === "string" ? properties.path.trim() : "";
    const path = raw ? (raw.startsWith("/") ? raw : "/" + raw).replace(/\/+$/, "") || "/"
        : `/input/flowgraph/${Array.from(fq).map(c => /^[A-Za-z0-9_-]$/.test(c) ? c : "_").join("")}`;
    const requested = typeof properties.method === "string" ? properties.method.trim().toUpperCase() : "POST";
    const method = ["GET", "POST", "PUT"].includes(requested) ? requested : "POST";
    return { path, method };
}

/** Read-only binding for guided setup; the Studio remains the editor of the same graph. */
export function resolveSetupFields(
    document: FlowgraphFileDocument,
    template: FlowgraphFileDocument,
    fields: readonly SetupField[],
): { fields: ResolvedSetupField[]; reason: string | null } {
    const reject = (reason: string) => ({ fields: [], reason });
    const signature = (doc: FlowgraphFileDocument) => JSON.stringify({
        nodes: doc.nodes.map(n => [n.id, n.feature]).sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))),
        edges: doc.edges.map(e => [e.from, e.to]).sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))),
        graph: { kind: doc.graph?.kind ?? "entry", inputs: doc.graph?.inputs ?? [], outputs: doc.graph?.outputs ?? [] },
        instances: doc.instances ?? [],
    });
    if (new Set(document.nodes.map(n => n.id)).size !== document.nodes.length) return reject("ノードIDが重複しています。Studioで構成を確認してください。");
    if (signature(document) !== signature(template)) return reject("配線またはノード構成が変更されています。Studioで設定してください。");
    const resolved: ResolvedSetupField[] = [];
    for (const field of fields) {
        const node = document.nodes.find(n => n.id === field.node_id && n.feature === field.feature);
        const baseline = template.nodes.find(n => n.id === field.node_id && n.feature === field.feature);
        const value = node?.properties[field.property];
        const original = baseline?.properties[field.property];
        if (value === undefined || original === undefined) return reject("設定項目が見つかりません。Studioで構成を確認してください。");
        const kind = (v: unknown) => Array.isArray(v) ? "array" : v === null ? "null" : typeof v;
        if (kind(value) !== kind(original)) return reject("設定値の型が変更されています。Studioで確認してください。");
        resolved.push({ ...field, value: structuredClone(value) });
    }
    return { fields: resolved, reason: null };
}

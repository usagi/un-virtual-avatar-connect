import type { FlowgraphDiagnosticsResponse, FlowgraphTreeFileEntry } from "../types";

type RuntimeEvidence = Pick<FlowgraphDiagnosticsResponse, "root_dir" | "ok" | "composition" | "source_digests"> & Partial<Pick<FlowgraphDiagnosticsResponse, "diagnostics" | "runtime_issues">>;
export function featureRuntimeStatus(file: Pick<FlowgraphTreeFileEntry, "fq" | "parse_error" | "source_digest">, root: string, runtime: RuntimeEvidence | null) {
	if (file.parse_error) return { label: "保存内容にエラー", detail: file.parse_error, attention: true };
	if (!runtime) return { label: "実行構成を確認できません", detail: "保存された内容を表示しています。", attention: true };
	const normalize = (path: string) => path.replaceAll("\\", "/").replace(/\/+$/, "");
	if (normalize(root) !== normalize(runtime.root_dir)) return { label: "別の保存先を実行中", detail: "この一覧の保存先と実行構成の取得元が異なります。", attention: true };
	if (!runtime.ok) {
		// A failed readiness check alone does not prove a graph error. Keep
		// unavailable execution evidence distinct from an actual error report.
		const diagnostics = runtime.diagnostics;
		const issues = runtime.runtime_issues;
		if (diagnostics && issues && ![...diagnostics, ...issues].some(item => item.severity === "error")) {
			return { label: "実行構成を確認できません", detail: issues[0]?.message ?? diagnostics[0]?.message ?? "実行構成の確認が完了していません。診断を確認してください。", attention: true };
		}
		return { label: "実行構成にエラー", detail: "診断で原因を確認してください。", attention: true };
	}
	if (!runtime.composition.graphs.some((graph) => graph.fq === file.fq)) return { label: "実行構成に未反映", detail: "保存済みですが、取得した実行構成には含まれていません。", attention: true };
	const loadedDigest = runtime.source_digests?.[file.fq];
	if (loadedDigest && file.source_digest) {
		if (loadedDigest !== file.source_digest) return { label: "保存後の変更が未反映", detail: "実行構成に読み込まれた内容と、このファイルの保存内容が異なります。", attention: true };
		return { label: "保存内容と読込内容が一致", detail: "取得時点のこのファイルを照合しました。依存ファイル・接続・イベントの実行結果は含みません。", attention: false };
	}
	return { label: "実行構成に読込済み", detail: "保存後の変更が適用済みか、接続やイベントが正常かは別途確認が必要です。", attention: false };
}

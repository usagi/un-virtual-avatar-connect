import type { FlowgraphSharePlan } from '../types';
export const unverifiedLabels: Record<string,string> = {
 node_specific_types_and_enums:'ノード固有の型・列挙型',
 assets_and_dynamic_references:'画像・音声素材と動的な参照',
 credentials_and_private_content:'認証情報・個人情報の混入確認',
 full_graph_validation:'構成全体の整合性',
 immutable_export_snapshot:'確認した内容と書き出す内容の一致',
 import_and_runtime_verification:'別環境への取込・動作確認',
};
const referenceErrorGuidance: Record<string,string> = {
 'Invalid package ID':'パッケージIDの形式が正しくありません。定義と参照側のIDを確認してください。',
 'Unsupported version requirement':'対応していないバージョン指定です。対応形式は完全一致のバージョン番号、または * です。',
 'Package depends on itself':'パッケージが自分自身を依存先に指定しています。依存先のIDを確認してください。',
 'Package is missing':'必要なパッケージが保存先にありません。依存先のパッケージを追加してから確認し直してください。',
 'Package ID has multiple definitions':'同じパッケージIDの定義が複数あり、参照先を選べません。重複した定義を確認してください。',
 'Invalid package version':'パッケージのバージョン番号が正しくありません。依存先の定義を確認してください。',
 'Record schema is missing':'参照しているレコード型の定義がありません。その型を定義した構成ファイルを保存先に追加してください。',
 'Record schema has multiple definitions':'同じレコード型の定義が複数あり、参照先を選べません。重複した型定義を確認してください。',
};
export function explainShareReferenceError(error: string | null | undefined): string | null | undefined {
 if(!error) return error;
 const guidance=referenceErrorGuidance[error]
  ?? (error.startsWith('Version mismatch:') ? '必要なバージョンと、保存先にあるパッケージのバージョンが一致していません。下記の required（必要）と found（現在）を確認してください。' : undefined)
  ?? (error.startsWith('Graph file not found:') ? '参照先の構成ファイルが見つかりません。ファイルの場所と参照に書かれたパスを確認してください。' : undefined);
 return guidance ? `${guidance}（${error}）` : error;
}
export function sharePlanReasons(plan:FlowgraphSharePlan){return [
 ...plan.references.map(r=>({from:r.from_file,field:`グラフ ${r.instance_id}`,raw:r.graph,target:r.target,error:r.error})),
 ...plan.package_references.map(r=>({from:r.from_file,field:'パッケージ',raw:`${r.package_id} · ${r.requirement}`,target:r.target,error:r.error})),
 ...plan.static_references.map(r=>({from:r.from_file,field:r.field,raw:r.raw,target:r.target,error:r.error})),
 ...plan.type_references.map(r=>({from:r.from_file,field:r.field,raw:r.expression,target:r.target,error:r.error})),
].map(reason=>({...reason,error:explainShareReferenceError(reason.error)}));}

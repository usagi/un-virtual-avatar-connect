import type { FlowgraphDiagnosticsResponse } from '../../types';

export type SpeechRoute = NonNullable<FlowgraphDiagnosticsResponse['speech_routes']>[number];
type Diagnostics = Pick<FlowgraphDiagnosticsResponse,'ok'|'root_dir'|'speech_routes'|'source_digests'|'inactive_exec_nodes'>;
export type SpeechRouteView =
    | {kind:'unavailable'|'invalid';message:string}
    | {kind:'ready';root:string;routes:Array<SpeechRoute & {modeSuppressed:boolean}>;digests:Record<string,string>|null};

export function speechRouteView(value:Diagnostics):SpeechRouteView {
    if (!value.ok) return {kind:'invalid',message:'Flowgraphの読込に問題があります。接続先がない状態とは区別して、診断を確認してください。'};
    if (!Array.isArray(value.speech_routes)) return {kind:'unavailable',message:'このVACは読み上げ先の接続情報を提供していません。既存のStudioから構成を確認できます。'};
    const inactive=new Set(value.inactive_exec_nodes);
    return {kind:'ready',root:value.root_dir,
        routes:value.speech_routes.map(route=>({...route,incoming:route.incoming.map(link=>({...link})),modeSuppressed:inactive.has(route.runtime_node)})),
        digests:value.source_digests?{...value.source_digests}:null};
}

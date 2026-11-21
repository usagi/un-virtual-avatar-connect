import type { SpeechRouteView, SpeechRoute } from './speechRouteView';

type Link = SpeechRoute['incoming'][number];
export type SpeechConnectionPlan = {
    destination: string;
    affected: string[];
    paths: Array<{destination:string;text:Link[];triggers:Link[];retained:Link[]}>;
    blockers: string[];
};

// Explanatory plan only. Resolved runtime edges cannot be written back as source
// edges: module boundaries, imports and the current on-disk revisions need a
// separate backend transaction. Never treat this plan as write authorization.
export function speechConnectionPlan(view:SpeechRouteView, destination:string):SpeechConnectionPlan {
    const result:SpeechConnectionPlan={destination,affected:[],paths:[],blockers:[]};
    if(view.kind!=='ready') {result.blockers.push('読み上げ先の情報を取得し直してください。');return result;}
    const selected=view.routes.find(route=>route.runtime_node===destination);
    if(!selected) {result.blockers.push('選んだ読み上げ先が現在の一覧にありません。');return result;}
    const local=(node:string)=>node.slice(node.indexOf('::')+2);
    const affected=view.routes.filter(route=>route.source_file===selected.source_file && local(route.runtime_node)===local(selected.runtime_node));
    result.affected=affected.map(route=>route.runtime_node);
    if(affected.length!==selected.definition_instances) result.blockers.push('共有定義の使用先をすべて確認できていません。');
    if(!view.digests || Object.keys(view.digests).length===0) result.blockers.push('読み込んだ構成の版情報がありません。');
    for(const route of affected) {
        const copy=(links:Link[])=>links.map(link=>({...link}));
        const text=route.incoming.filter(link=>!link.exec && link.to_port==='text');
        const triggers=route.incoming.filter(link=>link.exec && link.to_port==='exec_in');
        result.paths.push({destination:route.runtime_node,text:copy(text),triggers:copy(triggers),
            retained:copy(route.incoming.filter(link=>!text.includes(link)&&!triggers.includes(link)))});
        if(text.length!==1) result.blockers.push(`${route.runtime_node}：文章の接続元を一つに特定できません。既存の上書き入力やStudioの構成を確認する必要があります。`);
        if(triggers.length===0) result.blockers.push(`${route.runtime_node}：実行の接続元がありません。直接呼出の引数を含めた確認が必要です。`);
    }
    return result;
}

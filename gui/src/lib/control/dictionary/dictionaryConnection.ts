export interface DictionaryConnectionInput {
    expected_source:string; revision:string; dictionary_key:string; destination:string;
    failure_policy:'skip_speech'|'speak_original';
}
export interface DictionaryConnectionPreview {
    context:{profile_source:string;profile_revision:string;graph_root:string;dictionary_key:string;dictionary_path:string;target_graph:string;target_node:string;failure:DictionaryConnectionInput['failure_policy']};
    review_revision:string;dictionary_revision:string;affected_destinations:string[];
    changes:Record<string,{original:string;proposed:string;edge_inputs:number;boundary_inputs:number}>;
    basis:'saved_configuration';graph_validated:true;saved:false;runtime_applied:false;
}
export interface DictionaryConnectionSaveInput extends DictionaryConnectionInput {
    review_revision:string;dictionary_revision:string;confirm_shared_definition:boolean;
}
export interface DictionaryConnectionSaved {
    profile_source:string;graph_root:string;transaction_id:string;changed_files:string[];
    affected_destinations:string[];saved:true;runtime_applied:false;
}
export interface DictionaryConnectionRecoveryInput {expected_source:string;revision:string}
export interface DictionaryConnectionRecovery {
    profile_source:string;profile_revision:string;graph_root:string;
    pending:null|{transaction_id:string;changed_files:string[];conflicting_files:string[];restorable:boolean};
    restored:boolean;runtime_applied:false;
}
export const connectionInputKey=(input:DictionaryConnectionInput)=>JSON.stringify([
    input.expected_source,input.revision,input.dictionary_key,input.destination,input.failure_policy,
]);
export async function prepareConnectionPreview(input:DictionaryConnectionInput,load:(input:DictionaryConnectionInput)=>Promise<DictionaryConnectionPreview>) {
    const submitted={...input};
    const preview=await load({...submitted});
    const context=preview.context;
    if(context.profile_source!==submitted.expected_source || context.profile_revision!==submitted.revision
        || context.dictionary_key!==submitted.dictionary_key || context.failure!==submitted.failure_policy
        || !preview.affected_destinations.includes(submitted.destination)
        || preview.basis!=='saved_configuration' || preview.graph_validated!==true
        || preview.saved!==false || preview.runtime_applied!==false || !preview.review_revision) {
        throw new Error('接続案の対象または状態が一致しません。入力を保持して確認し直してください。');
    }
    return {input:submitted,preview};
}

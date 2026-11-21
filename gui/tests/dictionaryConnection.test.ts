import test from 'node:test';
import assert from 'node:assert/strict';
import {prepareConnectionPreview,connectionInputKey,type DictionaryConnectionInput,type DictionaryConnectionPreview} from '../src/lib/control/dictionary/dictionaryConnection.ts';
const input=():DictionaryConnectionInput=>({expected_source:'a/conf.toml',revision:'v1',dictionary_key:'reading',destination:'voice::speaker',failure_policy:'skip_speech'});
const reply=(request:DictionaryConnectionInput):DictionaryConnectionPreview=>({context:{profile_source:request.expected_source,profile_revision:request.revision,graph_root:'a/graphs',dictionary_key:request.dictionary_key,dictionary_path:'a/reading.tsv',target_graph:'voice',target_node:'speaker',failure:request.failure_policy},review_revision:'r1',dictionary_revision:'d1',affected_destinations:[request.destination],changes:{},basis:'saved_configuration',graph_validated:true,saved:false,runtime_applied:false});
test('late connection preview keeps submitted identity rather than changed form',async()=>{
    const request=input();let resolve!:(value:DictionaryConnectionPreview)=>void;
    const pending=prepareConnectionPreview(request,()=>new Promise(r=>resolve=r));
    const original={...request};request.dictionary_key='other';
    resolve(reply(original));const result=await pending;
    assert.equal(result.input.dictionary_key,'reading');
    assert.notEqual(connectionInputKey(result.input),connectionInputKey(request));
});
test('wrong profile, destination or applied response cannot become a review',async()=>{
    for(const mutate of [
        (value:DictionaryConnectionPreview)=>{value.context.profile_source='other/conf.toml';},
        (value:DictionaryConnectionPreview)=>{value.affected_destinations=['other::speaker'];},
        (value:DictionaryConnectionPreview)=>{Object.assign(value,{runtime_applied:true});},
    ]) {
        const request=input(),value=reply(request);mutate(value);
        await assert.rejects(prepareConnectionPreview(request,async()=>value));
    }
});

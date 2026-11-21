import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/RuntimeModeControl.svelte',import.meta.url),'utf8');
const ast=ts.createSourceFile('mode.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const names=['refreshModes','refreshTransitionStatus','refreshTransitionPlan'];
const code=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function fixture(){
 const calls={};const api=Object.fromEntries(['modesList','currentMode','modeTransition','modePlan'].map(k=>[k,()=>new Promise((resolve,reject)=>(calls[k]??=[]).push({resolve,reject}))]));
 const h=new Function('api',`let loading=false,loadError=null,configuredModes=[],currentModeId=null,selectedModeId=null,transitionStatus=null,transitionPlan=null,managedAppOps=[],showTransitionFeedback=false,completionHideTimer=null,modesRequestSeq=0,statusRequestSeq=0,planRequestSeq=0,planLoading=false,planError=null; const formatControlApiError=e=>e.message;${code};return {${names.join(',')},state:()=>({loading,loadError,currentModeId,selectedModeId,transitionStatus,transitionPlan,planError})}`)(api);
 return {...h,calls};
}
test('late old mode list cannot replace newer current mode',async()=>{
 const h=fixture();const a=h.refreshModes(),b=h.refreshModes();
 h.calls.modesList[1].resolve({modes:[{id:'streaming'}]});h.calls.currentMode[1].resolve({mode:'streaming'});await b;
 h.calls.modesList[0].resolve({modes:[{id:'desktop'}]});h.calls.currentMode[0].resolve({mode:'desktop'});await a;
 assert.equal(h.state().currentModeId,'streaming');assert.equal(h.state().selectedModeId,'streaming');
});
test('old mode load failure cannot hide newer success',async()=>{
 const h=fixture();const a=h.refreshModes(),b=h.refreshModes();
 h.calls.modesList[1].resolve({modes:[]});h.calls.currentMode[1].resolve({mode:'streaming'});await b;
 h.calls.modesList[0].reject(Error('old failure'));h.calls.currentMode[0].resolve({mode:'desktop'});await a;
 assert.equal(h.state().loadError,null);
});
test('late completed status cannot replace newer active transition',async()=>{
 const h=fixture();const a=h.refreshTransitionStatus(),b=h.refreshTransitionStatus();
 h.calls.modeTransition[1].resolve({active:true,phase:'running'});await b;
 h.calls.modeTransition[0].resolve({active:false,phase:'completed',managed_apps:[]});await a;
 assert.equal(h.state().transitionStatus.active,true);
});

test('completed transition history cannot overwrite selected target preview',async()=>{
 const h=fixture();const selected={to_effective_id:'streaming',target_flowgraph_groups:{enable:['chat'],disable:[]}};
 const preview=h.refreshTransitionPlan('streaming');h.calls.modePlan[0].resolve(selected);await preview;
 const poll=h.refreshTransitionStatus();h.calls.modeTransition[0].resolve({active:false,phase:'completed',managed_apps:[],plan:{to_effective_id:'desktop'}});await poll;
 assert.deepEqual(h.state().transitionPlan,selected);
 assert.equal(h.state().transitionStatus.phase,'completed');
});
test('changing target clears previous details while new preview is pending',async()=>{
 const h=fixture();const a=h.refreshTransitionPlan('desktop');h.calls.modePlan[0].resolve({to_effective_id:'desktop'});await a;
 const b=h.refreshTransitionPlan('streaming');assert.equal(h.state().transitionPlan,null);
 h.calls.modePlan[1].reject(Error('unavailable'));await b;
 assert.equal(h.state().transitionPlan,null);assert.equal(h.state().planError,'unavailable');
});

test('failed mode load can retry while preserving the selected destination',async()=>{
 const h=fixture();const initial=h.refreshModes();
 h.calls.modesList[0].resolve({modes:[{id:'streaming'}]});h.calls.currentMode[0].resolve({mode:'streaming'});await initial;
 const failed=h.refreshModes(true);h.calls.modesList[1].reject(Error('offline'));h.calls.currentMode[1].resolve({mode:'desktop'});await failed;
 assert.equal(h.state().loadError,'offline');assert.equal(h.state().selectedModeId,'streaming');
 const retry=h.refreshModes(true);h.calls.modesList[2].resolve({modes:[{id:'streaming'},{id:'desktop'}]});h.calls.currentMode[2].resolve({mode:'desktop'});await retry;
 assert.equal(h.state().loadError,null);assert.equal(h.state().loading,false);
 assert.equal(h.state().currentModeId,'desktop');assert.equal(h.state().selectedModeId,'streaming');
});

test('failed preview can retry the same target without requesting a mode change',async()=>{
 const h=fixture();const failed=h.refreshTransitionPlan('streaming');h.calls.modePlan[0].reject(Error('offline'));await failed;
 const retry=h.refreshTransitionPlan('streaming');h.calls.modePlan[1].resolve({to_effective_id:'streaming'});await retry;
 assert.equal(h.state().planError,null);assert.equal(h.state().transitionPlan.to_effective_id,'streaming');
});

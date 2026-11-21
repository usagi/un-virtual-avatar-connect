import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/lib/QuickEffectsPanel.svelte',import.meta.url),'utf8');
function body(name){const start=source.indexOf(`\tasync function ${name}(`);assert.ok(start>=0);return source.slice(start,source.indexOf('\n\t}',start)+3);}
function harness(errorCode = "config_changed"){
 let fail=true,conflict=false;const writes=[];class ApiError extends Error {status=409;statusText='Conflict';body={error:errorCode};}
 const api={quickEffectPresets:async()=>{if(fail)throw Error('offline');return {revision:'read-revision',presets:[{id:'existing',label:'Keep'}]};},putQuickEffectPresets:async req=>{writes.push(req);if(conflict)throw new ApiError('changed');return {...req,revision:'saved-revision'};}};
 const code=`let fallbackPresets=[{id:'sample'}],presets=fallbackPresets,presetError=null,presetsReady=false,presetRevision=null,presetsLoading=false,busy=null;
 let customLabel='new',channel='effect',content='retained input',flagsText='',durationMs=5000;
 const presetIdFromLabel=s=>s,parseFlags=()=>[];
 ${body('refreshPresets')} ${body('saveCustomPreset')}
 return {refreshPresets,saveCustomPreset,state:()=>({content,presets,presetError,presetsReady})};`;
 const js=ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
 return {...new Function('api','ControlApiError','toastStore',js)(api,ApiError,{error(){},success(){}}),writes,recover:()=>fail=false,conflict:()=>conflict=true};
}
test('preset save cannot replace unread remote configuration with fallback samples',async()=>{
 const h=harness();await h.saveCustomPreset();assert.equal(h.writes.length,0);
});
test('failed preset read retains custom input and prevents saving fallback samples',async()=>{
 const h=harness();await h.refreshPresets();await h.saveCustomPreset();
 assert.equal(h.writes.length,0);assert.equal(h.state().content,'retained input');
});
test('retry reads existing presets before adding the retained custom entry',async()=>{
 const h=harness();await h.refreshPresets();h.recover();await h.refreshPresets();await h.saveCustomPreset();
 assert.deepEqual(h.writes[0].presets.map(p=>p.id),['existing','new']);
});

test('save submits read revision and conflict retains draft until explicit re-read',async()=>{
 const h=harness();h.recover();await h.refreshPresets();h.conflict();await h.saveCustomPreset();
 assert.equal(h.writes[0].expected_revision,'read-revision');assert.equal(h.state().content,'retained input');
 assert.equal(h.state().presetsReady,false);await h.saveCustomPreset();assert.equal(h.writes.length,1);
});

 test('busy save retains draft and explains contention without claiming data changed',async()=>{
 const h=harness('profile_busy');h.recover();await h.refreshPresets();h.conflict();await h.saveCustomPreset();
 assert.match(h.state().presetError,/別の保存処理が進行中/);
 assert.equal(h.state().content,'retained input');assert.equal(h.state().presetsReady,false);
 await h.saveCustomPreset();assert.equal(h.writes.length,1);
 });

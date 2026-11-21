import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraph/FlowgraphRecoveryPreview.svelte',import.meta.url),'utf8').match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
const ast=ts.createSourceFile('panel.ts',source,ts.ScriptTarget.Latest,true);
const body=ast.statements.filter(n=>!ts.isImportDeclaration(n)).map(n=>n.getText(ast)).join('\n');
const code=ts.transpileModule(body,{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const effects=[];
let finishRestore,finishHistory;
const store={tree:{root_dir:'A'},mutating:false,isDirty:false,hasDraftForFile:()=>false,restoreFile:()=>new Promise(r=>finishRestore=r)};
const api={flowgraphRecoveryHistory:()=>new Promise(r=>finishHistory=r)};
const panel=new Function('api','flowgraphStore','$state','$derived','$props','$effect','tabNavStore',`${code};return {restore,refresh,inspect,seed(value){plan=value},state(){return {history,plan,result,error,busy}}}`)(api,store,x=>x,x=>x,()=>({}),fn=>effects.push(fn),{featureFocus:null});
const effect=()=>effects.forEach(fn=>fn());
effect();
panel.seed({fq:'main',root_dir:'A'});
let pending=panel.restore();
finishRestore({ok:true});await Promise.resolve();
assert.match(panel.state().result,/復元/);
finishHistory({root_dir:'A',entries:[{fq:'main',current_present:true}]});await pending;
assert.equal(panel.state().history.entries[0].current_present,true);
panel.seed({fq:'main',root_dir:'A'});
pending=panel.restore();
store.tree={root_dir:'B'};effect();
finishRestore({ok:true});await pending;
assert.deepEqual(panel.state(),{history:null,plan:null,result:'',error:'',busy:false});
// A failed history refresh must not disguise a completed restoration as failed.
panel.seed({fq:'main',root_dir:'B'});
api.flowgraphRecoveryHistory=async()=>{throw Error('offline')};
pending=panel.restore();finishRestore({ok:true});await pending;
assert.match(panel.state().result,/復元・適用済み/);
assert.match(panel.state().error,/履歴を更新できません/);
assert.equal(panel.state().history,null);
assert.equal(panel.state().busy,false);
console.log('PASS: actual recovery handlers refresh history; delayed restore cannot publish across roots; history failure preserves successful restore result');
// A history response from another root must never become an actionable list.
api.flowgraphRecoveryHistory=async()=>({root_dir:'other-root',entries:[{fq:'foreign'}]});
await panel.refresh();
assert.equal(panel.state().history,null);
assert.match(panel.state().error,/保存先が変わりました/);
// Matching-root history remains usable after the rejected response.
api.flowgraphRecoveryHistory=async()=>({root_dir:'B',entries:[]});
await panel.refresh();
assert.equal(panel.state().history.root_dir,'B');
assert.equal(panel.state().error,'');
console.log('PASS: mismatched history root rejected; matching-root retry recovers');

// A second action while restoration is pending must not clear its reviewed
// plan, supersede its generation or hide the eventual save result.
let historyCalls=0, inspectionCalls=0;
api.flowgraphRecoveryHistory=async()=>{historyCalls++;return {root_dir:'B',entries:[]}};
api.flowgraphRecoveryPlan=async()=>{inspectionCalls++;return {fq:'other',root_dir:'B'}};
panel.seed({fq:'main',root_dir:'B'});
pending=panel.restore();
await panel.refresh();await panel.inspect('other','backup');
assert.equal(historyCalls,0,'pending restore must not start a competing history refresh');
assert.equal(inspectionCalls,0,'pending restore must not replace its reviewed plan');
assert.equal(panel.state().plan.fq,'main');
finishRestore({ok:true});await pending;
assert.match(panel.state().result,/復元・適用済み/);
assert.equal(historyCalls,1,'completed restore refreshes its own history');
console.log('PASS: pending restore owns its plan and result until completion');

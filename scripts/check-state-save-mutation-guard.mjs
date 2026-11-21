import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraph/FlowgraphDiagnostics.svelte',import.meta.url),'utf8').match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
const ast=ts.createSourceFile('diagnostics.ts',source,ts.ScriptTarget.Latest,true);
const names=['onSaveLoadedStateSnapshot','onSaveLiveStateSnapshot'];
const methods=ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name.text));
assert.equal(methods.length,2);
const code=ts.transpileModule(methods.map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
for(const name of names)for(const fail of [false,true]){
 const calls=[],messages=[];
 const api=Object.fromEntries(['flowgraphSaveLoadedStateSnapshot','flowgraphSaveLiveStateSnapshot'].map(n=>[n,()=>new Promise((resolve,reject)=>calls.push({n,resolve,reject}))]));
 const store={mutating:true,refreshDiagnostics:async()=>{}};
 const toast=Object.fromEntries(['success','error'].map(n=>[n,(...args)=>messages.push({n,args})]));
 const h=new Function('api','flowgraphStore','toastStore','describeError',`let savingStateSnapshot=false,savingLiveStateSnapshot=false,reloadingPreservingState=false,restoringStateSnapshot=false;${code};return {${names.join(',')}};`)(api,store,toast,String);
 await h[name]();assert.equal(calls.length,0,'other Studio mutation prevents snapshot save');
 store.mutating=false;
 const pending=h[name]();assert.equal(store.mutating,true,'snapshot save guards other Studio mutations');
 for(const other of names)await h[other]();assert.equal(calls.length,1);
 if(fail)calls[0].reject(Error('write failed'));else calls[0].resolve({snapshot_node_count:1,path:'isolated/state.json'});
 await pending;assert.equal(store.mutating,false);
 assert.equal(messages[0].n,fail?'error':'success');
 const retry=h[name]();assert.equal(calls.length,2,'operation can be retried after completion');
 calls[1].resolve({snapshot_node_count:1,path:'isolated/state.json'});await retry;
 assert.equal(store.mutating,false);
}
console.log('PASS: snapshot saves share Studio mutation guard, reject overlap, and release on success/failure for retry');

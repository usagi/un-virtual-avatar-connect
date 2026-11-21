import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraph/FeatureLibrary.svelte',import.meta.url),'utf8');
const ast=ts.createSourceFile('Feature.ts',source.split('<script lang="ts">')[1].split('</script>')[0],ts.ScriptTarget.Latest,true);
const method=ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='install').getText(ast);
const code=ts.transpileModule(`
 const featureZip=()=>new Uint8Array(),identity='a/conf.toml',importRoot='a/graphs';
 let chosen={source:'graph',title:'test'},preview={conflicts:[],target_prefix:'features/test'},busy=false,error='',result='',resultTree=null;
 let purposeSnapshot={root:importRoot},purposeRefresh=0,installIssue=null;
 const api={desktopProfiles:async()=>({current_conf_path:identity})};
 ${method}
 return {run:install,state:()=>({error,result,resultTree,preview})};
`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
for(const namedInstall of [false,true])for(const changeTarget of [false,true]){
 let release;const store={tree:{root_dir:'a/graphs'},isDirty:false,importZip:()=>new Promise(resolve=>release=resolve),refreshTree:async()=>{},refreshDiagnostics:async()=>{}};
 const subject=new Function('flowgraphStore','namedInstall','purposeRequest',code)(store,namedInstall,()=>new Promise(resolve=>release=resolve));
 const pending=subject.run();await new Promise(resolve=>setImmediate(resolve));
 if(changeTarget)store.tree={root_dir:'b/graphs'};
 release({kind:'report',registration:{id:'test'},reload_ok:true,restart_required:false,written_files:['features/test/main']});await pending;
 if(changeTarget){
  assert.equal(subject.state().result,'','old target success must not label new target');
  assert.equal(subject.state().resultTree,null);
  assert.match(subject.state().error,/a\/graphs/);
  assert.equal(subject.state().preview,null,'completed attempt must not remain executable');
 }else assert.match(subject.state().result,/保存.*適用済み/);
}
console.log('Actual install handler: same-root success retained; late completion after root switch cannot label new target or leave repeatable preview. Mock persistence only.');

{
 const store={tree:{root_dir:'a/graphs'},isDirty:false,refreshTree:async()=>{},refreshDiagnostics:async()=>{}};
 const message='用途は保存・登録済みですが、別の操作が同じ保存先を処理中のため適用していません。完了後に再読込してください。';
 let calls=0;
 const subject=new Function('flowgraphStore','namedInstall','purposeRequest',code)(store,true,async()=>{calls++;return {registration:{id:'test'},reload_attempted:false,reload_ok:null,apply_error:message,written_files:['features/test/main']}});
 await subject.run();
 assert.match(subject.state().result,/保存・登録済み。適用は未完了/);
 assert.equal(subject.state().error,message);
 assert.equal(subject.state().preview,null,'saved installation must not leave a repeatable import preview');
 assert.equal(calls,1,'busy apply must not automatically repeat import');
}
console.log('PASS: saved-but-busy installation remains saved/unapplied, exposes recovery guidance and does not reimport');

// Inspect the actual rendering predicate: the follow-up action must have the
// same target lifetime as its result, including changes after completion.
const condition=source.match(/\{#if (chosen[^\n]+?)\}<button[^\n]+?この機能をStudioで編集/)[1];
const visible=new Function('chosen','importRoot','flowgraphStore','result','resultTree','preview',`return Boolean(${condition});`);
const originalTree={root_dir:'a/graphs'};
assert.equal(visible({},'a/graphs',{tree:originalTree},'saved',originalTree,null),true);
assert.equal(visible({},'a/graphs',{tree:{root_dir:'b/graphs'}},'saved',originalTree,null),false,'Completed action must not refer to another store');
assert.equal(visible({},'a/graphs',{tree:{root_dir:'a/graphs'}},'saved',originalTree,null),false,'Refreshed inventory invalidates old result action');
assert.equal(visible({},'a/graphs',{tree:originalTree},'',null,{conflicts:[{}]}),true,'Existing conflicting feature remains editable at the reviewed destination');
assert.equal(visible({},'a/graphs',{tree:{root_dir:'b/graphs'}},'',null,{conflicts:[{}]}),false,'Conflict action must not open the same name at another destination');
console.log('PASS: post-install editor action follows result inventory and reviewed conflict destination');

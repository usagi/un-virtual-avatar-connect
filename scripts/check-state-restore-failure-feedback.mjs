import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraphStore.svelte.ts',import.meta.url),'utf8');
const ast=ts.createSourceFile('store.ts',source,ts.ScriptTarget.Latest,true);
const cls=ast.statements.find(n=>ts.isClassDeclaration(n)&&n.name.text==='FlowgraphStore');
for(const [methodName,apiName] of [['reloadPreservingState','flowgraphReloadPreservingState'],['restoreProfileLocalStateSnapshot','flowgraphRestoreProfileLocalStateSnapshot']]){
 const method=cls.members.find(n=>ts.isMethodDeclaration(n)&&n.name.getText(ast)===methodName);
 const code=ts.transpileModule(`class Subject { ${method.getText(ast)} }`,{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
 for(const ok of [false,true]){
  const messages=[],reads=[];
  const toast=Object.fromEntries(['error','warn','success'].map(level=>[level,(...args)=>messages.push({level,args})]));
  const api={[apiName]:async()=>({ok,restart_required:false,diagnostics:[{message:'snapshot is corrupt'}],path:'isolated/saved.json',saved_node_count:1,restored_node_count:ok?1:null,node_count:2})};
  const Subject=new Function('api','toastStore','describeError',`${code};return Subject;`)(api,toast,String);
  const subject=Object.assign(new Subject(),{mutating:false,currentFq:'main',currentFileRequest:1,currentFile:{root_dir:'isolated'},draft:'retained edit',refreshTree:async()=>reads.push('tree'),refreshDiagnostics:async()=>reads.push('diagnostics'),refreshEditorAfterReload:async()=>reads.push('editor')});
  await subject[methodName]();
  assert.equal(subject.mutating,false);
  assert.equal(subject.draft,'retained edit');
  if(!ok){
   assert.deepEqual(reads,[],'failed restore must not refresh editor or mask the failure with current-runtime diagnostics');
   assert.equal(messages.length,1);assert.equal(messages[0].level,'error');
   assert.match(messages[0].args[1],/snapshot is corrupt/);
   assert.match(messages[0].args[1],/維持/);
  }else{
   assert.deepEqual(reads,['tree','diagnostics','editor']);
   assert.equal(messages[0].level,'success');
  }
 }
}
console.log('PASS: rejected restore retains editor and reports candidate error; successful restore refreshes runtime/editor');

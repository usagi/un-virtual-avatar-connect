import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';

const source = fs.readFileSync(new URL('../gui/src/lib/flowgraph/FlowgraphShareDialog.svelte', import.meta.url), 'utf8').split('<script lang="ts">')[1].split('</script>')[0];
const ast = ts.createSourceFile('dialog.ts', source, ts.ScriptTarget.Latest, true);
const method = ast.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === 'onImportApply');
assert.ok(method);
const code = ts.transpileModule(method.getText(ast), { compilerOptions: { target: ts.ScriptTarget.ESNext } }).outputText;
for (const outcome of [null, {kind:'report'}]) {
 let resolve;
 const calls=[];
 const store={importZip:(file,params)=>{calls.push({file,params});return new Promise(r=>resolve=r);}};
 const h=new Function('flowgraphStore', `let zipFile={name:'feature.zip'},zipPreview={kind:'preview'},zipRoot='root-a',zipBusy=false,companionConflict=false,requestGeneration=0,zipTargetPrefix='imported',zipOnConflictNode='suffix',zipOnConflictFile='rename',closed=false; function close(){closed=true;zipPreview=null;} ${code}; return {apply:onImportApply,state:()=>({zipPreview,zipBusy,closed,zipFile,zipRoot}),review:()=>zipPreview={kind:'preview'}};`)(store);
 const pending=h.apply();
 assert.equal(h.state().zipPreview,null,'dispatch must consume the reviewed plan before any partial write');
 await h.apply();assert.equal(calls.length,1);
 resolve(outcome);await pending;
 assert.equal(h.state().zipBusy,false);
 assert.equal(calls[0].params.expected_root,'root-a');
 await h.apply();assert.equal(calls.length,1,'failure or success cannot reuse the old preview');
 if(outcome===null){
  assert.equal(h.state().closed,false);
  assert.equal(h.state().zipFile.name,'feature.zip');
  h.review();const retry=h.apply();assert.equal(calls.length,2);resolve(null);await retry;
 }
}
console.log('PASS: ZIP apply consumes review, retains failed input, and requires a new preview before retry');

import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const component=fs.readFileSync(new URL('../gui/src/lib/flowgraph/PurposeLibraryPreview.svelte',import.meta.url),'utf8');
const source=component.split('<script lang="ts">')[1].split('</script>')[0];
const ast=ts.createSourceFile('component.ts',source,ts.ScriptTarget.Latest,true);
const fn=ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='reloadFromUser');
assert(fn,'Manual reload needs a draft-preserving handler');
const js=ts.transpileModule(fn.getText(ast),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function harness(pending={}){
 return new Function('pending',`let {busy=false,transferPending=false,recoveryPending=false,studyingPurpose=null,editingId=null,removal=null,name=''}=pending;
 let error='',snapshot={revision:'reviewed',document:{purposes:[{id:'p',name:'old'}]}},calls=0;
 const reload=async()=>{calls++;snapshot={revision:'concurrent',document:{purposes:[{id:'p',name:'another writer'}]}}};
 ${js};return {run:reloadFromUser,state:()=>({calls,snapshot,error,name,editingId})};`)(pending);
}
for(const pending of [{name:'unsaved'},{editingId:'p',name:''},{busy:true},{transferPending:true},{recoveryPending:true},{studyingPurpose:{}},{removal:{}}]){
 const h=harness(pending);await h.run();const s=h.state();
 assert.equal(s.calls,0);assert.equal(s.snapshot.revision,'reviewed');assert.equal(s.name,pending.name??'');
 assert.match(s.error,/編集・確認/);
}
const h=harness();await h.run();assert.equal(h.state().calls,1);assert.equal(h.state().snapshot.revision,'concurrent');
assert.match(component,/onclick=\{\(\)=>void reloadFromUser\(\)\}>用途一覧を再取得/);
console.log('PASS: manual reload preserves pending draft and conflict baseline; reload works after pending work is cleared');

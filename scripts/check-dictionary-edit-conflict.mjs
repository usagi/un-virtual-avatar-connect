import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/control/dictionary/DictionaryEditorPane.svelte',import.meta.url),'utf8');
const ast=ts.createSourceFile('pane.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const code=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='onConflict').map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function fixture() {
 const reads=[];const original={key:'a',path:'a.tsv',content_hash:'old',rows:[]};
 const store={currentKey:'a',currentTable:original,applyTableOverride(dto){this.currentTable=dto;},reloadCurrent(){throw Error('Must not unmount draft');}};
 const api={getControlTable(key){return new Promise((resolve,reject)=>reads.push({key,resolve,reject}));}};
 const h=new Function('dictionaryEditorStore','api',`let conflictBaseRow=null,conflictMyValues={},conflictServerRow=null,conflictOpen=false;const findConflictRow=(base,rows)=>rows[0];${code};return {run:onConflict,state:()=>({conflictOpen,conflictMyValues})};`)(store,api);
 return {...h,reads,store,original};
}
const ctx={mode:{kind:'edit',row:{row_index:0,values:{surface:'old'}}},myValues:{surface:'retained'}};
for(const scenario of ['success','failed-read','foreign-key','foreign-path','selection-changed','same-key-reloaded']) {
 const h=fixture(),p=h.run(ctx),latest={key:'a',path:'a.tsv',content_hash:'new',rows:[{}]};
 assert.equal(h.store.currentTable,h.original);assert.equal(h.state().conflictOpen,false);
 assert.equal(h.reads[0].key,'a');
 if(scenario==='failed-read')h.reads[0].reject(Error('offline'));
 else {
  if(scenario==='foreign-key')latest.key='b';
  if(scenario==='foreign-path')latest.path='b.tsv';
  if(scenario==='selection-changed')h.store.currentKey='b';
  if(scenario==='same-key-reloaded')h.store.currentTable={...h.original};
  h.reads[0].resolve(latest);
 }
 if(scenario==='success') {
  assert.equal(await p,true);assert.equal(h.store.currentTable,latest);
  assert.equal(h.state().conflictOpen,true);assert.deepEqual(h.state().conflictMyValues,ctx.myValues);
 } else {
  await assert.rejects(p);assert.equal(h.state().conflictOpen,false);
  assert.notEqual(h.store.currentTable,latest);
 }
}
console.log('Dictionary edit conflict: no draft-unmount reload; failure and changed target/revision reject publication; successful handoff retains values.');

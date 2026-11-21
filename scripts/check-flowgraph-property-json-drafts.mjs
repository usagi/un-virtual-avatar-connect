import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const typesAst=ts.createSourceFile('types.ts',fs.readFileSync(new URL('../gui/src/lib/types.ts',import.meta.url),'utf8'),ts.ScriptTarget.Latest,true);
const errorClass=typesAst.statements.find(n=>ts.isClassDeclaration(n)&&n.name.text==='ControlApiError').getText(typesAst).replace(/^export /,'');
const ControlApiError=new Function(ts.transpileModule(errorClass,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText+';return ControlApiError;')();
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraphStore.svelte.ts',import.meta.url),'utf8');
const ast=ts.createSourceFile('store.ts',source,ts.ScriptTarget.Latest,true);
const cls=ast.statements.find(n=>ts.isClassDeclaration(n)&&n.name.text==='FlowgraphStore');
const functions=ast.statements.filter(n=>ts.isFunctionDeclaration(n)).map(n=>n.getText(ast).replace(/^export /,'')).join('\n');
const code=ts.transpileModule(`${functions}\n${cls.getText(ast).replace(/^export /,'')}`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
let puts=0;const warnings=[];
const file=(root,fq)=>({root_dir:root,fq,raw_toml:'original',parsed:{nodes:['a','b'].map(id=>({id,feature:'fixture',position:[0,0],properties:{value:{old:true}}})),edges:[],groups:[],instances:[]}});
let activeRoot='root';
const api={flowgraphFile:async fq=>file(activeRoot,fq),flowgraphPutFile:async()=>{puts++;return {ok:true,diagnostics:[]}}};
const Store=new Function('ControlApiError','$state','api','toastStore',code+';return FlowgraphStore;')(ControlApiError,x=>x,api,{warn:(...x)=>warnings.push(x),info(){},error(){},success(){}});
const s=new Store();s.repairOverlappingLayout=()=>false;
assert.equal(await s.openFile('one'),true);
for(const ids of [['a'],['a','b']]) {
 s.editPropertyJson(ids,'value','{"x":');
 assert.equal(s.isDirty,true);assert.equal(s.hasUnsavedDrafts,true);
 assert.equal(s.propertyJsonText(ids,'value'),'{"x":');
 assert.equal(await s.saveCurrent(),false);assert.equal(await s.saveRawToml('replacement'),false);assert.equal(puts,0);
 await s.openFile('two');assert.equal(s.invalidPropertyJsonDrafts.length,0);
 const recovery = JSON.parse(s.workspaceDraftText('root','one'));
 assert.equal(recovery.format,'unvac-property-draft-recovery-v1');
 assert.equal(recovery.editor.propertyJsonDrafts[0].text,'{"x":');
 await s.openFile('one');assert.equal(s.propertyJsonText(ids,'value'),'{"x":');
 s.editPropertyJson(ids,'value','{"x":1}');
 assert.equal(s.invalidPropertyJsonDrafts.length,0);
 for(const id of ids)assert.deepEqual(s.draftNodes.find(n=>n.id===id).properties.value,{x:1});
}
s.editPropertyJson(['a'],'value','broken');
s.editPropertyJson(['b'],'value','also broken');
s.discardPropertyJson('a','value');assert.equal(s.propertyJsonText(['b'],'value'),'also broken');
assert.equal(s.invalidPropertyJsonDrafts.length,1);
await s.openFile('two');activeRoot='other';await s.openFile('one');
assert.equal(s.invalidPropertyJsonDrafts.length,0);assert.equal(s.draftNodes[1].properties.value.old,true);
activeRoot='root';await s.openFile('one');assert.equal(s.propertyJsonText(['b'],'value'),'also broken');
s.discardPropertyJson('b','value');assert.equal(s.invalidPropertyJsonDrafts.length,0);
s.editPropertyJson(['a'],'value','bad');s.removeSelection(['a'],[]);assert.equal(s.invalidPropertyJsonDrafts.length,0);s.undo();assert.equal(s.invalidPropertyJsonDrafts.length,1);
assert.ok(warnings.length>=4);
console.log('PASS: invalid JSON retains single/multiple node input across files and roots, blocks both save routes, supports correction/discard and deletion undo; no API write while invalid.');
s.editPropertyJson(['a'],'value','{"restored":true}');
s.refreshTree=async()=>{};s.refreshDiagnostics=async()=>{};
assert.equal(await s.saveCurrent(),true);assert.equal(puts,1);
assert.equal(await s.saveRawToml('valid raw draft'),true);assert.equal(puts,2);
console.log('PASS: correction re-enables both parsed and raw save paths.');
const numeric = new Store();numeric.repairOverlappingLayout=()=>false;await numeric.openFile('numbers');
for (const integer of [true,false]) {
 for(const text of ['', ' ', '1e', 'Infinity', ...(integer?['1.9','9007199254740992']:[])]) {
  const before=structuredClone(numeric.draftNodes);
  numeric.editPropertyNumber(['a','b'],'value',text,integer);
  assert.deepEqual(numeric.draftNodes,before);
  assert.equal(numeric.invalidPropertyJsonDrafts.length,2);
  assert.equal(await numeric.saveCurrent(),false);
  assert.equal(numeric.propertyJsonText(['a','b'],'value'),text);
 }
 numeric.editPropertyNumber(['a','b'],'value',integer?'1e3':'-1.25',integer);
 assert.equal(numeric.invalidPropertyJsonDrafts.length,0);
 assert.equal(numeric.draftNodes[0].properties.value,integer?1000:-1.25);
}
console.log('PASS: numeric empty/nonfinite/fraction/unsafe integers retain input; valid exponent and decimal values apply without truncation.');
for (const raw of ['null','{"nested":[null]}','1e400','{"nested":[1e400]}']) {
 const before=structuredClone(numeric.draftNodes);
 numeric.editPropertyJson(['a','b'],'value',raw);
 assert.deepEqual(numeric.draftNodes,before);assert.equal(numeric.propertyJsonText(['a','b'],'value'),raw);
 assert.equal(await numeric.saveCurrent(),false);
}
numeric.editPropertyJson(['a','b'],'value','{"ok":true}');assert.equal(numeric.invalidPropertyJsonDrafts.length,0);
const unsupported = new Store();unsupported.repairOverlappingLayout=()=>false;await unsupported.openFile('unsupported');
unsupported.updateNodeProperty('a','value',null);
const writesBefore=puts;assert.equal(await unsupported.saveCurrent(),false);assert.equal(puts,writesBefore);
unsupported.retainCurrentWorkspaceDraft();const rescued=JSON.parse(unsupported.workspaceDraftText('root','unsupported'));
assert.equal(rescued.editor.draftNodes[0].properties.value,null);
console.log('PASS: nested null and nonfinite JSON remain raw drafts; unsupported defaults refuse writes and retain rescue data.');

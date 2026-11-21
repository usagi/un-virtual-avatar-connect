import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
import test from 'node:test';
import assert from 'node:assert/strict';
const src=fs.readFileSync(new URL('../gui/src/lib/tabs/RunWithEditorPanel.svelte',import.meta.url),'utf8').split('<script lang="ts">')[1].split('</script>')[0];
const ast=ts.createSourceFile('panel.ts',src,ts.ScriptTarget.Latest,true);
const methods=['load','addEntry','deleteEntry'].map(name=>ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text===name).getText(ast)).join('\n');
function fixture(){
 const calls=[];const api=Object.fromEntries(['runWithList','runWithAdd','runWithUpdate','runWithDelete'].map(cmd=>[cmd,(...args)=>new Promise((resolve,reject)=>calls.push({cmd,args,resolve,reject}))]));
 const code=ts.transpileModule(`const linkedAppDraft={value:null};const retainDraft=()=>{};let dirty=false,sourceChanged=false,availableModes=[],review=null,loading=false,listRequest=0,error=null,entries=[],revision='old',sourcePath='isolated',submitting=false,busyIndex=null,editingIndex=null,editingRevision=null;
 let draft='retained',resets=0;const toastStore={success(){},error(){},warn(){}};
 const window={confirm:()=>true};const extractMessage=e=>String(e);const buildDto=()=>({command:draft});const resetForm=()=>{draft='';resets++;};
 ${methods}
 return {load,addEntry,deleteEntry,snapshot:()=>({entries,revision,error,loading,submitting,busyIndex,draft,resets})};`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
 return {...new Function('api',code)(api),calls};
}
const response=(name)=>({entries:[{index:0,display_label:name}],revision:name,source_path:'isolated'});
test('late list success cannot replace a completed mutation result',async()=>{
 const f=fixture();const load=f.load();const add=f.addEntry();
 f.calls[1].resolve(response('saved'));await add;
 f.calls[0].resolve(response('stale'));await load;
 assert.equal(f.snapshot().revision,'saved');assert.equal(f.snapshot().entries[0].display_label,'saved');assert.equal(f.snapshot().loading,false);
});
test('only one mutation is sent while save is pending',async()=>{
 const f=fixture();const saving=f.addEntry();
 await f.addEntry();await f.deleteEntry({index:0,display_label:'Other'});await f.load();
 assert.equal(f.calls.length,1);assert.equal(f.calls[0].cmd,'runWithAdd');
 f.calls[0].reject(Error('conflict'));await saving;
 assert.equal(f.snapshot().draft,'retained');assert.equal(f.snapshot().resets,0);assert.equal(f.snapshot().submitting,false);
});
test('delete blocks other mutations and late list errors do not poison success',async()=>{
 const f=fixture();const load=f.load();const deleting=f.deleteEntry({index:0,display_label:'Target'});
 await f.deleteEntry({index:1,display_label:'Other'});await f.addEntry();assert.equal(f.calls.length,2);
 f.calls[1].resolve(response('deleted'));await deleting;f.calls[0].reject(Error('stale failure'));await load;
 assert.equal(f.snapshot().revision,'deleted');assert.equal(f.snapshot().error,null);assert.equal(f.snapshot().draft,'retained');assert.equal(f.snapshot().busyIndex,null);
});

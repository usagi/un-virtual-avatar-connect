import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/tabs/ProfileBackupsPanel.svelte', import.meta.url),'utf8');
const ast=ts.createSourceFile('backups.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const names=['restoreKey','hasDraft','mayLeave','restore','load','requestDiscard','confirmDiscard'];
const code=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function fixture(){
 const calls=[],prompts=[];let restored=0;
 const api={profileRestore:(...args)=>new Promise((resolve,reject)=>calls.push({args,resolve,reject})),profileBackups:()=>{throw Error('unexpected overlapping load')}};
 return {calls,prompts,restored:()=>restored,...new Function('api','onRestored','prompts',`let discardReview=null,selected='',filename='',completedKey='',busy=false,error='',result='',entries=[],directory='',loaded=false;const message=e=>e.message,window={confirm:q=>{prompts.push(q);return false}};${code};return {${names.join(',')},edit:(s,f)=>{selected=s;filename=f},cancel:()=>discardReview=null,setDirectory:d=>directory=d,state:()=>({selected,filename,busy,error,result,discardReview})}`)(api,()=>restored++,prompts)};
}
test('restore blocks departure/reentry; conflict retains target and retry completes',async()=>{
 const h=fixture();assert.equal(h.mayLeave(),true);
 h.edit('old.toml.bak-1',' restored.toml ');assert.equal(h.mayLeave(),false);assert.equal(h.prompts.length,0);
 const pending=h.restore();assert.equal(h.mayLeave(),false);assert.equal(h.prompts.length,0);
 await h.restore();await h.load();assert.equal(h.calls.length,1);
 assert.deepEqual(h.calls[0].args,['old.toml.bak-1','restored.toml']);
 h.calls[0].reject(Error('profile_busy'));await pending;
 assert.equal(h.state().selected,'old.toml.bak-1');assert.equal(h.state().filename,' restored.toml ');assert.equal(h.hasDraft(),true);assert.equal(h.restored(),0);
 const retry=h.restore();h.calls[1].resolve({filename:'restored.toml'});await retry;
 assert.equal(h.hasDraft(),false);assert.equal(h.mayLeave(),true);assert.equal(h.restored(),1);
 h.edit('old.toml.bak-1','another.toml');assert.equal(h.hasDraft(),true);assert.equal(h.mayLeave(),false);
});
test('empty restoration never invokes API',async()=>{const h=fixture();await h.restore();h.edit('backup','  ');await h.restore();assert.equal(h.calls.length,0)});

test('discard is explicit, cancel retains input, stale target refuses, review blocks restore',async()=>{
 const h=fixture();h.edit('backup','restored.toml');h.requestDiscard();
 assert.equal(h.mayLeave(),false);await h.restore();await h.load();assert.equal(h.calls.length,0);
 h.cancel();assert.equal(h.state().filename,'restored.toml');
 h.requestDiscard();h.edit('backup','newer.toml');h.confirmDiscard();assert.equal(h.state().filename,'newer.toml');assert.ok(h.state().error);
 h.requestDiscard();h.setDirectory('changed');h.confirmDiscard();assert.equal(h.state().filename,'newer.toml');
 h.requestDiscard();h.confirmDiscard();assert.equal(h.state().filename,'');assert.equal(h.state().selected,'');assert.equal(h.mayLeave(),true);assert.equal(h.calls.length,0);assert.equal(h.prompts.length,0);
});

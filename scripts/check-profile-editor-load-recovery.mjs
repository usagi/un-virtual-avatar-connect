import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/tabs/ProfileEditorModal.svelte', import.meta.url),'utf8');
const ast=ts.createSourceFile('editor.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const code=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&['load','save'].includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function fixture(){
 const reads=[],writes=[],closes=[];
 const api={profileContent:()=>new Promise((resolve,reject)=>reads.push({resolve,reject})),profilePutContent:(...args)=>new Promise((resolve,reject)=>writes.push({args,resolve,reject}))};
 const h=new Function('api','onClose',`let loading=true,loaded=false,saving=false,originalContent='',revision,sourcePath='',content='',isCurrent=false,size=0,error=null,discardReview=null;const filename='target.toml',toastStore={success(){},warn(){}},extractMessage=e=>e.message;${code};return {load,save,edit:v=>content=v,state:()=>({loading,loaded,saving,content,revision,error})}`)(api,v=>closes.push(v));
 return {...h,reads,writes,closes};
}
test('failed read cannot overwrite unread config; retry obtains revision before save',async()=>{
 const h=fixture();const first=h.load();h.reads[0].reject(Error('read failed'));await first;
 h.edit('replacement');void h.save();assert.equal(h.writes.length,0);assert.equal(h.closes.length,0);
 const retry=h.load();h.reads[1].resolve({content:'original',revision:'rev-1',path:'isolated/target.toml',is_current:false,size:8});await retry;
 h.edit('edited');const save=h.save();assert.deepEqual(h.writes[0].args,['target.toml',{content:'edited',expected_revision:'rev-1'}]);
 h.writes[0].reject(Error('conflict'));await save;assert.equal(h.state().content,'edited');assert.equal(h.closes.length,0);
 const again=h.save();h.writes[1].resolve({});await again;assert.deepEqual(h.closes,[true]);
});
test('a response without revision is not a writable loaded document',async()=>{
 const h=fixture();const read=h.load();h.reads[0].resolve({content:'original',path:'target.toml',size:8});await read;
 h.edit('changed');void h.save();assert.equal(h.writes.length,0);assert.ok(h.state().error);
});

test('in-page discard retains cancelled or changed input and rejects stale targets',async()=>{
 assert(!source.includes('window.confirm('));
 const discardCode=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&['requestClose','cancelDiscard','confirmDiscard'].includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
 const h=new Function(`let filename='target.toml',content='draft',revision='r1',saving=false,error=null,discardReview=null,closes=0;const dirty=true,tick=async()=>{},panel={focus(){}},onClose=()=>closes++;${discardCode};return {requestClose,cancelDiscard,confirmDiscard,edit:v=>content=v,move:v=>filename=v,busy:v=>saving=v,state:()=>({content,closes,discardReview,error})}`)();
 h.requestClose();assert.equal(h.state().closes,0);h.cancelDiscard();assert.equal(h.state().content,'draft');
 h.requestClose();h.edit('newer');h.confirmDiscard();assert.equal(h.state().closes,0);assert.ok(h.state().error);
 h.requestClose();h.move('other.toml');h.confirmDiscard();assert.equal(h.state().closes,0);
 h.requestClose();h.busy(true);h.confirmDiscard();assert.equal(h.state().closes,0);
 h.busy(false);h.confirmDiscard();assert.equal(h.state().closes,1);assert.equal(h.state().discardReview,null);
});

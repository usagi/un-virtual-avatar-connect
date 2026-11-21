import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/lib/QuickEffectsPanel.svelte',import.meta.url),'utf8');
function body(name){
 const start=source.indexOf(`\tasync function ${name}(`);assert.ok(start>=0);
 return source.slice(start,source.indexOf('\n\t}',start)+3);
}
function harness(){
 const pending=[];
 const call=kind=>(...args)=>new Promise((resolve,reject)=>pending.push({kind,args,resolve,reject}));
 const api={quickEffects:call('read'),cancelQuickEffect:call('cancel'),enqueueQuickEffect:call('send')};
 const code=`let queueRequest=0,queue=[{id:'one',status:'active'}],queueError=null,busy=null,content='sent text';
 ${body('refreshQueue')} ${body('cancelEffect')} ${body('send')}
 return {refreshQueue,cancel:()=>cancelEffect(queue[0]),send:()=>send({label:'custom',req:{content:'sent text'}}),edit:text=>content=text,state:()=>({queue,queueError,content})};`;
 const js=ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
 return {...new Function('api','ControlApiError','toastStore',js)(api,class extends Error {},{success(){},error(){}}),pending};
}
for(const fail of [false,true])test(`late queue poll cannot replace cancellation, failure=${fail}`,async()=>{
 const h=harness();const read=h.refreshQueue();const cancel=h.cancel();
 h.pending[1].resolve({id:'one',status:'cancelled'});await cancel;
 if(fail)h.pending[0].reject(Error('old poll failure'));else h.pending[0].resolve({effects:[{id:'one',status:'active'}]});
 await read;assert.equal(h.state().queue[0].status,'cancelled');assert.equal(h.state().queueError,null);
});
test('late send retains next composed text',async()=>{
 const h=harness();const send=h.send();h.edit('next message');h.pending[0].resolve({id:'two',channel:'effect'});await send;
 assert.equal(h.state().content,'next message');
});
test('successful send clears the unchanged submitted text',async()=>{
 const h=harness();const send=h.send();h.pending[0].resolve({id:'two',channel:'effect'});await send;
 assert.equal(h.state().content,'');
});
test('newer queue poll wins when reads complete out of order',async()=>{
 const h=harness();const old=h.refreshQueue();const latest=h.refreshQueue();
 h.pending[1].resolve({effects:[{id:'one',status:'expired'}]});await latest;
 h.pending[0].resolve({effects:[{id:'one',status:'active'}]});await old;
 assert.equal(h.state().queue[0].status,'expired');
});
test('current poll failure remains visible',async()=>{
 const h=harness();const read=h.refreshQueue();h.pending[0].reject(Error('offline'));await read;
 assert.equal(h.state().queueError,'offline');
});

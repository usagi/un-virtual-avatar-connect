import fs from 'node:fs';
import test from 'node:test';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/ManagedAppDrawer.svelte',import.meta.url),'utf8');
const ast=ts.createSourceFile('drawer.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const names=['operationSignature','requestOperation','confirmOperation','close'];
const code=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function fixture(){
 let resolve,reject;const calls=[];const api={managedApps:()=>new Promise((a,b)=>{resolve=a;reject=b;calls.push('read')})};
 const h=new Function('api','calls',`let open=true,actionReview=null,reviewing=false,resp=null;const busyIds=new Set();const toastStore={warn(){},error(){}};const formatErr=String;const actStop=async e=>calls.push(['stop',e.id]);const actRestart=async e=>calls.push(['restart',e.id]);${code};return {${names.join(',')},cancel:()=>actionReview=null,state:()=>({actionReview,reviewing})}`)(api,calls);
 const entry={id:'a',label:'Avatar A',command:'a.exe',working_dir:'a',process_marker:'a',run_as_admin:false,supports_status:true,status:{running:true,pids:[12,13]}};
 return {...h,calls,entry,resolve:e=>resolve({entries:e}),reject:e=>reject(e)};
}
test('confirmation does not mutate; cancel and close during refresh prevent execution',async()=>{
 for(const cancel of ['cancel','close']){const h=fixture();h.requestOperation('stop',h.entry);assert.deepEqual(h.calls,[]);const p=h.confirmOperation();h[cancel]();h.resolve([h.entry]);await p;assert.deepEqual(h.calls,['read']);}
});
test('changed process or settings invalidate confirmation; PID ordering alone does not',async()=>{
 for(const change of [e=>e.command='b.exe',e=>e.working_dir='b',e=>e.status.pids=[14],e=>e.status.running=false]){const h=fixture();h.requestOperation('restart',h.entry);const p=h.confirmOperation();change(h.entry);h.resolve([h.entry]);await p;assert.deepEqual(h.calls,['read']);}
 const h=fixture();h.requestOperation('stop',h.entry);const p=h.confirmOperation();h.entry.status.pids.reverse();h.resolve([h.entry]);await p;assert.deepEqual(h.calls,['read',['stop','a']]);
});
test('double submit is blocked, missing target stops, read error allows deliberate retry',async()=>{
 const h=fixture();h.requestOperation('restart',h.entry);const p=h.confirmOperation();await h.confirmOperation();h.reject(Error('offline'));await p;assert.equal(h.state().reviewing,false);assert.ok(h.state().actionReview);const retry=h.confirmOperation();h.resolve([]);await retry;assert.equal(h.state().actionReview,null);assert.deepEqual(h.calls,['read','read']);
});
assert.doesNotMatch(source,/\bconfirm\(/);

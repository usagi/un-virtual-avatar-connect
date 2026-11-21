import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const s=fs.readFileSync(new URL('../gui/src/lib/tabs/ProfileManagerPanel.svelte', import.meta.url),'utf8');
const ast=ts.createSourceFile('panel.ts',s.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const fn=ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='load');
const code=ts.transpileModule(fn.getText(ast),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function fixture(){const calls=[];const model=new Function('api',`let loading=false,error=null,entries=[],currentPath=null,directory=null,loadRequest=0;const extractMessage=e=>e.message;${code};return {load,state:()=>({loading,error,entries,currentPath,directory})};`)({profiles:()=>new Promise((resolve,reject)=>calls.push({resolve,reject}))});return {...model,calls};}
const result=name=>({entries:[{filename:name}],current:name,directory:name});
test('older list completion cannot undo a post-operation refresh',async()=>{
 const h=fixture();const old=h.load();const fresh=h.load();h.calls[1].resolve(result('new'));await fresh;h.calls[0].resolve(result('old'));await old;assert.equal(h.state().entries[0].filename,'new');assert.equal(h.state().currentPath,'new');assert.equal(h.state().directory,'new');
});
test('older failure cannot clear loading or replace latest error state',async()=>{
 const h=fixture();const old=h.load();const fresh=h.load();h.calls[0].reject(Error('old failure'));await old;assert.equal(h.state().loading,true);assert.equal(h.state().error,null);h.calls[1].resolve(result('new'));await fresh;assert.equal(h.state().loading,false);
});

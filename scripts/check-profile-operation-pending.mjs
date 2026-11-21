import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source = fs.readFileSync(new URL('../gui/src/lib/tabs/ProfileManagerPanel.svelte', import.meta.url), 'utf8');
const ast = ts.createSourceFile('panel.ts', source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const names = ['submitClone', 'submitRename', 'doDelete', 'openEditor', 'cancelClone', 'cancelRename'];
const code = ts.transpileModule(ast.statements.filter(n => ts.isFunctionDeclaration(n) && names.includes(n.name?.text)).map(n => n.getText(ast)).join('\n'), { compilerOptions: { target: ts.ScriptTarget.ESNext } }).outputText;
function fixture(kind) {
 const calls = [];
 const api = Object.fromEntries(['profileClone', 'profileRename', 'profileDelete'].map(key => [key, (...args) => new Promise((resolve, reject) => calls.push({ key, args, resolve, reject }))]));
 return { calls, ...new Function('api', `let cloneFor='source.toml',cloneName='copy.toml',renameFor='source.toml',renameName='renamed.toml',busyName=null,editingFilename=null,confirmation=null;const mayLeave=()=>!busyName; const window={confirm:()=>true},toastStore={success:()=>{},error:()=>{}},extractMessage=e=>e.message,load=async()=>{}; ${code}; return {${names.join(',')},state:()=>({cloneFor,cloneName,renameFor,renameName,busyName,editingFilename})};`)(api) };
}
for (const kind of ['Clone', 'Rename']) {
 test(`${kind}: another row cannot mutate or replace pending operation; failure retains draft and allows retry`, async () => {
  const h = fixture(kind);
  const pending = h[`submit${kind}`]('source.toml');
  void h.doDelete({ filename:'other.toml', is_current:false });
  void h[`submit${kind}`]('source.toml');
  h.openEditor({ filename:'other.toml' });
  h[`cancel${kind}`]();
  assert.equal(h.calls.length, 1);
  assert.equal(h.state().editingFilename, null);
  assert.equal(h.state()[kind.toLowerCase()+'For'], 'source.toml');
  h.calls[0].reject(Error('busy')); await pending;
  assert.equal(h.state().busyName, null);
  assert.equal(h.state()[kind.toLowerCase()+'Name'], kind==='Clone'?'copy.toml':'renamed.toml');
  const retry=h[`submit${kind}`]('source.toml');
  h.calls[1].resolve({filename:'saved.toml'}); await retry;
  assert.equal(h.state()[kind.toLowerCase()+'For'], null);
 });
}
test('pending UI locks both name fields and actions across rows', () => {
 for (const name of ['cloneName','renameName']) {
  const input=source.match(new RegExp('<input[^>]*bind:value=\\{'+name+'\\}[^>]*>', 's'))?.[0];
  assert.match(input ?? '', /disabled=\{isBusy\}/);
 }
 assert.match(source, /\{@const isBusy = busyName !== null \|\| loading \|\| confirmation !== null\}/);
});

const managedSource = fs.readFileSync(new URL('../gui/src/lib/tabs/DesktopProfileManagerPanel.svelte', import.meta.url), 'utf8');
const managedAst = ts.createSourceFile('managed.ts', managedSource.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const managedCode = ts.transpileModule(managedAst.statements.filter(n => ts.isFunctionDeclaration(n) && ['setNextProfile', 'restoreProfile'].includes(n.name?.text)).map(n => n.getText(managedAst)).join('\n'), { compilerOptions: { target: ts.ScriptTarget.ESNext } }).outputText;
for (const first of ['setNextProfile', 'restoreProfile']) {
 test(`managed ${first}: pending target cannot be superseded, including during list refresh`, async () => {
  const calls=[], refreshes=[];
  const api=Object.fromEntries(['activateDesktopProfile','restoreDesktopProfile'].map(key=>[key,(id)=>new Promise((resolve,reject)=>calls.push({key,id,resolve,reject}))]));
  const h=new Function('api','load',`let busy=false,error=null,confirmation=null;const message=e=>e.message,toastStore={info(){}};${managedCode};return {setNextProfile,restoreProfile,state:()=>({busy,error})}`)(api,()=>new Promise(resolve=>refreshes.push(resolve)));
  const pending=h[first]('first');
  await h.setNextProfile('other');await h.restoreProfile('other');
  assert.equal(calls.length,1);assert.equal(calls[0].id,'first');
  calls[0].reject(Error('conflict'));await pending;
  assert.equal(h.state().busy,false);assert.equal(h.state().error,'conflict');
  const retry=h[first]('first');calls[1].resolve({});await Promise.resolve();
  assert.equal(refreshes.length,1);
  await h.setNextProfile('other');await h.restoreProfile('other');
  assert.equal(calls.length,2);assert.equal(h.state().busy,true);
  refreshes[0]();await retry;
  assert.equal(h.state().busy,false);assert.equal(h.state().error,null);
 });
}

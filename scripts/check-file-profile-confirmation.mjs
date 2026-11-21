import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';

const source = fs.readFileSync(new URL('../gui/src/lib/tabs/ProfileManagerPanel.svelte', import.meta.url), 'utf8');
const ast = ts.createSourceFile('panel.ts', source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const names = ['draftKey', 'mayLeave', 'requestReview', 'confirmOperation', 'doDelete', 'switchTo', 'openEditor', 'cancelClone', 'cancelRename'];
const code = ts.transpileModule(ast.statements.filter(n => ts.isFunctionDeclaration(n) && names.includes(n.name?.text)).map(n => n.getText(ast)).join('\n'), { compilerOptions: { target: ts.ScriptTarget.ESNext } }).outputText;
function fixture() {
  const calls = [];
  const api = Object.fromEntries(['profileDelete', 'restart'].map(key => [key, (...args) => new Promise((resolve, reject) => calls.push({ key, args, resolve, reject }))]));
  const model = new Function('api', `
    let loading=false,busyName=null,confirmation=null,editingFilename=null,error=null;
    let cloneFor=null,cloneName='',renameFor=null,renameName='',hasNameDraft=false;
    let directory='root-a',currentPath='root-a/current.toml';
    let entries=[{filename:'other.toml',path:'root-a/other.toml',is_current:false}];
    const toastStore={success(){},info(){},error(){}},extractMessage=e=>e.message,load=async()=>{};
    const linkedAppDraft={value:null};
    const flowgraphStore={hasUnsavedDrafts:false,mutating:false};
    let otherGuard=true;
    const tabNavStore={canLeaveCurrent:()=>mayLeave() && otherGuard};
    ${code}
    return {${names.join(',')}, entry:()=>entries[0],
      cancel:()=>confirmation=null, move:()=>directory='root-b',
      activate:()=>entries[0].is_current=true, replace:()=>entries[0].path='root-a/different.toml',
      draft:()=>{cloneFor='other.toml';cloneName='copy.toml';hasNameDraft=true},
      edit:()=>cloneName='newer.toml', blockOther:()=>otherGuard=false,
      dirtyApp:()=>linkedAppDraft.value={},
      dirtyGraph:()=>flowgraphStore.hasUnsavedDrafts=true,
      state:()=>({confirmation,busyName,cloneFor,cloneName,error,editingFilename})};
  `)(api);
  return { ...model, calls };
}
test('review and cancel do not delete; exact target executes once; pending operation blocks reentry', async () => {
  const h = fixture();
  h.requestReview('delete', h.entry());
  assert.equal(h.calls.length, 0); assert.equal(h.mayLeave(), false);
  h.cancel(); assert.equal(h.calls.length, 0);
  h.requestReview('delete', h.entry());
  const pending = h.confirmOperation();
  h.requestReview('switch', h.entry()); await h.confirmOperation();
  assert.equal(h.calls.length, 1);
  assert.deepEqual(h.calls[0].args, ['other.toml']);
  h.calls[0].reject(Error('conflict')); await pending;
  assert.equal(h.state().busyName, null);
  h.requestReview('delete', h.entry());
  const retry = h.confirmOperation(); h.calls[1].resolve({ backup: 'backup' }); await retry;
});
test('changed directory, active profile or path cannot consume an old confirmation', async () => {
  for (const change of ['move', 'activate', 'replace']) {
    const h = fixture(); h.requestReview('delete', h.entry()); h[change]();
    await h.confirmOperation(); assert.equal(h.calls.length, 0); assert.ok(h.state().error);
  }
});
test('discard preserves inputs on cancel and refuses stale inputs; editing cannot bypass a draft', async () => {
  const h = fixture(); h.draft(); h.openEditor(h.entry());
  assert.equal(h.state().editingFilename, null);
  h.cancelClone(); h.cancel(); assert.equal(h.state().cloneName, 'copy.toml');
  h.cancelClone(); h.edit(); await h.confirmOperation(); assert.equal(h.state().cloneName, 'newer.toml');
  h.cancelClone(); await h.confirmOperation(); assert.equal(h.state().cloneName, '');
  assert.equal(h.calls.length, 0);
});
test('restart rechecks other drafts and targets the reviewed path', async () => {
  for (const block of ['blockOther', 'dirtyGraph', 'dirtyApp']) {
    const h = fixture(); h.requestReview('switch', h.entry()); h[block]();
    await h.confirmOperation(); assert.equal(h.calls.length, 0);
  }
  const h = fixture(); h.requestReview('switch', h.entry());
  const pending = h.confirmOperation();
  assert.deepEqual(h.calls[0], { key: 'restart', args: [{ conf: 'root-a/other.toml', graceful_ms: 800 }], resolve: h.calls[0].resolve, reject: h.calls[0].reject });
  h.calls[0].resolve({ new_pid: 1 }); await pending;
});

import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';

const source = fs.readFileSync(new URL('../gui/src/lib/control/dictionary/DictionaryEntryForm.svelte', import.meta.url), 'utf8');
const ast = ts.createSourceFile('form.ts', source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const code = ts.transpileModule(ast.statements.filter(n => ts.isFunctionDeclaration(n) && ['submit', 'extractMessage'].includes(n.name?.text)).map(n => n.getText(ast)).join('\n'), {compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
class ControlApiError extends Error { constructor(status) { super('conflict'); this.status = status; } }
function fixture(refresh) {
  const posts = [], callbacks = [];
  const api = {
    async postControlTableEntry(...args) { posts.push(args); if(posts.length === 1) throw new ControlApiError(409); return {affected_row_index:2}; },
    async getControlTable(key) { assert.equal(key,'a'); return refresh(); },
  };
  const form = new Function('api','ControlApiError','callbacks', `
    let reading='よみ',surface='入力を保持',priority=0,submitting=false,formError=null,retryHash=null,open=true;
    const table={key:'a',path:'a.tsv',content_hash:'old'},mode={kind:'new'};
    const buildValues=()=>({reading,surface,priority}),toastStore={success(){}},onSaved=()=>callbacks.push('saved'),onConflict=()=>callbacks.push('conflict');
    ${code}
    return {submit,state:()=>({reading,surface,open,submitting,formError,retryHash})};
  `)(api,ControlApiError,callbacks);
  return {...form,posts,callbacks};
}
const h = fixture(()=>({key:'a',path:'a.tsv',content_hash:'new',rows:[{},{}]}));
await h.submit();
assert.equal(h.posts.length,1); // no automatic write
assert.deepEqual(h.callbacks,[]);
assert.equal(h.state().open,true);
assert.equal(h.state().surface,'入力を保持');
assert.equal(h.state().retryHash,'new');
await h.submit();
assert.equal(h.posts[1][2],'new');
assert.deepEqual(h.posts[0][1],h.posts[1][1]);
assert.deepEqual(h.callbacks,['saved']);
assert.equal(h.state().open,false);
for(const refresh of [()=>{throw Error('offline');},()=>({key:'b',path:'b.tsv',content_hash:'wrong',rows:[]})]) {
  const f=fixture(refresh); await f.submit();
  assert.equal(f.state().open,true); assert.equal(f.state().retryHash,null);
  assert.equal(f.state().surface,'入力を保持'); assert.ok(f.state().formError);
  assert.deepEqual(f.callbacks,[]); assert.equal(f.posts.length,1);
}
console.log('Dictionary new-entry conflict: draft retained, explicit retry uses latest hash, failed/foreign refresh retains draft and old token; no automatic resubmit.');

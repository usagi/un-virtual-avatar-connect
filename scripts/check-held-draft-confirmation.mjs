import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';

const source = fs.readFileSync(new URL('../gui/src/App.svelte', import.meta.url), 'utf8');
const ast = ts.createSourceFile('app.ts', source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const functions = ['discardHeldDraft', 'confirmDiscardHeldDraft'].map(name =>
  ast.statements.find(n => ts.isFunctionDeclaration(n) && n.name?.text === name).getText(ast)).join('\n');
assert.ok(!functions.includes('window.confirm'));
const code = ts.transpileModule(functions, {compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const drafts = new Map([['first/main', 'first draft'], ['second/main', 'second draft']]);
const calls = [];
const store = {
  mutating: false,
  workspaceDraftText(root, fq) { return drafts.get(`${root}/${fq}`) ?? null; },
  discardWorkspaceDraft(root, fq) {
    if (this.mutating) return false;
    calls.push([root, fq]);
    return drafts.delete(`${root}/${fq}`);
  },
};
const h = new Function('flowgraphStore', 'toastStore', `
 let discardTarget=null, discardError=null;
 ${code}
 return {discardHeldDraft,confirmDiscardHeldDraft,cancel(){discardTarget=null;discardError=null;},state:()=>({discardTarget,discardError})};
`)(store, {error(){}});
h.discardHeldDraft('first', 'main');
assert.equal(calls.length, 0, 'Opening must retain draft');
h.cancel();
h.confirmDiscardHeldDraft();
assert.equal(calls.length, 0, 'Cancellation must retain draft');
h.discardHeldDraft('first', 'main');
drafts.set('first/main', 'newer edit');
h.confirmDiscardHeldDraft();
assert.equal(calls.length, 0, 'Stale confirmation must not delete newer input');
assert.ok(h.state().discardError);
h.cancel();
h.discardHeldDraft('first', 'main');
store.mutating = true;
h.confirmDiscardHeldDraft();
assert.equal(calls.length, 0, 'Save beginning after confirmation must block discard');
assert.ok(h.state().discardError);
store.mutating = false;
h.cancel();
h.discardHeldDraft('first', 'main');
drafts.delete('first/main'); // Restored into active editor while dialog was open.
h.confirmDiscardHeldDraft();
assert.equal(calls.length, 0);
assert.ok(h.state().discardError);
h.cancel();
drafts.set('first/main', 'newer edit');
h.discardHeldDraft('first', 'main');
h.confirmDiscardHeldDraft();
assert.deepEqual(calls, [['first', 'main']]);
assert.equal(drafts.get('second/main'), 'second draft', 'Same file name in another root must survive');
assert.equal(h.state().discardTarget, null);
h.confirmDiscardHeldDraft();
assert.equal(calls.length, 1, 'Duplicate confirmation must do nothing');
console.log('Held draft confirmation: cancel, stale input, save guard, moved draft and exact target passed');
const downloadSource = ast.statements.find(n => ts.isFunctionDeclaration(n) && n.name?.text === 'downloadHeldDraft').getText(ast);
const downloadCode = ts.transpileModule(downloadSource,{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
let blob,downloadName;const exportErrors=[];
const download = new Function('flowgraphStore','toastStore','URL','document','setTimeout',downloadCode+';return downloadHeldDraft;')(store,{error:(...e)=>exportErrors.push(e)},{createObjectURL:b=>{blob=b;return 'blob:fixture';},revokeObjectURL(){}},{createElement:()=>({set href(v){},set download(v){downloadName=v;},click(){}})},()=>{});
const recoveryText=JSON.stringify({format:'unvac-property-draft-recovery-v1',editor:{propertyJsonDrafts:[{nodeId:'a',key:'value',text:'{"broken":'}]}},null,2);
drafts.set('first/main',recoveryText);download('first','main');
assert.equal(downloadName,'main.draft-recovery.json');assert.equal(blob.type,'application/json;charset=utf-8');assert.equal(await blob.text(),recoveryText);
h.discardHeldDraft('first','main');h.confirmDiscardHeldDraft();assert.equal(drafts.has('first/main'),false);
download('second','main');assert.equal(downloadName,'main.draft.flowgraph.toml');assert.equal(await blob.text(),'second draft');assert.deepEqual(exportErrors,[]);
console.log('Held invalid JSON exports all raw input as recovery JSON and remains explicitly discardable; valid draft retains TOML export.');

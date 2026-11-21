import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';

const source = fs.readFileSync(new URL('../gui/src/lib/control/dictionary/dictionaryEditorStore.svelte.ts', import.meta.url), 'utf8');
const ast = ts.createSourceFile('store.ts', source, ts.ScriptTarget.Latest, true);
const selected = ast.statements.filter(n => ts.isClassDeclaration(n) || ts.isFunctionDeclaration(n));
const code = ts.transpileModule(selected.map(n => n.getText(ast)).join('\n').replaceAll('export ', ''), {
  compilerOptions: { target: ts.ScriptTarget.ESNext },
}).outputText;
function make() {
  const reads = [], catalogs = [];
  const api = {
    getControlTable(key) { return new Promise((resolve, reject) => reads.push({key, resolve, reject})); },
    listControlTables() { return new Promise((resolve, reject) => catalogs.push({resolve, reject})); },
  };
  const store = new Function('api', '$state', 'ControlApiError', `${code};return new DictionaryEditorStore();`)(api, x => x, class extends Error {});
  return {store, reads, catalogs};
}
const table = (key, content_hash = key) => ({key, content_hash, path: `${key}.tsv`, rows: [], columns: [], editable: true});
for (const outcome of ['success', 'failure']) {
  const {store, reads} = make();
  const first = store.selectTable('a'), second = store.selectTable('b');
  reads[1].resolve(table('b')); await second;
  if (outcome === 'success') reads[0].resolve(table('a')); else reads[0].reject(Error('old failure'));
  await first;
  assert.equal(store.currentKey, 'b');
  assert.equal(store.currentTable.key, 'b');
  assert.equal(store.phase.kind, 'ready');
}
{
  const {store, reads} = make();
  const first = store.selectTable('a'), second = store.selectTable('a');
  reads[1].resolve(table('a', 'new')); await second;
  reads[0].resolve(table('a', 'old')); await first;
  assert.equal(store.currentTable.content_hash, 'new');
  const third = store.reloadCurrent();
  store.applyTableOverride(table('a', 'mutation'));
  reads[2].resolve(table('a', 'before-mutation')); await third;
  assert.equal(store.currentTable.content_hash, 'mutation');
}
for (const outcome of ['success', 'failure']) {
  const {store, reads, catalogs} = make();
  const oldCatalog = store.loadCatalog(), selected = store.selectTable('b');
  reads[0].resolve(table('b')); await selected;
  if (outcome === 'success') catalogs[0].resolve({tables: [], count: 0}); else catalogs[0].reject(Error('old catalog failure'));
  await oldCatalog;
  assert.equal(store.currentKey, 'b');
  assert.equal(store.currentTable.key, 'b');
  assert.equal(store.phase.kind, 'ready');
}
{
  const {store, reads} = make();
  const pending = store.selectTable('a');
  reads[0].resolve(table('b')); await pending;
  assert.equal(store.phase.kind, 'error');
  assert.equal(store.currentTable, null);
}
console.log('Dictionary reads: stale success/error/catalog, same-key refresh, mutation override and mismatched response passed.');

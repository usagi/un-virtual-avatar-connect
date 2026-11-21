import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source = fs.readFileSync(new URL('../gui/src/lib/flowgraphStore.svelte.ts', import.meta.url), 'utf8');
const ast = ts.createSourceFile('store.ts', source, ts.ScriptTarget.Latest, true);
const cls = ast.statements.find(n => ts.isClassDeclaration(n) && n.name.text === 'FlowgraphStore');
const method = cls.members.find(n => ts.isMethodDeclaration(n) && n.name.getText(ast) === 'saveRawToml');
const validation = cls.members.filter(n => n.name && ['rejectInvalidPropertyJson', 'invalidPropertyJsonDrafts'].includes(n.name.getText(ast))).map(n => n.getText(ast)).join('\n');
const code = ts.transpileModule(`class Subject { propertyJsonDrafts=[]; ${validation} ${method.getText(ast)} }`, { compilerOptions: { target: ts.ScriptTarget.ESNext } }).outputText;
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
for (const scenario of ['switch', 'edit', 'read-edit', 'read-failure', 'write-failure', 'unchanged', 'external-read', 'root-switch-read']) {
  const write = deferred(), read = deferred();
  const reads = [], writes = [], reloads = [];
  const api = { flowgraphPutFile: (fq, body) => { writes.push({ fq, body }); return write.promise; }, flowgraphFile: fq => { reads.push(fq); return read.promise; } };
  const Subject = new Function('api', 'toastStore', 'describeError', 'serializeFlowgraph', `${code}; return Subject;`)(api, { success() {}, warn() {}, error() {} }, String, JSON.stringify);
  const subject = Object.assign(new Subject(), { currentFq: 'a', currentState: 'ok', currentFileRequest: 1, currentFile: { raw_toml: 'old', root_dir: 'root-a', parsed: {} }, draftNodes: [{ id: 'a' }], draftEdges: [], draftToml: 'old', mutating: false, openFile: async fq => { reloads.push(fq); subject.draftNodes = []; }, refreshTree: async () => {}, refreshDiagnostics: async () => {} });
  const saving = subject.saveRawToml('raw saved');
  if (scenario === 'switch') { subject.currentFq = 'b'; subject.currentFileRequest++; subject.draftNodes = [{ id: 'b' }]; }
  if (scenario === 'edit') subject.draftNodes = [{ id: 'added' }];
  if (scenario === 'write-failure') write.reject(new Error('write failed'));
  else write.resolve({ ok: true, fq: 'a', diagnostics: [] });
  await Promise.resolve();
  if (scenario === 'read-edit') subject.draftNodes = [{ id: 'during-read' }];
  if (scenario === 'read-failure' && reads.length) read.reject(new Error('read failed'));
  else read.resolve({ fq: 'a', root_dir: scenario === 'root-switch-read' ? 'root-b' : 'root-a', raw_toml: scenario === 'external-read' ? 'another writer' : writes[0].body.content, parsed: { nodes: [{ id: 'saved', feature: 'test', properties: {} }], edges: [], groups: [], instances: [] } });
  const result = await saving;
  assert.deepEqual(reloads, [], `${scenario}: save must not reopen an editor`);
  assert.equal(writes[0].fq, 'a');
  assert.equal(writes[0].body.expected_content, 'old');
  assert.equal(writes[0].body.expected_root, 'root-a');
  if (scenario === 'external-read' || scenario === 'root-switch-read' || scenario === 'write-failure') {
    assert.equal(subject.currentFile.raw_toml, 'old');
    assert.equal(subject.draftToml, 'old');
  }
  if (scenario === 'unchanged') assert.equal(subject.currentFile.raw_toml, writes[0].body.content);
  assert.equal(subject.draftNodes[0].id, ({ switch: 'b', edit: 'added', 'read-edit': 'during-read' })[scenario] ?? (scenario === 'unchanged' ? 'saved' : 'a'));
  assert.equal(subject.mutating, false);
  assert.equal(result, scenario !== 'write-failure');
  if (scenario === 'switch') assert.deepEqual(reads, []);
}
console.log('PASS: raw save target/edits/write-read failure/conflicting read/root change and clean hydration');

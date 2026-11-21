import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';

const source = fs.readFileSync(new URL('../gui/src/lib/tabs/NowTab.svelte', import.meta.url), 'utf8');
const ast = ts.createSourceFile('home.ts', source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const fn = ast.statements.find(n => ts.isFunctionDeclaration(n) && n.name?.text === 'refreshNow');
const code = ts.transpileModule(fn.getText(ast), {compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const calls = {snapshot:[], mode:[], apps:[], diagnostics:[], tree:[]};
const pending = name => () => new Promise((resolve,reject) => calls[name].push({resolve,reject}));
const h = new Function('api','runtimeSnapshotStore','flowgraphStore', `
 let refreshRequest=0,loading=false,currentMode=null,managedApps=null,diagnostics=null,failedSources=[],error=null;
 ${code}
 return {refreshNow,state:()=>({loading,currentMode,managedApps,diagnostics,failedSources,error})};
`)({currentMode:pending('mode'),managedApps:pending('apps'),flowgraphDiagnostics:pending('diagnostics')},
 {refresh:pending('snapshot')},{refreshTree:pending('tree')});

const older = h.refreshNow();
calls.mode[0].resolve({mode:'standard'});
calls.apps[0].resolve({entries:[]});
await new Promise(resolve => setImmediate(resolve));
assert.equal(h.state().currentMode,'standard');
assert.deepEqual(h.state().managedApps,{entries:[]});
assert.equal(h.state().loading,true,'Slow diagnostics must not withhold already returned cards');
const newer = h.refreshNow();
calls.mode[1].resolve({mode:'streaming'});
calls.apps[1].resolve({entries:[{label:'new'}]});
calls.diagnostics[1].resolve({diagnostics:[]});
calls.snapshot[1].resolve({});
calls.tree[1].reject(Error('unavailable'));
await newer;
calls.diagnostics[0].resolve({diagnostics:[{message:'stale'}]});
calls.snapshot[0].resolve({});
calls.tree[0].resolve({});
await older;
assert.equal(h.state().currentMode,'streaming');
assert.deepEqual(h.state().managedApps,{entries:[{label:'new'}]});
assert.deepEqual(h.state().diagnostics,{diagnostics:[]});
assert.deepEqual(h.state().failedSources,['保存済み機能']);
assert.equal(h.state().loading,false);
console.log('PASS actual home refresh publishes independent results and rejects stale completion; mock API only');

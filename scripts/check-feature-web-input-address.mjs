import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraph/featureSetup.ts',import.meta.url),'utf8');
const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ESNext}}).outputText;
const {plannedWebInput}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
assert.deepEqual(plannedWebInput('features/counter/main::in',{}),{path:'/input/flowgraph/features_counter_main__in',method:'POST'});
assert.deepEqual(plannedWebInput('日本😀::in',{}),{path:'/input/flowgraph/_____in',method:'POST'});
for(const [value,expected] of [[' /custom/ ','/custom'],['custom','/custom'],['//custom/','//custom'],['///','/']]) {
 assert.equal(plannedWebInput('x',{path:value}).path,expected);
}
assert.equal(plannedWebInput('x',{method:' put '}).method,'PUT');
assert.equal(plannedWebInput('x',{method:'DELETE'}).method,'POST');
console.log('PASS: planned Web input matches backend normalization and observed normal-desktop counter endpoint; not live-readiness proof');

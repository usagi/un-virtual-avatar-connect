import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
import {parse} from '../gui/node_modules/smol-toml/dist/index.js';
const read = path => fs.readFileSync(new URL('../' + path, import.meta.url), 'utf8');
const source=read('gui/src/lib/flowgraph/featureSetup.ts');
const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ESNext}}).outputText;
const {resolveSetupFields}=await import('data:text/javascript;base64,'+Buffer.from(code).toString('base64'));
const contract=JSON.parse(read('docs/maintenance/unvac-feature-setup-bindings-20261017.json'));
let fields=0;
for(const pack of contract.packs){
 const template=parse(read(pack.source));
 template.edges??=[];
 const doc=structuredClone(template);const before=JSON.stringify(doc);
 const result=resolveSetupFields(doc,template,pack.controls);
 assert.equal(result.reason,null);fields+=result.fields.length;
 assert.equal(JSON.stringify(doc),before);
 doc.nodes.reverse();doc.edges.reverse();
 assert.equal(resolveSetupFields(doc,template,pack.controls).reason,null,'ordering is not topology');
 doc.nodes[0].feature='changed';
 assert.notEqual(resolveSetupFields(doc,template,pack.controls).reason,null);
 const rewired=structuredClone(template);rewired.edges.push({from:'unknown:out',to:'unknown:in'});
 assert.notEqual(resolveSetupFields(rewired,template,pack.controls).reason,null);
 if(pack.controls.length){
  const changed=structuredClone(template);const field=pack.controls[0];
  changed.nodes.find(n=>n.id===field.node_id).properties[field.property]=null;
  assert.notEqual(resolveSetupFields(changed,template,pack.controls).reason,null);
 }
}
assert.equal(fields,13);console.log('PASS: 4 actual templates / 13 settings; source unchanged; reordered, modified and rewired graphs distinguished');

const fixtures=JSON.parse(read('scripts/preview-profile-templates.json'));
const automation=fixtures.templates.find(item=>item.id==='automation');
const starter=parse(automation.files.find(file=>file.path==='flowgraph/main.flowgraph.toml').content);
const counter=contract.packs.find(item=>item.id==='counter');
const baseline=parse(read(counter.source));
assert.equal(resolveSetupFields(starter,baseline,counter.controls).reason,null,'new automation profile must reach the same setup/test-input guide');
console.log('PASS: native-materialized automation profile matches the frontend counter setup guide');

async function loadDataModule(path) {
 const output=ts.transpileModule(read(path),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ESNext}}).outputText;
 return import('data:text/javascript;base64,'+Buffer.from(output).toString('base64'));
}
const {FEATURE_USAGE}=await loadDataModule('gui/src/lib/flowgraph/featureUsage.ts');
const {FEATURES}=await loadDataModule('gui/src/lib/featureCatalog.ts');
const ids=new Set(FEATURES.map(feature=>feature.id));
for(const pack of contract.packs){
 const usage=FEATURE_USAGE[pack.id];
 assert.ok(usage?.steps.length, `${pack.id}: missing usage instructions`);
 for(const destination of usage.destinations) assert.ok(ids.has(destination), `${pack.id}: missing navigation target ${destination}`);
 // These four built-in non-AI features must remain usable without an AI node.
 const graph=parse(read(pack.source));
 assert.ok(!graph.nodes.some(node=>/^flowgraph\.(ai|openai)\./.test(node.feature)), `${pack.id}: unexpected AI dependency`);
}
console.log('PASS: all bundled feature guides have instructions and valid navigation targets; graphs contain no AI/OpenAI nodes');

const recovery=read('gui/src/lib/flowgraph/FlowgraphRecoveryPreview.svelte');
const recoveryAst=ts.createSourceFile('recovery.ts',recovery.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const focusEffect=recoveryAst.statements.find(node=>ts.isExpressionStatement(node)
 && ts.isCallExpression(node.expression) && node.expression.expression.getText(recoveryAst)==='$effect'
 && node.getText(recoveryAst).includes('tabNavStore.featureFocus'));
assert.ok(focusEffect,'recovery navigation must open the recovery panel');
const focusCode=ts.transpileModule('const effect = '+focusEffect.expression.arguments[0].getText(recoveryAst),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const evaluate=new Function('id','initial',`let open=initial;const tabNavStore={featureFocus:id?{id}:null};${focusCode};effect();return open;`);
assert.equal(evaluate(null,false),false);
assert.equal(evaluate('flowgraph-recovery',false),true);
assert.equal(evaluate('flowgraph-studio',false),false);
assert.equal(evaluate('flowgraph-studio',true),true,'unrelated navigation must not discard an open recovery review');
const studio=read('gui/src/lib/tabs/FlowgraphTab.svelte');
assert.match(studio, /<FeatureAnchor id="flowgraph-studio">\s*\{@render studioWorkspace\(\)\}\s*<\/FeatureAnchor>/);
console.log('PASS: recovery navigation opens its panel; Studio has a focusable feature destination (rendered behavior remains separate)');

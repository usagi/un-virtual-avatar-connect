import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
import test from 'node:test';
import assert from 'node:assert/strict';
const source=fs.readFileSync(new URL('../gui/src/lib/linkedAppDraft.svelte.ts',import.meta.url),'utf8');
const js=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ESNext,module:ts.ModuleKind.CommonJS}}).outputText;
const exports={};new Function('exports','$state',js)(exports,x=>x);
const form=(label,command='missing-test-app.exe',modes={})=>JSON.stringify([command,'marker','id-'+label,label,'','','',false,false,false,modes,{action:'close_only',on_vac_exit:'leave_running',grace_ms:12345,scope:'matched_processes'}]);
const clean=JSON.stringify(['','','','','','','',false,false,false,{}, {action:'close_only'}]);
const fixture=()=>({form:clean,baseline:clean,editingIndex:null,editingRevision:'r1',revision:'r1',sourcePath:'isolated.toml',entries:[{index:0,effective_id:'a',display_label:'A'},{index:1,effective_id:'b',display_label:'B'}],modes:[]});
test('blank active form still submits two retained details and one addition without duplicate targets',()=>{
 const d={...fixture(),otherForms:{0:{form:form('A2'),baseline:form('A')},1:{form:form('B2'),baseline:form('B')},new:{form:form('C'),baseline:clean}},modeEdits:{1:{work:'minimize'}}};
 const edits=exports.batchFromDraft(d),add=exports.additionsFromDraft(d);
 assert.deepEqual(edits.map(x=>x.index),[0,1]);assert.equal(edits[1].entry.label,'B2');assert.equal(edits[1].mode_actions.work,'minimize');assert.equal(add.length,1);assert.equal(add[0].start_on_launch,false);assert.deepEqual(add[0].shutdown,{action:'close_only',on_vac_exit:'leave_running',grace_ms:12345,scope:'matched_processes'});
});
test('active target takes precedence over its retained snapshot',()=>{
 const d={...fixture(),editingIndex:0,form:form('Newest'),baseline:form('A'),otherForms:{0:{form:form('Old'),baseline:form('A')}}};
 const edits=exports.batchFromDraft(d);assert.equal(edits.length,1);assert.equal(edits[0].entry.label,'Newest');
});
test('invalid hidden target is rejected without mutating any shared draft',()=>{
 for(const key of ['0','8','new']){const d={...fixture(),otherForms:{[key]:{form:form('Invalid',''),baseline:clean}}};const before=JSON.stringify(d);assert.throws(()=>key==='new'?exports.additionsFromDraft(d):exports.batchFromDraft(d));assert.equal(JSON.stringify(d),before);}
});

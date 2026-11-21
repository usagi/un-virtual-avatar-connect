import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraph/PurposeTransferPreview.svelte',import.meta.url),'utf8').split('<script lang="ts">')[1].split('</script>')[0];
const ast=ts.createSourceFile('component.ts',source,ts.ScriptTarget.Latest,true);
const fn=ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='chooseFile');
const js=ts.transpileModule(fn.getText(ast),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function harness(){
 return new Function(`let blocked=false,busy=false,error='',status='previous result',review={token:'review'},attempted=true,archive=new Uint8Array([1,2]),fileName='previous.zip',partialSave={written_files:['saved']};${js};return {chooseFile,state:()=>({busy,error,status,review,attempted,archive:archive ? [...archive] : null,fileName,partialSave})};`)();
}
for(const file of [{size:65*1024*1024,arrayBuffer:()=>{throw Error('must not read')}},{size:2,arrayBuffer:async()=>{throw Error('unreadable file')}}]){
 const h=harness(),input={files:[file],value:'selection'};
 await h.chooseFile({target:input});
 const s=h.state();
 assert.deepEqual(s.archive,[1,2]); assert.equal(s.fileName,'previous.zip');
 assert.deepEqual(s.review,{token:'review'});assert.equal(s.attempted,true);
 assert.deepEqual(s.partialSave,{written_files:['saved']});assert.equal(s.busy,false);assert.ok(s.error);assert.equal(input.value,'');
}
const h=harness();await h.chooseFile({target:{files:[{name:'new.zip',size:3,arrayBuffer:async()=>new Uint8Array([3,4,5]).buffer}],value:'new'}});
assert.deepEqual(h.state().archive,[3,4,5]);assert.equal(h.state().fileName,'new.zip');
assert.equal(h.state().review,null);assert.equal(h.state().attempted,false);assert.equal(h.state().partialSave,null);
console.log('PASS: rejected/unreadable ZIP retains previous input and recovery evidence; successful replacement clears old review and partial result');

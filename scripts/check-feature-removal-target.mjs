import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraph/FeatureLibrary.svelte',import.meta.url),'utf8');
const ast=ts.createSourceFile('library.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const names=['remove','inspectRemoval','inspect','install','editGraph'];
const methods=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function fixture(){
 let release,calls=0,reads=0;
 const flowgraphStore={tree:{root_dir:'A'},isDirty:false,mutating:false,deleteFile:()=>{calls++;return new Promise(resolve=>release=resolve)}};
 const api={desktopProfiles:async()=>{reads++;return {current_conf_path:'A/conf'}}};
 const subject=new Function('flowgraphStore','api',`let busy=false,error='',result='',resultTree=null,preview=null,chosen=null;let removal={fq:'main',identity:'A/conf',label:'A',plan:{root_dir:'A',can_delete:true,expected_content:'old'}};${methods};return {${names.join(',')},state:()=>({busy,error,result,resultTree,removal})}`)(flowgraphStore,api);
 return {subject,flowgraphStore,resolve:v=>release(v),counts:()=>({calls,reads})};
}
for(const switchRoot of [false,true]){
 const h=fixture();const pending=h.subject.remove();await Promise.resolve();
 await h.subject.inspect({id:'other'});await h.subject.inspectRemoval('other','other');await h.subject.install();await h.subject.editGraph('other');await h.subject.remove();
 assert.deepEqual(h.counts(),{calls:1,reads:1});assert.equal(h.subject.state().removal.fq,'main');
 if(switchRoot)h.flowgraphStore.tree={root_dir:'B'};
 h.resolve({ok:true,backup:'backup'});await pending;
 if(switchRoot){assert.equal(h.subject.state().result,'');assert.equal(h.subject.state().resultTree,null);assert.equal(h.subject.state().removal,null);assert.match(h.subject.state().error,/削除先 A/);}
 else assert.match(h.subject.state().result,/削除・適用済み/);
 assert.equal(h.subject.state().busy,false);
}
console.log('PASS: removal keeps its reviewed target during competing actions; late completion cannot label another root; same-root success retained');

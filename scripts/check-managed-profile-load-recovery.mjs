import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/tabs/ManagedProfileEditor.svelte',import.meta.url),'utf8');
const ast=ts.createSourceFile('editor.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const code=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&['load','save','apply','describeError'].includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function fixture(){
 const reads=[],writes=[];
 const api={desktopProfileContent:(id)=>new Promise((resolve,reject)=>reads.push({id,resolve,reject})),saveDesktopProfileContent:(...args)=>new Promise((resolve,reject)=>writes.push({args,resolve,reject}))};
 const h=new Function('api',`let id='isolated-profile',label='fixture',current=false,busy=false,loaded=false,content='',original='',revision='',error='',saved=false,confirmation=null;
 const dirty=()=>content!==original;
 ${code.replace(/\bdirty\b/g,'dirty()')}
 return {load,save,apply,edit:v=>content=v,state:()=>({content,original,revision,busy,loaded,error,saved})};`)(api);
 return {...h,reads,writes};
}
for(const missingRevision of [false,true]){
 const h=fixture();const first=h.load();await h.load();assert.equal(h.reads.length,1);
 if(missingRevision)h.reads[0].resolve({content:'unverified'});else h.reads[0].reject(Error('read failed'));
 await first;await h.save();await h.apply();assert.equal(h.writes.length,0);assert.equal(h.state().loaded,false);assert.ok(h.state().error);
 const retry=h.load();h.reads[1].resolve({content:'original',revision:'r1'});await retry;
 assert.equal(h.reads[1].id,'isolated-profile');h.edit('draft');await h.load();assert.equal(h.reads.length,2);
 const write=h.save();await h.save();assert.equal(h.writes.length,1);assert.deepEqual(h.writes[0].args,['isolated-profile','draft','r1']);
 const conflict=Object.assign(Error('Control API 409 Conflict'),{body:{detail:'設定が別の操作で変更されています。編集内容を控えて再読込してください',error:'profile_changed'}});
 h.writes[0].reject(conflict);await write;assert.equal(h.state().error,conflict.body.detail);assert.equal(h.state().content,'draft');assert.equal(h.state().revision,'r1');
 const again=h.save();h.writes[1].resolve({content:'draft',revision:'r2'});await again;
 assert.equal(h.state().original,'draft');assert.equal(h.state().revision,'r2');assert.equal(h.state().saved,true);
 await h.save();assert.equal(h.writes.length,2);
}
assert(source.includes('設定内容を再読込'));
console.log('PASS: managed profile failed/missing-revision load retries; drafts, target and conflict baseline retained; duplicate and clean writes refused');

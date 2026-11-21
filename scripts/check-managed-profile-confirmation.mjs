import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/tabs/ManagedProfileEditor.svelte',import.meta.url),'utf8');
assert(!source.includes('window.confirm('));
const ast=ts.createSourceFile('editor.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const names=['mayLeave','requestClose','confirmDiscard','apply','confirmApply','describeError'];
const code=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function fixture(){
 const calls=[];let complete;
 const api={desktopProfiles:async()=>({current_profile_id:'p',profiles:[{id:'p',conf_path:'isolated/p.toml'}]}),desktopProfileContent:()=>new Promise(resolve=>complete=resolve),restart:async args=>calls.push(args)};
 const h=new Function('api',`let busy=false,loaded=true,current=true,content='saved',original='saved',id='p',label='profile',revision='r1',error='',confirmation=null,closes=0;
 const linkedAppDraft={value:null};
 const flowgraphStore={hasUnsavedDrafts:false,mutating:false},toastStore={info(){}},onclose=()=>closes++;
 const dirty=()=>content!==original;const tabNavStore={canLeaveCurrent:()=>mayLeave()};
 ${code.replace(/\bdirty\b/g,'dirty()')}
 return {apply,confirmApply,requestClose,confirmDiscard,mayLeave,edit:v=>content=v,cancel:()=>confirmation=null,linkedAppDraft,flowgraphStore,state:()=>({confirmation,error,closes,busy,content})};`)(api);
 return {...h,calls,complete:result=>complete(result)};
}
let h=fixture();h.edit('draft');h.requestClose();assert.equal(h.state().closes,0);h.cancel();assert.equal(h.state().content,'draft');
h.requestClose();h.edit('newer draft');h.confirmDiscard();assert.equal(h.state().closes,0);assert.ok(h.state().error);
h.requestClose();h.confirmDiscard();assert.equal(h.state().closes,1);
for(const block of ['none','draft','revision','appDraft']){
 h=fixture();await h.apply();assert.equal(h.calls.length,0);assert.equal(h.state().confirmation.kind,'restart');
 const run=h.confirmApply();await Promise.resolve();await h.confirmApply();
 if(block==='appDraft')h.linkedAppDraft.value={};
 if(block==='draft')h.flowgraphStore.hasUnsavedDrafts=true;
 h.complete({revision:block==='revision'?'r2':'r1'});await run;
 assert.equal(h.calls.length,block==='none'?1:0);assert.equal(h.state().closes,block==='none'?1:0);
 if(block!=='none'){assert.ok(h.state().error);assert.equal(h.state().busy,false);}
}
console.log('PASS: in-page managed confirmation preserves cancelled/newer drafts; restart rechecks latest revision and late graph edits, rejects duplicate submission');

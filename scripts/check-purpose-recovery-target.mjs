import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraph/PurposeRemovalHistoryPreview.svelte',import.meta.url),'utf8').split('<script lang="ts">')[1].split('</script>')[0];
const ast=ts.createSourceFile('component.ts',source,ts.ScriptTarget.Latest,true);
const functions=ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&['load','restore'].includes(n.name?.text)).map(n=>n.getText(ast)).join('\n');
const js=ts.transpileModule(functions,{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function harness(){
 let resolve;const calls=[];
 const request=(...args)=>{calls.push(args);return new Promise(r=>resolve=r);};
 const h=new Function('request',`let root='A',blocked=false,busy=false,selected=null,entries=null,entriesRoot=null,disposed=false,error='',status='',changed=0;const $state={snapshot:structuredClone};const onchanged=async()=>{changed++};${js};return {load,restore,root:v=>root=v,dispose:()=>disposed=true,select:()=>selected=entries[0],state:()=>({root,busy,entries,entriesRoot,error,status,changed})};`)(request);
 return {...h,calls,reply:v=>resolve(v)};
}
const entry={id:'one',recovery:{id:'recovery',revision:'revision'},files:['main'],purpose:{id:'p',name:'P'}};
let h=harness(),p=h.load();h.root('B');h.reply({root:'A',entries:[entry]});await p;
assert.equal(h.state().entries,null);assert.match(h.state().error,/保存先/);
h=harness();p=h.load();h.reply({root:'A',entries:[entry]});await p;h.select();h.root('B');await h.restore();assert.equal(h.calls.length,1,'old entries must not dispatch mutation to new root');
h=harness();p=h.load();h.reply({root:'A',entries:[entry]});await p;h.select();p=h.restore();assert.equal(h.calls[1][1].expected_root,'A');h.root('B');h.reply({root:'A'});await p;assert.equal(h.state().changed,0);assert.equal(h.state().status,'');assert.match(h.state().error,/元の保存先/);
h=harness();p=h.load();h.reply({root:'A',entries:[entry]});await p;h.select();p=h.restore();h.reply({root:'A'});await p;assert.equal(h.state().changed,1);assert.match(h.state().status,/復元しました/);
h=harness();p=h.load();h.dispose();h.reply({root:'A',entries:[entry]});await p;assert.equal(h.state().entries,null);
console.log('PASS: late history, stale selection, target switch during restore, same-target restore, disposed response');

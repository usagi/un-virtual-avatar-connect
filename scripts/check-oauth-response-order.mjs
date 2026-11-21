import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/OAuthPanel.svelte',import.meta.url),'utf8').split('<script lang="ts">')[1].split('</script>')[0];
const ast=ts.createSourceFile('oauth.ts',source,ts.ScriptTarget.Latest,true);
const names=['invalidateReads','refreshAuthorization','refreshStatus','deleteTokens','start','cancel'];
const functions=ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n');
const code=ts.transpileModule(functions,{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
for(const oldFails of [false,true])for(const duringDeletion of [false,true]){
 const pending={};
 const api={oauthAuthorization:()=>new Promise((resolve,reject)=>pending.auth={resolve,reject}),oauthStatusOrNull:()=>new Promise((resolve,reject)=>pending.status={resolve,reject}),oauthDeleteTokens:()=>new Promise(resolve=>pending.delete=resolve)};
 const h=new Function('api',`const window={confirm:()=>true};const formatError=String;let disposed=false;let statusRequest={broadcaster:0,moderator:0},authorizationRequest={broadcaster:0,moderator:0};let busy={broadcaster:false,moderator:false},errors={},sessions={},authorizedThisSession={},authorization={},configured={};${code};return {refreshAuthorization,refreshStatus,deleteTokens,state:()=>({authorization,sessions,errors})};`)(api);
 let deletion;
 if(duringDeletion)deletion=h.deleteTokens('broadcaster');
 const auth=h.refreshAuthorization('broadcaster'),status=h.refreshStatus('broadcaster');
 if(!duringDeletion)deletion=h.deleteTokens('broadcaster');
 pending.delete();await deletion;
 if(oldFails){pending.auth.reject(new Error('obsolete auth error'));pending.status.reject(new Error('obsolete status error'));}
 else {pending.auth.resolve({authorized:true,configured:true});pending.status.resolve({status:'authorized'});}
 await Promise.all([auth,status]);
 assert.equal(h.state().authorization.broadcaster,false,'old polling must not undo completed token removal');
 assert.equal(h.state().sessions.broadcaster,null);
 assert.equal(h.state().errors.broadcaster,null);
}
console.log('PASS: completed token removal cannot be undone by stale authorization/status success or failure');

let eventCallback;
function visit(node){
 if(ts.isCallExpression(node)&&node.expression.getText(ast)==='eventsStore.subscribe')eventCallback=node.arguments[0];
 ts.forEachChild(node,visit);
}
visit(ast);assert.ok(eventCallback);
const eventCode=ts.transpileModule(`const eventHandler=${eventCallback.getText(ast)};`,{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function harness(){
 const calls=[];
 const api=Object.fromEntries(['oauthAuthorization','oauthStatusOrNull','oauthStart','oauthCancel','oauthDeleteTokens'].map(method=>[method,account=>new Promise((resolve,reject)=>calls.push({method,account,resolve,reject}))]));
 const h=new Function('api',`const window={confirm:()=>true};const formatError=String;let disposed=false;const statusRequest={broadcaster:0,moderator:0},authorizationRequest={broadcaster:0,moderator:0};let busy={broadcaster:false,moderator:false},errors={},sessions={},authorizedThisSession={},authorization={},configured={};${code}\n${eventCode}\nreturn {refreshAuthorization,refreshStatus,start,cancel,event:eventHandler,dispose:()=>disposed=true,state:()=>({sessions,authorization,authorizedThisSession,busy,errors})};`)(api);
 return {...h,calls};
}
for(const method of ['start','cancel'])for(const fails of [false,true]){
 const h=harness();const pending=h[method]('broadcaster');
 h.event({event:{kind:'oauth_status',account:'broadcaster',view:{status:'authorized'}}});
 if(fails)h.calls[0].reject(new Error('obsolete mutation failure'));
 else h.calls[0].resolve({session:{status:method==='start'?'pending':'canceled'}});
 await pending;
 assert.equal(h.state().authorization.broadcaster,true);
 assert.equal(h.state().sessions.broadcaster,null);
 assert.equal(h.state().errors.broadcaster,null);
 assert.equal(h.state().busy.broadcaster,false);
}
for(const method of ['refreshStatus','refreshAuthorization']){
 const h=harness();const older=h[method]('broadcaster'),newer=h[method]('broadcaster'),other=h[method]('moderator');
 const payload=method==='refreshStatus'?{status:'authorized'}:{authorized:true,configured:true};
 h.calls[1].resolve(payload);h.calls[2].resolve(payload);await Promise.all([newer,other]);
 h.calls[0].reject(new Error('obsolete read'));await older;
 assert.equal(h.state().authorization.broadcaster,true);assert.equal(h.state().authorization.moderator,true);
 assert.notEqual(h.state().errors.broadcaster,'Error: obsolete read');
 const disposed=h[method]('broadcaster');h.dispose();h.calls[3].reject(new Error('disposed read'));await disposed;
 assert.notEqual(h.state().errors.broadcaster,'Error: disposed read');
}
console.log('PASS: real OAuth event handler supersedes start/cancel responses; account isolation, read ordering and disposal retained');

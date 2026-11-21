import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/resources/TwitchSetupPanel.svelte',import.meta.url),'utf8');
assert.ok(!source.includes('window.confirm'));
const ast=ts.createSourceFile('panel.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const names=['canLeave','requestReload','confirmReload','load','save'];
const code=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
function harness(){let reads=0,writes=0,fail=false;const api={async twitchSetup(){reads++;if(fail)throw Error('read failed');return {broadcaster:'saved',moderator:'savedbot',revision:'v1',source_path:'profile-a'};},async saveTwitchSetup(){writes++;throw Error('unexpected write');}};
const h=new Function('api',`let status={broadcaster:'saved',moderator:'savedbot',revision:'v1',source_path:'profile-a'},broadcaster='draft',moderator='draftbot',dirty=true,busy=false,discardReview=null,error='',saved=false;function message(e){return e.message;}const confSyncStore={markSavedRequiringRestart(){}};${code};return {canLeave,requestReload,confirmReload,save,cancel(){discardReview=null;},change(){moderator='newer';},switchSource(){status={...status,source_path:'profile-b'};},lock(){busy=true;},state:()=>({broadcaster,moderator,error,discardReview})};`)(api);return {...h,counters:()=>({reads,writes}),fail(){fail=true;}};}
let h=harness();assert.equal(h.canLeave(),false);h.requestReload();assert.deepEqual(h.counters(),{reads:0,writes:0});h.cancel();await h.confirmReload();assert.equal(h.state().broadcaster,'draft');
for(const action of ['change','switchSource']){h=harness();h.requestReload();h[action]();await h.confirmReload();assert.equal(h.counters().reads,0);assert.ok(h.state().error);}
h=harness();h.requestReload();await h.save();assert.equal(h.counters().writes,0);h.lock();await h.confirmReload();assert.equal(h.counters().reads,0);
h=harness();h.requestReload();h.fail();await h.confirmReload();assert.equal(h.state().broadcaster,'draft');assert.equal(h.state().moderator,'draftbot');assert.equal(h.state().error,'read failed');
h=harness();h.requestReload();await h.confirmReload();assert.equal(h.state().broadcaster,'saved');assert.equal(h.state().moderator,'savedbot');await h.confirmReload();assert.equal(h.counters().reads,1);
console.log('Twitch reload: navigation/cancel retain input, stale input/target rejected, save/busy guarded, failed read retains both logins, confirmed read restores both.');

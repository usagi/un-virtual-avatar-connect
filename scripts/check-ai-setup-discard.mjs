import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/resources/DesktopAiSetupPanel.svelte',import.meta.url),'utf8');
assert.ok(!source.includes('window.confirm'));
const ast=ts.createSourceFile('panel.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const names=['canDiscard','requestDiscard','finishDiscard','confirmDiscard'];
const code=ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&names.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const make=()=>new Function(`let busy=false,loading=false,dirty=true,discardReview=null,error=null,content='draft',status={revision:'v1'},apiKey='secret',baseline='',editing=true,loads=0,applied=0;
function draft(){return content;} function applyStatus(){applied++;} async function load(){loads++;}
${code}
return {canDiscard,requestDiscard,confirmDiscard,cancel(){discardReview=null;},change(){content='newer';},saving(){busy=true;},state:()=>({discardReview,error,loads,applied,apiKey,editing})};`)();
let h=make(); assert.equal(h.canDiscard(),false);assert.equal(h.state().applied,0);
h.requestDiscard('cancel'); assert.equal(h.state().apiKey,'secret');h.cancel();await h.confirmDiscard();assert.equal(h.state().applied,0);
h.requestDiscard('cancel');h.change();await h.confirmDiscard();assert.equal(h.state().applied,0);assert.ok(h.state().error);
h=make();h.requestDiscard('reload');h.saving();await h.confirmDiscard();assert.equal(h.state().loads,0);
h=make();h.requestDiscard('reload');await h.confirmDiscard();assert.equal(h.state().loads,1);await h.confirmDiscard();assert.equal(h.state().loads,1);
h=make();h.requestDiscard('cancel');await h.confirmDiscard();assert.equal(h.state().applied,1);assert.equal(h.state().apiKey,'');assert.equal(h.state().editing,false);
console.log('AI setup discard: navigation retains input; cancellation, stale input, pending save, explicit reload and cancel verified.');
// Empty initial profiles must reset absent saved fields, not adopt the edited draft.
const applyCode=ts.transpileModule(ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='applyStatus').getText(ast),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const apply=new Function(`const defaultInstructions='default role';let displayName='edited',model='edited',instructions='edited',observeUser=false,observeTwitchChat=false,observeChannelPoints=false,observeRuntime=false,extraTriggers='',avatarActions=false,avatarLinkId='edited',avatarBaseKey='edited',avatarPublish=false;${applyCode};return next=>{applyStatus(next);return {displayName,instructions,avatarLinkId,avatarBaseKey};};`)();
assert.deepEqual(apply({observe_triggers:[],avatar_actions:false,avatar_publish:false}),{displayName:'AIパートナー',instructions:'default role',avatarLinkId:'ai_avatar',avatarBaseKey:'un-motion/frame/ai-avatar'});
assert.deepEqual(apply({display_name:'saved',custom_instructions:'saved role',avatar_link_id:'saved link',avatar_base_key:'saved key',observe_triggers:[]}),{displayName:'saved',instructions:'saved role',avatarLinkId:'saved link',avatarBaseKey:'saved key'});
console.log('AI setup reload: absent saved values restore defaults; explicit saved values retained.');

// The setup API edits the running profile, not the next-launch default or the
// independent profile picker in the perception panel.
const loadCode=ts.transpileModule(ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='load').getText(ast),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const loadFixture=new Function('inventory',`let busy=false,loading=false,error=null,loadError=null,available=false,currentProfile=null,status=null,apiKey='',baseline='',editing=false;
const api={desktopProfiles:async()=>inventory,desktopAiSetup:async()=>({configured:true,enabled:true,credential_source:'profile_file'})};
function applyStatus(){} function draft(){return '';} function message(e){return String(e);}
${loadCode}
return {load,state:()=>({currentProfile,error,loading,available})};`);
let fixture=loadFixture({managed_mode:true,current_profile_id:'running',active_profile_id:'next',profiles:[{id:'next',label:'Same',conf_path:'next/conf.toml'},{id:'running',label:'Same',conf_path:'running/conf.toml'}]});
await fixture.load(); assert.equal(fixture.state().currentProfile.id,'running');assert.equal(fixture.state().currentProfile.conf_path,'running/conf.toml');
fixture=loadFixture({managed_mode:true,current_profile_id:null,active_profile_id:'next',profiles:[{id:'next'}]});
await fixture.load();assert.equal(fixture.state().currentProfile,null);
console.log('AI setup identity: running profile shown even when next default differs; unknown identity never falls back to next default.');

// A failed read must never enable an unversioned save of the default form.
// Exercise the real load/save functions, including failure after a prior read.
const saveCode=ts.transpileModule(ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='saveAndRestart').getText(ast),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const recoveryFixture=new Function(`let busy=false,loading=false,error=null,loadError=null,available=false,currentProfile=null,status=null,apiKey='retained',baseline='',editing=false,discardReview=null;
let failure='settings',applied=0,saveReached=0;
const api={desktopProfiles:async()=>{if(failure==='inventory')throw Error('inventory unavailable');return {managed_mode:true,profiles:[]};},desktopAiSetup:async()=>{if(failure==='settings')throw Error('settings unavailable');return {configured:true,enabled:true,credential_source:'profile_file',revision:'saved-revision'};}};
function applyStatus(){applied++;} function draft(){return 'saved draft';} function message(e){return e.message;}
function triggers(){saveReached++;throw Error('save guard was bypassed');}
${loadCode}\n${saveCode}
return {load,saveAndRestart,fail(value){failure=value;},state:()=>({loadError,status,apiKey,applied,saveReached,loading})};`);
for(const failure of ['inventory','settings']) {
 const h=recoveryFixture();h.fail(failure);await h.load();await h.saveAndRestart(false);
 assert.equal(h.state().saveReached,0);assert.equal(h.state().applied,0);assert.equal(h.state().apiKey,'retained');assert.ok(h.state().loadError);assert.equal(h.state().loading,false);
 h.fail(null);await h.load();assert.equal(h.state().loadError,null);assert.equal(h.state().status.revision,'saved-revision');assert.equal(h.state().applied,1);
 h.fail('settings');await h.load();await h.saveAndRestart(false);assert.equal(h.state().saveReached,0);assert.equal(h.state().applied,1);
}
assert.ok(source.includes('{:else if loadError}'));
assert.equal((source.match(/>もう一度読み込む<\/button>/g)||[]).length,2);
console.log('AI setup read recovery: inventory/settings failures block save; retry recovers revision; failed reload cannot save stale status.');

// A response arriving after further typing must not mark the newer input saved.
const draftCode=ts.transpileModule(ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='draft').getText(ast),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const pendingSaveFixture=new Function(`
let busy=false,loading=false,loadError=null,status={revision:'v1',persona_id:'companion'},discardReview=null,error=null;
let displayName='submitted',model='test-model',apiKey='first-test-key',instructions='role',observeUser=true,observeTwitchChat=false,observeChannelPoints=false,observeRuntime=false,extraTriggers='',avatarActions=false,avatarLinkId='link',avatarBaseKey='base',avatarPublish=false;
let baseline='',savedPending=false,restartOpen=false,currentProfile=null,onsaved=null;
const confSyncStore={markSavedRequiringRestart(){}},toastStore={success(){}};
let resolve,reject,payload;
const api={configureDesktopAi:value=>{payload=value;return new Promise((yes,no)=>{resolve=yes;reject=no;});}};
function triggers(){return ['user'];}function message(e){return e.message;}
${draftCode}
${saveCode}
return {saveAndRestart,edit(){displayName='newer';apiKey='second-test-key';},complete(){resolve({status:{revision:'v2',persona_id:'companion'}});},fail(){reject(Error('save failed'));},state:()=>({displayName,apiKey,baseline,dirty:draft()!==baseline,restartOpen,busy,payload,status,error})};`);
let pending=pendingSaveFixture();let saving=pending.saveAndRestart(true);pending.edit();pending.complete();await saving;
assert.equal(pending.state().displayName,'newer');assert.equal(pending.state().apiKey,'second-test-key');assert.equal(pending.state().dirty,true);assert.equal(pending.state().restartOpen,false);assert.equal(pending.state().payload.display_name,'submitted');assert.equal(pending.state().status.revision,'v2');
pending=pendingSaveFixture();saving=pending.saveAndRestart(true);pending.complete();await saving;
assert.equal(pending.state().dirty,false);assert.equal(pending.state().apiKey,'');assert.equal(pending.state().restartOpen,true);
pending=pendingSaveFixture();saving=pending.saveAndRestart(true);pending.edit();pending.fail();await saving;
assert.equal(pending.state().apiKey,'second-test-key');assert.equal(pending.state().dirty,true);assert.equal(pending.state().status.revision,'v1');assert.equal(pending.state().busy,false);
console.log('AI setup pending save: newer inputs remain unsaved, newer key retained, restart deferred; unchanged success and failure preserved.');

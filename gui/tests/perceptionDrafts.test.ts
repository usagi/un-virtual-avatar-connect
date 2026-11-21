import assert from 'node:assert/strict';
import test from 'node:test';
import {PerceptionDrafts,perceptionEditKey} from '../src/lib/perceptionDrafts.ts';
import type {PerceptionEdit,PerceptionView,PerceptionSaveResult} from '../src/lib/perceptionSetupTypes.ts';
const a={instance:'vac-a',profile:'same'}, b={instance:'vac-b',profile:'same'};
const view=(revision='r1'):PerceptionView=>({revision,sources:[],archived_sources:[],history:[]});
const edit=(name:string):PerceptionEdit=>({kind:'source',source_id:'game',patch:{source_name:name}});

test('late save error belongs to its captured target and operation until retry succeeds',async()=>{
 const failed=deferred<PerceptionSaveResult>();let calls=0;
 const drafts=new PerceptionDrafts({read:async()=>view(),save:async()=>++calls===1?failed.promise:{revision:'r2',restart_required:true}});
 await drafts.load(a);await drafts.load(b);
 const source=edit('keep'), slot=perceptionEditKey(source);
 drafts.stage(a,source);const saving=drafts.save(a,slot);
 drafts.stage(a,{kind:'history',persona_index:0,visual_history_enabled:true});
 drafts.stage(b,edit('peer'));
 failed.reject(new Error('invalid source'));assert.equal(await saving,false);
 assert.equal(drafts.get(a).errorEditKey,slot);
 assert.equal(drafts.get(b).errorEditKey,null);assert.equal(drafts.get(b).error,'');
 assert.equal(drafts.get(a).pending.size,2);
 assert.equal(await drafts.save(a,slot),true);
 assert.equal(drafts.get(a).errorEditKey,null);assert.equal(drafts.get(a).error,'');
 assert.equal(drafts.get(a).pending.size,1);assert.equal(drafts.get(b).pending.size,1);
});
test('leaving protects hidden targets until each draft or pending save is settled',async()=>{
 const pending=deferred<PerceptionSaveResult>();
 const drafts=new PerceptionDrafts({read:async()=>view(),save:async()=>pending.promise});
 await drafts.load(a);await drafts.load(b);
 assert.deepEqual(drafts.unsettledTargets(),[]);
 drafts.stage(a,edit('hidden'));drafts.stage(b,edit('visible'));
 assert.deepEqual(drafts.unsettledTargets(),[a,b]);
 drafts.discard(b);
 const saving=drafts.save(a,perceptionEditKey(edit('hidden')));
 assert.deepEqual(drafts.unsettledTargets(),[a]);
 pending.resolve({revision:'r2',restart_required:true});await saving;
 assert.deepEqual(drafts.unsettledTargets(),[]);
});
function deferred<T>() { let resolve!:(value:T)=>void; let reject!:(error:Error)=>void; const promise=new Promise<T>((yes,no)=>{resolve=yes;reject=no;});return {promise,resolve,reject}; }

test('late save stays on captured instance/profile and retains newer typing',async()=>{
 const pending=deferred<PerceptionSaveResult>(); const calls:unknown[]=[];
 const drafts=new PerceptionDrafts({read:async()=>view(),save:async(target,revision,value)=>{calls.push([target,revision,value]);return pending.promise;}});
 await drafts.load(a);await drafts.load(b);drafts.stage(a,edit('first'));drafts.stage(b,edit('peer'));
 const mutable={...a};const saving=drafts.save(mutable,perceptionEditKey(edit('first')));
 mutable.instance=b.instance;drafts.stage(a,edit('newer'));
 pending.resolve({revision:'r2',restart_required:true});assert.equal(await saving,true);
 assert.deepEqual(calls,[[a,'r1',edit('first')]]);
 assert.deepEqual(drafts.get(a).pending.get(perceptionEditKey(edit('first'))),edit('newer'));
 assert.deepEqual(drafts.get(b).pending.get(perceptionEditKey(edit('peer'))),edit('peer'));
 assert.equal(drafts.get(b).view?.revision,'r1');assert.equal(drafts.get(a).needsRefresh,true);
});

test('conflict retains draft and baseline instead of rebasing onto newer settings',async()=>{
 const drafts=new PerceptionDrafts({read:async()=>view(),save:async()=>{throw new Error('409 profile_changed');}});
 await drafts.load(a);drafts.stage(a,edit('keep'));
 assert.equal(await drafts.save(a,perceptionEditKey(edit('keep'))),false);
 assert.equal(drafts.get(a).pending.size,1);assert.equal(drafts.get(a).view?.revision,'r1');
 assert.match(drafts.get(a).error,/409/);assert.equal(await drafts.load(a),false);
 await drafts.load(b);drafts.stage(b,edit('peer'));
 assert.equal(drafts.discard(a),true);assert.equal(await drafts.load(a),true);
 assert.equal(drafts.get(b).pending.size,1);
});

test('out-of-order reads and edits during reload cannot overwrite the baseline',async()=>{
 const first=deferred<PerceptionView>(),second=deferred<PerceptionView>();let n=0;
 const drafts=new PerceptionDrafts({read:()=>++n===1?first.promise:second.promise,save:async()=>({revision:'r3',restart_required:true})});
 const x=drafts.load(a),y=drafts.load(a);second.resolve(view('r2'));assert.equal(await y,true);
 first.resolve(view('r1'));assert.equal(await x,false);assert.equal(drafts.get(a).view?.revision,'r2');
 const reload=drafts.load(a);drafts.stage(a,edit('keep'));assert.equal(await reload,false);
 assert.equal(drafts.get(a).pending.size,1);
});

test('successful save clears only sent edit and prevents overlapping saves',async()=>{
 const pending=deferred<PerceptionSaveResult>();let calls=0;
 const drafts=new PerceptionDrafts({read:async()=>view(),save:async()=>{calls++;return pending.promise;}});
 await drafts.load(a);drafts.stage(a,edit('one'));
 const other:PerceptionEdit={kind:'source',source_id:'board',patch:{source_name:'two'}};drafts.stage(a,other);
 const saving=drafts.save(a,perceptionEditKey(edit('one')));
 assert.equal(drafts.discard(a),false);
 assert.equal(await drafts.save(a,perceptionEditKey(other)),false);
 pending.resolve({revision:'r2',restart_required:true});assert.equal(await saving,true);
 assert.equal(calls,1);assert.equal(drafts.get(a).pending.size,1);assert.deepEqual(drafts.get(a).pending.get(perceptionEditKey(other)),other);
});

test('editing two fields of one source retains both changes in the request',async()=>{
 let sent:PerceptionEdit|undefined;
 const drafts=new PerceptionDrafts({read:async()=>view(),save:async(_target,_revision,value)=>{sent=value;return {revision:'r2',restart_required:true};}});
 await drafts.load(a);drafts.stage(a,edit('renamed'));
 drafts.stage(a,{kind:'source',source_id:'game',patch:{refresh_before_reply:true}});
 assert.equal(await drafts.save(a,perceptionEditKey(edit('renamed'))),true);
 assert.deepEqual(sent,{kind:'source',source_id:'game',patch:{source_name:'renamed',refresh_before_reply:true}});
});


test('AI setup save refreshes only its profile and preserves hidden drafts',async()=>{
 const reads:string[]=[];let revision='r1';
 const drafts=new PerceptionDrafts({read:async target=>{reads.push(target.instance);return view(revision);},save:async()=>({revision:'r3',restart_required:true})});
 await drafts.load(a);await drafts.load(b);drafts.stage(b,edit('peer draft'));
 revision='r2';assert.equal(await drafts.refreshAfterExternalSave(a),true);
 assert.equal(drafts.get(a).view?.revision,'r2');
 assert.equal(drafts.get(b).view?.revision,'r1');
 assert.deepEqual(drafts.get(b).pending.get(perceptionEditKey(edit('peer draft'))),edit('peer draft'));
 drafts.stage(a,edit('keep mine'));const count=reads.length;
 assert.equal(await drafts.refreshAfterExternalSave(a),false);
 assert.equal(reads.length,count);assert.equal(drafts.get(a).view?.revision,'r2');
 assert.equal(drafts.get(a).needsRefresh,true);assert.match(drafts.get(a).error,/入力は保持/);
 assert.deepEqual(drafts.get(a).pending.get(perceptionEditKey(edit('keep mine'))),edit('keep mine'));
});

test('external save never rereads a perception save in flight',async()=>{
 const pending=deferred<PerceptionSaveResult>();let reads=0;
 const drafts=new PerceptionDrafts({read:async()=>{reads++;return view();},save:async()=>pending.promise});
 await drafts.load(a);drafts.stage(a,edit('in flight'));
 const saving=drafts.save(a,perceptionEditKey(edit('in flight')));
 assert.equal(await drafts.refreshAfterExternalSave(a),false);assert.equal(reads,1);
 assert.equal(drafts.get(a).pending.size,1);
 pending.reject(new Error('409 newer AI settings'));assert.equal(await saving,false);
 assert.equal(drafts.get(a).pending.size,1);assert.equal(drafts.get(a).view?.revision,'r1');
});

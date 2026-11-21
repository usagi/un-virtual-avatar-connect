import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const receiptSource=fs.readFileSync(new URL('../gui/src/lib/control/dictionary/quickAddReceipt.ts',import.meta.url),'utf8');
const receiptCode=ts.transpileModule(receiptSource,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText;
const {dictionaryRequestOutcome}=await import(`data:text/javascript;base64,${Buffer.from(receiptCode).toString('base64')}`);
const source = fs.readFileSync(new URL('../gui/src/lib/control/dictionary/DictionaryLiveQuickAdd.svelte', import.meta.url), 'utf8');
const ast = ts.createSourceFile('quick.ts', source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1], ts.ScriptTarget.Latest, true);
const code = ts.transpileModule(ast.statements.filter(n => ts.isFunctionDeclaration(n) && n.name?.text === 'onSubmit').map(n => n.getText(ast)).join('\n'), {compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
for (const format of ['direct','control_meta']) for (const scenario of ['unchanged', 'switch', 'removed', 'new-input', 'failure']) {
 const history = [], reloads = [], calls = [];
 let resolve, reject;
 const api = {triggerFlowgraphNode(...args) {calls.push(args); return new Promise((a,b) => {resolve=a; reject=b;});}};
 const store = {currentKey:'a', reloadCurrent(){reloads.push(this.currentKey);}};
 const h = new Function('api','dictionaryEditorStore','pushHistory','format','dictionaryRequestOutcome',`
 let selectedTable={key:'a',quick_add:{node_id:'a.learn',forget_node_id:'a.forget',request_format:format}};
 let canLearn=true,inputReading='read',inputSurface='word',effectiveKind='literal',submitting=false,nextSeq=1;
 const toastStore={success(){}},handleTriggerError=()=>{};
 ${code}
 return {run:onSubmit, change(kind){if(kind==='switch')selectedTable={key:'b',quick_add:{node_id:'b.learn'}};if(kind==='removed')selectedTable=null;if(kind==='new-input')inputSurface='new';},state:()=>({inputReading,inputSurface,submitting})};
 `)(api,store,item=>history.push(item),format,dictionaryRequestOutcome);
 const pending=h.run();
 assert.equal(calls[0][0],'a.learn');
 assert.deepEqual(calls[0][1].inputs,format==='control_meta'
  ? {source_actor:'gui:quick_add',meta:{reading:'read',surface:'word',part_of_speech:'名詞'}}
  : {reading:'read',surface:'word',part_of_speech:'名詞',by:'gui:quick_add'});
 h.change(scenario);
 if(scenario==='switch')store.currentKey='b';
 if(scenario==='failure')reject(Error('offline'));else resolve({});
 await pending;
 assert.equal(h.state().submitting,false);
 assert.equal(history.length,scenario==='failure'?0:1);
 if(history.length)assert.equal(history[0].target_key,'a');
 assert.equal(h.state().inputSurface,scenario==='unchanged'?'':scenario==='new-input'?'new':'word');
 assert.equal(reloads.length,['unchanged','new-input'].includes(scenario)?1:0);
}
console.log('Quick-add: delayed success stays with submitted table; switching/removal/new input/failure preserve drafts.');
const functions = ['loadHistory','persistHistory','canUndo'];
const historyCode = ts.transpileModule(ast.statements.filter(n => ts.isFunctionDeclaration(n) && functions.includes(n.name?.text)).map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const disk=new Map();
const storage={getItem:k=>disk.get(k)??null,setItem:(k,v)=>disk.set(k,v)};
function scoped(scope) {
 return new Function('localStorage','storageScope',`const LS_HISTORY_PREFIX='vac.dictQuickAdd.history.';let nextSeq=1,undoing=null;
 let selectedTable={key:'same',path:'a.tsv',quick_add:{node_id:'learn',forget_node_id:'forget'}};
 ${historyCode}
 return {load:loadHistory,save:persistHistory,canUndo,select:v=>selectedTable=v};`)(storage,scope);
}
const item={seq:1,target_key:'same',table_path:'a.tsv',node_id:'learn',forget_node_id:'forget',reading:'r',surface:'s',part_of_speech:'名詞',at:new Date().toISOString()};
disk.set('vac.dictQuickAdd.history.same',JSON.stringify([item]));
const a=scoped('profileA'),b=scoped('profileB'),unknown=scoped(null);
assert.deepEqual(a.load('same'),[]); // unscoped legacy data is not assigned to a profile
assert.ok(disk.has('vac.dictQuickAdd.history.same')); // not deleted
 a.save('same',[item]); assert.deepEqual(a.load('same'),[{...item,undone:undefined}]);assert.deepEqual(b.load('same'),[]);
unknown.save('same',[item]);assert.deepEqual(unknown.load('same'),[]);
assert.equal(a.canUndo(item),true);
for(const change of [{request_format:'control_meta'},{table_path:'b.tsv'},{node_id:'other'},{forget_node_id:'other'},{target_key:'other'},{table_path:undefined},{undone:true}])assert.equal(a.canUndo({...item,...change}),false);
a.select(null);assert.equal(a.canUndo(item),false);
console.log('Quick-add history: profile isolation, legacy preservation, unknown identity fallback and stale undo guards passed.');
const undoCode = ts.transpileModule(ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='onUndo').map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
for(const format of ['direct','control_meta']) {
 const calls=[];
 const run=new Function('api','dictionaryRequestOutcome',`let selectedTable=null,undoing=null;const canUndo=()=>true;const updateHistoryItem=()=>{};const toastStore={success(){}};const dictionaryEditorStore={currentKey:null};const handleTriggerError=e=>{throw e;};${undoCode};return onUndo;`)({triggerFlowgraphNode:async(...args)=>{calls.push(args);return {}; }},dictionaryRequestOutcome);
 await run({...item,request_format:format});
 assert.deepEqual(calls[0],['forget',{inputs:format==='control_meta'?{source_actor:'gui:quick_add',meta:{surface:'s',reading:'r'}}:{surface:'s',reading:'r',mode:'latest'}}]);
}
console.log('Quick-add and undo: legacy/direct and control metadata request contracts passed.');
for(const result of ['saved','unchanged','conflict','failed','pending','missing']) {
 const rules=[{node_id:'save',port:'ok',outcome:'saved'},{node_id:'save',port:'conflict',outcome:'conflict'},{node_id:'edit',port:'duplicate',outcome:'unchanged'}];
 const outputs=result==='saved'?[['save','ok']]:result==='unchanged'?[['edit','duplicate']]:result==='conflict'?[['save','conflict']]:[];
 const response=result==='missing'?{}:{receipt:{phase:result==='pending'?'running':result==='failed'?'failed':'finished',generation:2,outputs}};
 const writes=[],history=[],calls=[],errors=[];
 const api={triggerFlowgraphNode:async(...args)=>{calls.push(args);return response;}};
 const submit=new Function('api','dictionaryRequestOutcome','rules','pushHistory','errors',`
 let selectedTable={key:'a',path:'a.tsv',quick_add:{node_id:'entry',forget_node_id:'undo',request_format:'control_meta',learn_observe:rules}};
 let canLearn=true,inputReading='read',inputSurface='word',effectiveKind='literal',submitting=false,nextSeq=1;
 const toastStore={success(){},error(...e){errors.push(e);}},handleTriggerError=e=>{throw e;};
 const dictionaryEditorStore={currentKey:null};${code}
 return {run:onSubmit,state:()=>({inputReading,inputSurface,submitting})};`)(api,dictionaryRequestOutcome,rules,x=>history.push(x),errors);
 await submit.run();
 const complete=['saved','unchanged'].includes(result);
 assert.equal(submit.state().inputSurface,complete?'':'word');
 assert.equal(history.length,result==='saved'?1:0);
 assert.equal(errors.length,complete?0:1);
 assert.equal(calls.length,1);assert.equal(calls[0][1].wait_ms,2000);
 const undo=new Function('api','dictionaryRequestOutcome','rules','updateHistoryItem','errors',`
 let selectedTable={quick_add:{forget_observe:rules}},undoing=null;
 const canUndo=()=>true;const toastStore={success(){},error(...e){errors.push(e);}};
 const dictionaryEditorStore={currentKey:null};const handleTriggerError=e=>{throw e;};${undoCode};return onUndo;`)(api,dictionaryRequestOutcome,rules,(...x)=>writes.push(x),errors);
 await undo({...item,request_format:'control_meta'});
 assert.equal(writes.length,complete?1:0);
}
console.log('Observed quick-add: save/no-op handled; conflict/failure/timeout/missing receipt retain drafts and undo history; no automatic retry.');

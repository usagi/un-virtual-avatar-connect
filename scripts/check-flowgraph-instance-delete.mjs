import fs from 'node:fs';
import assert from 'node:assert/strict';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraphStore.svelte.ts',import.meta.url),'utf8');
const ast=ts.createSourceFile('store.ts',source,ts.ScriptTarget.Latest,true);
const cls=ast.statements.find(n=>ts.isClassDeclaration(n)&&n.name.text==='FlowgraphStore');
const methods=['removeSelection','undoLastDelete','undo','redo','#pushHistory','#snapshot','#restoreSnapshot','updateNodeProperty'];
const helpers=['cloneNodes','cloneEdges','cloneGroups','cloneInstances','pruneGroupsAfterNodeRemoval','edgeMentionsAny','parsePortRef','findLastSinglePortColon'];
const body=cls.members.filter(n=>ts.isMethodDeclaration(n)&&methods.includes(n.name.getText(ast))).map(n=>n.getText(ast)).join('\n');
const functions=ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&helpers.includes(n.name.text)).map(n=>n.getText(ast).replace(/^export /,'')).join('\n');
const code=ts.transpileModule(`${functions}\nclass Subject { #lastDeletion=null; undoStack=[]; redoStack=[]; ${body} }`,{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const Subject=new Function(code+';return Subject;')();
for(const connected of [false,true]){
 const s=new Subject();Object.assign(s,{draftNodes:[{id:'keep',feature:'flowgraph.literal.string',position:null,properties:{value:'safe'}}],draftInstances:[{id:'part',graph:'./module',position:[41,92]}],draftEdges:connected?[{from:'part:out',to:'keep:value'}]:[],draftGroups:[],selectedNodeId:'part',selectedNodeIds:['part'],selectedGroupId:null});
 const before=structuredClone({nodes:s.draftNodes,instances:s.draftInstances,edges:s.draftEdges});
 const result=s.removeSelection(['part'],[]);
 assert.equal(result.removedNodes,1,'instance must be removed, not only its edges');
 assert.deepEqual(s.draftInstances,[]);assert.deepEqual(s.draftNodes,before.nodes);assert.deepEqual(s.draftEdges,[]);
 assert.equal(s.undoLastDelete(),true);assert.deepEqual(s.draftInstances,before.instances);assert.deepEqual(s.draftNodes,before.nodes);assert.deepEqual(s.draftEdges,before.edges);
}
console.log('PASS: connected and isolated graph instance deletion/undo retain normal nodes and restore instance reference, position and edges');


const grouped=new Subject();Object.assign(grouped,{draftNodes:[],draftInstances:[{id:'part',graph:'./module',position:[1,2]}],draftEdges:[],draftGroups:[{id:'g',label:'Parts',node_ids:['part'],color:null}],selectedNodeId:'part',selectedNodeIds:['part'],selectedGroupId:'g'});
const groupBefore=structuredClone(grouped.draftGroups);
grouped.removeSelection(['part'],[]);assert.deepEqual(grouped.draftGroups,[]);
assert.equal(grouped.undoLastDelete(),true);assert.deepEqual(grouped.draftGroups,groupBefore,'toast undo must restore group membership');assert.equal(grouped.selectedGroupId,'g');
assert.equal(grouped.redo(),true);assert.deepEqual(grouped.draftInstances,[]);assert.deepEqual(grouped.draftGroups,[]);
console.log('PASS: deletion toast undo restores group and selection, and redo deletes again');
const stale=new Subject();Object.assign(stale,{draftNodes:[{id:'keep',feature:'flowgraph.literal.string',position:null,properties:{value:'old'}}],draftInstances:[{id:'part',graph:'./module',position:null}],draftEdges:[],draftGroups:[],selectedNodeId:'part',selectedNodeIds:['part'],selectedGroupId:null});
stale.removeSelection(['part'],[]);stale.updateNodeProperty('keep','value','new');
assert.equal(stale.undoLastDelete(),false);assert.equal(stale.draftNodes[0].properties.value,'new');assert.deepEqual(stale.draftInstances,[]);
assert.equal(stale.undo(),true);assert.equal(stale.draftNodes[0].properties.value,'old');assert.equal(stale.undo(),true);assert.equal(stale.draftInstances[0].id,'part');
console.log('PASS: stale delete toast cannot undo later edits; ordered undo still recovers both edits');

const sequential=new Subject();Object.assign(sequential,{draftNodes:[{id:'a',feature:'flowgraph.literal.string',properties:{}},{id:'b',feature:'flowgraph.literal.string',properties:{}}],draftInstances:[],draftEdges:[],draftGroups:[],selectedNodeIds:[]});
const first=sequential.removeSelection(['a'],[]),second=sequential.removeSelection(['b'],[]);
assert.equal(first.undoDelete(),false,'First notification must not undo second deletion');
assert.deepEqual(sequential.draftNodes,[]);
assert.equal(second.undoDelete(),true);assert.deepEqual(sequential.draftNodes.map(n=>n.id),['b']);
assert.equal(second.undoDelete(),false,'Consumed notification cannot undo twice');
assert.equal(sequential.undo(),true);assert.deepEqual(sequential.draftNodes.map(n=>n.id),['a','b']);
console.log('PASS: each deletion notification is bound to its own history entry; later deletion and repeated action cannot be undone accidentally');


import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from '../gui/node_modules/typescript/lib/typescript.js';
const source=fs.readFileSync(new URL('../gui/src/lib/flowgraphStore.svelte.ts',import.meta.url),'utf8');
const ast=ts.createSourceFile('store.ts',source,ts.ScriptTarget.Latest,true);
const cls=ast.statements.find(n=>ts.isClassDeclaration(n)&&n.name.text==='FlowgraphStore');
const method=cls.members.find(n=>ts.isMethodDeclaration(n)&&n.name.getText(ast)==='refreshTree');
const code=ts.transpileModule(`class Subject { ${method.getText(ast)} }`,{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
for(const lateFailure of [false,true]){
 const requests=[];
 const Subject=new Function('api','describeError',`${code};return Subject;`)({flowgraphTree:()=>new Promise((resolve,reject)=>requests.push({resolve,reject}))},String);
 const subject=Object.assign(new Subject(),{treeRequest:0,tree:{root_dir:'before'},treeState:'ok',treeError:null});
 const old=subject.refreshTree();const fresh=subject.refreshTree();
 requests[1].resolve({root_dir:'new-root',files:['newly-imported']});await fresh;
 if(lateFailure)requests[0].reject(new Error('old failure'));else requests[0].resolve({root_dir:'old-root',files:[]});
 await old;
 assert.equal(subject.tree.root_dir,'new-root');
 assert.deepEqual(subject.tree.files,['newly-imported']);
 assert.equal(subject.treeState,'ok');assert.equal(subject.treeError,null);
 const pending=subject.refreshTree();requests[2].reject(new Error('current failure'));await pending;
 assert.equal(subject.treeState,'error');assert.match(subject.treeError,/current failure/);
 assert.equal(subject.tree.root_dir,'new-root','failed refresh preserves last available inventory');
}
console.log('PASS: old tree responses cannot replace post-operation inventory; current errors preserve last data');

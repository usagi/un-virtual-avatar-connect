import test from 'node:test';
import assert from 'node:assert/strict';
import {speechRouteView} from '../src/lib/control/dictionary/speechRouteView.ts';
const base={ok:true,root_dir:'C:/profile/flowgraph',inactive_exec_nodes:[] as string[],source_digests:{main:'loaded-v1'}};
test('old server is not represented as an empty graph',()=>{
    assert.equal(speechRouteView(base).kind,'unavailable');
    const empty=speechRouteView({...base,speech_routes:[]});
    assert.equal(empty.kind,'ready');if(empty.kind==='ready')assert.equal(empty.routes.length,0);
    assert.equal(speechRouteView({...base,ok:false,speech_routes:[]}).kind,'invalid');
});
test('shared instances and mode suppression remain distinct without mutating the response',()=>{
    const routes=['left','right'].map(name=>({runtime_node:`main/@${name}::speaker`,source_file:'C:/profile/voice.flowgraph.toml',source_graph:'voice',instance_path:name,definition_instances:2,incoming:[{from_node:'main::input',from_port:'content',to_port:'text',exec:false}]}));
    const result=speechRouteView({...base,speech_routes:routes,inactive_exec_nodes:[routes[1].runtime_node]});
    assert.equal(result.kind,'ready');if(result.kind!=='ready')return;
    assert.deepEqual(result.routes.map(r=>r.modeSuppressed),[false,true]);
    assert.deepEqual(result.routes.map(r=>r.definition_instances),[2,2]);
    result.routes[0].incoming[0].from_node='changed';
    assert.equal(routes[0].incoming[0].from_node,'main::input');
    assert.deepEqual(result.digests,{main:'loaded-v1'});
});

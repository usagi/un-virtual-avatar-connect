import test from 'node:test';
import assert from 'node:assert/strict';
import {speechConnectionPlan} from '../src/lib/control/dictionary/speechConnectionPlan.ts';

const route=(instance:string)=>({runtime_node:`main/@${instance}::speaker`,source_file:'voice.toml',source_graph:'voice',instance_path:instance,definition_instances:2,modeSuppressed:false,
    incoming:[{from_node:`${instance}::input`,from_port:'content',to_port:'text',exec:false},
        {from_node:`${instance}::input`,from_port:'exec_out',to_port:'exec_in',exec:true},
        {from_node:'settings::engine',from_port:'value',to_port:'engine',exec:false}]});
const view=()=>({kind:'ready' as const,root:'graphs',digests:{main:'v1',voice:'v2'},routes:[route('left'),route('right')]});
test('definition edit enumerates both copies and preserves voice settings',()=>{
    const input=view(),plan=speechConnectionPlan(input,'main/@left::speaker');
    assert.deepEqual(plan.affected,['main/@left::speaker','main/@right::speaker']);
    assert.deepEqual(plan.blockers,[]);
    assert.deepEqual(plan.paths.map(p=>p.retained[0].to_port),['engine','engine']);
    plan.paths[0].text[0].from_node='changed';
    assert.equal(input.routes[0].incoming[0].from_node,'left::input');
});
test('missing usage, revisions and direct invocation are not silently converted',()=>{
    const input=view();input.routes.pop();input.digests={};input.routes[0].incoming=[];
    const plan=speechConnectionPlan(input,'main/@left::speaker');
    assert.equal(plan.blockers.length,4);
    assert.deepEqual(plan.paths[0].text,[]);
});
test('obsolete selection does not fall back to a different speech destination',()=>{
    const plan=speechConnectionPlan(view(),'deleted::speaker');
    assert.equal(plan.blockers.length,1);assert.deepEqual(plan.affected,[]);
});

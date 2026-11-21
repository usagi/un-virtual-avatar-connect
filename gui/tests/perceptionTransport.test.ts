import test from 'node:test';
import assert from 'node:assert/strict';
import {createPerceptionTransport} from '../src/lib/perceptionTransport.ts';

test('transport binds VAC identity and preserves selected profile and revision',async()=>{
	const calls:unknown[]=[];
	const transport=createPerceptionTransport('vac-a',{
		async desktopProfilePerception(id){calls.push(['read',id]);return {revision:'r',sources:[],archived_sources:[],history:[]};},
		async saveDesktopProfilePerception(id,revision,edit){calls.push(['save',id,revision,edit]);return {revision:'next',restart_required:true};},
	});
	const edit={kind:'history' as const,persona_index:2,visual_history_enabled:true};
	await transport.read({instance:'vac-a',profile:'chosen'});
	await transport.save({instance:'vac-a',profile:'chosen'},'exact-revision',edit);
	await assert.rejects(transport.save({instance:'vac-b',profile:'chosen'},'r',edit),/一致/);
	await assert.rejects(transport.read({instance:'vac-a',profile:''}),/一致/);
	assert.deepEqual(calls,[['read','chosen'],['save','chosen','exact-revision',edit]]);
});

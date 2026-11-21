import assert from 'node:assert/strict';
import { visibleReadinessStatus } from '../src/lib/readinessPresentation.ts';

const optionalTwitch = { id:'twitch_events', area:'stream', status:'attention' as const,
  title:'Twitch', detail:'Not configured', required:false, action:'resources' };
const summary = {status:'ready' as const, mode:null, checks:[optionalTwitch]};
assert.equal(visibleReadinessStatus(summary), 'attention', 'Configured optional failure must appear on home');
assert.equal(visibleReadinessStatus(summary, true), 'ready', 'Required-only mode gate stays separate');
assert.equal(visibleReadinessStatus({...summary, checks:[{...optionalTwitch,status:'inactive'}]}), 'ready', 'Unused services do not create warnings');
assert.equal(visibleReadinessStatus({...summary, checks:[{...optionalTwitch,status:'blocked',required:true}]}), 'blocked');
assert.equal(visibleReadinessStatus({...summary, checks:[{...optionalTwitch,status:'ready'}]}), 'ready');
assert.equal(visibleReadinessStatus({...summary, checks:[{...optionalTwitch,status:'unverified'}]}), 'ready', 'Unverified observation is not a detected failure');
console.log('PASS home reports configured optional failures without warning on unused services');

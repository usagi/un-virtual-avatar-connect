import assert from 'node:assert/strict';
import { twitchInputPresentation } from '../src/lib/twitchInputPresentation.ts';
import type { TwitchIrcIngressSummary } from '../src/lib/types.ts';

const connected: TwitchIrcIngressSummary = {
  node_id: 'a', label: 'A', credential_key: null,
  configured_channels: ['alpha'], joined_channels: ['alpha'], ready: true,
};
const pending = { ...connected, node_id: 'b', configured_channels: ['beta'], joined_channels: [], ready: false };
const mixed = twitchInputPresentation([connected, pending]);
assert.equal(mixed.label, '1チャンネルを受信中 · 1入力が開始待ち');
assert.equal(mixed.detail, '#alpha / 開始待ち: #beta');
assert.equal(twitchInputPresentation([connected, { ...connected, node_id: 'duplicate' }]).label, '1チャンネルを受信中');
assert.equal(twitchInputPresentation([pending]).label, '開始待ち');
assert.equal(twitchInputPresentation([{ ...connected, joined_channels: [] }]).label, '開始待ち');
assert.equal(twitchInputPresentation([{ ...connected, ready: false }]).label, '開始待ち');
assert.equal(twitchInputPresentation([]).label, '未使用');
console.log('PASS configured or disconnected channels never inflate receiving count');

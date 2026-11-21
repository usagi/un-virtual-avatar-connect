import assert from 'node:assert/strict';
import test from 'node:test';
import {perceptionError} from '../src/lib/perceptionError.js';

test('structured API detail stays readable while diagnostics remain available',()=>{
 const error=Object.assign(new Error('Control API 400 Bad Request: internal JSON'),{status:400,body:{error:'profile_store_error',detail:'有効な環境変数名を指定してください'}});
 const shown=perceptionError(error,'save');
 assert.equal(shown.message,'有効な環境変数名を指定してください');
 assert.match(shown.diagnostic,/Control API 400/);
 for(const field of ['reason','message']) {
  assert.equal(perceptionError(Object.assign(new Error('transport'),{status:409,body:{[field]:'再読込してください'}}),'save').message,'再読込してください');
 }
});

test('unstructured or lost responses do not claim that a save did not reach the server',()=>{
 for(const error of [new TypeError('Failed to fetch'),null,Object.assign(new Error('Control API 502'),{status:502,body:'<html>gateway</html>'}),Object.assign(new Error('Control API 400'),{status:400,body:{detail:{unexpected:true},error:'raw_code'}})]) {
  assert.match(perceptionError(error,'save').message,/保存結果を確認できませんでした/);
  assert.match(perceptionError(error,'read').message,/読み込めませんでした/);
 }
 assert.equal(perceptionError(new Error('別の画面で設定が変更されています'),'save').message,'別の画面で設定が変更されています');
});

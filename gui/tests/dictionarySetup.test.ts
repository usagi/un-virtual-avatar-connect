import assert from 'node:assert/strict';
import test from 'node:test';
import { DictionarySetupReview, prepareDictionarySetupReview } from '../src/lib/control/dictionary/dictionarySetup.ts';
import type { DictionarySetupInput, DictionarySetupPreview, DictionarySetupSaved } from '../src/lib/control/dictionary/dictionarySetup.ts';

const origin='http://127.0.0.1:57000';
const input=():DictionarySetupInput=>({expected_source:'C:/profiles/a/conf.toml',revision:'config-v1',key:'stream',label:'配信の読み方'});
const preview=():DictionarySetupPreview=>({source_path:input().expected_source,revision:'config-v1',dictionary_path:'C:/profiles/a/dictionaries/stream.tsv',existing_revision:null,existing_rows:null,creates_file:true,saved:false,runtime_connected:false,review_revision:'review-v1',storage_scope:'profile_directory'});
const saved=():DictionarySetupSaved=>({source_path:input().expected_source,dictionary_path:preview().dictionary_path,revision:'config-v2',saved:true,restart_required:true,runtime_connected:false,storage_scope:'profile_directory'});

test('late save receipt stays with its submitted VAC and profile without changing newer drafts',()=>{
    const original=input(),review=new DictionarySetupReview(origin,original,preview());
    review.createSaveRequest(origin,original,false);
    const edited={...original,label:'次の入力'};
    const result=review.resolveSaved(origin,edited,saved());
    assert.equal(result.refreshCurrentSource,true);
    assert.equal(result.submittedInputUnchanged,false);
    assert.equal(edited.label,'次の入力');
    assert.equal(edited.revision,'config-v1');
    for(const [currentOrigin,current] of [
        ['http://127.0.0.1:57001',original],
        [origin,{...original,expected_source:'C:/profiles/b/conf.toml'}],
        [origin,{...original,revision:'newer-config-v3'}],
    ] as const) {
        const resolved=review.resolveSaved(currentOrigin,current,saved());
        assert.equal(resolved.refreshCurrentSource,false);
        assert.equal(resolved.submittedInputUnchanged,false);
        assert.equal(resolved.origin,origin);
        assert.equal(resolved.receipt.source_path,original.expected_source);
    }
    const unchanged=review.resolveSaved(origin,original,saved());
    assert.equal(unchanged.submittedInputUnchanged,true);
    assert.equal(unchanged.refreshCurrentSource,true);
});

test('save receipt requires a submitted review and matching saved file',()=>{
    const original=input(),review=new DictionarySetupReview(origin,original,preview());
    assert.throws(()=>review.resolveSaved(origin,original,saved()));
    review.createSaveRequest(origin,original,false);
    for(const invalid of [
        {...saved(),source_path:'other'}, {...saved(),dictionary_path:'other.tsv'},
        {...saved(),revision:''}, {...saved(),storage_scope:'external_file' as const},
    ]) assert.throws(()=>review.resolveSaved(origin,original,invalid));
    const response=saved(),resolved=review.resolveSaved(origin,original,response);
    response.revision='mutated';
    assert.equal(resolved.receipt.revision,'config-v2');
    assert.equal(Object.isFrozen(resolved.receipt),true);
    assert.throws(()=>review.createSaveRequest(origin,original,false));
});

test('pending transport captures the old form without replacing newer input',async()=>{
    const current=input();
    let resolve!:(value:DictionarySetupPreview)=>void;
    const pending=new Promise<DictionarySetupPreview>(done=>resolve=done);
    const reviewing=prepareDictionarySetupReview(origin,current,async captured=>{
        assert.equal(captured.label,'配信の読み方');return pending;
    });
    current.label='次の入力';
    resolve(preview());
    const reviewed=await reviewing;
    assert.equal(reviewed.input.label,'配信の読み方');
    assert.equal(current.label,'次の入力');
    assert.equal(reviewed.matches(origin,current),false);
});

test('delayed review cannot authorize a different VAC, profile, revision or edited field',()=>{
    const original=input(),review=new DictionarySetupReview(origin,original,preview());
    assert.equal(review.matches('http://127.0.0.1:57001',original),false);
    for(const [field,value] of Object.entries({expected_source:'C:/profiles/b/conf.toml',revision:'v2',key:'other',label:'new label',existing_path:'C:/shared.tsv'})) {
        const current={...original,[field]:value};
        assert.equal(review.matches(origin,current),false);
        assert.throws(()=>review.createSaveRequest(origin,current,false));
        assert.equal(current[field],value);
    }
    assert.equal(review.matches(origin,original),true);
});

test('submission consumes review; retry requires fresh preview even when reply is lost',()=>{
    const original=input(),review=new DictionarySetupReview(origin,original,preview());
    const request=review.createSaveRequest(origin,original,false);
    assert.deepEqual(request.registration,original);
    assert.equal(request.review_revision,'review-v1');
    assert.throws(()=>review.createSaveRequest(origin,original,false));
    assert.equal(original.label,'配信の読み方');
    const retry=new DictionarySetupReview(origin,original,{...preview(),review_revision:'review-v2'});
    assert.equal(retry.createSaveRequest(origin,original,false).review_revision,'review-v2');
});

test('external dictionary needs explicit confirmation and preserves captured values',()=>{
    const original={...input(),existing_path:'C:/shared.tsv'},view={...preview(),storage_scope:'external_file' as const,creates_file:false,dictionary_path:'C:/shared.tsv',existing_revision:'file-v1',existing_rows:1};
    const review=new DictionarySetupReview(origin,original,view);
    assert.throws(()=>review.createSaveRequest(origin,original,false));
    assert.equal(review.matches(origin,original),true);
    view.review_revision='mutated';
    assert.equal(review.createSaveRequest(origin,original,true).review_revision,'review-v1');
    assert.equal(original.existing_path,'C:/shared.tsv');
});

test('a mismatched backend response does not produce a usable review',()=>{
    assert.throws(()=>new DictionarySetupReview(origin,input(),{...preview(),source_path:'other'}));
    assert.throws(()=>new DictionarySetupReview(origin,input(),{...preview(),revision:'other'}));
    assert.throws(()=>new DictionarySetupReview(origin,input(),{...preview(),review_revision:''}));
});

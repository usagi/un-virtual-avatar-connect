<script lang="ts">
 import { requestPurposeResponse } from "../api";
 import {sharePlanReasons,unverifiedLabels} from './sharePlanDetails';
 import type { FlowgraphSharePlan, ZipImportPreview, FlowgraphCompanionInventory } from '../types';
 import {api} from '../api';
 type PartialSave={written_files:(string|{path:string})[];written_companions:string[];backups:string[]};
 class PurposeRequestError extends Error {status:number;detail:string;partial:PartialSave|null;constructor(status:number,detail:string,partial:PartialSave|null=null){super(detail);this.status=status;this.detail=detail;this.partial=partial;}}
 type Purpose={id:string;name:string;roots:string[]};
 type Snapshot={root:string;revision:string|null;document:{schema_version:number;purposes:Purpose[]}};
 let {snapshot,blocked,onchanged,onpending}:{snapshot:Snapshot|null;blocked:boolean;onchanged:()=>Promise<void>;onpending:(value:boolean)=>void}=$props();
 let partialSave=$state<PartialSave|null>(null);
 const intentLabels:Record<string,string>={matches_intended:'予定した内容と一致',matches_previous:'取込前の内容と一致',not_created:'未作成',missing:'元のファイルが見つからない',changed:'内容が変更されている',unreadable:'照合できない'};
 type IntentObservation={sequence?:number;path?:string;observation:string;error?:string};
 type Attempt={id:string;graph_plan?:{files:IntentObservation[]};graph_plan_error?:string;write_intents?:IntentObservation[];inspection_error?:string;started?:{target_prefix:string;started_at_ms:number;purpose_id?:string};unfinished?:boolean;error?:string;result?:{outcome:{status:string;error?:string;progress?:PartialSave;report?:{registration:Purpose|null}}}};
 let history=$state<{root:string;entries:Attempt[]}|null>(null);
 type ResumePlan={stage?:string;root:string;attempt_id:string;expected:Snapshot;graphs:{path:string;action:string}[];materials:{path:string;action:string}[];purpose:{name:string;roots:string[];missing_roots:string[]};conflicts:string[];skipped_files:string[];unresolved_danglings:unknown[];execution_available:boolean;already_registered:boolean};
 let resumeDraft=$state<{id:string;root:string;name:string;archive:Uint8Array<ArrayBuffer>}|null>(null),resumePlan=$state<ResumePlan|null>(null),resumeError=$state('');
 const resumeChanged=$derived(!!resumeDraft&&(resumeDraft.root!==snapshot?.root||(!!resumePlan&&resumePlan.expected.revision!==snapshot?.revision)));
 async function chooseResume(event:Event,id:string){
  const input=event.target as HTMLInputElement,file=input.files?.[0];
  if(!file||!snapshot||busy||blocked)return;
  if(file.size>64*1024*1024){resumeError='ZIPは64 MiB以下を選択してください。';input.value='';return;}
  const root=snapshot.root;busy=true;resumeError='';resumePlan=null;
  try{resumeDraft={id,root,name:file.name,archive:new Uint8Array(await file.arrayBuffer())};}
  catch(e){resumeError=String(e);}finally{busy=false;input.value='';}
 }
 async function previewResume(){
  if(!snapshot||!resumeDraft||snapshot.root!==resumeDraft.root||busy||blocked)return;
  const expected=$state.snapshot(snapshot),draft=$state.snapshot(resumeDraft);busy=true;resumeError='';resumePlan=null;
  try{const plan=await request<ResumePlan>('resume-preview',{expected,id:draft.id,archive:draft.archive});
   if(plan.root!==expected.root||plan.attempt_id!==draft.id||snapshot?.root!==expected.root||snapshot?.revision!==expected.revision)throw Error('確認中に操作対象が変わりました。ZIPを保持しているので対象を確認し直してください。');
   resumePlan=plan;
  }catch(e){resumeError=String(e);}finally{busy=false;}
 }
 function resumedRegistration(entry:Attempt){return !!entry.started?.purpose_id&&!!snapshot?.document.purposes.some(purpose=>purpose.id===entry.started?.purpose_id);}
 async function executeResume(){
  if(!resumeDraft||!resumePlan||resumeChanged||!resumePlan.execution_available||busy||blocked)return;
  const draft=$state.snapshot(resumeDraft),plan=$state.snapshot(resumePlan);busy=true;resumeError='';status='';
  try{const result=await request<{graphs_completed?:boolean;requires_reinspection?:boolean;result?:{registration:Purpose;reload_attempted:boolean};already_registered?:boolean;journal_error?:string|null}>('resume-execute',{review:plan,archive:draft.archive});
   if(result.graphs_completed&&result.requires_reinspection){status='構成の保存を完了しました。ZIPを保持しています。素材と用途登録の内容をもう一度確認してください。';resumePlan=null;return;}
   if(!result.result?.registration&&!result.already_registered)throw Error('登録結果を確認できません。履歴を取得し直してください。');
   status=`保存先 ${draft.root} の取込を再開し、用途登録を確認しました。実行への適用は行っていません。`;
   if(result.journal_error)error=`登録は完了しましたが、再開結果の記録に失敗しました: ${result.journal_error}`;
   resumeDraft=null;resumePlan=null;
  }catch(e){resumeError=`${String(e)} 選択したZIPは保持しています。保存済み内容を確認し直してください。`;resumePlan=null;}
  finally{try{await onchanged();}catch(e){error=`一覧の再取得に失敗しました: ${String(e)}`;}busy=false;}
 }
 async function loadHistory(){
  if(!snapshot||busy||blocked)return;
  const root=snapshot.root;busy=true;error='';
  try{const value=await request<{root:string;entries:Attempt[]}>('transfer-history',{});
   if(value.root!==root||snapshot?.root!==root)throw Error('保存先が変わりました。対象を確認して履歴を取得し直してください。');
   history=value;
  }catch(e){error=String(e);}finally{busy=false;}
 }
 let purposeId=$state(''),busy=$state(false),error=$state(''),status=$state('');
 let exported=$state<{archive:number[]|Uint8Array<ArrayBuffer>;files:string[];plan:FlowgraphSharePlan;purpose:{name:string;roots:string[]}}|null>(null);
 let acknowledgeIncomplete=$state(false);
 let companions=$state<FlowgraphCompanionInventory|null>(null),selectedCompanions=$state<string[]>([]),companionQuery=$state('');
 let exportedRoot=$state('');
 const companionTargetChanged=$derived(!!companions&&companions.root_dir!==snapshot?.root);
 const visibleCompanions=$derived(companions?.files.filter(file=>file.path.toLowerCase().includes(companionQuery.toLowerCase()))??[]);
 const problems=$derived(exported ? sharePlanReasons(exported.plan).filter(r=>r.error) : []);
 let archive=$state<Uint8Array<ArrayBuffer>|null>(null),fileName=$state(''),prefix=$state('received');
 let review=$state<{preview:ZipImportPreview;expected:Snapshot;prefix:string}|null>(null),attempted=$state(false);
 const changed=$derived(!attempted&&!!review&&(review.expected.root!==snapshot?.root||review.expected.revision!==snapshot?.revision));
 const companionConflict=$derived(review?.preview.companions.some(file=>review?.preview.conflicts.includes(file.dest_path))??false);
 $effect(()=>onpending(busy||!!archive||!!exported||!!resumeDraft||selectedCompanions.length>0));
 async function loadCompanions(){
  if(!snapshot||blocked||busy||exported||archive)return;
  const root=snapshot.root;busy=true;error='';
  try{const value=await api.flowgraphCompanions();
   if(value.root_dir!==root||snapshot?.root!==root)throw Error('保存先が変わりました。対象を確認して素材一覧を取得し直してください。');
   companions=value;selectedCompanions=[];
  }catch(e){error=String(e);}finally{busy=false;}
 }
 async function request<T>(action:string,body:unknown):Promise<T>{
  if(body&&typeof body==='object'&&'archive' in body&&body.archive instanceof Uint8Array){
   const {archive:bytes,...metadata}=body;
   const encoded=new TextEncoder().encode(JSON.stringify(metadata));
   if(encoded.length>1024*1024)throw Error('確認情報が1 MiBを超えています。共有する構成を分けて確認してください。');
   const header=new Uint8Array(12);header.set(new TextEncoder().encode('UNVACZIP'));new DataView(header.buffer).setUint32(8,encoded.length,true);
   const payload=new Blob([header,encoded,new Uint8Array(bytes)],{type:'application/vnd.unvac.purpose-zip'});
   if(payload.size>64*1024*1024)throw Error('ZIPと確認情報の合計が64 MiBを超えています。同梱素材を減らして確認し直してください。');
   const r=await requestPurposeResponse(action,{method:'POST',body:payload});
   const v=await r.json();if(!r.ok)throw new PurposeRequestError(r.status,v.detail??v.error??`HTTP ${r.status}`,v.error==='partial_write'?{written_files:v.written_files??[],written_companions:v.written_companions??[],backups:v.backups??[]}:null);return v;
  }
  const encoded=JSON.stringify(body);
  if(new Blob([encoded]).size>64*1024*1024)throw Error('送信データが64 MiBを超えています。同梱する画像・音声などを減らして確認し直してください。');
  const r=await requestPurposeResponse(action,{method:'POST',headers:{'Content-Type':'application/json',...(action==='export'?{Accept:'application/vnd.unvac.purpose-zip'}:{})},body:encoded});
  if(r.ok&&r.headers.get('Content-Type')?.split(';')[0]==='application/vnd.unvac.purpose-zip'){
   const buffer=await r.arrayBuffer(),bytes=new Uint8Array(buffer);
   if(bytes.length<13||bytes.length>64*1024*1024||new TextDecoder().decode(bytes.subarray(0,8))!=='UNVACZIP')throw Error('書き出しデータの形式を確認できません。');
   const length=new DataView(buffer).getUint32(8,true);
   if(!length||length>1024*1024||12+length>=bytes.length)throw Error('書き出しデータが不完全です。');
   const metadata=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes.subarray(12,12+length)));
   if(!metadata||typeof metadata!=='object'||Array.isArray(metadata)||'archive' in metadata)throw Error('書き出しの確認情報が不正です。');
   return {...metadata,archive:bytes.slice(12+length)} as T;
  }
  const v=await r.json();if(!r.ok)throw new PurposeRequestError(r.status,v.detail??v.error??`HTTP ${r.status}`,v.error==='partial_write'?{written_files:v.written_files??[],written_companions:v.written_companions??[],backups:v.backups??[]}:null);return v;
 }
 async function prepareExport(){
  if(!snapshot||blocked||busy||!purposeId||companionTargetChanged)return;busy=true;error='';status='';
  const expected=$state.snapshot(snapshot);
  try{const value=await request<NonNullable<typeof exported>>('export',{expected,id:purposeId,companions:[...selectedCompanions]});if(!value.plan)throw Error('参照先の確認情報を取得できません。');
   if(snapshot?.root!==expected.root)throw Error('保存先が変わりました。書出し内容を確認し直してください。');
   exported=value;exportedRoot=expected.root;acknowledgeIncomplete=false;}catch(e){error=String(e);}finally{busy=false;}
 }
 function download(){
  if(!exported||exportedRoot!==snapshot?.root||(problems.length&&!acknowledgeIncomplete))return;
  const url=URL.createObjectURL(new Blob([new Uint8Array(exported.archive)],{type:'application/zip'}));
  const a=document.createElement('a');a.href=url;a.download='unvac-purpose.flowgraph.zip';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
  status='ZIPのダウンロードを開始しました。保存結果はブラウザーのダウンロード一覧で確認できます。';exported=null;
 }
 async function chooseFile(event:Event){
  const input=event.target as HTMLInputElement,file=input.files?.[0];if(!file||blocked||busy)return;
  error='';
  if(file.size>64*1024*1024){error='ZIPは64 MiB以下を選択してください。';input.value='';return;}
  busy=true;
  try{
   const bytes=new Uint8Array(await file.arrayBuffer());
   // Replace the existing input and its recovery context only after a complete read.
   archive=bytes;fileName=file.name;review=null;attempted=false;partialSave=null;status='';
  }catch(e){error=`${String(e)} 以前に選択したZIPと確認内容は保持しています。`;}
  finally{busy=false;input.value='';}
 }
 async function preview(){
  if(!snapshot||!archive||blocked||busy||attempted)return;busy=true;error='';status='';review=null;
  const expected=$state.snapshot(snapshot),target=prefix;
  try{const value=await request<ZipImportPreview>('preview',{expected,archive:$state.snapshot(archive),options:{target_prefix:target,paste:{on_conflict_file:'rename'}}});
   if(!value.purpose)throw Error('用途名のないZIPです。通常のFlowgraph取込を使ってください。');
   review={preview:value,expected,prefix:target};
  }catch(e){error=String(e);}finally{busy=false;}
 }
 async function importPurpose(){
  if(!review||!archive||blocked||busy||changed||attempted||companionConflict)return;busy=true;attempted=true;error='';status='';
  try{const result=await request<{registration:Purpose|null;registration_error:string|null;journal_error?:string|null;imported:{written_files:string[]}}>('import',{expected:$state.snapshot(review.expected),archive:$state.snapshot(archive),options:{target_prefix:review.prefix,paste:{on_conflict_file:'rename'}}});
   status=result.registration ? `「${result.registration.name}」を取り込み、用途として登録しました。実行への適用は行っていません。` : `${result.imported.written_files.length}ファイルを保存しましたが、用途登録は完了していません。`;
   if(result.registration_error)error=result.registration_error;
   if(result.journal_error)error+=` 取込結果の記録に失敗しました: ${result.journal_error}`;
  }catch(e){
   if(e instanceof PurposeRequestError&&e.partial){partialSave=e.partial;error=`${e.detail} 自動再取込は行いません。保存済みの構成とバックアップを確認してください。`;}
   else if(e instanceof PurposeRequestError&&e.status===409&&e.detail==='purpose revision changed'){
    attempted=false;review=null;error='用途一覧が確認後に更新されました。構成は保存していません。選択したZIPと取込先を保持しているので、内容を確認し直してください。';
   }else{error=`${String(e)} 保存された構成や復旧記録を確認してください。再取込は自動で行いません。`;}
  }
  finally{try{await onchanged();}catch(e){error+=` 一覧の再取得に失敗しました: ${String(e)}`;}busy=false;}
 }
 function clear(){partialSave=null;archive=null;fileName='';review=null;attempted=false;exported=null;selectedCompanions=[];companions=null;error='';}
</script>
<section aria-label="用途の共有と取込" class="my-4 border-t pt-4 text-sm">
 <h3 class="font-semibold">用途の共有・取込</h3>
 <details data-purpose-transfer-history class="my-3"><summary>過去の取込・途中で止まった取込</summary>
  <button disabled={busy||blocked||!snapshot} onclick={()=>void loadHistory()}>保存先の取込履歴を取得</button>
  {#if history&&history.root===snapshot?.root}
   <p class="break-all">保存先: {history.root}</p>
   {#if !history.entries.length}<p>保存された取込記録はありません。</p>{/if}
   {#each history.entries as entry}
    <section class="my-2 border p-2" aria-label="保存された取込記録">
     <p class="break-all">{entry.id} · {resumedRegistration(entry)?'用途登録済み（現在の保存先）':entry.unfinished?'終了結果未記録':entry.result?.outcome.status==='failed'?'取込失敗':entry.result?.outcome.report?.registration?'用途登録済み':'要確認'}</p>
     {#if entry.started}<p>{new Date(entry.started.started_at_ms).toLocaleString()} · 取込先: {entry.started.target_prefix||'直下'}</p>{/if}
     {#if entry.unfinished&&!resumedRegistration(entry)}<p>中断、または終了結果の保存失敗の可能性があります。保存済みファイルを確認してください。自動再取込は行いません。</p>{/if}
     {#if entry.error}<p role="alert">{entry.error}</p>{/if}
     {#if !entry.result?.outcome.report?.registration&&!resumedRegistration(entry)}
      {#if resumeDraft?.id===entry.id}<p class="my-2">選択済みの元のZIP: {resumeDraft.name}</p>
      {:else}<label class="my-2 block">再開内容を確認する元のZIP<input type="file" accept=".zip" disabled={busy||blocked||!!resumeDraft} onchange={event=>void chooseResume(event,entry.id)}/></label>{/if}
     {/if}
     {#if entry.inspection_error}<p role="alert">保存先の照合に失敗: {entry.inspection_error}</p>{/if}
     {#if entry.graph_plan_error}<p role="alert">構成全体の保存計画を確認できません: {entry.graph_plan_error}</p>{/if}
     {#if entry.graph_plan}
      <details class="my-2"><summary>構成全体の保存予定と現在の状態（{entry.graph_plan.files.length}件）</summary>
       <p>未保存の構成も含めた照合です。再開する場合は元のZIPと保存済み内容を確認してください。</p>
       <ul>{#each entry.graph_plan.files as file}<li class="break-all">{file.path} — {intentLabels[file.observation]??'要確認'}{#if file.error}<p role="alert">{file.error}</p>{/if}</li>{/each}</ul>
      </details>
     {/if}
     {#if entry.write_intents?.length}
      <details class="my-2"><summary>保存予定と現在のファイルを照合（{entry.write_intents.length}件）</summary>
       <p>履歴取得時点の内容との比較です。一致していても取込全体の完了を意味しません。</p>
       <ul>{#each entry.write_intents as intent}<li class="break-all">{intent.path??'記録の保存先を確認できません'} — {intentLabels[intent.observation]??'要確認'}{#if intent.error}<p role="alert">{intent.error}</p>{/if}</li>{/each}</ul>
      </details>
     {/if}
     {#if entry.result?.outcome.error||entry.result?.outcome.progress||entry.unfinished}
      <details class="my-2" open={!resumedRegistration(entry)}>
       <summary>最初の取込時の記録{resumedRegistration(entry)?'（現在は用途登録済み）':''}</summary>
       {#if resumedRegistration(entry)}<p>以下は当時の結果です。現在のファイルの状態は上の照合欄で確認できます。</p>{/if}
       {#if entry.unfinished}<p>最初の取込の終了結果は記録されていません。</p>{/if}
     {#if entry.result?.outcome.error}<p>{entry.result.outcome.error}</p>{/if}
     {#if entry.result?.outcome.progress}
      <h5>保存された構成・素材・バックアップ</h5>
      <ul>{#each entry.result.outcome.progress.written_files as file}<li class="break-all">構成: {typeof file==='string'?file:file.path}</li>{/each}
       {#each entry.result.outcome.progress.written_companions as path}<li class="break-all">素材: {path}</li>{/each}
       {#each entry.result.outcome.progress.backups as path}<li class="break-all">バックアップ: {path}</li>{/each}</ul>
     {/if}
      </details>
     {/if}
    </section>
   {/each}
  {:else if history}<p>表示対象が変わりました。履歴を取得し直してください。</p>{/if}
 </details>
 {#if resumeError&&!resumeDraft}<p role="alert">{resumeError}</p>{/if}
 {#if resumeDraft}
  <section aria-label="用途取込の再開内容" class="my-3 border p-3">
   <h4>中断した取込の再開内容</h4><p class="break-all">保存先: {resumeDraft.root}</p><p>記録: {resumeDraft.id} · ZIP: {resumeDraft.name}</p>
   {#if resumeChanged}<p role="alert">操作対象または用途一覧が変わりました。選択したZIPは保持しています。</p>{/if}
   <button disabled={busy||blocked||snapshot?.root!==resumeDraft.root} onclick={()=>void previewResume()}>元のZIPと保存済み内容を確認</button>
   {#if resumePlan&&!resumeChanged}
    <p>{resumePlan.already_registered?'登録済み':'登録予定'}: {resumePlan.purpose.name}</p><p>{resumePlan.stage==='graphs_incomplete'?'構成の保存途中で中断しています。現在の状態を確認します。':'構成は再取込せず保持します。'}</p>
    <ul>{#each resumePlan.graphs as file}<li class="break-all">{file.action==='keep'?'保持':file.action==='create'?'未保存':file.action==='replace'?'元データを退避済み・更新予定':'競合あり'}: {file.path}</li>{/each}</ul>
    <ul>{#each resumePlan.materials as file}<li class="break-all">{file.action==='keep'?'保持':file.action==='create'?'追加予定':'競合あり'}: {file.path}</li>{/each}</ul>
    <p>用途の起点: {resumePlan.purpose.roots.join('、')}</p>
    {#if resumePlan.conflicts.length}<p role="alert">保存先に競合があります。既存の内容は変更していません。</p>{/if}
    {#if resumePlan.purpose.missing_roots.length}<p role="alert">取り込まれていない起点: {resumePlan.purpose.missing_roots.join('、')}</p>{/if}
    <p>取込を省いた構成: {resumePlan.skipped_files.length}件 · 未解決の参照: {resumePlan.unresolved_danglings.length}件</p>
    <p>この段階は内容の確認のみです。追加保存・用途登録・実行への適用は行っていません。</p>
    {#if !resumePlan.already_registered}<button disabled={busy||blocked||resumeChanged||!resumePlan.execution_available} onclick={()=>void executeResume()}>{resumePlan.stage==='graphs_incomplete'?'確認した構成の保存を再開':'確認した残りの素材を保存して用途を登録'}</button>{/if}
    {#if resumePlan.stage==='graphs_incomplete'}<p>{resumePlan.execution_available?'追加・更新予定の構成を保存し、保存済みの構成は保持します。更新前のデータは退避済みです。その後、素材と用途登録を改めて確認します。':'この構成の再開保存は利用できません。現在は内容の確認のみ利用できます。'}</p>{:else if !resumePlan.execution_available&&!resumePlan.already_registered&&!resumePlan.conflicts.length&&!resumePlan.purpose.missing_roots.length}<p>この記録では再開に必要な情報が足りません。内容の確認のみ利用できます。</p>{/if}
   {/if}
   {#if resumeError}<p role="alert">{resumeError}</p>{/if}
   <button disabled={busy} onclick={()=>{resumeDraft=null;resumePlan=null;resumeError='';}}>再開内容の確認を閉じる</button>
  </section>
 {/if}
 <fieldset disabled={blocked||busy}>
  <label class="my-2 block">書き出す用途<select aria-label="書き出す用途" class="ml-2 rounded border p-2" bind:value={purposeId} disabled={!!exported||!!archive}><option value="">選択してください</option>{#each snapshot?.document.purposes??[] as purpose}<option value={purpose.id}>{purpose.name} · {purpose.id}</option>{/each}</select></label>
  <details class="my-3"><summary>説明書・画像・音声などを同梱</summary>
   <p>必要なファイルだけを選択します。未選択なら構成だけを書き出します。</p>
   <button disabled={!!exported||!!archive} onclick={()=>void loadCompanions()}>同梱候補を取得</button>
   {#if companionTargetChanged}<p role="alert">保存先が変わっています。同梱候補を取得し直してください。</p>{/if}
   {#if companions&&!companionTargetChanged}
    {#if companions.truncated}<p>候補一覧は一部です。必要な素材が揃っているか確認してください。</p>{/if}
    <label class="my-2 block">同梱候補を検索<input class="mt-1 block w-full min-w-0 rounded border p-2" type="search" placeholder="ファイル名やフォルダー名" bind:value={companionQuery}/></label>
    <div class="max-h-64 overflow-auto">{#each visibleCompanions as file (file.path)}<label class="my-2 block break-all"><input type="checkbox" bind:group={selectedCompanions} value={file.path} disabled={!!exported||!!archive}/> {file.path} · {file.bytes.toLocaleString()} B</label>{/each}</div>
    <p>{selectedCompanions.length}件選択</p>
   {/if}
  </details>
  <button disabled={!purposeId||!!archive||companionTargetChanged} onclick={()=>void prepareExport()}>ZIPに含める構成を確認</button>
  {#if exported}<div class="my-3 border p-3"><strong>{exported.purpose.name}</strong><p>参照先を含む構成ファイルです。選択した説明書や素材も同梱します。</p><ul>{#each exported.files as fq}<li class="break-all">{fq}{exported.purpose.roots.includes(fq)?'（選択した構成）':'（参照先）'}</li>{/each}</ul>{#if problems.length}<div role="alert" class="my-2"><strong>見つからない参照先などが{problems.length}件あります</strong><ul>{#each problems as problem}<li class="break-all">{problem.from} · {problem.field}: {problem.raw} — {problem.error}</li>{/each}</ul></div><label class="my-2 block"><input type="checkbox" bind:checked={acknowledgeIncomplete}/> 未解決の参照を含むことを確認した</label>{/if}
   <p>同梱ファイル: {selectedCompanions.length}件</p><ul>{#each selectedCompanions as path}<li class="break-all">{path}</li>{/each}</ul>
   <details class="my-2"><summary>この確認では保証できない項目</summary><ul>{#each exported.plan.unverified as item}<li>{unverifiedLabels[item]??item}</li>{/each}</ul></details>
   <button class="mt-2" disabled={exportedRoot!==snapshot?.root||(problems.length>0&&!acknowledgeIncomplete)} onclick={download}>確認したZIPを保存</button></div>{/if}
  <label class="my-3 block">用途ZIPを選択<input class="ml-2" type="file" accept=".zip" onchange={(e)=>void chooseFile(e)} disabled={!!exported||!!archive}/></label>
  {#if archive}<p>{fileName}</p><label class="my-2 block">取込先フォルダー<input class="ml-2 rounded border p-2" bind:value={prefix} disabled={attempted} oninput={()=>review=null}/></label>
   <button disabled={attempted} onclick={()=>void preview()}>用途と取込先を確認</button>
   {#if review}<div class="my-3 border p-3"><strong>{review.preview.purpose?.name}</strong><p class="break-all">保存先: {review.expected.root} / {review.prefix}</p><p>同名ファイルは別名で取り込み、既存ファイルを保持します。</p><h4 class="mt-2 font-semibold">構成ファイル（{review.preview.entries.length}件）</h4><ul>{#each review.preview.entries as entry}<li class="break-all">{entry.dest_path}</li>{/each}</ul><h4 class="mt-2 font-semibold">同梱素材（{review.preview.companions.length}件）</h4><ul>{#each review.preview.companions as entry}<li class="break-all">{entry.dest_path}</li>{/each}</ul>{#if review.preview.conflicts.length}<p>同名の保存先: {review.preview.conflicts.length}件。構成ファイルは保存時に別名を割り当てます。</p>{/if}
    {#if changed}<p role="alert">用途一覧が変わりました。取込内容を確認し直してください。</p>{/if}
    {#if companionConflict}<p role="alert">同名の素材が保存先にあります。既存素材を保持するため、別の取込先フォルダーを指定してください。</p>{/if}
    <button disabled={changed||attempted||companionConflict} onclick={()=>void importPurpose()}>この保存先へ取り込んで用途を登録</button>
   </div>{/if}
  {/if}
  {#if archive||exported||selectedCompanions.length}<button class="ml-3 underline" onclick={clear}>{attempted?'取込結果の確認を閉じる':'共有・取込の確認を取り消す'}</button>{/if}
 </fieldset>
 {#if partialSave}<section aria-label="用途取込の保存済み内容" class="my-3 border p-3"><h4>途中まで保存された内容</h4><p>用途登録と実行への適用は完了していません。再取込を繰り返さず、保存先とファイルの復元履歴を確認してください。</p><h5>構成ファイル</h5><ul>{#each partialSave.written_files as file}<li class="break-all">{typeof file==='string'?file:file.path}</li>{/each}</ul><h5>素材</h5><ul>{#each partialSave.written_companions as path}<li class="break-all">{path}</li>{/each}</ul><h5>バックアップ</h5><ul>{#each partialSave.backups as path}<li class="break-all">{path}</li>{/each}</ul>{#if !partialSave.backups.length}<p>上書き前のバックアップはありません。</p>{/if}</section>{/if}
 {#if error}<p role="alert" class="my-2 text-warning-500">{error}</p>{/if}
 {#if status}<p role="status" class="my-2">{status}</p>{/if}
</section>

<script lang="ts">
 import { api } from '../api';
 import type { FlowgraphSharePlan } from '../types';
 import { flowgraphStore } from '../flowgraphStore.svelte';
 import FlowgraphExportPreview from './FlowgraphExportPreview.svelte';
 import { sharedUsage } from './sharedUsage';
 import {sharePlanReasons,unverifiedLabels as unfinished} from './sharePlanDetails';
 let { fq, files, onclose }: { fq: string; files?: string[]; onclose: () => void } = $props();
 const selectedFiles = $derived(files?.length ? files : [fq]);
 let plan = $state<FlowgraphSharePlan | null>(null);
 let target = $state('');
 let error = $state('');
 let loading = $state(false);
 let revision = $state(0);
 let exporting = $state(false);
 let usage = $state<ReturnType<typeof sharedUsage> | null>(null);
 let usageError = $state('');
 let usageBusy = $state(false);
 async function inspectUsage(){
  const selected=plan, identity=target;
  if(!selected?.source_digests || usageBusy)return;
  usageBusy=true;usageError='';usage=null;
  try{
   const all=await api.flowgraphSharePlan(Object.keys(selected.source_digests));
   const profile=await api.desktopProfiles();
   if(plan!==selected)return;
   if(profile.current_conf_path!==identity)throw Error('操作対象が変わりました。共有候補を再取得してください。');
   usage=sharedUsage(selected,all);
  }catch(e){if(plan===selected)usageError=String(e);}
  finally{if(plan===selected)usageBusy=false;}
 }
 const reasons = $derived(plan ? sharePlanReasons(plan) : []);
 const problems = $derived(reasons.filter(r=>r.error));
 $effect(()=>{
  const requested = [...selectedFiles];
  revision;
  let cancelled=false;
  plan=null; error=''; target=''; loading=true; exporting=false; usage=null;usageError='';usageBusy=false;
  void (async()=>{
   try {
    const before=await api.desktopProfiles();
    if(cancelled) return;
    const identity=before.current_conf_path;
    if(!identity) throw Error('接続中のVACを確認できません。');
    const result=await api.flowgraphSharePlan(requested);
    if(cancelled) return;
    const after=await api.desktopProfiles();
    if(cancelled) return;
    if(after.current_conf_path!==identity) throw Error('操作対象が変わりました。対象を確認して再取得してください。');
    target=identity;
    plan=result;
   } catch(e) {if(!cancelled) error=String(e);}
   finally {if(!cancelled) loading=false;}
  })();
  return ()=>{cancelled=true;};
 });
</script>

<section class="share-preview" aria-label="機能一式の共有を検討">
 <header><div><p class="eyebrow">共有する内容の確認</p><h3>この機能に必要なファイル</h3></div><button type="button" onclick={onclose}>閉じる</button></header>
 <p class="file">{selectedFiles.join(' / ')}</p>
 <p>保存済みの構成から参照先を調べます。編集中の変更は含みません。</p>
 {#if loading}<p role="status">参照先を確認中…</p>{/if}
 {#if error}<p role="alert">{error}</p>{/if}
 {#if plan}
  <p class="file">確認対象: {target}</p>
  <h4>共有に含める候補 · {plan.files.length}ファイル</h4>
  <ul>{#each plan.files as file}<li><span class="file">{file}</span>{selectedFiles.includes(file) ? ' · 選んだファイル' : ' · 参照先'}
   {#if !selectedFiles.includes(file)}<details><summary>必要になる理由</summary><ul>{#each reasons.filter(r=>r.target===file && r.from!==file) as reason}<li><span class="file">{reason.from}</span> の {reason.field}</li>{/each}</ul></details>{/if}
  </li>{/each}</ul>
  {#if problems.length}<h4>解決できなかった参照 · {problems.length}件</h4><ul>{#each problems as problem}<li><span class="file">{problem.from}</span> · {problem.field}<br>{problem.raw}<br>{problem.error}</li>{/each}</ul>{/if}
  <details><summary>まだ確認できていない項目</summary><ul>{#each plan.unverified as item}<li>{unfinished[item] ?? item}</li>{/each}</ul></details>
  <section aria-label="共有候補を利用する別の構成">
   <h4>ほかの構成でも使われている部品</h4>
   <p>共有に含める候補が、この用途だけの所有物とは限りません。</p>
   <button type="button" disabled={usageBusy || !plan.source_digests} onclick={()=>void inspectUsage()}>{usageBusy ? '参照元を確認中…' : '他の構成からの利用を確認'}</button>
   {#if usageError}<p role="alert">{usageError}</p>{/if}
   {#if usage}
    {#if usage.incoming.length}<ul>{#each usage.incoming as item}<li><span class="file">{item.target}</span> ← <span class="file">{item.from}</span></li>{/each}</ul>
    {:else}<p role="status">候補の外からの、解決済みの静的参照は見つかりませんでした。</p>{/if}
    {#if usage.unresolved}<p role="status">構成全体に未解決の参照が{usage.unresolved}件あります。利用元のない部品とは判断できません。</p>{/if}
    <p class="muted">動的な参照や外部アプリの利用は含みません。この確認で削除対象や所有範囲を自動決定することはありません。</p>
   {/if}
  </section>
  <p>候補のグラフをまとめてZIPにし、同梱する画像・音声などを次の画面で選べます。保存済みの内容が対象です。</p>
  {#if !plan.root_dir || !plan.source_digests}<p role="status">この接続先は確認内容を保持した一式書出に未対応です。</p>{/if}
  {#if plan.files.some(file=>flowgraphStore.hasDraftForFile(file, plan?.root_dir))}<p role="status">候補に未保存の編集があります。Studioで保存してから共有してください。</p>{/if}
  <button type="button" disabled={loading || exporting || problems.length>0 || !plan.root_dir || !plan.source_digests || plan.files.some(file=>flowgraphStore.hasDraftForFile(file, plan?.root_dir))}
   onclick={()=>exporting=true}>候補のファイルと素材を書き出す</button>
 {/if}
 <button type="button" disabled={loading} onclick={()=>revision++}>保存済みの内容を再確認</button>
</section>

{#if exporting && plan?.root_dir && plan.source_digests}
 <FlowgraphExportPreview root={plan.root_dir} filename={`${fq.replaceAll('/','-')}-feature.zip`}
  assetCandidates={plan.asset_candidates ?? []}
  request={{scope:'mixed',targets:plan.files.map(file=>({kind:'file' as const,fq:file})),expected_graph_sources:plan.source_digests}}
  onclose={()=>exporting=false} />
{/if}

<style>
 .share-preview{margin-top:1rem;padding:1rem;border:1px solid var(--color-surface-300-700);border-radius:var(--vac-panel-radius,10px);background:var(--color-surface-50-950);font-size:.85rem;line-height:1.6}
 header{display:flex;justify-content:space-between;gap:1rem;align-items:start}h3{font-size:1.1rem;font-weight:650}h4{font-weight:650;margin-top:1rem}.eyebrow{font-size:.7rem;opacity:.7}.file{overflow-wrap:anywhere}p{margin:.5rem 0}ul{list-style:disc;padding-left:1.25rem}li{margin:.5rem 0}summary{cursor:pointer}button{border:1px solid var(--color-surface-300-700);border-radius:.4rem;padding:.35rem .65rem}button:disabled{opacity:.5}
</style>

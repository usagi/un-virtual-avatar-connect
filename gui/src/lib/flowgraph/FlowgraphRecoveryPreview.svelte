<script lang="ts">
 import { api } from '../api';
 import { flowgraphStore } from '../flowgraphStore.svelte';
 import { tabNavStore } from '../tabs.svelte';
 import type { FlowgraphRecoveryPlan } from '../types';
 let { drawer = false }: { drawer?: boolean } = $props();
 let open = $state(false);
 $effect(() => {
  if (tabNavStore.featureFocus?.id === 'flowgraph-recovery') open = true;
 });
 let history = $state<Awaited<ReturnType<typeof api.flowgraphRecoveryHistory>> | null>(null);
 let plan = $state<FlowgraphRecoveryPlan | null>(null);
 let busy = $state(false);
 let error = $state('');
 let result = $state('');
 let generation = 0;
 let historyRoot: string | null = null;
 $effect(() => {
  const root=flowgraphStore.tree?.root_dir ?? null;
  if(root===historyRoot) return;
  historyRoot=root; generation++;
  history=null; plan=null; result=''; error=''; busy=false;
 });
 const blocked = $derived(busy || flowgraphStore.mutating || flowgraphStore.isDirty || !!(plan && flowgraphStore.hasDraftForFile(plan.fq, plan.root_dir)));
 async function refresh() {
  if(busy) return;
  const current = ++generation;
  const requestedRoot=flowgraphStore.tree?.root_dir;
  busy=true; error=''; plan=null; result='';
  try {
   const value=await api.flowgraphRecoveryHistory();
   if(current!==generation) return;
   if(!requestedRoot||flowgraphStore.tree?.root_dir!==requestedRoot||value.root_dir!==requestedRoot) throw Error('保存先が変わりました。履歴を更新してください。');
   history=value;
  } catch(e) {if(current===generation) {history=null;error=String(e);}}
  finally {if(current===generation) busy=false;}
 }
 async function inspect(fq:string, backup:string) {
  if(busy) return;
  const current=++generation;
  busy=true;error='';result='';plan=null;
  try {
   const value=await api.flowgraphRecoveryPlan(fq,backup);
   if(current!==generation) return;
   if(value.root_dir!==history?.root_dir) throw Error('保存先が変わりました。履歴を更新してください。');
   plan=value;
  } catch(e) {if(current===generation) error=String(e);}
  finally {if(current===generation) busy=false;}
 }
 async function restore() {
  if(!plan || blocked) return;
  const current=++generation;
  const restoredRoot=plan.root_dir;
  busy=true;error='';result='';
  try {
   const response=await flowgraphStore.restoreFile(plan);
   if(current!==generation) return;
   if(!response) {error='復元を完了できませんでした。競合・未保存編集・通知を確認してください。';return;}
   result=(response.ok ? (response.restart_required ? 'ファイルは復元済み。' : '復元・適用済み。') : 'ファイルは復元済み。適用時の診断を確認してください。') + (response.restart_required ? 'Web入力の反映にはこのVACの再起動が必要です。' : '');
   plan=null;
   // The old history still says "deleted" and omits the backup created by
   // restoring over an existing file. Fetch it again without losing success.
   history=null;
   try {
    const value=await api.flowgraphRecoveryHistory();
    if(current!==generation) return;
    if(value.root_dir!==restoredRoot) throw Error('保存先が変わりました。履歴を更新してください。');
    history=value;
   } catch(e) {if(current===generation) error=`復元後の履歴を更新できませんでした: ${String(e)}`;}
  } finally {if(current===generation) busy=false;}
 }
</script>

<div class="recovery-entry">
 <button type="button" aria-expanded={open} onclick={()=>open=!open}>履歴から戻す <span aria-hidden="true">↶</span></button>
</div>
<section class:drawer hidden={!open} aria-label="グラフの復元" class="recovery-panel vac-panel">
 <header><div>{#if import.meta.env.VITE_UN_DESIGN_PREVIEW === "1"}<small>確認用</small>{/if}<h3>履歴から戻す</h3></div><button type="button" onclick={()=>open=false} aria-label="履歴を閉じる">×</button></header>
 <p>削除したグラフや変更前の版を探し、内容を確認して戻せます。</p>
 <button type="button" disabled={busy} onclick={refresh}>{history ? '履歴を更新' : 'この保存先の履歴を表示'}</button>
 {#if history}
  <p class="target"><strong>保存先</strong><br />{history.root_dir}</p>
  <ul>{#each history.entries as entry (entry.fq)}
   <li><strong>{entry.fq}</strong><span class="state">{entry.current_present ? '現在のファイルあり' : '削除済み'}</span>
    {#each entry.backups as backup}<button class="version" type="button" disabled={busy} onclick={()=>inspect(entry.fq,backup)}>{backup}</button>{/each}
   </li>
  {:else}<li>この保存先にはバックアップがありません。</li>{/each}</ul>
 {/if}
 {#if error}<p role="alert" class="error">{error}</p>{/if}
 {#if result}<p role="status">{result}</p>{/if}
 {#if plan}
  <div class="comparison">
   <h4>{plan.fq} を復元</h4>
   <p class="target">復元元: {plan.backup}</p>
   <details open><summary>現在 {plan.current.state==='missing' ? '· 削除済み' : ''}</summary><pre>{plan.current.state==='present' ? plan.current.content : 'ファイルはありません'}</pre></details>
   <details open><summary>復元する内容</summary><pre>{plan.backup_content}</pre></details>
   <p>現在の内容がある場合は別のバックアップに残します。復元後はFlowgraph全体を再読み込みします。</p>
   {#if flowgraphStore.isDirty || flowgraphStore.hasDraftForFile(plan.fq, plan.root_dir)}<p role="status">未保存の編集があります。保存または退避してから復元してください。</p>{/if}
   <button type="button" class="restore" disabled={blocked} onclick={restore}>この内容へ復元</button>
  </div>
 {/if}
</section>

<style>
 .recovery-entry {display:flex;justify-content:flex-end;margin-bottom:.5rem}
 .recovery-panel {padding:1rem;border-top:2px solid var(--color-primary-500);max-height:78vh;overflow:auto;border-radius:.8rem;}
 .recovery-panel[hidden] {display:none}
 .drawer {position:fixed;right:1rem;bottom:1rem;width:min(38rem,calc(100vw - 2rem));z-index:1000;box-shadow:0 1rem 4rem #0008;background:var(--color-surface-900,#151b26)}
 header {display:flex;justify-content:space-between;align-items:start;gap:1rem}
 h3 {font-size:1.15rem;font-weight:700} h4 {font-weight:650;overflow-wrap:anywhere}
 small,.state {font-size:.75rem;opacity:.75} .state {display:block}
 p {font-size:.82rem;margin:.65rem 0;line-height:1.6}
 .target,.version {overflow-wrap:anywhere} .target {opacity:.8}
 button {padding:.45rem .7rem;border:1px solid var(--color-surface-500);border-radius:.4rem;text-align:left;font-size:.8rem}
 button:hover {border-color:var(--color-primary-500)} button:disabled {opacity:.45}
 ul {display:grid;gap:.7rem;margin-top:.8rem} li {border-bottom:1px solid var(--color-surface-500);padding-bottom:.7rem}
 .version {display:block;width:100%;margin-top:.4rem;font-size:.7rem}
 .comparison {margin-top:1rem;border-top:1px solid var(--color-primary-500);padding-top:1rem}
 details {margin:.6rem 0} summary {font-size:.8rem;cursor:pointer}
 pre {font-size:.72rem;overflow:auto;max-height:14rem;padding:.6rem;background:#0003;white-space:pre}
 .restore {border-color:var(--color-primary-500);font-weight:650}.error {color:var(--color-error-400,#f7a5aa)}
</style>

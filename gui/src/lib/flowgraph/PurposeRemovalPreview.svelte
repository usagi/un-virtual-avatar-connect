<script lang="ts">
 import { requestPurposeResponse } from "../api";
 import {onMount} from 'svelte';
 type Purpose={id:string;name:string;roots:string[]};
 type Snapshot={root:string;revision:string|null;document:{schema_version:number;purposes:Purpose[]}};
 type Plan={root:string;requested_files:string[];retained_files:string[];other_purpose_uses:{id:string;name:string;files:string[];error:string|null}[];graph_validation_ok:boolean;diagnostics:{message:string;severity:string}[];purpose_reference_errors:string[];delete_ready:false};
 let {purpose,snapshot,currentRoot,onclose,onchanged}:{purpose:Purpose;snapshot:Snapshot;currentRoot:string|undefined;onclose:()=>void;onchanged:()=>Promise<void>}=$props();
 const storageReview=import.meta.env.VITE_UN_DESIGN_PREVIEW!=='1'||new URLSearchParams(location.search).get('purposeRemoval')==='storage';
 let acknowledged=$state(false),attempted=$state(false),result=$state('');

 async function removeFiles(){
  if(!storageReview||!plan||!canRemove||!acknowledged||attempted||busy)return;
  if(currentRoot!==snapshot.root||plan.root!==snapshot.root){error='保存先が変わりました。撤去確認を閉じ、対象を選び直してください。';return;}
  const requestedRoot=snapshot.root;
  busy=true;attempted=true;error='';
  try{const response=await requestPurposeResponse('remove-files',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({expected:$state.snapshot(snapshot),id:purpose.id,files:[...selected],reviewed:$state.snapshot(plan)})});
   const value=await response.json();if(!response.ok)throw Error(value.detail??value.error);
   if(currentRoot!==requestedRoot)throw Error('保存先が変わりました。元の保存先で撤去結果を確認してください。');
   result=`削除済み: ${value.removed_files.join(' / ')}。復元記録: ${value.recovery.id}。実行状態は変更していません。`;
   if(value.error)error=value.error;else result+='用途登録も外しました。';
  }catch(e){error=`${String(e)}。再実行せず、閉じて撤去・復元履歴を確認してください。`;}
  finally{try{if(currentRoot===requestedRoot)await onchanged();}finally{busy=false;}}
 }
 let selected=$state<string[]>([]),candidates=$state<string[]>([]),plan=$state<Plan|null>(null),busy=$state(false),error=$state('');
 const canRemove=$derived(!!plan&&plan.graph_validation_ok&&!plan.other_purpose_uses.length&&!plan.purpose_reference_errors?.length);
 async function refresh(){
  if(busy||attempted||!selected.length)return;busy=true;error='';plan=null;acknowledged=false;
  const requestedRoot=snapshot.root;
  try{const response=await requestPurposeResponse('removal-plan',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({expected:$state.snapshot(snapshot),id:purpose.id,files:[...selected]})});
   const value=await response.json();if(!response.ok)throw Error(value.detail??value.error);
   if(currentRoot!==requestedRoot||snapshot.root!==requestedRoot||value.root!==requestedRoot)throw Error('確認対象の保存先が変わりました。');
   plan=value;candidates=[...new Set([...value.requested_files,...value.retained_files])].sort();
  }catch(e){error=String(e);}finally{busy=false;}
 }
 onMount(()=>{selected=[...purpose.roots];void refresh();});
 function toggle(fq:string,checked:boolean){selected=checked?[...selected,fq]:selected.filter(item=>item!==fq);plan=null;acknowledged=false;}
</script>
<section aria-label="用途の構成撤去の影響確認" class="my-4 rounded border p-4 text-sm">
 <h3 class="font-semibold">{purpose.name}の構成を外した場合</h3><p class="text-xs break-all">{purpose.id} · {snapshot.root}</p>
 <p class="my-2">{storageReview?'選択したファイルと用途登録を表示中の保存先から外し、復元記録を保存します。実行状態は変更しません。':'この画面は影響の確認のみ。ファイルの削除や実行状態の変更は行いません。'}</p>
 <fieldset disabled={busy||attempted}><legend>確認対象の構成</legend>
  {#each candidates as fq}<label class="my-1 block break-all"><input type="checkbox" aria-label={`撤去確認の対象: ${fq}`} checked={selected.includes(fq)} onchange={event=>toggle(fq,event.currentTarget.checked)}/> {fq}{purpose.roots.includes(fq)?'（用途に登録した構成）':'（参照先の部品）'}</label>{/each}
 </fieldset>
 <button disabled={busy||attempted||!selected.length} onclick={()=>void refresh()}>選んだ構成を外した影響を確認</button><button class="ml-3 underline" disabled={busy} onclick={onclose}>影響確認を閉じる</button>
 {#if result}<p role="status">{result}</p>{/if}
 {#if busy}<p role="status">残る構成を確認中…</p>{/if}
 {#if error}<p role="alert" class="my-2 text-warning-500">{error}</p>{/if}
 {#if plan}
  {#if plan.purpose_reference_errors?.length}<div role="alert"><p>参照関係の確認・解消が必要なため、この範囲では撤去できません。</p><ul>{#each plan.purpose_reference_errors as reason}<li>{reason}</li>{/each}</ul></div>{/if}
  <p class="my-2" role="status">残るグラフの検証: {plan.graph_validation_ok?'構成上のエラーなし':'構成上のエラーあり'}</p>
  <h4 class="mt-3 font-semibold">残す構成</h4><ul>{#each plan.retained_files as fq}<li class="break-all">{fq}</li>{/each}</ul>{#if !plan.retained_files.length}<p>この用途の参照範囲ではありません。</p>{/if}
  {#if plan.other_purpose_uses.length}<section aria-label="他の用途への影響" class="my-3"><h4 class="font-semibold">他の用途でも使われています</h4>{#each plan.other_purpose_uses as usage}<p>{usage.name} · {usage.id}</p><ul>{#each usage.files as fq}<li class="break-all">{fq}</li>{/each}</ul>{#if usage.error}<p role="alert">参照範囲を確認できません: {usage.error}</p>{/if}{/each}</section>{/if}
  {#if plan.diagnostics.length}<details class="my-2" open={!plan.graph_validation_ok}><summary>構成の診断 ({plan.diagnostics.length}件)</summary><ul>{#each plan.diagnostics as diagnostic}<li class="break-all">{diagnostic.message}</li>{/each}</ul></details>{/if}
  <p class="my-2 text-xs">動的な参照と実行中の影響は、この確認では判定できません。保存内容を実行に反映するには、撤去後にStudioで再読込してください。</p>
  {#if storageReview}<fieldset disabled={busy||attempted||!canRemove} class="mt-3"><label><input type="checkbox" bind:checked={acknowledged}/> 選んだファイルと用途登録を表示中の保存先から外す</label><button class="ml-3 underline" disabled={!acknowledged} onclick={()=>void removeFiles()}>復元記録を残して撤去</button></fieldset>{/if}
 {/if}
</section>


<script lang="ts">
 import { requestPurposeResponse } from "../api";
 import {onDestroy} from 'svelte';
 type Entry={id:string;recovery:{id:string;revision:string}|null;purpose:{id:string;name:string}|null;files:string[];error:string|null};
 let {root,blocked,onchanged,onpending}:{root:string;blocked:boolean;onchanged:()=>Promise<void>;onpending:(busy:boolean)=>void}=$props();
 let entries=$state<Entry[]|null>(null),selected=$state<Entry|null>(null),busy=$state(false),error=$state(''),status=$state('');
 let entriesRoot=$state<string|null>(null);
 let disposed=false;
 $effect(()=>onpending(busy||selected!==null));
 onDestroy(()=>{disposed=true;onpending(false);});
 async function request(action:string,body:unknown){const r=await requestPurposeResponse(action,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});const v=await r.json();if(!r.ok)throw Error(v.detail??v.error);return v;}
 async function load(){
  if(blocked||busy)return;
  const requestedRoot=root;
  busy=true;error='';status='';selected=null;entries=null;entriesRoot=null;
  try{
   const v=await request('removal-history',{});
   if(disposed)return;
   if(root!==requestedRoot||v.root!==requestedRoot)throw Error('保存先が変わりました。履歴を再取得してください。');
   entries=v.entries;entriesRoot=requestedRoot;
  }catch(e){if(!disposed)error=String(e);}finally{if(!disposed)busy=false;}
 }
 async function restore(){
  if(blocked||busy||!selected?.recovery)return;
  if(entriesRoot!==root){error='保存先が変わりました。履歴を再取得してください。';return;}
  const requestedRoot=entriesRoot;
  const recovery=$state.snapshot(selected.recovery);
  busy=true;error='';status='';
  try{
   const v=await request('restore-removal',{expected_root:requestedRoot,recovery});
   if(disposed)return;
   if(root!==requestedRoot||v.root!==requestedRoot)throw Error('保存先が変わりました。元の保存先で復元結果を確認してください。');
   status='構成ファイルと用途登録を復元しました。';
  }catch(e){if(!disposed)error=String(e);}
  finally{
   if(!disposed){
    selected=null;entries=null;entriesRoot=null;
    try{if(root===requestedRoot)await onchanged();}finally{busy=false;}
   }
  }
 }

</script>
<section aria-label="用途の撤去・復元履歴" class="my-4 rounded border p-4 text-sm">
 <h3 class="font-semibold">撤去した用途を戻す</h3><p class="break-all text-xs">{root}</p>
 <button disabled={blocked||busy} onclick={()=>void load()}>撤去・復元履歴を確認</button>
 {#if error}<p role="alert">{error}</p>{/if}{#if status}<p role="status">{status}</p>{/if}
 {#if entries?.length===0}<p>撤去記録はありません。</p>{/if}
 {#each entries??[] as entry}<div class="my-3"><strong>{entry.purpose?.name??entry.id}</strong><p class="text-xs">{entry.id}</p><p>{entry.files.join(' / ')}</p>
  {#if entry.error}<p role="alert">{entry.error}</p>{:else}<button disabled={blocked||busy} onclick={()=>selected=entry}>復元する内容を確認</button>{/if}
 </div>{/each}
 {#if selected}<section aria-label="用途の復元確認" class="my-3 border p-3"><h4>{selected.purpose?.name}を戻す</h4><p>{selected.files.join(' / ')}</p><p>記録された構成ファイルと用途登録を戻します。変更された内容は上書きせず競合として扱います。実行状態は変更しません。</p><button disabled={blocked||busy} onclick={()=>void restore()}>この記録から復元</button><button disabled={busy} onclick={()=>selected=null}>復元を取り消す</button></section>{/if}
</section>



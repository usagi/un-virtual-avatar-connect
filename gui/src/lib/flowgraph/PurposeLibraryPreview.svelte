<script lang="ts">
 import { requestPurposeResponse } from "../api";
 import { onMount, tick } from 'svelte';
 import PurposeTransferPreview from './PurposeTransferPreview.svelte';
 import PurposeRemovalPreview from './PurposeRemovalPreview.svelte';
 import PurposeRemovalHistoryPreview from './PurposeRemovalHistoryPreview.svelte';
 let purposeArea=$state<'manage'|'transfer'|'recovery'>('manage');
 const areaLabels={manage:'用途と構成',transfer:'共有・取込',recovery:'履歴・復元'};
 let libraryElement:HTMLElement;
 let transferPending=$state(false);
 let recoveryPending=$state(false);
 import { flowgraphStore } from '../flowgraphStore.svelte';
 import { tabNavStore } from '../tabs.svelte';
 type Purpose={id:string;name:string;roots:string[]};
 let studyingPurpose=$state<Purpose|null>(null);
 type Snapshot={root:string;revision:string|null;document:{schema_version:number;purposes:Purpose[]}};
 // Storage capability is explicit, independent of the chosen visual layout.
 // Preview-only URL flags are interpreted by the host, not by this editor.
 let {selected,onselect,onedit,refreshVersion=0,layout='stacked',storage='native'}:{selected:string[];onselect:(files:string[])=>void;onedit?:()=>void;refreshVersion?:number;layout?:'stacked'|'workspace';storage?:'native'|'memory'}=$props();
 const groupedLayout=$derived(layout==='workspace');
 let seenRefresh=$state(0);
 let snapshot=$state<Snapshot|null>(null),name=$state(''),error=$state(''),status=$state(''),busy=$state(false);
 let editingId=$state<string|null>(null);
 let editingMembers=$state(false);
 let previousSelection=$state<string[]>([]);
 let removal=$state<{purpose:Purpose;expected:Snapshot}|null>(null);
 let history=$state<{root:string;entries:{backup:string;document:Snapshot['document']|null;error:string|null}[]}|null>(null);
 const nativeStorage=$derived(storage==='native');
 type Recovery={id:string;snapshot:{id:string;revision:string;receipt:{purpose:Purpose}}|null;error:string|null};
 let recovery=$state<{root:string;entries:Recovery[]}|null>(null);
 async function loadRecovery(){
  if(busy)return;busy=true;error='';
  try{recovery=await request<NonNullable<typeof recovery>>('recovery',{});}catch(e){error=String(e);}finally{busy=false;}
 }
 async function retryRegistration(entry:Recovery){
  if(!snapshot||!recovery||!entry.snapshot||busy||wrongRoot||recovery.root!==snapshot.root||editingId||removal||name.trim())return;
  busy=true;error='';status='';
  try{snapshot=await request('retry',{expected:$state.snapshot(snapshot),id:entry.id,revision:entry.snapshot.revision});status='用途の登録を復旧しました。構成ファイルの再取込は行っていません。';}catch(e){error=String(e);}finally{busy=false;}
 }
 export async function revealTransferHistory(){
  if(busy||transferPending||recoveryPending||studyingPurpose||editingId||removal||name.trim()){
   error='現在の編集・確認を完了するか取り消してから、取込履歴を開いてください。';return false;
  }
  purposeArea='transfer';
  await tick();
  const history=libraryElement.querySelector<HTMLDetailsElement>('[data-purpose-transfer-history]');
  if(!history)return false;
  history.open=true;history.scrollIntoView({block:'center'});history.querySelector('summary')?.focus();
  return true;
 }
 async function openMember(fq:string){
  if(busy||wrongRoot||transferPending||recoveryPending||studyingPurpose||editingId||removal||name.trim()||flowgraphStore.treeError)return;
  if(!flowgraphStore.tree?.files.some(file=>file.fq===fq)){error='保存された構成が見つかりません。一覧を更新してください。';return;}
  if(await flowgraphStore.openFile(fq))onedit?.();
 }
 const editingPurpose=$derived(snapshot?.document.purposes.find(purpose=>purpose.id===editingId));
 const wrongRoot=$derived(!!snapshot && snapshot.root!==flowgraphStore.tree?.root_dir);
 async function request<T=Snapshot>(action:string,body:unknown):Promise<T>{
  const response=await requestPurposeResponse(action,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  const value=await response.json();
  if(!response.ok)throw Error(value.detail ?? value.error ?? `HTTP ${response.status}`);
  return value;
 }
 async function removeRegistration(){
  if(!removal||busy||wrongRoot||name.trim()||editingId)return;
  busy=true;error='';status='';
  const reviewed=$state.snapshot(removal);
  try{
   snapshot=await request('save',{expected:reviewed.expected,document:{...reviewed.expected.document,purposes:reviewed.expected.document.purposes.filter(item=>item.id!==reviewed.purpose.id)}});
   removal=null;history=null;status=`「${reviewed.purpose.name}」の登録を外しました。グラフは削除していません。`;
  }catch(e){error=String(e);}finally{busy=false;}
 }
 async function loadHistory(){
  if(busy)return;
  busy=true;error='';
  try{history=await request<NonNullable<typeof history>>('history',{});}catch(e){error=String(e);}finally{busy=false;}
 }
 async function restoreRegistration(purpose:Purpose){
  if(!snapshot||!history||history.root!==snapshot.root||wrongRoot||busy||name.trim()||editingId||removal||snapshot.document.purposes.some(item=>item.id===purpose.id))return;
  busy=true;error='';status='';
  try{
   snapshot=await request('save',{expected:$state.snapshot(snapshot),document:{...$state.snapshot(snapshot.document),purposes:[...$state.snapshot(snapshot.document.purposes),$state.snapshot(purpose)]}});
   status=`「${purpose.name}」の登録を復元しました。グラフの実行状態は変更していません。`;
  }catch(e){error=String(e);}finally{busy=false;}
 }
 async function reloadFromUser(){
  // Preserve the snapshot used to begin editing, so reload cannot silently
  // rebase an old draft onto another writer's revision before saving.
  if(busy||transferPending||recoveryPending||studyingPurpose||editingId||removal||name.trim()){
   error='用途の編集・確認が残っています。保存するか取り消してから、一覧を再取得してください。';return;
  }
  await reload();
 }
 async function reload(){
  if(busy)return;
  busy=true;error='';status='';
  try{snapshot=await request('read',{});}catch(e){error=String(e);}finally{busy=false;}
 }
 async function save(){
  if(!snapshot||wrongRoot||busy||!name.trim()||(editingId ? !editingPurpose : !selected.length)||(editingMembers&&!selected.length)||flowgraphStore.treeError)return;
  busy=true;error='';status='';
  const purpose=editingPurpose ? {...$state.snapshot(editingPurpose),...(editingMembers ? {roots:[...selected]} : {name:name.trim()})} : {id:crypto.randomUUID(),name:name.trim(),roots:[...selected]};
  try{
   const purposes=editingId ? $state.snapshot(snapshot.document.purposes).map(item=>item.id===editingId ? purpose : item) : [...$state.snapshot(snapshot.document.purposes),purpose];
   snapshot=await request('save',{expected:$state.snapshot(snapshot),document:{...$state.snapshot(snapshot.document),purposes}});
   name='';editingId=null;editingMembers=false;previousSelection=[];status=`「${purpose.name}」を保存しました。グラフの実行状態は変更していません。`;
  }catch(e){error=String(e);}finally{busy=false;}
 }
 onMount(()=>{
  let mounted=true;
  void reload().then(async()=>{
   await tick();
   if(mounted && import.meta.env.VITE_UN_DESIGN_PREVIEW==='1' && new URLSearchParams(window.location.search).get('purposeFocus')==='library'){
    libraryElement?.scrollIntoView({block:'start'});
    libraryElement?.focus({preventScroll:true});
   }
  });
  const unguard=tabNavStore.guardLeaving(()=>{
   if(busy||transferPending||recoveryPending||studyingPurpose||editingId||removal||name.trim()){error='用途の編集・確認が残っています。保存するか、入力・確認を取り消してから移動してください。';return false;}
   return true;
  });
  return ()=>{mounted=false;unguard();};
 });
 $effect(()=>{
  if(refreshVersion!==seenRefresh && !busy && !transferPending && !recoveryPending && !studyingPurpose && !editingId && !removal && !name.trim()){
   seenRefresh=refreshVersion;void reload();
  }
 });
</script>

<section bind:this={libraryElement} tabindex="-1" class="mt-4 scroll-mt-4 rounded border border-surface-300-700 p-4 text-sm" aria-label="名前を付けた用途の管理">
 <h3 class="font-semibold">{groupedLayout?'用途を管理する':'用途に名前を付ける'}</h3>
 {#if groupedLayout}
  <p class="my-2 text-xs">用途は、複数の構成ファイルを名前でまとめたもの。構成の編集は既存のFlowgraphエディターで行えます。</p>
  <nav aria-label="用途管理の作業" class="purpose-areas">
   {#each ['manage','transfer','recovery'] as area}
    <button aria-current={purposeArea===area?'page':undefined} disabled={busy||transferPending||recoveryPending||!!studyingPurpose||!!editingId||!!removal||!!name.trim()} onclick={()=>purposeArea=area as typeof purposeArea}>{areaLabels[area as keyof typeof areaLabels]}</button>
   {/each}
  </nav>
  {#if busy||transferPending||recoveryPending||studyingPurpose||editingId||removal||name.trim()}<p role="status" class="my-2 text-xs">現在の編集・確認を完了するか取り消してから、作業を切り替えられます。</p>{/if}
 {/if}
 <fieldset disabled={transferPending||recoveryPending||!!studyingPurpose}>
 <p class="my-2 text-xs opacity-70">選んだ構成をまとめて呼び出すための登録。参照先の部品はその都度確認します。</p>
 {#if snapshot}<p class="my-2 break-all text-xs">保存先: {snapshot.root}</p>{/if}
 <p class="text-xs opacity-70">同じ構成フォルダーを使うVACでは、この一覧も共有されます。{nativeStorage ? '表示中の保存先へ保存します。VACを終了しても登録は残ります。' : '隔離試作ではサーバー終了までの保存です。'}</p>
 {#if wrongRoot}<p role="alert">表示中の構成と用途の保存先が異なります。保存先を確認して一覧を再取得してください。</p>{/if}
 {#if error}<p role="alert" class="my-2 text-warning-500">{error}</p>{/if}
 {#if status}<p role="status" class="my-2">{status}</p>{/if}
 <div hidden={groupedLayout&&purposeArea!=='manage'}>
 <label class="my-3 flex flex-col gap-1">用途名<input class="rounded border bg-surface-50-950 p-2" bind:value={name} disabled={busy||!!removal||editingMembers} /></label>
 {#if editingMembers}<section aria-label="用途の構成変更" class="my-2 text-xs"><p class="break-all">変更対象: {editingPurpose?.name ?? '一覧に見つかりません'} · {editingId}</p><p class="break-all">保存済み: {editingPurpose?.roots.join(' / ')}</p><p class="break-all">変更後: {selected.join(' / ') || '未選択'}</p><p>上の全構成一覧で選び直してください。名前とグラフの内容は変更しません。</p></section>
 {:else if editingId}<p class="break-all text-xs">名前の変更対象: {editingPurpose?.name ?? '一覧に見つかりません'} · {editingId}。構成の選択は変更しません。</p>
 {:else}<p class="text-xs">現在の選択: {selected.length}ファイル</p>{/if}
 <button class="my-2 rounded border px-3 py-2 disabled:opacity-50" disabled={busy||!snapshot||wrongRoot||!name.trim()||(editingId ? !editingPurpose : !selected.length)||(editingMembers&&!selected.length)||!!flowgraphStore.treeError} onclick={()=>void save()}>{editingMembers ? '構成の組合せを保存' : editingId ? '名前の変更を保存' : '選択した構成を用途として保存'}</button>
 <button class="ml-3 underline" disabled={busy||(!name&&!editingId)} onclick={()=>{if(editingMembers)onselect([...previousSelection]);name='';editingId=null;editingMembers=false;previousSelection=[];error='';}}>{editingMembers ? '構成の変更を取り消す' : '用途名の入力を取り消す'}</button>
 <button class="ml-3 underline" disabled={busy} onclick={()=>void reloadFromUser()}>用途一覧を再取得</button>
 {#if snapshot&&!wrongRoot}
  <ul class="divide-y divide-surface-200-800">
   {#each snapshot.document.purposes as purpose (purpose.id)}
    <li class="py-3"><strong>{purpose.name}</strong><p class="break-all text-xs opacity-60">{purpose.id}</p>
     <ul class="my-2 text-xs">{#each purpose.roots as fq}<li class="my-2 flex flex-wrap items-center gap-3 break-all"><span>{fq}</span>{#if flowgraphStore.tree&&!flowgraphStore.treeError&&!flowgraphStore.tree.files.some(file=>file.fq===fq)}<span>保存された構成が見つかりません</span>{:else}<button class="rounded border px-2 py-1" aria-label={`構成を編集: ${fq}`} disabled={busy||wrongRoot||!!editingId||!!removal||!!name.trim()||!!flowgraphStore.treeError} onclick={()=>void openMember(fq)}>構成を編集</button>{/if}</li>{/each}</ul>
     <button class="underline" disabled={busy||!!editingId||!!removal||!!flowgraphStore.treeError} onclick={()=>onselect([...purpose.roots])}>この用途の構成を選択</button>
     <button class="ml-3 underline" disabled={busy||!!editingId||!!removal||!!name.trim()} onclick={()=>{editingId=purpose.id;name=purpose.name;status='';error='';}}>名前を変更</button>
     <button class="ml-3 underline" disabled={busy||!!editingId||!!removal||!!name.trim()||!!flowgraphStore.treeError} onclick={()=>{previousSelection=[...selected];editingId=purpose.id;editingMembers=true;name=purpose.name;onselect([...purpose.roots]);status='';error='';}}>構成の組合せを編集</button>
     <button class="ml-3 underline" disabled={busy||!!editingId||!!removal||!!name.trim()} onclick={()=>{if(snapshot)removal={purpose:$state.snapshot(purpose),expected:$state.snapshot(snapshot)};status='';error='';}}>登録を外す前に確認</button>
     {#if nativeStorage}<button class="ml-3 underline" disabled={busy||!!editingId||!!removal||!!name.trim()} onclick={()=>studyingPurpose=$state.snapshot(purpose)}>構成ファイルへの影響を確認</button>{/if}
    </li>
   {/each}
  </ul>
 {/if}
 {#if removal}
  <section class="my-3 rounded border border-warning-500 p-3" aria-label="用途登録を外す確認">
   <p>{removal.purpose.name} · {removal.purpose.id}</p><p class="break-all text-xs">保存先: {removal.expected.root}</p>
   <p class="my-2">用途一覧からこの登録を外します。構成ファイルと実行状態は変わりません。登録は保存履歴から戻せます。</p>
   <button disabled={busy||wrongRoot} onclick={()=>void removeRegistration()}>この用途の登録だけを外す</button>
   <button class="ml-3 underline" disabled={busy} onclick={()=>removal=null}>登録を外す操作を取り消す</button>
  </section>
 {/if}
 </div>
 <div hidden={groupedLayout&&purposeArea!=='recovery'}>
 <button class="mt-3 underline" disabled={busy} onclick={()=>void loadHistory()}>用途の保存履歴を確認</button>
 {#if history}
  <section aria-label="用途の保存履歴" class="my-3">
   <p class="break-all text-xs">履歴の保存先: {history.root}</p>
   {#each history.entries as entry}
    <details class="my-2"><summary class="cursor-pointer break-all text-xs">{entry.backup}</summary>
     {#if entry.error}<p role="alert">この履歴は読み込めません: {entry.error}</p>{/if}
     {#each entry.document?.purposes ?? [] as purpose}
      <div class="my-2 border-l border-surface-300-700 pl-3"><strong>{purpose.name}</strong><p class="break-all text-xs">{purpose.id}</p><p class="break-all text-xs">{purpose.roots.join(' / ')}</p>
       <button disabled={busy||wrongRoot||history.root!==snapshot?.root||!!editingId||!!removal||!!name.trim()||!!snapshot?.document.purposes.some(item=>item.id===purpose.id)} onclick={()=>void restoreRegistration(purpose)}>この登録を復元</button>
       {#if snapshot?.document.purposes.some(item=>item.id===purpose.id)}<span class="ml-2 text-xs">同じIDの登録があります</span>{/if}
      </div>
     {/each}
    </details>
   {/each}
  </section>
 {/if}
 {#if nativeStorage}
  <button class="my-3 underline" disabled={busy} onclick={()=>void loadRecovery()}>取込後の登録復旧を確認</button>
  {#if recovery}<section aria-label="取込後の登録復旧" class="my-3">
   <p class="break-all text-xs">記録の保存先: {recovery.root}</p>
   {#if !recovery.entries.length}<p>復旧記録はありません。</p>{/if}
   {#each recovery.entries as entry (entry.id)}
    <div class="my-3 border-l pl-3">
     {#if entry.error}<p role="alert">この記録は読み込めません: {entry.error}</p>{/if}
     {#if entry.snapshot}<strong>{entry.snapshot.receipt.purpose.name}</strong><p class="break-all text-xs">{entry.id}</p><p class="break-all text-xs">{entry.snapshot.receipt.purpose.roots.join(' / ')}</p>
      {#if snapshot?.document.purposes.some(p=>p.id===entry.id)}<p>同じIDの登録があります。</p>
      {:else}<button disabled={busy||wrongRoot||recovery.root!==snapshot?.root||!!editingId||!!removal||!!name.trim()} onclick={()=>void retryRegistration(entry)}>構成を再取込せず登録だけを復旧</button>{/if}
     {/if}
    </div>
   {/each}
  </section>{/if}
 {/if}
 </div>
 </fieldset>
 {#if studyingPurpose&&snapshot}<PurposeRemovalPreview purpose={studyingPurpose} {snapshot} currentRoot={flowgraphStore.tree?.root_dir} onclose={()=>studyingPurpose=null} onchanged={async()=>{await reload();await flowgraphStore.refreshTree();}}/>{/if}
 <div hidden={groupedLayout&&purposeArea!=='recovery'}>
 {#if nativeStorage&&snapshot}<PurposeRemovalHistoryPreview root={snapshot.root} blocked={busy||transferPending||wrongRoot||!!studyingPurpose||!!editingId||!!removal||!!name.trim()} onchanged={async()=>{await reload();await flowgraphStore.refreshTree();}} onpending={(value)=>recoveryPending=value}/>{/if}
 </div>
 <div hidden={groupedLayout&&purposeArea!=='transfer'}>
 {#if groupedLayout&&!nativeStorage}<p class="my-3">この比較データでは用途ZIPの操作を接続していません。全構成一覧の共有から、構成ファイルの書出しを確認できます。</p>{/if}
 {#if nativeStorage}<PurposeTransferPreview {snapshot} blocked={busy||recoveryPending||wrongRoot||!!studyingPurpose||!!editingId||!!removal||!!name.trim()} onchanged={async()=>{await reload();await flowgraphStore.refreshTree();}} onpending={(value)=>transferPending=value}/>{/if}
 </div>
</section>

<style>
 .purpose-areas{display:flex;flex-wrap:wrap;gap:.5rem;margin:1rem 0;}
 .purpose-areas button{border:1px solid var(--color-surface-400);border-radius:.65rem;padding:.55rem .9rem;}
 .purpose-areas button[aria-current="page"]{border-color:var(--color-primary-500);background:color-mix(in srgb,var(--color-primary-500) 15%,transparent);}
 .purpose-areas button:disabled{opacity:.55;}
</style>

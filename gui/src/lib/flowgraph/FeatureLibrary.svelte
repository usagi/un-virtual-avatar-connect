<script lang="ts">
 import { requestPurposeResponse } from "../api";
 import { api } from '../api';
 import { flowgraphStore } from '../flowgraphStore.svelte';
 import { tabNavStore } from '../tabs.svelte';
 import type { ZipImportPreview } from '../types';
 import { FEATURE_PACKS, featureZip } from './featurePacks';
 import FeatureAnchor from '../FeatureAnchor.svelte';
 import FeatureUsageGuide from './FeatureUsageGuide.svelte';
 import FeatureSharePreview from './FeatureSharePreview.svelte';
 import PurposeLibraryPreview from './PurposeLibraryPreview.svelte';
 const designPreview=import.meta.env.VITE_UN_DESIGN_PREVIEW === '1';
 const previewParams=typeof window !== 'undefined' ? new URLSearchParams(window.location.search) : new URLSearchParams();
 const purposeEnabled=!designPreview || previewParams.get('purposes')==='preview';
 const namedInstall=!designPreview || (purposeEnabled && previewParams.get('purposeStorage')==='native');
 const purposeLayout: 'workspace' | 'stacked'=!designPreview || previewParams.get('purposeLayout')==='workspace' ? 'workspace' : 'stacked';
 let purposeSnapshot: {root:string;revision:string|null;document:unknown}|null = null;
 let purposeRefresh=$state(0);
 let purposeLibrary=$state<{revealTransferHistory:()=>Promise<boolean>} | undefined>();
 let installIssue=$state<{root:string;title:string;archive:Uint8Array;detail:string}|null>(null);
 function saveRecoveryZip(){
  if(!installIssue)return;
  const url=URL.createObjectURL(new Blob([installIssue.archive as BlobPart],{type:'application/zip'}));
  const link=document.createElement('a');link.href=url;link.download='feature-install-recovery.zip';link.click();URL.revokeObjectURL(url);
 }
 function showTransferHistory(){void purposeLibrary?.revealTransferHistory();}

 async function purposeRequest(action:string,body:unknown) {
  const response=await requestPurposeResponse(action,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  const value=await response.json();
  if(!response.ok) throw Error(JSON.stringify(value));
  return value;
 }
 let shareCandidate = $state<string | null>(null);
 let shareFiles = $state<string[]>([]);
 let selectedForShare = $state<string[]>([]);
 let selectionRoot = $state<string | null>(null);
 let { onedit, expanded = false }: { onedit?: () => void; expanded?: boolean } = $props();
 async function editGraph(fq: string) {
  if(busy || flowgraphStore.mutating) return;
  if (await flowgraphStore.openFile(fq)) onedit?.();
 }
 let removal = $state<{fq:string; identity:string; label:string; plan:Awaited<ReturnType<typeof api.flowgraphRemovalPlan>>} | null>(null);
 let includeAllSaved = $state(false);
 const inventoryFocused = $derived(tabNavStore.featureFocus?.id === 'feature-inventory');
 const allSaved = $derived(flowgraphStore.tree?.files ?? []);
 $effect(()=>{
  const root=flowgraphStore.tree?.root_dir ?? null;
  if(root!==selectionRoot){selectionRoot=root;selectedForShare=[];shareFiles=[];shareCandidate=null;}
 });
 const completeInventory = $derived(includeAllSaved || inventoryFocused);
 const installed = $derived(completeInventory ? allSaved : allSaved.filter(file=>file.fq.startsWith('features/')));
 async function inspectRemoval(fq:string, label:string) {
  if(busy || flowgraphStore.mutating) return;
  const root=flowgraphStore.tree?.root_dir;
  busy=true; error=''; removal=null; result=''; chosen=null; preview=null;
  try {
   const [profile,plan]=await Promise.all([api.desktopProfiles(),api.flowgraphRemovalPlan(fq)]);
   if(!profile.current_conf_path) throw Error('操作対象を確認できません。');
   if(!root || flowgraphStore.tree?.root_dir!==root || plan.root_dir!==root) throw Error('保存先が変わりました。削除内容を再確認してください。');
   removal={fq,label,plan,identity:profile.current_conf_path};
  } catch(e) { error=String(e); } finally {busy=false;}
 }
 async function remove() {
  if(busy || flowgraphStore.mutating || !removal || !removal.plan.can_delete || flowgraphStore.isDirty) return;
  const target=removal;
  busy=true; error='';
  try {
   const current=await api.desktopProfiles();
   if(current.current_conf_path!==target.identity || flowgraphStore.tree?.root_dir!==target.plan.root_dir) throw Error('操作対象が変わりました。削除内容を再確認してください。');
   const report=await flowgraphStore.deleteFile(target.fq, target.plan.expected_content, target.plan.root_dir);
   if(flowgraphStore.tree?.root_dir!==target.plan.root_dir){
    removal=null; result=''; resultTree=null;
    throw Error(`表示中の保存先が変わりました。削除先 ${target.plan.root_dir} の構成と履歴で結果を確認してください。`);
   }
   if(!report) throw Error('削除を完了できませんでした。編集中の変更・処理状態・通知を確認してください。');
   result=`${report.ok ? (report.restart_required ? '削除済み' : '削除・適用済み') : '削除済み。適用時の診断を確認してください'}。${report.restart_required ? 'Web入力の反映にはこのVACの再起動が必要です。' : ''}バックアップ: ${report.backup ?? '不明'}`;
   resultTree=flowgraphStore.tree;
   removal=null; preview=null;
  } catch(e) {error=String(e);} finally {busy=false;}
 }
 async function exportInstalled(fq:string) {
  if (busy || flowgraphStore.mutating) return;
  if (flowgraphStore.hasDraftForFile(fq, flowgraphStore.tree?.root_dir)) {
   error='このファイルには未保存の変更があります。Studioで保存してから書き出してください。';
   return;
  }
  busy=true; error='';
  try {
   const blob=await flowgraphStore.exportZip({scope:'file',targets:[{kind:'file',fq}]});
   if(!blob) throw Error('書出しを完了できませんでした。対象の保存先と通知を確認してください。');
   const url=URL.createObjectURL(blob); const link=document.createElement('a');
   link.href=url; link.download=fq.replaceAll('/','-')+'.zip'; link.click(); URL.revokeObjectURL(url);
  } catch(e) {error=String(e);} finally {busy=false;}
 }
 let chosen = $state<(typeof FEATURE_PACKS)[number] | null>(null);
 let preview = $state<ZipImportPreview | null>(null);
 let scope = $state('');
 let identity = $state('');
 let importRoot = $state('');
 let busy = $state(false);
 let error = $state('');
 let result = $state('');
 // An operation result describes this inventory snapshot, not later restores,
 // imports or a different profile loaded elsewhere in the interface.
 let resultTree = $state.raw<typeof flowgraphStore.tree>(null);
 async function inspect(pack:(typeof FEATURE_PACKS)[number]) {
  if(busy || flowgraphStore.mutating) return;
  busy=true; error=''; result=''; preview=null; removal=null; chosen=pack; purposeSnapshot=null;
  try {
   importRoot=flowgraphStore.tree?.root_dir ?? '';
   if(!importRoot) throw Error('取込先を確認できません。ファイル一覧を更新してください。');
   const profiles=await api.desktopProfiles();
   identity=profiles.current_conf_path ?? '';
   if(!identity) throw Error('操作対象を確認できません。接続状態を確認してください。');
   scope=`${profiles.profiles.find(p=>p.id===profiles.current_profile_id)?.label ?? '設定ファイル'} · ${identity}`;
   if(namedInstall){
    purposeSnapshot=await purposeRequest('read',{});
    if(purposeSnapshot?.root!==importRoot) throw Error('用途の保存先が変わりました。もう一度確認してください。');
   }
   const next=namedInstall ? await purposeRequest('preview',{expected:purposeSnapshot,archive:Array.from(featureZip(pack.source,pack.title)),options:{dry_run:true,target_prefix:`features/${pack.id}`,on_conflict_file:'skip'}}) : await api.flowgraphImportZip(featureZip(pack.source),{dry_run:true,expected_root:importRoot,target_prefix:`features/${pack.id}`,on_conflict_file:'skip'});
   if(next.kind==='preview') preview=next;
  } catch(e) { error=String(e); } finally { busy=false; }
 }
 async function install() {
  if(busy || flowgraphStore.mutating || !chosen || !preview || preview.conflicts.length || flowgraphStore.isDirty) return;
  busy=true; error='';
  let sentArchive:Uint8Array|null=null;
  try {
   const current=await api.desktopProfiles();
   if(current.current_conf_path!==identity) throw Error('操作対象が変わりました。もう一度追加内容を確認してください。');
   if(flowgraphStore.tree?.root_dir!==importRoot) throw Error('保存先が変わりました。もう一度追加内容を確認してください。');
   if(namedInstall){
    if(!purposeSnapshot || purposeSnapshot.root!==importRoot)throw Error('用途の追加内容を再確認してください。');
    sentArchive=featureZip(chosen.source,chosen.title);
    const report=await purposeRequest('install',{expected:purposeSnapshot,archive:Array.from(sentArchive),options:{dry_run:false,target_prefix:preview.target_prefix,on_conflict_file:'skip'}});
    if(flowgraphStore.tree?.root_dir!==importRoot){
     preview=null;
     throw Error(`表示中の保存先が変わりました。追加先 ${importRoot} の取込履歴で結果を確認してください。`);
    }
    installIssue=report.registration && !report.journal_error ? null : {root:importRoot,title:chosen.title,archive:sentArchive,detail:String(report.registration_error ?? report.journal_error ?? '用途の登録結果を確認できません。')};
    result=report.registration ? report.reload_ok ? report.restart_required ? '用途として保存・登録し、実行構成を読み込みました。Web入力の変更を反映するには、このVACの再起動が必要です。' : '用途として保存・登録・適用済み。必要な接続と出力を確認してください。' : '用途として保存・登録済み。適用は未完了です。診断を確認してください。' : 'ファイルの保存結果を用途の取込履歴で確認してください。用途の登録は未完了です。';
    if(report.registration_error || report.apply_error || report.journal_error)error=String(report.registration_error ?? report.apply_error ?? report.journal_error);
    preview=null; purposeSnapshot=null; purposeRefresh++;
    await flowgraphStore.refreshTree(); await flowgraphStore.refreshDiagnostics();
    if(flowgraphStore.tree?.root_dir!==importRoot){
     result='';
     throw Error(`表示中の保存先が変わりました。追加先 ${importRoot} の取込履歴で結果を確認してください。`);
    }
    resultTree=flowgraphStore.tree;
    return;
   }
   const report=await flowgraphStore.importZip(new Blob([featureZip(chosen.source) as BlobPart]),{dry_run:false,expected_root:importRoot,target_prefix:preview.target_prefix,on_conflict_file:'skip'});
   if(flowgraphStore.tree?.root_dir!==importRoot){
    preview=null;
    throw Error(`表示中の保存先が変わりました。追加先 ${importRoot} の構成と診断で結果を確認してください。`);
   }
   if(!report || report.kind!=='report') throw Error('追加に失敗しました。通知と診断を確認してください。');
   result=report.reload_ok===false ? 'ファイルは保存済みですが、適用を確認できません。診断を確認してください。' : report.restart_required ? '保存済み。Web入力の変更を反映するには、このVACの再起動が必要です。' : report.written_files.length===0 ? '追加対象は保存済みです。既存ファイルは上書きしていません。' : report.reload_ok ? '保存・適用済み。必要な接続と出力を確認してください。' : 'ファイルは保存済みですが、適用を確認できません。診断を確認してください。';
   resultTree=flowgraphStore.tree;
   preview=null;
  } catch(e) {
   error=String(e);
   if(sentArchive && chosen){
    installIssue={root:importRoot,title:chosen.title,archive:sentArchive,detail:String(e)};
    preview=null;purposeSnapshot=null;
   }
  } finally { busy=false; }
 }
</script>
<details open={expanded || tabNavStore.featureFocus?.id === "feature-library" || inventoryFocused} class="vac-panel p-4" aria-label="用途から機能を追加">
 <summary class="cursor-pointer font-semibold">機能を追加・管理 <span class="text-xs font-normal opacity-65">AIなしでも利用可能</span></summary>
 <p class="my-3 text-xs opacity-65">今接続しているVACに、編集可能なFlowgraphを追加します。新しいプロファイルは設定画面で作成できます。</p>
 {#if installIssue}
 <section aria-label="機能追加の結果確認" class="my-3 rounded border border-warning-500 p-3 text-sm">
  <h3>{installIssue.title} · 保存結果の確認が必要</h3>
  <p class="my-2 break-all">対象の保存先: {installIssue.root}</p>
  <p>保存が進んでいる可能性があります。追加を繰り返す前に取込履歴を取得し、保存済みの内容と用途登録を確認してください。復旧は自動では実行しません。</p>
  <button class="mt-2 underline" disabled={busy||flowgraphStore.tree?.root_dir!==installIssue.root} onclick={showTransferHistory}>取込履歴の確認へ</button>
  <button class="ml-3 underline" disabled={busy} onclick={saveRecoveryZip}>復旧用に同じZIPを保存</button>
  <p class="mt-2 text-xs">履歴で取込の再開にZIPが必要な場合は、このZIPを選びます。復旧後の実行への適用は別途確認してください。</p>
  <details class="mt-2 text-xs"><summary>処理結果の詳細</summary><p class="break-all">{installIssue.detail}</p></details>
 </section>
 {/if}
 {#if flowgraphStore.isDirty}
  <p role="status" class="my-3 text-sm">
   {flowgraphStore.currentFq} に未保存の変更があります。
   {#if flowgraphStore.canRetainCurrentDraft}別の機能を編集しても、この下書きは保持します。追加・削除は保存後に行えます。
   {:else}別の機能の編集・追加・削除は、編集中の変更を保存してから行えます。{/if}
   {#if onedit}<button class="ml-2 underline" onclick={()=>onedit?.()}>編集中のグラフへ戻る</button>{/if}
  </p>
 {/if}
 <div class="grid gap-3 md:grid-cols-2">
  {#each FEATURE_PACKS as pack}
   <article class="rounded border border-surface-300-700 p-3">
    <h3 class="text-sm font-semibold">{pack.title}</h3><p class="my-2 text-xs">{pack.description}</p>
    <p class="text-xs opacity-65">必要なもの: {pack.needs}</p>
    <button class="mt-3 rounded border px-3 py-1.5 text-xs" disabled={busy || flowgraphStore.mutating} onclick={()=>void inspect(pack)}>追加内容を確認: {pack.title}</button>
    <FeatureUsageGuide packId={pack.id} />
   </article>
  {/each}
 </div>
 <FeatureAnchor id="feature-installed"><section class="mt-4 border-t border-surface-300-700 pt-3" aria-label="導入済み機能">
   <FeatureAnchor id="feature-inventory"><label class="my-2 flex items-center gap-2 text-sm"><input type="checkbox" checked={completeInventory} disabled={inventoryFocused} onchange={event=>includeAllSaved=event.currentTarget.checked} />保存場所を問わず全構成を表示（{allSaved.length}ファイル）</label></FeatureAnchor>
  <h3 class="font-semibold">{completeInventory ? '保存されている全構成' : '追加先に保存されている機能'}</h3>
  {#if completeInventory}
   <div class="my-3 text-sm">
    <p>用途を構成するファイルを複数選び、必要な参照先と一緒に共有できます。選択は削除対象や所有範囲を指定するものではありません。</p>
    <button class="mt-2 rounded border px-3 py-2" disabled={!selectedForShare.length || busy || flowgraphStore.mutating || !!flowgraphStore.treeError} onclick={()=>{shareFiles=[...selectedForShare];shareCandidate=shareFiles[0];}}>選択した{selectedForShare.length}ファイルの共有範囲を確認</button>
    <button class="ml-3 underline" disabled={!selectedForShare.length} onclick={()=>selectedForShare=[]}>選択を解除</button>
   </div>
  {/if}
  <p class="my-2 text-xs opacity-65">{completeInventory ? '取込先や作成方法に関係なく実ファイルを表示。1ファイルが1つの用途とは限りません。' : 'features配下の実ファイルを表示。編集された構成もそのまま扱います。'}削除は選択ファイルだけが対象で、認証情報・共有設定・関連する別ファイルは残します。</p>
  <p class="my-2 text-xs opacity-65">書出は選択した保存済みファイルのZIPです。参照する別ファイルや画像・音声素材は自動では含まれません。</p>
  {#if flowgraphStore.treeError}<p role="alert" class="text-xs">一覧を更新できません: {flowgraphStore.treeError}</p><button class="my-2 text-sm underline" onclick={()=>void flowgraphStore.refreshTree()}>保存一覧を再取得</button>
  {:else if !flowgraphStore.tree}<p class="text-xs">保存された機能を確認中…</p>
  {:else if installed.length===0}<p class="text-xs">追加先に機能はありません。既存の別フォルダーの構成はStudioのファイル一覧で管理できます。</p>{/if}
  {#each (flowgraphStore.treeError ? [] : installed) as file (file.fq)}
   <div class="my-2 flex flex-wrap items-center gap-2 rounded border border-surface-300-700 p-2 text-xs">
    {#if completeInventory}<input type="checkbox" aria-label={`共有対象に選択: ${file.fq}`} checked={selectedForShare.includes(file.fq)} onchange={event=>selectedForShare=event.currentTarget.checked ? [...selectedForShare,file.fq] : selectedForShare.filter(fq=>fq!==file.fq)} />{/if}
    <div class="min-w-0 flex-1"><strong>{file.title ?? file.fq}</strong><p class="break-all opacity-65">{file.path}</p></div>
    <button disabled={busy || flowgraphStore.mutating || (flowgraphStore.isDirty && !flowgraphStore.canRetainCurrentDraft)} onclick={()=>void editGraph(file.fq)}>編集: {file.title ?? file.fq}</button>
    <button disabled={busy || flowgraphStore.mutating} onclick={()=>{shareFiles=[];shareCandidate=file.fq;}}>共有に必要なものを確認</button>
    <button disabled={busy || flowgraphStore.mutating || flowgraphStore.hasDraftForFile(file.fq, flowgraphStore.tree?.root_dir)} onclick={()=>void exportInstalled(file.fq)}>保存済みファイルを書出: {file.title ?? file.fq}</button>
    <button disabled={busy || flowgraphStore.mutating || flowgraphStore.isDirty || flowgraphStore.hasDraftForFile(file.fq, flowgraphStore.tree?.root_dir) || !!flowgraphStore.treeError} onclick={()=>void inspectRemoval(file.fq,file.title ?? file.fq)}>削除前に確認: {file.title ?? file.fq}</button>
   </div>
  {/each}
 </section></FeatureAnchor>
 {#if purposeEnabled}<PurposeLibraryPreview bind:this={purposeLibrary} {onedit} refreshVersion={purposeRefresh} selected={selectedForShare} storage={namedInstall?'native':'memory'} layout={purposeLayout} onselect={files=>{selectedForShare=files;includeAllSaved=true;}} />{/if}
 {#if shareCandidate}<FeatureSharePreview fq={shareCandidate} files={shareFiles} onclose={()=>{shareCandidate=null;shareFiles=[];}} />{/if}
 {#if removal}
 <section class="mt-3 rounded border border-warning-500 p-3 text-xs" aria-label="機能削除の影響">
  <h3 class="font-semibold">{removal.label} の削除</h3>
  <p class="my-2 break-all">操作対象: {removal.identity}</p><p class="break-all">保存先: {removal.plan.root_dir}/{removal.fq}.flowgraph.toml</p>
  <p class="my-2">{removal.plan.can_delete ? '削除後の静的構成検証は成功。バックアップを残して削除し、このVACのFlowgraph全体を再読み込みします。' : '削除後の構成にエラーがあります。参照元を変更するか、先に診断を解消してください。'}</p>
  <p class="opacity-65">グラフ・型・パッケージの参照を検証します。動的に指定された宛先や、外部アプリからの利用までは判定できません。</p>
  {#each removal.plan.diagnostics as diagnostic}<p class="mt-2 break-all">{diagnostic.file ?? ''}: {diagnostic.message}</p>{/each}
  <button class="mt-3 rounded border border-warning-500 px-3 py-2" disabled={busy || flowgraphStore.mutating || flowgraphStore.isDirty || !removal.plan.can_delete} onclick={()=>void remove()}>このファイルを削除して適用</button>
  <button class="ml-3" disabled={busy} onclick={()=>removal=null}>キャンセル</button>
 </section>
 {/if}
 {#if error}<p role="alert" class="mt-3 text-sm text-error-500">{error}</p>{/if}
 {#if chosen && preview}
 <section class="mt-4 rounded border border-primary-500 p-3" aria-label="機能の追加内容">
  <h3 class="font-semibold">{chosen.title}</h3><p class="my-2 break-all text-xs">操作対象: {scope}</p>
  <p class="my-2 break-all text-xs">保存フォルダー: {importRoot}</p>
  <ul class="text-xs">{#each preview.entries as entry}<li>保存先: {entry.dest_path}</li>{/each}</ul>
  <p class="mt-2 text-xs">追加するとFlowgraph全体を再読み込みし、処理を開始します。一時状態は初期化される場合があります。{chosen.needs}</p>
  {#if preview.conflicts.length}<p role="status" class="mt-2 text-xs">同じ保存先が既にあります。重複追加しません。Studioのファイル一覧から編集できます。</p>{/if}
  {#if flowgraphStore.isDirty}<p class="mt-2 text-xs">編集中の変更を先に保存してください。</p>{/if}
  <button class="mt-3 rounded bg-primary-500 px-3 py-2 text-xs text-white" disabled={busy || flowgraphStore.mutating || flowgraphStore.isDirty || preview.conflicts.length>0} onclick={()=>void install()}>このVACへ追加して適用</button>
 </section>
 {/if}
 {#if result && resultTree === flowgraphStore.tree}<p role="status" class="mt-3 text-sm">{result}</p>{/if}
 {#if chosen && importRoot === flowgraphStore.tree?.root_dir && ((result && resultTree === flowgraphStore.tree) || preview?.conflicts.length)}<button class="mt-3 rounded border px-3 py-2 text-xs" disabled={busy || flowgraphStore.mutating || (flowgraphStore.isDirty && !flowgraphStore.canRetainCurrentDraft)} onclick={()=>void editGraph(`features/${chosen?.id}/main`)}>この機能をStudioで編集</button>{/if}
 <div class="mt-3 flex gap-4 text-xs"><button onclick={()=>tabNavStore.openFeature('resources','twitch')}>Twitch接続を設定</button><button onclick={()=>tabNavStore.openFeature('live','outputs')}>画面出力を確認</button><button onclick={()=>tabNavStore.openFeature('settings','profiles')}>プロファイルを作成・管理</button></div>
</details>

<script lang="ts">
 import { onMount } from 'svelte';
 import { api } from './api';
 import { linkedAppDraft, batchFromDraft, additionsFromDraft, formsFromDraft, draftEntry, modeAppSelection } from './linkedAppDraft.svelte';
 import { tabNavStore } from './tabs.svelte';
 import type { RunWithListResponse } from './types';
 let loaded=$state<RunWithListResponse|null>(null), error=$state(''), loading=$state(true), busy=$state(false), selected=$state(modeAppSelection.id);
 $effect(()=>{modeAppSelection.id=selected;});
 let review=$state<string|null>(null), reviewKind=$state<'save'|'discard'>('save');
 const actions=[['inherit','指定なし'],['start','起動'],['stop','終了要求'],['minimize','最小化'],['leave','状態を維持']];
 const modes=$derived(linkedAppDraft.value?.modes ?? loaded?.modes ?? []);
 const newForm=$derived(linkedAppDraft.value?formsFromDraft(linkedAppDraft.value).new:undefined);
 const entries=$derived([...(linkedAppDraft.value?.entries ?? loaded?.entries ?? []),...(newForm?[{index:-1,effective_id:'new',display_label:draftEntry(newForm.form).label||'新規登録',supports_status:false,entry:draftEntry(newForm.form),modes:[]}]:[])]);
 const changed=$derived(!!linkedAppDraft.value);
 async function load(){loading=true;error='';try{loaded=await api.runWithList();if(linkedAppDraft.value && (loaded.source_path!==linkedAppDraft.value.sourcePath || loaded.revision!==linkedAppDraft.value.revision))error='保存内容が変わっています。下書きは保持しました。保存時には競合として拒否されます。';if(!modes.some(m=>m.id===selected))selected=modes[0]?.id??'';}catch(e){error=String(e)}finally{loading=false}}
 onMount(()=>{void load();return tabNavStore.guardLeaving(()=>!busy&&!review)});
 function action(index:number){const draft=linkedAppDraft.value;const form=draft?formsFromDraft(draft)[index<0?'new':index]:null;if(form)return JSON.parse(form.form)[10]?.[selected]??'inherit';return draft?.modeEdits?.[index]?.[selected]??entries.find(e=>e.index===index)?.modes?.find(m=>m.id===selected)?.action??'inherit'}
 function change(index:number,value:string){
  if(!loaded?.revision || !loaded.source_path || loading || busy || review)return;
  const empty=JSON.stringify(['','','','','','','',false,false,false,{}, {action:'close_only'}]);
  const draft=linkedAppDraft.value ?? {form:empty,baseline:empty,editingIndex:null,editingRevision:null,revision:loaded.revision,sourcePath:loaded.source_path,entries:loaded.entries,modes:loaded.modes??[],modeEdits:{}};
  const key=index<0?'new':String(index);
  const held=formsFromDraft(draft)[key];
  if(held){const form=JSON.parse(held.form);form[10]={...form[10],[selected]:value};
   if((draft.editingIndex??'new').toString()===key)linkedAppDraft.value={...draft,form:JSON.stringify(form)};
   else linkedAppDraft.value={...draft,otherForms:{...draft.otherForms,[key]:{...held,form:JSON.stringify(form)}}};return;
  }
  const modeEdits=structuredClone($state.snapshot(draft.modeEdits??{}));
  modeEdits[index]={...modeEdits[index],[selected]:value};
  const original=draft.entries.find(e=>e.index===index)?.modes?.find(m=>m.id===selected)?.action??'inherit';
  if(value===original)delete modeEdits[index][selected];
  if(!Object.keys(modeEdits[index]).length)delete modeEdits[index];
  linkedAppDraft.value=!Object.keys(modeEdits).length && !Object.keys(draft.otherForms??{}).length && draft.form===draft.baseline?null:{...draft,modeEdits};
 }
 function modeChanges(edit: ReturnType<typeof batchFromDraft>[number]) {
  return Object.entries({...edit.entry?.mode_actions,...edit.mode_actions}).flatMap(([id,after])=>{
   const before=entries.find(e=>e.index===edit.index)?.modes?.find(m=>m.id===id)?.action??'inherit';
   return before===after?[]:[{label:modes.find(m=>m.id===id)?.label??id,before:actions.find(a=>a[0]===before)?.[1]??before,after:actions.find(a=>a[0]===after)?.[1]??after}];
  });
 }
 function detailChanges(edit: ReturnType<typeof batchFromDraft>[number]) {
  if(!edit.entry)return [];
  const old=edit.index<0?undefined:entries.find(e=>e.index===edit.index)?.entry;
  const before:Record<string,unknown>=typeof old==='string'?{command:old}:{...old};
  const after:Record<string,unknown>={...edit.entry};
  const labels:Record<string,string>={command:'起動コマンド',label:'表示名',id:'連携ID',if_not_running:'確認するプロセス名',start_on_launch:'VAC起動時に開始',minimized:'最小化で起動',run_as_admin:'管理者権限',working_dir:'作業フォルダー',shutdown:'終了方針',unaa_z_app_id:'連携先アプリID',unaa_z_instance_id:'連携先個体ID'};
  return [...new Set([...Object.keys(before),...Object.keys(after)])].filter(k=>k!=='mode_actions'&&JSON.stringify(before[k])!==JSON.stringify(after[k])).map(k=>({label:labels[k]??k,before:JSON.stringify(before[k])??'未指定',after:JSON.stringify(after[k])??'未指定'}));
 }
 function changes(){try{return linkedAppDraft.value?batchFromDraft(linkedAppDraft.value):[]}catch{return[]}}
 function requestSave(){if(!changed||loading||busy)return;error='';try{if(!linkedAppDraft.value||!(batchFromDraft(linkedAppDraft.value).length+additionsFromDraft(linkedAppDraft.value).length))return;}catch(e){error=String(e);return;}reviewKind='save';review=JSON.stringify(linkedAppDraft.value)}
 async function save(){
  if(!review||busy)return;
  if(review!==JSON.stringify(linkedAppDraft.value)){review=null;error='確認後に下書きが変わりました。確認し直してください。';return;}
  if(reviewKind==='discard'){linkedAppDraft.value=null;review=null;await load();return;}
  const draft=linkedAppDraft.value;if(!draft?.revision)return;
  busy=true;
  try{
   const current=await api.runWithList();
   if(current.source_path!==draft.sourcePath||current.revision!==draft.revision)throw Error('保存先または内容が変わりました。下書きは保持しています。');
   loaded=await api.runWithBatch(batchFromDraft(draft),draft.revision,additionsFromDraft(draft));linkedAppDraft.value=null;review=null;
  }catch(e){error=String(e);review=null}finally{busy=false}
 }
</script>
<section class="vac-panel mt-5 p-4" aria-label="モード別のアプリ設定">
 <h2 class="text-lg font-semibold">モード別のアプリ設定</h2>
 <p class="mt-1 text-xs opacity-70">ここで選ぶのは編集対象。稼働中のモードは切り替えません。保存だけではアプリを起動・終了しません。</p>
 <p class="mt-1 break-all text-xs opacity-60">保存先: {linkedAppDraft.value?.sourcePath ?? loaded?.source_path ?? '読込中'}</p>
 {#if error}<p role="alert" class="my-3 text-sm text-warning-500">{error}</p>{/if}
 {#if loading}<p class="mt-3 text-sm">読み込み中…</p>{:else if !modes.length}<p class="mt-3 text-sm">モードは未定義です。連携アプリの登録と個別操作は利用できます。</p>{:else}
 <div class="my-3 flex flex-wrap gap-2" role="group" aria-label="編集するモード">
 {#each modes as mode}<button class="min-w-0 max-w-full whitespace-normal break-words rounded border px-3 py-2 text-sm" class:border-primary-500={selected===mode.id} aria-pressed={selected===mode.id} disabled={busy||!!review} onclick={()=>selected=mode.id}>{mode.label}</button>{/each}
 </div>
 <fieldset disabled={busy||!!review} class="grid gap-3">
 {#each entries as entry}
 <label class="grid min-w-0 gap-2 rounded border border-surface-200-800 p-3 sm:grid-cols-[minmax(0,1fr)_minmax(120px,200px)]"><span class="break-words">{entry.display_label}</span><select class="min-w-0 rounded border bg-surface-50-950 p-2" aria-label={`${entry.display_label}のモード操作`} value={action(entry.index)} onchange={e=>change(entry.index,e.currentTarget.value)}>{#each actions as [id,label]}<option value={id}>{label}</option>{/each}</select></label>
 {/each}
 {#if !entries.length}<p class="text-sm">連携アプリは未登録です。</p>{/if}
 </fieldset>
 {/if}
 <div class="mt-3 flex flex-wrap gap-2">
 <button class="rounded border px-3 py-2 text-sm" disabled={busy||!!review} onclick={()=>tabNavStore.openFeature('resources','run-with')}>アプリの登録・詳細設定へ</button>
 <button class="rounded border px-3 py-2 text-sm" disabled={!changed||loading||busy||!!review} onclick={requestSave}>変更を確認して保存</button>
 <button class="rounded border px-3 py-2 text-sm" disabled={!changed||busy||!!review} onclick={()=>{reviewKind='discard';review=JSON.stringify(linkedAppDraft.value)}}>変更を破棄…</button>
 </div>
 {#if review}<section class="mt-4 rounded border border-warning-500 p-3" aria-label="モード設定保存の確認">
 <h3 class="font-semibold">{reviewKind==='save'?'共有している下書きをまとめて保存':'共有している未保存の変更をすべて破棄'}</h3>
 {#each changes() as edit}<div class="mt-2 text-sm"><strong>{entries.find(e=>e.index===edit.index)?.display_label}</strong>{#if edit.entry}<p>アプリ詳細の変更:</p>{#each detailChanges(edit) as change}<p class="break-all">{change.label}: {change.before} → {change.after}</p>{/each}{/if}{#each modeChanges(edit) as change}<p>{change.label}: {change.before} → {change.after}</p>{/each}</div>{/each}
 {#if newForm}<div class="mt-3 text-sm"><strong>新規登録: {draftEntry(newForm.form).label||'名称未指定'}</strong>{#each detailChanges({index:-1,effective_id:'new',mode_actions:{},entry:draftEntry(newForm.form)}) as change}<p class="break-all">{change.label}: {change.after}</p>{/each}<p>VAC起動時: {draftEntry(newForm.form).start_on_launch===false?'起動しない':'起動する'}</p>{#each Object.entries(draftEntry(newForm.form).mode_actions??{}) as [id,action]}<p>{modes.find(m=>m.id===id)?.label??id}: {actions.find(a=>a[0]===action)?.[1]??action}</p>{/each}</div>{/if}
 <button class="mt-3 rounded border px-3 py-2 text-sm" disabled={busy} onclick={()=>void save()}>{reviewKind==='save'?'確認して一括保存':'共有下書きを破棄'}</button>
 <button class="ml-2 rounded border px-3 py-2 text-sm" disabled={busy} onclick={()=>review=null}>キャンセル</button>
 </section>{/if}
</section>

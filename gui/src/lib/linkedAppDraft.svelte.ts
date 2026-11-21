import type { RunWithTableDto, RunWithView } from './types';
// In-memory only: scoped to this VAC page, never copied to another connection.
export interface LinkedAppDraft {
 otherForms?: Record<string, {form: string; baseline: string}>;
 modeEdits?: Record<string, Record<string, string>>;
 form: string;
 baseline: string;
 editingIndex: number | null;
 editingRevision: string | null;
 revision: string | null;
 sourcePath: string | null;
 entries: RunWithView[];
 modes: {id: string; label: string; action: string}[];
}
export const linkedAppDraft = $state<{value: LinkedAppDraft | null}>({value:null});

export function draftEntry(form: string): RunWithTableDto {
 const [command, marker, id, label, app, instance, cwd, minimized, admin, launch, modes, shutdown] = JSON.parse(form);
 return {command: command.trim(), ...(marker.trim()?{if_not_running:marker.trim()}:{}), ...(id.trim()?{id:id.trim()}:{}), ...(label.trim()?{label:label.trim()}:{}), ...(app.trim()?{unaa_z_app_id:app.trim()}:{}), ...(instance.trim()?{unaa_z_instance_id:instance.trim()}:{}), ...(cwd.trim()?{working_dir:cwd.trim()}:{}), ...(minimized?{minimized:true}:{}), ...(admin?{run_as_admin:true}:{}), ...(!launch?{start_on_launch:false}:{}), ...(shutdown?{shutdown}:{}), mode_actions:modes};
}
export function formsFromDraft(draft: LinkedAppDraft) {
 const forms={...draft.otherForms};
 if(draft.form!==draft.baseline)forms[draft.editingIndex ?? 'new']={form:draft.form,baseline:draft.baseline};
 return forms;
}
export function additionsFromDraft(draft: LinkedAppDraft) {
 const form=formsFromDraft(draft).new;
 if(!form)return [];
 const entry=draftEntry(form.form);
 if(!entry.command)throw Error("新規登録の起動コマンドを入力してください。");
 return [entry];
}
export function batchFromDraft(draft: LinkedAppDraft) {
 const edits = Object.entries(draft.modeEdits ?? {}).map(([index, mode_actions]) => {
  const item=draft.entries.find(e=>e.index===Number(index));
  if(!item) throw Error("編集対象が見つかりません。");
  return {index:item.index,effective_id:item.effective_id,mode_actions,entry:undefined as RunWithTableDto | undefined};
 });
 for(const [key,form] of Object.entries(formsFromDraft(draft))){
  if(key==='new')continue;
  const item=draft.entries.find(e=>e.index===Number(key));
  if(!item)throw Error("編集中のアプリが見つかりません。");
  const entry=draftEntry(form.form);
  if(!entry.command)throw Error(`${item.display_label}の起動コマンドを入力してください。`);
  const existing=edits.find(e=>e.index===item.index);
  if(existing)existing.entry=entry;
  else edits.push({index:item.index,effective_id:item.effective_id,entry,mode_actions:{}});
 }
 return edits.sort((a,b)=>a.index-b.index);
}
export const modeAppSelection = $state({id: ""});

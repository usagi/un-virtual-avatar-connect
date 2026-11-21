import {perceptionError} from './perceptionError.js';
import type { PerceptionEdit, PerceptionSaveResult, PerceptionView } from './perceptionSetupTypes.ts';

export interface PerceptionTarget { instance: string; profile: string }
export interface PerceptionTransport {
	read(target: Readonly<PerceptionTarget>): Promise<PerceptionView>;
	save(target: Readonly<PerceptionTarget>, revision: string, edit: PerceptionEdit): Promise<PerceptionSaveResult>;
}
export interface PerceptionDraft {
	view: PerceptionView | null;
	pending: Map<string, PerceptionEdit>;
	loading: boolean;
	saving: boolean;
	error: string;
	errorDiagnostic: string;
	errorEditKey: string | null;
	restartRequired: boolean;
	needsRefresh: boolean;
}
const key = (target: PerceptionTarget) => JSON.stringify([target.instance, target.profile]);
export const perceptionEditKey = (edit: PerceptionEdit) => JSON.stringify(edit.kind === 'create' ? ['create'] : edit.kind === 'source'
	? ['source', edit.source_id] : edit.kind === 'history' ? ['history',edit.persona_index]
	: edit.kind === 'source_lifecycle' ? ['source_lifecycle',edit.source_id,edit.restore]
	: edit.kind === 'connection' ? ['connection',edit.source_id]
	: ['assignment', edit.persona_index, ...[...edit.shared_source_ids].sort()]);

/** Transport must bind the instance identity, not consult a mutable selection. */
export class PerceptionDrafts {
	private drafts = new Map<string, PerceptionDraft>();
	private versions = new Map<string, number>();
	private transport: PerceptionTransport;
	constructor(transport: PerceptionTransport) { this.transport = transport; }
	/** Includes drafts hidden by a switch to another VAC or profile. */
	unsettledTargets(): PerceptionTarget[] {
		return [...this.drafts.entries()]
			.filter(([,draft]) => draft.saving || draft.pending.size > 0)
			.map(([id]) => { const [instance,profile] = JSON.parse(id); return {instance,profile}; });
	}
	get(target: PerceptionTarget): PerceptionDraft {
		const id = key(target);
		if (!this.drafts.has(id)) this.drafts.set(id, {view:null,pending:new Map(),loading:false,saving:false,error:'',errorDiagnostic:'',errorEditKey:null,restartRequired:false,needsRefresh:false});
		return this.drafts.get(id)!;
	}
	stage(target: PerceptionTarget, edit: PerceptionEdit) {
		const draft = this.get(target);
		if (!draft.view) throw new Error('設定を読み込んでから編集してください');
		const previous=draft.pending.get(perceptionEditKey(edit));
		const merged=previous?.kind==='source' && edit.kind==='source'
			? {...edit,patch:{...previous.patch,...edit.patch}}
			: previous?.kind==='history' && edit.kind==='history' ? {...previous,...edit} : edit;
		draft.pending.set(perceptionEditKey(edit), structuredClone(merged));
	}
	discard(target: PerceptionTarget, editKey?: string): boolean {
		const draft=this.get(target);
		if(draft.saving) return false;
		if(editKey===undefined) draft.pending.clear(); else draft.pending.delete(editKey);
		draft.error=''; draft.errorDiagnostic=''; draft.errorEditKey=null; return true;
	}
	async refreshAfterExternalSave(target: PerceptionTarget): Promise<boolean> {
		const draft=this.get(target);
		if(draft.pending.size || draft.saving) {
			draft.needsRefresh=true;
			draft.errorEditKey=null;
			draft.errorDiagnostic='';
			draft.error='AI基本設定が保存されました。画面設定の入力は保持しています。保存時は競合を確認します。下書きを取り消す場合は、保存済み設定を再読込してください。';
			return false;
		}
		return this.load(target);
	}
	async load(target: PerceptionTarget): Promise<boolean> {
		const fixed = {...target}, id = key(fixed), draft = this.get(fixed);
		if (draft.pending.size || draft.saving) return false;
		const version = (this.versions.get(id) ?? 0) + 1;
		this.versions.set(id,version); draft.loading=true; draft.error=''; draft.errorDiagnostic=''; draft.errorEditKey=null;
		try {
			const view=await this.transport.read(fixed);
			if (this.versions.get(id)!==version || draft.pending.size || draft.saving) return false;
			draft.view=structuredClone(view); draft.needsRefresh=false; return true;
		} catch(error) {
			if (this.versions.get(id)===version) { const shown=perceptionError(error,'read'); draft.error=shown.message; draft.errorDiagnostic=shown.diagnostic; }
			return false;
		} finally { if(this.versions.get(id)===version) draft.loading=false; }
	}
	async save(target: PerceptionTarget, editKey: string): Promise<boolean> {
		const fixed={...target}, draft=this.get(fixed), edit=draft.pending.get(editKey);
		if (!draft.view || !edit || draft.saving || draft.loading) return false;
		const sent=structuredClone(edit), revision=draft.view.revision;
		draft.saving=true; draft.error=''; draft.errorDiagnostic=''; draft.errorEditKey=null;
		try {
			const result=await this.transport.save(fixed,revision,sent);
			draft.view.revision=result.revision;
			// Effective reasons must be reread, never guessed from a checkbox.
			draft.needsRefresh=true;
			draft.restartRequired ||= result.restart_required;
			// A user may keep typing while a previous edit is saving.
			if (JSON.stringify(draft.pending.get(editKey))===JSON.stringify(sent)) draft.pending.delete(editKey);
			return true;
		} catch(error) { const shown=perceptionError(error,'save'); draft.error=shown.message; draft.errorDiagnostic=shown.diagnostic; draft.errorEditKey=editKey; return false; }
		finally { draft.saving=false; }
	}
}

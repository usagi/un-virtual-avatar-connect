import type {PerceptionTransport,PerceptionTarget} from './perceptionDrafts';
import type {PerceptionView} from './perceptionSetupTypes';

/** Only imported by the design preview. No filesystem, network, or capture. */
export function createPerceptionReviewTransport() {
	const connection={server:'127.0.0.1:4455',secure:false,custom_path_or_query:false,environment_password:false,saved_password:false,local_obs_password:false};
	const data=new Map<string,PerceptionView>();let sequence=1;let conflict=false;
	const key=(t:PerceptionTarget)=>JSON.stringify([t.instance,t.profile]);
	function get(t:PerceptionTarget){
		const id=key(t);const fresh=!data.has(id);
		if(!data.has(id)) data.set(id,{revision:`r${sequence++}`,archived_sources:[],history:[{persona_index:0,persona_id:'host',display_name:'共演者',turn_management_enabled:true,visual_history_enabled:false,past_screen_image_enabled:false},{persona_index:1,persona_id:'guide',display_name:'解説役',turn_management_enabled:false,visual_history_enabled:false,past_screen_image_enabled:true}],sources:[{connection:{...connection},id:'game',scene_name:'ゲーム',source_name:'ゲームキャプチャ',enabled:true,refresh_before_reply:true,active_modes:['streaming'],channel:'obs:game',shared_source_ids:['game'],observers:[{persona_index:0,persona_id:'host',display_name:'共演者',enabled:true,rule:'all'},{persona_index:1,persona_id:'guide',display_name:'解説役',enabled:true,rule:'excluded'}]}]});
		if(fresh){
			const value=data.get(id)!;const game=value.sources[0];game.shared_source_ids=['game','game-detail'];
			value.sources.push({...structuredClone(game),id:'game-detail',source_name:'ゲーム詳細'});
			value.sources.push({...structuredClone(game),id:'board',scene_name:'解説',source_name:'資料',channel:'obs:board',shared_source_ids:['board'],observers:game.observers.map(o=>({...o,rule:'all'}))});
		}
		return data.get(id)!;
	}
	const transport:PerceptionTransport={
		read:async target=>structuredClone(get(target)),
		save:async(target,revision,edit)=>{
			const value=get(target);
			if(conflict){conflict=false;value.revision=`r${sequence++}`;}
			if(value.revision!==revision) throw new Error('別の操作で設定が変わったため保存していない（409）');
			if(edit.kind==='create'){
				const input=edit.source;
				if(!input.id || !input.scene_name.trim() || !input.source_name.trim())throw new Error('ID・シーン名・ソース名が必要');
				if([...value.sources,...value.archived_sources].some(s=>s.id===input.id))throw new Error('同じ取得元IDが存在する');
				value.sources.push({connection:{...connection},id:input.id,scene_name:input.scene_name,source_name:input.source_name,channel:`obs:source:${input.id}`,enabled:false,refresh_before_reply:false,active_modes:['streaming'],shared_source_ids:[input.id],observers:value.history.map(h=>({persona_index:h.persona_index,persona_id:h.persona_id,display_name:h.display_name,enabled:true,rule:'all'}))});
				value.revision=`r${sequence++}`;return {revision:value.revision,restart_required:true};
			}
			if(edit.kind==='source_lifecycle'){
				const from=edit.restore ? value.archived_sources : value.sources;
				const to=edit.restore ? value.sources : value.archived_sources;
				const index=from.findIndex(s=>s.id===edit.source_id);if(index<0)throw new Error('取得元が存在しない');
				to.push(...from.splice(index,1));
				for(const s of [...value.sources,...value.archived_sources])s.shared_source_ids=value.sources.filter(v=>v.channel===s.channel).map(v=>v.id);
				value.revision=`r${sequence++}`;return {revision:value.revision,restart_required:true};
			}
			if(edit.kind==='connection'){
				const item=value.sources.find(s=>s.id===edit.source_id);if(!item)throw new Error('取得元が存在しない');
				if(edit.url && !/^wss?:\/\//.test(edit.url))throw new Error('ws/wssの接続先が必要');
				if(edit.credential?.kind==='password' && !edit.credential.value)throw new Error('空のパスワードは保存できない');
				if(edit.url){const u=new URL(edit.url);item.connection.server=u.host;item.connection.secure=u.protocol==='wss:';item.connection.custom_path_or_query=u.pathname!=='/' || !!u.search;}
				if(edit.credential){item.connection.environment_password=edit.credential.kind==='environment';item.connection.saved_password=edit.credential.kind==='password';item.connection.local_obs_password=edit.credential.kind==='local_obs';}
				value.revision=`r${sequence++}`;return {revision:value.revision,restart_required:true};
			}
			if(edit.kind==='history'){
				const item=value.history.find(h=>h.persona_index===edit.persona_index);if(!item)throw new Error('共演者が存在しない');
				if(edit.visual_history_enabled!==undefined)item.visual_history_enabled=edit.visual_history_enabled;
				if(edit.past_screen_image_enabled!==undefined)item.past_screen_image_enabled=edit.past_screen_image_enabled;
				value.revision=`r${sequence++}`;return {revision:value.revision,restart_required:true};
			}
			const source=value.sources.find(s=>s.id===edit.source_id);if(!source)throw new Error('取得元が存在しない');
			if(edit.kind==='source') {
				if(edit.patch.scene_name?.trim()==='' || edit.patch.source_name?.trim()==='')throw new Error('シーン名とソース名が必要');
				Object.assign(source,structuredClone(edit.patch));
			} else {
				if(JSON.stringify([...edit.shared_source_ids].sort())!==JSON.stringify([...source.shared_source_ids].sort()))throw new Error('共有範囲が不一致');
				for(const s of value.sources.filter(s=>s.channel===source.channel)) {
					const observer=s.observers.find(o=>o.persona_index===edit.persona_index);if(!observer)throw new Error('共演者が存在しない');
					observer.rule=edit.observe ? 'all' : 'excluded';
				}
			}
			value.revision=`r${sequence++}`;
			return {revision:value.revision,restart_required:true};
		},
	};
	return {transport,conflictNextSave:()=>{conflict=true;}};
}

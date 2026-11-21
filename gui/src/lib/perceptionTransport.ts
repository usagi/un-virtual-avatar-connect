import type {PerceptionTransport,PerceptionTarget} from './perceptionDrafts';
import type {PerceptionView,PerceptionEdit,PerceptionSaveResult} from './perceptionSetupTypes';

export function createPerceptionTransport(instance:string, client:{
	desktopProfilePerception(id:string):Promise<PerceptionView>;
	saveDesktopProfilePerception(id:string,revision:string,edit:PerceptionEdit):Promise<PerceptionSaveResult>;
}):PerceptionTransport {
	function profile(target:Readonly<PerceptionTarget>){
		if(target.instance!==instance || !target.profile) throw new Error('画面設定の保存先が接続中のVACと一致しません');
		return target.profile;
	}
	return {
		read:async target=>client.desktopProfilePerception(profile(target)),
		save:async(target,revision,edit)=>client.saveDesktopProfilePerception(profile(target),revision,edit),
	};
}

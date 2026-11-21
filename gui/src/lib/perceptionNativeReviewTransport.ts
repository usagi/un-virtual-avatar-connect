import {api} from './api';
import type {PerceptionTransport,PerceptionTarget} from './perceptionDrafts';

/** Preview-only: refuse production servers or a replaced isolated store. */
export function createNativePerceptionReviewTransport():PerceptionTransport {
	let root:string|undefined;
	async function verify(target:PerceptionTarget){
		if(target.instance!=='隔離VAC')throw new Error('実保存の確認対象が一致しない');
		const response=await fetch('/review-identity',{cache:'no-store'});
		if(!response.ok)throw new Error('隔離保存先を確認できない');
		const identity=await response.json();
		if(identity.kind!=='unvac-isolated-perception-review' || identity.devices!==false || typeof identity.root!=='string' || !identity.root)throw new Error('隔離保存先の識別情報が不正');
		if(root!==undefined && root!==identity.root)throw new Error('保存先が切り替わったため操作を中止');
		root=identity.root;
	}
	return {
		read:async target=>{await verify(target);return api.desktopProfilePerception(target.profile);},
		save:async(target,revision,edit)=>{await verify(target);return api.saveDesktopProfilePerception(target.profile,revision,edit);},
	};
}

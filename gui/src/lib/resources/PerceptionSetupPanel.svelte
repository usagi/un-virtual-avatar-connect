<script lang="ts">
	import {onMount} from 'svelte';
	import {api} from '../api';
	import type {DesktopProfilesResponse} from '../types';
	import {createPerceptionTransport} from '../perceptionTransport';
	import PerceptionSavedEditor from './PerceptionSavedEditor.svelte';
	import RestartDialog from '../RestartDialog.svelte';
	import {confSyncStore} from '../confSync.svelte';
	import type {PerceptionTarget} from '../perceptionDrafts';
	let { externalSave = null }: {externalSave?: PerceptionTarget | null} = $props();
	const instance=window.location.origin;
	const transport=createPerceptionTransport(instance,api);
	let profiles=$state<DesktopProfilesResponse|null>(null);
	let profile=$state('');
	let error=$state('');
	let loading=$state(true);
	let restartOpen=$state(false);
	let savedCurrent=$state(false);
	function saved(target:PerceptionTarget,restartRequired:boolean){
		if(restartRequired && target.instance===instance && target.profile===profiles?.current_profile_id){
			savedCurrent=true;confSyncStore.markSavedRequiringRestart();
		}
	}
	$effect(()=>{if(savedCurrent && confSyncStore.phase==='synced')savedCurrent=false;});
	async function load(){
		loading=true;error='';
		try{
			const next=await api.desktopProfiles();
			profiles=next;
			// Never substitute the next-launch default for an unknown running profile.
			if(!profile && next.current_profile_id && next.profiles.some(p=>p.id===next.current_profile_id)) profile=next.current_profile_id;
		}catch(e){error=e instanceof Error?e.message:String(e);}
		finally{loading=false;}
	}
	onMount(()=>{void load();});
</script>

<RestartDialog bind:open={restartOpen} currentOnly />
<section class="vac-panel" aria-label="共演者に見せる画面">
	<header class="vac-panel-header px-4 py-3">
		<h3 class="text-sm font-semibold">共演者に見せる画面</h3>
		<p class="mt-1 text-xs opacity-70">共演者を選び、OBSのどの画面を見せるか設定する。ここで保存しても撮影は開始しない。</p>
	</header>
	<div class="p-4">
		{#if loading}<p role="status">保存先を確認中</p>
		{:else if error}<p role="alert">{error}</p><button onclick={load}>保存先の確認を再試行</button>
		{:else if profiles?.managed_mode}
			<p>接続中のVAC：{instance}</p>
			<label>編集するプロファイル
				<select aria-label="編集するプロファイル" bind:value={profile}>
					<option value="" disabled>保存先を選択</option>
					{#each profiles.profiles as item}
						<option value={item.id}>{item.label} · {item.id}{item.id===profiles.current_profile_id?' · 起動中':''}{item.id===profiles.active_profile_id?' · 次回既定':''}</option>
					{/each}
				</select>
			</label>
			{#if profile}<PerceptionSavedEditor {externalSave} target={{instance,profile}} {transport} layout="partner" runningProfileId={profiles.current_profile_id ?? null} onsaved={saved} />{/if}
			{#if savedCurrent}
				<p>起動中のプロファイル「{profiles.current_profile_id}」に保存した画面設定は、再起動待ち。</p>
				<button onclick={()=>restartOpen=true}>起動中のVACへの反映を確認</button>
			{/if}
		{:else}
			<p>外部設定ファイルで起動中。画面設定は既存の設定ファイルエディターから編集できる。このフォームでは管理プロファイルの設定を扱う。</p>
		{/if}
	</div>
</section>

<style>
	select{display:block;width:100%;margin-top:6px;padding:8px;background:var(--color-surface-900,#18232d);color:#e8f2f3;border:1px solid #63808e;border-radius:8px}label{display:block;margin-top:12px}p{font-size:.875rem;line-height:1.6}
</style>

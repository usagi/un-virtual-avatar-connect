<script lang="ts">
	import {onMount,untrack} from 'svelte';
	import {tabNavStore} from '../tabs.svelte';
	import {PerceptionDrafts,perceptionEditKey,type PerceptionTarget,type PerceptionTransport,type PerceptionDraft} from '../perceptionDrafts';
	import type {PerceptionSource,PerceptionSourcePatch,PerceptionSourceCreate,ObservationRule,PerceptionEdit} from '../perceptionSetupTypes';
	let {target,transport,layout='sources',runningProfileId,onsaved,externalSave=null}: {target:PerceptionTarget;transport:PerceptionTransport;externalSave?:PerceptionTarget|null;layout?:'partner'|'sources';runningProfileId?:string|null;onsaved?:(target:PerceptionTarget,restartRequired:boolean)=>void}=$props();
	let personaIndex=$state(0);
	// Transport is immutable for this component's lifetime; each request has a target.
	const store=new PerceptionDrafts(untrack(()=>transport));
	let draftState=$state<PerceptionDraft>(structuredClone(store.get(untrack(()=>target))));
	let sourceId=$state('');
	let sourceSettingsOpen=$state(false);
	let leaveError=$state('');
	function canLeave(){
		const held=store.unsettledTargets();
		if(!held.length){leaveError='';return true;}
		leaveError=`画面設定に未保存の入力または保存中の処理があるため、移動していない。対象：${held.map(t=>`${t.instance} / ${t.profile}`).join('、')}。対象を選び、保存または下書きの取り消しを行ってください。`;
		return false;
	}
	function beforeUnload(event:BeforeUnloadEvent){
		if(!canLeave()){event.preventDefault();event.returnValue='';}
	}
	onMount(()=>tabNavStore.guardLeaving(canLeave));
	const targetIdentity=$derived(JSON.stringify(target));
	let lifecycle=$state<{source:PerceptionSource;restore:boolean;target:string}|null>(null);
	const lifecycleBusy=$derived(draftState.loading || draftState.saving || draftState.needsRefresh || draftState.pending.size>0);
	function proposeLifecycle(item:PerceptionSource,restore:boolean){if(!lifecycleBusy)lifecycle={source:$state.snapshot(item),restore,target:targetIdentity};}
	async function confirmLifecycle(){
		if(!lifecycle || lifecycleBusy || lifecycle.target!==JSON.stringify(target))return;
		const item=lifecycle.source;
		const edit:PerceptionEdit={kind:'source_lifecycle',source_id:item.id,restore:lifecycle.restore,shared_source_ids:[...item.shared_source_ids],observer_indices:item.observers.filter(o=>o.rule!==null && !['excluded','unobserved'].includes(o.rule)).map(o=>o.persona_index)};
		store.stage(target,edit);lifecycle=null;sync();await save(perceptionEditKey(edit));
	}
	const source=$derived(draftState.view?.sources.find(s=>s.id===sourceId) ?? draftState.view?.sources[0]);
	const persona=$derived(draftState.view?.history.find(h=>h.persona_index===personaIndex) ?? draftState.view?.history[0]);
	const visibleObservers=$derived((source?.observers ?? []).filter(o=>layout==='sources' || o.persona_index===persona?.persona_index));
	const visibleHistory=$derived((draftState.view?.history ?? []).filter(h=>layout==='sources' || h.persona_index===persona?.persona_index));
	const slot=$derived(source ? perceptionEditKey({kind:'source',source_id:source.id,patch:{}}) : '');
	const pending=$derived(draftState.pending.get(slot));
	const patch=$derived(pending?.kind==='source' ? pending.patch : {});
	const connectionSlot=$derived(source ? perceptionEditKey({kind:'connection',source_id:source.id,url:null,credential:null}) : '');
	const connectionDraft=$derived(draftState.pending.get(connectionSlot));
	const connection=$derived(connectionDraft?.kind==='connection' ? connectionDraft : {url:null,credential:null});
	function stageConnection(value:Partial<Extract<PerceptionEdit,{kind:'connection'}>>){if(source){store.stage(target,{kind:'connection',source_id:source.id,...$state.snapshot(connection),...value});sync();}}
	const blankSource:PerceptionSourceCreate={id:'',scene_name:'',source_name:'',url:'ws://127.0.0.1:4455',password_env:null,use_local_obs_config_password:false};
	const createSlot=perceptionEditKey({kind:'create',source:blankSource});
	const creating=$derived(draftState.pending.get(createSlot));
	const newSource=$derived(creating?.kind==='create' ? creating.source : blankSource);
	const reasons:Record<ObservationRule,string>={excluded:'明示的に除外',all:'すべてのチャンネルを観測',own_utterance:'自身の発話チャンネル',trigger:'発話トリガーとして観測',additional:'追加の文脈として観測',unobserved:'観測対象外'};
	function sync(){draftState=structuredClone(store.get(target));if(!store.unsettledTargets().length)leaveError='';}
	async function load(fixed:PerceptionTarget){const work=store.load(fixed);sync();await work;sync();}
	$effect(()=>{const changed=externalSave;if(changed)untrack(()=>{const work=store.refreshAfterExternalSave(changed);sync();void work.then(()=>sync());});});
	$effect(()=>{const fixed={...target};untrack(()=>{sync();if(!store.get(fixed).view) void load(fixed);});});
	$effect(()=>{targetIdentity;sourceId;untrack(()=>{lifecycle=null;});});
	function stage(value:PerceptionSourcePatch){if(source){store.stage(target,{kind:'source',source_id:source.id,patch:value});sync();}}
	function stageCreate(value:Partial<PerceptionSourceCreate>){store.stage(target,{kind:'create',source:{...newSource,...value}});sync();}
	function assignment(index:number,observe:boolean):PerceptionEdit{return {kind:'assignment',source_id:source!.id,persona_index:index,shared_source_ids:[...source!.shared_source_ids],observe};}
	function stageAssignment(index:number,observe:boolean){store.stage(target,assignment(index,observe));sync();}
	function stageHistory(index:number,patch:{visual_history_enabled?:boolean;past_screen_image_enabled?:boolean}){store.stage(target,{kind:'history',persona_index:index,...patch});sync();}
	async function save(key:string){const fixed={...target};const work=store.save(fixed,key);sync();const ok=await work;
		if(ok){onsaved?.(fixed,store.get(fixed).restartRequired);if(store.get(fixed).pending.size===0) await store.load(fixed);}sync();}
	async function discard(){const fixed={...target};if(store.discard(fixed)){await load(fixed);}sync();}
</script>

{#snippet saveError(key:string)}
    {#if draftState.error && draftState.errorEditKey===key}
        <p class="save-error" role="note" aria-label="この操作の保存エラー">{draftState.error}　入力は保持している。内容を確認して再保存してください。</p>
    {/if}
{/snippet}

<svelte:window onbeforeunload={beforeUnload} />
<section aria-label="保存経路の確認">
	<h3>保存済み設定の編集</h3>
	{#if leaveError}<p role="alert">{leaveError}</p>{/if}
	<p>対象：{target.instance} / {target.profile}　下書き {draftState.pending.size} 件</p>
	{#if layout==='partner' && persona}
		<nav aria-label="共演者から選ぶ" class="overview">
			{#each draftState.view?.history ?? [] as item}<button aria-pressed={persona?.persona_index===item.persona_index} onclick={()=>personaIndex=item.persona_index}>{item.display_name ?? item.persona_id ?? `共演者 ${item.persona_index+1}`}</button>{/each}
		</nav>
		<p>選択中の共演者の観測割当と画面履歴を編集する。取得元の条件は共有設定。</p>
	{:else if layout==='sources'}
		<nav aria-label="取得元から選ぶ" class="overview">
			{#each draftState.view?.sources ?? [] as item}<button aria-pressed={source?.id===item.id} onclick={()=>sourceId=item.id}>{item.source_name} · {item.id}<small>{item.observers.filter(o=>o.rule!==null && !['excluded','unobserved'].includes(o.rule)).map(o=>o.display_name ?? o.persona_id ?? `共演者 ${o.persona_index+1}`).join('、') || '観測する共演者なし'}</small></button>{/each}
		</nav>
		<p>取得元を選ぶと、全共演者の観測割当を一覧で編集できる。</p>
	{/if}
	{#if draftState.loading}<p role="status">設定を読込中</p>{/if}
	{#if draftState.error}
        <p role="alert">{draftState.error}{draftState.pending.size ? '　入力は保持している。' : ''}</p>
        {#if draftState.errorDiagnostic && draftState.errorDiagnostic!==draftState.error}
            <details><summary>診断情報</summary><pre class="error-diagnostic">{draftState.errorDiagnostic}</pre></details>
        {/if}
    {/if}
	{#if draftState.view && !draftState.loading && layout==='partner' && !persona}
		<div class="setup-guidance" role="note" aria-label="共演者が未設定の場合">
			<p><strong>このプロファイルには共演者が登録されていない。</strong></p>
			<p>画面を見て応答するには、まず編集対象のプロファイルにAIパートナーを設定する。設定を保存した後、この欄の「保存済み設定を再読込」で共演者を読み込める。</p>
			<p>OBSの取得元だけを先に準備することもできる。AIを使わない配信支援では、この設定は不要。</p>
		</div>
	{/if}
	{#each [...draftState.pending.entries()].filter(([,edit])=>edit.kind==='source_lifecycle') as [key,edit]}
		{#if edit.kind==='source_lifecycle'}
			<p>{edit.source_id} の{edit.restore ? '復元' : '削除・保管'}は未完了。対象：{target.instance} / {target.profile}。</p>
			<button disabled={draftState.saving || draftState.loading} onclick={()=>save(key)}>{edit.source_id} の{edit.restore ? '復元' : '削除・保管'}を再試行</button>
		{@render saveError(key)}
		{/if}
	{/each}
	{#if draftState.restartRequired}
		<p>{runningProfileId===undefined ? '保存済み。実行中への反映には再起動が必要。' : runningProfileId===target.profile ? '保存済み。このVACを再起動すると反映される。' : `保存済み。プロファイル「${target.profile}」で起動すると反映される。現在稼働中の設定は変更していない。`}</p>
	{/if}
	{#if draftState.needsRefresh}<p>保存後の観測理由は未確認。下書きを保存または取り消して再読込する。</p>{/if}
	{#if source}
		<label>保存先の取得元<select aria-label="保存先の取得元" value={source.id} onchange={e=>sourceId=e.currentTarget.value}>{#each draftState.view?.sources ?? [] as item}<option value={item.id}>{item.source_name} · {item.id}</option>{/each}</select></label>
		<details class="settings" aria-label="取得条件の編集" bind:open={sourceSettingsOpen}>
			<summary>取得条件 · {source.scene_name} / {source.source_name} · {source.enabled ? '有効' : '無効'}{pending ? '（未保存）' : ''}</summary>
		<fieldset disabled={draftState.loading || draftState.needsRefresh}>
			<legend>取得元 {source.id}</legend>
			<label><input type="checkbox" aria-label="保存する取得元を有効にする" checked={patch.enabled ?? source.enabled} onchange={e=>stage({enabled:e.currentTarget.checked})}/>この取得元を有効にする</label>
			<label>保存するシーン名<input aria-label="保存するシーン名" value={patch.scene_name ?? source.scene_name} oninput={e=>stage({scene_name:e.currentTarget.value})} /></label>
			<label>保存するソース名<input value={patch.source_name ?? source.source_name} oninput={e=>stage({source_name:e.currentTarget.value})} /></label>
			<label><input type="checkbox" checked={patch.refresh_before_reply ?? source.refresh_before_reply} onchange={e=>stage({refresh_before_reply:e.currentTarget.checked})} />返答直前に更新する</label>
			<label>取得するモードID<input value={(patch.active_modes ?? source.active_modes).join(', ')} oninput={e=>stage({active_modes:e.currentTarget.value.trim()==='' ? [] : e.currentTarget.value.split(',').map(v=>v.trim())})}/></label>
			<p>モードIDをカンマで区切る。空欄は全モード。保存だけでは実行中の取得範囲は変更しない。</p>
		</fieldset>
		<button disabled={!pending || draftState.saving || draftState.loading} onclick={()=>save(slot)}>{draftState.saving ? '保存中' : 'この取得元の変更を保存'}</button>
		{@render saveError(slot)}
		</details>
		<p>同じ観測先を共有：{source.shared_source_ids.join('、')}</p>
		<p>割当の変更は上記の取得元すべてに共通。発話トリガーの設定は保持する。</p>
		<ul>{#each visibleObservers as observer}
			{@const name=observer.display_name ?? observer.persona_id ?? `共演者 ${observer.persona_index+1}`}
			{@const assignmentSlot=perceptionEditKey(assignment(observer.persona_index,true))}
			{@const draft=draftState.pending.get(assignmentSlot)}
			<li>{name}：保存済みの理由「{observer.rule ? reasons[observer.rule] : '出力先が未設定'}」{observer.enabled ? '' : '（共演者は無効）'}
				<label><input type="checkbox" aria-label={`${name} にこの画面を見せる`} disabled={observer.rule===null || draftState.loading || draftState.needsRefresh}
					checked={draft?.kind==='assignment' ? draft.observe : observer.rule!==null && !['excluded','unobserved'].includes(observer.rule)}
					onchange={e=>stageAssignment(observer.persona_index,e.currentTarget.checked)} />この画面を見せる{draft ? '（下書き）' : ''}</label>
				<button disabled={!draft || draftState.saving || draftState.loading} onclick={()=>save(assignmentSlot)}>{name} の観測設定を保存</button>
		{@render saveError(assignmentSlot)}
			</li>
		{/each}</ul>

		<button disabled={lifecycleBusy} onclick={()=>proposeLifecycle(source,false)}>この取得元を削除して保管</button>
		<details><summary>OBS接続・認証を変更</summary>
			<p aria-label="保存済みOBS接続先">保存済みサーバー：{source.connection.server ?? 'URLを解釈できない'}{source.connection.secure ? '（暗号化接続）' : ''}</p>
			{#if source.connection.custom_path_or_query}<p>接続先に追加のパス・パラメーター等がある。ここには表示せず、接続先を置換しなければ保持する。</p>{/if}
			<p aria-label="保存済みOBS認証">保存済みの認証参照：{[source.connection.environment_password ? '環境変数' : '',source.connection.saved_password ? '保存したパスワード' : '',source.connection.local_obs_password ? 'この端末のOBS設定' : ''].filter(Boolean).join(' → ') || 'なし'}。接続の成否は未確認。</p>
			{#if [source.connection.environment_password,source.connection.saved_password,source.connection.local_obs_password].filter(Boolean).length>1}<p>左から優先して空でない値を使う。方式を置換すると、この予備の参照も外れる。</p>{/if}
			<p>空欄の接続先と「現在の認証を維持」は保存済みの設定を保持する。保存しても接続・撮影は始まらず、反映には再起動が必要。</p>
			<fieldset disabled={draftState.loading || draftState.saving || draftState.needsRefresh}>
				<label>置き換えるOBS接続先<input value={connection.url ?? ''} placeholder="維持する場合は空欄" oninput={e=>stageConnection({url:e.currentTarget.value || null})}/></label>
				<label>認証の変更<select aria-label="認証の変更" value={connection.credential?.kind ?? 'keep'} onchange={e=>stageConnection({credential:e.currentTarget.value==='keep' ? null : e.currentTarget.value==='environment' ? {kind:'environment',name:''} : e.currentTarget.value==='password' ? {kind:'password',value:''} : e.currentTarget.value==='local_obs' ? {kind:'local_obs'} : {kind:'none'}})}>
					<option value="keep">現在の認証を維持</option><option value="local_obs">この端末のOBS設定を読む</option><option value="password">パスワードを置換</option><option value="environment">環境変数を指定</option><option value="none">認証なしに置換</option>
				</select></label>
				{#if connection.credential?.kind==='password'}
					<label>新しいOBSパスワード<input type="password" autocomplete="new-password" value={connection.credential.value} oninput={e=>stageConnection({credential:{kind:'password',value:e.currentTarget.value}})}/></label>
					<p>入力した値はこのプロファイルの設定ファイルに保存される。保存済みパスワードは画面へ読み戻さない。</p>
				{:else if connection.credential?.kind==='environment'}
					<label>新しいOBSパスワード環境変数名<input value={connection.credential.name} oninput={e=>stageConnection({credential:{kind:'environment',name:e.currentTarget.value}})}/></label>
				{/if}
				{#if connection.credential}<p>認証方式を置換すると、この取得元の以前のパスワード・環境変数・OBS設定参照を外す。他の取得元は変更しない。</p>{/if}
			</fieldset>
			<button disabled={!connectionDraft || draftState.saving || draftState.loading} onclick={()=>save(connectionSlot)}>この取得元の接続設定を保存</button>
		{@render saveError(connectionSlot)}
		</details>
	{:else if draftState.view && !draftState.loading}
		<p>取得元が登録されていない。「新しい取得元を追加」で、見せたいOBSのシーン名とソース名を指定する。</p>
		{#if layout==='partner' && persona}<p>追加後に「この画面を見せる」を確認し、取得元を有効にして保存する。</p>{/if}
	{/if}
	{#if draftState.view?.archived_sources.length}
		<details><summary>削除した取得元を復元</summary>
			{#each draftState.view.archived_sources as item}
				<p>{item.scene_name} / {item.source_name} · {item.id}</p>
				<button disabled={lifecycleBusy} onclick={()=>proposeLifecycle(item,true)}>{item.id} を復元</button>
			{/each}
		</details>
	{/if}
	{#if lifecycle}
		<section aria-label="取得元の削除・復元確認">
			<p>{target.instance} / {target.profile} の {lifecycle.source.id} を{lifecycle.restore ? '復元' : '削除して保管'}する。</p>
			<p>シーン：{lifecycle.source.scene_name} ／ ソース：{lifecycle.source.source_name}</p>
			<p>観測設定のある共演者：{lifecycle.source.observers.filter(o=>o.rule!==null && !['excluded','unobserved'].includes(o.rule)).map(o=>o.display_name ?? o.persona_id ?? `共演者 ${o.persona_index+1}`).join('、') || 'なし'}</p>
			<p>同じ観測先の取得元：{lifecycle.source.shared_source_ids.join('、') || 'なし'}。他の取得元と共演者の観測ルールは保持する。</p>
			<p>{lifecycle.restore ? `元の接続・取得条件を復元する。有効状態は「${lifecycle.source.enabled ? '有効' : '無効'}」。` : '接続情報と取得条件を保管し、後から復元できる。'}実行中への反映には再起動が必要。</p>
			<button disabled={lifecycleBusy} onclick={confirmLifecycle}>{lifecycle.restore ? '復元を確定' : '削除して保管を確定'}</button>
			<button onclick={()=>lifecycle=null}>取り消す</button>
		</section>
	{/if}
	<details><summary>新しい取得元を追加</summary>
		<fieldset disabled={!draftState.view || draftState.loading || draftState.needsRefresh}>
			<legend>追加する画面</legend>
			<label>新しい取得元ID<input value={newSource.id} oninput={e=>stageCreate({id:e.currentTarget.value})}/></label>
			<label>追加するOBS接続先<input value={newSource.url} oninput={e=>stageCreate({url:e.currentTarget.value})}/></label>
			<label>追加するシーン名<input value={newSource.scene_name} oninput={e=>stageCreate({scene_name:e.currentTarget.value})}/></label>
			<label>追加するソース名<input value={newSource.source_name} oninput={e=>stageCreate({source_name:e.currentTarget.value})}/></label>
			<label>OBSパスワードの環境変数名（任意）<input value={newSource.password_env ?? ''} oninput={e=>stageCreate({password_env:e.currentTarget.value || null})}/></label>
			<label><input type="checkbox" checked={newSource.use_local_obs_config_password} onchange={e=>stageCreate({use_local_obs_config_password:e.currentTarget.checked})}/>この端末のOBS設定からパスワードを読む</label>
		</fieldset>
		<p>追加時は無効・配信モードのみ。撮影や接続は始めない。「全チャンネルを観測」の共演者は、有効化するとこの画面も対象になる。</p>
		<button disabled={!creating || draftState.saving || draftState.loading} onclick={()=>save(createSlot)}>取得元を無効状態で追加</button>
		{@render saveError(createSlot)}
	</details>
	{#each visibleHistory as history}
		{@const name=history.display_name ?? history.persona_id ?? `共演者 ${history.persona_index+1}`}
		{@const historySlot=perceptionEditKey({kind:'history',persona_index:history.persona_index})}
		{@const historyDraft=draftState.pending.get(historySlot)}
		{@const notes=historyDraft?.kind==='history' ? historyDraft.visual_history_enabled ?? history.visual_history_enabled : history.visual_history_enabled}
		{@const image=historyDraft?.kind==='history' ? historyDraft.past_screen_image_enabled ?? history.past_screen_image_enabled : history.past_screen_image_enabled}
		<details class="settings" aria-label={`${name} の履歴編集`}>
			<summary>{name} の画面履歴 · {notes ? 'メモ有効' : 'メモ無効'}{historyDraft ? '（未保存）' : ''}</summary>
		<fieldset disabled={draftState.loading || draftState.needsRefresh}>
			<legend>{name} の画面履歴</legend>
			<label><input type="checkbox" aria-label={`${name} の画像メモを残す`} checked={notes} onchange={e=>stageHistory(history.persona_index,{visual_history_enabled:e.currentTarget.checked})} />画像から読んだ短いメモを残す</label>
			<label><input type="checkbox" aria-label={`${name} の過去画像を追加する`} checked={image} disabled={!notes} onchange={e=>stageHistory(history.persona_index,{past_screen_image_enabled:e.currentTarget.checked})} />明示的な過去画面への言及時だけ直前画像を追加</label>
			{#if !history.turn_management_enabled}<p>この共演者の発話管理が無効なため、履歴機能は現在動作しない。ここで保存しても発話方式は変更しない。</p>{/if}
			{#if !notes && image}<p>過去画像を追加する設定値は保持中。画像メモが無効な間は動作しない。</p>{/if}
		</fieldset>
		<button disabled={!historyDraft || draftState.saving || draftState.loading} onclick={()=>save(historySlot)}>{name} の履歴設定を保存</button>
		{@render saveError(historySlot)}
		</details>
	{/each}
	{#if draftState.view?.history.length}<p>過去画像は、現在の入力画像が1枚で、明示的に過去画面を参照した場合のみ。同じ取得元の60秒以内の画像を最大1枚追加し、入力料金や待ち時間が増える場合がある。</p>{/if}
	<button disabled={draftState.saving || draftState.loading || draftState.pending.size>0} onclick={()=>load({...target})}>保存済み設定を再読込</button>
	<button disabled={draftState.saving || draftState.loading || draftState.pending.size===0} onclick={discard}>この対象の下書きを取り消して再読込</button>
</section>

<style>
	.error-diagnostic{white-space:pre-wrap;overflow-wrap:anywhere;font-size:.8rem;}
	.save-error{color:#ffbcad;border-left:3px solid currentColor;padding:8px 12px;margin:8px 0;}
	details.settings{border:1px solid #405963;border-radius:10px;margin:12px 0;padding:10px}summary{cursor:pointer;line-height:1.6}summary:focus-visible{outline:2px solid #9aebd4;outline-offset:4px}
	.overview{display:flex;flex-wrap:wrap;gap:8px}.overview button{flex:1;min-width:150px;text-align:left}.overview small{display:block;margin-top:6px;color:#c0d3da}.overview [aria-pressed=true]{border-color:#9aebd4;background:#284a4c;box-shadow:inset 0 0 0 1px #9aebd4}
	section{padding:16px;border:1px solid #63808e;border-radius:12px;margin-top:16px;background:#18232d;color:#e8f2f3;}label{display:block;margin:10px 0;}input:not([type=checkbox]),select{display:block;width:100%;background:#101a22;color:inherit;padding:8px;border:1px solid #63808e;border-radius:8px;}button{margin:8px 8px 0 0;padding:8px;border:1px solid #759da8;border-radius:8px;}p,li{font-size:.875rem;line-height:1.6;}[role=alert]{color:#ffbcad;}
</style>

<script lang="ts">
    import { onMount } from "svelte";
    import { api } from "../../api";
    import { ControlApiError } from "../../types";
    import { speechRouteView, type SpeechRouteView } from "./speechRouteView";
    import { speechConnectionPlan } from "./speechConnectionPlan";
    import { prepareConnectionPreview, connectionInputKey, type DictionaryConnectionInput, type DictionaryConnectionPreview } from "./dictionaryConnection";
    import type { DictionaryConnectionSaved, DictionaryConnectionRecovery } from "./dictionaryConnection";
    import { DictionarySetupReview, prepareDictionarySetupReview, type DictionarySetupInput, type DictionarySetupSaved } from "./dictionarySetup";

    let { comparison = true }: { comparison?: boolean } = $props();

    type Draft = { key: string; label: string; mode: string; file: string };
    let target = $state<{source_path:string;revision:string}|null>(null);
    let drafts = $state<Record<string,Draft>>({});
    let layout = $state('all');
    let step = $state(0);
    let busy = $state(false);
    let error = $state('');
    let review = $state<DictionarySetupReview|null>(null);
    let externalConfirmed = $state(false);
    let receipt = $state<DictionarySetupSaved|null>(null);
    let routes = $state<SpeechRouteView|null>(null);
    let routeSelection = $state('');
    let failurePolicy = $state('');
    let savedConnection = $state<{input:DictionaryConnectionInput;preview:DictionaryConnectionPreview}|null>(null);
    let sharedConfirmed = $state(false);
    let connectionReceipt = $state<DictionaryConnectionSaved|null>(null);
    let recoveryStatus = $state<DictionaryConnectionRecovery|null>(null);
    let recoveryConfirmed = $state(false);
    async function inspectRecovery() {
        if(!target)return;
        const submitted={...target};busy=true;error='';recoveryStatus=null;recoveryConfirmed=false;
        try {
            const result=await api.dictionaryConnectionRecovery({expected_source:submitted.source_path,revision:submitted.revision});
            if(result.profile_source!==submitted.source_path || result.profile_revision!==submitted.revision) throw new Error('復元状態の対象が一致しません。');
            if(target?.source_path===submitted.source_path && target.revision===submitted.revision) recoveryStatus=result;
        }catch(e){error=message(e);}finally{busy=false;}
    }
    async function restoreConnection() {
        const submitted=recoveryStatus;
        if(!submitted?.pending?.restorable || !recoveryConfirmed || target?.source_path!==submitted.profile_source || target.revision!==submitted.profile_revision)return;
        busy=true;error='';recoveryConfirmed=false;savedConnection=null;
        try {
            const result=await api.restoreDictionaryConnection({expected_source:submitted.profile_source,revision:submitted.profile_revision,transaction_id:submitted.pending.transaction_id});
            if(result.profile_source!==submitted.profile_source || !result.restored || result.runtime_applied!==false)throw new Error('復元結果を確認できません。再実行せず状態を確認してください。');
            recoveryStatus=result;
        }catch(e){recoveryStatus=null;error=`${message(e)} 復元待ち状態を確認し直してください。`;}finally{busy=false;}
    }
    async function saveConnection() {
        const submitted=savedConnection;
        if(!submitted || !visibleConnection || (submitted.preview.affected_destinations.length>1 && !sharedConfirmed))return;
        busy=true;error='';savedConnection=null;
        try {
            const result=await api.saveDictionaryConnection({...submitted.input,review_revision:submitted.preview.review_revision,
                dictionary_revision:submitted.preview.dictionary_revision,confirm_shared_definition:sharedConfirmed});
            if(result.profile_source!==submitted.input.expected_source || result.graph_root!==submitted.preview.context.graph_root
                || result.saved!==true || result.runtime_applied!==false)throw new Error('保存結果を確認できません。再実行せず保存状態を確認してください。');
            connectionReceipt=result;
        }catch(e){error=`${message(e)} 入力は保持しています。再保存せず「接続の復元待ち状態を確認」で状態を確認してください。`;}
        finally{busy=false;sharedConfirmed=false;}
    }
    function connectionInput():DictionaryConnectionInput|null {
        if (!target || !draft || !routeSelection || !failurePolicy) return null;
        return {expected_source:target.source_path,revision:target.revision,dictionary_key:draft.key,destination:routeSelection,
            failure_policy:failurePolicy==='skip'?'skip_speech':'speak_original'};
    }
    let currentConnection=$derived(connectionInput());
    let visibleConnection=$derived(savedConnection && currentConnection && connectionInputKey(savedConnection.input)===connectionInputKey(currentConnection) ? savedConnection.preview : null);
    async function previewConnection() {
        const requested=connectionInput();if(!requested)return;
        const requestedOrigin=origin();busy=true;error='';savedConnection=null;sharedConfirmed=false;
        try {
            const checked=await prepareConnectionPreview(requested,body=>api.previewDictionaryConnection(body));
            const current=connectionInput();
            if(origin()===requestedOrigin && current && connectionInputKey(current)===connectionInputKey(checked.input)) savedConnection=checked;
            else error='確認中に対象または入力が変わりました。入力を保持して確認し直してください。';
        }catch(e){error=message(e);}finally{busy=false;}
    }
    let connectionPlan = $derived(routes && routeSelection ? speechConnectionPlan(routes,routeSelection) : null);
    let draft = $derived(target ? drafts[target.source_path] : undefined);
    const origin = () => window.location.origin;
    function message(error:unknown) {
        if (error instanceof ControlApiError && error.body && typeof error.body==='object' && 'reason' in error.body && typeof error.body.reason==='string') return error.body.reason;
        return error instanceof Error ? error.message : String(error);
    }
    function input(): DictionarySetupInput {
        if (!target || !draft) throw new Error('操作対象を確認してください。');
        return { expected_source:target.source_path,revision:target.revision,key:draft.key,label:draft.label,
            existing_path:draft.mode==='existing'?draft.file:null };
    }
    function validReview() { return !!review && !!target && !!draft && review.matches(origin(),input()); }
    async function refreshTarget() {
        const next = await api.dictionarySetupTarget();
        drafts[next.source_path] ??= {key:'reading',label:'配信の読み方',mode:'new',file:''};
        target=next;
        routes=null;
        routeSelection='';
        failurePolicy='';
        savedConnection=null;
        recoveryStatus=null;recoveryConfirmed=false;sharedConfirmed=false;
        review=null; externalConfirmed=false;
    }
    async function load() {
        busy=true;error='';
        try { await refreshTarget(); } catch(e) { error=message(e); } finally { busy=false; }
    }
    async function preview() {
        busy=true;error='';review=null;externalConfirmed=false;
        try {
            const checked=await prepareDictionarySetupReview(origin(),input(),body=>api.previewDictionarySetup(body));
            if (checked.matches(origin(),input())) { review=checked;step=2; }
            else error='確認中に入力が変わりました。入力を残したまま、もう一度確認できます。';
        } catch(e) { error=message(e); } finally { busy=false; }
    }
    async function inspectRoutes() {
        if (!target) return;
        const requested={...target};
        busy=true;error='';routes=null;routeSelection='';failurePolicy='';savedConnection=null;
        try {
            const diagnostics=await api.flowgraphDiagnostics();
            const current=await api.dictionarySetupTarget();
            if (current.source_path!==requested.source_path || current.revision!==requested.revision
                || target.source_path!==requested.source_path || target.revision!==requested.revision) {
                throw new Error('確認中に操作対象または設定が変わりました。対象・保存状態を確認し直してください。');
            }
            routes=speechRouteView(diagnostics);
        } catch(e) {error=message(e);} finally {busy=false;}
    }
    async function save() {
        if (!review) return;
        const submitted=review;
        busy=true;error='';
        try {
            const result=await api.registerDictionary(submitted.createSaveRequest(origin(),input(),externalConfirmed));
            const resolved=submitted.resolveSaved(origin(),input(),result);
            receipt=resolved.receipt;
            review=null;
            if (resolved.refreshCurrentSource) {
                try { await refreshTarget(); }
                catch(e) { error=`辞書登録は保存済みです。保存状態の再取得に失敗しました：${message(e)} 登録を繰り返さず「対象・保存状態を確認」で再取得してください。`; }
            }
        } catch(e) {
            // A failed or lost response is not permission to submit again.
            review=null;
            error=`${message(e)} 入力は保持しています。保存先の状態を確認してから、変更予定を確認し直してください。`;
        } finally { busy=false; }
    }
    onMount(()=>{void load();});
</script>

<section class="setup-review" aria-label={comparison?'辞書導入の実保存比較':'辞書の追加と読み上げ接続'}>
    <header><h3>{comparison?'辞書を使い始める · 配置の確認用':'辞書を追加・読み上げに接続'}</h3>
        {#if comparison}<p>配置の比較用。このVACの設定と辞書へ実際に保存する。隔離した検証用VACで使用する。</p>
        {:else}<p>辞書を登録し、使う読み上げ先を選んで接続する。保存先と変更予定を確認してから保存する。</p>{/if}
    </header>
    <div class="toolbar">
        {#if comparison}
        <button type="button" aria-pressed={layout==='all'} onclick={()=>layout='all'}>A · 全体を見渡す</button>
        <button type="button" aria-pressed={layout==='steps'} onclick={()=>layout='steps'}>B · 順に進める</button>
        {/if}
        <button type="button" disabled={busy} onclick={load}>対象・保存状態を確認</button>
    </div>
    {#if target && draft}
        <p>操作対象・保存先：<strong class="path">{target.source_path}</strong></p>
        <p>辞書の登録はこの保存先だけを変更する。別プロファイルの次回既定は変更しない。</p>
        {#if layout==='steps'}
            <nav class="toolbar" aria-label="辞書導入の段階">
                {#each ['1 辞書','2 反映方法','3 変更予定'] as name,index}
                    <button type="button" aria-pressed={step===index} onclick={()=>step=index}>{name}</button>
                {/each}
            </nav>
        {/if}
        {#if layout==='all' || step===0}
            <fieldset disabled={busy}>
                <legend>1. 辞書</legend>
                <label>表示名<input bind:value={draft.label} /></label>
                <label>使い始め方<select bind:value={draft.mode}><option value="new">新しい辞書を作る</option><option value="existing">既存のTSVを登録する</option></select></label>
                {#if draft.mode==='existing'}<label>TSVのファイルパス<input bind:value={draft.file} placeholder="D:/辞書/配信.tsv" /></label>{/if}
                <label>辞書ID<input bind:value={draft.key} oninput={()=>savedConnection=null} /></label>
                <p>一覧とFlowgraphから参照する識別名。新規作成時はファイル名にも使う。保存先は変更予定で確認できる。</p>
            </fieldset>
        {/if}
        {#if layout==='all' || step===1}
            <section class="part"><h4>2. 保存と反映</h4>
                <p>登録後は、このVACを明示的に再起動すると既存の「辞書の編集」に現れる。入力中の作業を保存してから再起動する。</p>
                <p>読み上げ処理への接続は別の操作だ。下で接続案を確認し、明示的に保存するとFlowgraphを書き換える。音声再生や稼働中への適用は行わない。既存エディターとStudioは引き続き使える。</p>
                <button type="button" disabled={busy} onclick={inspectRoutes}>読み上げ先を確認（変更しない）</button>
                {#if routes}
                    {#if routes.kind!=='ready'}<p role="status">{routes.message}</p>
                    {:else}
                        <p>現在読み込まれている構成：<span class="path">{routes.root}</span></p>
                        <p>保存しただけの未適用変更は含まない。ここでは辞書の接続・読み上げは実行しない。</p>
                        {#if routes.routes.length===0}<p>読み込まれた構成に読み上げ先はない。用途の追加やStudioで構成できる。</p>{/if}
                        {#each routes.routes as route (route.runtime_node)}
                            <div class="part">
                                <strong class="path">{route.runtime_node}</strong>
                                <button type="button" aria-pressed={routeSelection===route.runtime_node} onclick={()=>{routeSelection=route.runtime_node;failurePolicy='';savedConnection=null;}}>この接続への挿入案を見る</button>
                                <p>{route.modeSuppressed?'現在のモードでは抑止中':'モードによる抑止なし（再生確認ではない）'}</p>
                                {#if route.definition_instances>1}<p>同じ定義を{route.definition_instances}か所で使用。定義の編集は他の使用先にも影響する。</p>{/if}
                                <details><summary>元の定義と接続を確認</summary><p class="path">{route.source_file}</p>
                                    <ul>{#each route.incoming as link}<li class="path">{link.exec?'実行':'データ'}：{link.from_node}:{link.from_port} → {link.to_port}</li>{/each}</ul>
                                </details>
                            </div>
                        {/each}
                        {#if connectionPlan}
                            <div class="part" aria-label="辞書の接続計画">
                                <h4>定義へ辞書を挟む場合の影響範囲</h4>
                                <p>まだ保存・接続はしない。現在の文章と実行の接続を辞書へ渡し、読込成功後に置換した文章を同じ読み上げ先へ送る案。</p>
                                <p>声やエンジンなど、他の入力接続はそのまま残す。</p>
                                <ul>{#each connectionPlan.affected as name}<li class="path">{name}</li>{/each}</ul>
                                {#if connectionPlan.affected.length>1}<p>この案は共有定義への変更なので、上記の使用先すべてが変わる。一つだけ変えたい場合は、呼出側での挿入や定義の分離を先に検討する。</p>{/if}
                                {#each connectionPlan.paths as path}
                                    <details><summary class="path">{path.destination} の接続</summary>
                                        <ul>{#each path.text as link}<li class="path">文章：{link.from_node}:{link.from_port} → 辞書 → 読み上げ</li>{/each}
                                            {#each path.triggers as link}<li class="path">実行：{link.from_node}:{link.from_port} → 辞書読込 → 成功後に読み上げ</li>{/each}
                                            {#each path.retained as link}<li class="path">保持：{link.from_node}:{link.from_port} → {link.to_port}</li>{/each}</ul>
                                    </details>
                                {/each}
                                {#each connectionPlan.blockers as reason}<p>{reason}</p>{/each}
                                <label>辞書を読み込めなかったとき
                                    <select bind:value={failurePolicy} onchange={()=>savedConnection=null}>
                                        <option value="">方針を選ぶ</option>
                                        <option value="skip">その文章は読み上げず、エラーを記録する</option>
                                        <option value="original">辞書を使わず元の文章を読み、エラーを記録する</option>
                                    </select>
                                </label>
                                {#if failurePolicy==='skip'}<p>この案では、辞書の読込に失敗した文章を読み上げない。次の入力では再び辞書を読み込む。</p>
                                {:else if failurePolicy==='original'}<p>この案では、辞書の読込に失敗しても元の文章を読み上げる。登録した読み方は、その回には適用されない。</p>{/if}
                                <p>ここで選んでも設定は変わらない。実際の接続前に、文章と実行の対応、保存済み構成との差を確認する必要がある。</p>
                                <p>次の確認は、保存済み設定に登録されている辞書ID「{draft.key}」を使う。上の辞書フォームの未保存変更は使わない。</p>
                                <button type="button" disabled={busy || !currentConnection} onclick={previewConnection}>保存済み設定から接続案を確認</button>
                                {#if visibleConnection}
                                    <div class="part" aria-label="保存済み構成の接続案">
                                        <h4>変更後の構成検査済み · 未保存</h4>
                                        <p>使う辞書：<span class="path">{visibleConnection.context.dictionary_path}</span></p>
                                        <p>保存先：<span class="path">{visibleConnection.context.graph_root}</span></p>
                                        <p>影響する読み上げ先：{visibleConnection.affected_destinations.join('、')}</p>
                                        {#each Object.entries(visibleConnection.changes) as [file,change]}
                                            <details><summary>{file} の変更前後</summary><h5>変更前</h5><pre>{change.original}</pre><h5>変更後</h5><pre>{change.proposed}</pre></details>
                                        {/each}
                                        <p>保存済み構成に対する案。保存後も稼働中の構成には自動適用しない。</p>
                                        {#if visibleConnection.affected_destinations.length>1}
                                            <label class="confirmation"><input type="checkbox" bind:checked={sharedConfirmed} disabled={busy} />上記の読み上げ先すべてに共通する定義を変更することを確認した。</label>
                                        {/if}
                                        <button type="button" disabled={busy || (visibleConnection.affected_destinations.length>1 && !sharedConfirmed)} onclick={saveConnection}>確認した接続を保存（まだ適用しない）</button>
                                    </div>
                                {/if}
                            </div>
                        {/if}
                    {/if}
                {/if}
            </section>
        {/if}
        {#if layout==='all' || step===2}
            <section class="part"><h4>3. 変更予定</h4>
                <button type="button" disabled={busy || !draft.key.trim() || !draft.label.trim() || (draft.mode==='existing' && !draft.file.trim())} onclick={preview}>変更予定を確認（まだ保存しない）</button>
                {#if review && validReview()}
                    <p>{review.preview.creates_file?'新しいTSVを作成して登録':'既存TSVを内容変更せず登録'}：<span class="path">{review.preview.dictionary_path}</span></p>
                    {#if review.preview.existing_rows!==null}<p>現在の登録行数：{review.preview.existing_rows}</p>{/if}
                    {#if review.preview.storage_scope==='external_file'}
                        <label class="confirmation"><input type="checkbox" bind:checked={externalConfirmed} />この設定フォルダー外のファイルを使う。別の構成でも同じファイルを使っていれば、今後の辞書編集が共有されることを確認した。</label>
                    {/if}
                    <p>保存後も読み上げへの接続は未実施。辞書一覧への反映には再起動が必要。</p>
                    <button type="button" disabled={busy || (review.preview.storage_scope==='external_file' && !externalConfirmed)} onclick={save}>この保存先へ辞書を登録</button>
                {:else if review}<p>入力が変わった。変更予定を確認し直してください。</p>{/if}
            </section>
        {/if}
        {#if layout==='steps' && step<2}<button type="button" onclick={()=>step++}>次へ</button>{/if}
    {/if}
    {#if busy}<p role="status">処理中…</p>{/if}
    {#if error}<p role="alert">{error}</p>{/if}
    {#if target}
        <section class="part" aria-label="接続の保存復旧">
            <button type="button" disabled={busy} onclick={inspectRecovery}>接続の復元待ち状態を確認</button>
            {#if recoveryStatus}
                <p class="path">{recoveryStatus.profile_source}</p>
                {#if recoveryStatus.pending}
                    <p>未完了の保存：{recoveryStatus.pending.changed_files.join('、')}</p>
                    {#if recoveryStatus.pending.restorable}
                        <label class="confirmation"><input type="checkbox" disabled={busy} bind:checked={recoveryConfirmed} />この未完了の保存を取り消し、保存開始前のファイルへ戻す。</label>
                        <button type="button" disabled={busy || !recoveryConfirmed} onclick={restoreConnection}>保存開始前へ復元</button>
                    {:else}<p>後からの編集または読込不能があるため、自動復元しない：{recoveryStatus.pending.conflicting_files.join('、')}</p>{/if}
                {:else}<p>{recoveryStatus.restored?'保存開始前へ復元した。稼働中への適用は行っていない。':'未完了の保存記録はない。保存済みの接続は変更案で確認できる。'}</p>{/if}
            {/if}
        </section>
    {/if}
    {#if connectionReceipt}<div role="status" class="part"><strong>読み上げへの接続を保存した</strong><p class="path">{connectionReceipt.profile_source}</p><p class="path">{connectionReceipt.graph_root}</p><p>変更ファイル：{connectionReceipt.changed_files.join('、')}</p><p>この保存操作では稼働中へ適用していない。その後の再読込状況はStudioで確認できる。まだ再読込していなければ、他の未保存作業を確認してから構成を再読込する。</p></div>{/if}
    {#if receipt}<div role="status" class="part"><strong>辞書登録を保存した · 再起動待ち</strong><p class="path">{receipt.source_path}</p><p class="path">{receipt.dictionary_path}</p><p>この登録操作では読み上げ処理への接続・再起動は行っていない。</p></div>{/if}
</section>

<style>
    .setup-review{display:grid;gap:1rem;border:1px solid var(--color-surface-400,#677080);border-radius:1rem;padding:1rem}
    h3,h4{font-weight:700}p{margin:.5rem 0} .toolbar{display:flex;gap:.5rem;flex-wrap:wrap}
    button,input,select{border:1px solid var(--color-surface-400,#677080);border-radius:.5rem;padding:.5rem;background:transparent;color:inherit}
    button{cursor:pointer}button:disabled{opacity:.5;cursor:default}button[aria-pressed=true]{outline:2px solid var(--color-primary-400,#8fdcc6)}
    fieldset,.part{border:1px solid var(--color-surface-400,#677080);padding:1rem;border-radius:.75rem;min-width:0}
    label{display:grid;gap:.3rem;margin:.7rem 0}input,select{max-width:100%;min-width:0} .confirmation{display:flex;align-items:start;gap:.6rem}
    .path{overflow-wrap:anywhere} [role=alert]{border-left:3px solid #e5ae84;padding-left:.75rem}
    pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:.85rem}
</style>

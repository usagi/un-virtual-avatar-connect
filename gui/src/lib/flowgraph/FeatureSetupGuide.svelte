<script lang="ts">
    import { parse } from "smol-toml";
    import { flowgraphStore } from "../flowgraphStore.svelte";
    import type { FlowgraphFileDocument } from "../types";
    import { FEATURE_PACKS } from "./featurePacks";
    import { resolveSetupFields, plannedWebInput } from "./featureSetup";
    import FeatureUsageGuide from "./FeatureUsageGuide.svelte";
    import { api } from "../api";
    import contract from "../../../../docs/maintenance/unvac-feature-setup-bindings-20261017.json";

    const templates = FEATURE_PACKS.map(pack => ({ ...pack,
        document: parse(pack.source) as unknown as FlowgraphFileDocument,
        fields: contract.packs.find(item => item.id === pack.id)?.controls ?? [],
    }));
    const guide = $derived.by(() => {
        const saved = flowgraphStore.currentFile?.parsed;
        if (!saved || !flowgraphStore.draftNodes || !flowgraphStore.draftEdges) return null;
        const document = { ...saved, nodes: flowgraphStore.draftNodes, edges: flowgraphStore.draftEdges,
            instances: flowgraphStore.draftInstances ?? [] };
        for (const pack of templates) {
            const result = resolveSetupFields(document, pack.document, pack.fields);
            if (!result.reason || flowgraphStore.currentFq?.startsWith(`features/${pack.id}/`)) {
                return { id: pack.id, title: pack.title, ...result };
            }
        }
        return null;
    });
    function select(node: string) {
        flowgraphStore.selectedNodeId = node;
        flowgraphStore.selectedNodeIds = [node];
        flowgraphStore.selectedGroupId = null;
    }
    let inputText=$state(''),sending=$state(false);
    const externalInput = $derived.by(() => {
        const node = flowgraphStore.draftNodes?.find(node => node.id === 'in' && node.feature === 'flowgraph.ingress.web_input');
        return node && flowgraphStore.currentFq ? plannedWebInput(`${flowgraphStore.currentFq}::in`, node.properties) : null;
    });
    let sendResult=$state<{fq:string;root:string;text:string}|null>(null);
    async function sendTest(){
        const fq=flowgraphStore.currentFq, root=flowgraphStore.currentFile?.root_dir;
        if(sending||flowgraphStore.isDirty||!fq||!root||guide?.reason||!['counter','voicevox'].includes(guide?.id??''))return;
        const content=inputText;
        sending=true;sendResult=null;
        try{
            const diagnostics=await api.flowgraphDiagnostics();
            const saved=flowgraphStore.tree?.files.find(file=>file.fq===fq)?.source_digest;
            if(flowgraphStore.currentFq!==fq||flowgraphStore.currentFile?.root_dir!==root||flowgraphStore.isDirty||diagnostics.root_dir!==root||!saved||diagnostics.source_digests?.[fq]!==saved)throw Error('対象または読み込み内容が変わりました。保存・適用状態を確認してください。');
            const fixedChannel=flowgraphStore.draftNodes?.find(node=>node.id==='in')?.properties.fixed_channel;
            const sourceKind=typeof fixedChannel==='string'&&fixedChannel.trim()?fixedChannel.trim():'web_input';
            await api.triggerFlowgraphNode(`${fq}::in`,{expected_root:root,expected_source_digest:saved,exec_port:'__trigger__',inputs:{__content__:content,__source_actor__:'gui',__source_kind__:sourceKind}});
            sendResult={fq,root,text:`${fq} への入力を受け付けました。処理結果はログや出力で確認してください。`};
            if(flowgraphStore.currentFq===fq&&flowgraphStore.currentFile?.root_dir===root)await flowgraphStore.refreshDiagnostics();
        }catch(e){sendResult={fq,root,text:String(e)};}finally{sending=false;}
    }
</script>

{#if guide}
    <details class="border-b border-surface-300-700 p-3 text-xs" aria-label="用途設定への案内">
        <summary class="cursor-pointer font-semibold">{guide.title} · 設定箇所</summary>
        <p class="my-2 opacity-65">項目を選ぶと、この下の既存設定で編集できます。表示値は編集中の内容です。</p>
        <p class="my-2 opacity-65">Studioで保存すると実行構成を再読み込みします。他の構成も対象となり、カウンターなどの一時状態が戻る場合があります。保存後は適用結果を確認してください。</p>
        {#if guide.reason}<p>{guide.reason}</p>{:else}
            <FeatureUsageGuide packId={guide.id} />
            {#if guide.id==='counter'||guide.id==='voicevox'}
                <section aria-label="この機能へ試験入力" class="my-3 rounded border p-2">
                    <p class="break-all">入力先: {flowgraphStore.currentFq} · Web入力ノード</p>
                    <p>この構成を直接試します。外部HTTPの受付状態は別途確認してください。</p>
                    {#if externalInput}
                        <details class="my-2">
                            <summary class="cursor-pointer">外部アプリから入力する場合</summary>
                            <p class="mt-2">編集中の設定から決まる送信先です。このVACの接続先アドレスに、次のパスを付けて使用します。</p>
                            <code class="block my-2 break-all select-all">{externalInput.method} {externalInput.path}</code>
                            <p>保存後、入力先の追加・変更にはVACの再起動が必要です。この表示だけでは受付中と判断できません。</p>
                        </details>
                    {/if}
                    {#if guide.id==='voicevox'}<p>送信すると音声を合成・再生します。</p>{/if}
                    <label class="block my-2">試す文章<input class="input" bind:value={inputText} disabled={sending}/></label>
                    <button class="underline" disabled={sending||flowgraphStore.mutating||flowgraphStore.isDirty} onclick={()=>void sendTest()}>この機能へ入力を送る</button>
                    {#if flowgraphStore.isDirty}<p class="mt-2">未保存の編集があります。Studioで保存・適用してから送信してください。自動補正された配置も編集に含まれます。</p>{/if}
                    {#if sendResult && sendResult.fq===flowgraphStore.currentFq && sendResult.root===flowgraphStore.currentFile?.root_dir}<p role="status" class="mt-2 break-all">{sendResult.text}</p>{/if}
                </section>
            {/if}
            <div class="flex flex-col gap-1">
                {#each guide.fields as field}
                    <button class="rounded border border-surface-300-700 px-2 py-1.5 text-left" onclick={() => select(field.node_id)}>
                        <span class="block">{field.label}</span>
                        <span class="block truncate opacity-65">{JSON.stringify(field.value)}</span>
                    </button>
                {/each}
            </div>
        {/if}
    </details>
{/if}

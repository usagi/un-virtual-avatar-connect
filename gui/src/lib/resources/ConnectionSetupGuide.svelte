<script lang="ts">
	import { onMount } from "svelte";
	import { api } from "../api";
	import { tabNavStore, type TabId } from "../tabs.svelte";
	import { connectionSetup } from "./connectionSetup.svelte";
	type Step = { title: string; description: string; tab: TabId; target: string; action: string };
	const paths: { id: string; title: string; description: string; steps: Step[] }[] = [
		{ id: "stream", title: "通知・チャット・クリップ", description: "AI契約は不要。TwitchのイベントやコマンドをFlowgraphで処理します。", steps: [
			{title:"使う機能を確認する",description:"「Twitch配信支援（AIなし）」で作成した場合、通知とクリップ紹介は導入済みです。そのまま次のTwitch接続へ進めます。既存プロファイルでは導入済みの一覧を確認し、足りない機能を追加します。",tab:"flowgraph",target:"feature-library",action:"導入済み機能と追加内容を確認"},
			{title:"Twitchを接続する",description:"配信先と投稿アカウントをフォームで設定・保存し、対象VACへ適用してから認証します。TOML編集やAI契約は不要です。",tab:"resources",target:"twitch",action:"Twitch設定・認証へ"},
			{title:"OBSに画面出力を追加する",description:"出力管理の「表示する出力チャンネル」にstream-alertsを入力し、「OBS Template Effects」のURLをOBSのブラウザーソースへ設定します。複数VACでは接続先とポートを区別してください。",tab:"live",target:"outputs",action:"出力URLを確認"},
			{title:"送信先と表示・音声を確認する",description:"追加したFlowgraphの送信先を確認してから動作確認します。通知を使うだけなら、AIやマイクの設定は不要です。",tab:"flowgraph",target:"feature-installed",action:"導入済み機能を確認"},
		]},
		{ id:"speech",title:"文章の読み上げ",description:"VOICEVOXを使う構成。AI契約なしで文章を音声にできます。",steps:[
			{title:"読み上げ機能を追加する",description:"「入力した文章を読み上げる」を追加し、VOICEVOX ENGINEの起動・接続先・話者を確認します。",tab:"flowgraph",target:"feature-library",action:"読み上げ機能を選ぶ"},
			{title:"入力経路を確認する",description:"Studioで入力チャンネルと音声の出力先を確認します。保存時にFlowgraph全体を再読み込みします。",tab:"flowgraph",target:"flowgraph-studio",action:"構成を確認"},
			{title:"短い文章で試す",description:"設定した入力先へ文章を送り、実際に聞こえることを確認します。操作すると音声などの出力が発生します。",tab:"live",target:"ingress",action:"入力画面へ"},
		]},
		{ id:"apps",title:"アバター・外部アプリ",description:"UNAvatarやUNMotionなどを登録・操作します。",steps:[
			{title:"起動するアプリを登録する",description:"実行ファイルと表示名を設定。複数の同じアプリを使う場合は識別ID・接続先も分けます。保存だけでは起動しません。",tab:"resources",target:"run-with",action:"連携アプリを設定"},
			{title:"接続対象を確認する",description:"上部の「連携アプリ」から必要なアプリを起動し、診断で対象と接続状態を確認します。",tab:"resources",target:"connections",action:"接続状態を確認"},
		]},
		{ id:"ai",title:"AIパートナー",description:"AIを使いたい場合だけ設定します。通知などの配信支援とは独立しています。",steps:[
			{title:"モデル・人格・資格情報を設定する",description:"現在の管理プロファイルに設定します。「保存して再起動」は対象VACを再起動する操作です。",tab:"ai",target:"ai-setup",action:"AIの基本設定へ"},
			{title:"入力と音声・画面出力を確認する",description:"AIを設定しただけでは読み上げやアバター出力は完成しません。利用する構成の接続を確認します。",tab:"live",target:"preflight",action:"動作確認へ"},
		]},
	];
	const path = $derived(paths.find(item => item.id === connectionSetup.selected)!);
	let scope = $state("操作対象を確認中…");
	onMount(() => {
		void api.desktopProfiles().then(info => {
			const label = info.profiles.find(item => item.id === info.current_profile_id)?.label;
			scope = info.current_conf_path ? `${label ?? "設定ファイル"} · ${info.current_conf_path}` : "操作対象を確認できません。接続認証を確認してください。";
		}).catch(() => scope = "操作対象を確認できません。接続認証を確認してください。");
	});
</script>

<section class="vac-panel p-4" aria-label="接続の準備">
	<h3 class="font-semibold">何を使いたいですか？</h3>
	<p class="mt-2 break-all text-xs opacity-65">操作対象: {scope}</p>
	<p class="my-3 text-xs opacity-65">必要な用途だけ選べます。この案内を開くだけでは設定の変更・認証・送信は行いません。</p>
	<div class="flex flex-wrap gap-2" role="group" aria-label="接続する用途">
		{#each paths as item}<button class="rounded border px-3 py-2 text-sm" class:border-primary-500={connectionSetup.selected === item.id} aria-pressed={connectionSetup.selected === item.id} onclick={() => connectionSetup.selected = item.id}>{item.title}</button>{/each}
	</div>
	<p class="my-3 text-sm">{path.description}</p>
	<ol class="grid gap-3 md:grid-cols-2">
		{#each path.steps as step, index}<li class="rounded border border-surface-300-700 p-3">
			<h4 class="text-sm font-semibold">{index + 1}. {step.title}</h4>
			<p class="my-2 text-xs opacity-75">{step.description}</p>
			<button class="rounded border px-3 py-2 text-xs" onclick={() => tabNavStore.openFeature(step.tab, step.target)}>{step.action}</button>
		</li>{/each}
	</ol>
	<p class="mt-3 text-xs opacity-65">案内の順番は接続完了の判定ではありません。資格情報や外部ファイルを複数プロファイルで共有している場合、その変更も共有されます。</p>
	<button class="mt-2 text-xs underline" onclick={() => tabNavStore.openFeature("settings", "profiles")}>プロファイルを作成・設定内容を編集</button>
</section>

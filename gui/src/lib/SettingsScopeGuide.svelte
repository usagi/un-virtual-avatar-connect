<script lang="ts">
 import { tabNavStore, type TabId } from './tabs.svelte';
 const rows: {name:string; save:string; apply:string; tab:TabId; id:string}[] = [
  {name:'Twitchの基本設定・認証',save:'基本設定は接続中のVAC。認証情報は標準では同じPC内で共有',apply:'基本設定は対象VACの再起動後に適用。認証の開始とブラウザーを開く操作は別',tab:'resources',id:'twitch'},
  {name:'連携アプリの起動設定',save:'接続中のVACのconf.toml',apply:'管理一覧へ即時反映。既に動いているアプリは停止・再起動しない',tab:'resources',id:'run-with'},
  {name:'保存辞書',save:'選択した辞書のTSVファイル（画面に保存先を表示）',apply:'ファイル保存後、利用側の次回読込で反映',tab:'dictionary',id:'dictionary-edit'},
  {name:'動作モードの切替',save:'現在の実行状態。起動時の既定モードとは別',apply:'切替計画に従って即時適用',tab:'modes',id:'mode-control'},
  {name:'プロファイル本体',save:'選択したプロファイルのconf.toml',apply:'保存後、対象VACを再起動',tab:'settings',id:'profiles'},
  {name:'Flowgraph・用途別機能',save:'接続中のVACのFlowgraphフォルダー',apply:'保存・追加時に全体を再読み込み',tab:'flowgraph',id:'flowgraph-studio'},
  {name:'AIの導入・基本設定',save:'現在の管理プロファイルと専用の資格情報ファイル',apply:'保存後に対象VACの再起動を確認。「保存のみ」では稼働状態を変えない',tab:'ai',id:'ai-setup'},
  {name:'AI詳細設定',save:'「confへ保存」と一時変更を選択',apply:'一時変更は即時。保存だけでは再起動後に反映',tab:'ai',id:'ai-detail'},
  {name:'一時reload',save:'プロファイル本体には保存しない',apply:'接続中のVACのメモリへ反映',tab:'settings',id:'reload'},
  {name:'テーマ',save:'このブラウザーの接続先ごとの保存領域',apply:'即時。プロファイルの書出には含まない',tab:'settings',id:'appearance'},
 ];
</script>
<details class="vac-panel p-4">
 <summary class="cursor-pointer text-sm font-semibold">保存先と適用の違い</summary>
 <p class="my-3 text-xs opacity-65">上部の操作対象と接続先を確認してください。複数プロファイルが同じ外部ファイルを参照する場合、そのファイルの変更は共有されます。</p>
 <div class="overflow-x-auto"><table class="w-full text-left text-xs"><thead><tr><th class="p-2">設定</th><th class="p-2">保存先</th><th class="p-2">適用</th></tr></thead><tbody>{#each rows as row}<tr class="border-t border-surface-300-700"><td class="p-2"><button class="text-primary-600-400 underline" onclick={()=>tabNavStore.openFeature(row.tab,row.id)}>{row.name}</button></td><td class="p-2">{row.save}</td><td class="p-2">{row.apply}</td></tr>{/each}</tbody></table></div>
</details>

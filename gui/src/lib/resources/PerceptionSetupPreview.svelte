<script lang="ts">
 import PerceptionSavedEditor from './PerceptionSavedEditor.svelte';
 import {createPerceptionReviewTransport} from '../perceptionReviewTransport';
 import {createNativePerceptionReviewTransport} from '../perceptionNativeReviewTransport';
 const savedReview=createPerceptionReviewTransport();
 const nativeStorage=new URLSearchParams(window.location.search).get('perceptionStorage')==='native';
 const editorTransport=nativeStorage ? createNativePerceptionReviewTransport() : savedReview.transport;
 let reviewInstance=$state('VAC-A');
 let profileId=$state('stream');
 let layout=$state<'partner'|'sources'>('partner');
</script>
<section class="perception-preview" aria-label="画面理解の配置比較">
 <header><strong>画面理解 · 操作構成の比較</strong>
 <p>Aは「この共演者に何を見せるか」から、Bは「この画面を誰に見せるか」から操作する。切り替えても設定・下書きは同じ。機能を減らす比較ではない。</p></header>
 <div class="layout-buttons" role="group" aria-label="画面理解のA/B切替">
 <button type="button" aria-pressed={layout==='partner'} onclick={()=>layout='partner'}>A · 共演者から設定</button>
 <button type="button" aria-pressed={layout==='sources'} onclick={()=>layout='sources'}>B · 画面から共有管理</button>
 </div>
 <p class="current-layout" aria-live="polite">表示中：{layout==='partner' ? 'A — 共演者を選び、その人に見せる画面を設定' : 'B — 画面を選び、見せる共演者を一覧で設定'}</p>
 <section class="design-rationale" aria-label="この比較を作った理由">
 <h3>何を解決するための比較だったか</h3>
 <p>「共演者にゲームを見せたい」という個別の設定と、「誰が何を見ているか確認したい」という全体の管理を、どちらも迷わず行えるようにするための比較。</p>
 <p><strong>Aの狙い：</strong>共演者を作る流れで、その人に見せる画面を選べる。ただし、画面の取得条件は他の共演者にも共通なので、一人だけの設定と誤解させない表示が必要。</p>
 <p><strong>Bの狙い：</strong>画面の設定を変更すると誰に影響するかを見渡せる。ただし、初めて「共演者にゲームを見せたい」人には、取得元の管理から始める理由が伝わりにくい。</p>
 <p><strong>採用する土台はA：</strong>共演者を選び、その人に見せる画面を設定する流れを優先する。複数共演者の利用を前提にした管理画面を通常操作の入口にはしない。Bは比較用に残すが、選び直す必要はない。正式UIへの反映と動作検証はこれから行う。</p>
 <p><strong>既存構成の注意点：</strong>同じ観測チャンネルを使う画面は、共演者への割当も一緒に変わる。画面一枚ずつ独立した設定とは限らない。次の案でも、この変更範囲を明示する。</p>
 </section>
 <details class="comparison-guide"><summary>既存A/Bサンプルの操作と確認範囲</summary>
 <div class="comparison-options">
 <article><h3>A · 共演者から設定</h3><p>最初に共演者を選ぶ。その人の観測割当と画面履歴を表示する。</p><p>「この共演者だけ、ゲーム画面を見せたくない」という作業を絞り込みやすい。一方、同じ画面を見ている他の共演者を比べるには切替が必要。</p></article>
 <article><h3>B · 取得元を共有管理</h3><p>最初にOBSの画面を選ぶ。その画面を誰が見るか、共演者を並べて表示する。</p><p>「ゲーム画面を2人で共有し、もう1人には見せない」という全体の割当を見渡しやすい。一方、共演者が増えると表示する項目も増える。</p></article>
 </div>
 <p><strong>比較する操作：</strong>同じプロファイルでA/Bを切り替え、共演者の観測割当を1つ変更してみる。未保存のまま切り替えても入力は保持する。どちらが「誰に何を見せているか」を掴みやすいかを見てほしい。</p>
 <p><strong>両案で共通：</strong>OBS接続・シーン・ソースの条件は取得元の共有設定。Aで共演者を1人選んでも、その人だけの接続設定になるわけではない。追加・保存・削除・復元の機能は同じ。</p>
 <p>Aを土台に進める判断は済んでいる。このサンプルでは、共演者からの設定と共有範囲の表示を確認できる。</p>
 </details>
 <div class="choices">
 <label>模擬プロファイル<select aria-label="模擬プロファイル" bind:value={profileId}><option value="stream">配信用 · stream</option><option value="desk">デスクトップ用 · desk</option></select></label>
 {#if !nativeStorage}<label>保存確認のVAC<select aria-label="保存確認のVAC" bind:value={reviewInstance}><option>VAC-A</option><option>VAC-B</option></select></label>{/if}
 </div>
 <p>{nativeStorage ? '隔離フォルダーへ実保存する検証版。' : 'この比較画面のメモリー内に保存するサンプル。'}実OBS・撮影・常用設定・認証には接続しない。</p>
 <PerceptionSavedEditor target={{instance:nativeStorage ? '隔離VAC' : reviewInstance,profile:profileId}} transport={editorTransport} {layout}/>
 <details><summary>確認範囲と未確認のこと</summary><p>取得元の追加・編集・認証設定・観測割当・履歴・削除と復元を同じ保存経路で操作する。実OBSへの接続成否、再起動による適用、正式UIへの採用は今回の比較に含まない。</p>
 {#if !nativeStorage}<button onclick={savedReview.conflictNextSave}>次の保存で競合を再現</button>{/if}</details>
</section>
<style>
 .layout-buttons{display:flex;flex-wrap:wrap;gap:12px;margin:16px 0 8px}.layout-buttons button{padding:14px 20px;border:2px solid #8496ac;border-radius:12px;background:#222b3b;color:#edf2f9;font-size:1rem;font-weight:700;cursor:pointer}.layout-buttons button[aria-pressed="true"]{background:#aadbd3;color:#152d31;border-color:#d6fff7}.layout-buttons button:focus-visible{outline:3px solid #fff;outline-offset:3px}.current-layout{font-weight:600}
 .perception-preview{border:1px solid #617c89;border-radius:18px;padding:20px;background:#18232d;color:#e8f2f3;}
 header strong{font-size:1.2rem}p{font-size:.875rem;line-height:1.65;margin:.6rem 0;color:#c0d3da}
 .choices{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:12px}label{display:flex;flex-direction:column;gap:6px}
 select,button{background:#101a22;border:1px solid #63808e;border-radius:8px;padding:9px;color:inherit}details{margin-top:16px}
 .comparison-guide{margin-bottom:16px}.comparison-guide summary{cursor:pointer;padding:8px 0}.comparison-options{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px}.comparison-options article{padding:12px;border:1px solid #617c89;border-radius:10px}.comparison-options h3{margin:0;font-size:1rem}
 .design-rationale{margin-top:16px;padding:16px;border-left:4px solid #aadbd3;background:#101a22}.design-rationale h3{margin:0 0 12px}
 @media(max-width:700px){.comparison-options{grid-template-columns:1fr}}
 @media(max-width:900px){.choices{grid-template-columns:1fr}}
</style>

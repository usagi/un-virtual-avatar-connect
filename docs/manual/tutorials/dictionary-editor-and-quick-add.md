# 辞書エディタとクイック追加

配信中の用語登録と取消に加え、辞書の初回登録と既存の読み上げ処理への接続をGUIから行う手順です。クイック追加の実行経路や、接続案が対応しない構成は、引き続きStudioで編集できます。

## 画面から辞書を使い始める

「辞書」の **辞書を追加・読み上げに接続** を開きます。辞書・保存と反映・変更予定を同じ画面で確認できます。既存の「辞書の編集」「クイック追加」はその下に残ります。

1. 操作対象と保存先を確認し、新しい辞書を作るか、既存のTSVを登録するかを選びます。表示名と辞書IDを入力します。
2. 「変更予定を確認」でパスと共有範囲を確認し、「この保存先へ辞書を登録」を押します。設定フォルダー外のファイルは別構成とも共有される場合があります。登録だけでは読み上げへ接続されません。
3. 未保存の作業を保存してVACを再起動すると、登録した辞書が既存の編集一覧に現れます。
4. 「読み上げ先を確認」で接続先を選び、辞書を読めない場合にその文章を読まないか、元の文章を読むかを選びます。「保存済み設定から接続案を確認」は、登録済み辞書IDを使います。辞書フォームの未保存入力は接続に使いません。
5. 接続案の保存先・影響する読み上げ先・変更前後を確認します。共有定義の場合、一覧の全使用先に変更が及びます。一つの使用先だけを変える場合は、Studioで定義を分けるなど構成を先に調整します。
6. 「確認した接続を保存」でファイルへ保存します。稼働中の構成は自動では変わりません。他の未保存編集を確認してからStudioで構成を再読込してください。

既存の読み上げ先の声・エンジン・出力接続は保持します。接続案を作れない構成は理由を表示し、既存構成を保存し直すことはありません。新しい読み上げ処理自体を作る場合は、用途の追加やStudioを使います。

### 接続の保存が途中で失敗したとき

同じ保存を繰り返さず、「接続の復元待ち状態を確認」を押してください。未完了の保存記録があれば、対象ファイルを確認して保存開始前へ復元できます。後から編集された内容や読めないファイルがある場合、自動復元は拒否されます。記録とファイルを確認し、後からの編集を失わないように調整してください。

復元は保存ファイルを戻す操作です。稼働中の構成へ自動適用する操作ではありません。未完了の保存中や不正な構成への再読込は拒否し、それまで稼働していた構成を維持します。

以下は、手動で構成する場合と、既存の辞書編集・クイック追加の詳細です。

> 関連ソース:
> - Control API: [`src/web_interface/control/table/mod.rs`](https://github.com/usagi/virtual-avatar-connect/blob/v2/src/web_interface/control/table/mod.rs) /
>   [`src/web_interface/control/flowgraph/mod.rs`](https://github.com/usagi/virtual-avatar-connect/blob/v2/src/web_interface/control/flowgraph/mod.rs)
> - GUI: [`gui/src/lib/control/dictionary/`](https://github.com/usagi/virtual-avatar-connect/tree/v2/gui/src/lib/control/dictionary)
> - ロードマップ仕様: [phase-phi-control-api-dictionary-editor.md](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/phase-phi-control-api-dictionary-editor.md)

---

## ねらい

- **Glossary Editor Pane**（11 カラム一覧 / sort / filter / 編集 / 削除 / 409 衝突解決）で TSV 用語集を直接編集する
- **Live Quick-Add Widget** で `glossary.learn` / `glossary.forget` に 1-shot trigger を発火する
- `conf.toml` の `[[control_api.tables]]` + `quick_add` をどう書くと GUI から見えるようになるかを押さえる
- `control_triggerable` opt-in の範囲を知る（任意ノードの暴発トリガは不可）

---

## 全体像

```text
辞書エディタ → Table API（リビジョン照合）→ TSVファイル
                                                ↓ 次の読込実行
                                  load_tsv → glossary.replace → 読み上げ

クイック追加 → Trigger API → glossary.learn / forget → 更新済みTable
                                                        ↓ 明示的な保存配線が必要
                                                    TSVファイル
```

- **辞書エディタ**は TSV ファイルを直接読み書きします。更新時の `If-Match: b3:<hash>` で競合を検出します。
- **クイック追加**は Flowgraph の実行要求です。受付成功はファイル保存成功ではありません。
- `glossary.learn` / `forget` は渡された Table を変換するノードです。内部に辞書を蓄積する機能ではありません。既存辞書を入力しない場合、既定の空 Table を基に処理します。
- 保存には、最新の既存辞書を入力し、更新結果を対象 TSV へ書き戻す経路が必要です。単に `learn → write_tsv` をつなぐだけでは、既存行を保持できません。

---

## 前提

- [`flowgraph.example/dictionary/basic-replace.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/dictionary/basic-replace.flowgraph.toml)
  などの辞書を食わせる flowgraph が既に動いている
- GUI （Svelte ビルド済み）が `gui/dist/` に配置済み、または `npm run dev` で別ポート経由でアクセスしている

---

## Step 1 — `conf.toml` に Table を登録

```toml
# [control_api] セクションは既存。allow-list をここに追加する。
[[control_api.tables]]
key      = "chat_dict"                       # URL / localStorage キー
path     = "dictionary.chat.dict.tsv"        # ディスク上の TSV
label    = "Chat 用語集"
role     = "glossary"                        # これが無いと Editor の tab に出ない
editable = true
```

ここまでで既存の辞書エディターに登録できます。クイック追加は任意です。設定する場合は、後述の読込・更新・保存を通る入口ノードを指定します。`glossary.learn` 単体を入口にしても、既存TSVの読込と保存は保証されません。

`fq ID` は `{フォルダ}/{ファイル名}::{ノード ID}` 形式（拡張子 `.flowgraph.toml` は抜く）。
GUI の Flowgraph Studio → Node Card の「id」欄や、`GET /api/v1/control/flowgraph/tree` でも確認できる。

> ※ `[[control_api.tables]]` は **起動時 snapshot**。反映には VAC の再起動が必要（reload では拾わない）。

---

## Step 2 — 対応する Flowgraph を配線

通常の単語追加・編集は、まず辞書エディタを使ってください。登録済み TSV の行を読み込み、リビジョンを照合して保存します。Step 1 の `quick_add` は、専用の実行経路を検証できるまでは省略できます。

クイック追加を独自に配線する場合は、次の条件を満たす必要があります。

1. 更新直前に対象 TSV の最新内容を読み込み、その Table を `learn:dictionary` または `forget:dictionary` へ渡す。
2. 読込成功後に学習・取消を実行する。GUI の直接トリガだけでは、先行する `load_tsv` の実行を保証しない。
3. 学習と取消の保存先ノードを分け、それぞれの `updated_dictionary` と成功 exec を対応させる。二つのデータ出力を一つの `write:table` 入力へ直接束ねない。
4. `load_tsv:revision` を対応する `write_tsv:expected_revision` へ渡し、`on_conflict` では入力を保持して再読込・再確認する。この入力を空のままにするとリビジョン照合は行われない。
5. 既存行を含む辞書で追加・取消・再読込を行い、既存行保持と対象ファイルへの永続化を確認する。

以前掲載していた配線例は、辞書の読込実行と `dictionary` 入力が欠けていたため撤回しました。その例を流用している場合は、クイック追加からの書込を止め、辞書エディタで現在の内容を確認してください。初期導入で自動生成する経路は、上記の条件を検証してから提供します。

読み上げへの適用は別の経路です。[basic-replace のサンプル](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/dictionary/basic-replace.flowgraph.toml) は Web 入力ごとに TSV を読み、置換結果をログへ出します。実際の読み上げでは、その結果を選択した音声合成経路へ接続してください。

---

## Step 3 — GUI から編集する

VAC を再起動して辞書タブを開く。以下 2 つのウィジェットが現れる。

### Glossary Editor Pane（11 カラム一覧）

- 列: `reading / surface / part_of_speech / comment / priority / is_locked / enabled / by / created_at / expires_at / tags`
- ヘッダクリックでソート方向トグル
- 検索ボックスは `reading / surface / part_of_speech / comment / tags / by` を部分一致
- 通常語 / 正規表現、`disabled を隠す`、`expired を隠す` のフィルタ付き
- 行右端の **[編集]** でダイアログが開く（全カラムを編集可能、`Ctrl+Enter` で送信）
- **[削除]** は `is_locked = true` の行では disabled
- 他クライアントと編集が衝突すると **409 → 3-way 解決ダイアログ** が自動で開く
  - `base`（編集開始時）/ `mine`（送信値）/ `server`（最新）を並べて見せる
  - 「自分の編集を強制」「サーバ現行を採用」「フィールドごとに merge」の 3 パスから選ぶ

### Live Quick-Add Widget（1 行フォーム）

- Google IME列順で `reading`（読み）と `surface`（単語）を入力して **[Learn]**（`Ctrl+Enter` でも可）。置換動作は `surface → reading`
- 履歴は per-table で直近 10 件、各行に **[Undo]**
  - `forget_node_id` が conf に未設定なら自動的に disabled
  - Locked な行は `forget` ノード側で保護されます。取消の受付だけでは削除完了を確認できません。保存結果を観測する構成では、`on_locked` を失敗として扱い履歴を保持します。

以下は、既存の直接入力形式（`request_format = "direct"`、省略時も同じ）の送信例です。`chat-echo/main::learn` は例示用の入口名で、組み込み機能ではありません。この入力を受けて最新TSVを読み、更新・保存するグラフが必要です。Control入力から導入する場合は、[辞書更新サンプルの設定](https://github.com/usagi/virtual-avatar-connect/tree/v2/flowgraph.example/dictionary-control-update) の `request_format = "control_meta"` と入口・結果観測を組み合わせてください。両形式の入力は同一ではありません。

```
POST /api/v1/control/flowgraph/default/trigger/chat-echo/main::learn
Content-Type: application/json
Authorization: Bearer <token>

{
  "inputs": {
    "reading": "どくたーうさぎ",
    "surface": "Dr.USAGI",
    "part_of_speech": "固有名詞",
    "by": "gui:quick_add"
  }
}
```

---

## クイック追加の保存結果

`quick_add.learn_observe` / `forget_observe` を設定した構成では、その要求の保存ノードの結果を最大2秒待ちます。「保存完了」「既に登録」「競合」「失敗」を区別し、失敗・競合・結果未確認では入力を保持します。待機時間を過ぎても処理を取り消したことにはならず、自動再送もしません。現在の辞書を確認してから次の操作を行ってください。

観測設定のない既存構成では、従来どおり実行要求の受付までを表示します。受付は保存完了の証明ではありません。結果観測を含む配線・設定例は [dictionary-control-update](https://github.com/usagi/virtual-avatar-connect/tree/v2/flowgraph.example/dictionary-control-update) にあります。初回導入フォームからの自動接続は未提供です。

## よくある落とし穴

| 症状 | 原因 | 対処 |
|---|---|---|
| Editor のタブに何も出ない | `role = "glossary"` 未指定 | conf の該当エントリに `role` を追加して再起動 |
| Quick-Add が出ない | `quick_add` ブロック未指定 | `[control_api.tables.quick_add]` を書き、`node_id` を fq ID で指定 |
| Trigger 時に 400 `control_triggerable_forbidden` | 発火先が opt-in していないノード | `node_id` が typo、または learn/forget 以外の feature を指している。`control_triggerable` 対応ノードを指定する。更新前の読込を含む構成には `flowgraph.ingress.control` を入口として使える |
| Trigger 時に 404 `instance_not_found` | V2 は単一 instance のため `instance_id = "default"` のみ有効 | GUI は自動で `default` を使うので、手動 curl の場合のみ注意 |
| Learn しても Editor に反映されない | 実行受付後の保存が未実装・未完了・失敗 | Step 2 の読込・更新・競合検出・保存成功を確認する。Reload だけでは未保存の結果は保存されない |
| 409 が連続する | 他ユーザ（別タブ）と同時編集中 | 3-way 解決ダイアログで採用方針を決める。衝突が辛ければ `editable = false` で片側を read-only 化 |
| Quick-Add 履歴が消えた | `localStorage` をクリアした | ブラウザー保存の履歴です。設定ファイル別に分離され、サーバー側には保存しません。旧版の対象未識別履歴は自動で引き継ぎません |

---

## 履歴の対象と取り消し

クイック追加の履歴は、接続先ブラウザーの保存領域内で、稼働中の設定ファイルごとに分けて保存します。次回起動用の既定プロファイルを変えても、現在の操作対象は変わりません。設定ファイルを識別できない場合やブラウザー保存が使えない場合は、その画面を開いている間だけ履歴を扱います。

取り消しは、履歴の辞書名・ファイルパス・追加ノード・取消ノードが現在の構成と一致する場合だけ利用できます。旧版の対象未識別履歴は削除せず保存領域に残しますが、別プロファイルを誤操作しないため操作対象には読み込みません。辞書ファイルの内容自体は変更しません。

## 安全設計のポイント

- **allow-list 外のファイルは 404**（存在隠蔽）。Control API のパストラバーサル攻撃に対する一次防衛。
- **is_locked = true の行は PATCH / DELETE 不可（403）**。`forget` ノード側も `on_locked` 経路に出す。
- **Trigger API は `control_triggerable = true` のノードに限定**（400 で拒否）。任意ノード乗っ取り防止。
- 辞書エディタは `If-Match` によるリビジョン照合とファイル書込ロックを使います。Flowgraph の保存には `expected_revision` を明示的に接続してください。

---

## 関連

- [conf-reference.md §5.1](../conf-reference.md#51-control_apitables--辞書--汎用-table-の-gui-編集許可リスト-phase-φ)
- [Node Catalog](../node-catalog.md) の `flowgraph.glossary.*` / `flowgraph.table.*`
- Roadmap: [phase-phi-control-api-dictionary-editor.md](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/phase-phi-control-api-dictionary-editor.md)
- Backlog: [backlog-nodes.md](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/backlog-nodes.md)（`flowgraph.util.timer_interval` 予定など）

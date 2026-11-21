# 辞書更新の明示的なControl入力

初期導入用の配線を検証するサンプル。既存の汎用ノードだけで、現在の辞書を読み、行を追加・削除し、読込時のリビジョンを照合して保存する。常用設定やGUIへ自動登録するものではない。

`main.flowgraph.toml` と `assets/user.tsv` を同じ構成でコピーする。対象辞書はこのグラフファイルを基準にした `assets/user.tsv`。既存の辞書を使う場合は `path` ノードの値を明示的に変更する。

Control APIで `main::learn_request` を指定する入力例:

```json
{"inputs":{"source_actor":"gui:quick_add","meta":{"surface":"UNVAC","reading":"ゆーえぬばっく","part_of_speech":"名詞"}}}
```

取消には `main::forget_request` と同じ `surface`・`reading` を渡す。`exact` モードで一致行を削除し、ロック済み行はノードの保護対象になる。呼出元は非空の読み・単語などを事前検証すること。`source_actor` は呼出者が指定する注記で、本人確認や権限付与ではない。アクセス権は既存Control API設定で管理する。

各要求は `load_tsv → learn/forget → write_tsv` の順に進む。読込失敗では更新しない。`load:revision` を `save:expected_revision` に渡すため、別の保存が入った場合は競合として失敗する。成功ログは保存成功後に出る。HTTP 202は要求の受付であり、保存完了ではない。

GUIへの接続は、対象辞書の `quick_add` に次を指定する。省略時の `request_format` は従来の直接入力形式なので、このサンプルでは明示指定する。グラフのファイル名を変えた場合はノードIDも合わせる。

```toml
[control_api.tables.quick_add]
node_id = "main::learn_request"
forget_node_id = "main::forget_request"
request_format = "control_meta"
kind = "literal"
learn_observe = [
  { node_id = "main::learn_save", port = "on_success", outcome = "saved" },
  { node_id = "main::learn_save", port = "on_conflict", outcome = "conflict" },
  { node_id = "main::learn_save", port = "on_error", outcome = "failed" },
  { node_id = "main::learn_load", port = "on_error", outcome = "failed" },
  { node_id = "main::learn_edit", port = "on_duplicate", outcome = "unchanged" },
]
forget_observe = [
  { node_id = "main::forget_save", port = "on_success", outcome = "saved" },
  { node_id = "main::forget_save", port = "on_conflict", outcome = "conflict" },
  { node_id = "main::forget_save", port = "on_error", outcome = "failed" },
  { node_id = "main::forget_load", port = "on_error", outcome = "failed" },
  { node_id = "main::forget_edit", port = "on_nothing", outcome = "unchanged" },
  { node_id = "main::forget_edit", port = "on_locked", outcome = "failed" },
]
```

この送信形式と結果観測は開発中ソースで追加したもので、既存の配布候補r39には未収録。GUIは最大2秒待ち、上で指定した出力から保存・競合・失敗を判定する。結果を確認できなければ入力を保持し、自動再送しない。要求はまだ処理中の場合があるため、再操作前に辞書の内容を確認する。`learn_observe` / `forget_observe` を省略した設定は従来どおり送信受付のみを示す。読み上げへの反映にも、選択した処理で対象TSVを読み直す接続が必要だ。

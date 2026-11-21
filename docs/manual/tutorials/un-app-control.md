# Tutorial: un-app-control

UN App API over Zenoh (UNAA/Z) を Flowgraph から扱う first-slice サンプル。

> ソース: [`flowgraph.example/un-app-control/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/un-app-control/main.flowgraph.toml) + [`docs/manual/un-app-api-over-zenoh.md`](../un-app-api-over-zenoh.md)

## ねらい

- `flowgraph.un.app.status` で UNAA/Z discovery snapshot から app readiness を読む
- `flowgraph.un.app.command` で safe command envelope を作る
- `flowgraph.un.app.event` で command response / event key contract を確認する
- UNAA/Z は app control plane、UNMF/Z は motion data plane として分ける

## グラフ構造

```text
literal.json(snapshot) -> un.app.status -> log(display_name)
web_input trigger      -> un.app.command(dry_run) -> log(command key)
                                      └-> un.app.event(dry_run) -> log(response key)
```

## 実行手順

`un-app-control` は dry-run 用の snapshot literal を含むため、UNA / UNM 実体や Zenoh session なしで確認できます。

1. `flowgraph.example/un-app-control/` を `flowgraph/` にコピー
2. VAC 再起動 or Reload
3. <http://127.0.0.1:57000/input/un-app-control> に任意文字列を投稿
4. `UN Avatar Renderer`、`un/app/un-avatar/main/command`、`response/{request_id}` key が log に出ることを確認

実運用では `snapshot` literal を Control API diagnostics 由来の UNAA/Z snapshot に差し替え、`dry_run = false` と `wait_response_ms` を設定します。送れる command は UNAA/Z safe command allow-list と app が advertise した `commands` の両方で制限されます。

## 関連ノード

[`flowgraph.un.app.status`](../node-catalog.md#flowgraph-un-app-status) / [`flowgraph.un.app.command`](../node-catalog.md#flowgraph-un-app-command) / [`flowgraph.un.app.event`](../node-catalog.md#flowgraph-un-app-event)

# Tutorial: twitch-chat-send

Twitch のコメントを受けて、`echo> <内容>` というプレフィックスを付けて返信するエコーボット。

> ソース: [`flowgraph.example/twitch-chat-send/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/twitch-chat-send/main.flowgraph.toml) + [`conf.example-twitch.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-twitch.toml)

## ねらい

- `string.concat` / `util.rate_limit` / `twitch.chat_send` の配線を学ぶ
- `twitch.get_token` で認証済みアカウントのトークンを参照し、秘密値をグラフへ書かずに送信する

## グラフ構造

```text
ingress.twitch → util.rate_limit（max_count=20, window_ms=30000）
                 → twitch.get_token（token_key="moderator"）
                   ├ on_success → twitch.chat_send → 結果ログ
                   └ on_failure → エラーログ

本文: ingress.content → string.concat（"echo> " + 本文）→ chat_send.text
認証: get_token.access_token / client_id → chat_send
```

## 設定と実行

1. 利用するVACの「Twitchの基本設定」を保存・適用し、「Twitch接続」で投稿用アカウントを認証します。この例の `token_key` は `moderator` です。別のキーを使う場合は、そのキーに対応するアカウントの設定と認証を用意します。
2. `flowgraph.example/twitch-chat-send/` のグラフを対象プロファイルへ取り込みます。受信設定のトリガー先を、このグラフの `in` ノードへ接続します。
3. `broadcaster_id` は配信チャンネルの数値ユーザーID、`sender_user_id` は投稿用アカウントの数値ユーザーIDへ置き換えます。ログイン名やアクセストークンを入力する欄ではありません。
4. 保存して再読込します。接続設定に再起動が必要と表示された場合は、その案内に従います。
5. 対象チャンネルにコメントすると、グラフは `echo> <内容>` を送信します。投稿用アカウント自身のコメントを再入力しないよう、受信側の除外設定も確認します。

`get_token` は保存済み認証から `access_token` と `client_id` を取得します。これらの秘密値を `literal.string` に書き込む操作は不要です。認証が未完了なら `log_token_fail`、送信が失敗した場合は `log_err`、送信がスキップされた場合は `log_skip` を確認します。グラフの共有先では、その利用者自身の認証と数値IDを設定します。

このサンプルのレート制限はグラフ内で20回／30秒です。Twitch側の送信成功を保証する値ではなく、APIの結果は送信ノードのログで確認します。

## 投稿結果を確認する

イベント一覧とセッション診断ログの `twitch_chat_send_result` に、送信ノード名と結果を記録します。
`sent` はTwitch APIが送信を承認した状態、`skipped_empty` は整形後の本文が空で送らなかった状態、`failed` は入力不備や通信・API応答の失敗です。失敗の詳細はノードの `error` / `result` 出力で確認します。

この診断イベントには本文・トークン・APIのエラー本文を複製しません。Twitchの送信承認は視聴者側の表示確認とは別で、通知を受信しただけでも送信成功とは扱いません。処理中にVACが終了した場合は結果が残らないことがあります。

## よくある拡張

- `util.rate_limit` の `max_count` / `window_ms` で頻度を調整
- `string.concat` の代わりに `regex.replace` で複雑整形
- プレフィックスをコマンドで変更可能にする → [chat-filters](./chat-filters.md)

## 関連ノード

[`flowgraph.ingress.twitch`](../node-catalog.md#flowgraph-ingress-twitch) / [`flowgraph.string.concat`](../node-catalog.md#flowgraph-string-concat) / [`flowgraph.util.rate_limit`](../node-catalog.md#flowgraph-util-rate-limit) / [`flowgraph.twitch.chat_send`](../node-catalog.md#flowgraph-twitch-chat-send)

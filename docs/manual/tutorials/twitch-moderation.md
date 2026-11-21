# Tutorial: twitch-moderation

Web 経由の ad hoc トリガーから特定視聴者を 10 分 timeout する管理者向けサンプル。

> ソース: [`flowgraph.example/twitch-moderation/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/twitch-moderation/main.flowgraph.toml) + [`conf.example-twitch.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-twitch.toml)

## ねらい

- `twitch.user_id_by_login`（login → user_id 解決）と `twitch.timeout` の連携
- モデレーター専用スコープ（`moderator:manage:banned_users`）のトークン利用を確認

## グラフ構造

```text
ingress.web_input → twitch.get_token（token_key="moderator"）
                    → twitch.user_id_by_login（login=入力本文）
                      → twitch.timeout（duration_secs=600）
各段階の失敗 → 対応するログ
```

## 設定と実行

1. 「Twitch接続」で対象チャンネルのモデレーション権限を持つアカウントを認証します。この例は `moderator` キーの認証を使用します。
2. `flowgraph.example/twitch-moderation/` を対象プロファイルへ取り込みます。
3. `broadcaster_id` を対象チャンネルの数値ユーザーID、`moderator_id` を認証したモデレーターの数値ユーザーIDへ変更します。アクセストークンや `client_id` は `twitch.get_token` から渡されるため、グラフへの直書きは不要です。
4. Web入力を作成し、そのトリガー先をこのグラフの `in` ノードへ指定します。保存・再読込し、Web入力の追加に再起動が必要と表示された場合は再起動します。
5. 作成したWeb入力のページへ対象視聴者のログイン名を投稿すると、実際に600秒のタイムアウトを要求します。画面の単なるプレビューではありません。

Web入力のURLは作成した入力のものを使います。固定の `/input` へ投稿するだけでは対象ノードへの配線になりません。入力元がTwitchチャットの場合も同様に、配信者・モデレーターの権限判定を上流に組み込みます。この例にはチャット投稿者の権限判定は含まれていません。

認証取得の失敗は `log_token_fail`、ログイン名の解決失敗は `log_resolve_err`、タイムアウトの結果は `log_timeout_ok` / `log_timeout_err` で確認します。共有先では利用者自身の認証と数値IDを用意します。

## よくある拡張

- `twitch.timeout` の代わりに `twitch.ban`（永久 BAN）に差し替え
- duration / reason を上流で動的に組み立て
- トリガー元を `ingress.twitch`（特定コマンド `/timeout <login>`）に変える

## 関連ノード

[`flowgraph.twitch.user_id_by_login`](../node-catalog.md#flowgraph-twitch-user-id-by-login) / [`flowgraph.twitch.timeout`](../node-catalog.md#flowgraph-twitch-timeout) / [`flowgraph.twitch.ban`](../node-catalog.md#flowgraph-twitch-ban)

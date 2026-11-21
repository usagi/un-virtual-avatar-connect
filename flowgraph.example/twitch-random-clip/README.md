# モデレーターのランダムクリップ紹介

`!rc ユーザー名` または `!randomclip ユーザー名`。配信者本人か、そのチャットのモデレーターだけが実行できる。対象はそのユーザーが作成したクリップではなく、そのユーザーの配信を切り抜いたクリップ。

## Flowgraph

IRC ingress → `twitch.clip_command` → 保存済みトークン取得・検証 → `twitch.random_clip` → `bos.output` と `twitch.chat_send`。

- `clip_command`: IRCが生成したmetaの送信者ID・配信先ID・モデレーターバッジで判定する。表示名や本文の自己申告は使わない。無権限・無関係な発言は無視。ノード存続中の最大4096メッセージIDを重複抑止し、チャンネルごと5秒以内の追加要求を無視する。
- `random_clip`: 任意の有効なOAuthでユーザーとクリップを取得する。既定は視聴数順の上位100件から均等抽選。`max_pages` を1〜10にすると最大1000件。全クリップからの均等抽選ではない。削除済み・非公開・取得不能なものは再生できない。
- 成功時: `clip` JSON、表示用 `content`、紹介用 `chat_text`、BOS用 `meta`/`flags`。失敗時: `on_error` と説明文。投稿するか、画面だけに出すかは配線で選択する。
- `broadcaster` は受信用token key、`moderator` はAPIとチャット投稿用token key。`moderator`には`user:write:chat`が必要。例のIRC login/channelsは利用者のチャンネルに設定する。既存IRC ingressがある場合はそこから分岐し、二重接続を避ける。

## OBS

通知先は `stream-alerts`。Webサーバーの `/obs-template/effect?channel=stream-alerts&limit=100` をブラウザソースで表示する。クリップIDだけをプレイヤーURLに組み立て、任意URLをiframeへ通さない。タイトルはHTMLエスケープする。

通知とクリップは順番に表示。ページ起動前の要求は再生せず、同じ要求をpollごとに再生し直さない。動画尺＋5秒でiframeを破棄する。Twitchのクリップ埋め込みは終了イベントを公開していないため、実際の再生完了との同期ではない。読込遅延が5秒を超えると末尾が欠ける可能性がある。

## 検証中の外部不具合

2026-10-15、実クリップは再生したが `muted=false` と音付きautoplay許可の両方を指定してもミュートになる現象を再現した。現在の公式プレイヤー経路を、音付き自動再生の完了品として扱わない。

- [公式クリップ埋め込み仕様](https://dev.twitch.tv/docs/embed/video-and-clips/#non-interactive-iframes-for-clips)
- [同症状の開発者報告](https://discuss.dev.twitch.com/t/clip-embed-starts-muted-even-with-muted-false-when-unmuted-autoplay-is-allowed-aborterror-on-quality-switch/65077)

## 導入後の確認

1. 利用するVACで構成を保存・適用し、受信用と投稿用のTwitch接続を確認する。AIの設定は不要。
2. OBSのブラウザソースを表示した状態で、配信者またはモデレーターが `!rc 対象のログイン名` を投稿する。公開クリップがあるチャンネルを指定する。
3. 紹介文がチャットへ届き、OBSに対象チャンネルのクリップが一度だけ表示されることを確認する。音付き自動再生は上記の未解決事項と分けて確認する。

他のVAC・プロファイルへ構成を共有しても、Twitchの認証情報やOBSのブラウザソースは一緒に設定されない。取込先でも `broadcaster` / `moderator` の接続と、表示先の `stream-alerts` を確認する。

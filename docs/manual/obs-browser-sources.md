# OBSブラウザソース

UNVAC本体はOBSのブラウザソース向けに、透過背景の表示ページを配信する。Live画面の「BOS / OBS出力プレビュー」で見た目を確認し、生成されたURLをOBSへ登録する。

## 配信用の基本構成

UNVACが`127.0.0.1:46000`で動作している場合の推奨URL:

| 用途 | URL |
|---|---|
| 配信者とAIの会話 | `http://127.0.0.1:46000/obs-template/conversation?channels=user%2Cai-synth&theme=minimal&poll_ms=200&limit=4&ttl_ms=60000` |
| 最新発話だけの字幕 | `http://127.0.0.1:46000/obs-template/subtitle?channels=user%2Cai-synth&theme=minimal&poll_ms=200&limit=1&ttl_ms=30000` |
| Twitchコメント | `http://127.0.0.1:46000/obs-template/chat?channel=chat&theme=minimal&poll_ms=300&limit=8&ttl_ms=60000` |
| RAID・コメント強調・画面フラッシュ | `http://127.0.0.1:46000/obs-template/effect?channels=effect%2Cchat&poll_ms=100&ttl_ms=15000` |

`conversation`と`subtitle`を同時に置く必要はない。配信レイアウトに合う一方を選ぶ。AIだけ、配信者だけを別々に配置する場合は、`channels`の代わりに`channel=ai-synth`または`channel=user`を指定する。

## パラメーター

| 名前 | 意味 |
|---|---|
| `channel` | 1つのUNVACチャンネル |
| `channels` | カンマ区切りの複数チャンネル。最大8件を時刻順に統合する |
| `theme` | `default`または背景を抑えた`minimal` |
| `poll_ms` | 更新間隔。100から60000ms |
| `limit` | 保持する表示件数。1から100件 |
| `ttl_ms` | 発話を表示する期間。1000から300000ms |

テンプレートは短時間に同じチャンネルへ届いた同文を1件へ畳む。音声認識の途中結果は同じ表示を更新し、確定後は`ttl_ms`を過ぎると自動的に消える。配信前セルフテストの発話はOBSへ表示しない。

`chat`テンプレートは`source_actor`をTwitchユーザー名として表示する。`flowgraph:*`など内部処理の識別子は表示しない。

## OBSでの設定

OBS WebSocketを有効にしてUNVACへ接続情報を保存済みなら、Live画面だけで設定できる。

1. 「BOS / OBS出力プレビュー」で用途と外観を選ぶ。
2. 「OBSから取得」でBrowser Source、シーン、基準解像度を取得する。
3. 既存ソースを更新する場合は対象を選び「OBSへ反映」を押す。
4. 新しく作る場合は配置先シーンとソース名を指定し「新規作成」を押す。

新規作成ではOBSの基準解像度を幅と高さに使い、現在のURLを設定した有効なBrowser Sourceを対象シーンへ追加する。同名ソースは作成せず、既存ソースへの反映を案内する。

OBS WebSocketを使わない場合は、OBSでソースを追加して「ブラウザ」を選び、Live画面でコピーしたURLを指定する。幅と高さは配信キャンバスに合わせる。背景は透過のまま使え、カスタムCSSは必須ではない。

OBS WebSocketへの接続は8秒でタイムアウトする。接続が完了しない場合はOBSの「ツール」→「WebSocketサーバー設定」で、サーバーの有効化、ポート、パスワードを確認する。明示的に「切断」を押した場合は接続喪失警告を表示しない。

OBS側が古い内容を保持している場合は、ブラウザソースの「現在のページのキャッシュを更新」を実行する。UNVACを再起動した場合もURLは変わらない。

# v1 → v2 移行ガイド

現在のVAC v2・β版同士の更新は[更新・バックアップ・復元](./update-and-recovery.md)を参照してください。このページは旧 `[[processors]]` 構成からFlowgraphへの変換用です。以下のv0.9.x・v0.10.0は内部構成の移行が始まった当時の版番号で、今回の配布版番号を示すものではありません。

v0.9.x 系（V1 processor + Flowgraph 並走）から v0.10.0（Flowgraph-only、v2）への移行手順を
まとめる。ここでの「V1」は `[[processors]]` と V1 Control API、「V2」は `flowgraph_dir`
配下の Flowgraph Runtime を指す。

## 概要

v0.10.0 では **V1 processor 層が完全撤去**された。`[[processors]]` は TOML には残っても
無視され、起動時に deprecation 警告が出る。主な処理の移行先は下表の Flowgraph ノードです。
ノードが存在するだけでは、旧構成と同じ接続・出力・認証・動作になったことの確認にはなりません。
退避した旧構成と照合し、使用する経路ごとに入力から出力まで確認してください。

| v1 の feature                  | v2 での代替                              | 備考                                                      |
| ------------------------------ | ---------------------------------------- | --------------------------------------------------------- |
| `feature = "twitch"`           | `flowgraph.ingress.twitch` / `twitch_eventsub` | `token_key` 指定で DCF 永続トークンを使う。             |
| `feature = "twitch-out"`       | `flowgraph.twitch.chat_send`             | モデレーターボット発話も兼ねる。                          |
| `feature = "command"`          | `flowgraph.command.match` + `flowgraph.command.set` | scene switcher は `command.set` の `sets` prop で表現。 |
| `feature = "dictionary-command"` | 権限判定・構文照合・`glossary.learn` / `forget`・競合確認付き保存 | 単一のTwitch入力などから指定辞書への接続に対応。未対応経路は移行エラー。 |
| `feature = "coeiroink"`/`voicevox`/`aivis_speech`/`bouyomichan`/`os_tts` | `flowgraph.tts.speak` | `engine = "coeiroink"` 等で切り替え。 |
| `feature = "gas-translation"`  | `flowgraph.translate.gas`                |                                                           |
| `feature = "libre-translation"`| `flowgraph.translate.libre`              |                                                           |
| `feature = "voice"`            | `flowgraph.ingress.voice`                | Vosk / Whisper worker は Flowgraph ingress node から起動。 |
| `feature = "screenshot"`/`ocr` | `flowgraph.screenshot.capture` + `flowgraph.ocr.recognize` |                                               |
| `feature = "modify"`           | `flowgraph.table.load_tsv` + `flowgraph.glossary.replace` | 移行ツールは辞書を同梱TSVへコピー。接続の対応範囲は下記参照。 |

## 手順

### 1. バックアップ

移行対象を使用するVACをすべて終了してから、次を別フォルダーへコピーします。同じ設定を複数VACで使用している場合は、その全個体が対象です。

- 使用中のEXEと同梱ファイルを含む、元の配布フォルダー。
- 管理プロファイルを使う場合は、**プロファイルストア全体**。Windowsの通常の保存先は `%APPDATA%\UN Virtual Avatar Connect\` です。`UN_VAC_CONFIG_DIR` を指定して起動している場合は、その指定先を退避します。GUIの「管理プロファイル」で個別の保存先を確認できますが、`profiles\<ID>\` だけのコピーではストア全体の退避になりません。
- 明示した `conf.toml` で起動している場合は、その設定のフォルダー全体。管理プロファイルも併用している場合は、上記ストアも別途退避します。
- `flowgraph_dir` が指す**使用中の構成**、参照する辞書・素材・音声等。設定フォルダー外の相対参照・絶対パス参照は、別途コピーします。`flowgraph.example/` の保存だけでは自作・編集済みの構成を退避できません。

プロファイルZIPは構成の共有・再取込用です。認証情報、編集履歴、退避済みグラフ等は含まれないため、元の環境へ戻すためのフォルダー退避とは区別してください。資格情報を含む退避データは公開・共有用ZIPに入れず、手元で保管します。共有のOAuth保存先は移行作業中に変更しません。

ストア内の `settings.toml` は次回起動対象・初回案内の完了状態・マイクのミュート状態を保持します。`profiles/` は各プロファイル、`deleted-profiles/` は削除済みプロファイルの復元用データです。存在するものをまとめて退避してください。複数のストアを使い分けている場合は、起動方法と保存先の対応も残します。

元データを直接編集せず、コピーした作業用構成で以下の移行を試します。管理プロファイルのコピーで試す場合は、試験用VACの `UN_VAC_CONFIG_DIR` をコピー先に指定します。新しいEXEを別フォルダーへ置くだけでは設定は分離されず、引数なしの起動は通常のストアを使います。明示した設定で試す場合も、元の辞書・素材等を指す外部参照を確認してください。

元へ戻す場合は、次の順に操作します。

1. 戻す対象のストア・設定を使うVACをすべて終了します。ウィンドウを閉じるだけではトレイに常駐するため、アプリの終了操作を使います。
2. 移行後の設定と編集結果を、退避済みの旧設定とは別の場所に残します。
3. 退避したストアまたは設定フォルダーを元の保存先へ戻します。新旧のファイルを混ぜず、設定フォルダー外の参照ファイルも対応する版に戻します。
4. 元の配布物を、元と同じ保存先指定で起動します。プロファイル名・読み込まれた設定の場所・マイクのミュート状態を確認してから使用します。

移行後の設定だけを古いEXEへ渡さないでください。設定の復元はTwitch等ですでに実行した投稿や外部操作を取り消すものではありません。

### 2. `flowgraph_dir` を設定

```toml
# conf.toml
flowgraph_dir = "flowgraph"   # 任意のディレクトリ
```

ここに `.flowgraph.toml` ファイルを配置する。初心者は `flowgraph.example/` 配下の
サンプル（`chat-echo`, `tts-coeiroink`, `twitch-events`, `command-sets` 等）を
コピーして使うのが近道。

### 3. `[[processors]]` を Flowgraph に置換

移行ツールの `modify` 変換は、`dictionary_files` と `regex_files` を元の
`conf.toml` の場所を基準に読み、出力グラフの `assets/dictionary.tsv` へコピーします。
元ファイルは変更しません。生成グラフはTSVの読込成功後に置換し、読込失敗はエラーとしてログへ出します。
ファイルが読めない場合は移行エラーとなり、空辞書の成功扱いにはしません。

`writable_dictionary_file` または `writable_regex_file` がある場合は、
そのファイルが対応する読込一覧に含まれていることを確認したうえで、
各辞書を別の `assets/dictionary-N.tsv` にコピーします。
`dictionary-assets.json` に元の指定名、コピー先、書込対象かどうかを記録します。
生成グラフは一覧順に読み込み、`flowgraph.table.concat` で重複行を残したまま連結します。
このため、書込対象だけを更新しても、他の辞書ファイルは置き換わりません。

旧 `channel_from/channel_to` は、対応処理間では
明示的なグラフ間接続へ変換します。音声・Twitch・Web入力、辞書置換、翻訳、
スクリーンショット・OCRから、対応する後段処理やTTS・チャット送信へ接続できます。
複数の対応送信元から同じ文字列処理への入力は、型付きイベント合流へ変換します。
後段処理を送信元ごとに複製せず、各イベントを同じ処理へ渡します。
送信元の欠落や未対応処理の接続は移行エラーとして報告し、
対応する送信元だけを選んで残りを捨てることはしません。
辞書コマンドの権限判定に必要な送信者の引継ぎには、後述の制約があります。
移行エラーを無視した生成物を、そのまま配信に使わないでください。

学習・忘却の権限・保存先や、各処理の未対応パラメーターは別途移行が必要です。
`MIGRATION-NOTES.md` の警告を確認し、入力から出力まで検証してください。
書込対象の分離だけでは学習・忘却は動作しません。コマンドの権限確認、
対応する書込先への保存、同時更新の競合制御は別途配線が必要です。

権限判定には `flowgraph.command.authorize` を使用できます。
信頼できる入力元の送信者ログインを `login`、付与済みの権限フラグを `flags` に接続し、
`allowed_logins` と `allowed_flags` の両方を明示します。
一覧の定数には `flowgraph.literal.strings` を使えます。ログインは大小文字を区別せず、
フラグは完全一致でOR判定します。両方の許可一覧を明示的に空にすると全許可です。
判定後の更新処理は `on_allowed` につなぎます。
このノード自体は認証を行いません。チャット本文や自己申告された名前を
送信者の認証情報の代わりに使わないでください。

`flowgraph.example/dictionary-managed-command/` は、許可判定、構文照合、
学習・忘却、実ファイル保存をつないだ構成例です。
`学習(X:=Y)` / `learn(X=Y)`、`忘却(X)` / `forget(X)`、
`forget(X=Y)` を扱います。半角・全角括弧を使えます。
`forget(X)` は直前の登録を一件取り消し、完全一致指定も一件を削除します。
ロック行は保持します。これは旧版の全一致削除とは区別した、現行のUNDO方式です。
書込エラー・競合時には成功の案内を出さず、辞書を保持してエラーを記録します。

この例の許可ユーザー `example_moderator` と入力元は利用環境に合わせて変更します。
Web入力の自己申告名を公開チャットの認証に転用せず、Twitch等が確認した
送信者情報を渡してください。

旧`dictionary-command`からの移行では、忘却の意味も引き継ぎます。
`forget(X)`はXに一致する行をすべて、`forget(X=Y)`はXとYの両方に一致する行をすべて削除します。
移行時だけ`glossary.forget`の`all`モードへ接続し、新規サンプルの1件UNDOは変更しません。
どちらの場合もロック行と別の語の行は保持します。

移行ツールは `target_modify_id` を一意に照合し、指定された
`writable_dictionary_file` のコピー先へ接続します。Twitchの単一入力では、
既定の `strip_chat_prefix=true` に対して受信時の元の `message_text` を使い、
権限判定には送信者情報を使います。表示用本文から名前を推定しません。
Web入力は `strip_chat_prefix=false` の構成に対応します。
応答の `channel_to` は、単一送信元の対応処理へ明示接続します。
同時に`flowgraph.bos.output`を追加し、指定された名前のブラウザー出力にも公開します。
後続のprocessorがないブラウザー専用の出力先も扱えます。空の出力先は移行エラーです。
学習・完全一致忘却・表記一致忘却の結果は型付き合流ノードでまとめ、
保存成功後または正当な変更不要の結果だけを後続へ渡します。
権限拒否・構文不一致・読込失敗・保存失敗は通常応答を発火しません。
診断用エラーは既存のLogノードへ残します。
辞書置換や翻訳を経由した単一の入力経路では、加工後の本文と、元の入力元の送信者情報を別々に接続します。
この場合は`strip_chat_prefix=false`が必要です。加工後の本文を元の未加工本文へ戻したり、
本文に現れた名前で権限を判定したりはしません。循環・未対応の経由処理は移行エラーです。
辞書コマンド直前の複数入力は、各入力の本文と元の送信者をJSONオブジェクトへまとめ、
型付きイベント合流で1件ずつ同じ権限判定・保存処理へ渡します。
辞書置換・翻訳の手前で複数入力が合流する構成でも、各イベントの本文と送信者を組にして引き継ぎます。
共通の加工処理を送信者ごとに複製せず、加工後の本文に対して元の送信者の権限を判定します。
各分岐が対応するTwitch・Web入力へたどれない場合は移行エラーです。
権限フラグを含む旧クライアントの自動変換も引き続き未対応として報告します。
旧`allowed_flags`は`ChannelDatum.flags`の照合であり、Twitchのモデレーターバッジではありません。
名前の曖昧な対象を適当に選んだり、権限を緩めて変換を成功扱いにすることはありません。

各 processor エントリに対応する Flowgraph ノードを `flowgraph_dir` に追加する。

- ingress: `flowgraph.ingress.web_input` / `voice` / `twitch` / `twitch_eventsub`、
  AI出力は`flowgraph.ingress.ai_response` / `ai_speech` / `ai_avatar_intent` / `ai_avatar_pose`を使う。
- 変換層: `flowgraph.glossary.replace` / `flowgraph.regex.replace` /
  `flowgraph.translate.*` 等でテキスト加工。
- 出口: `flowgraph.tts.speak` / `flowgraph.twitch.chat_send` /
  `flowgraph.bos.output`（browser-output / WS 配信）など。

### 4. Twitch 設定の統合

- v2 では `[twitch]` + `[twitch.eventsub]` + `[twitch.moderator]` の 3 セクション編成に
  なっている。`token_key` で「どの DCF セッションのトークンを使うか」をノードごとに選べる。
- `broadcaster_login` を `[twitch.eventsub]` と `[twitch]` で揃えると、
  Flowgraph に `flowgraph.ingress.twitch_eventsub` がある場合は V1 EventSub ループが
  自動スキップされます。`[twitch.eventsub].force_v1_loop = true` は残存する互換ループを
  強制起動する設定であり、旧 processor 処理全体を復活させるものではありません。
  移行後は Flowgraph の購読を使用し、同じ通知を二重購読しない構成にします。

### 5. AI ペルソナ（`[[ai.personas]]`）

- `[[ai.personas]]` は v2 でも常駐タスクとして残る。Flowgraph とは別ラインで動く。
- `vac_twitch_chat_say` ツールを使う場合は `custom_instructions` に行動規範を書いておくこと
  （`conf.example-openai-chat.toml` 参照）。AI が通常発話の代わりに Twitch へ直接
  打ち込んでしまう事故を防ぐ。

### 6. 公開channel経路を明示接続へ置換

`flowgraph.channel.emit`と`flowgraph.ingress.channel_subscribe`はv2公開言語から廃止済み。
Flowgraph間は通常edge、cross-file portal、型付き`[[instances]]`で接続する。AI入力は
`flowgraph.ai.observe`、字幕/WS互換境界は`flowgraph.bos.output`、TTSは
`flowgraph.tts.speak`へ直接配線する。BOSの`destination`は外部表示先であり、Flowgraph間busではない。

desktop managed profileの旧solo構成を移行する場合は、プロファイルを停止済みであることを確認し、
最初に`--dry-run`、続けて本適用を実行する。スクリプトはsibling stagingで生成と安全検査を完了し、
本適用時だけ同じ`profiles` directoryへ日時付きprofile全体backupを作ってから`flowgraph.local`を
directory単位で入れ替える。既知の旧構成と一致しない場合や安全検査失敗時はactive treeを変更しない。
資格情報・persona本文・辞書・resourceは書き換えない。

```console
python scripts/migrate-managed-flowgraph-explicit.py "C:\Users\name\AppData\Roaming\UN Virtual Avatar Connect\profiles\profile-id" --dry-run
python scripts/migrate-managed-flowgraph-explicit.py "C:\Users\name\AppData\Roaming\UN Virtual Avatar Connect\profiles\profile-id"
cargo xtask flowgraph-check --root "C:\Users\name\AppData\Roaming\UN Virtual Avatar Connect\profiles\profile-id\flowgraph.local" --require-streaming-contract
virtual-avatar-connect-cli.exe "C:\Users\name\AppData\Roaming\UN Virtual Avatar Connect\profiles\profile-id\conf.toml" --validate-conf
```

移行後は旧2 node、`flowgraph.library.*`、`[meta].library_uses`がactiveな
`*.flowgraph.toml`に0件であることを確認する。
移行helper自体のtransaction試験は
`python -m unittest discover -s scripts/tests -p "test_*.py" -v`で再実行できる。

### 7. GUI での編集

v2 では GUI の `Flowgraph Studio` で:

- ファイル一覧で対象を開き、上部の「編集中・保存対象」でファイルを確認します。
- 「ノードを追加」からノードを追加し、「設定・用途の案内」で選択したノードの設定を編集します。
  配置は画面幅に応じて変わるため、固定の右上・右下ではなく欄の名前を目印にします。
- 「保存」または Ctrl+S / Cmd+S で現在のファイルを保存します。「未保存の変更」と「保存済み」を区別します。
- Delete / Backspace で選択したノード・線を削除できます。直後の通知から取り消せます。

グラフの保存は、接続しているVACの **Flowgraph全体を再読み込み** する操作です。
ファイル一つの編集でも、他のファイルを含む稼働構成へ影響します。一時的な処理状態が
初期化される場合があるため、配信中の編集前に対象と反映範囲を確認してください。
プロファイル本体の保存・再起動とは異なります。

対応ノードの状態を保持して再読込する操作や、グラフファイルの退避・復元については
[構成・保存・復元の説明](flowgraph-composition.md)を参照してください。
状態保存は設定フォルダー全体のバックアップの代わりにはなりません。

## デバッグ

- `GET /api/v1/control/flowgraph/diagnostics`：現在の graph 診断。
- diagnostics JSON には `capability_summary`、`loaded_state_summary`、`loaded_state_snapshot` も含まれる。
  state 系の値は reload 直後の read-only metadata で、worker 実行後の live state ではない。
- GUI の Diagnostics パネルで error / warning / info 件数とメッセージを確認。
- `reload` API（`POST /api/v1/control/flowgraph/reload`）でディスクから再読込。
  bridges も自動で respawn される。web_input endpoint の追加／削除時だけは
  プロセス再起動を促す toast が出る。

## 音声エンジンの話者一覧と簡易テスト

- `--coeiroink-speakers` / `--aivisspeech-speakers` / `--voicevox-speakers` は、起動済みエンジンの話者・スタイル一覧をJSONで表示します。設定ファイルは不要です。既定の接続先は順に `http://127.0.0.1:50032`、`http://127.0.0.1:10101`、`http://127.0.0.1:50021`。接続失敗・不正な応答では終了コード1になります。
- 一覧のフィールド名・IDはエンジンの応答を保持します。COEIROINKの `speakerUuid` と `styleId` は `uuid:style_id`、VOICEVOX/AivisSpeechの `styles[].id` はその数値を、`flowgraph.tts.speak` の `voice` に指定できます。
- `--test-os-tts` は、OS音声の一覧と速度・高さ・音量の調整範囲を表示し、各音声の短いサンプルを順に再生します。実行すると音が出ます。音声がない場合や合成・再生に失敗した場合は終了コード1になります。通常の構成からOS音声を使う場合は `flowgraph.tts.speak` を利用します。

## 関連ドキュメント

- [`docs/manual/node-catalog.md`](node-catalog.md): 全ノードの入出力/プロパティ一覧
- [`docs/manual/quickstart.md`](quickstart.md): 起動からの導線
- [`CHANGELOG.md`](https://github.com/usagi/virtual-avatar-connect/blob/v2/CHANGELOG.md): 0.9.x → 0.10.0 の差分
- `flowgraph.example/`: 実働サンプル


### フラグ付きの管理入力

管理ツールから本文・送信者・権限フラグを渡す場合は、`flowgraph.ingress.control`を配置し、
`exec_out`、`content`、`source_actor`、`flags`を処理と権限判定へ明示接続します。
既存のControl API認証方針を通る`POST /api/v1/control/flowgraph/default/trigger/{node_id}`に、
例えば`{"exec_port":"exec_in","inputs":{"content":"learn(X=Y)","source_actor":"operator","flags":["dictionary.write"]}}`を送ります。
`node_id`はファイルを含む完全なノードIDをURLエンコードして指定します。
応答の`202 Accepted`は実行キューへの受理であり、権限判定の通過や辞書の保存完了を意味しません。
実行結果は、グラフに接続した結果通知やログで確認します。

ここでの送信者とフラグは、管理APIを使える呼出者から受け取る文脈情報です。
フラグ文字列自体でAPIの利用権限が得られるわけではありません。
既存の公開WebInputやTwitchチャット本文から、この管理用フラグを取り出す変更はしていません。

旧`/ingress`やWebSocketへのチャンネル名指定だけでFlowgraphを自動選択する方式には戻しません。
旧クライアントは、この明示的な対象ノードへの呼出しへ移行が必要です。
自動移行器は、`allowed_flags`付き構成について、対象辞書と権限条件を保持した
`main.control-input.toml`と、移行先ノード・リクエスト例・必要な変更を示す
`control-input-migration.json`を生成します。下書きは自動読込の対象外であり、通常の
`main.flowgraph.toml`を置き換えません。呼出し側の変更を確認できないため、移行結果にはエラーを残します。

適用時はJSONに記載されたノード宛てに、信頼できる呼出し側から認証付きで送信するよう変更し、
本文・実際の送信者・許可されたフラグを別々に渡します。リクエスト例の空のフラグは権限を与えません。
その後、下書きの対象辞書と権限を確認して、明示的に`main.flowgraph.toml`へ置き換えます。
旧チャンネルへの送信を続けるだけでは、この管理入力は発動しません。


## 旧Web入力のURLと本文形式

移行器は `web_input_path` をFlowgraph Web入力の `path`、`web_input_method` を
`method`、固定の `channel_to` を `fixed_channel` へ引き継ぎます。
パスを省略した旧構成は `/input` を保持します。GETとPOSTのJSON・TOML・MessagePackが対象です。
明示的に空欄にしたパスは旧版と同じ `/` を保持し、自動生成URLには変更しません。
GETでは旧経路と同様にクエリから本文を読み、本文形式の指定は使いません。

MessagePackは名前付きマップと旧 `InputPayload` の配列順（channel、content、is_final）を
読み取れます。本文が壊れている場合は400応答となり、Flowgraphは実行されません。
Web入力の `meta.web_input` には、呼出者が指定した `channel` と `is_final` を保持します。
GETクエリ・JSON・フォーム・TOML・MessagePackが対象で、未指定項目は補いません。
`is_final=false` と未指定を区別でき、どちらも指定しなければ `meta` は `null` です。
これは呼出者由来のデータです。固定の処理対象や `source_kind` を切り替えたり、
管理権限を与えたりするものではありません。
旧 `is_final` フラグをFlowgraphの条件へ接続する処理は自動移行されません。
本文の `channel` による動的な振り分けも自動変換されません。
固定チャンネルがない場合は警告が出るため、Flowgraphの分岐・接続を確認してください。
URLの保持だけでは、旧構成と同じ出力や処理を再現できたことにはなりません。

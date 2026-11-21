# Tutorial: stream-control

配信中の操作を Flowgraph から扱うサンプル集。command set、Twitch Helix action、OBS scene control を入口として確認する。

> ソース: [`flowgraph.example/command-sets/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/command-sets/main.flowgraph.toml) + [`flowgraph.example/twitch-stream-tools/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/twitch-stream-tools/main.flowgraph.toml) + [`flowgraph.example/vmc-obs-scene-control/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/vmc-obs-scene-control/main.flowgraph.toml)

## ねらい

- `flowgraph.command.set` で scene / pose / subtitle などの複合 cue をまとめて発火する
- Twitch stream marker / clip / chat settings update などの Helix action node を確認する
- VMC expression trigger から OBS scene を切り替える標準形を読む

## command set

`command-sets` は `/command` へ `morning` / `gaming` / `ending` を POST すると、対応する `ChannelDatum` 群をまとめて push する例。

```powershell
Invoke-RestMethod -Method Post -Uri http://127.0.0.1:57000/command -Body "gaming"
```

`channel_contents` には `obs_scene` / `vmc_pose` など任意の channel を置けます。OBS や motion へ実接続する場合は、後段で `channel_subscribe` し、OBS / UNMF / VMC node へ配線します。

## Twitch stream tools

`twitch-stream-tools` は `flowgraph.twitch.stream_marker_create` / `clip_create` / `chat_settings_update` のロード確認用サンプル。

実運用では literal の `access_token` / `client_id` / `broadcaster_id` / `moderator_id` を直接共有しないこと。`flowgraph.twitch.get_token` や `flowgraph.twitch.user_id_by_login` で解決する構成へ置き換えるのが安全。

## OBS scene control

`vmc-obs-scene-control` は VMC の `Joy > 0.7` を検出したとき、OBS WebSocket の `SetCurrentProgramScene` を呼ぶ例。

1. OBS Studio で WebSocket Server を有効化
2. `obs_url` / `obs_password` / `scene_name` literal を環境に合わせる
3. VMC sender から `properties.bind` の UDP port へ送る

この例は gesture / expression から配信画面を動かすための最小形。UNMF/Z を標準入口にする場合は [motion-unmfz](./motion-unmfz.md) の `flowgraph.ingress.unmfz` から `flowgraph.unmf.extract_blendshape` へ接続する。

## 関連ノード

[`flowgraph.command.set`](../node-catalog.md#flowgraph-command-set) / [`flowgraph.twitch.stream_marker_create`](../node-catalog.md#flowgraph-twitch-stream-marker-create) / [`flowgraph.obs.set_current_program_scene`](../node-catalog.md#flowgraph-obs-set-current-program-scene) / [`flowgraph.vmc.extract_blendshape`](../node-catalog.md#flowgraph-vmc-extract-blendshape)

### デスクトップの通常起動と別個体
通常のデスクトップ起動（設定ファイル指定なし）は、同じユーザー設定保管先で既に起動した通常個体のウィンドウを開きます。初回起動が準備中の場合も、新たな配信処理や連携アプリを重複起動せず、ウィンドウ準備後に表示要求を処理します。設定ファイルを明示した起動は独立した運用経路として維持します。別個体を使う場合は別設定と競合しない待受ポート等が必要です。これはデスクトップの受付方針であり、CLIの動作は変更しません。

### 連携アプリをモードに合わせて起動・終了する

1. 「連携」→「連携アプリ設定」で、起動コマンドと表示名を登録します。モードから起動・終了を操作するアプリには、起動中の確認に使うプロセス名も指定します。
2. 「動作モード」→「モード別のアプリ設定」で、編集するモードを選び、アプリごとに「起動」「終了要求」「最小化」「状態を維持」を選びます。「指定なし」はそのモードから操作しない指定です。
3. 「変更を確認して保存」で変更内容を確認し、まとめて保存します。アプリ詳細とモード設定は同じ下書きを共有するので、画面を行き来しても入力は残ります。
4. 実際に動作を変えるときはホームまたは動作モード画面の切替ボタンを押します。編集対象のモード選択や保存だけでは、稼働モードやアプリの起動状態は変わりません。

「VAC終了時」と「手動・モード切替で終了を要求した時」は別に設定できます。たとえば「VAC終了時は残す」「個別の終了要求には応じる」という運用が可能です。既存設定の終了方針は、自分で変更しない限り引き継がれます。

複数アプリの詳細編集と新規登録はまとめて保存できます。確認画面でキャンセルしても下書きは残ります。別の操作で保存内容が変わった場合は上書きを拒否し、入力を保持します。必要な内容を控えたうえで共有下書きを破棄し、最新内容を読み直して編集してください。下書きは画面内の一時保持であり、保存前にVACを終了しても保持されるものではありません。


### UNA・UNMもまとめて操作する例

「連携アプリ設定」でそれぞれのSupervisorを登録します。保存先は自分のインストール先を使ってください。

| 項目 | UNA | UNM |
| --- | --- | --- |
| 起動コマンド | `"D:\UN\UNAvatar\un-avatar-supervisor.exe"` | `"D:\UN\UNMotion\un-motion-supervisor.exe"` |
| 確認するプロセス名 | `un-avatar-supervisor` | `un-motion-supervisor` |
| 終了する範囲 | 登録アプリだけ | 登録アプリだけ |
| 終了方針 | 終了を要求・強制終了しない | 終了を要求・強制終了しない |

配信用に使うモードで「起動する」、終わった後のモードで「終了を要求する」を選び、保存します。モード名は自由で、「通常」「配信」に固定されません。VAC起動時には起動せずモード切替時だけ起動したい場合は、「UNVAC起動時に開始」を外します。

Renderer/Capturerを一緒に起動するプロファイルと、Supervisor終了時に子アプリも終了するかは、UNA・UNM側の設定で決めます。トレイ収納が有効なら終了要求は画面を隠す操作になり、未保存の変更があれば終了を拒否する場合があります。VACの「終了要求を送った」は終了完了の保証ではなく、稼働状態で結果を確認できます。

プロセス名は部分一致です。同じSupervisorを複数起動している場合、「登録アプリだけ」は1個体という意味ではなく、判定名に一致するプロセス全体が対象になります。個体を指定した運用には、各製品の個体向け操作を使ってください。

# Twitch チャンネルポイント連携

UNVAC 2.0 のチャンネルポイント連携は、次の責務を分離する。

1. **OAuth**: Broadcaster の資格情報を管理する。
2. **Helix**: 設定時にカスタム報酬の ID、タイトル、コスト、利用状態を取得する。
3. **EventSub Ingress**: 実行時に交換イベントと入力文を受信する。
4. **Flowgraph**: 報酬 ID ごとに会話、アバター、OBS、外部アプリなどの動作を割り当てる。

連携画面の「チャンネルポイント報酬」は明示的に「報酬を取得」を押したときだけ Helix へ問い合わせる。
OAuth 状態の初期表示を遅くしないため、自動取得は行わない。

## Flowgraph

[twitch-channel-points-observe](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/twitch-channel-points-observe/main.flowgraph.toml) は全カスタム報酬を購読し、交換者を source_actor、報酬名を本文として channel-points へ出力する最小例。

個別動作は payload.reward.id を flowgraph.json.get で取得し、報酬 UUID と比較して分岐する。タイトルはユーザーが変更できるため、実行時の識別子には使わない。

主な payload パス:

- reward.id: 報酬 UUID
- reward.title: 表示名
- reward.cost: 必要ポイント
- user_input: 視聴者の入力文
- user_id / user_login / user_name: 交換者

全報酬を同じアバター動作へ直結しない。会話を起こす報酬、配信者へ通知するだけの報酬、明示確認後に実行する報酬を Flowgraph で別々に扱う。

### 会話・アバター・OBSを同時に起動する報酬

AIへ話しかける報酬は、1回の対象分岐から次の3経路を並行して発火する。

1. `user_input`へ報酬名を示す固定prefixを付け、`flowgraph.ai.observe`へ直接配線する。入力が空でも交換文脈を失わない。
2. 固定の小さな`AvatarIntent` JSONを`flowgraph.avatar.intent_play_unmfz`へ型付きedgeで渡す。AvatarLink compositorで瞬き・口パクと合成する。
3. `flowgraph.bos.output`の`effect` destinationへ`obs.effect.flash` flagと`quick_effect_label` / `quick_effect_duration_ms` metaを付けて送る。

`flowgraph.ai.observe`と`flowgraph.bos.output`の`meta`入力はJSON objectを受ける。
`flowgraph.bos.output`の`flags`入力は文字列のJSON配列。`source_actor`を明示した場合、
`meta.source_actor`より明示値を優先する。AIとOBS effectには同じ報酬IDとroute名をmetaへ付け、
AvatarIntentは同じEventSub exec分岐とruntime traceで照合する。

視聴者が応答への対価として交換する報酬では、`flowgraph.ai.observe`のpropertyに
`request_class = "twitch_channel_points_reward"`を設定する。この分類はEventSub本文やmetaではなく
Flowgraph定義からだけ付与される。Turn Managerは必ず`respond`を選び、通常観測より実質的な回答を要求される。
`openai_model_policy.trigger_request_classes`で上位モデルへ昇格でき、
`openai_tool_policy.by_request_class`で当該報酬だけに`web_search`などを許可できる。
通常会話へ高コスト・高権限toolを広げないこと。

```toml
[nodes.properties]
persona_id = "kaltsit"
request_class = "twitch_channel_points_reward"
```

persona設定では、同じ依頼種別を昇格条件とtool許可条件へ使う。

```toml
# 対象の [[ai.personas]] 内
openai_model_policy = { model = "gpt-5.6-terra", reasoning_effort = "medium", max_output_tokens = 1200, trigger_request_classes = ["twitch_channel_points_reward"] }
openai_tool_policy = { require_strict = true, default_allowed = ["vac_ping"], by_request_class = { twitch_channel_points_reward = ["web_search"] } }
```

対象報酬はタイトルではなく`reward.id`で分岐する。fixtureではEventSub相当payloadを注入し、
AI observe、AvatarIntent再生、BOS effectのexec/data edgeを確認する。fixture実行器は実資格情報、
Renderer、OBSを持たないため、実交換による外部到達は有人release gateで別途確認する。

## 配信前チェック

EventSub workerの起動と、実際の交換イベント受信は別の状態として扱う。

- **接続済み・実イベント未確認**: 購読は開始できたが、現在のVAC起動セッションでは通知をまだ受信していない。
- **実イベント受信済み**: EventSub通知をFlowgraph triggerへ渡した。Control Eventには`twitch_event_sub_received`が残る。
- **要対応**: token、scope、購読、WebSocketなどのruntime errorがある。連携画面からOAuthと購読状態を確認する。

`twitch_event_sub_received`にはevent type、報酬ID、報酬名、交換者login、入力文字数だけを記録する。視聴者の入力本文とEventSub生payloadは複製しない。

実交換は配信中にだけ確認できる。FrostyToolsなどが通常のTwitch chatへ出す交換通知が読み上げられても、EventSub連携の成功証拠にはならない。実交換後は配信前チェックが「実イベント受信済み」へ変わり、対象Flowgraphの後続処理まで到達したことを診断画面で照合する。

会話・アバター・OBSを同時に起動する報酬では、同じ交換時刻について次を保存する。

- `twitch_event_sub_received`の報酬ID・報酬名・交換者login。
- `flowgraph.ai.observe`の報酬prefix付き入力、route meta、後続AI decision。
- `flowgraph.avatar.intent_play_unmfz`のruntime trace、target link、Rendererの受信・適用frame増分。
- `effect` channelのflag・label・durationと、OBS Browser Source上の表示。

4項目の一部だけが成功した場合は交換成功と扱わず、EventSub、Flowgraph分岐、compositor、OBS出力のどこで止まったかを分けて記録する。

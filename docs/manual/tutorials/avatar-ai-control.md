# Tutorial: avatar-ai-control

UNVAC 2.0 で AI persona に身体表現を持たせるための最小構成。

> ソース: [`flowgraph.example/ai-avatar-unmfz/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/ai-avatar-unmfz/main.flowgraph.toml) + [`flowgraph.example/avatar-pose-asset/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/avatar-pose-asset/main.flowgraph.toml) + [`flowgraph.example/twitch-command-to-avatar-expression/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/twitch-command-to-avatar-expression/main.flowgraph.toml) + [`conf.example-avatar-ai-control.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-avatar-ai-control.toml) + [`conf.example-streaming-ai-avatar.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-streaming-ai-avatar.toml) + [`conf.example-openai-tools.json`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-openai-tools.json)

## ねらい

- UNAvatar / UNMotion 側の必須改修なしに、AI を UNMF/Z motion source として扱う
- AI には直接 UNMF やファイルpathを書かせず、Turn Managerから`AvatarIntent`または許可済みpose preset名を返させる
- UNVAC が `AvatarIntent` を検証し、表情・頷き・首振り・視線の小さな動きへ変換する
- 実 UNAvatar 接続前に dry-run fixture で経路を確認する

## v2.0 の標準経路

```text
AI persona / Turn Manager
  -> VAC channel: avatar_intent / avatar_pose
  -> flowgraph.avatar.intent_play_unmfz
     または pose_catalog_lookup -> pose_asset_play_unmfz
  -> 10-60 fps の時系列UNMF/Z + neutral復帰
  -> UNAvatar Renderer
```

`ai_control` は `reference` を含む AvatarLink capability として扱う。UNAvatar profile 自体は人間用 / AI 用に分類しない。

## dry-run smoke

実 UNAvatar を起動する前に、UNVAC 単体で intent から UNMF/Z publish までを確認する。

まとめて確認する場合:

```console
cargo xtask avatar-control smoke
```

AI AvatarIntent 経路だけを確認する場合:

```console
cargo run --bin virtual-avatar-connect-cli -- --flowgraph-test-dir flowgraph.example/ai-avatar-unmfz
```

成功時は `avatar intent playback: un-motion/frame/kaltsit-ai/v1` と送信フレーム数を含む trace が出る。ここで失敗する場合、UNAvatar 側を見る前に UNVAC の Flowgraph / tool 設定を直す。

malformed intent の fallback も確認する。

```console
cargo run --bin virtual-avatar-connect-cli -- --flowgraph-test-dir flowgraph.example/ai-avatar-unmfz-invalid
```

成功時は `unknown avatar expression` を記録しつつ、neutral fallback の UNMF/Z dry-run publish に落ちる。

## 実配信用の設定

実機接続用のローカル conf / Flowgraph は xtask で作れる。

```console
cargo xtask avatar-control prepare --base-key "un-motion/frame/kaltsit-ai" --link-id "kaltsit_ai" --profile-hint "Kaltsit"
```

この時点では `dry_run.value = true` のまま。UNAvatar Renderer 側の UNMF/Z input key と一致することを確認してから、実送信用に作り直す。

生成した Flowgraph は fixture ではないため、`--flowgraph-load-dir` でロードだけ確認する。

```console
cargo run --bin virtual-avatar-connect-cli -- --flowgraph-load-dir flowgraph.avatar-ai-control.local
```

```console
cargo xtask avatar-control prepare --base-key "un-motion/frame/kaltsit-ai" --link-id "kaltsit_ai" --profile-hint "Kaltsit" --enable-publish --force
```

手で設定する場合:

1. `conf.example-avatar-ai-control.toml` を作業用 `conf.toml` のベースまたは差分として取り込む
2. `flowgraph.example/ai-avatar-unmfz/` を実運用用の `flowgraph/` へコピーする
3. `un_avatar.links.<id>.unmfz.base_key_expr` と `unmfz_base_key.value` を同じ値にする
4. UNAvatar Renderer 側の UNMF/Z input key を同じ base key にする
5. 実送信時だけ `dry_run.value = false` に変更する
6. `conf.example-openai-chat.toml` と同様に `vac_emit_avatar_intent` を persona の tool list に入れる

配信中の AI 共演者としてまとめて設定する場合は、`conf.example-streaming-ai-avatar.toml` をベースにする。この example は Twitch / runtime event を観測する persona、`vac_emit_avatar_intent` tool、`kaltsit_ai` AvatarLink、UNMF/Z base key を 1 つの conf にまとめている。

実配信用の反応 preset は [`avatar-reaction-presets.example.json`](https://github.com/usagi/virtual-avatar-connect/blob/v2/avatar-reaction-presets.example.json) をベースにする。これは runtime conf ではなく、チャネポ、slash command、voice observe などへ割り当てる AvatarIntent の調整表。AI tool と同じ `AvatarIntent` 形式なので、手動 smoke と後続の Flowgraph 分岐の両方に使える。

```console
cargo xtask avatar-control reaction --list
cargo xtask avatar-control reaction --validate-all
cargo xtask avatar-control reaction --preset channel_point_surprise --print-only
```

xtask で配信用 persona 込みの local conf を作る場合:

```console
cargo xtask avatar-control prepare --base-key "un-motion/frame/kaltsit-ai" --link-id "kaltsit_ai" --profile-hint "Kaltsit" --streaming-persona --force
```

生成、load-check、VAC 起動まで一度に進める場合:

```console
cargo xtask avatar-control start --prepare --streaming-persona --enable-publish --force
```

起動前チェックだけで止めたい場合は `--validate-only` を付ける。

実機 smoke 前に、生成、load-check、UNAvatar 実行ファイル候補、Renderer 検出、dry-run smoke、payload preview をまとめて確認する場合:

```console
cargo xtask avatar-control preflight --prepare --streaming-persona --enable-publish --force
```

実機 readiness を OK / pending / fail の一覧として確認する場合:

```console
cargo xtask avatar-control audit --prepare --streaming-persona --enable-publish --force
```

`unavatar_renderer_running` が `pending` の場合、UNVAC 側は実機直前まで通っており、次は UNAvatar Supervisor から対象 profile の Renderer を起動する段階。

実配信テストを始める前に、1 回分の記録ファイルを作る。

```console
cargo xtask avatar-control new-live-note
```

既定では `docs/roadmap/avatar-ai-control-live-runs/` に `avatar-ai-control-live-YYYYMMDD-HHmm.md` を作る。readiness、live smoke、チャネポ、AI persona の tool call 頻度、OBS 出力の結果を同じファイルへ記録する。

note 作成、readiness audit、UNAvatar Supervisor 候補表示までをまとめて始める場合:

```console
cargo xtask avatar-control start-live-test
```

作成した note には UNVAC の commit と readiness の全チェック結果が自動追記される。audit が fail または Renderer 待ちでも、その時点の判定を記録してから終了する。Supervisor も開く場合だけ `--launch-supervisor` を付ける。profile の選択と Renderer 起動は UNAvatar 側で行う。

チャネルポイント交換はローカル監査では成功判定しない。次回の実配信で交換を発生させ、note の「実ユースケース」へ時刻と結果を記録する。

UNAvatar Renderer が起動しているかを確認する場合:

```console
cargo xtask avatar-control detect
```

UNAvatar をまだ起動していない場合、候補 exe だけ確認できる。

```console
cargo xtask avatar-control find-executables
cargo xtask avatar-control find-executables --all
```

Supervisor を開いて profile から Renderer を起動する場合:

```console
cargo xtask avatar-control start-supervisor
cargo xtask avatar-control start-supervisor --launch
```

`cargo xtask avatar-control find-executables` は起動補助ではなく発見補助。`cargo xtask avatar-control start-supervisor` も profile を自動起動しない。v2.0 では profile 起動契約を UNAvatar 側へ要求せず、Supervisor または既存の起動手順で対象 profile の Renderer を起動する。

`profile_hint` / `unmfz_key_hint` は command line からの best-effort 推定であり、v2.0 では確定照合ではない。確定照合は UNAA/Z による v2.1 以降の範囲。

例:

```toml
[un_avatar.links.kaltsit_ai]
label = "Kal'tsit AI"
capabilities = ["reference", "ai_control"]
profile_hint = "Kaltsit"

[un_avatar.links.kaltsit_ai.unmfz]
base_key_expr = "un-motion/frame/kaltsit-ai"
topic_mode = "frame"
schema_major = 1
```

Flowgraph 側:

```toml
[[nodes]]
id = "unmfz_base_key"
feature = "flowgraph.literal.string"
properties.value = "un-motion/frame/kaltsit-ai"

[[nodes]]
id = "dry_run"
feature = "flowgraph.literal.bool"
properties.value = false

[[nodes]]
id = "play_intent"
feature = "flowgraph.avatar.intent_play_unmfz"
```

## 表情の強度と継続

AvatarIntentの`expression`は`intensity`が開始時の強度、`duration_ms`が表情を出す時間を表す。`curve = "hold"`はその間の強度を維持し、`linear`（省略時）は開始直後から直線的に、`ease_out`はさらに早く減衰する。時間だけを延ばしても、減衰型では同じ強度を維持しない。

Turn Managerは、発話に添える感情には900〜1400msの保持を目安とし、一瞬の驚きには短い減衰を選ぶよう指示する。表情を出す頻度や強度を増やすための変更ではない。既存の明示指定やcurveの計算は変えず、終了時は中立へ戻す。モデルの表情形状と配信表示サイズによって見え方が異なるため、実際の映像でも確認する。

## TTSに同期する口パク・瞬き・Iポーズ

AI共演者を読み上げる `flowgraph.tts.speak` の次の入力を設定すると、WAVのRMS包絡に合わせた口パクをUNMF/Zへ送る。

| 入力 | 推奨値 | 用途 |
|---|---|---|
| `lip_sync_base_key` | `un-motion/frame/kaltsit-ai` | 対象Rendererと同じUNMF/Z base key |
| `lip_sync_expression` | `JawOpen` | 口の開閉に使うexpression |
| `lip_sync_target_link_id` | `kaltsit_ai` | motion sourceの識別子 |
| `lip_sync_gain` | `1.0` | 口の開き方。`0.1..4.0`へclamp |

`lip_sync_base_key` が空なら無効。VAC起動時には`ai_control` capabilityとUNMF/Z endpointを持つAvatarLinkをmotion controllerへ登録し、TTSを一度も実行していなくても30 fpsで自然な両目の瞬きと、左右上腕を下ろすIポーズを送る。音声再生開始時だけWAVのRMS包絡を同じframeへ加えて口パクし、終了後は口を閉じて瞬きとIポーズを維持する。

controllerは`lip_sync_target_link_id`ごとにpublisher、sequence、口パク包絡、瞬き時計、AvatarIntentを分離する。同じlink IDのbase keyが変わった場合はそのlinkだけpublisherを置換し、別のAIアバターには影響させない。

AvatarIntent、保存pose、口パク、瞬き、IポーズはAvatarLink単位の単一motion compositorが1つのUNMotionFrameへ合成し、1つのpublisherから送信する。保存pose同士、AvatarIntent同士は新しいtokenが古いtaskを失効させる。保存poseとAvatarIntentは併存し、poseが腕を保持したままAvatarIntentがHeadや表情を上書きできる。口パクと瞬きは常に保護する。

## 名前付き保存pose

複雑な腕・全身姿勢は、事前にUNMotionで作成したUNMotionFrameをcatalogへ登録する。AIへ渡すのはcatalog上のpreset IDだけで、pathやUNMF本文ではない。

```text
Turn Manager avatar_pose { target_link_id, preset }
  -> avatar_pose channel
  -> flowgraph.avatar.pose_catalog_lookup
  -> catalogで許可されたpose / release / owned_bones / timing
  -> flowgraph.avatar.pose_asset_play_unmfz
  -> smoothstep + quaternion slerpでattack / hold / release
```

catalogは`schema = unvac.avatar_pose_catalog.v1`を要求する。preset IDは小文字ASCII、数字、`_`、`-`だけを許可し、絶対path、`.`、`..`、未知field、不正boneを拒否する。poseとreleaseは同じ`owned_bones`をすべて含める。資産内のface、eyes、signals、rootは再生時に除去され、表情・口パク・瞬きとの競合を避ける。手指は未指定なら従来どおり除去する。

指を使う場合、catalogに任意の`owned_fingers`配列（例：`["left.index", "left.middle"]`）を加え、lookupの同名出力をplayの同名入力へ接続する。左右は`left` / `right`、指名は`thumb` / `index` / `middle` / `ring` / `little`。poseとreleaseの双方に、指定した指のproximal・intermediate・distalの3関節をこの順で含める。各関節は有効な回転を必須とし、指の長さを変えるtranslationやscaleは受け付けない。親指も現在のUNAが扱う3スロットへ対応付ける。手首はこの指定に含めず、`owned_bones`のLeftHand / RightHandで扱う。

指定した指だけが補間・合成され、反対側の手や未指定の指は上書きしない。終了・取消時は資産のreleaseへ戻す方式であり、外部トラッキングの直前姿勢を自動復元する機能ではない。旧catalogは変更不要。

最小例は次で確認する。

```console
cargo run --bin virtual-avatar-connect-cli -- --flowgraph-test-dir flowgraph.example/avatar-pose-asset
```

Turn Managerの追加system instructionには、AvatarLink IDと使用可能なpreset名を列挙する。許可名がないprofileでは`avatar_pose`を常に`null`にする。

Live画面の配信前セルフテストにある「AIアバター > 動作を確認」は、設定済みAvatarLinkのbase keyと起動中UNAvatar Rendererのstatusを照合する。連続するstatus sampleで`unmotion_zenoh_received_frames`と`motion_applied_frames`の両方が増えた場合だけ、Readinessを確認済みにする。通常のsnapshot更新ではZenohを再照会しないため、UI pollingの負荷にはならない。

## Twitch chat 起点の preset reaction

AI 判断を挟まず、Twitch chat の slash command から固定リアクションを出したい場合は `twitch-command-to-avatar-expression` を使う。

```console
cargo run --bin virtual-avatar-connect-cli -- --flowgraph-test-dir flowgraph.example/twitch-command-to-avatar-expression
```

この例は `/avatar` のような slash command を受けると、固定の `smile + head_nod` intent を UNMF/Z dry-run publish へ流す。v2.0 では「チャネポごとの複雑な animation editor」は作らず、このような preset intent を Flowgraph 側で増やす。

channel point redemption から固定リアクションを出す場合は `channel-point-to-avatar-reaction` を使う。

```console
cargo run --bin virtual-avatar-connect-cli -- --flowgraph-test-dir flowgraph.example/channel-point-to-avatar-reaction
```

この例は `channel.channel_points_custom_reward_redemption.add` を受けると、固定の `surprised + head_nod` intent を UNMF/Z dry-run publish へ流す。実配信では `properties.channel_points_reward_id` で特定 reward に絞るか、payload の `reward.title` を使って preset を分岐する。

配信者の voice observe から軽い頷きを出す場合は `voice-observe-to-avatar-nod` を使う。

```console
cargo run --bin virtual-avatar-connect-cli -- --flowgraph-test-dir flowgraph.example/voice-observe-to-avatar-nod
```

この例は音声認識済みテキストを受けると、固定の `head_nod` intent を UNMF/Z dry-run publish へ流す。AI persona の意味判断とは別に、「聞いている」小さな身体反応だけを Flowgraph で出したい場合の最小形。

## AI に渡す操作面

v2.0 で AI に許可する標準 intent は小さく保つ。

| intent | 用途 | 備考 |
|---|---|---|
| `expression` | 笑う、困る、怒る、驚くなど | profile 互換の UNMotionFrame expression 候補へ変換する |
| `head_nod` | 同意、聞いている反応 | 顔を下へ向けて中立へ戻す。count / intensity / duration を clamp する |
| `head_shake` | 否定、制止 | 大きな首振りは避ける |
| `look_at` | 視線の向き | v2.0 では小さい角度の補助表現に留める |

AI には「配信者の発話やチャットに対する身体反応」として使わせる。長い連続モーション、全身移動、直接 UNMF patch は v2.0 の標準操作面に含めない。

`ExpressionSample.name` は UNMotion / UNMotionFrame / UNAvatar が扱う生の表情名を保持する。UNVAC は `vmc:` prefix を付加しない。`smile` などの意味的 intent は、旧来 profile 向けの `Joy` と PerfectSync profile 向けの `MouthSmileLeft` / `MouthSmileRight` のような候補へ展開する。`vmc:Joy` の入力受付は旧 UNVAC データとの読み取り互換だけに残す。

## 実配信前チェック

検証結果は [`UNVAC 2.0 AI アバター制御 smoke 記録`](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/unvac-2-avatar-ai-control-smoke.md) に残す。

- dry-run fixture が通る
- `conf.toml` の `un_avatar.links.<id>.capabilities` が `["reference", "ai_control"]`
- `base_key_expr` が conf / Flowgraph / UNAvatar Renderer で一致している
- `dry_run.value = false` にした実運用 Flowgraph を使っている
- AI persona が `vac_emit_avatar_intent` を tool として読んでいる
- 最初の実機 smoke は `expression: smile` と `head_nod` のみで行う
- 時系列再生後に自動neutral returnが届く
- 口パクを使う場合はAI用TTS nodeの`lip_sync_base_key`がRendererと一致している

VAC 起動後、AI を介さず Control API から直接 intent を投入する場合:

Renderer 起動待ち、Control API ingress 確認、表情 sequence 送信をまとめて行う場合:

```console
cargo xtask avatar-control live-smoke --sequence smile-then-neutral
```

この command は起動中 UNAvatar Renderer を待機し、VAC Control API に neutral preflight datum を通してから sequence を送る。Renderer 起動待ちを飛ばしたい場合だけ `--skip-renderer-wait` を使う。

個別に intent を投入する場合:

```console
cargo xtask avatar-control send --preset smile
cargo xtask avatar-control send --preset nod
cargo xtask avatar-control send --preset concerned-shake
cargo xtask avatar-control send --preset neutral
```

受信・適用フレーム数をRenderer runtime busから確認する場合:

```console
cargo xtask avatar-control renderer-status
cargo xtask avatar-control renderer-screenshot --pid <PID> --path tmp/avatar/renderer.png
```

`renderer-status`の受信・適用数は通信診断であり、見た目の証明ではない。姿勢、表情、カメラ内の見え方は`renderer-screenshot`または実画面で別に確認する。

実配信用 preset から投入する場合:

```console
cargo xtask avatar-control reaction --preset chat_agree
cargo xtask avatar-control reaction --preset channel_point_surprise
cargo xtask avatar-control reaction --preset moderation_warning
```

`[control_api].require_token_for_loopback = true` の場合は `--token` に Bearer token を渡す。link ID を `kaltsit_ai` 以外にした場合は `--link-id` も指定する。payload だけ確認したい場合は `--print-only` を付ける。
`neutral` は自動neutral returnとは別に、旧来表情名とPerfectSync表情名をまとめて0にする明示的な復帰・診断preset。

表情変化と neutral 復帰を続けて見る場合:

```console
cargo xtask avatar-control send-sequence --sequence smile-then-neutral
cargo xtask avatar-control send-sequence --sequence concerned-then-neutral
```

## モデルごとの表情対応

`expression_bindings`の出力先は、Rendererが公開している表情preset名である必要がある。モデルの生のモーフ名を書くだけで、Renderer側の表情一覧へ追加されるわけではない。

現在のUNAvatarの`.unavatar`読み込みでは、ARKit標準名に加え、有効なAnimator動作から得た表情名を一覧へ登録する。独自名の表情がその動作に依存するモデルでは、同じモデルファイルでもAnimator設定を省いた起動で口パクだけ動き、独自表情は動かない場合がある。検証用Rendererにも、対象プロファイルの必要なAnimator設定をコピーする。原本の通信先や保存先は流用せず、検証用に分離する。

送信ログや受信数だけで成功とせず、対応するモーフの描画値と画像で、表情の開始・口パクとの共存・終了後の0復帰を確認する。衣装や体型用のモーフまで表情として公開する変更は、この問題の対処として一括で行わない。

モデルが公開するpresetと、AIが選ぶ感情名は別のもの。標準の対応で表現が合わない場合、AvatarLinkに明示的な対応を設定できる。

```toml
[un_avatar.links.kaltsit_ai.expression_bindings.smile]
happy = 1.0

[un_avatar.links.kaltsit_ai.expression_bindings.concerned]
sad1 = 0.8

[un_avatar.links.kaltsit_ai.expression_bindings.surprised]
surprise = 1.0
```

これは当該presetを公開するモデルだけの例で、名前を自動推測する機能ではない。Rendererの公開catalogと実描画を確認してから指定する。`.unavatar`に形状が入っているだけでは外部制御できるとは限らない。VACはこの設定でモデルを加工したりpresetを公開したりしない。

設定キーは`smile` / `laugh` / `annoyed` / `concerned` / `surprised`。既存の同義語（例：`happy`、`pleased`）も対応する感情の設定を使う。設定した感情では既定の形状群を置き換え、指定重み（有限の0〜1）にAIの強度と時間曲線を掛ける。未設定の感情・リンクは従来の対応を使う。各感情は1〜16個のpresetを指定できる。

`flowgraph.avatar.intent_play_unmfz`はrequestの`target_link_id`から設定を取り、通常再生、dry run、終了、取消時のneutralへ同じ対応を使う。途中で感情を切り替える場合も未使用の設定済みpresetを0にする。純粋変換ノード`flowgraph.avatar.intent_to_unmf`はruntime設定を暗黙参照しないため、同じ対応を`expression_bindings` JSON入力へ渡す。

笑い目などの変形と瞬きが加算され、閉眼時に瞼が潰れるモデルでは、リンク直下の`blink_suppressed_expressions`へ該当presetを指定する。例えば`eye_smile1`を表情対応に登録した場合は次のように設定する。

```toml
[un_avatar.links.kaltsit_ai]
blink_suppressed_expressions = ["eye_smile1"]
```

同じリンクの既存テーブルへ項目を追加する。指定名は`expression_bindings`のpreset名との完全一致が必要。合成後の送信値だけを`1 - 瞬き強度`倍するため、開眼時にはその時点の表情へ戻り、解除済みの表情は復活しない。指定しない成分は変わらない。設定は起動時に読み込む。純粋変換ノードの出力には適用せず、VACのAvatarLink合成で適用する。

口パクと瞬きの専用preset（JawOpen、EyeBlink左右、Blink系、母音A/I/U/E/O）は対応先として拒否する。固有presetが目や口の頂点を含む場合の見た目の干渉までは名前の検証では判定できない。共存とneutral復帰を実モデルで確認する。AIの指示は従来の感情名のままでよく、モデル固有名を自由に生成させない。

## 失敗時の切り分け

| 症状 | 見る場所 |
|---|---|
| AI が tool を呼ばない | persona の tool list / instructions / model reasoning 設定 |
| `avatar_intent` が来ない | `vac_emit_avatar_intent` の tool result と ChannelDatum |
| Flowgraph が発火しない | `avatar_intent_in.properties.channels` と `require_final` |
| dry-run trace は出るが UNAvatar が動かない | `dry_run`、Zenoh 起動、UNAvatar Renderer の UNMF/Z input key |
| malformed intent で大きな動きが出る | `ai-avatar-unmfz-invalid` fixture と `flowgraph.avatar.intent_to_unmf` の validation |
| 表情が戻らない | `intent_play_unmfz`の終端frame、Renderer受信数、競合する別motion source。診断時は`neutral` intentも明示投入する |
| 口パクが動かない | AI用TTS nodeの`lip_sync_base_key`、Rendererの購読key、`lip_sync_expression` |
| 起動時のIポーズ・瞬きが動かない | AvatarLinkの`ai_control` capabilityとUNMF/Z endpoint、VAC起動時log、Renderer購読key |

## Turn Managerの要求と送出を照合する

`ai_turn_decision` の `avatar_intent_parameters` は、validation済みの要求パラメータを最大2個記録する。表情の名前・強度・時間、頷き等の種類・回数を、`observation_datum_id` と時刻で発言やVODへ対応させる。省略値の `null` はmapperの既定値を使う指定であり、実測値ではない。

`avatar_intent_requested_count` と `avatar_intent_emitted_count`、`avatar_intent_error` も確認する。パラメータが記録されていても、音声開始待ちの期限などで送出されない場合がある。送出はRendererでの適用や画面上の可視性を保証しない。これらのパラメータは `control-events.diagnostic.jsonl` にも残るが、発話本文とepisode memoryは同ログから除外される。追加前のログにはパラメータがない。

## 関連ノード

[`flowgraph.avatar.intent_play_unmfz`](../node-catalog.md#flowgraph-avatar-intent-play-unmfz) / [`flowgraph.avatar.intent_to_unmf`](../node-catalog.md#flowgraph-avatar-intent-to-unmf) / [`flowgraph.tts.speak`](../node-catalog.md#flowgraph-tts-speak) / [`flowgraph.ingress.ai_avatar_intent`](../node-catalog.md#flowgraph-ingress-ai-avatar-intent)

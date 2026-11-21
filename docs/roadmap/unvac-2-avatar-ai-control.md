# UNVAC 2.0 アバター AI 制御計画

Status: UNVAC 2.0 の範囲固定ドラフト。UNAvatar / UNMotion 側の必須改修を要求せず、Twitch 配信中の AI 共演者が UNAvatar のアバターを身体表現として使うための設計正本。

UNVAC 2.0 は、UNAvatar / UNMotion を必須従属させる統合アプリではない。UNAvatar、UNMotion、UNVAC はそれぞれ独立して十分に使えるべきであり、UNVAC 連携は高度な配信・AI 共演ユースケースとして扱う。

---

## 1. 目標ユースケース

UNVAC 2.0 でまず実現したいのは、Twitch 配信中に同席する AI 共演者である。単なるチャットボットではなく、配信者の戦友・モデレーター・アシスタントとして振る舞い、必要な場面でアバターとして反応する。

### 1.1 共演者としての反応

入力:

- 配信者の発話
- Twitch chat
- channel point redemption
- command / web input
- OBS scene / runtime mode などの配信状態

出力:

- 短い発話
- 表情
- 頷き / 首振り
- 視線 / 注目
- 待機姿勢
- 必要に応じた OBS / Flowgraph 側の効果起動

例:

```text
視聴者: 「今の判断かなり危なくない？」
AI: 「危ない。だが通した判断だ。」
AvatarIntent:
  expression = concerned 0.45 / 1200ms
  head_nod = 1 / 600ms
```

### 1.2 モデレーターとしての反応

入力:

- 荒れたコメント
- スパム傾向
- 配信者が読まない方がよいコメント
- moderation command

出力:

- 表情を硬くする
- 視線をチャット側へ向ける
- 必要なら注意文や timeout action を Flowgraph 側へ出す

v2.0 では moderation 自体の完全自動化よりも、「怪しい入力を検知して AI が控えめに反応する」範囲を優先する。

### 1.3 アシスタントとしての反応

入力:

- 配信者の作業状態
- scene / mode
- voice command
- tool result

出力:

- 次の操作の提案
- 補足説明
- 軽い肯定 / 否定 gesture
- 状況に応じた表情

### 1.4 v2.0 で諦めること

完璧な自律共演者は v2.0 の目標ではない。コストとモデル性能の制約から、次を避ける。

- AI が毎フレーム姿勢を生成する
- AI が UNMF 全仕様を常時直接操作する
- AI が自由に全身モーションを作る
- UNAvatar / UNMotion 側の大改修を前提にする
- UNVAC なしでは UNAvatar / UNMotion が使いにくくなる導線

v2.0 は「AI が配信中に身体を持つ最小安定版」として切る。

---

## 2. v2.0 の基本方針

UNVAC 2.0 では AI を「UNAvatar を操作する外部リモコン」ではなく、**UNMF/Z motion source** として扱う。

```text
AI / Flowgraph / Twitch ingress
  -> AvatarIntent
  -> UNVAC motion mapper
  -> UNMF
  -> UNMF/Z publish
  -> 既存 UNAvatar Renderer の UNMF/Z input
```

通常のユーザー導線は変えない。

```text
UNMotion or VMC mocap app
  -> UNMF/Z or VMC
  -> UNAvatar Renderer
  -> OBS / window / Spout2
```

UNVAC はこの本線に無理に割り込まない。AI 用アバターは、可能ならユーザー本人用 Renderer とは別の Renderer / profile / UNMF/Z key として用意する。

---

## 3. AvatarLink

UNAvatar profile は中立であり、人間用・AI 用という属性を UNAvatar 側へ持たせない。UNVAC 側で「その profile / instance を何に使うか」をリンクとして設定する。

### 3.1 capability trait（能力特性）

`AvatarLink` は capability 集合として扱う。

```text
AvatarLink
  reference
  ai_control
  launch
```

- `reference`: 状態参照、OBS 効果の anchor、実行中 Renderer の対応づけ。
- `ai_control`: AI が表情・姿勢・gesture を送る。`reference` を含む。
- `launch`: UNVAC が起動コマンドを知っている。v2.0 では任意。

`ai_control` は `reference` を要求する。排他的な mode ではなく trait として扱う。

### 3.2 設定例

```toml
[un_avatar.links.doctor_main]
label = "Doctor main avatar"
profile_hint = "main"
capabilities = ["reference"]

[un_avatar.links.kaltsit_ai]
label = "Kal'tsit AI avatar"
profile_hint = "kaltsit"
capabilities = ["reference", "ai_control"]

[un_avatar.links.kaltsit_ai.unmfz]
base_key_expr = "un-motion/frame/kaltsit-ai"
topic_mode = "frame"
schema_major = 1
```

`profile_hint` は v2.0 では確定 ID ではない。profile id、profile name、manifest stem、ユーザーメモのどれでもよい。確定照合は UNAA/Z 導入後の v2.1 で扱う。

### 3.3 link resolution state（リンク解決状態）

GUI / diagnostics では link の状態を分けて表示する。

- `configured`: 設定はある。
- `process_guess`: process scan で候補 Renderer を推定した。
- `unmfz_ready`: UNMF/Z publish target は構成済み。
- `unaa_z_confirmed`: 将来。Renderer が UNAA/Z で自分を広告し、確定照合できた。

v2.0 では `unaa_z_confirmed` は基本的に出ない。

---

## 4. UNAvatar profile / 実行パス取得

v2.0 では UNAvatar 側に catalog API を要求しない。信頼できる順に layered に扱う。

### 4.1 明示設定

v2.0 の正本はユーザー設定。

登録できる項目:

- UNAvatar executable path
- Renderer executable path
- Supervisor executable path
- launch command / `run_with` entry id
- profile hint
- manifest path
- group name memo
- UNMF/Z endpoint

これだけあれば、UNAvatar 側の改修なしに AI 用 UNMF/Z 出力は成立する。

### 4.2 remembered paths（記憶済みパス）

UNVAC は成功した起動や GUI で選ばれた path を cache してよい。

- executable path
- working directory
- last command line
- last matched `AvatarLink`

これは利便性のための cache であり、正本ではない。

### 4.3 process scan（プロセス走査）

UNVAC は起動中の UNAvatar Renderer を best-effort で検出してよい。

見る情報:

- exe name / path
- process id
- command line
- manifest path argument
- runtime control / status argument
- window title

制約:

- command line は外部安定契約ではない。
- Supervisor 起動と直接起動で差が出る。
- 同じ profile の複数起動は曖昧。
- profile rename / temporary manifest path に弱い。

したがって process scan は「補助」。`AvatarLink` の確定手段にはしない。

### 4.4 v2.1 以降の確定経路

UNAvatar 2.2 / UNVAC 2.1 では、Renderer が UNAA/Z で自分を広告する。

```json
{
  "app_id": "un-avatar-renderer",
  "instance_id": "renderer-1",
  "profile_id": "kaltsit",
  "profile_name": "Kal'tsit",
  "endpoints": [
    {
      "protocol": "unmfz",
      "direction": "input",
      "base_key_expr": "un-motion/frame/kaltsit-ai",
      "topic_mode": "frame",
      "schema_major": 1
    }
  ]
}
```

v2.0 ではこの future shape に合わせて `AvatarLink` を設計するだけに留める。

---

## 5. AI の身体制御モデル

AI に UNMF を常時直接書かせない。v2.0 の標準経路は、AI が小さな intent を出し、UNVAC が UNMF に変換する二層式。

```text
AI model
  -> AvatarIntent
  -> deterministic mapper
  -> UNMF
  -> UNMF/Z publish
```

### 5.1 三段階の制御

| Level | AI output | UNVAC の責務 | 用途 |
|---|---|---|---|
| Intent | `expression`, `gesture`, `look_at` など | UNMF 変換、schedule、blend、clamp | 通常運用 |
| Parameter | expression weight / head 値 | validate、smooth、UNMF 変換 | 精密だが安全な制御 |
| UNMF patch | UNMotionFrame 風 JSON 差分 | 厳格 validate、merge、publish | advanced / 実験 |

既定は Intent。Parameter と UNMF patch は escape hatch。

### 5.2 v2.0 の最小身体チャンネル

まず低リスクで配信上の効果が大きいものに絞る。

- 表情 weight
- blink / eye openness
- head rotation
- nod / shake
- look-at direction
- 上半身の軽い lean
- breathing / idle motion

全身 locomotion は v2.0 では扱わない。avatar 依存が大きく、AI 共演者の最小価値に対してリスクが高い。

### 5.3 AvatarIntent DTO 案

```json
{
  "target_link_id": "kaltsit_ai",
  "intents": [
    {
      "kind": "expression",
      "name": "concerned",
      "intensity": 0.45,
      "duration_ms": 1200,
      "curve": "ease_out"
    },
    {
      "kind": "head_nod",
      "count": 1,
      "intensity": 0.35,
      "duration_ms": 700
    }
  ]
}
```

validation:

- `duration_ms`: 100..5000 程度に制限。
- `intensity`: 0..1。
- unknown `kind`: reject。
- unknown expression: fallback alias があれば変換、なければ reject または `emotion_state` へ downgrade。
- 1 回の AI 応答で intent は 1-3 個程度。
- scheduler が merge し、AI が直接現在 state を上書きしない。

### 5.4 mapper（変換器）の責務

UNVAC が持つ。

- expression alias map
- neutral baseline
- current output state
- intent scheduler
- priority / merge rule
- interpolation curve
- neutral return / decay
- head / torso / expression clamp
- output cadence, 例: 30 Hz
- malformed intent の安全停止

同じ intent と timestamp なら概ね同じ UNMF を出せるようにし、fixture test 可能な determinism を保つ。

### 5.5 AI tool surface（AI に見せる操作面）

AI に見せる標準 tool は小さくする。

```text
emit_avatar_intent(target_link_id, intents[])
```

AI への指示:

- 1 応答につき 1-2 個の intent を基本にする。
- intensity は控えめにする。
- 発話内容に合う表情を選ぶ。
- nod / shake は多用しない。
- 継続 animation を AI に直接作らせない。

advanced profile だけ:

```text
emit_avatar_parameters(target_link_id, expression_weights, head, duration_ms)
emit_unmf_patch(target_link_id, patch, duration_ms)
```

### 5.6 preset（プリセット）

大きな animation library は作らない。v2.0 は小さい preset から始める。

- `neutral`
- `attentive`
- `thinking`
- `agree`
- `disagree`
- `happy`
- `concerned`
- `surprised`

`AvatarLink` ごとの tuning:

- expression aliases
- expression intensity scale
- max head yaw / pitch / roll
- idle amount
- default emotion bias
- parameter / patch control を許可するか

---

## 6. 配信用ユースケースと実装方法

### 6.1 コメントへの軽い反応

Flow:

```text
twitch.chat
  -> chat filter / importance score
  -> AI response decision
  -> emit_avatar_intent
  -> UNMF/Z publish
```

必要データ:

- comment text
- author / badges
- recent context
- whether AI replied verbally
- selected expression intent

v2.0 実装:

- low importance comment では avatar だけ反応して発話しない選択を許す。
- high importance comment では発話 + expression を組み合わせる。

### 6.2 channel point redemption

Flow:

```text
twitch.channel_point
  -> redemption kind
  -> predefined intent preset
  -> optional AI short comment
  -> UNMF/Z publish
```

例:

- redeem: `びっくりして`
- intent: `surprised`, blink, small head recoil

v2.0 では redemption ごとの full animation editor は作らない。Flowgraph で preset intent を発火する。

### 6.3 配信者発話への相槌

Flow:

```text
voice transcript
  -> AI observe
  -> no verbal response needed
  -> nod / attentive / concerned
```

重要:

- AI が毎回喋ると邪魔。
- 身体表現だけの反応が共演者らしさを作る。

v2.0 では「発話なし intent」を許す。

### 6.4 moderation cue（モデレーション合図）

Flow:

```text
twitch.chat
  -> moderation heuristic / AI classification
  -> concerned / stern expression
  -> optional moderator action
```

v2.0 では action 実行より、AI の状態表示とログを優先する。

### 6.5 OBS effect anchor（OBS 効果の基準点）

v2.0 では UNAvatar から正確な pose telemetry を読む経路は必須にしない。

代替:

- UNVAC が自分で出した intent / generated UNMF を effect anchor として使う。
- AI avatar 用 Renderer だけに限定すれば、UNVAC は出力済み head / expression state を知っている。

UNAvatar から実 pose を読むのは v2.1 以降。

---

## 7. 比較実験

### 7.1 Intent / Parameter / UNMF patch の比較

同じ 10 個の配信シナリオで比較する。

シナリオ例:

- 軽い肯定
- 否定
- 驚き
- 考え込み
- チャットへの注意
- 配信者への相槌
- ツール成功
- ツール失敗
- channel point reaction
- 無反応が適切な場面

評価:

- JSON validity
- 表情の妥当性
- 動きの滑らかさ
- 過剰反応の少なさ
- neutral return
- 小型モデルでの成功率

期待:

- 既定は Intent。
- Parameter は optional。
- UNMF patch は advanced。

### 7.2 cadence / cost（頻度とコスト）

比較:

- AI が応答ごとに 1 intent
- AI が 1-3 秒の timeline
- AI が frame-like update

期待:

- frame-like update は高コストで不安定。
- 1-3 秒 timeline が上限。
- 常時出力は local scheduler が担当。

### 7.3 expression mapping（表情変換）

比較対象:

- VRM preset expression
- ARKit / Perfect Sync 風 expression
- `.unavatar` / VRC-derived runtime action
- profile 固有名

課題:

- UNVAC が default alias dictionary を持つか。
- v2.0 は link ごとの alias 設定を必須にするか。
- unknown expression を reject するか fallback するか。

### 7.4 motion safety（モーション安全性）

検証:

- head yaw / pitch / roll clamp
- nod / shake amplitude
- expression intensity scale
- neutral return time
- stuck expression recovery

合格条件:

- violent snapping がない。
- 極端な回転が持続しない。
- intent 期限切れ後に neutral へ戻る。
- malformed AI output で motion が壊れない。

### 7.5 model capability / cost（モデル能力と費用）

比較:

- 常用小型モデル
- 強いモデル

評価:

- valid tool call rate
- intent 過多にならないか
- Twitch chat 1 時間あたり token cost
- 主観的な共演者らしさ
- 強いモデルを preset 生成だけに使えるか

方針:

- 常用は小型モデルが bounded intent を出す。
- 強いモデルは gesture / preset 設計、難しい状況判断、UNMF patch 実験に使う。

### 7.6 UNAvatar receiver smoke（受信スモーク）

最低限の実機確認:

- expression only
- head nod only
- expression + nod
- neutral return
- UNMotion が別 Renderer を動かしている状態で UNVAC AI avatar が独立して動く

確認したいこと:

- AI avatar は原則 separate Renderer / profile / key がよいか。
- UNMotion と UNVAC が同じ UNMF/Z key へ publish した場合どう見えるか。
- 同じ key に別 producer がいる場合、UNVAC が warning すべきか。

---

## 8. Flowgraph integration（Flowgraph 統合）

UNVAC 2.0 では Flowgraph から intent を発火できるようにする。

候補:

- `flowgraph.avatar.intent.emit`
- `flowgraph.avatar.expression`
- `flowgraph.avatar.gesture`
- `flowgraph.avatar.look_at`
- `flowgraph.avatar.intent.to_unmf`
- 既存 `flowgraph.unmfz.publish` の再利用

低レベル data plane:

- `flowgraph.unmfz.publish`
- `flowgraph.unmf.map`
- `flowgraph.unmf.filter`
- `flowgraph.ingress.unmfz`

UNAA/Z ノードは v2.0 では必須にしない。

---

## 9. UNMotion integration（UNMotion 統合）

UNMotion は motion source であり、UNVAC の必須依存ではない。

複雑な表情・ポーズについては、UNMotionのスチル画像入力とMediaPipeからUNMFスナップショットを作り、UNVACの表現プリセットとして再利用するオーサリング経路を検討する。責務境界、既存部品、比較実験、段階導入は [`unvac-unmotion-avatar-performance-authoring.md`](./unvac-unmotion-avatar-performance-authoring.md) を正本とする。

UNVAC v2.0 で可能なこと:

- UNMotion の UNMF/Z output を購読して配信効果や観測に使う。
- 通常の `UNMotion -> UNAvatar` route をそのまま残す。
- 明示設定された場合だけ launch command として扱う。

UNMotion profile/group lifecycle の所有は v2.1 以降へ送る。

---

## 10. 実装タスク

### 10.1 設定

- [x] `un_avatar.links` config model を追加。
- [x] `ai_control` が `reference` を要求する validation を追加。
- [x] link ごとの UNMF/Z endpoint 設定を追加。
- [x] executable / launch command / `run_with` 参照を任意で持てるようにする。
- [x] `profile_hint` / manifest path metadata を持てるようにする。
- [ ] AvatarLink GUI editor を追加（v2.1+）。

### 10.2 runtime（実行時）

v2.0 は runtime 内の AvatarLink registry で publish target を自動解決しない。UNAvatar 側の API 追加を要求しないため、UNMF/Z の送信先は Flowgraph の `unmfz_base_key.value` と `un_avatar.links.<id>.unmfz.base_key_expr` を明示的に同じ値へ揃える。`cargo xtask avatar-control prepare` と readiness audit はこの重複設定を実運用で迷わないようにする補助である。

- [x] 実運用用 local conf / Flowgraph 生成で `base_key_expr` と `unmfz_base_key.value` を同期。
- [x] fixture では明示 `base_key` から既存 UNMF/Z publish path へ流す。
- [x] Windows の best-effort UNAvatar Renderer process scan script を追加。
- [x] UNAvatar executable discovery script を追加。
- [x] Renderer 未起動を readiness audit で `pending` として出す。

v2.1+ follow-up:

- [ ] `State` に AvatarLink registry を追加。
- [ ] link -> UNMF/Z publish target resolution を runtime に実装。
- [ ] link resolution state を diagnostics に出す。
- [ ] Windows の best-effort UNAvatar Renderer process scan を GUI / diagnostics に接続。
- [ ] remembered executable / command-line cache を実装。
- [ ] 複数 Renderer candidate の conflict 表示を実装。
- [ ] link はあるが Renderer が未確認の場合の UI 文言を GUI に追加。

### 10.3 AI / motion

- [x] `AvatarIntent` DTO を定義。
- [x] intent validation を実装。
- [x] expression intent -> UNMF expression weights を実装。
- [x] UNMotionFrame の生 expression 名を保持し、旧来名と PerfectSync 名の compatibility candidate 展開を実装。
- [x] head nod / shake mapper を実装。
- [x] AvatarLink起動時のIポーズと自然瞬きを単一compositorへ実装。
- [ ] overlapping intents scheduler を実装。
- [x] 保存poseとAvatarIntentの二層仲裁を実装。pose同士・intent同士は置換し、poseの腕とintentのHead / 表情は同時合成。
- [x] 時系列UNMF/Z再生の終端でneutral returnを実装。
- [ ] overlapping intentを含む連続stateのdecay / release policyを実装。
- [x] UNMF/Z session を保持し、対象 key の matching subscriber を待って送信。
- [ ] advanced `unmf_patch` path を明示 flag の下に追加。

### 10.4 実験

- [ ] Intent vs Parameter vs UNMF patch 比較。
- [x] `kp.unavatar` で expression mapping compatibility smoke。
- [ ] cadence / cost 比較。
- [ ] motion safety clamp の初期値決定。
- [ ] 小型モデルと強いモデルの tool call 比較。
- [x] 実 UNAvatar profile で expression / nod / neutral return smoke。
- [x] スチル画像由来UNMFスナップショットのcapture、保存、Flowgraph replay、neutral baseline release PoC。

### 10.5 examples / docs（例と文書）

- [x] `ai-avatar-unmfz` example を追加。
- [x] AI tool `vac_emit_avatar_intent` から `avatar_intent` チャンネルへ intent を送る経路を追加。
- [x] `twitch-command-to-avatar-expression` example を追加。
- [x] `channel-point-to-avatar-reaction` example を追加。
- [x] `voice-observe-to-avatar-nod` example を追加。
- [x] `avatar-pose-asset` exampleを追加し、保存UNMF poseとrelease baselineをcompositor経由で再生。
- [x] strict pose catalog、attack / release補間、Turn Managerの名前付き`avatar_pose`発行経路を追加。
- [x] intent -> UNMF mapper の fixture test を追加。
- [x] UNAvatar 側で同じ UNMF/Z key を受ける設定手順を書く。

---

## 11. UNVAC 2.0 release criteria（リリース基準）

実機確認は [`unvac-2-avatar-ai-control-smoke.md`](./unvac-2-avatar-ai-control-smoke.md) に記録する。

- ユーザーが UNAvatar 側を改修せず `ai_control` AvatarLink を設定できる。
- UNVAC が既存 UNAvatar Renderer へ UNMF/Z で表情と head motion を送れる。
- Twitch chat / channel point / voice observe の少なくとも 1 例から avatar intent を発火できる。
- Flowgraph から表情 1 種と gesture 1 種を発火できる。
- 通常の `UNMotion -> UNAvatar` route に影響しない。
- malformed AI output は reject または neutral return へ落ちる。
- process detection は best-effort と明示される。
- UNAA/Z 必須機能は v2.1+ として文書化される。

---

## 12. Follow-up（後続）: UNVAC 2.1 / UNAvatar 2.2

v2.1 / UNAvatar 2.2 では best-effort link を confirmed link にする。

- Renderer が UNAA/Z で instance / profile / endpoints を広告する。
- Supervisor または launcher が profile/group catalog と launch command を公開する。
- Supervisor <-> Renderer private IPC は UNAA/Z transport へ移行できる。
- Renderer command は raw Rust enum 名ではなく semantic command 名で公開する。
- UNVAC は UNAA/Z で link 解決、profile 起動、Wardrobe / Spout2 / hide / camera / action 制御を行う。

transport security は Zenoh TLS/mTLS/ACL に委ねる。アプリ側の command acceptance は capability / allow-list で扱う。

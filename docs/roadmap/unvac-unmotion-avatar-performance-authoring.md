# UNVAC / UNMotion / UNAvatar 表現スナップショット連携構想

Status: 設計・比較実験ドラフト。UNVAC 2.0 の直接 `AvatarIntent -> UNMF/Z` 経路を成立させた上で、複雑な表情やポーズをスチル画像から制作・保存・再利用する方法を整理する。

この文書は [`unvac-2-avatar-ai-control.md`](./unvac-2-avatar-ai-control.md) の身体表現を拡張する検討正本である。UNVAC、UNMotion、UNAvatarの独立性は維持し、画像生成やMediaPipeを配信時の必須依存にはしない。

---

## 1. 目的

AI共演者へ身体表現を与える方法には、少なくとも次の二つがある。

1. UNVACが意味的な `AvatarIntent` を決定論的にUNMFへ変換する。
2. 表情やポーズを取った人物画像をUNMotionへ入力し、MediaPipeからUNMFスナップショットを得る。

単純な頷き、首振り、視線、口パク、瞬き、呼吸は直接生成しやすい。一方、複数関節を使う全身ポーズや、言葉だけでは指定しにくい構えは、視覚的な参照から制作する方が狙いを共有しやすい可能性がある。

採用方針は二者択一ではなく、次のハイブリッドとする。

- 直接UNMF生成は、低遅延な実行時制御と手続き的な小動作に使う。
- スチル画像とMediaPipeは、表現プリセットを制作するオフラインまたは低頻度のオーサリング手段に使う。
- 配信中は解析済みUNMFを再利用し、画像生成やMediaPipe推論を原則として要求しない。

---

## 2. 既存実装から使えるもの

UNMotionには既に次の部品がある。

- `un-motion-input-file-image`: スチル画像入力。
- MediaPipe NativeのIMAGE running mode。
- `ImageFrame -> MediaPipe Native -> post-process -> UNMotionFrame` の正式なCapturer経路。
- `coordinate_space = UNMotion` と `stream_id` を持つ正規UNMotionFrame出力。
- `cargo xtask unmf capture-frame`: UNMF/Zから条件に合う1フレームを受信し、JSONへ保存する検査コマンド。

UNAvatarには既に次の部品がある。

- UNMF/Z receiver。
- humanoid bone、face head、expressionのretarget。
- Renderer runtime status上のUNMF/Z key、受信フレーム数、適用フレーム数。

UNVACには既に次の部品がある。

- `AvatarIntent` と決定論的mapper。
- UNMF socket、UNMF/Z publish / subscribe。
- Flowgraphによる入力、変換、保存、再生経路。
- AIへ公開するbounded tool surface。

したがって最初の実験では、新しいwire protocolや独自motion schemaを作らない。既存のUNMF/ZとUNMotionFrame JSONを観測・保存形式として使う。

---

## 3. 三アプリの責務境界

### 3.1 UNMotion

UNMotion Capturerは、1入力を1本のUNMotionFrame streamへ正規化する。

- スチル画像の読込。
- MediaPipe推論。
- post-process、座標系正規化、confidence付与。
- UNMF/Zへの一回または低頻度publish。

UNMotionは次を所有しない。

- 「困惑」「警戒」などAI向けの意味分類。
- 複数プリセットの優先度、blend、TTL。
- UNVACのAI personaや配信文脈。
- UNAvatar固有profileへの最終的な見え方調整。

### 3.2 UNVAC

UNVACは表現資産と実行時演出を所有する。

- UNMFスナップショットの取得、検査、保存。
- 意味タグ、強度、用途、対象AvatarLinkとの対応づけ。
- neutralとの差分化、bone / expression mask。
- transition、hold、release、競合解決。
- 呼吸、瞬き、視線、口パクなど手続き的motionとの合成。
- AIが選べる小さな意味的操作面。

### 3.3 UNAvatar

UNAvatar RendererはUNMFをアバターへ反映する。

- humanoid retarget。
- expression compatibility。
- 描画、Spout2、OBSへの出力。
- 実際に解決されたbone / expressionと適用結果の診断。

UNAvatarは、入力UNMFがAI、MediaPipe、手動編集のどこから来たかを動作条件にしない。

---

## 4. 用語と資産モデル

### 4.1 参照画像

人間または仮想的な役者が、狙った表情・ポーズを取った画像。実写、3Dレンダリング、画像生成のいずれでもよい。runtime資産へ必ず同梱するとは限らない。

### 4.2 UNMFスナップショット

ある時点の姿勢・表情を表す1個のUNMotionFrame。単一画像由来では速度や自然な遷移を持たないため、そのまま長時間再生するmotion clipとは区別する。

### 4.3 表現プリセット

UNVACが所有する意味的な再生単位。UNMFスナップショットだけでなく、再生規則を持つ。

概念例:

```json
{
  "id": "kaltsit_guarded",
  "label": "警戒して見守る",
  "tags": ["guarded", "observing", "serious"],
  "target_link_id": "kaltsit_ai",
  "snapshot": "poses/kaltsit_guarded.unmf.json",
  "neutral_baseline": "kaltsit_neutral",
  "bone_mask": ["Hips", "Spine", "Chest", "Head", "LeftUpperArm", "RightUpperArm"],
  "expression_mask": ["BrowInnerUp", "BrowDownLeft", "BrowDownRight"],
  "transition_in_ms": 450,
  "hold_ms": 1800,
  "transition_out_ms": 600,
  "intensity": 0.7
}
```

これは現時点の永続schema決定ではない。比較実験で必要な情報を確認するための概念形である。

### 4.4 表現クリップ

複数のUNMF keyframeまたは時間サンプルを持つ再生単位。単一スナップショットからの補間で足りない身振りに使う。v2.0で汎用animation editorを作ることは目的にしない。

---

## 5. オーサリング経路

```text
表現の意図
  -> 参照画像を撮影 / レンダリング / 画像生成
  -> UNMotion file-image Capturer
  -> MediaPipe Native IMAGE inference
  -> UNMotion post-process
  -> UNMF/Z
  -> UNVACが1フレーム取得
  -> confidence / missing parts / coordinate space検査
  -> neutralとの差分化とmask調整
  -> UNAvatar実機preview
  -> 表現プリセットとして保存
```

画像生成はこの制作経路の任意手段。配信中にその都度生成する設計にはしない。事前に複数候補を生成し、人間が見た目とMediaPipe結果の両方を確認して採用する。

---

## 6. 実行時経路

```text
配信文脈 / AI判断 / Flowgraph event
  -> semantic expression preset
  -> UNVAC scheduler
  -> neutralからtransition in
  -> snapshot poseをhold
  -> lip-sync / blink / gaze / breathingをoverlay
  -> transition out
  -> UNMF/Z
  -> UNAvatar Renderer
```

AIには画像、bone quaternion、UNMF全体を常時生成させない。通常はプリセットID、強度、保持時間、注目方向程度を選ばせる。

### 6.1 motion layer仲裁とWriter境界

口パク、瞬き、基礎姿勢、AvatarIntentはAvatarLink単位のmotion compositorへ集約し、最終UNMotionFrameを単一Writerから送る。v2.0ではこの最小compositorまで実装済み。視線、gesture、snapshot poseを追加するときも同じ境界を拡張する。

合成と切替は次の規則で分ける。

- 異なるbone、異なるexpressionなど、操作する自由度が重ならないlayerは同一frameへ合成する。
- 同じ自由度でも加算可能と明示された小さなoffsetは、上限をclampした上で合成する。
- 同じbone rotation、同じexpression、全身pose同士など意味が競合するlayerは、priority、TTL、mask、attack / releaseで1つを選ぶ。
- safety / neutral復帰、ユーザーの明示操作、会話に同期するarticulation、AIのsemantic action、ambient procedural motion、base poseの順を既定priorityとする。
- 高priority layer終了時は下位layerへstep切替せず、release期間で戻す。
- layerは所有するbone / expression maskを宣言し、未所有の自由度を0で上書きしない。

v2.0の最小compositorは、VAC起動時に`ai_control` AvatarLinkごとにZenoh sessionをprewarmし、TTS未実行でも30 fps loopを開始する。`EyeBlinkLeft` / `EyeBlinkRight`の自然瞬きは約2.4〜6.5秒の揺らぎ、短い閉眼保持、低確率の二度瞬きを持つ。body baselineにはHead、左右上腕・前腕・手の決定論的Iポーズを含め、保存pose開始時にも未定義boneによるstep切替を起こさない。再生開始時だけWAVのRMS包絡から`JawOpen`を生成し、発話終了後は口を0に戻す。

各AvatarLinkはlink IDをkeyにpublisher、sequence、口パク包絡、瞬き時計、semantic layer群を独立保持する。同じlink IDのbase key変更時はそのtargetだけを置換する。保存pose同士、AvatarIntent同士は同じ分類内で新しいtokenが古い再生taskを失効させる。保存poseとAvatarIntentは併存し、poseが所有する腕・身体へAvatarIntentのHeadや表情を上位layerとして合成する。

v2.0では`JawOpen`、`EyeBlinkLeft`、`EyeBlinkRight`をambient / articulation側の保護領域とする。保存poseは宣言したboneだけを所有し、face、eyes、`left_hand` / `right_hand`ランドマーク節、signals、rootを資産から除去する。保存poseは優先度10、AvatarIntentは優先度20として、Head Intentがposeの腕を維持したまま上書きできる。外部UNMotion source、任意数のlayer、加算motionを含む一般仲裁は後続範囲とする。

---

## 7. スナップショットに必要な付帯情報

UNMotionFrame本体を改変せず、UNVAC側のsidecar metadataで次を管理する。

- asset id、label、semantic tags。
- 取得日時と生成元。
- UNMotion / MediaPipe / model / post-process profileの版。
- 元画像の参照またはhash。画像保存は任意。
- source `stream_id` とUNMF schema version。
- body / face / handsのconfidence summary。
- missing / low confidenceなbone、expression、landmark。
- neutral baseline。
- 適用するbone / expression mask。
- transition in / hold / transition out。
- intensity clamp。
- preview確認済みのAvatarLink / profile。

画像生成サービスのprompt、seed、model情報を残す場合も、runtime必須情報にはしない。

---

## 8. 直接生成と画像由来の使い分け

| 表現 | 推奨経路 | 理由 |
|---|---|---|
| nod / shake | 直接UNMF | 低次元、低遅延、決定論的 |
| look-at | 直接UNMF | runtime targetへ追従する必要がある |
| lip-sync / blink | 直接UNMFまたは音声解析 | 時系列入力が必要 |
| breathing / idle揺れ | 手続き的UNMF | 長時間反復し、画像1枚では時間情報がない |
| 複雑な上半身の構え | 画像由来スナップショット | 視覚的に指定しやすい |
| 全身ポーズ | 画像由来を初期候補にする | 多関節を手書きする負担が大きい |
| 手指の細かな形 | 比較実験後に判断 | 単眼画像の認識安定性が不明 |
| 微妙な顔表現 | expression直接指定と画像由来を比較 | profile差とMediaPipeのidentity biasがある |

画像由来スナップショットは、直接UNMF生成を置き換えるものではない。複雑な目標姿勢を得るための初期値または制作素材として扱う。

---

## 9. 現在の基盤課題

2026-07-26の `kp.unavatar` 実機試験では、UNVACから送ったnodについて次を確認した。

- Control API ingressは受理した。
- Flowgraph channel subscriptionは発火した。
- 30 fpsの時系列UNMF/Z publishへ変更した。
- Renderer runtime statusの `unmotion_zenoh_received_frames` は1回のnodで24増加した。
- `motion_applied_frames` は同じ試験で22増加した。
- VMC互換座標として生成していた誤りを修正し、AI intent由来フレームを `CoordinateSpace::UNMotion` / `Handedness::RightHanded` に変更した。

座標修正前はRenderer画面でhead motionを視認できなかった。座標修正後の目視判定は未記録。`motion_applied_frames` はフレームをretarget処理へ渡した回数であり、対象boneが実際に可視変形した保証ではない。

スチル画像由来方式も最終的には同じUNMF適用経路を通る。したがって、次をPhase 0の共通ブロッカーとして扱う。

1. 送信payload内のHead quaternionを記録する。
2. Rendererが解決したHead node indexと適用前後local transformを診断できるようにする。
3. dynamics、constraint、animator、別motion sourceによる後段上書きを確認する。
4. expressionを既知のcontrol case、固定look-atをhead rotationのcontrol caseとして比較する。
5. UNMotion実フレームとUNVAC生成フレームのheader / body Headを同じRendererで比較する。

---

## 10. 比較実験

### E0: 直接UNMF基準試験

- neutral、固定10度pitch、固定10度yaw、nod、shakeを個別送信する。
- payload、受信数、retarget先、適用後transform、画面を対応づける。
- UNMotion Capturer由来のHeadフレームをcontrolとして使う。

合格条件: 同じ意味のUNMotion由来フレームとUNVAC由来フレームが、同じboneへ同程度の回転を与える。

### E1: 既存スチル入力の一回撮影

- 実写または既存の全身画像を `un-motion-input-file-image` へ入力する。
- MediaPipe IMAGE modeで1回推論する。
- `cargo xtask unmf capture-frame` で結果を保存する。
- 保存フレームをUNVACから再送し、UNAvatarで同じ見え方になるか確認する。

合格条件: live Capturer出力と保存後のreplayが視覚的に一致する。

#### 2026-08-03 手動PoC結果

`../un-motion/tests/pose/pose-2.png` を `un-motion-input-file-image` とMediaPipe IMAGE / holisticで解析し、UNMotion Capturerの出力を保存した。保存フレームはbody 7 bones、expression 51、signal 170、confidence 0.987で、Headはyaw 19.552度、roll 14.999度だった。

UNVACの `yaw-right` を実際の `AvatarIntent -> UNMF` mapperで生成した場合はyaw 20.000度、roll 0度となった。yaw差は0.448度で同じ方向・同程度、quaternion全体の角度差15.123度は画像由来フレームのrollに対応する。同じUNAvatar Rendererへ送った画像でも、両者が同じ向きへ顔を回すことを確認した。

保存JSONをUNVACから別keyへUNMF/Z replayし、UNMotion側で再captureしたHead quaternionが完全一致することも確認した。したがって、保存と再送で数値が変質する問題はない。

一方、全身スナップショットの後にHeadだけのneutralを送っても、スナップショットが書いた腕・手のboneは残留した。`pose-I.png` から保存した全身neutral baselineを送ると通常姿勢へ復帰した。これはtransportの問題ではなく、複数の表現源が所有するboneと解除値を明示しなければならないcompositor上の問題である。

結論:

- 直接Head制御と画像由来Head制御の座標方向は一致する。
- 画像由来スナップショットのcapture、保存、UNVAC replayは技術的に成立する。
- 表現資産は適用フレームだけでなく、所有bone maskと全身neutral baselineを必須metadataとして持つ。
- Head-only neutralを全身スナップショットの解除には使わない。
- runtimeではpose適用後に明示的なrelease frameを流し、後続のblink、lip-sync、gazeへ所有権を返す。

#### 2026-08-03 Flowgraph / compositor統合

手動PoCをruntimeへ接続するため、`flowgraph.avatar.pose_asset_play_unmfz` を追加した。このノードは保存済みposeと解除用baselineをUNMF socketで受け取り、`owned_bones` に列挙されたboneだけをAvatarLink compositor上で一時所有する。

契約:

- `owned_bones` は1個以上を必須とし、不明なhumanoid bone名を拒否する。
- poseとreleaseの両方が、列挙された全boneを含まなければ実行を拒否する。
- root移動は所有しない。
- `JawOpen`、`EyeBlinkLeft`、`EyeBlinkRight` はpose assetから上書きせず、compositorの口パク・瞬きを維持する。
- 新しいsemantic motionが始まった場合は既存tokenを失効させ、古い再生が後から状態を上書きしない。
- hold後はrelease UNMFを指定時間送信してからsemantic所有権を解放する。

`flowgraph.example/avatar-pose-asset` に、MediaPipe由来の7 bone pose、決定論的neutral baseline、名前付きcatalog、ファイル読込、dry-run fixtureを追加した。fixtureは17 node、49 sample（attack 9、hold 32、release 8）、catalog / pose / release 3ファイルの実読込で成功した。

同じ例を実行中のKalt'sit Pseudo Rendererへ接続し、次を画像で確認した。

- pose適用中はHead、両UpperArm、両LowerArm、両Handが画像由来姿勢へ変化する。
- pose保持中の70枚連続撮影で、両腕を保持したまま自然瞬きの閉眼と再開眼が動作する。
- release後は両腕がIポーズ由来baselineへ戻り、前回判明したbone残留がない。

Phase 2の最初の実装として、`flowgraph.avatar.pose_catalog_lookup` は厳格schemaのcatalogから小文字ASCIIのpreset IDだけを解決し、絶対path、`.`、`..`、未知field、不正bone、過大な時間値を拒否する。`pose_asset_play_unmfz` はquaternion shortest-path slerpとsmoothstepでreleaseからposeへattackし、poseからreleaseへ戻す。

Turn Managerのstrict schemaへ`avatar_pose`を追加した。AIは許可されたpreset名とAvatarLink IDだけを返し、UNVACは`avatar_pose` channelへ`unvac.avatar_pose_preset.v1` datumを発行する。実プロファイルでは`open_hands`だけを許可し、同channelをcatalog、保存UNMF、compositorへ接続した。

別ポートの副作用限定VACから実Renderer PID 52216へ`open_hands`を投入し、`un-motion/frame/kaltsit-ai/v1`へ49 sampleを送信完了した。Rendererセルフテストでは同keyで受信+38、適用+19 frameを確認した。attack / releaseの連続撮影では変化が複数frameへ分散した。一方、Renderer windowが画面外へ一部はみ出した状態のcrop差分であり、最終的な全身演技の自然さは次回配信で確認する。

### E2: 再現性

- 同じ画像を10回処理する。
- bone rotation、expression、confidenceの分散を測る。
- IMAGE modeと同一画像を低fps streamとして処理した場合を比較する。

### E3: 画像生成された仮想役者

- neutral、警戒、困惑、考え込み、軽い肯定の候補画像を作る。
- 正面性、全身の可視性、手の遮蔽、カメラ画角を変えて比較する。
- 人間が意図一致を評価し、MediaPipe confidenceとUNAvatar previewを併記する。

### E4: runtime品質

- step切替、線形補間、ease補間を比較する。
- neutralとの差分poseにした場合の破綻を確認する。
- lip-sync、blink、gaze、breathingとのoverlay競合を確認する。

評価軸:

- 意図した表情・姿勢との一致。
- profile間の再利用性。
- 制作時間。
- runtime開始遅延。
- CPU / GPU負荷。
- jitterと再現性。
- transitionの自然さ。

---

## 11. 段階導入

### Phase 0: UNMF適用契約の証明

- 直接Head制御の不可視問題を解決する。
- Renderer診断を強化する。
- UNMotion由来とUNVAC由来の同等フレームを比較する。

### Phase 1: 手動PoC

- 既存UNMotion file-image profileを使う。
- UNMF/Zから1フレームを手動captureする。
- UNVACから保存フレームをreplayする。
- 新しい常設APIは作らない。

### Phase 2: UNVAC表現資産

- [x] 厳格catalogによるpreset選択。
- [x] bone mask、attack / hold / release、neutral return。
- [x] AvatarIntentとの二層合成。
- [x] Kalt'sit Pseudo配信profileへの`open_hands`組込み。
- [ ] Studio上のpreviewと採否。
- [ ] 複数presetの制作と実配信中の自然さ評価。

### Phase 3: UNMotion一回撮影API

手動PoCで価値が確認できた場合だけ、UNAA/Zでone-shot captureを要求できるようにする。

概念request:

```json
{
  "request_id": "uuid",
  "operation": "capture_still",
  "profile_id": "pose-authoring",
  "input": {
    "kind": "artifact",
    "content_type": "image/png",
    "artifact_ref": "..."
  },
  "publish_key": "un-motion/frame/authoring"
}
```

filesystem pathだけを共有契約にはしない。ローカルpath、bytes、artifact referenceの扱いはUNAA/Z artifact設計と揃える。

### Phase 4: 生成支援

- 画像生成prompt template。
- 候補のbatch生成。
- confidenceによる一次選別。
- 人間のpreview承認。

自動生成から自動配信までを一度に接続しない。

---

## 12. v2.0との境界

UNVAC 2.0のrelease条件は、直接 `AvatarIntent -> UNMF/Z` で最低限の表情とhead motionが動くこと。UNMotion側の改修や画像生成を必須条件にはしない。

v2.0で行う:

- Phase 0。
- 既存部品を使ったPhase 1の技術的成立確認。
- 将来schemaを固定しない実験用保存。

v2.0をブロックさせない:

- 表現資産GUIの完成。
- one-shot capture API。
- 画像生成サービス統合。
- 汎用motion clip editor。

ただし、Phase 1が小さい変更で実用価値を示した場合は、実験機能としてUNVAC v2配信profileへ限定導入できる。

---

## 13. 次の作業

- [x] Phase 0の固定pitch / yaw control testを作る。
- [ ] RendererのHead retarget診断を取得する方法を決める。
- [x] UNMotion Capturer由来Headフレームをcaptureする。
- [x] UNVACフレームとのheader / quaternion / 同一Renderer結果を比較する。
- [x] `un-motion-input-file-image` の現行profile設定と起動手順を確認する。
- [x] 1枚の既存画像からUNMFスナップショットを保存する。
- [x] 保存フレームをUNVACからreplayし、所有boneをreleaseする最小Flowgraphを作る。
- [x] `cargo xtask avatar-control replay-frame` で保存フレームと解除用baselineを検査送信できるようにする。
- [x] E1の結果からsidecar metadata最小項目を確定する。
- [x] 名前付きpreset catalogと補間transitionを実装する。
- [x] Turn Managerからraw pathを渡さずpreset IDだけを発行する。
- [x] 保存poseとAvatarIntentを同時合成する二層仲裁を実装する。
- [ ] 画像生成候補へ進むか判断する。

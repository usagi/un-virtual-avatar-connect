# TTSの受理と再生確認を区別する

`flowgraph.tts.speak` の `non_blocking=true` は、共有schedulerに処理を渡してFlowgraphの実行を返す。従来の `on_success` はこの時点で一度だけ発火し、`played=false`、`audio_path=""` を返す。字幕の表示や「話した内容」の記録には、この受理通知を使わない。

再生に合わせた出力には `on_played` と `spoken_text` を使う。

```text
ai_speech.exec_out ───> speak.exec_in
ai_speech.content ────> speak.text
ai_speech.utterance_id > speak.utterance_id
true ─────────────────> speak.non_blocking

speak.on_played ──────> subtitle.exec_in
speak.spoken_text ────> subtitle.content
```

- `on_played` はドライバ／schedulerが `played=true` を返した片だけで発火する。
- `spoken_text` はその片のTTS入力本文。返答全文や未再生の続きは含めない。
- `spoken_utterance_id` は、その片に渡された `utterance_id`。非同期の結果でも元の要求を保持する。
- 非同期処理の失敗・取り下げは `on_playback_error` と `error` へ出る。従来の受理通知 `on_success` を再発火しない。
- 保存だけ成功して再生しなかった場合は `on_played` を発火しない。
- 同期実行では従来の `on_success` と併せて、再生確認時だけ `on_played` が発火する。

VACの共有schedulerではsinkへの音声投入成功が基準で、音声全体の終了を待たない。OS等の自己再生ドライバは各ドライバの結果が基準となる。どちらも視聴者の実聴取を保証する通知ではない。非同期通知はFlowgraphのtrigger busで処理するため、他の長時間ノードがworkerを占有すると字幕側にも遅延が出る。

非同期の再開は新しいFlowgraph generationになる。字幕側の本文は `spoken_text` から配線し、元のingressの一時的な出力を再取得しない。生成済み全文を別経路で同じ字幕へ送る配線が残っていれば、再生を確認していない本文が表示されるため、経路全体を確認する。英訳もどの本文を訳すかを明示する。

この出力の追加だけで既存Flowgraphは変更されない。発話権を使う通常のstreaming返答では、音声片の再生・取り下げが落ち着いてから、実再生の本文を`ai_response`へ一度だけ確定通知する。途中の会話履歴は再生済みの本文へ更新するが、片ごとに`ai_response`を再発火しない。英訳をこの確定通知から一度だけ行う配線は維持できる。優先依頼や非streaming返答は従来の確定経路を使う。

EpisodeMemoryも実再生本文を参照する。生成した全文が再生本文と一致するまでは、未再生の結論を含み得る生成要約を使わず、元のユーザー発言を記憶の根拠として保持する。TTS直前にFlowgraphで本文を変更した場合も、元の全文を話したと推定しない。

通常返答が再生前に終了した場合、診断の `ai_activation_decision.outcome` は、新しい入力による置換なら `discarded_superseded`、配送失敗・配送待ち期限切れなど再生されず終了した場合は `discarded_undelivered` と区別する。後者だけでは合成失敗とは断定できないため、同じ発話の `tts_lifecycle` を併せて確認する。最初に確定した取り下げ理由を、後続の失敗や入力更新で上書きしない。


### 再生前の破棄段階

AIの通常返答では、`tts_lifecycle` の `observation_channel` と `observation_datum_id` が元の入力を示します。同じ起動セッションの `ai_activation_decision` とこの2項目で照合でき、音声片どうしは `utterance_id` でまとめられます。新しい入力で返答を取り消しても、このIDは元の入力を保持します。置き換えた新入力のIDではありません。通常のチャット読み上げなど、AIの入力対応が登録されていない発話では項目を省略します。追加前のログにも項目はないため、近い時刻だけで対応を断定しないでください。

`tts_lifecycle`の`dropped.error`には既存の`reason`と`wait_ms`に加え、`stage`と`segment_index`を記録します。`stage=queue`は選択前、`synthesis`は合成結果待ち、`speech_floor`は配信者の発話権解放待ち、`before_playback`は再生直前、`playback_commit`は再生確定処理です。段階は破棄が検出された場所であり、その段階で`wait_ms`全体を過ごした意味ではありません。発話本文は含めません。期限切れや後続発話による置換という`reason`を段階で上書きしません。

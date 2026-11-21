# Tutorial: voice-input

マイク → Vosk / Whisper → OS TTS のechoパイプライン。

> ソース: [`flowgraph.example/voice-input/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/voice-input/main.flowgraph.toml) + [`conf.example-voice.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-voice.toml)

## ねらい

- `ingress.voice` が音声認識結果（`is_final`）をトリガーとして受け取る動作を確認
- 最小で「マイクに話しかけると OS TTS でエコー」を実現

## グラフ構造

```
ingress.voice ─→ tts.speak (engine="os", voice="")
                       ├→ log(audio_path)
                       └→ log(error)
```

## 前提

- voice **サービス**が起動していること
  - v2 では `flowgraph.ingress.voice` ノードが voice worker を起動する
  - engine / model_path / language_code は `flowgraph.ingress.voice` の properties に書く
  - v1 時代の `[[processors]] feature = "voice"` は廃止済みで、残っていても警告のみ
- Vosk の場合: 日本語モデル（例 `vosk-model-small-ja-0.22`）をダウンロード済み、または model ID 指定で初回 DL 可
- Whisperを使う一般配布ビルド: `cargo xtask release-build --variant portable`
  - 本体、`vac-stt-worker-vulkan.exe`、`vac-stt-worker-cpu.exe`を含む。Vulkan workerを開始できなければCPU workerへ自動降格する

## 実行手順

1. `flowgraph.example/voice-input/` を `flowgraph/` にコピー
2. `flowgraph.ingress.voice` のモデルを設定する
   - `engine = "vosk"`: `model_path` に Vosk モデル
   - `engine = "whisper"`: `model_path` に Whisper GGML モデル
3. OS のマイク許可を有効化（Windows のプライバシー設定）
4. VAC 起動
5. マイクに日本語で話す → 認識結果が OS TTS で再生される

## Whisper

`engine = "whisper"`はVADで発話開始と無音終端を検出し、一発話を単一のWhisper workerで確定する。
Voskは同時起動しない。WhisperはVulkan worker、利用できなければCPU workerを逐次選択する。
`partial_channel`を設定すると、発話中の音声を同じworkerで逐次認識し、AIを発火しない字幕previewとして同じdatumを更新する。
Flowgraphへは無音終端後の確定結果だけを一度送る。

```toml
[nodes.voice_in.properties]
engine = "whisper"
model_path = "C:/models/ggml-small.bin"
source = "CABLE Output"
language_code = "ja"
partial_channel = "user"
partial_interval_ms = 800
partial_min_audio_ms = 1300
vad_threshold_db = -45
vad_endpoint_ms = 600
vad_pre_roll_ms = 400
vad_min_speech_ms = 100
```

`source` は入力デバイス表示名の部分一致です。空なら Windows の既定入力を使います。
短い語頭が欠ける場合は `vad_pre_roll_ms`、発話確定が遅い場合は `vad_endpoint_ms`、
環境音で誤作動する場合は `vad_threshold_db` を調整します。
`partial_interval_ms`はWhisper途中推論の間隔です。短くすると字幕更新は速くなりますが、発話中のGPUまたはCPU負荷が増えます。
`partial_min_audio_ms`は最初の途中推論へ渡す最小音声長です。短すぎる音声による誤認識や推論時間の急増を避けるため、既定値は1300 msです。
`0`にすると途中推論を止め、確定認識だけを実行します。途中字幕を不要とする場合は`partial_channel`も空にします。

## 応用

- `tts.speak` を voicevox に差し替え → 「マイク入力 → キャラクターの声で読み上げ」
- 途中に `dictionary.replace` を挟んで配信用語を正規化
- `flowgraph.ai.observe`へ認識結果を明示配線して「マイク → AI」を構成する。字幕は`flowgraph.bos.output`、AI側は`flowgraph.ingress.ai_response` / `flowgraph.ingress.ai_speech`から明示的に後段へ接続する（[openai-persona](./openai-persona.md)参照）。

## `speech_floor`

`speech_floor_key`を設定すると、VoskのpartialまたはWhisperのVAD発話開始からfinal後の
`speech_floor_release_delay_ms`まで発話権を保持する。AI側の
`respect_speech_floor` と同じキーを使うことで、人が話している最中の応答開始を抑制できます。

## 関連ノード

[`flowgraph.ingress.voice`](../node-catalog.md#flowgraph-ingress-voice) / [`flowgraph.tts.speak`](../node-catalog.md#flowgraph-tts-speak)

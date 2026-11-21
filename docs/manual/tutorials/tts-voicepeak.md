# Tutorial: tts-voicepeak

VoicePeak の付属 CLI `voicepeak(.exe)` で読み上げる。

> ソース: [`flowgraph.example/tts-voicepeak/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/tts-voicepeak/main.flowgraph.toml) + [`conf.example-tts.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-tts.toml)

## ねらい

- `tts.speak` の `engine = "voicepeak"` を動かす
- `endpoint` で VoicePeak CLI の実行ファイルパスを上書きできることを確認する
- `voice` が VoicePeak の narrator 名として扱われることを理解する

## グラフ構造

```text
ingress.web_input -> tts.speak (engine="voicepeak",
                                  voice="Japanese Female 1",
                                  endpoint="")
                                  -> log(audio_path / error)
```

## 前提

- VoicePeak をインストール済みで、利用する音声のライセンスが有効
- 既定パス以外にある場合は `main.flowgraph.toml` の `endpoint` literal に絶対パスを設定
- narrator 名は `voicepeak --list-narrator` で確認

## 実行手順

1. `flowgraph.example/tts-voicepeak/` を `flowgraph/` にコピー
2. `main.flowgraph.toml` の `voice` / `endpoint` を環境に合わせて差し替え
3. VAC 再起動 or Reload
4. <http://127.0.0.1:57000/input> に文字列 → VoicePeak が読み上げ

## 現時点の限界（`extra` 入力）

VoicePeak の emotion CSV や raw speed / pitch 指定は `extra: map<json>` で受ける設計。現時点では map 値を直接構築する literal node が無いため、本例では `extra` は未配線。通常の速度・ピッチは `speed` / `pitch` 入力で調整する。

## 関連ノード

[`flowgraph.tts.speak`](../node-catalog.md#flowgraph-tts-speak)

# Tutorial: tts-coeiroink

CoeiroInk で読み上げる。

> ソース: [`flowgraph.example/tts-coeiroink/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/tts-coeiroink/main.flowgraph.toml) + [`conf.example-tts.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-tts.toml)

## ねらい

- `tts.speak` の `engine = "coeiroink"` を動かす
- voice 指定が `speaker_uuid:style_id` の **2 段** になることを理解する

## グラフ構造

```
ingress.web_input ─→ tts.speak (engine="coeiroink",
                                  voice="<speaker_uuid>:<style_id>",
                                  endpoint="http://127.0.0.1:50032")
                                  ├→ log(audio_path)
                                  └→ log(error)
```

## 前提

- CoeiroInk v2 / v2.x 系が起動（既定ポート 50032）
- 使いたいキャラクター・スタイルの UUID + style_id を知っている

UUID は以下のいずれかで取得:

- CoeiroInk 本体の GUI で確認
- CoeiroInk Engine の `/v1/speakers` API で確認

## 実行手順

1. `flowgraph.example/tts-coeiroink/` を `flowgraph/` にコピー
2. `main.flowgraph.toml` の `voice` に `<uuid>:<style_id>` を設定、必要なら `endpoint` も
3. VAC 再起動 or Reload
4. <http://127.0.0.1:57000/input> に文字列

## 外部engineの起動管理

COEIROINKはUNVACとは別プロセスのため、`tts.speak`ノードが正常に読み込まれていてもengine停止中の発話は失敗する。常用プロファイルではCOEIROINKをManaged Appへ登録し、必要なRuntime Modeで起動する。

```toml
run_with = [
  {
    id = "coeiroink",
    label = "COEIROINK",
    command = "C:/path/to/COEIROINKv2.exe",
    if_not_running = "COEIROINKv2",
    start_on_launch = false,
  },
]

[modes.streaming.managed_apps]
start = ["coeiroink"]
```

`command`と`if_not_running`は実際のインストール先とプロセス名に合わせる。配信前チェックでCOEIROINKの失敗が出た場合は、連携画面で起動状態を確認し、既定endpointの`127.0.0.1:50032`が応答する状態にしてからTTSセルフテストを行う。

## 応用

- `speed` / `pitch` / `volume` を配線（CoeiroInk ドライバがスケール変換）
- 複数キャラ切替: `voice` を上流で `string.concat` / `regex.replace` 等で動的に組み立てる

## 関連ノード

[`flowgraph.tts.speak`](../node-catalog.md#flowgraph-tts-speak)

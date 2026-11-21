# Tutorial: openai-persona

AI Persona（OpenAI Responses API）を独立サービスとして運用し、その発話を Flowgraph で受けて TTS に流すサンプル。

> ソース: [`flowgraph.example/openai-persona/main.flowgraph.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/flowgraph.example/openai-persona/main.flowgraph.toml) + [`conf.example-openai-chat.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-openai-chat.toml)

> **注記 (Phase χ)**: VAC の AI ペルソナは OpenAI **Responses API (`/v1/responses`)** を使う。旧 Chat Completions (`/v1/chat/completions`) 経路は撤去済み。設定ファイル名に `-chat` が残っているのは歴史的経緯で、内容は Responses API 用。

## ねらい

- `[[ai.personas]]` を常駐サービスとして起動し、`observe.triggers` に書いたチャンネルを購読させる
- AI の応答を `channel_utterance` に書き出す経路を把握する
- Flowgraph 側で「AI の発話を TTS に流す」最小サンプルを動かす

## 前提

- OpenAI API key（`VAC_OPENAI_API_KEY` 環境変数 or `conf.toml` の `api_key`）
- conf.toml に `[[ai.personas]]` を 1 つ以上定義（`conf.example-openai-chat.toml` を参照）

例（GPT-5.6 Luna、最小）:

```toml
[[ai.personas]]
id = "main"
model = "gpt-5.6-luna"
observe.triggers = ["user"]
channel_utterance = "ai"
custom_instructions = "あなたは配信のアシスタントです..."
openai_max_output_tokens = 1024
openai_reasoning_effort = "none"
```

例（配信AI共演者）:

```toml
[[ai.personas]]
id = "main"
model = "gpt-5.6-luna"
observe.triggers = ["user"]
channel_utterance = "ai"
custom_instructions = "あなたは配信のアシスタントです..."
openai_max_output_tokens = 700
openai_reasoning_effort = "none"
openai_store = false
memory_capacity = 4
memory_overflow_summary_enabled = false
turn_management = { enabled = true }
observation_buffer = { settle_ms = 750, max_wait_ms = 5000 }
```

`turn_management`は実況を常に音声応答へ変換せず、質問への`respond`、短い`react`、
終話の`close`、沈黙する`observe`を選ぶ。Active Episodeは相談中の話題と根拠だけを保持する。
通常はoverflow要約APIを併用せず、必要な長期記憶だけを`memory_summary`へ明示する。

## Flowgraph 側の入口

AI persona は `channel_utterance = "ai"` に確定全文をemitする。Turn Managerとstreamを
有効にした場合、TTS向けの確定発話片は別の`ai.speech`へ
`speech_segment`フラグ付きでemitする。

`flowgraph.ingress.ai_speech`の`content`は現在の発話片、`utterance_content`は同じ
`utterance_id`でその時点までに確定した累積本文となる。低遅延TTSには`content`を、音声開始前から
全文へ伸びる字幕には`utterance_content`を使う。応答完了後の正規全文は従来どおり
`flowgraph.ingress.ai_response`から取得できる。

全文確定後に一括で読む構成は、Flowgraph側で
確定全文字幕へは`flowgraph.ingress.ai_response`、先行字幕と低遅延の逐次読み上げには
`flowgraph.ingress.ai_speech`を使う。
`require_flags = ["speech_segment"]`を購読し、`tts.speak`の`lane`へ
`"assistant"`を指定する。両方をTTSへ接続すると二重に読むため、音声経路にはどちらか一方だけを使う。

## 実行手順

1. `flowgraph.example/openai-persona/` を `flowgraph/` にコピー
2. `conf.toml` に `[[ai.personas]]` と OpenAI key を設定
3. VAC 起動
4. GUIの入力または`POST /api/v1/control/ingress`で`user`チャンネルへ投入
5. AIが必要な場合だけ`ai`チャンネルへ応答し、`channel_subscribe`経由でTTSが再生

音声やTTSを使わず会話制御だけを再現する場合:

```text
cargo xtask ai-turn-eval --base-url http://127.0.0.1:46000
```

標準fixtureは通常実況、相談、追加根拠、終話、直後の別相談、単発反応を順番に投入し、
行動、発話有無、Episode ID、記憶内容、AvatarIntentの要求数・送信数・拒否理由、平均/p95遅延、token/cache使用量を
`target/ai-turn-eval/`のJSONへ保存する。前半だけ確認する場合は
`--through evidence-continues`を指定する。

fixtureの各caseには`avatar_intent = "any" | "required" | "forbidden"`を指定できる。標準fixtureでは通常実況、終了直後の無関係な実況、思考待ち、抑制期間後の通常実況を`forbidden`とし、AIが発話を抑えても身体反応だけを過剰に要求する回帰を検出する。`--dry-run`はAPIを呼ばずfixture schemaだけを検査する。

## 応用

- `ai.personas.decision` で新しい観測に対する発話判定を調整
- `function_calling` を有効化し、`conf.example-openai-tools.json` の tool 群を登録（`vac_emit_effect` で GUI 上にエフェクトを出す、など）

## 関連ノード

[`flowgraph.ingress.ai_response`](../node-catalog.md#flowgraph-ingress-ai-response) / [`flowgraph.ingress.ai_speech`](../node-catalog.md#flowgraph-ingress-ai-speech) / [`flowgraph.tts.speak`](../node-catalog.md#flowgraph-tts-speak)

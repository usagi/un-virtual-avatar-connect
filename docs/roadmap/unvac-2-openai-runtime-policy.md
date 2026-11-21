# UNVAC 2.0 OpenAI ランタイム方針

## 目的

Twitch 配信中の AI 共演者を、常時高価なモデルへ固定せずに運用する。日常会話と小さな身体反応は常用モデル、調査・事実確認・モデレーション判断は上位モデルへ限定的に昇格する。

UNVAC 2.0 では既存の Responses API、SSE、function tool loop を維持し、モデル選択、tool 制約、Prompt Cache 可視化を追加する。Realtime API、WebSocket mode、server-side compaction への移行は実配信上の必要性を測定してから扱う。

## 実行ポリシー

`model` と `openai_reasoning_effort` を常用設定とする。`[ai.personas.openai_model_policy]` の channel または本文正規表現に一致した入力だけ、別の `model`、`reasoning_effort`、`max_output_tokens` を使う。

昇格条件は狭く保つ。単なる長文、感情表現、アバター反応を上位モデルへ送る理由にはしない。事実確認、外部調査、不可逆なモデレーション判断、運用者の明示要求を主対象とする。

モデル名と reasoning effort の対応は OpenAI 側で変化するため設定値として保持する。UNVAC は既知モデルの不正な組み合わせを警告して effort を外し、未知の将来モデルは API の検証へ委ねる。

## Tool 制御

同梱 `conf.example-openai-tools.json` は全 function を `strict: true` とし、object schema に `additionalProperties: false` と完全な `required` を要求する。省略可能な引数は nullable な required property として表す。

`[ai.personas.openai_tool_policy]` は通常入力で公開する tool を `default_allowed` に限定し、必要な channel だけ `by_channel` で置き換える。UNVAC は API の `allowed_tools` に加え、ローカル dispatcher でも同じ集合を検証する。通常会話から BAN やメッセージ削除を直接呼べない二重防御。

## Prompt Cache と利用量

`openai_prompt_cache_key` は persona ごとの安定値を使う。未指定時は `unvac-persona:<id>` を生成する。system instructions、tool 定義、few-shot の順序を安定させ、動的な会話履歴を後方へ置く既存構造を維持する。

Control API と GUI には persona ごとの API request 数、tool call 数、input、cache read、cache write、reasoning、output token、直近モデル、遅延、失敗数を表示する。単価計算は価格改定で陳腐化するため v2.0 本体には固定実装しない。

## 実配信評価

同一の入力記録を使い、常用モデル候補と上位モデル候補を比較する。

| 評価軸 | 判定方法 |
|---|---|
| 人格維持 | 口調違反、長文化、不要な追撃質問を記録 |
| 身体表現 | AvatarIntent の妥当性、頻度、強度、重複を記録 |
| Tool 安全性 | 許可外 tool、不要な moderation、引数 validation failure を記録 |
| 応答性 | end-to-end latency と API latency を通常・昇格別に集計 |
| コスト | input、cache read/write、reasoning、output token を配信時間当たりで集計 |
| 昇格精度 | 昇格すべき入力の取りこぼしと、不要な昇格を記録 |

2026-07-30の価格改定を受け、v2の既定モデル系列はGPT-5.6へ統一する。
常用は `gpt-5.6-luna` / `none`、一段昇格は `gpt-5.6-terra` / `medium` とする。
`gpt-5.6-sol` は不可逆なモデレーション判断、複雑な調査、運用者の明示指定に限る最上位候補とし、
現行の一段model policyでは常用しない。旧モデルIDは既存設定の互換入力としてのみ扱う。

Active Episode、
Observation Buffer、memory 4、明示Prompt Cacheを併用する。
隔離ランタイム評価では8件中8件に合格し、平均1.77秒、p95 3.03秒だった。
上位モデルへの昇格は設定だけで追加できる状態を維持し、モデルをコードへ固定しない。

## v2.0 完了条件

- strict tool schema と channel 別許可集合が起動時に検証される
- 通常・昇格モデルの選択が設定とログから確認できる
- persona ごとの cache / reasoning / output token と失敗が GUI で確認できる
- `conf.example-streaming-ai-avatar.toml` から AI 発話と AvatarIntent を実配信で試せる
- 実配信記録から v2.0 の既定候補を決定できる

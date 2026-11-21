# Virtual Avatar Connect — Manual (v2)

VAC v2 のユーザー向けマニュアル。Twitchの通知・クリップ紹介、読み上げ、AI共演者など、使いたい機能から始められます。AIを使わずに利用することもできます。

初めて使う場合は [Quickstart](./quickstart.md) の「最初に、使いたいことを選ぶ」から進んでください。既存設定の移行は [v1からの移行手順](./v1-to-v2-migration.md)、独自の処理を組む場合は [Flowgraph Composition](./flowgraph-composition.md) と [Node Catalog](./node-catalog.md) を参照してください。

本マニュアルは v2 の使い方と利用可能なノード一覧の**正本**です。v2の型付きノード・接続による実行構造は、後半の「v2 アーキテクチャの前提」で説明します。

---

## 目次

### はじめに

- [更新・バックアップ・復元](./update-and-recovery.md) — VAC v2・β版からの通常更新と旧版への復帰

- [Quickstart](./quickstart.md) — AIなしのTwitch配信支援から始める初回設定、機能の追加、保存・復元
- [conf.toml リファレンス](./conf-reference.md) — v2 配布 `conf.toml` のキー一覧
- [OBSブラウザソース](./obs-browser-sources.md) — 配信者・AI字幕とTwitchコメントをOBSへ表示する
- [AI配信知覚基盤](./ai-stream-perception.md) — Twitch宣言状態と明示OBSゲームsourceの画像観測
- [xtask](./xtask.md) — repository 内の生成、検証、実験、AI アバター実機検査 command
- [Node Catalog](./node-catalog.md) — 組み込みノードの全ポート・全プロパティ、effect / capability / state metadata の逆引き（自動生成）
- [Dimensional Quantity System](./dimensional-quantity-system.md) — SI 準拠の単位次元システム（Phase ξ）。数値に単位を貼り付け、次元不一致をエンジンで検知する
- [DateTime System](./datetime-system.md) — 絶対時刻の `datetime` 型と `flowgraph.datetime.*` ノード（Phase π）。Duration は `quantity`（時間次元）に統一
- [Flowgraph Composition](./flowgraph-composition.md) — file / module / instance、型付き graph I/O、cross-file portal、公開 channel 廃止の正本
- [UNVAC v2 active managed profile release gate](./v2-release-gate.md) — offline検証と有人実配信を分離したrelease checklist
- [Flowgraph Enum & Library](./flowgraph-enum-and-library.md) — 閉集合 `string` と旧 Phase λ の履歴（構成境界は Composition 正本を優先）
- [UNVAC 2.0 アバター AI 制御計画](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/unvac-2-avatar-ai-control.md) — UNAvatar / UNMotion 側の必須改修なしに、AI が UNMF/Z でアバターを制御する v2.0 範囲
- [UNVAC 2.0 アバター AI 制御 fix タスク](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/unvac-2-avatar-ai-control-tasks.md) — 実機 smoke と実配信テストから 2.0 fix へ戻す作業表
- [表現スナップショット連携構想](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/unvac-unmotion-avatar-performance-authoring.md) — スチル画像、UNMotion MediaPipe、UNMF資産を使う表現オーサリング設計

### Tutorials（各 `flowgraph.example/` の解説）

- [chat-echo](./tutorials/chat-echo.md)：Twitch ingress → sibling TTS の multi-file echo
- [chat-filters](./tutorials/chat-filters.md)：command.match / dictionary.replace / tts.speak
- [english-pronunciation](./tutorials/english-pronunciation.md)：英単語 → english2kana ONNX → 日本語TTS
- [ocr-screencap](./tutorials/ocr-screencap.md)：screenshot.capture → ocr.recognize
- [openai-persona](./tutorials/openai-persona.md)：AI Persona × Flowgraph の接続
- [translate-multilang](./tutorials/translate-multilang.md)：GAS と Libre の並列翻訳
- [tts-os](./tutorials/tts-os.md)：OS TTS
- [tts-voicevox](./tutorials/tts-voicevox.md)：VOICEVOX / AivisSpeech
- [tts-coeiroink](./tutorials/tts-coeiroink.md)：CoeiroInk
- [tts-voicepeak](./tutorials/tts-voicepeak.md)：VoicePeak CLI
- [tts-bouyomichan](./tutorials/tts-bouyomichan.md)：棒読みちゃん
- [twitch-echo](./tutorials/twitch-echo.md)：Twitch チャット取り込み
- [twitch-chat-send](./tutorials/twitch-chat-send.md)：rate_limit + chat_send
- [twitch-moderation](./tutorials/twitch-moderation.md)：user_id_by_login + timeout
- [stream-control](./tutorials/stream-control.md)：command set / Twitch stream tools / OBS scene control
- [voice-input](./tutorials/voice-input.md)：Vosk / Whisper 認識 → TTS echo
- [un-app-control](./tutorials/un-app-control.md)：UNAA/Z app status / safe command / event key
- [motion-unmfz](./tutorials/motion-unmfz.md)：VMC → UNMF → UNMF/Z と UNMF/Z ingress
- [avatar-ai-control](./tutorials/avatar-ai-control.md)：AI persona → AvatarIntent → UNMF/Z → UNAvatar Renderer（配信用一体設定は `conf.example-streaming-ai-avatar.toml`）
- [twitch-command-to-avatar-expression](./tutorials/avatar-ai-control.md#twitch-chat-起点の-preset-reaction)：Twitch slash command → preset AvatarIntent → UNMF/Z
- [channel-point-to-avatar-reaction](./tutorials/avatar-ai-control.md#twitch-chat-起点の-preset-reaction)：Twitch channel point → preset AvatarIntent → UNMF/Z
- [Twitch チャンネルポイント連携](./twitch-channel-points.md)：報酬一覧、EventSub、報酬別 Flowgraph
- [voice-observe-to-avatar-nod](./tutorials/avatar-ai-control.md#twitch-chat-起点の-preset-reaction)：Voice observe → preset AvatarIntent → UNMF/Z
- [dictionary-editor-and-quick-add](./tutorials/dictionary-editor-and-quick-add.md)：GUI から TSV 用語集を直接編集する / Live Quick-Add で `glossary.learn` を即発火する（Phase φ / GRN）
- [gui-e2e](./tutorials/gui-e2e.md)：Playwright で GUI の回帰テストを実行する / fixture 構成と失敗時の切り分け（Phase ν）
- [flowgraph-fixtures](./tutorials/flowgraph-fixtures.md)：CLI から Flowgraph fixture を実行し、mock IO と recorded effects で言語コアを検証する
- [un-suite-vmc-hub](./tutorials/un-suite-vmc-hub.md)：U.N. Motion → VAC → U.N. Avatar を VMC/UDP で接続する

---

## v2 アーキテクチャの前提

### 3 つの実行層

```
┌───────────────────────────────────────────────────────────┐
│ 1. Service layer（conf.toml が設定する常駐サービス）      │
│    - AI Persona（OpenAI Responses API）                    │
│    - Twitch（OAuth / EventSub / Chat Send）                │
│    - LibreTranslate / Screenshot / OCR の補助設定         │
│    - run_with（外部プロセス起動）                          │
└───────────────────────────────────────────────────────────┘
                              ↓ TriggerSource
┌───────────────────────────────────────────────────────────┐
│ 2. Flowgraph layer（flowgraph.example/*.flowgraph.toml）  │
│    - ingress.{web_input,voice,twitch} で外部入力を受ける  │
│    - Voice（Vosk / Whisper）は ingress.voice が起動        │
│    - Pure / Stateful / Effectful ノードで処理             │
│    - util.log / tts.speak / twitch.chat_send で出力       │
└───────────────────────────────────────────────────────────┘
                              ↓
┌───────────────────────────────────────────────────────────┐
│ 3. UI layer（GUI / OBS Browser Source）                    │
│    - GUI（/gui/）: 利用 / 構成 / 接続と管理                 │
│    - /obs-template/* と Control API の output surfaces     │
└───────────────────────────────────────────────────────────┘
```

### Flowgraph 用語ミニ辞書

| 用語 | 意味 |
|---|---|
| **Feature** | ノードの種類を示す文字列 ID。例: `flowgraph.tts.speak` |
| **Socket** | 入出力端子。型は `bool` / `int` / `float` / `string` / `json` / `list<T>` / `map<T>` / `table` / `quantity` / `datetime` / `exec`。`quantity` は [Dimensional Quantity System](./dimensional-quantity-system.md)、`datetime` は [DateTime System](./datetime-system.md) |
| **Port** | ノードに所属する個別のソケット（name + type + exec/data）|
| **Edge** | `from = "node_id:port"` → `to = "node_id:port"` の TOML 記述 |
| **Exec 線** | 処理の発火タイミングを伝える制御フロー（`exec_in` / `exec_out`）|
| **Property** | ノード外部では見えないが、ノード自身が使う静的設定（literal ノードの `value` など）|
| **Pure / Stateful / Effectful** | 副作用クラス。PureNode は I/O 不可、StatefulNode は状態あり I/O なし、EffectfulNode のみ I/O 可 |

### 3 つの設定ファイル

1. **`conf.toml`** — サービス設定のみ。`[[processors]]` は v2 では不可。
2. **`flowgraph.example/*.flowgraph.toml`** — DAG 本体。`[meta]` / `[[nodes]]` / `[[edges]]` の 3 種類のテーブル。
3. **`dictionary.*.txt` / `regex.*.txt`** — 辞書ファイル（v2 では flowgraph ノードの入力として渡すが、永続化用 fs ノードは δ-9）。

---

## CLI / Desktop runner

- `virtual-avatar-connect-desktop.exe` — 通常配布向け。tray 常駐 + GUI WebView から Service + Flowgraph を起動
- `virtual-avatar-connect-cli.exe` — ターミナル実行・開発・診断向け。Control API と GUI 配信も同じ core で起動
- `--coeiroink-speakers`, `--voicevox-speakers`, `--aivisspeech-speakers` — 起動済み音声エンジンの話者・スタイル一覧を表示。`--test-os-tts` はOS音声の一覧表示と順次試聴（音が出ます）。[接続先と使い方](v1-to-v2-migration.md#音声エンジンの話者一覧と簡易テスト)
- `conf.toml --migrate --migrate-out-dir migrated` — v1 `conf.toml` を best-effort で v2 `conf.toml` + `flowgraph/` へ変換
- `conf.toml --validate-conf` — 外部アプリを起動せず、conf・AI参照file・AvatarLink・指定Flowgraphを一括検証して終了
- `--flowgraph-load-dir <dir>` — fixture ではない実運用 Flowgraph をロード検証して終了。AI アバター制御の local Flowgraph 確認にも使う
- `--openai-chat-fine-tuning` — AI Persona のファインチューニングジョブ実行
- `--openai-api-clear-files` — OpenAI API に残留するアップロードファイルを一括削除
- `virtual-avatar-connect-migrate-dict` — 旧 v1 辞書 / regex ファイルを v2 Glossary TSV へ変換（`--features tools` でビルド）

---

## 関連リンク

- [roadmap/phase-delta-spec.md](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/phase-delta-spec.md) — v2 移行ロードマップ（設計・進捗の一次情報）
- [README.md](https://github.com/usagi/virtual-avatar-connect/blob/v2/README.md) — プロジェクト概要
- GitHub: <https://github.com/usagi/virtual-avatar-connect>

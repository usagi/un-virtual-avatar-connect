# VAC Implementation Roadmap

Virtual Avatar Connect の全フェーズ × サブフェーズ単位のチェックボックス一覧。
各フェーズの詳細設計は `docs/roadmap/phase-*.md`（UNVET の `docs/plan.md` 相当）を参照。
全体方針・レイヤ構成・Commit Granularity Rule は [`architecture.md`](architecture.md) を参照。
VAC Flowgraph を常駐型汎用データフロー処理エンジン、および汎用プログラミング言語に近い実行記述へ伸ばす横断計画は [`roadmap/flowgraph-language-roadmap.md`](roadmap/flowgraph-language-roadmap.md) を参照。
VAC 常駐化に伴う配信・日常・仕事・睡眠などの動作状態切替計画は [`roadmap/runtime-mode-roadmap.md`](roadmap/runtime-mode-roadmap.md) を参照。
v2 GUI を常駐ランタイムの管制卓と Flowgraph Studio へ再設計する計画は [`roadmap/gui-redesign-roadmap.md`](roadmap/gui-redesign-roadmap.md) を参照。
常駐 VAC が外部データ源・OS 通知・OBS テンプレート出力を扱う次期機能波は [`roadmap/resident-io-roadmap.md`](roadmap/resident-io-roadmap.md) を参照。
U.N. シリーズアプリを Zenoh で統合制御する **UN App API over Zenoh (UNAA/Z)** のプロトコル正本は [`manual/un-app-api-over-zenoh.md`](manual/un-app-api-over-zenoh.md)、計画は [`roadmap/un-app-api-zenoh-roadmap.md`](roadmap/un-app-api-zenoh-roadmap.md) を参照。
UNVAC 2.0 で UNAvatar / UNMotion 側の必須改修なしに AI が UNMF/Z でアバターを制御する範囲は [`roadmap/unvac-2-avatar-ai-control.md`](roadmap/unvac-2-avatar-ai-control.md)、fix タスクは [`roadmap/unvac-2-avatar-ai-control-tasks.md`](roadmap/unvac-2-avatar-ai-control-tasks.md) を参照。

複雑な表情・ポーズをスチル画像、UNMotion MediaPipe、UNMFスナップショットから制作する構想と比較実験は [`roadmap/unvac-unmotion-avatar-performance-authoring.md`](roadmap/unvac-unmotion-avatar-performance-authoring.md) を参照。
OpenAI API / モデル更新を v2.0 に取り込む範囲と実配信評価は [`roadmap/unvac-2-openai-runtime-policy.md`](roadmap/unvac-2-openai-runtime-policy.md) を参照。
一般ユーザーから高度利用者までを同じruntimeで扱うdesktop UX、profile store、段階的開示は [`roadmap/unvac-v2-desktop-experience.md`](roadmap/unvac-v2-desktop-experience.md) を参照。
Flowgraph の乱数・分布・データ構造・アルゴリズム標準ライブラリー計画は [`roadmap/flowgraph-stdlib-roadmap.md`](roadmap/flowgraph-stdlib-roadmap.md) を参照。
VAC 専用 Dictionary を Glossary へ改名し、Dictionary を汎用 key-value 型に譲る計画は [`roadmap/glossary-rename-roadmap.md`](roadmap/glossary-rename-roadmap.md) を参照。
Flowgraph の言語基盤（Schema / Capability / Testing など）の計画は [`roadmap/flowgraph-language-foundation-roadmap.md`](roadmap/flowgraph-language-foundation-roadmap.md) を参照。
将来の `.unflow` 表層言語と UN Flow の思想メモは [`roadmap/unflow-philosophy.md`](roadmap/unflow-philosophy.md) を参照。

---

## Completed Phases

### Phase δ — Flowgraph Baseline

- [x] δ-0 仕様確定（型付き IO コネクター + exec pin + DAG）
- [x] δ-1〜δ-8 コアノード群実装
- [x] δ-9 Part A-C: Flowgraph Runtime 並走稼働（v0.9.0）
- [x] δ-X `flowgraph.command.set` ノード（V1 scene switcher Flowgraph ネイティブ化）
- 仕様書: [`roadmap/phase-delta-spec.md`](roadmap/phase-delta-spec.md)

### Phase ε — Shutdown 統合と Tauri 移行段取り

- [x] ε-1 ShutdownBroker（Ctrl+C / POST /shutdown / Desktop / Fatal の経路集約）
- [x] ε-2 Tauri ネイティブウィンドウ化 / CLI 可視性ポリシー。crate 再構造化 → `virtual-avatar-connect-cli` / `virtual-avatar-connect-desktop` の 2 runner → desktop runner に Tauri tray + WebView shell を組み込んだ。desktop は tray 常駐を主動線にし、CLI は従来型の API / ログ / 開発 runner として残す（[`architecture.md`](architecture.md)「実行入口」、[`roadmap/v2-vmc-and-restructure.md`](roadmap/v2-vmc-and-restructure.md) §1.1）。
- 仕様書: [`roadmap/phase-epsilon-shutdown-and-tauri.md`](roadmap/phase-epsilon-shutdown-and-tauri.md)

### Phase ζ — v2 → main merge 準備

- [x] ζ-3 Flowgraph reload 時のブリッジ再配線
- [x] ζ-4 AI-Twitch ハイブリッド方針の conf コメント化
- 関連: [`roadmap/v2-merge-pr.md`](roadmap/v2-merge-pr.md)

### Phase η — Dictionary / Table Unification

- [x] η-0 docs: 11 カラム辞書仕様書固定
- [x] η-1a feat(flowgraph): `SocketType::Table` 追加
- [x] η-1b feat(flowgraph): `src/flowgraph/table.rs` 新設
- [x] η-2 feat(flowgraph/nodes/table): `flowgraph.table.*` 4 ノード
- [x] η-3 feat(flowgraph/nodes/dictionary): `flowgraph.dictionary.*` 4 ノード刷新
- [x] η-4 refactor: `flowgraph.dictionary.command` 削除 + commands.tsv 方式
- [x] η-5 feat(bin): `virtual-avatar-connect-migrate-dict` V1→η migrate CLI
- [x] η-6 feat(gui): Table ポート視覚区別
- 仕様書: [`roadmap/phase-eta-dictionary-unification.md`](roadmap/phase-eta-dictionary-unification.md)

### Phase φ — Control API + Glossary Editor + Live Quick-Add

- [x] φ-0 docs: Control API / Table CRUD / Trigger 仕様書
- [x] φ-1 feat(web_interface/control): Table CRUD API 実装
- [x] φ-2 feat(web_interface/control): Flowgraph Trigger API 実装
- [x] φ-3a feat(gui): `types.ts` / `api.ts` に Control Table / Trigger 型追加
- [x] φ-3b feat(gui): Glossary Table / DictionaryEditorPane 骨格
- [x] φ-3c feat(gui): DictionaryEntryForm（11 カラム編集モーダル）
- [x] φ-3d feat(gui): DictionaryConflictDialog（3-way merge）
- [x] φ-3e feat(gui): LiveTab 統合
- [x] φ-4 feat(gui): Live Quick-Add Widget + forget_node_id
- [x] φ-5 docs: `[[control_api.tables]]` / quick_add reference + tutorial
- [x] φ-6 feat(flowgraph,gui): Flowgraph Editor 上の Trigger ボタン
- 仕様書: [`roadmap/phase-phi-control-api-dictionary-editor.md`](roadmap/phase-phi-control-api-dictionary-editor.md)

### Phase χ — OpenAI Responses API 全面移行

Chat Completions API から Responses API へ全面移行。streaming + tool loop + gpt-5 retry + overflow summary + hot-reload を **1 PR / 9 commit** で full_parity 移植。Option A（`reqwest` + 自前 DTO）で実装、shared crate 化を視野に境界設計する。

- [x] χ-0 docs: roadmap.md / architecture.md 新設 + phase-chi-openai-responses.md 新設 + cross-link 更新
- [x] χ-1 feat(ai/responses): scaffold + Cargo deps 調整 + non-stream DTO types + `create` + golden tests
- [x] χ-2 feat(ai/responses): `StreamEvent` enum + SSE parser + モック stream tests
- [x] χ-3 refactor(ai/tools): Tool 定義 + `dispatch_tool_call` を Responses 型に移行
- [x] χ-4 refactor(ai/context,reload,model_policy): request assembly を `CreateResponseRequest` に置換
- [x] χ-5 refactor(ai/service,completion): streaming / tool-loop / gpt-5 retry を Responses 全面移行
- [x] χ-6 refactor(conf): `openai_max_output_tokens` + レガシーフォールバック
- [x] χ-7 docs: manual / conf.example / CHANGELOG 更新 + roadmap.md の χ tick
- [x] χ-8 test: cargo test + svelte-check + 実機スモーク + 観測器追加
  - [x] χ-8.0 fix(flowgraph/docs): `node_catalog_md_up_to_date` テストを line-ending 正規化して CRLF 環境でも通るように（χ-5 以前からの既知 pre-existing bug、Windows の `core.autocrlf=true` で検出）
  - [x] χ-8.1 test(ai/responses): `cargo test --lib --release` 481 pass / 0 fail / 1 ignored、`gui/ npm run check` / `npm run build` 緑、実機スモーク (gpt-4o-mini happy / gpt-4o-mini + tools / gpt-5-mini + `reasoning.effort=low`) 全成功
  - [x] χ-8.2 feat(ai/responses): `ResponsesClient` / `drive_responses_tool_loop` に送信・status・SSE イベント受信の debug/trace ログを追加。`connect_timeout(10s)` を明示し、Windows TLS 交渉ストールで total timeout が事実上無効化されるケースの耐性を上げる
- 仕様書: [`roadmap/phase-chi-openai-responses.md`](roadmap/phase-chi-openai-responses.md)

### Phase ψ-α — Encrypted Reasoning Passthrough

Phase χ の次の一手として、gpt-5 系の tool loop round 間で `include: ["reasoning.encrypted_content"]` を使い、`store: false` を維持したまま reasoning state を client 側で持ち回る **狭い範囲の最適化**。`drive_responses_tool_loop` の round 間で暗号化 reasoning blob を `output[]` → 次ラウンドの `input[]` に pass-through する。

- [x] ψ-α-0 docs: `phase-psi-alpha-encrypted-reasoning.md` 新設 + roadmap を backlog→Active に移行 + χ の §11.2 設計メモから phase doc に昇格
- [x] ψ-α-1 feat(ai/responses): DTO 拡張（`CreateResponseRequest.include` / `InputItem::Reasoning` / `OutputItem::Reasoning.encrypted_content`）+ golden tests 7 本追加
- [x] ψ-α-2 feat(ai/service,config): `openai_reasoning_encrypted_passthrough` (既定 `true`) + `drive_responses_tool_loop` の round 間 pass-through（gpt-5 系のみ有効、他モデルは no-op）+ resolver / helper unit tests 9 本
- [x] ψ-α-3 docs: CHANGELOG / conf-reference / conf.example 更新 + 実機スモーク（gpt-4o-mini で include 非付与・既存経路 bit-互換 / gpt-5-mini で include 付与・OpenAI 側 200 OK・SSE 正常完了）+ roadmap tick
- 仕様書: [`roadmap/phase-psi-alpha-encrypted-reasoning.md`](roadmap/phase-psi-alpha-encrypted-reasoning.md)
- scope: `drive_responses_tool_loop` の round-over-round に閉じる narrow change。`react()` 呼び出しを跨ぐ持ち回りはしない（hot-reload / memory window trim で context shape が変わるため）
- 実用的 reasoning token 節約の数値比較は VAC 同梱 tool がすべて 1 round で返るため将来 phase（user 作成 multi-round tools を使う）に委ねる

### Phase ν — GUI E2E testing with Playwright

Playwright による E2E テスト基盤を `gui/` 配下に閉じ込めて導入。初期 5 ケースのうち **3 ケース** (§3.1 / §3.4 / §3.5) を着地させ、残り 2 ケース (§3.2 canvas DnD / §3.3 editor 409 merge) は **ν-β** に分離する narrow-scoped な幕引きとした。`webServer` が `cargo run --release -- conf.fixture.e2e.toml` を起動し、外部 IO ゼロの fixture で auth / HTTP / WS / widget 操作 / flowgraph trigger までを 3 specs / 8.4s で回帰検知できる状態になった。

- [x] ν-0 docs: `phase-nu-gui-e2e-playwright.md` 本文書き下ろし + 5 ケース仕様 + fixture 設計 + webServer 戦略 + セレクタ規約確定
- [x] ν-1 chore(gui): `@playwright/test` 追加 + `playwright.config.ts` + `gui/tests/e2e/` + `conf.fixture.e2e.toml`（外部 IO ゼロ）+ fixture flowgraph 同梱
- [x] ν-2 test(gui): 3 ケース実装完了（`control-panel-smoke` / `channels-ws-live-update` / `live-quick-add-learn-undo`）— §3.2 / §3.3 は ν-β へ分離
- [x] ν-3 docs: CHANGELOG / manual 追記（run 手順 + CI optional 方針）+ roadmap tick（commit `4f8b15d`）
- 仕様書: [`roadmap/phase-nu-gui-e2e-playwright.md`](roadmap/phase-nu-gui-e2e-playwright.md)
- scope: narrow-scoped。`gui/` 配下に閉じ、Rust 側 `Cargo.toml` や CI には一切触れない。visual regression / 多ブラウザ matrix / component testing は ν+ に送る
- Linux / Windows どちらでも手元で回せることが必須。CI 化は optional（ν-β 以降で検討）

---

### Phase ν-β — Flowgraph Canvas + Glossary Editor E2E

ν-2 から分離した後続フェーズ。Glossary Editor の 409 race condition、Flowgraph Canvas の編集 → Ctrl+S 往復、engine 側の Table default coerce 失敗 (`MissingRequiredInput`) の 3 点を一気に着地させた。最終的に `gui/tests/e2e/` は **5 specs / 1 worker / ~9 s** で全通し、fixture flowgraph は `table.from_json` 補助ノードを外して `in/log/learn/forget` の 4 ノード最小構成に戻せた（§6.4 の追補条件が全部成立）。

- [x] ν-β-3 fix(flowgraph/table): `Table::from_json_array(&[], None) → Table::empty()` で空 Table default の coerce 経路を通す + `PortSpec::with_default` の round-trip test + fixture から `dict_src` 削除
- [x] ν-β-1 test(gui): §3.3 glossary editor 409 merge spec（PATCH 409 → conflict dialog 3-way merge → 自分の編集を強制 → cleanup）
- [x] ν-β-2 test(gui): §3.2 `flowgraph-canvas-basic.spec.ts`（Palette click-add → dirty badge → Ctrl+S → PUT 200 → clean、edge drag / beforeunload は ν-β+ に送る）
- 仕様書: [`roadmap/phase-nu-gui-e2e-playwright.md`](roadmap/phase-nu-gui-e2e-playwright.md) §3.2〜§3.3 および §6.4

### Phase ο — Flowgraph Enhancement I (計算系 + 時間 + signal util + GUI 小改善)

Flowgraph 機能向上の第 1 波。旧 ο-0 の本数想定 **78 (42+1+20+5+5+5)** では §3.4 が **timer 1 + time 案 4 = 5** 本だった。time 4 種（旧 `flowgraph.time.*` 想定）は **Phase π** の `flowgraph.datetime.*` **8 ノード**に置換し、**ο-4** は **`flowgraph.util.timer_interval`（Stateful）** のみにスコープ縮小。合算 **(42+1+20+1+5+5+5) + 8 = 82 本**（**Phase ο 側 engine+GUI 74** + **π 8**）。外部 IO は **ρ → σ → τ → υ → ω** の backlog 順（旧 π〜υ 1 文字繰り下げ後の命名）で追って取る。

- [x] ο-0 docs: `phase-omicron-flowgraph-enhancement.md` 新設 + roadmap.md の Active 差し替え + backlog-nodes.md §1 を ο-4 昇格 pointer 化（+ 追補: angle normalization 4 / 双曲線 6 ノード追加 + Phase ξ 依存明記）
- [x] ο-1 feat(flowgraph/math): §3.1 **42 ノード**追加（abs/min/max/clamp/lerp/inverse_lerp/remap/smoothstep/trig/arctrig/atan2/hyperbolic/arc-hyperbolic/sqrt/pow/exp/log/sign/floor/ceil/round/deg↔rad/normalize_angle × 4）。全 Quantity-aware、30 unit tests 全緑、node-catalog.md 再生成済み。ο-0 docs の「37」表記は実装時に 42 へ正確化
- [x] ο-2 feat(flowgraph/easing): §3.2 `flowgraph.easing.apply` + curve enum 19 種 (linear + quad/cubic/sine/expo/elastic/bounce × in/out/inOut) + `PropertySpec.choices` 追加 + GUI `FlowgraphPropertyEditor` の `<select>` dropdown hook。13 unit tests 全緑、node-catalog.md 再生成済み
- [x] ο-3 feat(flowgraph/vec): §3.3 vec2 / vec3 × 10 ノード ✕ 2 = **20 ノード**（make/unpack/add/sub/scale/dot/length/normalize/lerp/distance、JSON 配列表現）。`decode_vec::<N>` / `encode_vec::<N>` の `const N: usize` generic helper + 6 種マクロで型安全に実装、19 unit tests 全緑（round-trip / 零ベクトル normalize / 次元不一致 error / 非有限値 reject 含む）、node-catalog.md 再生成済み
- [x] ο-4 feat(flowgraph/util): §3.4 `flowgraph.util.timer_interval`（[backlog-nodes.md §1](roadmap/backlog-nodes.md)）。Stateful + `ctx.trigger`、lazy 初回の `sources` engine 例外、lib test + `node-catalog`。**旧案 `flowgraph.time.*` 4 種は Phase π の `flowgraph.datetime.*` 8 ノードに置換済み**（[phase-pi-datetime-system.md](roadmap/phase-pi-datetime-system.md) §4.9 / [datetime-system.md](manual/datetime-system.md)）
- [x] ο-5 feat(flowgraph/util,random,noise): §3.5 signal util 5 種 + §3.6 random 3 + Perlin 1D/2D（`noise = 0.9`）、`node-catalog` 再生成、lib test +10、`edge_detect` 実時間 integration（`timer_interval` + `state.bool` + `log`）
- [x] ο-6 feat(gui): §3.7 palette カテゴリ非表示トグル（`localStorage`）+ pane DnD drop（`FlowgraphPaneDropBridge` + `addCatalogNodeAt`）+ `Ctrl/Cmd+D` 複製 + `flowgraph-canvas-basic.spec.ts` 回帰 3 本（E2E 前に `gui` を `npm run build`）
- [x] ο-7 docs: CHANGELOG に ο-0..ο-7 総括節、`BLESS_NODE_CATALOG=1 cargo test --lib node_catalog_md_up_to_date` で `docs/manual/node-catalog.md` 整合確認、本ファイルで Phase ο を Completed へ移動、`npm run check` / `cargo test --lib` / `npx playwright test` 緑確認
- 仕様書: [`roadmap/phase-omicron-flowgraph-enhancement.md`](roadmap/phase-omicron-flowgraph-enhancement.md)
- scope: narrow-scoped。engine 内完結 Pure ノード + GUI 小改善に閉じる。新規 crate 依存は `noise` 1 件のみ。Breaking change なし

### Phase π — DateTime Type System (jiff 採用 + chrono 全面置換)

Flowgraph engine に **絶対時刻を表す `DateTime` 型**を第一級概念として導入した基盤フェーズ（**完了**）。Phase ξ の単位次元と同じ「型は事故を防ぐ砦」哲学。`chrono` 直接依存を撤去し **`jiff`** に全面置換。`Duration` は ξ の `Quantity<time>` に統一。設計上の先決条件（ο-4 `timer_interval` など）を満たしたうえで実装された。

- [x] π-0 docs: `phase-pi-datetime-system.md` 新設 + roadmap.md の phase 名・順序の再編（旧 π/ρ/σ/τ/υ を 1 文字繰り下げて ρ/σ/τ/υ/ω に、DateTime を新 π に割り当て）+ cross-reference 修正
- [x] π-1 feat(deps): `jiff = "0.2.24"` (features: `serde` + デフォルト `tz-system` / `tzdb-*`) 追加 + `tests/jiff_smoke.rs` 14 tests 全緑（Timestamp / SignedDuration / Offset / Zoned / serde round-trip）。副産物: `Offset` は `FromStr` 非実装、`DateTimeParser::parse_time_zone` は bare `Z` を拒否する仕様を pin 止め（phase doc §3.5 更新）
- [x] π-2 refactor(chrono->jiff): 既存 chrono 使用 21 箇所を jiff に全面置換（3 commit 構成: batch1 leaf 14 / batch2 dictionary TTL 境界 + 7 境界テスト / batch3 struct field 5 + 4 serde round-trip snapshot、計 702→706 lib tests all green）。wire format drift `+00:00` → `Z` は RFC3339 互換範囲として許容し commit message に明記
- [x] π-3 chore(deps): `Cargo.toml` から `chrono` 直接依存を解除、`src/**` は `use chrono` ゼロ、`cargo tree -i chrono` で直接依存なしを確認（間接依存は `twitch-irc v6.0.0` 経由で残存、これは twitch-irc 側の内部実装で π スコープ外）、全 706 lib tests green
- [x] π-4 feat(flowgraph/datetime): `SocketType::DateTime` + `SocketValue::DateTime` + engine 側 String ↔ DateTime 暗黙 coerce + `FlowgraphInstanceConfig.default_timezone: Option<String>`（未設定時 UTC、FixedOffset `+09:00` 形式のみ、IANA tz 非対応）+ naive datetime パース policy（config default tz 適用）
- [x] π-5 feat(flowgraph/nodes/datetime): 8 ノード（`now` / `parse` / `format` / `add_duration` / `sub_duration` / `diff` / `epoch_ms` / `from_epoch_ms`）+ `get_required_datetime` + lib test +30、`node-catalog` 再生成済
- [x] π-6 docs: CHANGELOG + `docs/manual/datetime-system.md` 新設 + `docs/manual/index.md` + [phase-omicron-flowgraph-enhancement.md](roadmap/phase-omicron-flowgraph-enhancement.md) §3.4/§5.4/§6.4 更新 + roadmap 本節 tick（`node-catalog` は π-5 時点で更新済）
- 仕様書: [`roadmap/phase-pi-datetime-system.md`](roadmap/phase-pi-datetime-system.md)
- scope: chrono → jiff 全面移行 + Flowgraph `DateTime` 型。Breaking change なし（wire 互換・config 既定で後方互換）。IANA tz / DST / `Span`（暦幅）/ 独自 affine 単位は **π+** 扱い
- 順序: **π-0..π-6 完了**。`flowgraph.datetime.*` による日時ワイヤは本フェーズで揃い、**ρ → σ → τ → υ → ω** backlog とは独立

### Phase ξ — Dimensional Quantity System (SI 準拠の単位次元システム)（完了）

Flowgraph engine に **SI 準拠の単位次元システム**を第一級概念として導入した基盤フェーズ。数値に unit を付与、unit は次元（L·M·T·I·Θ·N·J + 疑似次元 Angle の 8 成分）を持つ。strict default + 明示 `flowgraph.unit.strip`。IO 系は pass-through。既存 flow は dimensionless fallback で完全後方互換。

- [x] ξ-0 docs: `phase-ksi-dimensional-quantity-system.md` 新設 + roadmap.md への Phase ξ 追加 + Phase ο doc の依存注記
- [x] ξ-1 feat(flowgraph/quantity): `Dimension` / `Unit` / `Quantity` 型 + SI 基本 7 単位 + 主要誘導単位 + SI 接頭辞 20 種 + Angle 疑似次元（rad / deg）+ 温度 delta 分離 (K / ΔK) + unit 文字列 parser + unit test 30+
- [x] ξ-2 feat(flowgraph/nodes/unit): `flowgraph.unit.*` 操作ノード 7 種 + `SocketType::Quantity` / `SocketValue::Quantity` + TOML / JSON wire format + `Unit::to_si_base` atom-canonical 係数込み
- [x] ξ-3 refactor(flowgraph): `Float ↔ Quantity` 暗黙 coerce + `flowgraph.math.float_*` の Quantity 演算化
- [x] ξ-4 feat(flowgraph/util): `Quantity → String` 暗黙 coerce + `flowgraph.util.format` + log / channel.emit の unit-aware 化
- [x] ξ-5 feat(gui): `FlowgraphNodeCard.svelte` で Quantity ポートの handle 色分け（次元 family）+ default から復元できる場合のみ unit バッジ + tooltip。`FlowgraphCanvas` で engine 互換の型接続（Float↔Quantity 等）と型不一致エッジの赤破線表示。`FlowgraphPropertyEditor` で `unit` / `target_unit` / `unit_override` の `GET /flowgraph/parse-unit` 検証。node-catalog JSON に `quantity_dim` 等を注入。E2E `flowgraph-canvas-basic` に Quantity handle 回帰を追加。
- [x] ξ-6 docs: `docs/manual/dimensional-quantity-system.md` ほか
- 仕様書: [`roadmap/phase-ksi-dimensional-quantity-system.md`](roadmap/phase-ksi-dimensional-quantity-system.md)
- 順序: **ξ-0..ξ-6 完了**。次の本編候補は Backlog（[Phase ρ](#phase-ρ--osc--vmc--vrc-bridge-tbd) など）。**閉集合 string・ライブラリ再利用 v0** は [Phase λ](roadmap/phase-lambda-flowgraph-enum-and-library.md)（本ファイル Completed 節）で完了。

### Phase λ — Flowgraph: Enum 型システム + ライブラリ再利用（v0 完了）

閉集合 string（`PortSpec` メタ）、ユーザ定義 `[[enums]]`、ライブラリ境界ノード（`flowgraph.library.*`）、`[meta]` 拡張（author/name/version / `library_uses`）、依存グラフの閉路検出を一つの設計線で実装した。設計の単一ソースは [`roadmap/phase-lambda-flowgraph-enum-and-library.md`](roadmap/phase-lambda-flowgraph-enum-and-library.md)。δ 当初の `FlowgraphFile` 3 セクション方針は **後方互換のもとで拡張**（同 phase doc §4、[`phase-delta-spec.md`](roadmap/phase-delta-spec.md) §8.6）。

- [x] λ-0 docs: `phase-lambda-flowgraph-enum-and-library.md` 新設 + `roadmap.md` Active 化 + `phase-delta-spec.md` / `architecture.md` への cross-link
- [x] λ-1 feat(flowgraph): Enum MVP（`closed_string_variants` + engine / loader / GUI + `tts.speak` の `engine`）
- [x] λ-2 feat(flowgraph,gui): `[[enums]]` パース・診断・パレット `user_defined`
- [x] λ-3 feat(flowgraph): `flowgraph.library.input` / `flowgraph.library.output` スタブ登録
- [x] λ-4 feat(flowgraph): `FileMeta` 拡張 + `library_uses` + 閉路検出
- [x] λ-5 docs: CHANGELOG + manual 断片 + example 更新
- ユーザ向け: [`manual/flowgraph-enum-and-library.md`](manual/flowgraph-enum-and-library.md) / 例: [`flowgraph.example/lambda-demo/main.flowgraph.toml`](../flowgraph.example/lambda-demo/main.flowgraph.toml)
- **λ+（未スケジュール）**: 境界ポートの接続駆動増減、Exec 境界、fragment との二重経路整理などは phase λ 文書に従い別マイルストーンで扱う。

---

## Active Phases

### Phase M — VMC パススルーと motion 層（[`v2-vmc-and-restructure.md`](roadmap/v2-vmc-and-restructure.md)）

ギリシャ文字フェーズ（δ / φ …）とは別ラベルの **M0〜M5** で追う。全体方針・アーキ図は [`roadmap/v2-vmc-and-restructure.md`](roadmap/v2-vmc-and-restructure.md)。**M0 / M1 の実装メモ**は [`roadmap/phase-mu-vmc-motion-m0.md`](roadmap/phase-mu-vmc-motion-m0.md)（M1 は同文書 §8）。

UNVAC の motion 方針は、VMC を内部コアにしない。内部姿勢・表情表現は **UNMF**、送受信・保存・外部連携の motion data plane は **UNMF/Z** を中核にする。VMC は `VMC -> UNMF` / `UNMF -> VMC` adapter として互換維持する。U.N. シリーズアプリ操作の control plane は別計画の **UN App API over Zenoh (UNAA/Z)** で扱う。

- [x] M-0 docs: `phase-mu-vmc-motion-m0.md` 新設 + `roadmap.md` Active 化 + `architecture.md` / `conf-reference.md` + `v2-vmc-and-restructure.md` cross-link + `conf.example-motion.toml`
- [x] M-0 feat(motion): `src/motion/`（`vmc_raw` + `router` + `osc` プレースホルダ）+ `[motion]` / `MotionHandles` 起動・`ShutdownBroker` 連携
- [x] M-1 feat(flowgraph,bridges): `flowgraph.ingress.vmc_udp` + `vmc_ingress` ブリッジ + `` `TriggerEvent` `` 投入（`phase-mu` §8、`flowgraph.example/vmc-udp-ingress`）
- [x] M-2 feat(motion): パススルー・ハブ運用の conf / ログ整備（複数受信ソケットの運用例）
- [x] M-3 feat(web_interface,gui): Control API `GET /api/v1/control/vmc/status` + runtime bind / forward add/remove + Resources GUI 管理。
- [x] M-4a feat(flowgraph,motion,deps): `flowgraph.motion.vmc_parse`（Base64→OSC→JSON）+ Phase ρ 先取り `flowgraph.osc.send`（UDP）+ `rosc` + `flowgraph::fixture_runner` 最小 + `src/app_core/`（Step 7 一段）
- [x] M-4 feat(flowgraph): `MotionFrame` 第一級型 + `motion_frame` ソケット（`json` と coerce 往復）+ `flowgraph.motion.vmc_parse` / `filter` / `map`（ワイヤ表現は `byte_len` + `osc_messages`。**head_pose 等の意味 IR**は M5 以降 / `v2-vmc` §M4 参照）
- [x] M-5 docs+flowgraph: 用途拡張の first slice として `flowgraph.vmc.extract_blendshape` と型付き pose / blendshape 出力、`flowgraph.example/vmc-blendshape-trigger` / `vmc-ai-mode-control` / `vmc-obs-scene-control`、`flowgraph.obs.request` / `flowgraph.obs.set_current_program_scene` を追加（表情・gesture トリガー、AI 入力、Runtime Mode 制御、OBS scene 制御の土台）。
- [~] M-6 UN Suite integration: `un-motion -> VAC -> un-avatar` を VMC/UDP hub として接続する。初段として [`roadmap/un-suite-integration-roadmap.md`](roadmap/un-suite-integration-roadmap.md) と [`conf.example-un-suite.toml`](../conf.example-un-suite.toml) を追加。UNMF/Z / Zenoh は VMC 実運用確認後に判断。
- [x] M-6 docs+design: UNMF / UNMF/Z を motion core / transport として正式化し、既存 `MotionFrame` / VMC ノードを互換 adapter 方針へ整理する。**実施**: `vac-motion::unmf` を追加し、crates.io の `un-motion-frame = 1.1.0` / `un-motion-frame-zenoh = 1.0.0` を正本として採用。既存 `motion_frame` は VMC/OSC wire adapter として維持。
- [x] M-7a feat(flowgraph,motion): `VMC -> UNMF` / `UNMF -> VMC` adapter ノードと、UNMF/Z encode/decode ノードの first slice。**実施**: `flowgraph.unmf.from_vmc` / `to_vmc` / `extract_bone` / `extract_blendshape`、`flowgraph.unmfz.encode` / `decode` を追加。
- [x] M-7b feat(flowgraph,motion): schema-aware helper として `flowgraph.unmf.map` / `filter` を追加。`filter` は bone/expression の絞り込み、`map` は translation scale / expression scale / clamp を担当し、`VMC -> UNMF -> process -> VMC/UNMFZ` の標準形をグラフ内で組める。
- [x] M-7c-a feat(flowgraph): UNMF 専用 `unmf` socket type と `json <-> unmf` coerce を追加。UNMF ノード群は `unmf` を第一級型として入出力し、JSON ノードとは明示 coerce で接続する。
- [x] M-7c-b feat(flowgraph,motion): UNMF/Z publish / subscribe first slice と UNAA/Z endpoint metadata resolver。`flowgraph.unmfz.publish` は Zenoh publish、`flowgraph.unmfz.subscribe` は exec 発火ごとに最大 1 フレーム受信、`flowgraph.unmfz.resolve_endpoint` は app endpoint metadata から base key / topic mode / schema major を解決する。
- [x] M-7c-c feat(gui,flowgraph): UNAA/Z endpoint metadata から FlowGraph UI の UNMF/Z publish / subscribe 候補を提示し、`flowgraph.literal.json` + `flowgraph.unmfz.resolve_endpoint` + publish / subscribe の実サブグラフを自動生成する。現段階は diagnostics に載る `unaa_z_endpoint_candidates` / `unaa_z.endpoints` 等を入力とする GUI first slice。
- [x] M-7c-d-a feat(control,gui): UNAA/Z discovery snapshot schema を Control API diagnostics へ正式供給し、Managed App registry 由来の app snapshot と flattened endpoint candidate surface を返す。GUI は `unaa_z_endpoint_candidates` を直接利用できる。
- [x] M-7c-d-b feat(control,zenoh): timeout-bounded Zenoh-backed UNAA/Z JSON announcement reader を追加し、`un/app/**` / `unaa_z/**`（`UNVAC_UNAAZ_DISCOVERY_KEYS` で変更可）の snapshot / app / endpoint payload を diagnostics snapshot へ合流する。`UNVAC_UNAAZ_DISCOVERY_TIMEOUT_MS` と `UNVAC_UNAAZ_ZENOH_DISCOVERY=0` をサポート。
- [x] M-7c-d-c design+feat(control,zenoh): UNAA/Z key expression / queryable / liveliness 契約を固定し、diagnostics reader を announcement 依存から queryable snapshot 優先へ拡張する。既定 query は `un/app/**/snapshot` / `unaa_z/**/snapshot`、announcement は `un/app/**/announce` / `unaa_z/**/announce`、liveliness は `un/app/{app_id}/{instance_id}/live` / `unaa_z/{app_id}/{instance_id}/live`。
- [x] M-7c-d-d feat(control,managed_app): Managed App spec と UNAA/Z identity (`unaa_z_app_id` / `unaa_z_instance_id`) の明示 binding を conf / run_with GUI に追加し、process-level 管理と Zenoh discovery app を安定相関する。
- [x] M-7c-d-e feat(gui,resources): Resources / Managed App UI に UNAA/Z discovery readiness / capabilities / endpoint count を表示し、process running と Zenoh ready の差分を観測できるようにする。**実施**: `/managed_apps` response に `unaa_z_status` と discovery source / errors を同梱し、Resources 概要と連携アプリ drawer で Zenoh seen / ready / endpoint / capability count と process-only 差分を表示。
- [x] M-7c-d-f feat(control,unaa_z): UNAA/Z command bridge first slice。安全な generic command request / response schema を Control API に追加し、low-risk command（focus / open GUI / reload preview 等）から GUI に出す。**実施**: `POST /api/v1/control/un_app_api/command` を追加し、`focus` / `open_gui` / `reload.preview` / `profile.switch.preview` の allow-list command を `un/app/{app_id}/{instance_id}/command` へ JSON envelope として publish する。Managed App drawer から focus / open GUI / reload preview を送信可能。
- [x] M-7c-d-g feat(control,unaa_z): UNAA/Z command response / event reader first slice。`request_id` に対応する response / event key contract を固定し、Control API が accepted だけでなく bounded wait の結果を返せるようにする。**実施**: response key `un/app/{app_id}/{instance_id}/response/{request_id}` と event key `un/app/{app_id}/{instance_id}/event` を command dispatch に含め、`wait_response_ms` 指定時は publish 前に購読を張って JSON response / request_id 一致 event を短時間待機する。GUI は応答受信と timeout を toast で区別する。
- [x] M-7c-d-h feat(control,unaa_z): UNAA/Z command history / observability。直近 command の request_id / key / response / timeout を Resources または Observability で確認できるようにする。**実施**: `State` に UNAA/Z command ring buffer を追加し、`GET /api/v1/control/un_app_api/commands/history` で直近 command の request / response / timeout / publish failure を返す。Observability タブに UNAA/Z command 履歴パネルを追加。
- [x] M-7c-d-i feat(control,unaa_z): UNAA/Z command capability gating。discovery `capabilities` / advertised command metadata と safe allow-list を突き合わせ、GUI ボタンを app ごとの対応 command に絞る。**実施**: discovery app payload の `commands` を読み、`commands` 未宣言時は `capabilities` から低リスク command を控えめに導出する。Managed App API は `commands` / `allowed_commands` を返し、drawer は app ごとの `allowed_commands` だけを表示。command API も Zenoh discovery で見えている app には同じ gating を適用する。
- [x] M-7c-d-j feat(control,unaa_z): UNAA/Z command result contract の実アプリ側実装追従準備。UNA / UNM / Renderer / Supervisor Console 側が後から追従できるように、UNVAC 内で `commands` advertisement と response payload の契約を先に固定する。**実施**: UNVAC 側に `GET /api/v1/control/un_app_api/contract` を追加し、discovery key、command / response / event key template、safe command、capability fallback、snapshot / response / event sample を機械可読 JSON として返す。manual に同契約の実装者向けページを追加。
- [x] M-7c-d-k test(control,unaa_z): UNAA/Z contract fixture / mock Zenoh app による command publish → response wait の統合テストを追加する。**実施**: web control tests に in-process fake Zenoh app smoke を追加し、command envelope 受信、`response/{request_id}` publish、UNVAC bounded wait の response observation を検証。Zenoh session が開けない環境では skip ログで終了する。
- [x] M-7c-d-l feat(control,unaa_z): UNAA/Z command history を command API 以外の検証 path からも確認できるようにし、mock app response と履歴反映の結合をテストする。**実施**: command publish / bounded wait / history append を `publish_command_and_record_history` に切り出し、Control API handler と test が同じ記録経路を使うようにした。web control 側に fake Zenoh app smoke を追加し、response observation が `response_observed` として履歴に残ることを検証。
- [x] M-7c-d-m feat(gui,observability): UNAA/Z command history から app ごとの直近応答 / timeout / publish failure を Managed App drawer と Resources 概要に薄く反映する。**実施**: Resources 概要は直近 command 件数、response / timeout / failed 集計を表示し、起動中 app 行に最新 command status を併記。Managed App drawer は app / instance ごとの最新 command を `last={command}:{status}` として表示し、command 送信後に履歴を再取得する。
- [x] M-7c-d-n feat(control,unaa_z): UNAA/Z discovery / command history の stale 判定を追加し、古い app announcement や古い command result を GUI で区別する。**実施**: discovery app payload の `reported_at` / `updated_at` / `announced_at` / `timestamp` / `time` を RFC3339 として読み、`UNVAC_UNAAZ_DISCOVERY_STALE_MS`（既定 10000ms）超過を stale として Managed App API に返す。GUI は UNAA/Z stale badge と command history の stale 表示を追加。
- [x] M-7c-d-o feat(control,unaa_z): UNAA/Z discovery snapshot に source key / payload kind を保持し、どの query / announce から app metadata が来たかを diagnostics で追えるようにする。**実施**: Zenoh discovery payload を `source_key` / `kind` 付きで扱い、app metadata に `discovery_source_key` と `discovery_payload_kind` を保存。Managed App API と GUI は `via=query|announce|managed_app_registry|missing` と source key tooltip を表示する。
- [x] M-7c-d-p feat(control,unaa_z): UNAA/Z discovery provenance を endpoint candidates にも伝播し、FlowGraph palette の UNMF/Z endpoint 候補で source app / source key を確認できるようにする。**実施**: flattened endpoint candidate に app / instance / display name、discovery source key、payload kind、stale / age を補完し、Palette の UNMF/Z endpoint 候補で source app と `via=query|announce|managed_app_registry`、tooltip の source key を確認可能にした。
- [x] M-7c-d-q feat(gui,flowgraph): UNMF/Z endpoint 候補から生成した publish / subscribe subgraph に provenance note / labels を埋め込み、後からどの UNAA/Z app metadata から作った接続か追跡できるようにする。**実施**: endpoint graph 生成時に `unaa_z_provenance` を JSON literal payload へ同梱し、生成ノード群を `UNMF/Z publish|subscribe: app / endpoint via kind` の group として保存する。group label と endpoint payload の両方で app / instance / source key / payload kind / stale を追跡できる。
- [x] M-7c-d-r feat(gui,flowgraph): UNMF/Z endpoint graph 生成後の Inspector / group UI から provenance 詳細を読みやすく表示し、source key と app identity をコピーできるようにする。**実施**: group Inspector が member JSON literal の `unaa_z_provenance` を検出し、app / instance / endpoint / mode / base key / source key / stale を表示する。`app_id`、`discovery_source_key`、provenance JSON 全体を clipboard へコピー可能。
- [x] M-7c-d-s feat(control,unaa_z): UNAA/Z discovery endpoint candidate の schema / validation diagnostics を追加し、欠落した app id / endpoint id / base key / direction を FlowGraph palette で警告できるようにする。**実施**: backend は UNMF/Z endpoint candidate の `app_id` / `endpoint_id` / `base_key_expr` / `direction` を検証し、candidate ごとの `validation_warnings` と diagnostics response の `unaa_z_endpoint_candidate_diagnostics` を返す。Palette は警告 badge と tooltip で欠落・不正 metadata を表示する。
- [x] M-7c-d-t feat(control,gui): UNAA/Z endpoint candidate diagnostics を Resources / Observability にも集約し、どの app が不完全な endpoint advertisement を出しているか一覧できるようにする。**実施**: Resources 概要は endpoint warning 件数 / app 件数と running app 行の warning 数を表示し、Observability の UNAA/Z command 履歴パネルは endpoint diagnostics の code / field / app / source key / message を一覧表示する。
- [x] M-7c-d-u feat(control,unaa_z): UNAA/Z discovery snapshot の app / endpoint payload を fixture 化し、diagnostics validation と GUI parser の回帰テストを追加する。**実施**: `tests/fixtures/unaa_z/discovery_snapshot_mixed.json` を追加し、complete / invalid / root endpoint を含む discovery payload から app-scoped candidate provenance、GUI-facing fields、`validation_warnings`、集約 diagnostics を検証する Rust 回帰テストを追加。
- [x] M-7c-d-v feat(gui,flowgraph): UNAA/Z endpoint candidate parser を独立 module 化し、Node / browser どちらからも同じ fixture で直接テストできるようにする。**実施**: FlowGraph store 内の endpoint candidate 正規化を `gui/src/lib/flowgraph/unmfzEndpointCandidates.ts` へ分離し、共有 fixture `tests/fixtures/unaa_z/discovery_snapshot_mixed.json` を直接読む `npm run test:unit` を追加。complete / invalid endpoint、fallback、warning、dedupe / sort を GUI 側でも固定した。
- [x] M-7c-d-w test(gui,flowgraph): UNMF/Z endpoint candidate warning / provenance UI を Playwright E2E で確認し、Palette warning badge と group Inspector provenance 表示を固定する。**実施**: `gui/tests/e2e/flowgraph-unmfz-endpoints.spec.ts` を追加し、共有 discovery fixture 由来の endpoint candidates を diagnostics route に注入して、Palette warning badge、Publish 生成、group Inspector の UNAA/Z provenance を production `/gui/` で検証する。Palette / provenance block には安定 test hook を追加。
- [x] M-7c-d-x fix(runtime,unaa_z): actix current-thread runtime 上の Zenoh open panic を防ぐ。**実施**: CLI entrypoint を `tokio::main(flavor = "multi_thread", worker_threads = 1)` に変更し、UNAA/Z discovery / command path は current-thread runtime 上では Zenoh 処理だけ専用 1-worker multi-thread runtime へ退避するようにした。
- [x] M-7c-d-y test(control,unaa_z): fixture server で UNAA/Z discovery diagnostics を実 API 経由で叩き、Zenoh 未到達時も panic せず JSON contract で戻ることを E2E または integration smoke で固定する。**実施**: `gui/tests/e2e/flowgraph-unaa-z-diagnostics-api.spec.ts` を追加し、production fixture server の `/api/v1/control/flowgraph/diagnostics` を route mock なしで呼ぶ。`unaa_z`、`unaa_z_endpoint_candidates`、`unaa_z_endpoint_candidate_diagnostics` の contract と、直後の `/ping` 生存を確認する。
- [x] M-7c-d-z test(control,unaa_z): UNVAC 内 fake UNAA/Z snapshot queryable を別 session で立て、query 由来 app / endpoint candidate / provenance / validation warning を取り込むことを E2E または Rust integration smoke で固定する。**実施**: `zenoh_queryable_snapshot_flows_into_endpoint_candidates` を追加し、UNA / UNM 実体には依存せず、UNVAC 内 fake queryable の snapshot reply から app metadata、UNMF/Z endpoint candidates、`discovery_source_key` / payload kind、invalid endpoint diagnostics まで検証する。
- [x] M-7c-d-z2 feat(flowgraph,bridges): long-lived UNMF/Z ingress bridge。**実施**: `flowgraph.ingress.unmfz` と `bridges::unmfz_ingress` を追加し、UNMF/Z Zenoh topic を常時購読して `unmf` / `content_bytes` / `key_expr` を frame ごとの FlowGraph trigger として投入する。`flowgraph.example/unmfz-ingress` で常時 stream 入口の標準形を追加。
- [x] M-7c-d-z3 feat(gui,flowgraph): UNMF/Z endpoint candidate の受信側 graph 生成を一回 subscribe から常時 ingress へ更新。**実施**: FlowGraph palette / command palette の受信ボタンを `Ingress` とし、`flowgraph.ingress.unmfz` を生成して endpoint の base key / topic mode / schema major を properties に保存する。provenance JSON は従来通り group に保持する。
- [x] M-7c-e-a feat(control,unaa_z): UNAA/Z discovery diagnostics の API / GUI 表示を整理し、query / announce / managed registry の件数、timeout / open failure、candidate warning 件数を一つの health summary として返す。**実施**: diagnostics response に `unaa_z_health` を追加し、source / app source 件数 / stale / root endpoint / candidate / warning / discovery error / query key / announce key counts を返す。Resources 概要はこの health summary を表示し、API E2E と Rust 集計テストで contract を固定。
- [x] M-7c-e-b feat(gui,observability): UNAA/Z health summary を Observability 側にも小さく表示し、discovery error と warning がある場合に command history と同じ確認導線へ誘導する。**実施**: UNAA/Z command 履歴パネルに `unaa_z_health` summary、discovery errors、endpoint diagnostics anchor を追加。warning / error badge から詳細リストへ移動でき、Observability E2E で表示を固定。
- [x] M-7c-e-c test(gui,observability): UNAA/Z discovery error / endpoint warning を fixture route で注入し、Observability の badge → details 導線を Playwright で固定する。**実施**: Observability E2E に diagnostics route mock を追加し、`unaa_z_health` の error / warning badge、detail anchor、discovery error detail、endpoint diagnostic code 表示を検証。
- [x] M-7c-e-d chore(unaa_z): UNAA/Z first-slice 完了後の差分を整理し、残る Flowgraph app-control nodes と実アプリ相互運用テストを次フェーズへ切り分ける。**実施**: first-slice は discovery / Managed App binding / command bridge / diagnostics / GUI observability / UNMF/Z endpoint advertisement までで完了とし、`flowgraph.un.app.*` nodes と UNA / UNM 実アプリ相互運用テストは UNAA/Z next phase へ送る方針を明文化。
- [x] M-8-a feat(flowgraph,unaa_z): `flowgraph.un.app.status` first slice。UNAA/Z discovery snapshot から app readiness / capabilities / stale / endpoint counts を読む Pure node を追加する。**実施**: `flowgraph.un.app.status` を追加し、UNAA/Z snapshot または diagnostics 全体 JSON から app / instance を検索して found / ready / running / stale / counts / app JSON を返す。Zenoh を直接叩かないため effect policy は M-8-b へ温存。
- [x] M-8-b feat(flowgraph,unaa_z): `flowgraph.un.app.command` first slice。safe command allow-list と capability gating を Flowgraph effect policy に接続し、manual trigger から focus / open_gui / reload preview を送れるようにする。**実施**: `flowgraph.un.app.command` を effectful node として追加し、UNAA/Z safe allow-list、snapshot 由来 advertised command gating、dry-run dispatch、Zenoh publish / bounded wait response をサポート。capability metadata は `network` + `app_control`。
- [x] M-8-c feat(flowgraph,unaa_z): `flowgraph.un.app.event` first slice。UNAA/Z command response / event stream を Flowgraph ingress または polling node として扱う設計を固定する。**実施**: first slice は bounded wait polling node として `flowgraph.un.app.event` を追加。`app_id` / `instance_id` / `request_id` から response key と event key を購読し、観測時は payload、timeout 時は key contract を返す。常駐 ingress 化は後続へ送る。
- [ ] M-8-d test(integration,unaa_z): UNA / UNM / UN Avatar Renderer が UNAA/Z に対応した後、UNVAC-local fake ではなく実アプリ discovery / command response / endpoint advertisement の相互運用 smoke を追加する。

### v2 crate / runner / GUI 同梱（再構造化メタ）

ギリシャ文字フェーズに先行して **ワークスペース化〜配布形態**を追うチェックリスト。正本: [`roadmap/v2-vmc-and-restructure.md`](roadmap/v2-vmc-and-restructure.md) §1.1 / §1.2 / §3（`vac-gui-assets` 図・移行手順 Step 4 以降）。実装順と担当境界は [`roadmap/crate-runner-desktop-restructure.md`](roadmap/crate-runner-desktop-restructure.md)。

- [x] Step 4（一段）: [`architecture.md`](architecture.md) レイヤ境界表 + `motion` / `bridges` モジュール契約。`state→web_interface` 型依存は `twitch_oauth_sessions` / `control_events` で中立化済み
- [x] Step 5（部分）: ワークスペース化（`crates/vac-gui-assets` のみメンバ追加、`default-members = ["."]`）。他 crate の分割は継続
- [x] Step 6a: Cargo feature **`embed-gui`** — `/gui/*` メモリ配信（`gui_embedded` / `gui_path`）
- [x] Step 6b（crate）: **`crates/vac-gui-assets`** メンバ（`include_dir` + `build.rs`）。**CI は §1.3 どおり未着手**
- [x] Step 7（一段）: `src/app_core/` の `AppCore`（`ShutdownBroker` 以降〜 cleanup）+ `boot` / `server` / `cleanup` / `types` module 分離。将来 `vac-app` への切り出し境界
- [x] Step 7d: crate / runner / desktop 再編の実装順と担当境界を固定（[`roadmap/crate-runner-desktop-restructure.md`](roadmap/crate-runner-desktop-restructure.md)）
- [x] R1: `AppCore::boot` / `runtime_handle` / `run` 分離。CLI / desktop runner が共有する起動境界をコード上に固定し、`types/parts.rs` / `types/run.rs` へ内部型を整理
- [x] R4（一段）: `crates/vac-motion` 追加。`MotionFrame` / OSC decode / UDP forwarding helper を root crate から物理分割
- [x] R4（一段追加）: OSC / VMC / VRChat encoding helper を `crates/vac-motion` へ移動。既存 `crate::flowgraph::{osc, vmc, vrchat}::*` API は再エクスポートで維持
- [x] R4（二段）: `crates/vac-flowgraph` 追加。pure な Quantity system を root crate から物理分割し、既存 `crate::flowgraph::quantity::*` API は再エクスポートで維持
- [x] R4（三段）: `FlowgraphInstanceConfig` / fixed offset parser を `crates/vac-flowgraph` へ物理分割し、既存 `crate::flowgraph::config::*` API は再エクスポートで維持
- [x] R4（core 一段）: `crates/vac-core` 追加。`ShutdownBroker` / `ShutdownReason` を root crate から物理分割し、既存 `crate::shutdown::*` API は再エクスポートで維持
- [x] R4（core 二段）: `RuntimePaths` / `RuntimePathConfig` を `crates/vac-core` へ物理分割し、root `src/runtime.rs` は `Conf` adapter に縮小
- [x] R4（core 三段）: `DateTime` / `ParseError` を `crates/vac-core` へ物理分割し、既存 `crate::datetime::*` API は再エクスポートで維持
- [x] R4（core 四段）: 汎用 `utility` 関数群を `crates/vac-core` へ物理分割し、既存 `crate::utility::*` API は再エクスポートで維持
- [x] R4（core 五段）: HTTP content type 定数を `crates/vac-core` へ物理分割し、既存 `crate::resource::*` API は再エクスポートで維持
- [x] R4（core 六段）: Twitch OAuth session table を `crates/vac-core` へ物理分割し、既存 `crate::twitch_oauth_sessions::*` API は再エクスポートで維持
- [x] R4（core 七段）: Control API event 型を `crates/vac-core` へ物理分割し、既存 `crate::control_events::*` API は再エクスポートで維持
- [x] R2/R3: `run_cli` / `run_desktop_headless` entry API と `virtual-avatar-connect-cli` / `virtual-avatar-connect-desktop` bin を追加（root package 名と同名の exe alias は作らない）
- [x] Step 8: CLI / desktop の 2 runner（仮称どおり）
- [x] Step 9: desktop に Tauri WebView + 同梱静的 + トレイ。Windows 実機で tray / taskbar icon / GUI 終了 cleanup を確認済み。`cargo build --release --features embed-gui` で CLI / desktop の 2 exe と GUI 内蔵配信を確認済み。

---

## Backlog / Future

### Flowgraph Language Roadmap

VAC Flowgraph を「設定ファイル」ではなく、常駐型ランタイムへロードされる **プログラム**として扱うための横断計画。既存 Phase δ / λ / ξ / π / ο / M / ρ... を置き換えず、サブグラフ関数化（λ+）、コレクション処理、第一級 `Result` / `Record` / `Bytes` / `MotionFrame` 型、永続 state、capability、debug/test runner を順序立てる上位設計として管理する。

- 仕様草案: [`roadmap/flowgraph-language-roadmap.md`](roadmap/flowgraph-language-roadmap.md)
- 次の文書化候補: Flowgraph Language Spec、λ+ graph-as-node detailed spec、Flowgraph fixture test runner 設計

### Runtime Mode Roadmap

VAC を常駐型データフローアプリとして動かし続ける前提で、配信・日常・仕事・睡眠・RTA などのユーザー状態に合わせた runtime mode 切替を扱う横断計画。`conf` profile の丸ごと切替ではなく、Flowgraph file / group activation、AI assistant、通知、Managed App desired state を runtime 差分適用する設計として管理する。

- 仕様草案: [`roadmap/runtime-mode-roadmap.md`](roadmap/runtime-mode-roadmap.md)
- 初期 Flowgraph node 方針: `flowgraph.mode.get` / `flowgraph.mode.equals` / `flowgraph.mode.transit` の 3 種に絞る。`on_transit` 専用 ingress node は初期実装しない。

### UN App API over Zenoh Roadmap（UNAA/Z）

U.N. シリーズアプリの control plane。正式名称は **UN App API over Zenoh**、略称は **UNAA/Z**。UNMF/Z が姿勢・表情 frame の data plane であるのに対し、UNAA/Z は app discovery / status / capability / command / profile / endpoint advertisement を扱う。UNVAC はこれを Managed App / Resources / Diagnostics / Runtime Mode に接続し、後段で必要な範囲だけ Flowgraph ノード化する。

- プロトコル正本: [`manual/un-app-api-over-zenoh.md`](manual/un-app-api-over-zenoh.md)
- 計画: [`roadmap/un-app-api-zenoh-roadmap.md`](roadmap/un-app-api-zenoh-roadmap.md)
- 初期順序: docs / vocabulary 固定 → discovery snapshot → Managed App binding → command bridge → Flowgraph nodes → UNMF/Z endpoint advertisement

### GUI Redesign Roadmap

VAC GUI を「機能別の設定パネル」から「常駐ランタイムの管制卓 + Flowgraph Studio」へ再設計する横断計画。トップレベル IA を `利用 / 構成 / 接続と管理` に分類し、AIパートナー、Flowgraph Studio、辞書を直接開ける構成項目として、運用画面や外部連携から分離する。

- 仕様草案: [`roadmap/gui-redesign-roadmap.md`](roadmap/gui-redesign-roadmap.md)
- 初期実装スコープ: `Now` first screen、`Live` operation surface、`Modes` placeholder、新 IA への hash fallback、既存 Flowgraph editor 内部は温存

### Phase ρ — OSC / VMC / VRC bridge (TBD)

**Phase M0〜M2** で VMC **生 UDP** のパススルーと（将来）Flowgraph ingress を先に切る。[`v2-vmc-and-restructure.md`](roadmap/v2-vmc-and-restructure.md) / [`phase-mu-vmc-motion-m0.md`](roadmap/phase-mu-vmc-motion-m0.md)。本 Phase ρ は **OSC の型付き送受信・VRChat ヘルパー・Flowgraph ノード群**（`rosc` 等）の横串として従来どおり backlog に残す（M 系完了後の接続を想定）。

Flowgraph から OSC（Open Sound Control）を使ってアバターアプリ・VRChat・その他 OSC 対応ソフトを制御する基盤。UNVAC は独自 renderer を持たず、UNA / UNM / UNMF/Z と外部アプリ連携を統合するハブとして扱う phase。

- [x] `rosc` crate 追加（`Cargo.toml`）— デコードは `src/motion/vmc_osc.rs`、単発送信は `flowgraph.osc.send`
- [x] `src/flowgraph/osc.rs` 共有基盤（JSON args → OSC エンコード・UDP 単発送出）。受信 ingress 型は従来どおり `bridges` / `motion` 側
- [x] `flowgraph.osc.send`（host / port / path / args JSON、`on_success` / `on_error`）
- [x] `flowgraph.ingress.osc_udp`（`bind` + `fixed_channel`、メタ `profile: "osc_udp"`。アドレス前置フィルタは graph 側 `motion.filter` 等で）
- [x] VMC Protocol pose send（`/VMC/Ext/Bone/Pos` / `/VMC/Ext/Root/Pos` の単発送出ノード + `flowgraph::vmc` ヘルパ。全骨ストリームはグラフ側ループ）
- [x] VMC Protocol pose recv（UDP 受信は既存 `ingress.vmc_udp` / `ingress.osc_udp` + `motion.vmc_parse`。**抽出**: `flowgraph.vmc.extract_bone_pos` / `extract_root_pos` が `MotionFrame` から `/VMC/Ext/*/Pos` を JSON 化）
- [x] VRChat OSC 専用ヘルパー（`flowgraph::vrchat` + `flowgraph.vrchat.avatar_parameter_{float,int,bool}` / `chatbox_input` / `chatbox_typing`。公式 OSC ドキュメント準拠の固定アドレス）
- scope: iFacialMocap 等の mocap app 個別対応は UNM 側の責務。UNVAC は UNMF/Z ingress と汎用 motion/gesture 処理で受ける。

### Phase σ — 外部連携 HTTP + OBS + System Metrics + Twitch Helix 拡張 (TBD)

外部 API 系の横串拡張フェーズ。既存 `[src/flowgraph/nodes/twitch.rs](../src/flowgraph/nodes/twitch.rs)` の Helix / OAuth 基盤を流用しつつ、HTTP 汎用ノード / OBS WebSocket / system metrics を同じ phase に詰める。

- [x] `flowgraph.http.request`（GET/POST/PUT/DELETE/PATCH、headers / JSON body / timeout / status / body / retry_count）
- [x] `flowgraph.obs.*`（`obs.request` / `obs.set_current_program_scene` / `obs.get_current_program_scene` / `obs.set_scene_item_enabled` / record start-stop / stream start-stop / studio mode transition）
- [x] `flowgraph.system.*`（`sysinfo` crate、`system.cpu_usage` / `system.memory` / `system.load_avg` / `system.process_list`。GPU は NVML 依存で後回し）
- [x] `flowgraph.twitch.*` 拡張（Phase σ first slice）: `raid_start` / `raid_cancel` / `ad_run` / `chat_settings_update`（emote / subscriber / follower / slow / unique ほか）/ `prediction_create` / `prediction_end` / `poll_create` / `poll_end` / `shield_mode_update` / `stream_marker_create` / `clip_create` / `channel_info_update` / `goals_get` / `chat_clear`。`flowgraph.example/twitch-stream-tools` 追加。
- follow-up: チャット履歴リフレッシュは Twitch Helix の単発 mutation ではなく VAC 側の chat bridge / channel_data 再同期の設計対象として別タスク化する。
- open question: ユーザ要求の「RAID を 1 時間停止する」は Twitch 側に 1:1 の Helix エンドポイントが無く、`blocked_terms` 運用か独自 state で "incoming raid 遮断" を表現する必要あり → phase doc 内で TBD として扱う

### Phase τ — Process / Window 制御 (TBD)

OS プロセス / ウィンドウ制御ノード群。Windows を第一級 target、他 OS は degrade policy を phase doc で固定する。

- [x] `flowgraph.process.spawn` / `.kill` / `.wait` / `.running`（PID / exe 名で条件判定、shell を介さない command + args 実行、`flowgraph.example/process-control` 追加）
- [x] `flowgraph.window.enum`（現在開いているウィンドウ一覧を Table で返す、`flowgraph.example/window-enum` 追加）
- [x] `flowgraph.window.move` / `.resize` / `.minimize` / `.maximize` / `.restore` / `.close` / `.foreground`
- [x] `flowgraph.window.pseudo_fullscreen` / `.pseudo_fullscreen_exit`（first slice: placement 保存 + maximize / restore。borderless + monitor-size 化は必要なら後続で強化）
- scope: 既存 `windows` crate を再利用。macOS / Linux は明示 error + 空結果で degrade する

### Phase υ — GUI 大物 (Undo/Redo + multi-select + subgraph) (進行中)

Flowgraph editor の大規模 UX 改修。ν-β で送った "Svelte Flow handle drag edge の E2E" もここに合流させ、履歴モデルを第一級概念化する。

- **λ との分担**: 再利用の**意味論**（閉集合、`library_uses`、境界ノード v0）は [Phase λ（完了）](roadmap/phase-lambda-flowgraph-enum-and-library.md)。υ の subgraph / グループは **エディタ上のカプセル化・Undo 等**が主で、将来の engine 側合成は λ（λ+）の境界モデルと整合させる。

- [x] 汎用 Undo/Redo スタック（add/delete/move/connect/disconnect/property-edit を draft snapshot stack で一括対象化。Redo stack も保持）
- [x] 本物のマルチ選択（`selectedNodeIds` + Svelte Flow 選択 + toolbar layout command + 同一 feature 複数選択時の property editor batch edit / mixed 差異ハイライト + 複数ノード fragment copy / ZIP export + E2E 回帰まで完了。リリース基準では完了）
- [x] Flowgraph subgraph / group（`[[groups]]` metadata + GUI での Create group + node badge 表示 + Inspector 上の group label/color 編集 + canvas group frame 表示/選択/移動 + member 追加/除外 + group command palette 操作 + group member fragment copy / ZIP export + fragment paste 時の group metadata 復元まで完了。リリース基準では editor group 機能として完了。engine 側 subgraph 合成と入出力 port 再 export は λ+ 系の将来機能へ分離）
- [x] Svelte Flow handle drag edge の E2E 回帰（source/target handle の安定 test id + Playwright pointer drag で exec edge 作成を検証）

### Phase ω — Audio-reactive + Physics (TBD)

Phase ο の vec2/3 と signal util に直接乗る形で、音声反応と古典力学系を追加する。procedural avatar motion のコア。

- [x] `flowgraph.audio.play`（first slice: EffectfulNode。encoded audio bytes または path を VAC 共有 `audio_sink` へ `rodio` で enqueue。volume / speed 対応、headless では `played=false` で成功。専用 soundboard UI / stop / bus mixer は後続）
- [x] `flowgraph.audio.envelope`（first slice: PureNode。`bytes` PCM (`pcm_s16le` / `pcm_f32le`) から RMS / peak / dBFS / sample_count / frame_count / duration_ms を算出。voice ingress 副産物との直結と平滑化は後続）
- [x] `flowgraph.audio.pitch`（first slice: PureNode。`bytes` PCM (`pcm_s16le` / `pcm_f32le`) から normalized autocorrelation で frequency_hz / confidence / voiced を推定。voice ingress 直結と高精度 F0 は後続）
- [x] `flowgraph.physics.spring`（first slice: scalar StatefulNode。`target` / `initial` / `dt` / `stiffness` / `damping` で value + velocity を積分し、snapshot / restore 対応。vec2/vec3 spring は後続）
- [x] `flowgraph.physics.damper` / `.integrator` / `.gravity`（first slice: scalar。`damper` / `gravity` は PureNode、`integrator` は snapshot / restore 対応 StatefulNode。vec2/vec3 版は後続）
- scope: avatar renderer を持たない前提で、OSC / VMC 経由で外部 renderer に流すことを想定

### Unscheduled Flowgraph Nodes

- **Phase λ（v0 完了）**: `PortSpec.closed_string_variants`、`[[enums]]`、`flowgraph.library.input` / `output`（スタブ）、`FileMeta` 拡張 + `library_uses` 閉路検出。解説: [`manual/flowgraph-enum-and-library.md`](manual/flowgraph-enum-and-library.md)。動的境界ポート等の **λ+** は [`phase-lambda-flowgraph-enum-and-library.md`](roadmap/phase-lambda-flowgraph-enum-and-library.md) 参照。
- **Phase π**: `flowgraph.datetime.*` 8 ノード（[`phase-pi-datetime-system.md`](roadmap/phase-pi-datetime-system.md) / [`manual/datetime-system.md`](manual/datetime-system.md)）**実装済み**。
- **Phase ο-4（実装済み）**: `flowgraph.util.timer_interval`（[`roadmap/backlog-nodes.md`](roadmap/backlog-nodes.md) §1、`phase-omicron` §3.4、[`src/flowgraph/nodes/timer_interval.rs`](../src/flowgraph/nodes/timer_interval.rs)）

### Phase ψ+（TBD）

Phase χ / ψ-α を経てなお残る将来フェーズ候補:

- [ ] `previous_response_id` / `conversation` / `compact` API による server-side memory（ψ-α を経てなお解決しない長期会話ユースケースが残る場合のみ検討。VAC の `include_all` / `overflow_summary` / hot-reload と構造的に衝突するため、既定は `store: false` を維持。設計判断メモ: [`roadmap/phase-chi-openai-responses.md`](roadmap/phase-chi-openai-responses.md) §11.1）
- [ ] built-in tools（`web_search_preview` / `file_search` / `code_interpreter` / MCP tool）
- [ ] `vac-openai-responses` shared crate 化（un-discord-kaltsitpseudo との共有）

### Resident I/O Roadmap（計画中）

常駐型データフローアプリとして、VAC が「外部データを Table に入れる」「ユーザーへ OS 通知する」「OBS へ完成形テンプレートを出す」ための次期機能波。
詳細: [`roadmap/resident-io-roadmap.md`](roadmap/resident-io-roadmap.md)

推奨実装順:

- [x] RI-0 File I/O foundation。String / Bytes / JSON / TOML / RON の read/write effectful node、polling 型 `flowgraph.ingress.file_watch`、`file_read` / `file_write` / `file_watch` capability、JSON/TOML/RON 変換 helper を実装済み。OS native watcher / debounce 詳細化は必要時に後続で扱う。
- [x] RI-1 `.xlsx -> Table` 読み込み。`flowgraph.table.load_xlsx` を追加し、ローカル `.xlsx` から sheet / A1 range / header_mode を Table 化する。`calamine`、`file_read` capability、fixture mock contract、node catalog 反映まで完了。
- [x] RI-2 Google Sheets public URL / ID -> Table 読み込み。`flowgraph.table.load_google_sheet_public` を追加し、公開 Google Sheets を CSV export で取得して RI-1 と同じ Table 仕様へ正規化する。ID / URL / gid / sheet / A1 range、HTTP mock、`network` capability まで完了。OAuth / private sheet は RI-8。
- [x] RI-3 Table draw node。`flowgraph.table.draw` を追加し、`count` / `weight_column` / `seed` / `replacement` による Table 行抽選、単一 `row` 出力、重複なし抽選、全 0 重み時の通常抽選 fallback を実装。
- [x] RI-4 RSS polling。`flowgraph.web.rss_fetch` を追加し、RSS / Atom / JSON Feed を normalized item JSON と Table に変換。`limit` / `since` / `include_content` / `timeout_ms`、HTTP mock contract、`network` capability まで実装。WebSub bridge は後続 subphase。
- [x] RI-5 OS desktop notification。`flowgraph.notify.desktop` を追加し、Windows Toast 出力、`dry_run`、recorded effect、`desktop_notification` capability、Runtime Mode policy preview との接続面を実装。通知 policy の hard enforcement / action callback は後続。
- [x] RI-6 OBS Browser Source template output。`/obs-template/{subtitle|conversation|chat|raid}` を追加し、OBS Browser Source 1 つで `/output` polling による字幕・会話・チャット・RAID 表示を扱える first slice を実装。WebSocket/SSE、GUI 管理、演出詳細は後続。
- [x] RI-7 physical constants。`flowgraph.constants.math` / `flowgraph.constants.physics` を追加し、dimensionless math constants と SI 明示名つき physics Float constants を pure provider として実装。
- [x] RI-8 authenticated Google Sheets。`flowgraph.table.load_google_sheet_auth`、`token_env` / `token_file` 解決、Google Sheets OAuth Device Flow Control API、Resources GUI 認可パネルを実装。refresh token 自動更新と platform keyring は後続拡張。
- [x] RI-9 MsgPack support。`flowgraph.convert.json_to_msgpack` / `flowgraph.convert.msgpack_to_json` を追加し、JSON value と `bytes` 間の MessagePack 変換を pure node として実装。
- [x] RI-10 SQLite3 operations。`flowgraph.db.sqlite.query` を追加し、read-only SQLite 接続、JSON 配列 params の binding、結果 Table 化、`file_read` / `db_read` capability、recorded effect を実装。write / transaction / migration は後続。

### Flowgraph Standard Library Roadmap（計画中）

汎用言語化のため、乱数・分布・応用データ構造・アルゴリズムを段階追加する。
詳細: [`roadmap/flowgraph-stdlib-roadmap.md`](roadmap/flowgraph-stdlib-roadmap.md)

- [x] SL-0 Ranges / LINQ / Iterator foundation。first slice として `flowgraph.list.range` / `slice` / `map_json_get` / `filter_json_eq` / `window` / `aggregate_numbers` を追加し、有限 `List<Json>` の map / filter / window / aggregate と int range 生成を標準化。Table / stream 横断の iterator ABI は後続 SL subphase へ送る。
- [x] SL-1 PRNG foundation。first slice として `flowgraph.prng.sequence` / `uniform_int` / `uniform_float` を追加し、seed + algorithm + offset で deterministic sequence を扱えるようにした。対応 algorithm は `splitmix64` / `lcg64` / `pcg32` / `xoshiro256ss`。Philox / ChaCha は外部 crate と互換性を見て後続 SL subphase へ送る。
- [x] SL-2 Distribution library。first slice として `flowgraph.dist.normal` / `exponential` / `poisson` / `categorical` を追加し、SL-1 PRNG の seed + algorithm + offset に接続した。一様分布は `flowgraph.prng.uniform_int` / `uniform_float` で提供済み。Dirichlet や高精度 Poisson sampler は後続 SL subphase へ送る。
- [x] SL-3 Sequence / buffer data structures。first slice として worker-lifetime state の `flowgraph.collection.ring_buffer` / `deque` / `reservoir_sample` を追加し、JSON event stream の bounded buffer、両端 queue、seeded deterministic reservoir sampling を提供。restart 永続化や table / stream 専用 ABI は後続 SL subphase へ送る。
- [x] SL-4 Sort / search algorithms。first slice として `flowgraph.list.sort_json` / `top_k_json` / `binary_search_json` を追加し、`List<Json>` の stable sort、上位 K 件抽出、昇順 binary search を提供。bucket sort、skip list index、Table 専用 sort / search は後続 SL subphase へ送る。
- [x] SL-5 Indexed collections。first slice として `flowgraph.collection.priority_queue` を追加し、JSON item + float priority の bounded max-priority queue を worker-lifetime state として提供。ordered map、bucket / hash index、Table / record schema 接続は後続 SL subphase へ送る。

### Glossary Rename Roadmap（計画中）

現行の VAC 専用 `Dictionary` を `Glossary` へ改名し、`Dictionary` は将来の汎用 key-value データ構造へ譲る。
詳細: [`roadmap/glossary-rename-roadmap.md`](roadmap/glossary-rename-roadmap.md)

- [x] GRN-1 docs / terminology。`Glossary` / `Dictionary` / `.map` の意味を固定する。
- [x] GRN-2 flowgraph node rename。`flowgraph.glossary.*` を正規名にし、`flowgraph.dictionary.*` の deprecated alias は v2 破壊変更フェーズ中に撤去済み。
- [x] GRN-3 control API / config role。`role = "glossary"` を正にし、`role = "dictionary"` の互換 alias は v2 破壊変更フェーズ中に撤去済み。
- [x] GRN-4 GUI rename。Glossary Editor 系表示・テストを Glossary に揃える。
- [x] GRN-5 examples / tests / migration。sample / E2E / manual / migration note を更新する。
- [x] GRN-6 Dictionary as generic key-value。first slice として `dictionary<string,T>` の現行 runtime 表現である `Map<Json>` に対応する `flowgraph.dictionary.from_entries` / `get` / `set` / `remove` / `keys` / `has` を追加し、Glossary とは別の汎用 key-value 名前空間として `flowgraph.dictionary.*` を復活。非 string key の `dictionary<K,V>`、typed value specialization、Table / record schema index は後続設計へ送る。

### Flowgraph Language Foundation Roadmap（計画中）

Flowgraph を汎用プログラミング言語へ近づけるための基礎整備。詳細: [`roadmap/flowgraph-language-foundation-roadmap.md`](roadmap/flowgraph-language-foundation-roadmap.md)

優先:

- [x] LF-1 Schema / Contract。node catalog contract metadata、graph signature、signature API、package / type schema metadata の diagnostics / signature surface を完了。graph-as-node 実行、structural validation、schema migration は後続設計へ送る。
- [x] LF-2 Capability / Effect。node catalog effect metadata、graph capability summary、GUI effect visibility、Runtime Mode capability policy preview を完了。hard deny enforcement、approval UI、credential / UNAA/Z / WASM policy は後続設計へ送る。
- [x] LF-3 Testing / Debugger。CLI fixture runner、suite runner、trigger sequence、mock HTTP / file read / file write、recorded effects、state / signature / capability assertions、manual を完了。GUI debugger、step execution、mock OBS / Twitch / db は後続設計へ送る。

計画として保持:

- [x] LF-4 Module / Package System。local package manifest / dependency DAG / lock preview / `flowgraph.lock.json` envelope / save & status API / stale warning の基盤を完了。external registry resolution、version range solving、WASM ABI は後続設計へ送る。
- [x] LF-5 Generic / Type Parameter。`bytes` / `option<T>` / `result<T>` / `record<schema_id>`、型 AST、parser / compatibility API、GUI edge validation、record schema catalog / dependency metadata の基盤を完了。`dictionary<K,V>` 非 string key、structural validation、schema migration は後続設計へ送る。
- [x] LF-6 Error Model。fatal `NodeExecError` と recoverable `result<T>` を分離し、主要 I/O / Effectful ノードの result contract と try-parse 系 Pure ノードを完了。retry policy、typed error enum、result combinator は後続設計へ送る。
- [x] LF-7 Persistence / State Model。state model metadata、snapshot / restore contract、profile-local snapshot file、明示 save / restore / state-preserving reload、GUI / fixture / E2E coverage の基盤を完了。自動保存 / 自動復元 / migration registry は後続設計へ送る。
- [x] LF-8 Documentation Generation。`NodeRegistry` / `NodeSpec` 由来の generated manual catalog と Control API / GUI catalog metadata の同期基盤を完了。`library signature` / schema 由来の追加生成は LF-1 / LF-4 後続へ送る。

### Phase ν+（TBD）: E2E 拡張

- [x] BOS Preview の縦横比トグル。Live タブの BOS プレビューに 16:9 / 9:16 / 1:1 の OBS 代表キャンバス比率を追加し、選択ソースの ID / category / path とあわせて E2E で固定。`/obs-template/{subtitle|conversation|chat|raid}` も BOS 仮想エントリとして一覧へ露出し、template 系は `theme` / `poll_ms` / `limit` を GUI から URL 反映できる。
- [x] φ / γ 後続サーフェス（第 1 段）。Managed App Drawer は UNAA/Z identity / discovery / capability / command / latest command の詳細パネルを追加し、BOS Preview は template 系の `theme` / `poll_ms` / `limit` / aspect を一括反映するクイック演出プリセット（字幕 / 最小 / チャット / RAID）を追加。いずれも E2E で固定。
- [x] φ / γ 後続サーフェス（第 2 段: BGM / 画面効果キュー、AI 管理画面、OBS 出力管理の実運用 UI）first slice。Live タブに OBS 出力管理パネルを追加し、BOS 一覧をカテゴリ別に確認、標準 channel 付き URL をコピー / 別タブ起動できるようにした。AI 管理は Resources に persona 状態一覧、個別 pause / resume、persona ごとの runtime reload（system extra / heartbeat / decision threshold）を追加。BGM / 画面効果キューは Control API の in-memory quick-effect queue と GUI を追加し、RAID / chat highlight / screen effect / BGM fade の enqueue、duration、cancel、status 表示を扱える。OBS WebSocket widget は現行シーンの source item 一覧と表示 / 非表示トグルを追加。
- [x] φ / γ 後続サーフェス refinement: Quick Effects の scheduled fire / queue reorder、profile preset 永続化、OBS source item の transform / volume / filter 操作、AI persona runtime edit の永続 conf 反映導線まで完了。

## OCR PaddleOCR Native Experiment

- 仕様草案: [`roadmap/ocr-paddle-native-roadmap.md`](roadmap/ocr-paddle-native-roadmap.md)
- 評価結果: v2.0既定backendは追加配布不要のWindows Media OCR。PaddleOCR ONNX/nativeはexperimental比較資産として維持し、製品runtimeへは統合しない。
- 詳細: [`roadmap/ocr-backend-evaluation.md`](roadmap/ocr-backend-evaluation.md)。native wrapperはstubのまま、通常workspace検証ではnative import libraryを要求しない。
- [x] visual regression（`toHaveScreenshot`）first slice。動的要素が少ない Settings theme surface を 4 テーマ分固定し、既存の広域 screenshot smoke は blank-screen / severe render regression 検出として維持。Now / Flowgraph 等の動的 surface baseline は fixture state をさらに固定してから拡張する。
- [x] GitHub Actions 上での chromium / firefox / webkit マトリクス。`.github/workflows/gui-e2e.yml` を追加し、Windows / Ubuntu / macOS × chromium / firefox / webkit で GUI check / build / VAC release build / Playwright E2E を実行する。ローカル既定は chromium のまま、`PLAYWRIGHT_BROWSERS` で対象 browser を切り替える。

### ε-2 Tauri ネイティブウィンドウ化

ψ-α / ν / ν-β / Phase ο 以降の Flowgraph 拡張と並行、または一段落した後に検討（ユーザー意向として "GUI の Tauri 化" を積んでいる）。**実装順**: 再構造化 → CLI + desktop の 2 実行ファイル → desktop runner に Tauri WebView と tray 常駐 UX を載せる。CLI は従来型の API / ログ / 開発 runner として維持する。仕様書: [`roadmap/phase-epsilon-shutdown-and-tauri.md`](roadmap/phase-epsilon-shutdown-and-tauri.md)

### 長期 backlog（phase 立て前の候補リスト）

設計重量が大きい or VAC 現役ユースケースへの直結度を需要確認してから phase 化する候補群。各 1 行のみ、詳細は phase doc 化のタイミングで起こす。

- [ ] **Discord account / voice presence overlay**（voice ingress より前に検討。Discord 認可で権限のあるサーバー / ボイスチャンネル参加者 / 発話中状態を取得し、OBS で合成できる独立 WebView または既存 BOS レイヤーとして表示する。配信支援機能として phase 化候補）
- [ ] **Discord voice ingress**（Discord bot + voice gateway + opus decode。上記 presence overlay より後。独立 phase / または VAC とは別プロセスの bridge 化も視野）
- [ ] **Global hotkey / MIDI**（StreamDeck 互換、OS 横断の global hotkey listener + MIDI input ingress ノード）

配信演出の進行中仕様: [`roadmap/unavatar-camera-effect.md`](roadmap/unavatar-camera-effect.md) — AvatarLinkで指定した単一UNAvatar Rendererのカメラを進入・保持・開始時状態へ復帰させるUNAA/Z原子命令。

---

## Commit Unit Convention

- サブフェーズ ID を commit message の先頭に付ける: `χ-0 docs: ...` / `χ-1 feat(ai/responses): ...`
- Conventional Commits タイプ（`feat` / `fix` / `refactor` / `docs` / `test` / `chore`）をサブフェーズ ID の後ろに続ける
- scope はモジュール経路で指定: `feat(ai/responses)` / `refactor(web_interface/control)` 等
- 1 commit = 1 behavior topic。横断的な変更は複数 commit に分割する
- Breaking change は必ず `CHANGELOG.md` の `### Breaking changes (<phase>)` に明記する

---

## Related Documents

- [`architecture.md`](architecture.md) — VAC のレイヤ構成と Commit Granularity Rule
- [`roadmap/v2-vmc-and-restructure.md`](roadmap/v2-vmc-and-restructure.md) — VMC 対応と再構造化（Phase **M0〜M5**）
- [`roadmap/phase-mu-vmc-motion-m0.md`](roadmap/phase-mu-vmc-motion-m0.md) — motion 層 M0（VMC UDP パススルー）詳細
- [`roadmap/phase-delta-spec.md`](roadmap/phase-delta-spec.md) — Flowgraph 基幹仕様
- [`manual/index.md`](manual/index.md) — ユーザー向けマニュアル
- [`../CHANGELOG.md`](../CHANGELOG.md) — Phase 単位の変更履歴

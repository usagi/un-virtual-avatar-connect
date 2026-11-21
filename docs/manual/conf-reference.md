# `conf.toml` リファレンス (v2)

v2 配布物に含まれる `conf.toml` の全キー一覧。個別の外部サービス設定（Twitch / OpenAI / TTS / 翻訳 / OCR / Voice / 起動時 run_with / Control API 認証）の詳細は `conf.example-*.toml` を参照してください。

U.N. Motion / U.N. Avatar と接続する最小例は `conf.example-un-suite.toml` と [tutorials/un-suite-vmc-hub.md](tutorials/un-suite-vmc-hub.md) を参照してください。

> **大原則**: v2 の `conf.toml` に `[[processors]]` は **書けません**。データの配線はすべて `flowgraph_dir` 配下の `*.flowgraph.toml` で行います。

---

## 1. 実行リソース

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `workers` | integer | CPU コア数 | 同時並行で走らせる Tokio ワーカー数。4〜8 が現実的 |
| `log_level` | string | `"Info"` | `"Error"` / `"Warn"` / `"Info"` / `"Debug"` / `"Trace"` / `"Off"` |

## 2. Web UI / Control Panel

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `web_ui_address` | string | `"127.0.0.1:57000"` | 内蔵 HTTP サーバの bind address。LAN 公開は `"0.0.0.0:57000"` 等 |
| `web_ui_compress` | bool | `true` | Actix のレスポンス圧縮。無効化したい場合のみ `false` |
| `web_ui_resources_path` | string | `"resources"` | `/resources/*` で配信する静的ファイル置き場。ディレクトリがなければ警告を出してこの配信だけを無効化（404）。素材を使う場合は作成して再起動する。GUI・字幕等の別経路は継続 |
| `gui_dist_path` | string | `"gui/dist"` | GUI (Svelte) のビルド成果物。無ければ案内 HTML を返す。**`cargo build --features embed-gui`** ではワークスペースの **`vac-gui-assets`** クレートが `gui/dist` を同梱するため、このキーは `/gui/*` 配信には使われない（Control API 等は従来どおり） |

## 3. Flowgraph

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `flowgraph_dir` | string | `"flowgraph"` | `*.flowgraph.toml` を再帰的に走査するルート。相対パスは `conf.toml` の所在ディレクトリ基準。存在しなくても起動継続 |

### `[flowgraph]` テーブル（instance スコープ）

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `default_timezone` | string | なし | naive datetime 解釈の既定 TZ（`+09:00` 等）。未指定は UTC。詳細は Flowgraph 設定型の doc |
| `runtime_mode_changed_trigger_node_id` | string | なし | **実効 Runtime Mode が変わった直後**（noop でない遷移のあと）に、ロード済みグラフの **ノード fq ID** へ内部 `TriggerEvent` を 1 発送る。`flowgraph.ingress.web_input` 等、`__trigger__` を持つ ingress を想定。`__source_kind__` は `runtime_mode_changed` |

## 3.1 `[motion]` — VMC 生 UDP パススルー（Phase M0）

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `[motion]` | テーブル | なし | 省略時は motion ワーカーなし |
| `[[motion.vmc_passthrough]]` | テーブル配列 | なし | 受信 UDP 1 ソケットごとに 1 行 |
| `enabled` | bool | `true` | `false` で当該エントリのみ無効 |
| `bind` | string | （必須） | 受信 `"host:port"`（例 `0.0.0.0:39539`） |
| `forward_to` | string 配列 | `[]` | 転送先。空なら当該エントリはスキップ（警告） |
| `label` | string | なし | ログ識別子。省略・空なら `bind` が文脈として使われる（複数受信口のハブ運用向け） |

仕様の正本: [`../roadmap/phase-mu-vmc-motion-m0.md`](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/phase-mu-vmc-motion-m0.md)（Phase M2 は同 doc §9）。例: リポジトリ直下の [`conf.example-motion.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-motion.toml)。

## 3.2 `[unmfz]` — UNMF/Z LAN transport

UNMF/Zのkey expressionと、Zenoh networkへ参加するためのIP/Portは別に設定する。`[unmfz]`を省略した場合は従来どおり自動探索だけを使う。明示endpointを追加しても自動探索は維持される。

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `[unmfz.listen]` | テーブル | 無効 | UNVACがpublisherとしてLAN待受するendpoint |
| `listen.enabled` | boolean | `false` | 固定TCP listenを有効化 |
| `listen.ip` | IP address | `"0.0.0.0"` | 待受NIC。全NICまたは明示したローカルIP |
| `listen.port` | integer | `47447` | UNMF/Z共通の既定TCP port。複数アプリが同じPCでlistenする場合は別portを割り当てる |
| `[[unmfz.connect]]` | table array | `[]` | UNMotion / UNVET等のpublisherが待ち受ける接続先。複数指定可能 |
| `connect.enabled` | boolean | `true` | この接続先を使用するか |
| `connect.ip` | IP address | 必須 | 接続先の明示IP。`0.0.0.0` / `::`は不可 |
| `connect.port` | integer | `47447` | 接続先port |

UNVAC内ではAvatar motion、Flowgraph publish、常駐ingress、subscribe-onceが同じZenoh sessionを共有する。同一listen portをノードごとに重複bindしない。接続先ごとに論理入力を分割せず、複数publisherから同じkeyへのfan-inを維持する。

例: [`conf.example-motion.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-motion.toml)。

## 3.3 `[un_avatar.links.*]` — UNAvatar AvatarLink

UNVAC 2.0 では UNAvatar profile 自体を人間用 / AI 用に分類しない。UNVAC 側の `AvatarLink` が、その profile / Renderer instance をどう扱うかを宣言する。

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `[un_avatar.links.<id>]` | テーブル | なし | `<id>` は UNVAC 内のリンク ID。AI tool の `target_link_id` と合わせる |
| `label` | string | なし | GUI / ログ表示名 |
| `expression_bindings` | 感情名→preset名→重み | `{}` | モデルが公開した表情への明示対応。`smile` / `laugh` / `annoyed` / `concerned` / `surprised`。詳細は[AIアバター制御](tutorials/avatar-ai-control.md#モデルごとの表情対応) |
| `blink_suppressed_expressions` | preset名の配列 | `[]` | 瞬き中に弱める表情成分。`expression_bindings`内の名前と完全一致が必要。値を`1 - 瞬き強度`倍し、開眼時に元の強度へ戻す |
| `capabilities` | string 配列 | `[]` | `reference` / `ai_control` / `camera_control` / `launch`。`ai_control` は `reference` と `unmfz`、`camera_control` は `reference` と `unaa_z` も必須 |
| `profile_hint` | string | なし | UNAvatar profile を人間が識別するためのヒント。v2.0 では正確性保証ではない |
| `manifest_path` | string | なし | profile manifest 等への明示パス |
| `run_with` | string | なし | 既存 `run_with` entry ID への参照 |
| `executable` | string | なし | UNAvatar 実行ファイルの明示パス |
| `launch_command` | string | なし | 起動コマンド。v2.0 では明示設定時のみ使用する想定 |
| `[un_avatar.links.<id>.unaa_z]` | テーブル | なし | このAvatarLinkが指す単一RendererのUNAA/Z論理identity |
| `unaa_z.app_id` | string | 必須 | discoveryの `app_id` と完全一致させる |
| `unaa_z.instance_id` | string | 必須 | 再起動をまたいで安定したRenderer論理ID。起動ごとの `session_id` は設定に固定せずdiscoveryから照合する |
| `[un_avatar.links.<id>.unmfz]` | テーブル | なし | AI control 用 UNMF/Z 送信先 |
| `unmfz.base_key_expr` | string | 必須 | UNAvatar Renderer が受ける UNMF/Z base key |
| `unmfz.topic_mode` | string | `"frame"` | `frame` / `primary_source` / `stream_id` |
| `unmfz.schema_major` | integer | `1` | UNMF/Z schema major |

AvatarLink 単体の例: [`conf.example-avatar-ai-control.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-avatar-ai-control.toml)。
Twitch 配信中の AI persona と組み合わせる一体例: [`conf.example-streaming-ai-avatar.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-streaming-ai-avatar.toml)。

カメラ演出対象の例:

```toml
[un_avatar.links.dr_usagi]
label = "Dr.USAGI"
capabilities = ["reference", "camera_control"]

[un_avatar.links.dr_usagi.unaa_z]
app_id = "un-avatar"
instance_id = "renderer-dr-usagi"
```

`flowgraph.unavatar.camera_effect` はこの完全一致identityから最新の `session_id` を取得し、fresh / running / ready、`camera_control`、`camera.effect` をすべて確認する。複数Rendererへのbroadcastや `main` への暗黙fallbackは行わない。

## 3.4 `[modes.*]` — Runtime Mode 宣言 (RM-1)

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `[modes.<id>]` | テーブル | なし | `<id>` はモード識別子（例 `daily`, `streaming`）。省略時は `modes` なしとして扱われる |
| `default_runtime_mode` | string | なし | 起動直後に選ぶ mode ID。指定時は `[modes.<同一 id>]` が必須 |
| `display_name` | string | なし | GUI 表示用の人間向けラベル |
| `flowgraph_groups.enable` | string 配列 | `[]` | 有効化したい Flowgraph グループ名 |
| `flowgraph_groups.disable` | string 配列 | `[]` | 無効化したいグループ名（同一モード内で enable と重複不可） |
| `managed_apps.start` / `stop` / `minimize` / `leave` | string 配列 | `[]` | `run_with` から解決される Managed App ID（明示 `id` または `run-with-<n>`） |
| `capability_policy.allow` | string 配列 | `[]` | この mode で許可したい capability。空なら allow-list 未指定として扱う。初期実装では preview のみ |
| `capability_policy.deny` | string 配列 | `[]` | この mode で抑止したい capability。`allow` との重複は validation error。初期実装では preview のみ |

ロード時に `managed_apps.*` の各 ID が `run_with` と整合するか検証される。
`capability_policy` は現時点では実行拒否ではなく、`/modes/plan` / `/modes/transit` の preview と GUI 表示で、ロード済み Flowgraph の required capability との衝突を確認するための metadata。
意味論・将来の Mode Manager との関係は [`../roadmap/runtime-mode-roadmap.md`](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/runtime-mode-roadmap.md) を参照。

## 4. 永続化 / 添付

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `state_data_path` | string | なし | ChannelDatum 配列を RON で保存するパス。未指定なら永続化なし |
| `state_data_auto_save` | bool | `true` | 入力のたびに自動保存。`false` で `/save-state` 手動時のみ |
| `state_data_pretty` | bool | `false` | 保存 RON を整形 |
| `state_data_capacity` | integer | `256` | 保持する ChannelDatum の上限件数 |
| `attachment_inline_max_bytes` | integer | `32768` | `ChannelDatum.attachments` の Inline 添付最大バイト。超過は File 降格 |
| `runtime_dir` | string | OS 既定 | ランタイム一時ディレクトリ。Windows: `%LOCALAPPDATA%\virtual-avatar-connect\runtime` |

## 5. Control API 認証（`[control_api]`）

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `bearer_token` | string | 自動生成 | 明示する場合のみ。未指定時は起動ごとに生成し `runtime_dir/control-token.txt` に書き出し |
| `require_token_for_loopback` | bool | `false` | ループバック (127.0.0.1/::1) に Bearer 必須化 |
| `require_token_for_non_loopback` | bool | `true` | LAN / 外部接続に Bearer 必須 |

※ 環境変数 `VAC_CONTROL_API_BEARER_TOKEN` でもトークンを渡せる（`conf.toml` より優先）。

#### Runtime Mode API（RM-3 / RM-5）

- `GET /api/v1/control/modes` — 応答 `mode_ids: string[]`（互換用のキー一覧）と `modes: { id, display_name }[]`。GUI は設定に存在するモードと表示名だけを列挙し、未設定モードの動作を補完しない。
- `GET /api/v1/control/modes/current` — 応答 `mode: string | null`（`null` は `default_runtime_mode` に従うことを意味する）。`managed_apps` は常に省略。
- `PUT /api/v1/control/modes/current` — 本文 JSON `{"mode": "..."}` または `{"mode": null}`。既知の mode 以外は 400。成功時に Flowgraph exec ゲートを再計算し、変化があれば WebSocket `runtime_mode_changed` を送る。**非 noop** のときは `[modes.*].managed_apps` を適用し、操作ログを応答の `managed_apps`（任意）と WS `runtime_mode_managed_apps` で返す。別遷移が走っているときは **409** `transition_busy`。
- `POST /api/v1/control/modes/plan` — 本文 `{"target": "..."}` または `{"target": null}`。遷移プレビュー（`ModeTransitionPlan`）。`modes` があるとき未知の `target` は 400。`capability_denied_by_target` / `capability_unlisted_by_target` は preview 専用で、現段階では実行拒否しない。
- `POST /api/v1/control/modes/transit` — 本文 `{"mode": "...", "dry_run": false, "reason": "..."}`。`dry_run: true` のときは状態を変えず `plan` のみ返す。`dry_run: false` で `PUT .../current` と同様の適用＋応答に `plan` を含む。**非 noop** 時は `managed_apps` 配列を任意同梱。再入時は **409**。

#### Flowgraph Diagnostics API（LF-7）

`GET /api/v1/control/flowgraph/diagnostics` は、ロード診断に加えて Flowgraph の signature / capability / state metadata を返します。
`GET /api/v1/control/flowgraph/signature` は、同じ `graph_signature` JSON だけを返します。
node catalog の `inputs[]` / `outputs[]` / `properties[]` / `contract.*[]` と `graph_signature` の port surface は、従来の `type` / `ty` 文字列に加えて `type_expr` を返します。`type_expr` は `kind`, `name`, `display`, `args[]` を持つ read-only generic type metadata です。
`[[types]]` は `record<schema_id>` 向けの named schema metadata として読み込まれ、`GET /api/v1/control/flowgraph/diagnostics` の `type_schemas[]` と `graph_signature.type_schemas[]` に `source_fq`, `id`, `kind`, `description`, `fields`, `field_type_exprs`, `field_record_refs`, `record_refs`, `external_record_refs`, `referenced_by` を返します。`type_schema_dependency_order[]` と `graph_signature.type_schema_dependency_order[]` は schema dependency graph の dependency-first order です。`type_schema_graph_summary` と `graph_signature.type_schema_graph_summary` は schema 数、既知 dependency edge 数、未定義参照数、root / leaf schema 数を返します。LF-5n 時点では `kind = "record"` の read-only catalog で、`fields` の値はサーバと同じ socket type parser で検証されます。`fields` 内の `record<schema_id>` が未定義の場合や、schema 参照に cycle がある場合は warning diagnostic を返します。full structural validation は後続です。
`GET /api/v1/control/flowgraph/parse-socket-type?text=...` は、サーバと同じ `SocketType::parse` で型文字列を検証し、valid、canonical_type、type_expr、error を返します。`option<T>` は第一級 socket type として受理され、wire では `null` または inner 型の値として扱います。`record<schema_id>` は named schema の read-model shell として受理され、現段階の wire 値は JSON object です。`dictionary<string,T>` は現行の string-key map と同じ意味の parser alias として受理され、canonical_type は `map<T>` になります。`collection<T>` は現行の list と同じ意味の parser alias として受理され、canonical_type は `list<T>` になります。
`GET /api/v1/control/flowgraph/socket-type-compatibility?from=...&to=...` は、サーバと同じ `SocketType::compatible_with` で上流型から下流型への接続可否を返します。`option<T>` 同士の inner 型も再帰的に同じ compatibility rule で評価します。
`GET /api/v1/control/flowgraph/package-lock` は、flowgraph root 直下の `flowgraph.lock.json` を読み、保存済み digest と現在の preview digest が一致するか、package id 単位の追加 / 削除 / 変更 / 不変差分を返します。
`GET /api/v1/control/flowgraph/package-lock-preview` は、`digest`, `entries`, `entry_count` だけを返す read-only package lock preview API です。
`POST /api/v1/control/flowgraph/package-lock-preview/save` は、現在の package lock preview を flowgraph root 直下の `flowgraph.lock.json` に保存します。`If-Match: b3:<digest>` を付けた場合は保存済み lockfile の digest と一致するときだけ更新します。
package lockfile の JSON envelope は `kind = "vac.flowgraph.package_lock"`, `schema_version = 1`, `digest`, `entry_count`, `entries` を持ち、既定ファイル名は `flowgraph.lock.json` です。
ロード時に `flowgraph.lock.json` が現在の preview digest と一致しない場合、または lockfile の読み込み / 検証に失敗した場合は warning diagnostic を返します。現段階では実行拒否しません。
LF-4 時点の package / lockfile 機能は local flowgraph root 内の package catalog、dependency DAG、preview、明示 save、status/diff、load-time warning までを対象にします。外部 registry resolution、version range solving、WASM ABI、resolver enforcement は後続設計です。
`package_manifests` は top-level `[package]` を file ごとに集約した catalog です。`package_dependency_order` は local package dependency DAG の dependency-first order です。`package_lock_preview` は lockfile 生成前に diagnostics API から観測できる read-only preview で、各 entry は raw `.flowgraph.toml` content の `source_digest` と lock entry 全体の `digest` を `b3:<64 hex>` 形式で持ちます。`package_lock_preview_digest` は preview 全体の `b3:<64 hex>` digest です。
`*.flowgraph.toml` は任意の top-level `[package]` を読めます。現在のフィールドは `id`, `version`, `exports` で、`graph_signature.files[]` の `package_id`, `package_version`, `package_exports` として公開されます。
`[package].id` は必須で、`example.pkg-name` のような lowercase dot-separated identifier です。同一 flowgraph root 内で package id は重複できません。`[package].version` は任意ですが、書く場合は SemVer 形式です。`[package].exports` は flowgraph ルート内に存在する fq を指定します。`[package.dependencies]` は dependency package id から version requirement への table で、現在は exact SemVer または `"*"` を読めます。package id は dot を含むため `"example.dep" = "1.2.3"` のように quoted key で書きます。dependency id は同一 flowgraph root 内の `[package].id` と一致する必要があります。exact SemVer は dependency package の `[package].version` と一致した場合だけ有効で、`"*"` は version 未指定の dependency も許可します。空エントリ、未知 fq、正規化後に重複する fq、不正 dependency、未存在 dependency、version mismatch、自分自身への dependency、dependency cycle は `invalid-package-manifest` としてロードエラーになります。
state 関連の値は reload 直後の read-only snapshot であり、worker 実行後の live state ではありません。

- `graph_signature`: graph-as-node / library signature の入口となる read-only boundary metadata。
- `graph_signature.files[]`: Flowgraph file fq、title / description、normalized library id、mode activation metadata。
- `graph_signature.external_triggers[]`: `flowgraph.ingress.*` と control-triggerable node の外部 trigger surface。
- `graph_signature.boundary_inputs[]` / `boundary_outputs[]`: edge で内部接続されていない port surface。
- `capability_summary.stateful_node_count`: graph 内の stateful node 数。
- `capability_summary.snapshot_supported_state_node_count`: graph 内で snapshot export に対応する stateful node 数。
- `capability_summary.restore_supported_state_node_count`: graph 内で snapshot restore に対応する stateful node 数。
- `capability_summary.state_nodes[]`: node ごとの scope / storage / lifetime / snapshot / restore / persistence policy。
- `loaded_state_summary`: ロード直後の stateful node、feature、version、state model の一覧。
- `loaded_state_summary.snapshot_supported_node_count`: ロード直後 program の snapshot 対応 node 数。
- `loaded_state_summary.restore_supported_node_count`: ロード直後 program の restore 対応 node 数。
- `loaded_state_snapshot`: ロード直後に export できた snapshot payload。現段階では対応 node のみ含む。

現在は `flowgraph.state.bool`、`flowgraph.state.int_counter`、`flowgraph.state.latch`、`flowgraph.state.accumulator`、`flowgraph.util.rate_limit` が JSON snapshot / restore に対応しています。
内部の state snapshot ファイル形式は `kind = "vac.flowgraph.state_snapshot"`、`schema_version = 1` の JSON envelope として固定しています。
profile-local snapshot の保存先は `<runtime_dir>/flowgraph-state/<profile-stem>-<hash>/state.snapshot.json` 形式で導出します。
runtime loader には明示的な load-time restore helper があり、raw snapshot payload または state snapshot file envelope から復元できます。
worker spawn 経路にも snapshot file envelope を明示指定する内部 helper があり、復元後の state を持つ program をそのまま起動できます。
明示 restore 付き load / spawn の結果は `loaded_state_restore_report` として diagnostics に残り、restore された node 数を確認できます。
`GET /flowgraph/diagnostics` は `state_snapshot_file_path` で現在の profile / flowgraph root に対応する予定保存先を返します。
同じ応答の `state_snapshot_file_exists` は、その path に snapshot file envelope が存在するかを返します。
runtime には `loaded_state_snapshot` をその予定保存先へ `ProgramStateSnapshotFile` envelope として明示 write する内部 helper があります。
`POST /flowgraph/state-snapshot/loaded/save` は同じ helper を呼び、worker 実行後の live state ではなく `loaded_state_snapshot` を明示保存します。
`POST /flowgraph/state-snapshot/live/save` は起動中 worker から現在の live state snapshot を取得し、同じ profile-local snapshot file path へ明示保存します。
`POST /flowgraph/reload/preserve-state` は live state snapshot を同 path へ保存してから、その file envelope を使って reload / restore します。
GUI の Flowgraph Diagnostics でも `state_snapshot_file_path` がある場合に loaded snapshot 保存用の `save loaded`、live snapshot 保存用の `save live`、live state 保持 reload 用の `reload keep state` を表示します。
Flowgraph Studio の toolbar / command palette からも通常 reload とは別に live state 保持 reload を明示実行できます。
`POST /flowgraph/state-snapshot/profile-local/restore` は `state_snapshot_file_path` の JSON envelope を明示 restore して Flowgraph runtime を reload / respawn します。
GUI の Flowgraph Diagnostics でも同じ path 行に `restore snapshot` 操作を表示し、restore 後に diagnostics を再取得します。
restore や snapshot file 読込に失敗した場合は `state-restore` diagnostic として報告されますが、profile-local persistence、migration、通常 reload 時の自動 restore はまだ導入していません。

### 5.1 `[[control_api.tables]]` — Glossary / 汎用 Table の GUI 編集許可リスト (Phase φ / GRN)

GUI の **Glossary Editor Pane** と **Live Quick-Add Widget** から操作できる TSV ファイルの allow-list。
`[[control_api.tables]]` に登録されていないファイルは Control API から **404**（存在隠蔽）として扱われ、
GUI のカタログにも出ません。

```toml
# Glossary を 1 件、GUI から編集 + Quick-Add トリガ可能にする例。
[[control_api.tables]]
key      = "chat_dict"                            # 必須。URL / localStorage のキー
path     = "dictionary.chat.dict.tsv"             # 必須。cwd 相対 or 絶対パス
label    = "Chat 用語集"                          # 任意。GUI 表示名。省略時は key
role     = "glossary"                             # 任意。"glossary" | "generic" など
editable = true                                   # 任意。false で read-only（全 mutation が 403）

[control_api.tables.quick_add]
node_id        = "chat-echo/main::learn"          # 必須。glossary.learn ノードの fq ID
kind           = "literal"                        # 任意。"literal" | "regex"（既定 literal）
forget_node_id = "chat-echo/main::forget"         # 任意。Undo に使う glossary.forget ノード
```

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `key` | string | — | 必須。URL (`/api/v1/control/table/{key}`) と GUI 内の識別子 |
| `path` | string | — | 必須。TSV の実パス。親ディレクトリは atomic rename 用に自動生成 |
| `label` | string | `key` | GUI タブに表示するラベル |
| `role` | string | `null` | `"glossary"` を指定した Table だけが Glossary Editor のタブに並ぶ |
| `editable` | bool | `true` | `false` で書き込み系 API を 403 に固定。閲覧のみ許可したい時に使う |
| `quick_add.node_id` | string | — | Live Quick-Add の入口ノードの fq ID。直接更新は `flowgraph.glossary.learn`、読込から実行する構成は `flowgraph.ingress.control` |
| `quick_add.kind` | string | `"literal"` | 既定 kind。GUI で上書き可能 |
| `quick_add.forget_node_id` | string | `null` | Undo の入口ノードの fq ID。未指定なら [Undo] を自動的に無効化 |
| `quick_add.request_format` | string | `"direct"` | `direct` は既存の辞書ノードへ各入力を直接送る。`control_meta` は `ingress.control` の `meta` に読み・単語等をまとめる。接続先の入力形式に合わせる |
| `quick_add.learn_observe` | array | `[]` | 追加結果を確認する `{node_id, port, outcome}` の配列。ノードの完全IDと exec 出力を指定する |
| `quick_add.forget_observe` | array | `[]` | 取消結果を確認する同形式の配列。追加用とは別に設定する |

観測する `outcome` は `saved`（対象ファイルへの保存成功）、`unchanged`（重複等で変更不要）、`conflict`（競合）、`failed`（失敗）。`saved` には編集ノードや入口ではなく、実際の保存ノードの成功出力を指定します。観測先は1要求につき最大16出力で、GUIは最大2秒待ちます。待機終了は処理の取消ではありません。結果未確認・失敗・競合では入力を保持し、自動で再送しません。

観測設定を省略した既存構成は実行要求の受付までを表示します。Flowgraphの実行終了だけでは保存成功を証明しません。配線例と設定一式は[辞書エディタとクイック追加](tutorials/dictionary-editor-and-quick-add.md)を参照してください。

#### 挙動メモ

- **optimistic lock**: mutation 系（PUT / POST / PATCH / DELETE）は `If-Match: b3:<content_hash>`
  を付与すると楽観ロックが効く。GUI は常に直近ハッシュを送るため、多タブ同時編集は 409
  → 3-way 解決ダイアログで明示的にマージさせる。
- **is_locked 保護**: `is_locked = true` の行は PATCH / DELETE が 403。「Arknights 固定辞書」など
  消えてほしくない seed データを保護する用途。
- **allow-list の反映**: 現在は起動時 snapshot。`[[control_api.tables]]` を増減した場合は VAC 再起動が必要。
- **Flowgraph 側との関係**: Quick-Add / Editor が書き戻すのは **ファイル**（TSV）。
  flowgraph は `table.load_tsv` が都度読み直す実装のため、次の実行タイミングで自然に反映される。

#### 対応 Flowgraph ノード（`control_triggerable = true`）

- `flowgraph.glossary.learn` — Quick-Add の Learn ボタン
- `flowgraph.glossary.forget` — Quick-Add の Undo ボタン、Editor の削除

上記以外のノードに trigger API を打つと **400 control_triggerable_forbidden** が返る（安全装置）。

### 5.2 `[[control_api.quick_effect_presets]]` — Live Quick Effects preset

Live タブの **BGM / 画面効果キュー** に表示する preset。
未指定時は組み込みの RAID / チャット強調 / 画面効果 / BGM fade が使われます。
GUI の「Preset 保存」は現在のプロファイルにこの配列を書き戻し、既存 `id` と同じ preset は置換されます。

```toml
[[control_api.quick_effect_presets]]
id          = "raid"
label       = "RAID"
channel     = "effect"
content     = "RAID incoming"
flags       = ["obs.raid"]
duration_ms = 8000

[[control_api.quick_effect_presets]]
id          = "stinger"
label       = "Stinger"
channel     = "effect"
content     = "sting"
flags       = ["obs.stinger", "audio.hit"]
duration_ms = 5000
```

| キー | 型 | 既定値 | 説明 |
|---|---|---|---|
| `id` | string | — | 必須。GUI 保存時の置換キー |
| `label` | string | — | 必須。Live タブの preset ボタン表示 |
| `channel` | string | — | 必須。投入する `ChannelDatum.channel` |
| `content` | string | `""` | OBS template / Flowgraph effects に渡す payload |
| `flags` | string[] | `[]` | `ChannelDatum.flags` に追加するタグ |
| `duration_ms` | integer | `null` | active / scheduled 表示の有効時間。0 以下は未指定扱い |

#### 対応 Control API

- `GET /api/v1/control/quick_effects/presets` — 現プロファイルの preset、未指定時は組み込み preset を返す。
- `PUT /api/v1/control/quick_effects/presets` — `[[control_api.quick_effect_presets]]` を書き戻す。更新前に `.bak-YYYYMMDDHHmmSS` を作成し、`Conf` として再パース検証する。
- `POST /api/v1/control/quick_effects` — preset または custom payload を `ChannelDatum` として投入する。`fire_at_ms` 指定時は scheduled として遅延発火する。

---

## 6. 外部サービス接続（セクション単位）

以下は「外部サービスへの接続情報」であり、**データ配線は Flowgraph で行う**のが v2 の原則です。

### 6.1 `[twitch]`

Twitch OAuth / EventSub / チャット取り込み / モデレーション設定。詳細は [`conf.example-twitch.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-twitch.toml)。

主要キー:

- `client_id`, `client_secret`（環境変数 `VAC_TWITCH_CLIENT_ID` / `VAC_TWITCH_CLIENT_SECRET` 可）
- `broadcaster_login`：配信者の Twitch login（小文字）
- `[twitch.eventsub]` / `[twitch.moderator]`：EventSub / 別トークン（モデレーター権限）

`GET /api/v1/control/snapshot`はworkerのJOIN・接続状態だけを参照し、OAuth tokenの外部検証を行いません。保存tokenの有効性とscopeを明示確認する場合は`GET /api/v1/control/oauth/twitch/{broadcaster|moderator}/authorization`を使います。GUIも連携画面を開いたときだけこの専用APIを呼び、ホームの定期更新をTwitch APIの応答待ちにしません。

OAuth validateの成功結果は5分だけキャッシュします。長時間起動中にEventSubのHelix呼び出しが401を返した場合は、成功キャッシュを破棄し、保存済みrefresh tokenでアクセストークンを更新して1回だけ再試行します。refresh tokenも拒否された場合に限り、GUIからDevice Code Flowを再実行してください。

### 6.2 `[[ai.personas]]`（OpenAI Responses API）

AI Persona を 1 個以上定義する配列。Phase χ 以降は **OpenAI Responses API (`/v1/responses`)** を唯一の経路として使う（Chat Completions は撤去済み）。詳細は [`conf.example-openai-chat.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-openai-chat.toml)。

主要キー:

- `id`, `display_name`（GUI表示名）、`api_key`, `api_key_file`, `model`
- `observe.triggers`（購読するチャンネル名）, `channel_utterance`（発話を書き出すチャンネル名）
- `custom_instructions` / `system_instructions_extra`
- `turn_management = { enabled = true }`（一回のResponses API呼び出しで `observe` / `respond` / `react` / `close`、発話本文、短い会話エピソード記憶をJSON Schema出力させる。状態遷移はローカルで管理し、単発reactや通常実況は新規episodeを開始できない。`observe` は発話しない。`openai_stream=true`では発話片を先行出力し、構造化結果の確定後に身体表現等を処理する）
- `turn_management.avatar_sync_with_speech = true`（既定false。発話付きAvatarIntent/poseを同じutterance IDの音声再生受付まで待つ。`openai_stream=true`、`flowgraph.ingress.ai_speech`から共有Speech Scheduler対応TTSへの接続が必要。最大30秒待ち、再生なし・終了済み・失敗では身体表現を発行せず既存のavatar error診断に理由を残す。無言の反応は待たない。待機中はAIターンの確定処理も保留される。再生受付は音響的な発音開始の実測ではない）
- `turn_management.continuation_gap_ms = 10000`（省略または0で無期限。通常返答の音声片が正常終了した後、続きが再開できずに空いた時間の上限。再生中の音声の長さは含めず、期限後の未再生片を`continuation_gap_exceeded`で取り下げる。チャネポ等の優先依頼には適用しない。通常返答の履歴・EpisodeMemory・学習用ログは実再生本文を参照し、`ai_response`の確定通知は音声片の処理が落ち着いた時点で一度だけ出す。片ごとの字幕は[`on_played`と`spoken_text`](./tts-playback-output.md)を明示接続する。運用前に、生成した各片が同じutterance IDで一つの共有scheduler対応TTSへ届く構成と字幕／英訳経路を検証する）
- `observation_buffer = { settle_ms = 750, max_wait_ms = 5000 }`（近接した確定観測を一回のAI評価へまとめる。未指定時もこの既定値で有効。`min_interval_in_secs` 中の新しい観測は捨てず、最新観測へ更新して評価可能時刻まで保留する）
- `initiative = { enabled = true, interval_secs = 15, startup_grace_secs = 60, idle_secs = 180, min_interval_secs = 300, max_attempts_per_hour = 4, max_speeches_per_hour = 2 }`（Twitchの宣言値がonlineで、値自体が鮮度内またはEventSub sessionのkeepaliveが直近2分以内の場合だけ、開始時・継続中の実況・長時間沈黙時の自発参加を評価する。継続中の実況では`min_interval_secs`を最初の参加までの待機時間にも使い、候補化後は原則として短い一言を返す。ただし、直近の会話に明示的な返答不要の要求があり、その後に新しい呼びかけや応答要求がない場合は、その要求を優先して無言にする。「黙って見ている」という宣言も発話しない。この判断はモデルへ渡る会話履歴に依存し、永続的なミュート設定ではない。通常応答の処理中は自発参加を次回の評価へ保留し、その保留は試行予算を消費しない。候補は人間の`user`発話へ偽装せずdeveloper文脈として渡す。通常応答のDecision Engine・名前呼び・直接質問とは独立した試行/実発話予算。ブロック省略または`enabled = false`なら無効）
  - 試行回数と次回までの間隔は、モデルへの最初のリクエストを送る時点から数えます。人の発話を待っている間に取り消された候補は消費しません。送信後の通信失敗は試行に含めます。実発話数には `max_speeches_per_hour` の別上限があります。
- `assistant_max_sentences`（usize、省略可。生成後の発話本文を `。` / `！` / `？` / `!` / `?` の文末境界で指定文数までに制限する。配信音声の暫定安全弁であり、会話終端判定とは別）
- `openai_max_output_tokens`（u32, Responses API `max_output_tokens`）／ 環境変数 `VAC_OPENAI_MAX_OUTPUT_TOKENS` で上書き可
- `openai_reasoning_effort`（`"none"` / `"minimal"` / `"low"` / `"medium"` / `"high"` / `"xhigh"` / `"max"`。対応範囲はモデルごとに異なる）
  - GPT-6 Luna は `none` / `low` / `medium` / `high` / `xhigh` / `max`、GPT-6 Astra・GPT-6.1 Sol は `low` 以上を指定できます。これらでは `minimal` は使いません。未対応の値は警告して送信から外すため、モデルを変更する際はこの項目も確認してください。対応条件は[OpenAIのモデル案内](https://developers.openai.com/api/docs/models)を参照してください。
- `[ai.personas.openai_model_policy]`（`model`, `reasoning_effort`, `max_output_tokens`, `trigger_channels`, `trigger_request_classes`, `trigger_regex_pattern`。一致した入力だけ別モデルへ昇格）
- `openai_prompt_cache_key`（Prompt Cache の安定 routing key。未指定時は persona `id` から生成）
- `openai_store`（bool、既定 `false`。OpenAI 側に会話 state を保存するか。VAC は client 側で memory window を完全管理するため通常 `false` のままで良い）
- `openai_reasoning_encrypted_passthrough`（bool、既定 `true`）— **gpt-5 系の tool loop round 間で `reasoning.encrypted_content` を client 側で持ち回る**（Phase ψ-α）。`store: false` を維持したまま reasoning state を transit することで、複数ラウンドに渡る tool loop の thought continuity を保つ。非 gpt-5 モデル（`gpt-4o-mini` 等）では完全 no-op（request も log も無変化）。デバッグや実機計測の比較用に明示 `false` で opt-out 可能。詳細: [`docs/roadmap/phase-psi-alpha-encrypted-reasoning.md`](https://github.com/usagi/virtual-avatar-connect/blob/v2/docs/roadmap/phase-psi-alpha-encrypted-reasoning.md)
- `[ai.personas.openai_tool_policy]`（`require_strict`, `default_allowed`, `by_channel`, `by_request_class`。strict schema 検証と channel / 信頼済み依頼種別ごとの `allowed_tools` を適用。`by_request_class` が最優先）
- `[ai.personas.memory]` / `[ai.personas.decision]` / `[ai.personas.function_calling]`

配信AI共演者の初期推奨値は `gpt-5.6-luna`、`openai_reasoning_effort = "none"`、
`memory_capacity = 4`、`memory_overflow_summary_enabled = false`。
短期窓から落ちるたびに別APIで要約せず、Turn ManagerのActive Episodeが話題と根拠を短く保持する。
完了直後のrefractory中でも、別の明示的な質問は新しいEpisodeとして開始する。
各Responses API要求には、静的prompt cacheの後ろへ実行ホストの現在日時を信頼済みdeveloper文脈として追加する。
これにより「今年」「今日」を実行時点で解決し、既知の現在年を利用者へ聞き返さない。

**互換性メモ**:

- legacy `max_tokens` (u16) は `openai_max_output_tokens` が未指定のときだけ fallback として使われる。両方指定時は新キーが優先。
- `memory_overflow_summary_max_output_tokens` (u32) が推奨。legacy `memory_overflow_summary_max_completion_tokens` (u16) は fallback。
- `openai_tools_json_path` は Responses API 形式 (`{"type":"function","name":...}`) と旧 Chat Completions 形式 (`{"type":"function","function":{...}}`) の双方を自動判別するので、既存 conf はそのまま動く。tools は **`openai_tools_json_path` に外部 JSON ファイルのパスを指定する方式のみサポート**（TOML 内に `openai_tools_json = """[...]"""` を inline で書いても黙って無視される）。

**環境変数オーバーライド**:

| 変数名 | 上書き対象 | 備考 |
|---|---|---|
| `VAC_OPENAI_API_KEY` | `api_key` | API キーは conf 直書きよりこちらを推奨 |
| `VAC_OPENAI_MAX_OUTPUT_TOKENS` | `openai_max_output_tokens` | env > 新キー > legacy `max_tokens` の順で解決 |

OpenAI API keyの解決順は `VAC_OPENAI_API_KEY`、`api_key_file`、`api_key`。desktop managed profileのResources画面から保存したkeyは `credentials/openai-api-key.txt` に分離され、confには `api_key_file` の相対pathだけが記録される。`credentials/` はworkspace bundleへの取り込み対象外。

各起動セッションでは、本文を除いたAI判断、音声認識、TTS、Twitch受信の診断JSONLをruntime session directoryの`control-events.diagnostic.jsonl`へ逐次保存する。GUIのSnapshotに表示される`session_dir`から取得でき、`ChannelDatum.content`、音声認識`text`、AIの`speak`とepisode memoryは記録しない。runtime sessionは一時領域であり、次回起動時に24時間以上古い終了済みsessionを整理する。稼働中の別VACが生存ロックを保持しているsessionは残す。生存ロックを持たない旧版のsessionや識別できないディレクトリは自動削除しない。障害調査に必要な診断ファイルは別の保存先へ退避する。この保護は更新後の整理処理に適用され、旧版自身の整理動作は変更しない。

### 6.3 TTS

TTS 関連は **conf.toml に何も書く必要がない**。すべて `flowgraph.tts.speak` ノードの入力で決める。
詳細は [`conf.example-tts.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-tts.toml)。

`flowgraph.tts.speak` の `respect_speech_floor` にVoice入力と同じキー（例: `human_voice`）を渡すと、合成後・各音声片の再生前にも人の発話終了を待つ。空文字は従来動作。persona側の同名設定はAI生成前の待機であり、この再生直前の待機とは別。共有runtime stateとscheduler対応driverが必要で、非対応時はエラーになる。再生中の音声を途中停止する機能ではなく、待機中は共有音声キューの後続も待つ。頻度やinitiativeの設定は変更しない。

合成失敗は人の発話終了を待たずに通知する。待ち時間上限を設定した音声は、人の発話終了を待っている間も上限を適用する。上限なしの設定に新しい期限を追加するものではない。

### 6.4 OCR / Screenshot（Windows）

基本 conf.toml に設定不要。詳細は [`conf.example-ss-ocr.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-ss-ocr.toml)。

### 6.5 Translation（GAS / LibreTranslate）

基本 conf.toml に設定不要。`[libretranslate]` に auto-embed 用の設定のみ任意で。詳細は [`conf.example-translate.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-translate.toml)。

### 6.6 Voice 認識（Vosk / Whisper）

音声入力は `flowgraph.ingress.voice` ノードで定義します。`engine` / `model_path` / `language_code` などはノード properties に書きます。詳細は [`conf.example-voice.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example-voice.toml) と [`voice-input`](tutorials/voice-input.md)。

### 6.7 Browser Source（`[browser_source]`）

OBS Browser Source 用 HTML のルート指定。

```toml
[browser_source]
document_root = "output"   # 既定: "output"。/output/* と /browser-output/* が参照
```

### 6.8 `run_with`

UNVAC が監視・操作する外部プロセスの配列。詳細は [`conf.example--run-with.toml`](https://github.com/usagi/virtual-avatar-connect/blob/v2/conf.example--run-with.toml)。

table 形式の entry は Managed App として扱われ、`id` / `label` / `if_not_running` に加えて UNAA/Z discovery との相関用に次の任意キーを持てます。

```toml
run_with = [
  {
    command = "C:/UN/un-avatar/un-avatar.exe",
    if_not_running = "un-avatar",
    id = "un-avatar",
    label = "UN Avatar",
    start_on_launch = false,
    unaa_z_app_id = "un-avatar",
    unaa_z_instance_id = "main",
  },
]
```

`start_on_launch = false` を指定すると、UNVAC起動時には開始せず、連携アプリ画面の手動操作または Runtime Mode の `managed_apps.start` からだけ起動します。未指定時は後方互換のため `true` です。

`shutdown.on_vac_exit` はVAC自身の終了時だけの操作指定です。`leave_running`（残す）、`close_only`（終了を要求し強制終了しない）、`close_and_wait`（待機後に強制終了）を指定できます。未指定時は既存の `shutdown.action` を継承します。手動操作・モード切替による終了要求では `shutdown.action` を使用するため、たとえば `{ action = "close_only", on_vac_exit = "leave_running" }` ならVAC終了時は残し、個別操作では終了を要求できます。`method`、`grace_ms`、`app_specific` は両経路で保持されます。

`shutdown.scope` は終了するプロセスの範囲です。`process_tree`（未指定時の従来動作）は登録アプリと子孫プロセス、`matched_processes` は判定条件に一致する登録アプリだけを対象にします。終了要求・終了待ち・強制終了・アプリ固有処理に同じ範囲を適用します。UNA/UNMのSupervisor自身にRenderer/Capturerの終了判断を任せる場合は `matched_processes` と `action = "close_only"` を組み合わせます。トレイへ収納する設定や未保存確認がある場合、終了要求後もアプリが残ることがあります。


連携アプリ編集フォームではモード別の起動・終了・最小化・維持を登録内容と一緒に保存できます。保存だけで切替や起動は実行しません。変更は次回のモード切替計画へ反映されます。新規登録フォームの初期値は自動起動なし・強制終了なしですが、既存の設定値を自動変更することはありません。


`unaa_z_app_id` は Zenoh discovery 上の app id、`unaa_z_instance_id` は同一 app の複数起動を区別する instance id です。未指定時は Managed App の `id` を仮の相関キーに使います。

---

## 7. 設定の検証

配信や長時間運用の前は、release packageのCLIでprofileを副作用なしに検証します。

```console
virtual-avatar-connect-cli.exe "C:\path\to\profile\conf.toml" --validate-conf
```

この検証は外部アプリや音声入力を起動せず、次を一括確認します。

- confの構文とRuntime Mode。
- `un_avatar.links` のcapabilityとUNMF/Z endpoint。
- AI credential fileとOpenAI tools JSON、strict schema、tool許可集合。
- `flowgraph_dir`をconf所在directoryから解決した全Flowgraphのロード結果。

通常起動時にも同じconf型検査が走り、エラーはstderrとGUIのDiagnosticsに出ます。よくあるエラー:

| エラー | 原因 |
|---|---|
| `[[processors]] は v2 では禁止` | conf.toml に `[[processors]]` が残っている。V1 processor 経路は廃止済みなので Flowgraph へ移行する |
| `flowgraph_dir が見つからない` | パスの typo。存在しなくても起動は通るので通常は警告のみ |
| `control_api token が 64 文字未満` | 手動指定時の長さチェック。自動生成なら問題なし |

より詳細な設定リファレンスは各 `conf.example-*.toml` 内のコメントが正本です。

### `ai.personas.openai_few_shot` — 応答の見本

見本のuser/assistantメッセージは口調や応答方法を示すために使用します。実際の会話履歴や現在のゲーム状態の記録には使いません。AI入力では本文とroleを持つ引用例としてまとめ、後続の実際の観測・会話と区別します。見本内の話題で曖昧な実況を補完したり、実際の出来事として会話メモリに記録したりしないよう明示します。設定済みの見本本文・roleは引用内に保持します。system/developerの設定項目は引用に変換せず、従来の指示メッセージとして順序を維持します。


### `perception.image_archive` — 採用画像の診断保存

任意設定。`directory`は専用の絶対パス、`max_bytes`は画像＋JSONの上限（既定1,000,000,000、範囲1 MB～10 GB）。`streaming` modeで最初のAIリクエストに採用された設定済みOBS画像だけを保存し、古い記録から整理します。未設定なら無効。詳細と保存形式は[ゲーム画面の観測](ai-stream-perception.md#採用画像のローカル診断保存任意)を参照。

### turn_management.visual_history_enabled

`[[ai.personas]]`の`turn_management`内のbool、既定false。Turn Manager有効時のみ、同じAPI応答から最新画像に対応する短い読取メモを取得する。最大3件・各160文字・取得後60秒、メモは非信頼の過去状態として扱う。追加画像/定期モデル呼出しはなし。詳細は[画面観測](ai-stream-perception.md)を参照。

### turn_management.past_screen_image_enabled

bool、既定false。`enabled`と`visual_history_enabled`も有効な場合に限り、明示的な過去画面参照の発言へ、同じscene/sourceの以前の画像を最大1枚追加する。既定値と個々のプロファイルでの有効・無効は別であり、保存したプロファイル設定で決まる。過去画像には元の取得時刻と別IDを付ける。現在画像の応答直前更新には、取得元の`refresh_before_reply = true`も必要。

利用期限は取得から60秒。新たな撮影や別source検索は行わず、応答入力へ選ばれた画像から直前の異なる画像を保持する。複数画像・source不明・取得欠落・scope変更では履歴を使わない。発動は「さっきのスキル」「前の画面」等の語句判定であり、すべての言い換えや数分前の場面を理解・検索する機能ではない。対象の応答は入力画像が最大2枚となり、入力量・待ち時間が増える可能性がある。利用条件と確認方法は[条件付きの過去画像](ai-stream-perception.md#条件付きの過去画像任意)を参照。

### openai_tool_policy.effect_allowed_channels

`[[ai.personas]]`の`openai_tool_policy`内の任意の文字列配列。例：`effect_allowed_channels = ["effect", "reaction"]`。`vac_emit_effect`が使える送り先を列挙する。未指定では従来の任意チャンネル指定を維持する。

指定時は`channel_effect`（未指定なら`effect`）を必ず含める。AIへ渡すchannel引数の選択肢を限定し、実行時にも照合する。許可外への要求はチャンネルへ書き込まずエラーを返す。null/省略は従来通り`channel_effect`へ送る。既存ツール定義のenum制限は広げず、許可リストとの共通部分を使う。観測・ユーザー発言など入力経路への誤送信を避けつつ、必要な複数の演出先を設定できる。

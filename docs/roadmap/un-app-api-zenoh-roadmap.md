# UN App API over Zenoh Roadmap

> Status: accepted design note. 本書は UNVAC が un-avatar / un-motion / un-motion-frame などの U.N. シリーズアプリを統合制御するための制御プレーン計画である。正式名称は **UN App API over Zenoh**、略称は **UNAA/Z** とする。実装者向けプロトコル正本は [`../manual/un-app-api-over-zenoh.md`](../manual/un-app-api-over-zenoh.md)。

---

## 1. Naming

- 正式名称: **UN App API over Zenoh**
- 略称: **UNAA/Z**
- 読み方: `ユーエヌエーエー・ゼット`
- 範囲: U.N. シリーズアプリの discovery / status / command / capability / event / endpoint advertisement を扱う Zenoh ベースの汎用操作 API。

UNAA/Z は UNMF/Z とは別の層である。

- **UNMF/Z**: 姿勢・表情・motion frame の transport / storage / conversion。
- **UNAA/Z**: アプリそのものの発見、状態監視、操作、capability 交渉、profile / renderer / supervisor 系の制御。

---

## 2. Position

UNVAC v2 は「配信者・AI・アバターをつなぐ、グラフィカル統合データフロー・システム」である。UNVAC 内部では Flowgraph が統合ロジックを担い、UNAA/Z は U.N. シリーズアプリを Flowgraph / Runtime Mode / Managed App 管理へ接続する制御プレーンになる。

```text
UNVAC Flowgraph / Runtime Mode / GUI
  -> UNAA/Z control plane
  -> UNA / UNM / UNMF tools / future U.N. apps

UNVAC Flowgraph motion pipeline
  -> UNMF / UNMF/Z data plane
  -> UNA / UNM / VMC compatible apps
```

VMC は引き続き互換入出力として扱う。ただし内部の姿勢・表情コアは UNMF、送受信コアは UNMF/Z を優先し、VMC は `VMC -> UNMF` / `UNMF -> VMC` adapter として扱う。

---

## 3. Goals

- UNA / UNM / UNMF tools を「単なる外部プロセス」ではなく U.N. managed app として扱う。
- アプリごとの個別 HTTP API や CLI 依存を増やす前に、capability discovery と typed command envelope を固定する。
- UNVAC は全機能を初期から制御しない。既知 capability だけを扱い、未知 capability は無視または GUI で観測に留める。
- Supervisor Console / Renderer で先行している Zenoh pattern を UN シリーズ全体へ拡張できる設計にする。
- Flowgraph ノード化は後段とし、まず Managed App / Resources / Diagnostics / Runtime Mode から接続する。

---

## 4. Initial Surface

### Discovery / Info

- `app.info`: app id, display name, version, build, vendor, protocol versions.
- `app.capabilities`: `renderer`, `motion_input`, `motion_output`, `avatar_load`, `scene_control`, `effect_control`, `profile_control`, `supervisor_control` など。
- `app.endpoints`: UNMF/Z endpoints, VMC endpoints, GUI URLs, local control URLs if any.

### Zenoh Key Contract

UNAA/Z discovery の first stable contract は次の key expression を使う。

- Snapshot queryable: `un/app/{app_id}/{instance_id}/snapshot`
- Announcement publish: `un/app/{app_id}/{instance_id}/announce`
- Liveliness token: `un/app/{app_id}/{instance_id}/live`
- Command publish: `un/app/{app_id}/{instance_id}/command`
- Command response publish: `un/app/{app_id}/{instance_id}/response/{request_id}`
- App event publish: `un/app/{app_id}/{instance_id}/event`
- Compatibility namespace: `unaa_z/{app_id}/{instance_id}/snapshot` / `announce` / `live`

`snapshot` と `announce` の payload は JSON object とし、最低限 `app_id` または `id` を持つ。推奨 payload は `app_id`, `instance_id`, `display_name`, `running`, `ready`, `capabilities`, `commands`, `endpoints`。複数アプリをまとめる aggregator は `{ "apps": [...], "endpoints": [...] }` の snapshot payload を返してよい。`commands` は string 配列、または `{ "command": "focus" }` / `{ "name": "focus" }` / `{ "id": "focus" }` の object 配列を受け付ける。

UNVAC は discovery で destructive command を送らない。diagnostics では `snapshot` queryable を優先し、短い timeout 内で `announce` も読む。`live` は存在確認・将来の readiness correlation 用で、command surface ではない。

### Status / Health

- `app.status`: running, ready, busy, profile id/name, renderer state, current scene/avatar if exposed.
- `app.health`: heartbeat, uptime, last error, warning count, degraded reason.
- `app.events`: status changed, capability changed, profile changed, avatar changed, error, warning.

### Commands

- `app.command`: typed command envelope with `command`, `args`, `request_id`, `timeout_ms`.
- Required generic commands: `reload`, `reset`, `focus`, `minimize`, `open_gui`.
- Optional commands by capability: `profile.load`, `profile.save`, `avatar.load`, `scene.load`, `effect.trigger`, `motion.route.set`.

UNVAC first-slice sender は destructive command を直接開かない。Control API から publish できる command は allow-list 方式で、初期値は `focus`, `open_gui`, `reload.preview`, `profile.switch.preview`。送信 envelope は JSON object とし、`schema_version`, `kind = "app.command"`, `app_id`, `instance_id`, `command`, `args`, `request_id`, `timeout_ms`, `issued_at` を含む。

Command response は `response/{request_id}` への JSON publish を主経路とする。app-wide `event` に command result を流す場合は payload に `request_id` または `requestId` を含める。UNVAC の bounded wait は response key を優先し、event key は `request_id` 一致 payload だけを同じ command の応答として扱う。

実装メモ（2026-05 command gating）: UNVAC は discovery app payload の `commands` と safe allow-list の交差だけを GUI / Control API で許可する。`commands` が未宣言の場合だけ、`capabilities` から低リスク command を控えめに導出する。現在の導出は `supervisor_control` / `app_control` / `window_control` / `gui` / `renderer` から `focus`, `open_gui`、`supervisor_control` / `app_control` / `profile_control` から `reload.preview`, `profile.switch.preview`。Zenoh discovery で app が見えている場合、Control API はこの gating に失敗した command を publish 前に拒否する。

### Profiles

- `profile.list`
- `profile.load`
- `profile.save`
- `profile.current`

Profile operation is capability gated. UNVAC should not assume every U.N. app has the same profile model.

---

## 5. UNVAC Integration Order

### UNAA-0 Docs / Vocabulary

- Fix naming: **UN App API over Zenoh** = **UNAA/Z**.
- Document separation from UNMF/Z.
- Update roadmap / architecture / v2 motion plan.

### UNAA-1 Discovery Snapshot

- Add a Zenoh-backed discovery reader or mockable abstraction.
- Surface discovered apps in Resources / Managed Apps diagnostics.
- No Flowgraph nodes yet.

実装メモ（2026-05 first slice）: Control API `GET /flowgraph/diagnostics` は `unaa_z` discovery snapshot と `unaa_z_endpoint_candidates` を返す。source は `managed_app_registry` を基底に、timeout-bounded Zenoh JSON reader が成功した場合は `managed_app_registry+zenoh` になる。reader は既定で `un/app/**/snapshot` / `unaa_z/**/snapshot` を query し、`un/app/**/announce` / `unaa_z/**/announce` も購読して snapshot / app / endpoint payload を diagnostics snapshot に合流する。`UNVAC_UNAAZ_DISCOVERY_QUERY_KEYS`、`UNVAC_UNAAZ_DISCOVERY_KEYS`、`UNVAC_UNAAZ_DISCOVERY_TIMEOUT_MS`、`UNVAC_UNAAZ_ZENOH_DISCOVERY=0` で運用調整できる。flattened endpoint candidate は app-scoped endpoint なら `app_id` / `instance_id` / `source_app_display_name` / `discovery_source_key` / `discovery_payload_kind` / `discovery_stale` / `discovery_age_ms` を補完し、root endpoint でも discovery source key / payload kind を保持する。

実装メモ（2026-05 FlowGraph provenance）: FlowGraph palette から UNMF/Z endpoint graph を生成すると、`flowgraph.literal.json` payload は `unaa_z_provenance` と `endpoints` を持つ。`unaa_z_provenance` は app / instance / endpoint / base key / topic mode / discovery source key / payload kind / stale を保存する。生成ノード群は同じ情報を短縮した group label で囲まれるため、保存後の graph でも接続元 metadata を追跡できる。

実装メモ（2026-05 Inspector）: FlowGraph group Inspector は member の JSON literal payload から `unaa_z_provenance` を読み、app / instance / endpoint / mode / base key / discovery source key / stale を表示する。`app_id`、`discovery_source_key`、provenance JSON 全体は UI から clipboard へコピーできる。

実装メモ（2026-05 validation）: UNMF/Z endpoint candidate は `app_id` / `endpoint_id` / `base_key_expr` / `direction` を検証する。backend は candidate JSON に `validation_warnings` を付与し、diagnostics response に `unaa_z_endpoint_candidate_diagnostics` を同梱する。FlowGraph palette は warning badge と tooltip で metadata 欠落や不正 `direction` を表示する。

実装メモ（2026-05 observability）: Resources 概要は endpoint candidate validation warning の総数、対象 app 数、running app 行の warning 数を表示する。Observability の UNAA/Z command 履歴パネルは command history と同じ更新導線で endpoint diagnostics を読み、app / endpoint / code / field / source key / message を一覧できる。

実装メモ（2026-05 fixture）: `tests/fixtures/unaa_z/discovery_snapshot_mixed.json` は UNAA/Z discovery payload の回帰 fixture。complete な app-scoped UNMF/Z input endpoint、不完全な UNMF/Z output endpoint、root endpoint を含む。backend test はこの fixture から flattened candidate の GUI-facing fields、app provenance、`validation_warnings`、diagnostics aggregation を検証する。

実装メモ（2026-05 GUI parser）: FlowGraph GUI の UNMF/Z endpoint candidate parser は `gui/src/lib/flowgraph/unmfzEndpointCandidates.ts` に分離した。`npm run test:unit` は同じ discovery fixture を Node から直接読み、complete / invalid endpoint、fallback、warning、dedupe / sort を検証する。これにより browser UI と unit smoke が同じ正規化契約を使う。

実装メモ（2026-05 GUI E2E）: `gui/tests/e2e/flowgraph-unmfz-endpoints.spec.ts` は production `/gui/` 上で UNMF/Z endpoint candidate UI を検証する。diagnostics route に共有 fixture 由来の candidates を注入し、Palette warning badge、Publish 生成、group Inspector の `unaa_z_provenance` 表示を固定する。fixture server の Zenoh runtime 要件に合わせ、CLI entrypoint は Tokio multi-thread scheduler で起動し、actix worker など current-thread runtime 上の Zenoh 処理は専用 1-worker multi-thread runtime へ退避する。

実装メモ（2026-05 diagnostics API E2E）: `gui/tests/e2e/flowgraph-unaa-z-diagnostics-api.spec.ts` は route mock なしで production fixture server の `/api/v1/control/flowgraph/diagnostics` を呼び、`unaa_z` discovery snapshot、endpoint candidates、endpoint diagnostics が JSON contract として返ることを確認する。直後に `/api/v1/control/ping` も叩き、Zenoh discovery probe 後も server が生存していることを固定する。

実装メモ（2026-05 queryable smoke）: Rust test `zenoh_queryable_snapshot_flows_into_endpoint_candidates` は UNVAC 内 fake UNAA/Z snapshot queryable を別 Zenoh session で立て、query reply 由来の app metadata、UNMF/Z endpoint candidates、`discovery_source_key` / payload kind、invalid endpoint diagnostics を検証する。UNA / UNM 実体にはまだ依存しない。これで discovery reader が announcement だけでなく queryable snapshot 契約から FlowGraph 候補を作れることを固定する。

実装メモ（2026-05 health summary）: Control API `GET /flowgraph/diagnostics` は `unaa_z_health` を返す。source、Managed App registry / Zenoh app 件数、stale app 件数、root endpoint 件数、endpoint candidate 件数、candidate warning 件数、warning 対象 app 件数、discovery error 件数、query / announce key 件数を集約し、Resources 概要から discovery 状態を一目で確認できる。

### UNAA-2 Managed App Binding

- Extend Managed App metadata with optional UNAA/Z identity.
- Correlate `run_with` entries and Zenoh-discovered apps by app id / instance id.
- Runtime Mode desired state may start/stop the process while UNAA/Z reports readiness and capabilities.

実装メモ（2026-05 first slice）: `run_with` table entry は任意キー `unaa_z_app_id` / `unaa_z_instance_id` を持てる。Managed App API と registry はこの identity を返し、`unaa_z` diagnostics snapshot では Managed App `id` の代わりに指定された UNAA/Z identity を app / instance key として使う。未指定時は従来通り Managed App `id` を仮の相関キーにする。RunWith editor GUI からも新規追加時に指定可能。

実装メモ（2026-05 Resources UI）: `GET /managed_apps` は各 entry に `unaa_z_status` を同梱する。`unaa_z_status` は app / instance id、discovery source、Zenoh 由来 payload の有無、ready、capability count、endpoint count を持つ。Resources 概要と連携アプリ drawer は process running と UNAA/Z ready を分けて表示し、process は起動中だが Zenoh discovery が未到達の entry を観測できる。

### UNAA-3 Command Bridge

- Add Control API endpoints for safe generic command dispatch.
- GUI exposes known low-risk commands first: focus, open GUI, reload, profile switch preview.
- Unknown or dangerous commands remain hidden until capability policy is designed.

実装メモ（2026-05 first slice）: `POST /api/v1/control/un_app_api/command` を追加した。target は `managed_app_id` または `app_id` / `instance_id` で指定し、Managed App 指定時は `run_with` の `unaa_z_app_id` / `unaa_z_instance_id` binding を使う。backend は safe allow-list を通過した command だけを `un/app/{app_id}/{instance_id}/command` へ JSON envelope として Zenoh publish し、`202 Accepted` で `request_id` と key expression を返す。Managed App drawer は Zenoh discovery 由来で ready な app に focus / open GUI / reload preview ボタンを出す。

実装メモ（2026-05 response wait）: request body に `wait_response_ms` を追加した。`wait_response_ms > 0` の場合、backend は response key `un/app/{app_id}/{instance_id}/response/{request_id}` と event key `un/app/{app_id}/{instance_id}/event` を publish 前に購読し、bounded wait 内に response JSON または `request_id` 一致 event JSON を受けたとき `200 OK` と `response_observed = true` を返す。応答がなければ従来通り `202 Accepted` と `response_observed = false`。

実装メモ（2026-05 observability）: `State` に UNAA/Z command history ring buffer を追加し、`GET /api/v1/control/un_app_api/commands/history` で直近 command の `request_id`、publish key、response key、event key、status、response payload、publish error を返す。Observability タブには UNAA/Z command 履歴パネルを追加し、response observed / accepted timeout / publish failed を視認できる。

実装メモ（2026-05 command gating）: `GET /managed_apps` の各 `unaa_z_status` は raw `commands` と `allowed_commands` を返す。Managed App drawer は `allowed_commands` に含まれる low-risk command ボタンだけを表示する。command API も discovery で対象 app を確認できた場合は同じ `allowed_commands` 判定を使い、未宣言または unsafe な command を拒否する。

実装メモ（2026-05 contract endpoint）: `GET /api/v1/control/un_app_api/contract` は実アプリ実装者向けの機械可読契約を返す。内容は snapshot / announcement key、command / response / event key template、safe command、capability fallback、snapshot / command response / command event sample。文章版は `docs/manual/un-app-api-over-zenoh.md` に置く。

実装メモ（2026-05 integration smoke）: web control unit tests に fake Zenoh app smoke を追加した。テストは app 側 session が command key を subscribe し、UNVAC の bounded wait helper が publish した envelope を検証してから `response/{request_id}` に JSON response を返す。Zenoh session が開けない環境では skip ログで終了する。

実装メモ（2026-05 history binding）: Control API command handler の publish / bounded wait / history append を共通 helper に切り出した。web control 側の fake Zenoh app smoke は同じ helper を通して `response_observed` history item が残ることを検証するため、Observability の履歴表示と command response wait の結合が崩れにくい。

実装メモ（2026-05 GUI observability）: Resources 概要は直近 UNAA/Z command 履歴から response / timeout / publish failure の小計を表示し、起動中 Managed App 行に最新 command status を併記する。Managed App drawer は app / instance ごとの最新 command を `last={command}:{status}` として表示し、command 送信後に履歴を再取得する。

実装メモ（2026-05 stale status）: discovery app payload の `reported_at` / `updated_at` / `announced_at` / `timestamp` / `time` を RFC3339 timestamp として読み、観測時刻との差が `UNVAC_UNAAZ_DISCOVERY_STALE_MS`（既定 10000ms）を超えた app を stale とする。Managed App API は observed / reported timestamp、age ms、stale bool を返し、GUI は UNAA/Z stale badge と command history stale 表示で古い discovery / command result を区別する。

実装メモ（2026-05 provenance）: Zenoh discovery payload を `source_key` / `kind` 付きで内部保持し、app metadata に `discovery_source_key` と `discovery_payload_kind` を保存する。`kind` は `query`, `announce`, `managed_app_registry`, `missing` を基本とする。Managed App drawer と Resources 概要は `via={kind}` を表示し、drawer では source key を tooltip で確認できる。

### UNAA-3.5 First-slice Closure

UNVAC v2 の UNAA/Z first-slice はここで完了扱いにする。

完了済み:

- Protocol vocabulary and canonical manual.
- Managed App identity binding.
- Timeout-bounded Zenoh discovery through queryable snapshots and announcements.
- Discovery provenance, stale status, endpoint candidate flattening, and endpoint validation diagnostics.
- Safe command bridge, bounded response / event wait, command history, and contract endpoint.
- Resources / Managed App drawer / Observability visibility.
- UNMF/Z endpoint advertisement to FlowGraph palette, generated graph provenance, Inspector visibility, and GUI / Rust / API / fake-app smoke tests.

次フェーズへ送る:

- `flowgraph.un.app.status`, `flowgraph.un.app.command`, and `flowgraph.un.app.event`.
- Capability / effect policy for Flowgraph app-control nodes.
- UNA / UNM / UN Avatar Renderer real-app interoperability tests after those apps implement UNAA/Z.
- Broader command families such as profile load / save, scene routing, and app-specific actions.

境界:

- UNAA/Z first-slice is the control plane foundation, not a promise that every U.N. app is already controllable.
- Existing tests intentionally depend on UNVAC-local fake UNAA/Z apps and fixtures until UNA / UNM adopt the protocol.
- UNMF/Z remains the motion data plane. UNAA/Z endpoint advertisement only helps discover and wire UNMF/Z routes.

### UNAA-4 Flowgraph Nodes

- `flowgraph.un.app.status` (first slice done)
- `flowgraph.un.app.command` (first slice done)
- `flowgraph.un.app.event` (first slice done)
- Capability metadata must mark these as network / app-control effects.

実装メモ（2026-05 status node）: `flowgraph.un.app.status` は Pure node として追加した。入力は UNAA/Z discovery snapshot JSON または `GET /flowgraph/diagnostics` 全体 JSON、`app_id`、任意 `instance_id`。出力は found / ready / running / stale、app / instance / display name / discovery source、capability / command / endpoint counts、capabilities / commands list、app JSON。Zenoh session は開かず、command / event node の app-control effect policy は後続に分離する。

実装メモ（2026-05 command node）: `flowgraph.un.app.command` は Effectful node として追加した。入力は exec、UNAA/Z snapshot JSON、app / instance、safe command、args、timeout、wait response、dry-run。dry-run は Zenoh に送信せず dispatch key / request id を作る。実送信時は既存 UNAA/Z command bridge helper を使い、snapshot 内に該当 app がある場合は advertised `commands` または `capabilities` fallback と safe allow-list の積で gating する。node capability metadata は `network` + `app_control`。

実装メモ（2026-05 event node）: `flowgraph.un.app.event` は first slice では常駐 ingress ではなく bounded wait polling node とした。入力は exec、app / instance、request id、wait response、dry-run。`un/app/{app_id}/{instance_id}/response/{request_id}` と `un/app/{app_id}/{instance_id}/event` を短時間購読し、response または request id 一致 event を payload として返す。常駐 event stream / ingress 化は FlowGraph の subscription lifecycle 設計後に送る。

### UNAA-5 UNMF/Z Endpoint Advertisement

- Use UNAA/Z app endpoint metadata to auto-wire UNMF/Z send/receive candidates in GUI.
- Keep VMC routes visible as compatibility routes, not as the primary internal model.

---

## 6. Non-goals

- Do not replace UNMF/Z with UNAA/Z.
- Do not require every U.N. app to expose every feature.
- Do not expose all commands directly to Flowgraph before capability / effect policy is settled.
- Do not make VMC the internal motion core. VMC remains a compatibility adapter.

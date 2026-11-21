# UN App API over Zenoh (UNAA/Z) Protocol

Status: draft stable contract for UNVAC v2 first-slice integration.

UNAA/Z is the U.N. series application control plane. It is used by UNVAC to discover U.N. apps, observe readiness and capabilities, advertise app endpoints, and send low-risk app commands. Motion and expression frame transport remain the UNMF/Z data plane; VMC remains a compatibility adapter.

This document is the implementation reference for U.N. apps adding UNAA/Z support. The live machine-readable UNVAC contract is also exposed at:

```text
GET /api/v1/control/un_app_api/contract
```

## 1. Versioning

Every JSON payload defined here should include:

```json
{ "schema_version": 1 }
```

UNVAC currently accepts payloads without `schema_version` for forward compatibility during rollout, but U.N. apps should emit `schema_version: 1`.

Unknown fields are allowed. Receivers must ignore fields they do not understand.

## 2. Namespaces

Primary key namespace:

```text
un/app/{app_id}/{instance_id}/...
```

Compatibility namespace accepted by UNVAC discovery:

```text
unaa_z/{app_id}/{instance_id}/snapshot
unaa_z/{app_id}/{instance_id}/announce
unaa_z/{app_id}/{instance_id}/live
```

New implementations should use `un/app/...`. The `unaa_z/...` namespace exists only for compatibility.

`app_id`, `instance_id`, and `request_id` are key path segments. They must be non-empty and must not contain `/`, `*`, `?`, or `#`.

## 3. Discovery

Each app should provide a queryable snapshot and may also publish announcements.

```text
Snapshot queryable:     un/app/{app_id}/{instance_id}/snapshot
Announcement publish:  un/app/{app_id}/{instance_id}/announce
Liveliness token:      un/app/{app_id}/{instance_id}/live
```

UNVAC currently queries:

```text
un/app/**/snapshot
unaa_z/**/snapshot
```

UNVAC currently subscribes briefly to:

```text
un/app/**/announce
unaa_z/**/announce
```

Snapshot and announce payloads are JSON objects. A single-app payload should look like:

```json
{
  "schema_version": 1,
  "app_id": "un-avatar",
  "instance_id": "renderer-main",
  "session_id": "renderer-main-20260907T120000Z-7a3f",
  "display_name": "UN Avatar Renderer",
  "version": "2.0.0",
  "running": true,
  "ready": true,
  "capabilities": ["renderer", "motion_input", "profile_control", "camera_control"],
  "commands": ["focus", "open_gui", "reload.preview", "camera.effect"],
  "updated_at": "2026-05-30T00:00:00Z",
  "endpoints": [
    {
      "protocol": "unmfz",
      "endpoint_id": "motion-in",
      "label": "Renderer Motion In",
      "direction": "input",
      "base_key_expr": "un-motion/frame/renderer-main",
      "topic_mode": "frame",
      "schema_major": 1
    }
  ]
}
```

Aggregators may return multiple apps and root endpoints:

```json
{
  "schema_version": 1,
  "apps": [
    {
      "app_id": "un-avatar",
      "instance_id": "renderer-main",
      "display_name": "UN Avatar Renderer",
      "ready": true,
      "capabilities": ["motion_input"],
      "commands": ["focus", "open_gui"],
      "endpoints": []
    }
  ],
  "endpoints": []
}
```

### Discovery Fields

Required for app-scoped payloads:

- `app_id`: Stable app family id, for example `un-avatar` or `un-motion`.
- `instance_id`: 同じ app family 内で Renderer を区別する安定した論理 ID。プロセス再起動で変えない。

Recommended:

- `display_name`: Human-readable app name.
- `session_id`: 起動ごとに変わる実行セッション ID。`camera.effect` のような状態変更命令では必須。
- `version`: App version string.
- `running`: Whether the app process/service is running.
- `ready`: Whether the app can accept commands or provide endpoints.
- `capabilities`: String array of capability tokens.
- `commands`: Command advertisement. See §5.
- `endpoints`: Endpoint advertisement. See §4.
- `reported_at`, `updated_at`, `announced_at`, `timestamp`, or `time`: RFC3339 timestamp.

UNVAC treats a timestamp older than `UNVAC_UNAAZ_DISCOVERY_STALE_MS` as stale. The current default is `10000` ms. If no timestamp is present, UNVAC does not mark the payload stale by age.

## 4. Endpoint Advertisement

UNAA/Z endpoint advertisement tells UNVAC what data-plane or local-control endpoints an app exposes. UNMF/Z endpoint metadata is currently consumed by FlowGraph Studio.

UNMF/Z endpoint object:

```json
{
  "protocol": "unmfz",
  "endpoint_id": "motion-in",
  "label": "Renderer Motion In",
  "direction": "input",
  "base_key_expr": "un-motion/frame/renderer-main",
  "topic_mode": "frame",
  "schema_major": 1
}
```

Fields:

- `protocol`: Must be `unmfz` or `unmf/z` for UNMF/Z endpoints.
- `endpoint_id`: Stable id within the app.
- `label`: Human-readable endpoint name.
- `direction`: `input`, `output`, or `both`, from the app's point of view.
- `base_key_expr`: Base Zenoh key expression used by the endpoint.
- `topic_mode`: `frame`, `primary_source`, or `stream_id`. If omitted or unknown, UNVAC falls back to `frame`.
- `schema_major`: UNMF/Z schema major version. If omitted, UNVAC falls back to `1`.

UNVAC currently validates UNMF/Z endpoint candidates and reports warnings for missing `app_id`, missing `endpoint_id`, missing `base_key_expr`, missing `direction`, and invalid `direction`. Invalid candidates may still be shown in diagnostics so the app author can fix metadata.

## 5. Commands

UNVAC first-slice command publishing is intentionally conservative. The current safe command allow-list is:

```text
focus
open_gui
reload.preview
profile.switch.preview
camera.effect
```

An app should advertise commands it can handle:

```json
{
  "commands": ["focus", "open_gui", "reload.preview"]
}
```

Object forms are also accepted:

```json
{
  "commands": [
    { "command": "focus" },
    { "name": "open_gui" },
    { "id": "reload.preview" }
  ]
}
```

If `commands` is omitted, UNVAC may derive only low-risk commands from capabilities:

- `supervisor_control`, `app_control`, `window_control`, `gui`, or `renderer`: `focus`, `open_gui`
- `supervisor_control`, `app_control`, or `profile_control`: `reload.preview`, `profile.switch.preview`
- `camera_control`: `camera.effect`

If an app is visible through discovery and advertises `commands`, UNVAC publishes only the intersection of advertised commands and the safe allow-list.

### `camera.effect`

UNAvatar Renderer のカメラを一時的に動かし、保持後に命令受付時の状態へ戻す原子的な演出命令。UNVACは対象の `app_id` / `instance_id` を明示し、直近discoveryの `session_id` と `target_session_id` が一致する場合だけ送信する。曖昧一致、stale、未起動、未ready、能力未広告のRendererへは送らない。

```json
{
  "target_session_id": "renderer-main-20260907T120000Z-7a3f",
  "pose": {
    "kind": "orbit",
    "target_xyz": [0.0, 1.35, 0.0],
    "longitude_deg": 180.0,
    "latitude_deg": 3.0,
    "radius": 0.85
  },
  "diagonal_fov_deg": 28.0,
  "enter": { "duration_ms": 450, "easing": "ease_in_out_cubic" },
  "hold_ms": 5000,
  "leave": { "duration_ms": 650, "easing": "ease_in_out_cubic" },
  "conflict_policy": "replace",
  "return_to": "captured_at_start"
}
```

- `target_session_id`だけが必須。ほかは演出開始時状態への部分更新であり、poseまたはFOVを少なくとも一つ変更すればよい。
- `pose.kind = "orbit"`: `target_xyz`、`longitude_deg`、`latitude_deg`、`radius` は個別に省略でき、未指定値は開始時状態を維持する。
- `pose.kind = "cartesian"`: `position_xyz`、`rotation_euler_deg`とその`yaw` / `pitch` / `roll`を個別に省略できる。`rotation_order`の省略値は`yxz`。現行UNAvatarのorbit cameraはroll軸を持たないため、指定する場合の`roll`は`0`だけを受理し、焦点距離には演出開始時のradiusを使う。
- `diagonal_fov_deg`: 対角視野角。省略時は開始時FOVを維持し、水平・垂直FOVへ暗黙変換しない。
- `enter` / `leave`の省略値はそれぞれ`450 ms` / `650 ms`の`ease_in_out_cubic`。`hold_ms`は`5000`、`conflict_policy`は`replace`、`return_to`は`captured_at_start`。
- easing: `none` / `linear` と、sine / quad / cubic / quart / quint の `ease_in_*` / `ease_out_*` / `ease_in_out_*`。`none` は `duration_ms = 0` のみ。
- `replace`: 現在の演出を打ち切り、置換受付時の実カメラ状態を新しい復帰先として捕捉する。`queue`: 受付順に実行し、各演出の開始時に復帰先を捕捉する。
- Rendererは進入・保持・復帰を一つの操作として所有する。UNVACの切断後も、受理済み演出の復帰責任はRenderer側に残る。

Flowgraphでは汎用 `UN App: Command` より、AvatarLinkを厳密解決する `UNAvatar: Camera Effect` ノードを推奨する。

### Command Key

UNVAC publishes command envelopes to:

```text
un/app/{app_id}/{instance_id}/command
```

Envelope:

```json
{
  "schema_version": 1,
  "kind": "app.command",
  "app_id": "un-avatar",
  "instance_id": "renderer-main",
  "command": "open_gui",
  "args": {},
  "request_id": "unvac-1",
  "timeout_ms": 800,
  "issued_at": "2026-05-30T00:00:00Z"
}
```

Apps should ignore unsupported commands and should publish a response when possible.

### Command Response

Primary response key:

```text
un/app/{app_id}/{instance_id}/response/{request_id}
```

Response payload:

```json
{
  "schema_version": 1,
  "kind": "app.command.response",
  "request_id": "unvac-1",
  "ok": true,
  "command": "open_gui",
  "message": "accepted"
}
```

Recommended response fields:

- `request_id`: Required for correlation.
- `ok`: Boolean success/failure.
- `command`: Echo of the command name.
- `message`: Human-readable detail.
- `error`: Machine-readable error code when `ok` is false.

### Event Fallback

An app may publish command result events to:

```text
un/app/{app_id}/{instance_id}/event
```

Event payload must include `request_id` or `requestId` for UNVAC to correlate it with a command:

```json
{
  "schema_version": 1,
  "kind": "app.command.event",
  "request_id": "unvac-1",
  "status": "completed",
  "command": "open_gui"
}
```

UNVAC bounded wait prefers the exact response key. It treats app-wide events as command responses only when `request_id` or `requestId` matches.

## 6. Capabilities

Capabilities are string tokens. Unknown capability tokens are allowed and ignored by UNVAC unless a feature explicitly understands them.

Current common tokens:

```text
renderer
motion_input
motion_output
avatar_load
scene_control
effect_control
profile_control
supervisor_control
app_control
window_control
gui
```

Capabilities are not permissions. They are discovery metadata used for UI, command gating fallback, and future routing.

## 7. UNVAC Current Behavior

UNVAC v2 currently:

- Builds a discovery snapshot from Managed App registry entries and short bounded Zenoh discovery probes.
- Queries `un/app/**/snapshot` and `unaa_z/**/snapshot`.
- Subscribes briefly to `un/app/**/announce` and `unaa_z/**/announce`.
- Preserves discovery provenance as `discovery_source_key` and `discovery_payload_kind`.
- Flattens UNMF/Z endpoint advertisements into `unaa_z_endpoint_candidates`.
- Validates endpoint candidates and exposes `unaa_z_endpoint_candidate_diagnostics`.
- Exposes `unaa_z_health` from `GET /api/v1/control/flowgraph/diagnostics` with app source counts, stale app count, root endpoint count, endpoint candidate count, endpoint warning count, discovery error count, and the active query / announcement key counts.
- Publishes only safe commands through `POST /api/v1/control/un_app_api/command`.
- Records command history at `GET /api/v1/control/un_app_api/commands/history`.

Current UNVAC tests use UNVAC-local fake UNAA/Z apps and fixtures. UNA / UNM real-app interoperability tests should be added after those apps implement this contract.

## 8. Future Reserved Surface

The following command families are planned or reserved, but not first-slice safe commands:

```text
profile.list
profile.load
profile.save
profile.current
avatar.load
scene.load
effect.trigger
motion.route.set
```

Do not assume UNVAC will publish these until capability policy, argument schemas, and UI affordances are fixed.

## 9. Minimal App Checklist

For a U.N. app implementing UNAA/Z:

1. Choose stable `app_id` and `instance_id`.
2. Serve a snapshot queryable at `un/app/{app_id}/{instance_id}/snapshot`.
3. Publish `announce` on startup and when readiness, capabilities, commands, or endpoints change.
4. Include `ready`, `capabilities`, `commands`, and `endpoints` in the snapshot when applicable.
5. Subscribe to `un/app/{app_id}/{instance_id}/command` if the app advertises commands.
6. Reply to `un/app/{app_id}/{instance_id}/response/{request_id}` for command completion or rejection.
7. Keep UNMF/Z motion data on UNMF/Z keys; advertise those keys through UNAA/Z endpoint metadata.

# Flowgraph Node Catalog

VAC v2 Flowgraph の組み込みノード一覧。**本ファイルは自動生成**されるので、手で編集せずノード定義側を更新してからテストで再生成してください。

再生成:

```console
cargo xtask docs bless-node-catalog
```

> 型の表記: `bool` / `bytes` / `datetime` / `exec` / `float` / `int` / `json` / `list<float>` / `list<int>` / `list<json>` / `list<list<json>>` / `list<list<string>>` / `list<string>` / `map<json>` / `motion_frame` / `quantity` / `result<bool>` / `result<bytes>` / `result<datetime>` / `result<int>` / `result<json>` / `result<quantity>` / `result<string>` / `result<table>` / `result<unmf>` / `string` / `table` / `unmf`

## Reading This Catalog

- **Index**: category ごとの通常一覧。feature 名から node 詳細へ移動するための入口です。
- **Metadata Index**: effect / capability / control trigger / snapshot support から node を逆引きするための一覧です。GUI catalog と同じ registry metadata から生成します。
- 各 node section の **Metadata** line は contract summary、effect class、capability、control trigger、state model を compact に示します。
- Port / Property table の **Note** 欄に出る `enum:` / `choices:` は、GUI catalog と同じ選択肢 metadata から生成されます。

## Generated Summary

| Metric | Count |
|---|---:|
| Nodes | 349 |
| Categories | 53 |
| Effectful nodes | 105 |
| Stateful nodes | 24 |
| Capability groups | 15 |
| Control-triggerable nodes | 4 |
| Snapshot-supported nodes | 11 |
| Restore-supported nodes | 11 |

## Index

- **ai**
  - [`flowgraph.ai.analyze`](#flowgraph-ai-analyze) — AI Analyze
  - [`flowgraph.ai.observe`](#flowgraph-ai-observe) — AI Observe
  - [`flowgraph.ingress.ai_avatar_intent`](#flowgraph-ingress-ai-avatar-intent) — AI Avatar Intent
  - [`flowgraph.ingress.ai_avatar_pose`](#flowgraph-ingress-ai-avatar-pose) — AI Avatar Pose
  - [`flowgraph.ingress.ai_response`](#flowgraph-ingress-ai-response) — AI Response
  - [`flowgraph.ingress.ai_speech`](#flowgraph-ingress-ai-speech) — AI Speech
- **audio**
  - [`flowgraph.audio.envelope`](#flowgraph-audio-envelope) — Audio Envelope
  - [`flowgraph.audio.pitch`](#flowgraph-audio-pitch) — Audio Pitch
  - [`flowgraph.audio.play`](#flowgraph-audio-play) — Audio Play
- **avatar**
  - [`flowgraph.avatar.intent_play_unmfz`](#flowgraph-avatar-intent-play-unmfz) — Avatar: Play Intent to UNMF/Z
  - [`flowgraph.avatar.intent_to_unmf`](#flowgraph-avatar-intent-to-unmf) — Avatar: Intent to UNMF
  - [`flowgraph.avatar.pose_asset_play_unmfz`](#flowgraph-avatar-pose-asset-play-unmfz) — Avatar: Play Pose Asset to UNMF/Z
  - [`flowgraph.avatar.pose_catalog_lookup`](#flowgraph-avatar-pose-catalog-lookup) — Avatar: Lookup Pose Catalog
- **bytes**
  - [`flowgraph.bytes.from_base64`](#flowgraph-bytes-from-base64) — Bytes From Base64
  - [`flowgraph.bytes.len`](#flowgraph-bytes-len) — Bytes Length
  - [`flowgraph.bytes.to_base64`](#flowgraph-bytes-to-base64) — Bytes To Base64
- **collection**
  - [`flowgraph.collection.deque`](#flowgraph-collection-deque) — Deque
  - [`flowgraph.collection.priority_queue`](#flowgraph-collection-priority-queue) — Priority Queue
  - [`flowgraph.collection.reservoir_sample`](#flowgraph-collection-reservoir-sample) — Reservoir Sample
  - [`flowgraph.collection.ring_buffer`](#flowgraph-collection-ring-buffer) — Ring Buffer
- **command**
  - [`flowgraph.command.authorize`](#flowgraph-command-authorize) — Command Authorize
  - [`flowgraph.command.match`](#flowgraph-command-match) — Command Match
  - [`flowgraph.command.set`](#flowgraph-command-set) — Command Set
- **compare**
  - [`flowgraph.compare.eq`](#flowgraph-compare-eq) — Equal
  - [`flowgraph.compare.float_gt`](#flowgraph-compare-float-gt) — Float >
  - [`flowgraph.compare.float_lt`](#flowgraph-compare-float-lt) — Float <
  - [`flowgraph.compare.int_ge`](#flowgraph-compare-int-ge) — Int >=
  - [`flowgraph.compare.int_gt`](#flowgraph-compare-int-gt) — Int >
  - [`flowgraph.compare.int_le`](#flowgraph-compare-int-le) — Int <=
  - [`flowgraph.compare.int_lt`](#flowgraph-compare-int-lt) — Int <
  - [`flowgraph.compare.neq`](#flowgraph-compare-neq) — Not Equal
- **constants**
  - [`flowgraph.constants.math`](#flowgraph-constants-math) — Math Constants
  - [`flowgraph.constants.physics`](#flowgraph-constants-physics) — Physics Constants
- **convert**
  - [`flowgraph.convert.bool_to_string`](#flowgraph-convert-bool-to-string) — Bool → String
  - [`flowgraph.convert.float_to_int`](#flowgraph-convert-float-to-int) — Float → Int
  - [`flowgraph.convert.float_to_string`](#flowgraph-convert-float-to-string) — Float → String
  - [`flowgraph.convert.int_to_float`](#flowgraph-convert-int-to-float) — Int → Float
  - [`flowgraph.convert.int_to_string`](#flowgraph-convert-int-to-string) — Int → String
  - [`flowgraph.convert.json_to_msgpack`](#flowgraph-convert-json-to-msgpack) — JSON → MessagePack
  - [`flowgraph.convert.json_to_toml`](#flowgraph-convert-json-to-toml) — JSON → TOML
  - [`flowgraph.convert.msgpack_to_json`](#flowgraph-convert-msgpack-to-json) — MessagePack → JSON
  - [`flowgraph.convert.ron_to_toml`](#flowgraph-convert-ron-to-toml) — RON → TOML
  - [`flowgraph.convert.string_to_float`](#flowgraph-convert-string-to-float) — String → Float
  - [`flowgraph.convert.string_to_int`](#flowgraph-convert-string-to-int) — String → Int
  - [`flowgraph.convert.toml_to_json`](#flowgraph-convert-toml-to-json) — TOML → JSON
  - [`flowgraph.convert.toml_to_ron`](#flowgraph-convert-toml-to-ron) — TOML → RON
- **datetime**
  - [`flowgraph.datetime.add_duration`](#flowgraph-datetime-add-duration) — DateTime + Duration
  - [`flowgraph.datetime.diff`](#flowgraph-datetime-diff) — DateTime - DateTime
  - [`flowgraph.datetime.epoch_ms`](#flowgraph-datetime-epoch-ms) — DateTime -> Epoch ms
  - [`flowgraph.datetime.format`](#flowgraph-datetime-format) — DateTime Format
  - [`flowgraph.datetime.from_epoch_ms`](#flowgraph-datetime-from-epoch-ms) — Epoch ms -> DateTime
  - [`flowgraph.datetime.now`](#flowgraph-datetime-now) — DateTime Now
  - [`flowgraph.datetime.parse`](#flowgraph-datetime-parse) — DateTime Parse
  - [`flowgraph.datetime.sub_duration`](#flowgraph-datetime-sub-duration) — DateTime - Duration
  - [`flowgraph.datetime.try_parse`](#flowgraph-datetime-try-parse) — DateTime Try Parse
- **db**
  - [`flowgraph.db.sqlite.query`](#flowgraph-db-sqlite-query) — SQLite Query
- **dictionary**
  - [`flowgraph.dictionary.from_entries`](#flowgraph-dictionary-from-entries) — Dictionary From Entries
  - [`flowgraph.dictionary.get`](#flowgraph-dictionary-get) — Dictionary Get
  - [`flowgraph.dictionary.has`](#flowgraph-dictionary-has) — Dictionary Has
  - [`flowgraph.dictionary.keys`](#flowgraph-dictionary-keys) — Dictionary Keys
  - [`flowgraph.dictionary.remove`](#flowgraph-dictionary-remove) — Dictionary Remove
  - [`flowgraph.dictionary.set`](#flowgraph-dictionary-set) — Dictionary Set
- **easing**
  - [`flowgraph.easing.apply`](#flowgraph-easing-apply) — Easing apply
- **file**
  - [`flowgraph.file.read_bytes`](#flowgraph-file-read-bytes) — File Read Bytes
  - [`flowgraph.file.read_json`](#flowgraph-file-read-json) — File Read JSON
  - [`flowgraph.file.read_ron`](#flowgraph-file-read-ron) — File Read RON
  - [`flowgraph.file.read_string`](#flowgraph-file-read-string) — File Read String
  - [`flowgraph.file.read_toml`](#flowgraph-file-read-toml) — File Read TOML
  - [`flowgraph.file.write_bytes`](#flowgraph-file-write-bytes) — File Write Bytes
  - [`flowgraph.file.write_json`](#flowgraph-file-write-json) — File Write JSON
  - [`flowgraph.file.write_ron`](#flowgraph-file-write-ron) — File Write RON
  - [`flowgraph.file.write_string`](#flowgraph-file-write-string) — File Write String
  - [`flowgraph.file.write_toml`](#flowgraph-file-write-toml) — File Write TOML
- **flow**
  - [`flowgraph.flow.branch`](#flowgraph-flow-branch) — Branch
  - [`flowgraph.flow.gate`](#flowgraph-flow-gate) — Gate
  - [`flowgraph.flow.merge_bool`](#flowgraph-flow-merge-bool) — Merge Events (bool)
  - [`flowgraph.flow.merge_bytes`](#flowgraph-flow-merge-bytes) — Merge Events (bytes)
  - [`flowgraph.flow.merge_datetime`](#flowgraph-flow-merge-datetime) — Merge Events (datetime)
  - [`flowgraph.flow.merge_float`](#flowgraph-flow-merge-float) — Merge Events (float)
  - [`flowgraph.flow.merge_int`](#flowgraph-flow-merge-int) — Merge Events (int)
  - [`flowgraph.flow.merge_json`](#flowgraph-flow-merge-json) — Merge Events (json)
  - [`flowgraph.flow.merge_motion_frame`](#flowgraph-flow-merge-motion-frame) — Merge Events (motion_frame)
  - [`flowgraph.flow.merge_quantity`](#flowgraph-flow-merge-quantity) — Merge Events (quantity)
  - [`flowgraph.flow.merge_string`](#flowgraph-flow-merge-string) — Merge Events (string)
  - [`flowgraph.flow.merge_table`](#flowgraph-flow-merge-table) — Merge Events (table)
  - [`flowgraph.flow.merge_unmf`](#flowgraph-flow-merge-unmf) — Merge Events (unmf)
- **glossary**
  - [`flowgraph.glossary.forget`](#flowgraph-glossary-forget) — Glossary Forget
  - [`flowgraph.glossary.learn`](#flowgraph-glossary-learn) — Glossary Learn
  - [`flowgraph.glossary.match`](#flowgraph-glossary-match) — Glossary Match
  - [`flowgraph.glossary.replace`](#flowgraph-glossary-replace) — Glossary Replace
- **http**
  - [`flowgraph.http.request`](#flowgraph-http-request) — HTTP Request
- **ingress**
  - [`flowgraph.ingress.control`](#flowgraph-ingress-control) — Control Input
  - [`flowgraph.ingress.file_watch`](#flowgraph-ingress-file-watch) — File Watch Ingress
  - [`flowgraph.ingress.osc_udp`](#flowgraph-ingress-osc-udp) — OSC UDP Ingress
  - [`flowgraph.ingress.twitch`](#flowgraph-ingress-twitch) — Twitch Ingress
  - [`flowgraph.ingress.twitch_eventsub`](#flowgraph-ingress-twitch-eventsub) — Twitch EventSub Ingress
  - [`flowgraph.ingress.unmfz`](#flowgraph-ingress-unmfz) — UNMF/Z Ingress
  - [`flowgraph.ingress.vmc_udp`](#flowgraph-ingress-vmc-udp) — VMC UDP Ingress
  - [`flowgraph.ingress.voice`](#flowgraph-ingress-voice) — Voice Ingress
  - [`flowgraph.ingress.web_input`](#flowgraph-ingress-web-input) — Web Input Ingress
- **json**
  - [`flowgraph.json.get`](#flowgraph-json-get) — JSON Get
  - [`flowgraph.json.parse`](#flowgraph-json-parse) — JSON Parse
  - [`flowgraph.json.set_string`](#flowgraph-json-set-string) — JSON Set String
  - [`flowgraph.json.stringify`](#flowgraph-json-stringify) — JSON Stringify
  - [`flowgraph.json.try_parse`](#flowgraph-json-try-parse) — JSON Try Parse
- **list**
  - [`flowgraph.list.aggregate_numbers`](#flowgraph-list-aggregate-numbers) — List Aggregate Numbers
  - [`flowgraph.list.binary_search_json`](#flowgraph-list-binary-search-json) — List Binary Search JSON
  - [`flowgraph.list.filter_json_eq`](#flowgraph-list-filter-json-eq) — List Filter JSON Eq
  - [`flowgraph.list.get`](#flowgraph-list-get) — List Get
  - [`flowgraph.list.is_empty`](#flowgraph-list-is-empty) — List Is Empty
  - [`flowgraph.list.len`](#flowgraph-list-len) — List Length
  - [`flowgraph.list.map_json_get`](#flowgraph-list-map-json-get) — List Map JSON Get
  - [`flowgraph.list.range`](#flowgraph-list-range) — List Range
  - [`flowgraph.list.slice`](#flowgraph-list-slice) — List Slice
  - [`flowgraph.list.sort_json`](#flowgraph-list-sort-json) — List Sort JSON
  - [`flowgraph.list.string_get`](#flowgraph-list-string-get) — String List Get
  - [`flowgraph.list.top_k_json`](#flowgraph-list-top-k-json) — List Top-K JSON
  - [`flowgraph.list.window`](#flowgraph-list-window) — List Window
- **literal**
  - [`flowgraph.literal.bool`](#flowgraph-literal-bool) — Bool Literal
  - [`flowgraph.literal.float`](#flowgraph-literal-float) — Float Literal
  - [`flowgraph.literal.int`](#flowgraph-literal-int) — Int Literal
  - [`flowgraph.literal.json`](#flowgraph-literal-json) — JSON Literal
  - [`flowgraph.literal.string`](#flowgraph-literal-string) — String Literal
  - [`flowgraph.literal.strings`](#flowgraph-literal-strings) — String List Literal
- **logic**
  - [`flowgraph.logic.and`](#flowgraph-logic-and) — And
  - [`flowgraph.logic.not`](#flowgraph-logic-not) — Not
  - [`flowgraph.logic.or`](#flowgraph-logic-or) — Or
  - [`flowgraph.logic.xor`](#flowgraph-logic-xor) — Xor
- **map**
  - [`flowgraph.map.get`](#flowgraph-map-get) — Map Get
  - [`flowgraph.map.has`](#flowgraph-map-has) — Map Has
  - [`flowgraph.map.keys`](#flowgraph-map-keys) — Map Keys
- **math**
  - [`flowgraph.math.abs_float`](#flowgraph-math-abs-float) — Float abs
  - [`flowgraph.math.abs_int`](#flowgraph-math-abs-int) — Int abs
  - [`flowgraph.math.acos`](#flowgraph-math-acos) — Float acos
  - [`flowgraph.math.acosh`](#flowgraph-math-acosh) — Float acosh
  - [`flowgraph.math.asin`](#flowgraph-math-asin) — Float asin
  - [`flowgraph.math.asinh`](#flowgraph-math-asinh) — Float asinh
  - [`flowgraph.math.atan`](#flowgraph-math-atan) — Float atan
  - [`flowgraph.math.atan2`](#flowgraph-math-atan2) — Float atan2
  - [`flowgraph.math.atanh`](#flowgraph-math-atanh) — Float atanh
  - [`flowgraph.math.ceil`](#flowgraph-math-ceil) — Float ceil
  - [`flowgraph.math.clamp_float`](#flowgraph-math-clamp-float) — Float clamp
  - [`flowgraph.math.clamp_int`](#flowgraph-math-clamp-int) — Int clamp
  - [`flowgraph.math.cos`](#flowgraph-math-cos) — Float cos
  - [`flowgraph.math.cosh`](#flowgraph-math-cosh) — Float cosh
  - [`flowgraph.math.deg_to_rad`](#flowgraph-math-deg-to-rad) — Float deg → rad
  - [`flowgraph.math.exp`](#flowgraph-math-exp) — Float exp
  - [`flowgraph.math.float_add`](#flowgraph-math-float-add) — Float +
  - [`flowgraph.math.float_div`](#flowgraph-math-float-div) — Float /
  - [`flowgraph.math.float_mul`](#flowgraph-math-float-mul) — Float *
  - [`flowgraph.math.float_sub`](#flowgraph-math-float-sub) — Float -
  - [`flowgraph.math.floor`](#flowgraph-math-floor) — Float floor
  - [`flowgraph.math.int_add`](#flowgraph-math-int-add) — Int +
  - [`flowgraph.math.int_div`](#flowgraph-math-int-div) — Int /
  - [`flowgraph.math.int_max`](#flowgraph-math-int-max) — Int max
  - [`flowgraph.math.int_min`](#flowgraph-math-int-min) — Int min
  - [`flowgraph.math.int_mod`](#flowgraph-math-int-mod) — Int %
  - [`flowgraph.math.int_mul`](#flowgraph-math-int-mul) — Int *
  - [`flowgraph.math.int_sub`](#flowgraph-math-int-sub) — Int -
  - [`flowgraph.math.inverse_lerp`](#flowgraph-math-inverse-lerp) — Float inverse_lerp
  - [`flowgraph.math.lerp`](#flowgraph-math-lerp) — Float lerp
  - [`flowgraph.math.ln`](#flowgraph-math-ln) — Float ln
  - [`flowgraph.math.log10`](#flowgraph-math-log10) — Float log10
  - [`flowgraph.math.log2`](#flowgraph-math-log2) — Float log2
  - [`flowgraph.math.max_float`](#flowgraph-math-max-float) — Float max
  - [`flowgraph.math.min_float`](#flowgraph-math-min-float) — Float min
  - [`flowgraph.math.normalize_angle_deg_0_360`](#flowgraph-math-normalize-angle-deg-0-360) — Normalize angle [0, 360) deg
  - [`flowgraph.math.normalize_angle_deg_signed`](#flowgraph-math-normalize-angle-deg-signed) — Normalize angle [-180, 180) deg
  - [`flowgraph.math.normalize_angle_rad_0_2pi`](#flowgraph-math-normalize-angle-rad-0-2pi) — Normalize angle [0, 2π) rad
  - [`flowgraph.math.normalize_angle_rad_signed`](#flowgraph-math-normalize-angle-rad-signed) — Normalize angle [-π, π) rad
  - [`flowgraph.math.pow`](#flowgraph-math-pow) — Float pow
  - [`flowgraph.math.rad_to_deg`](#flowgraph-math-rad-to-deg) — Float rad → deg
  - [`flowgraph.math.remap`](#flowgraph-math-remap) — Float remap
  - [`flowgraph.math.round`](#flowgraph-math-round) — Float round
  - [`flowgraph.math.sign_float`](#flowgraph-math-sign-float) — Float sign
  - [`flowgraph.math.sign_int`](#flowgraph-math-sign-int) — Int sign
  - [`flowgraph.math.sin`](#flowgraph-math-sin) — Float sin
  - [`flowgraph.math.sinh`](#flowgraph-math-sinh) — Float sinh
  - [`flowgraph.math.smoothstep`](#flowgraph-math-smoothstep) — Float smoothstep
  - [`flowgraph.math.sqrt`](#flowgraph-math-sqrt) — Float sqrt
  - [`flowgraph.math.tan`](#flowgraph-math-tan) — Float tan
  - [`flowgraph.math.tanh`](#flowgraph-math-tanh) — Float tanh
- **mode**
  - [`flowgraph.mode.equals`](#flowgraph-mode-equals) — Mode Equals
  - [`flowgraph.mode.get`](#flowgraph-mode-get) — Mode Get
  - [`flowgraph.mode.transit`](#flowgraph-mode-transit) — Mode Transit
- **motion**
  - [`flowgraph.motion.filter`](#flowgraph-motion-filter) — Motion: Filter OSC Messages
  - [`flowgraph.motion.map`](#flowgraph-motion-map) — Motion: Map Numeric Args
  - [`flowgraph.motion.vmc_parse`](#flowgraph-motion-vmc-parse) — Motion: VMC OSC Parse
- **noise**
  - [`flowgraph.noise.perlin_1d`](#flowgraph-noise-perlin-1d) — Perlin 1D
  - [`flowgraph.noise.perlin_2d`](#flowgraph-noise-perlin-2d) — Perlin 2D
- **notify**
  - [`flowgraph.notify.desktop`](#flowgraph-notify-desktop) — Notify: Desktop
- **obs**
  - [`flowgraph.obs.get_current_program_scene`](#flowgraph-obs-get-current-program-scene) — OBS: Get Current Program Scene
  - [`flowgraph.obs.get_source_screenshot`](#flowgraph-obs-get-source-screenshot) — OBS: Get Explicit Source Screenshot
  - [`flowgraph.obs.request`](#flowgraph-obs-request) — OBS: Request
  - [`flowgraph.obs.set_current_program_scene`](#flowgraph-obs-set-current-program-scene) — OBS: Set Current Program Scene
  - [`flowgraph.obs.set_scene_item_enabled`](#flowgraph-obs-set-scene-item-enabled) — OBS: Set Scene Item Enabled
  - [`flowgraph.obs.start_record`](#flowgraph-obs-start-record) — OBS: Start Record
  - [`flowgraph.obs.start_stream`](#flowgraph-obs-start-stream) — OBS: Start Stream
  - [`flowgraph.obs.stop_record`](#flowgraph-obs-stop-record) — OBS: Stop Record
  - [`flowgraph.obs.stop_stream`](#flowgraph-obs-stop-stream) — OBS: Stop Stream
  - [`flowgraph.obs.trigger_studio_mode_transition`](#flowgraph-obs-trigger-studio-mode-transition) — OBS: Trigger Studio Mode Transition
- **ocr**
  - [`flowgraph.ocr.recognize`](#flowgraph-ocr-recognize) — OCR Recognize
- **osc**
  - [`flowgraph.osc.send`](#flowgraph-osc-send) — OSC: UDP Send
- **output**
  - [`flowgraph.bos.output`](#flowgraph-bos-output) — BOS Output
- **physics**
  - [`flowgraph.physics.damper`](#flowgraph-physics-damper) — Physics Damper
  - [`flowgraph.physics.gravity`](#flowgraph-physics-gravity) — Physics Gravity
  - [`flowgraph.physics.integrator`](#flowgraph-physics-integrator) — Physics Integrator
  - [`flowgraph.physics.spring`](#flowgraph-physics-spring) — Physics Spring
- **process**
  - [`flowgraph.process.kill`](#flowgraph-process-kill) — Process: Kill
  - [`flowgraph.process.running`](#flowgraph-process-running) — Process: Running
  - [`flowgraph.process.spawn`](#flowgraph-process-spawn) — Process: Spawn
  - [`flowgraph.process.wait`](#flowgraph-process-wait) — Process: Wait
- **random**
  - [`flowgraph.dist.categorical`](#flowgraph-dist-categorical) — Distribution categorical
  - [`flowgraph.dist.exponential`](#flowgraph-dist-exponential) — Distribution exponential
  - [`flowgraph.dist.normal`](#flowgraph-dist-normal) — Distribution normal
  - [`flowgraph.dist.poisson`](#flowgraph-dist-poisson) — Distribution poisson
  - [`flowgraph.prng.sequence`](#flowgraph-prng-sequence) — PRNG Sequence
  - [`flowgraph.prng.uniform_float`](#flowgraph-prng-uniform-float) — PRNG uniform (float)
  - [`flowgraph.prng.uniform_int`](#flowgraph-prng-uniform-int) — PRNG uniform (int)
  - [`flowgraph.random.normal`](#flowgraph-random-normal) — Random normal
  - [`flowgraph.random.uniform_float`](#flowgraph-random-uniform-float) — Random uniform (float)
  - [`flowgraph.random.uniform_int`](#flowgraph-random-uniform-int) — Random uniform (int)
- **regex**
  - [`flowgraph.regex.replace`](#flowgraph-regex-replace) — Regex Replace
- **screenshot**
  - [`flowgraph.screenshot.capture`](#flowgraph-screenshot-capture) — Screenshot Capture
- **state**
  - [`flowgraph.state.accumulator`](#flowgraph-state-accumulator) — Accumulator
  - [`flowgraph.state.bool`](#flowgraph-state-bool) — Bool State
  - [`flowgraph.state.int_counter`](#flowgraph-state-int-counter) — Int Counter
  - [`flowgraph.state.latch`](#flowgraph-state-latch) — Latch
- **string**
  - [`flowgraph.string.concat`](#flowgraph-string-concat) — String Concat
  - [`flowgraph.string.contains`](#flowgraph-string-contains) — String Contains
  - [`flowgraph.string.english_to_katakana`](#flowgraph-string-english-to-katakana) — English to Katakana (ONNX)
  - [`flowgraph.string.join`](#flowgraph-string-join) — String Join
  - [`flowgraph.string.len`](#flowgraph-string-len) — String Length
  - [`flowgraph.string.replace`](#flowgraph-string-replace) — String Replace
  - [`flowgraph.string.split`](#flowgraph-string-split) — String Split
- **system**
  - [`flowgraph.system.cpu_usage`](#flowgraph-system-cpu-usage) — System: CPU Usage
  - [`flowgraph.system.load_avg`](#flowgraph-system-load-avg) — System: Load Average
  - [`flowgraph.system.memory`](#flowgraph-system-memory) — System: Memory
  - [`flowgraph.system.process_list`](#flowgraph-system-process-list) — System: Process List
- **table**
  - [`flowgraph.table.concat`](#flowgraph-table-concat) — Table Concat
  - [`flowgraph.table.draw`](#flowgraph-table-draw) — Table Draw
  - [`flowgraph.table.from_json`](#flowgraph-table-from-json) — Table From JSON
  - [`flowgraph.table.load_google_sheet_auth`](#flowgraph-table-load-google-sheet-auth) — Table Load Google Sheet Auth
  - [`flowgraph.table.load_google_sheet_public`](#flowgraph-table-load-google-sheet-public) — Table Load Google Sheet Public
  - [`flowgraph.table.load_tsv`](#flowgraph-table-load-tsv) — Table Load TSV
  - [`flowgraph.table.load_xlsx`](#flowgraph-table-load-xlsx) — Table Load XLSX
  - [`flowgraph.table.to_json`](#flowgraph-table-to-json) — Table To JSON
  - [`flowgraph.table.write_tsv`](#flowgraph-table-write-tsv) — Table Write TSV
- **translate**
  - [`flowgraph.translate.gas`](#flowgraph-translate-gas) — Translate (GAS)
  - [`flowgraph.translate.libre`](#flowgraph-translate-libre) — Translate (LibreTranslate)
- **tts**
  - [`flowgraph.tts.speak`](#flowgraph-tts-speak) — TTS: Speak
- **twitch**
  - [`flowgraph.ingress.twitch_stream_state`](#flowgraph-ingress-twitch-stream-state) — Twitch Stream State
  - [`flowgraph.twitch.ad_run`](#flowgraph-twitch-ad-run) — Twitch: Run Ad
  - [`flowgraph.twitch.ban`](#flowgraph-twitch-ban) — Twitch: Ban User
  - [`flowgraph.twitch.channel_info_update`](#flowgraph-twitch-channel-info-update) — Twitch: Update Channel Info
  - [`flowgraph.twitch.chat_clear`](#flowgraph-twitch-chat-clear) — Twitch: Clear Chat
  - [`flowgraph.twitch.chat_send`](#flowgraph-twitch-chat-send) — Twitch: Chat Send
  - [`flowgraph.twitch.chat_settings_update`](#flowgraph-twitch-chat-settings-update) — Twitch: Update Chat Settings
  - [`flowgraph.twitch.clip_command`](#flowgraph-twitch-clip-command) — Twitch: Moderator Clip Command
  - [`flowgraph.twitch.clip_create`](#flowgraph-twitch-clip-create) — Twitch: Create Clip
  - [`flowgraph.twitch.event_notice`](#flowgraph-twitch-event-notice) — Twitch: Event Notice
  - [`flowgraph.twitch.get_token`](#flowgraph-twitch-get-token) — Twitch: Get Token
  - [`flowgraph.twitch.goals_get`](#flowgraph-twitch-goals-get) — Twitch: Get Goals
  - [`flowgraph.twitch.poll_create`](#flowgraph-twitch-poll-create) — Twitch: Create Poll
  - [`flowgraph.twitch.poll_end`](#flowgraph-twitch-poll-end) — Twitch: End Poll
  - [`flowgraph.twitch.prediction_create`](#flowgraph-twitch-prediction-create) — Twitch: Create Prediction
  - [`flowgraph.twitch.prediction_end`](#flowgraph-twitch-prediction-end) — Twitch: End Prediction
  - [`flowgraph.twitch.raid_cancel`](#flowgraph-twitch-raid-cancel) — Twitch: Cancel Raid
  - [`flowgraph.twitch.raid_start`](#flowgraph-twitch-raid-start) — Twitch: Start Raid
  - [`flowgraph.twitch.random_clip`](#flowgraph-twitch-random-clip) — Twitch: Random Clip
  - [`flowgraph.twitch.shield_mode_update`](#flowgraph-twitch-shield-mode-update) — Twitch: Update Shield Mode
  - [`flowgraph.twitch.stream_marker_create`](#flowgraph-twitch-stream-marker-create) — Twitch: Create Stream Marker
  - [`flowgraph.twitch.timeout`](#flowgraph-twitch-timeout) — Twitch: Timeout User
  - [`flowgraph.twitch.user_id_by_login`](#flowgraph-twitch-user-id-by-login) — Twitch: User ID by Login
  - [`flowgraph.twitch.validate_token`](#flowgraph-twitch-validate-token) — Twitch: Validate Token
- **un_app**
  - [`flowgraph.un.app.command`](#flowgraph-un-app-command) — UN App: Command
  - [`flowgraph.un.app.event`](#flowgraph-un-app-event) — UN App: Event
  - [`flowgraph.un.app.status`](#flowgraph-un-app-status) — UN App: Status
  - [`flowgraph.unavatar.camera_effect`](#flowgraph-unavatar-camera-effect) — UNAvatar: Camera Effect
- **unit**
  - [`flowgraph.unit.assign`](#flowgraph-unit-assign) — Unit Assign
  - [`flowgraph.unit.convert`](#flowgraph-unit-convert) — Unit Convert
  - [`flowgraph.unit.get_dim_string`](#flowgraph-unit-get-dim-string) — Dimension -> String
  - [`flowgraph.unit.get_unit_string`](#flowgraph-unit-get-unit-string) — Unit -> String
  - [`flowgraph.unit.same_dimension`](#flowgraph-unit-same-dimension) — Same Dimension?
  - [`flowgraph.unit.strip`](#flowgraph-unit-strip) — Unit Strip
  - [`flowgraph.unit.to_json`](#flowgraph-unit-to-json) — Unit -> JSON
  - [`flowgraph.unit.try_parse`](#flowgraph-unit-try-parse) — Unit Try Parse
- **unmf**
  - [`flowgraph.unmf.extract_blendshape`](#flowgraph-unmf-extract-blendshape) — UNMF: Extract BlendShape
  - [`flowgraph.unmf.extract_bone`](#flowgraph-unmf-extract-bone) — UNMF: Extract Bone
  - [`flowgraph.unmf.filter`](#flowgraph-unmf-filter) — UNMF: Filter
  - [`flowgraph.unmf.from_vmc`](#flowgraph-unmf-from-vmc) — UNMF: From VMC
  - [`flowgraph.unmf.map`](#flowgraph-unmf-map) — UNMF: Map
  - [`flowgraph.unmf.to_vmc`](#flowgraph-unmf-to-vmc) — UNMF: To VMC
  - [`flowgraph.unmfz.decode`](#flowgraph-unmfz-decode) — UNMF/Z: Decode
  - [`flowgraph.unmfz.encode`](#flowgraph-unmfz-encode) — UNMF/Z: Encode
  - [`flowgraph.unmfz.publish`](#flowgraph-unmfz-publish) — UNMF/Z: Publish
  - [`flowgraph.unmfz.resolve_endpoint`](#flowgraph-unmfz-resolve-endpoint) — UNMF/Z: Resolve Endpoint
  - [`flowgraph.unmfz.subscribe`](#flowgraph-unmfz-subscribe) — UNMF/Z: Subscribe Once
- **util**
  - [`flowgraph.util.debounce`](#flowgraph-util-debounce) — Debounce
  - [`flowgraph.util.delay`](#flowgraph-util-delay) — Delay
  - [`flowgraph.util.edge_detect`](#flowgraph-util-edge-detect) — Edge detect
  - [`flowgraph.util.format`](#flowgraph-util-format) — Format Quantity
  - [`flowgraph.util.log`](#flowgraph-util-log) — Log
  - [`flowgraph.util.prev_value`](#flowgraph-util-prev-value) — Previous value
  - [`flowgraph.util.rate_limit`](#flowgraph-util-rate-limit) — Rate Limit
  - [`flowgraph.util.sample_hold`](#flowgraph-util-sample-hold) — Sample & hold
  - [`flowgraph.util.throttle`](#flowgraph-util-throttle) — Throttle
  - [`flowgraph.util.timer_interval`](#flowgraph-util-timer-interval) — Timer Interval
- **vec**
  - [`flowgraph.vec2.add`](#flowgraph-vec2-add) — Vec2 add
  - [`flowgraph.vec2.distance`](#flowgraph-vec2-distance) — Vec2 distance
  - [`flowgraph.vec2.dot`](#flowgraph-vec2-dot) — Vec2 dot
  - [`flowgraph.vec2.length`](#flowgraph-vec2-length) — Vec2 length
  - [`flowgraph.vec2.lerp`](#flowgraph-vec2-lerp) — Vec2 lerp
  - [`flowgraph.vec2.make`](#flowgraph-vec2-make) — Vec2 make
  - [`flowgraph.vec2.normalize`](#flowgraph-vec2-normalize) — Vec2 normalize
  - [`flowgraph.vec2.scale`](#flowgraph-vec2-scale) — Vec2 scale
  - [`flowgraph.vec2.sub`](#flowgraph-vec2-sub) — Vec2 sub
  - [`flowgraph.vec2.unpack`](#flowgraph-vec2-unpack) — Vec2 unpack
  - [`flowgraph.vec3.add`](#flowgraph-vec3-add) — Vec3 add
  - [`flowgraph.vec3.distance`](#flowgraph-vec3-distance) — Vec3 distance
  - [`flowgraph.vec3.dot`](#flowgraph-vec3-dot) — Vec3 dot
  - [`flowgraph.vec3.length`](#flowgraph-vec3-length) — Vec3 length
  - [`flowgraph.vec3.lerp`](#flowgraph-vec3-lerp) — Vec3 lerp
  - [`flowgraph.vec3.make`](#flowgraph-vec3-make) — Vec3 make
  - [`flowgraph.vec3.normalize`](#flowgraph-vec3-normalize) — Vec3 normalize
  - [`flowgraph.vec3.scale`](#flowgraph-vec3-scale) — Vec3 scale
  - [`flowgraph.vec3.sub`](#flowgraph-vec3-sub) — Vec3 sub
  - [`flowgraph.vec3.unpack`](#flowgraph-vec3-unpack) — Vec3 unpack
- **vmc**
  - [`flowgraph.vmc.extract_blendshape`](#flowgraph-vmc-extract-blendshape) — VMC: Extract BlendShape
  - [`flowgraph.vmc.extract_bone_pos`](#flowgraph-vmc-extract-bone-pos) — VMC: Extract Bone Pos
  - [`flowgraph.vmc.extract_root_pos`](#flowgraph-vmc-extract-root-pos) — VMC: Extract Root Pos
  - [`flowgraph.vmc.send_bone_pos`](#flowgraph-vmc-send-bone-pos) — VMC: Send Bone Pos
  - [`flowgraph.vmc.send_root_pos`](#flowgraph-vmc-send-root-pos) — VMC: Send Root Pos
- **vrchat**
  - [`flowgraph.vrchat.avatar_parameter_bool`](#flowgraph-vrchat-avatar-parameter-bool) — VRChat: Avatar Parameter (Bool)
  - [`flowgraph.vrchat.avatar_parameter_float`](#flowgraph-vrchat-avatar-parameter-float) — VRChat: Avatar Parameter (Float)
  - [`flowgraph.vrchat.avatar_parameter_int`](#flowgraph-vrchat-avatar-parameter-int) — VRChat: Avatar Parameter (Int)
  - [`flowgraph.vrchat.chatbox_input`](#flowgraph-vrchat-chatbox-input) — VRChat: Chatbox Input
  - [`flowgraph.vrchat.chatbox_typing`](#flowgraph-vrchat-chatbox-typing) — VRChat: Chatbox Typing
- **web**
  - [`flowgraph.web.rss_fetch`](#flowgraph-web-rss-fetch) — Web RSS Fetch
- **window**
  - [`flowgraph.window.close`](#flowgraph-window-close) — Window: Close
  - [`flowgraph.window.enum`](#flowgraph-window-enum) — Window: Enum
  - [`flowgraph.window.foreground`](#flowgraph-window-foreground) — Window: Foreground
  - [`flowgraph.window.maximize`](#flowgraph-window-maximize) — Window: Maximize
  - [`flowgraph.window.minimize`](#flowgraph-window-minimize) — Window: Minimize
  - [`flowgraph.window.move`](#flowgraph-window-move) — Window: Move
  - [`flowgraph.window.pseudo_fullscreen`](#flowgraph-window-pseudo-fullscreen) — Window: Pseudo Fullscreen
  - [`flowgraph.window.pseudo_fullscreen_exit`](#flowgraph-window-pseudo-fullscreen-exit) — Window: Pseudo Fullscreen Exit
  - [`flowgraph.window.resize`](#flowgraph-window-resize) — Window: Resize
  - [`flowgraph.window.restore`](#flowgraph-window-restore) — Window: Restore

## Metadata Index

### Effect Classes

- **effectful** (105): [`flowgraph.ai.observe`](#flowgraph-ai-observe), [`flowgraph.audio.play`](#flowgraph-audio-play), [`flowgraph.avatar.intent_play_unmfz`](#flowgraph-avatar-intent-play-unmfz), [`flowgraph.avatar.pose_asset_play_unmfz`](#flowgraph-avatar-pose-asset-play-unmfz), [`flowgraph.command.set`](#flowgraph-command-set), [`flowgraph.db.sqlite.query`](#flowgraph-db-sqlite-query), [`flowgraph.file.read_bytes`](#flowgraph-file-read-bytes), [`flowgraph.file.read_json`](#flowgraph-file-read-json), [`flowgraph.file.read_ron`](#flowgraph-file-read-ron), [`flowgraph.file.read_string`](#flowgraph-file-read-string), [`flowgraph.file.read_toml`](#flowgraph-file-read-toml), [`flowgraph.file.write_bytes`](#flowgraph-file-write-bytes), [`flowgraph.file.write_json`](#flowgraph-file-write-json), [`flowgraph.file.write_ron`](#flowgraph-file-write-ron), [`flowgraph.file.write_string`](#flowgraph-file-write-string), [`flowgraph.file.write_toml`](#flowgraph-file-write-toml), [`flowgraph.flow.merge_bool`](#flowgraph-flow-merge-bool), [`flowgraph.flow.merge_bytes`](#flowgraph-flow-merge-bytes), [`flowgraph.flow.merge_datetime`](#flowgraph-flow-merge-datetime), [`flowgraph.flow.merge_float`](#flowgraph-flow-merge-float), [`flowgraph.flow.merge_int`](#flowgraph-flow-merge-int), [`flowgraph.flow.merge_json`](#flowgraph-flow-merge-json), [`flowgraph.flow.merge_motion_frame`](#flowgraph-flow-merge-motion-frame), [`flowgraph.flow.merge_quantity`](#flowgraph-flow-merge-quantity), [`flowgraph.flow.merge_string`](#flowgraph-flow-merge-string), [`flowgraph.flow.merge_table`](#flowgraph-flow-merge-table), [`flowgraph.flow.merge_unmf`](#flowgraph-flow-merge-unmf), [`flowgraph.http.request`](#flowgraph-http-request), [`flowgraph.mode.transit`](#flowgraph-mode-transit), [`flowgraph.notify.desktop`](#flowgraph-notify-desktop), [`flowgraph.obs.get_current_program_scene`](#flowgraph-obs-get-current-program-scene), [`flowgraph.obs.get_source_screenshot`](#flowgraph-obs-get-source-screenshot), [`flowgraph.obs.request`](#flowgraph-obs-request), [`flowgraph.obs.set_current_program_scene`](#flowgraph-obs-set-current-program-scene), [`flowgraph.obs.set_scene_item_enabled`](#flowgraph-obs-set-scene-item-enabled), [`flowgraph.obs.start_record`](#flowgraph-obs-start-record), [`flowgraph.obs.start_stream`](#flowgraph-obs-start-stream), [`flowgraph.obs.stop_record`](#flowgraph-obs-stop-record), [`flowgraph.obs.stop_stream`](#flowgraph-obs-stop-stream), [`flowgraph.obs.trigger_studio_mode_transition`](#flowgraph-obs-trigger-studio-mode-transition), [`flowgraph.ocr.recognize`](#flowgraph-ocr-recognize), [`flowgraph.osc.send`](#flowgraph-osc-send), [`flowgraph.bos.output`](#flowgraph-bos-output), [`flowgraph.process.kill`](#flowgraph-process-kill), [`flowgraph.process.running`](#flowgraph-process-running), [`flowgraph.process.spawn`](#flowgraph-process-spawn), [`flowgraph.process.wait`](#flowgraph-process-wait), [`flowgraph.screenshot.capture`](#flowgraph-screenshot-capture), [`flowgraph.system.cpu_usage`](#flowgraph-system-cpu-usage), [`flowgraph.system.load_avg`](#flowgraph-system-load-avg), [`flowgraph.system.memory`](#flowgraph-system-memory), [`flowgraph.system.process_list`](#flowgraph-system-process-list), [`flowgraph.table.load_google_sheet_auth`](#flowgraph-table-load-google-sheet-auth), [`flowgraph.table.load_google_sheet_public`](#flowgraph-table-load-google-sheet-public), [`flowgraph.table.load_tsv`](#flowgraph-table-load-tsv), [`flowgraph.table.load_xlsx`](#flowgraph-table-load-xlsx), [`flowgraph.table.write_tsv`](#flowgraph-table-write-tsv), [`flowgraph.translate.gas`](#flowgraph-translate-gas), [`flowgraph.translate.libre`](#flowgraph-translate-libre), [`flowgraph.tts.speak`](#flowgraph-tts-speak), [`flowgraph.twitch.ad_run`](#flowgraph-twitch-ad-run), [`flowgraph.twitch.ban`](#flowgraph-twitch-ban), [`flowgraph.twitch.channel_info_update`](#flowgraph-twitch-channel-info-update), [`flowgraph.twitch.chat_clear`](#flowgraph-twitch-chat-clear), [`flowgraph.twitch.chat_send`](#flowgraph-twitch-chat-send), [`flowgraph.twitch.chat_settings_update`](#flowgraph-twitch-chat-settings-update), [`flowgraph.twitch.clip_create`](#flowgraph-twitch-clip-create), [`flowgraph.twitch.get_token`](#flowgraph-twitch-get-token), [`flowgraph.twitch.goals_get`](#flowgraph-twitch-goals-get), [`flowgraph.twitch.poll_create`](#flowgraph-twitch-poll-create), [`flowgraph.twitch.poll_end`](#flowgraph-twitch-poll-end), [`flowgraph.twitch.prediction_create`](#flowgraph-twitch-prediction-create), [`flowgraph.twitch.prediction_end`](#flowgraph-twitch-prediction-end), [`flowgraph.twitch.raid_cancel`](#flowgraph-twitch-raid-cancel), [`flowgraph.twitch.raid_start`](#flowgraph-twitch-raid-start), [`flowgraph.twitch.random_clip`](#flowgraph-twitch-random-clip), [`flowgraph.twitch.shield_mode_update`](#flowgraph-twitch-shield-mode-update), [`flowgraph.twitch.stream_marker_create`](#flowgraph-twitch-stream-marker-create), [`flowgraph.twitch.timeout`](#flowgraph-twitch-timeout), [`flowgraph.twitch.user_id_by_login`](#flowgraph-twitch-user-id-by-login), [`flowgraph.twitch.validate_token`](#flowgraph-twitch-validate-token), [`flowgraph.un.app.command`](#flowgraph-un-app-command), [`flowgraph.un.app.event`](#flowgraph-un-app-event), [`flowgraph.unavatar.camera_effect`](#flowgraph-unavatar-camera-effect), [`flowgraph.unmfz.publish`](#flowgraph-unmfz-publish), [`flowgraph.unmfz.subscribe`](#flowgraph-unmfz-subscribe), [`flowgraph.util.log`](#flowgraph-util-log), [`flowgraph.vmc.send_bone_pos`](#flowgraph-vmc-send-bone-pos), [`flowgraph.vmc.send_root_pos`](#flowgraph-vmc-send-root-pos), [`flowgraph.vrchat.avatar_parameter_bool`](#flowgraph-vrchat-avatar-parameter-bool), [`flowgraph.vrchat.avatar_parameter_float`](#flowgraph-vrchat-avatar-parameter-float), [`flowgraph.vrchat.avatar_parameter_int`](#flowgraph-vrchat-avatar-parameter-int), [`flowgraph.vrchat.chatbox_input`](#flowgraph-vrchat-chatbox-input), [`flowgraph.vrchat.chatbox_typing`](#flowgraph-vrchat-chatbox-typing), [`flowgraph.web.rss_fetch`](#flowgraph-web-rss-fetch), [`flowgraph.window.close`](#flowgraph-window-close), [`flowgraph.window.enum`](#flowgraph-window-enum), [`flowgraph.window.foreground`](#flowgraph-window-foreground), [`flowgraph.window.maximize`](#flowgraph-window-maximize), [`flowgraph.window.minimize`](#flowgraph-window-minimize), [`flowgraph.window.move`](#flowgraph-window-move), [`flowgraph.window.pseudo_fullscreen`](#flowgraph-window-pseudo-fullscreen), [`flowgraph.window.pseudo_fullscreen_exit`](#flowgraph-window-pseudo-fullscreen-exit), [`flowgraph.window.resize`](#flowgraph-window-resize), [`flowgraph.window.restore`](#flowgraph-window-restore)
- **stateful** (24): [`flowgraph.ai.analyze`](#flowgraph-ai-analyze), [`flowgraph.collection.deque`](#flowgraph-collection-deque), [`flowgraph.collection.priority_queue`](#flowgraph-collection-priority-queue), [`flowgraph.collection.reservoir_sample`](#flowgraph-collection-reservoir-sample), [`flowgraph.collection.ring_buffer`](#flowgraph-collection-ring-buffer), [`flowgraph.glossary.match`](#flowgraph-glossary-match), [`flowgraph.glossary.replace`](#flowgraph-glossary-replace), [`flowgraph.physics.integrator`](#flowgraph-physics-integrator), [`flowgraph.physics.spring`](#flowgraph-physics-spring), [`flowgraph.state.accumulator`](#flowgraph-state-accumulator), [`flowgraph.state.bool`](#flowgraph-state-bool), [`flowgraph.state.int_counter`](#flowgraph-state-int-counter), [`flowgraph.state.latch`](#flowgraph-state-latch), [`flowgraph.string.english_to_katakana`](#flowgraph-string-english-to-katakana), [`flowgraph.twitch.clip_command`](#flowgraph-twitch-clip-command), [`flowgraph.twitch.event_notice`](#flowgraph-twitch-event-notice), [`flowgraph.util.debounce`](#flowgraph-util-debounce), [`flowgraph.util.delay`](#flowgraph-util-delay), [`flowgraph.util.edge_detect`](#flowgraph-util-edge-detect), [`flowgraph.util.prev_value`](#flowgraph-util-prev-value), [`flowgraph.util.rate_limit`](#flowgraph-util-rate-limit), [`flowgraph.util.sample_hold`](#flowgraph-util-sample-hold), [`flowgraph.util.throttle`](#flowgraph-util-throttle), [`flowgraph.util.timer_interval`](#flowgraph-util-timer-interval)

### Capability Groups

- **app_control** (2): [`flowgraph.un.app.command`](#flowgraph-un-app-command), [`flowgraph.un.app.event`](#flowgraph-un-app-event)
- **audio_output** (2): [`flowgraph.audio.play`](#flowgraph-audio-play), [`flowgraph.tts.speak`](#flowgraph-tts-speak)
- **credential_access** (3): [`flowgraph.ai.analyze`](#flowgraph-ai-analyze), [`flowgraph.table.load_google_sheet_auth`](#flowgraph-table-load-google-sheet-auth), [`flowgraph.twitch.get_token`](#flowgraph-twitch-get-token)
- **db_read** (1): [`flowgraph.db.sqlite.query`](#flowgraph-db-sqlite-query)
- **desktop_capture** (2): [`flowgraph.ocr.recognize`](#flowgraph-ocr-recognize), [`flowgraph.screenshot.capture`](#flowgraph-screenshot-capture)
- **desktop_notification** (1): [`flowgraph.notify.desktop`](#flowgraph-notify-desktop)
- **file_read** (9): [`flowgraph.audio.play`](#flowgraph-audio-play), [`flowgraph.db.sqlite.query`](#flowgraph-db-sqlite-query), [`flowgraph.file.read_bytes`](#flowgraph-file-read-bytes), [`flowgraph.file.read_json`](#flowgraph-file-read-json), [`flowgraph.file.read_ron`](#flowgraph-file-read-ron), [`flowgraph.file.read_string`](#flowgraph-file-read-string), [`flowgraph.file.read_toml`](#flowgraph-file-read-toml), [`flowgraph.table.load_tsv`](#flowgraph-table-load-tsv), [`flowgraph.table.load_xlsx`](#flowgraph-table-load-xlsx)
- **file_watch** (1): [`flowgraph.ingress.file_watch`](#flowgraph-ingress-file-watch)
- **file_write** (6): [`flowgraph.file.write_bytes`](#flowgraph-file-write-bytes), [`flowgraph.file.write_json`](#flowgraph-file-write-json), [`flowgraph.file.write_ron`](#flowgraph-file-write-ron), [`flowgraph.file.write_string`](#flowgraph-file-write-string), [`flowgraph.file.write_toml`](#flowgraph-file-write-toml), [`flowgraph.table.write_tsv`](#flowgraph-table-write-tsv)
- **network** (49): [`flowgraph.ai.analyze`](#flowgraph-ai-analyze), [`flowgraph.http.request`](#flowgraph-http-request), [`flowgraph.ingress.unmfz`](#flowgraph-ingress-unmfz), [`flowgraph.obs.get_current_program_scene`](#flowgraph-obs-get-current-program-scene), [`flowgraph.obs.get_source_screenshot`](#flowgraph-obs-get-source-screenshot), [`flowgraph.obs.request`](#flowgraph-obs-request), [`flowgraph.obs.set_current_program_scene`](#flowgraph-obs-set-current-program-scene), [`flowgraph.obs.set_scene_item_enabled`](#flowgraph-obs-set-scene-item-enabled), [`flowgraph.obs.start_record`](#flowgraph-obs-start-record), [`flowgraph.obs.start_stream`](#flowgraph-obs-start-stream), [`flowgraph.obs.stop_record`](#flowgraph-obs-stop-record), [`flowgraph.obs.stop_stream`](#flowgraph-obs-stop-stream), [`flowgraph.obs.trigger_studio_mode_transition`](#flowgraph-obs-trigger-studio-mode-transition), [`flowgraph.osc.send`](#flowgraph-osc-send), [`flowgraph.table.load_google_sheet_auth`](#flowgraph-table-load-google-sheet-auth), [`flowgraph.table.load_google_sheet_public`](#flowgraph-table-load-google-sheet-public), [`flowgraph.translate.gas`](#flowgraph-translate-gas), [`flowgraph.translate.libre`](#flowgraph-translate-libre), [`flowgraph.twitch.ad_run`](#flowgraph-twitch-ad-run), [`flowgraph.twitch.ban`](#flowgraph-twitch-ban), [`flowgraph.twitch.channel_info_update`](#flowgraph-twitch-channel-info-update), [`flowgraph.twitch.chat_clear`](#flowgraph-twitch-chat-clear), [`flowgraph.twitch.chat_send`](#flowgraph-twitch-chat-send), [`flowgraph.twitch.chat_settings_update`](#flowgraph-twitch-chat-settings-update), [`flowgraph.twitch.clip_create`](#flowgraph-twitch-clip-create), [`flowgraph.twitch.get_token`](#flowgraph-twitch-get-token), [`flowgraph.twitch.goals_get`](#flowgraph-twitch-goals-get), [`flowgraph.twitch.poll_create`](#flowgraph-twitch-poll-create), [`flowgraph.twitch.poll_end`](#flowgraph-twitch-poll-end), [`flowgraph.twitch.prediction_create`](#flowgraph-twitch-prediction-create), [`flowgraph.twitch.prediction_end`](#flowgraph-twitch-prediction-end), [`flowgraph.twitch.raid_cancel`](#flowgraph-twitch-raid-cancel), [`flowgraph.twitch.raid_start`](#flowgraph-twitch-raid-start), [`flowgraph.twitch.random_clip`](#flowgraph-twitch-random-clip), [`flowgraph.twitch.shield_mode_update`](#flowgraph-twitch-shield-mode-update), [`flowgraph.twitch.stream_marker_create`](#flowgraph-twitch-stream-marker-create), [`flowgraph.twitch.timeout`](#flowgraph-twitch-timeout), [`flowgraph.twitch.user_id_by_login`](#flowgraph-twitch-user-id-by-login), [`flowgraph.twitch.validate_token`](#flowgraph-twitch-validate-token), [`flowgraph.un.app.command`](#flowgraph-un-app-command), [`flowgraph.un.app.event`](#flowgraph-un-app-event), [`flowgraph.vmc.send_bone_pos`](#flowgraph-vmc-send-bone-pos), [`flowgraph.vmc.send_root_pos`](#flowgraph-vmc-send-root-pos), [`flowgraph.vrchat.avatar_parameter_bool`](#flowgraph-vrchat-avatar-parameter-bool), [`flowgraph.vrchat.avatar_parameter_float`](#flowgraph-vrchat-avatar-parameter-float), [`flowgraph.vrchat.avatar_parameter_int`](#flowgraph-vrchat-avatar-parameter-int), [`flowgraph.vrchat.chatbox_input`](#flowgraph-vrchat-chatbox-input), [`flowgraph.vrchat.chatbox_typing`](#flowgraph-vrchat-chatbox-typing), [`flowgraph.web.rss_fetch`](#flowgraph-web-rss-fetch)
- **obs_control** (10): [`flowgraph.obs.get_current_program_scene`](#flowgraph-obs-get-current-program-scene), [`flowgraph.obs.get_source_screenshot`](#flowgraph-obs-get-source-screenshot), [`flowgraph.obs.request`](#flowgraph-obs-request), [`flowgraph.obs.set_current_program_scene`](#flowgraph-obs-set-current-program-scene), [`flowgraph.obs.set_scene_item_enabled`](#flowgraph-obs-set-scene-item-enabled), [`flowgraph.obs.start_record`](#flowgraph-obs-start-record), [`flowgraph.obs.start_stream`](#flowgraph-obs-start-stream), [`flowgraph.obs.stop_record`](#flowgraph-obs-stop-record), [`flowgraph.obs.stop_stream`](#flowgraph-obs-stop-stream), [`flowgraph.obs.trigger_studio_mode_transition`](#flowgraph-obs-trigger-studio-mode-transition)
- **process_control** (4): [`flowgraph.process.kill`](#flowgraph-process-kill), [`flowgraph.process.running`](#flowgraph-process-running), [`flowgraph.process.spawn`](#flowgraph-process-spawn), [`flowgraph.process.wait`](#flowgraph-process-wait)
- **trace_write** (1): [`flowgraph.util.log`](#flowgraph-util-log)
- **twitch_api** (21): [`flowgraph.twitch.ad_run`](#flowgraph-twitch-ad-run), [`flowgraph.twitch.ban`](#flowgraph-twitch-ban), [`flowgraph.twitch.channel_info_update`](#flowgraph-twitch-channel-info-update), [`flowgraph.twitch.chat_clear`](#flowgraph-twitch-chat-clear), [`flowgraph.twitch.chat_send`](#flowgraph-twitch-chat-send), [`flowgraph.twitch.chat_settings_update`](#flowgraph-twitch-chat-settings-update), [`flowgraph.twitch.clip_create`](#flowgraph-twitch-clip-create), [`flowgraph.twitch.get_token`](#flowgraph-twitch-get-token), [`flowgraph.twitch.goals_get`](#flowgraph-twitch-goals-get), [`flowgraph.twitch.poll_create`](#flowgraph-twitch-poll-create), [`flowgraph.twitch.poll_end`](#flowgraph-twitch-poll-end), [`flowgraph.twitch.prediction_create`](#flowgraph-twitch-prediction-create), [`flowgraph.twitch.prediction_end`](#flowgraph-twitch-prediction-end), [`flowgraph.twitch.raid_cancel`](#flowgraph-twitch-raid-cancel), [`flowgraph.twitch.raid_start`](#flowgraph-twitch-raid-start), [`flowgraph.twitch.random_clip`](#flowgraph-twitch-random-clip), [`flowgraph.twitch.shield_mode_update`](#flowgraph-twitch-shield-mode-update), [`flowgraph.twitch.stream_marker_create`](#flowgraph-twitch-stream-marker-create), [`flowgraph.twitch.timeout`](#flowgraph-twitch-timeout), [`flowgraph.twitch.user_id_by_login`](#flowgraph-twitch-user-id-by-login), [`flowgraph.twitch.validate_token`](#flowgraph-twitch-validate-token)
- **window_control** (10): [`flowgraph.window.close`](#flowgraph-window-close), [`flowgraph.window.enum`](#flowgraph-window-enum), [`flowgraph.window.foreground`](#flowgraph-window-foreground), [`flowgraph.window.maximize`](#flowgraph-window-maximize), [`flowgraph.window.minimize`](#flowgraph-window-minimize), [`flowgraph.window.move`](#flowgraph-window-move), [`flowgraph.window.pseudo_fullscreen`](#flowgraph-window-pseudo-fullscreen), [`flowgraph.window.pseudo_fullscreen_exit`](#flowgraph-window-pseudo-fullscreen-exit), [`flowgraph.window.resize`](#flowgraph-window-resize), [`flowgraph.window.restore`](#flowgraph-window-restore)

### Control Triggering

- **control triggerable** (4): [`flowgraph.glossary.forget`](#flowgraph-glossary-forget), [`flowgraph.glossary.learn`](#flowgraph-glossary-learn), [`flowgraph.ingress.control`](#flowgraph-ingress-control), [`flowgraph.ingress.web_input`](#flowgraph-ingress-web-input)

### Snapshot / Restore Support

- **snapshot** (11): [`flowgraph.collection.deque`](#flowgraph-collection-deque), [`flowgraph.collection.priority_queue`](#flowgraph-collection-priority-queue), [`flowgraph.collection.reservoir_sample`](#flowgraph-collection-reservoir-sample), [`flowgraph.collection.ring_buffer`](#flowgraph-collection-ring-buffer), [`flowgraph.physics.integrator`](#flowgraph-physics-integrator), [`flowgraph.physics.spring`](#flowgraph-physics-spring), [`flowgraph.state.accumulator`](#flowgraph-state-accumulator), [`flowgraph.state.bool`](#flowgraph-state-bool), [`flowgraph.state.int_counter`](#flowgraph-state-int-counter), [`flowgraph.state.latch`](#flowgraph-state-latch), [`flowgraph.util.rate_limit`](#flowgraph-util-rate-limit)
- **restore** (11): [`flowgraph.collection.deque`](#flowgraph-collection-deque), [`flowgraph.collection.priority_queue`](#flowgraph-collection-priority-queue), [`flowgraph.collection.reservoir_sample`](#flowgraph-collection-reservoir-sample), [`flowgraph.collection.ring_buffer`](#flowgraph-collection-ring-buffer), [`flowgraph.physics.integrator`](#flowgraph-physics-integrator), [`flowgraph.physics.spring`](#flowgraph-physics-spring), [`flowgraph.state.accumulator`](#flowgraph-state-accumulator), [`flowgraph.state.bool`](#flowgraph-state-bool), [`flowgraph.state.int_counter`](#flowgraph-state-int-counter), [`flowgraph.state.latch`](#flowgraph-state-latch), [`flowgraph.util.rate_limit`](#flowgraph-util-rate-limit)

## ai

### `flowgraph.ai.analyze`

**AI Analyze** — 用途別のモデルで資料・画像を分析。共演者の記憶・発話には入れず、完了後に結果を返す。

**Metadata:** contract: inputs=6 / outputs=10 / properties=7 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: `network`, `credential_access`; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `cancel` | `exec` (in) | — |  |
| `__resume__` | `exec` (in) | — |  |
| `content` | `string` | — |  |
| `image_data_url` | `string` | `""` |  |
| `context` | `json` | `{}` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `on_started` | `exec` (out) |  |
| `on_cancelled` | `exec` (out) |  |
| `summary` | `string` |  |
| `analysis` | `json` |  |
| `observation_text` | `string` |  |
| `receipt` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `purpose` | `string` | `"screen_detail"` |  |  |
| `model` | `string` | `"gpt-6.1-terra"` |  |  |
| `reasoning_effort` | `string` | `"none"` |  |  |
| `instructions` | `string` | `"質問で求められた情報を日本語で簡潔に抽出する。"` |  |  |
| `max_output_tokens` | `int` | `1600` |  |  |
| `timeout_ms` | `int` | `30000` |  |  |
| `api_key_file` | `string` | `""` |  |  |

### `flowgraph.ai.observe`

**AI Observe** — 明示edgeから受け取ったtextをAI personaの観測入力へ渡す。内部配送topicは公開Flowgraph接続ではない。

**Metadata:** contract: inputs=4 / outputs=3 / properties=2 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `content` | `string` | `""` |  |
| `source_actor` | `string` | `""` |  |
| `meta` | `json` | `{}` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `result` | `result<bool>` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `persona_id` | `string` | `"*"` |  | 対象persona ID。`*`は起動中の全persona。文字列channel名ではない。 |
| `request_class` | `string` | `""` |  | 信頼済みFlowgraph経路が付与するAI依頼種別。外部本文からは推測しない。例: `twitch_channel_points_reward`。空なら通常観測。 |

### `flowgraph.ingress.ai_avatar_intent`

**AI Avatar Intent** — AIが確定したAvatarIntent JSONを明示edgeへ公開する。

**Metadata:** contract: inputs=5 / outputs=5 / properties=1 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__persona_id__` | `string` | `""` |  |
| `__utterance_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `persona_id` | `string` |  |
| `utterance_id` | `string` |  |
| `meta` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `persona_id` | `string` | `""` |  | 空なら全persona。AIサービス境界の識別子であり、公開channel名ではない。 |

### `flowgraph.ingress.ai_avatar_pose`

**AI Avatar Pose** — AIが確定した名前付きAvatarPoseを明示edgeへ公開する。

**Metadata:** contract: inputs=5 / outputs=5 / properties=1 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__persona_id__` | `string` | `""` |  |
| `__utterance_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `persona_id` | `string` |  |
| `utterance_id` | `string` |  |
| `meta` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `persona_id` | `string` | `""` |  | 空なら全persona。AIサービス境界の識別子であり、公開channel名ではない。 |

### `flowgraph.ingress.ai_response`

**AI Response** — AI personaの確定応答全文を明示edgeへ公開する。

**Metadata:** contract: inputs=5 / outputs=5 / properties=1 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__persona_id__` | `string` | `""` |  |
| `__utterance_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `persona_id` | `string` |  |
| `utterance_id` | `string` |  |
| `meta` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `persona_id` | `string` | `""` |  | 空なら全persona。AIサービス境界の識別子であり、公開channel名ではない。 |

### `flowgraph.ingress.ai_speech`

**AI Speech** — AI personaが読み上げ対象として確定した発話segmentと、その時点までの累積発話を明示edgeへ公開する。

**Metadata:** contract: inputs=6 / outputs=6 / properties=1 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__persona_id__` | `string` | `""` |  |
| `__utterance_id__` | `string` | `""` |  |
| `__utterance_content__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `persona_id` | `string` |  |
| `utterance_id` | `string` |  |
| `utterance_content` | `string` |  |
| `meta` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `persona_id` | `string` | `""` |  | 空なら全persona。AIサービス境界の識別子であり、公開channel名ではない。 |

## audio

### `flowgraph.audio.envelope`

**Audio Envelope** — Pure PCM envelope analyzer. Computes RMS, peak, dBFS, sample count, frame count, and duration.

**Metadata:** contract: inputs=4 / outputs=6 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `samples` | `bytes` | `""` |  |
| `format` | `string` | `"pcm_s16le"` | enum: `pcm_s16le`, `pcm_f32le` |
| `channels` | `int` | `1` |  |
| `sample_rate` | `int` | `16000` |  |

| Output | Type | Note |
|---|---|---|
| `rms` | `float` |  |
| `peak` | `float` |  |
| `dbfs` | `float` |  |
| `sample_count` | `int` |  |
| `frame_count` | `int` |  |
| `duration_ms` | `float` |  |

### `flowgraph.audio.pitch`

**Audio Pitch** — Pure PCM pitch estimator using normalized autocorrelation.

**Metadata:** contract: inputs=6 / outputs=4 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `samples` | `bytes` | `""` |  |
| `format` | `string` | `"pcm_s16le"` | enum: `pcm_s16le`, `pcm_f32le` |
| `sample_rate` | `int` | `16000` |  |
| `min_hz` | `float` | `70.0` |  |
| `max_hz` | `float` | `1000.0` |  |
| `voiced_threshold` | `float` | `0.35` |  |

| Output | Type | Note |
|---|---|---|
| `frequency_hz` | `float` |  |
| `confidence` | `float` |  |
| `voiced` | `bool` |  |
| `sample_count` | `int` |  |

### `flowgraph.audio.play`

**Audio Play** — Effectful audio playback. Plays encoded audio bytes or a file path through the shared VAC audio sink. Use graph-file:assets/sound.wav for an asset relative to the defining graph file.

**Metadata:** contract: inputs=5 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `audio_output`, `file_read`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `bytes` | `bytes` | `""` |  |
| `path` | `string` | `""` |  |
| `volume` | `float` | `1.0` |  |
| `speed` | `float` | `1.0` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `played` | `bool` |  |
| `source` | `string` |  |
| `bytes_len` | `int` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## avatar

### `flowgraph.avatar.intent_play_unmfz`

**Avatar: Play Intent to UNMF/Z** — AvatarIntentを時系列UNMotionFrameへ展開し、終了時のneutral復帰までAvatarLink compositorへ投入する。

**Metadata:** contract: inputs=11 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__avatar_complete__` | `exec` (in) | — |  |
| `__avatar_job_id__` | `int` | `0` |  |
| `exec_in` | `exec` (in) | — |  |
| `intent` | `json` | — |  |
| `base_key_expr` | `string` | `""` |  |
| `topic_mode` | `string` | `"frame"` |  |
| `schema_major` | `int` | `1` |  |
| `endpoint` | `json` | `null` |  |
| `fps` | `int` | `30` |  |
| `release_ms` | `int` | `200` |  |
| `dry_run` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `frames_sent` | `int` |  |
| `key_expr` | `string` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.avatar.intent_to_unmf`

**Avatar: Intent to UNMF** — AI 用 AvatarIntent JSON を安全な UNMotionFrame に変換する。

**Metadata:** contract: inputs=4 / outputs=5 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `intent` | `json` | — |  |
| `sequence` | `int` | `0` |  |
| `elapsed_ms` | `int` | `0` |  |
| `expression_bindings` | `json` | `{}` |  |

| Output | Type | Note |
|---|---|---|
| `ok` | `bool` |  |
| `unmf` | `unmf` |  |
| `target_link_id` | `string` |  |
| `error` | `string` |  |
| `result` | `result<unmf>` |  |

### `flowgraph.avatar.pose_asset_play_unmfz`

**Avatar: Play Pose Asset to UNMF/Z** — 保存済みUNMF poseを所有boneだけ適用し、baselineで解除する。口パクと瞬きはAvatarLink compositorが保持する。

**Metadata:** contract: inputs=17 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__avatar_complete__` | `exec` (in) | — |  |
| `__avatar_job_id__` | `int` | `0` |  |
| `exec_in` | `exec` (in) | — |  |
| `pose` | `unmf` | — |  |
| `release` | `unmf` | — |  |
| `owned_bones` | `string` | `""` |  |
| `owned_fingers` | `string` | `""` |  |
| `target_link_id` | `string` | `""` |  |
| `base_key_expr` | `string` | `""` |  |
| `topic_mode` | `string` | `"frame"` |  |
| `schema_major` | `int` | `1` |  |
| `endpoint` | `json` | `null` |  |
| `attack_ms` | `int` | `250` |  |
| `hold_ms` | `int` | `1000` |  |
| `release_ms` | `int` | `200` |  |
| `fps` | `int` | `30` |  |
| `dry_run` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `frames_sent` | `int` |  |
| `key_expr` | `string` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.avatar.pose_catalog_lookup`

**Avatar: Lookup Pose Catalog** — 許可済みpreset名から保存UNMF poseと安全な再生設定を解決する。

**Metadata:** contract: inputs=3 / outputs=9 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `catalog` | `json` | — |  |
| `preset` | `string` | `""` |  |
| `asset_root` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `ok` | `bool` |  |
| `pose_path` | `string` |  |
| `release_path` | `string` |  |
| `owned_bones` | `string` |  |
| `owned_fingers` | `string` |  |
| `attack_ms` | `int` |  |
| `hold_ms` | `int` |  |
| `release_ms` | `int` |  |
| `error` | `string` |  |

## bytes

### `flowgraph.bytes.from_base64`

**Bytes From Base64** — Base64 string を bytes にデコードする。失敗時はエラー halt。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `text` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `bytes` | `bytes` |  |

### `flowgraph.bytes.len`

**Bytes Length** — bytes の byte length を返す。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `bytes` | `bytes` | — |  |

| Output | Type | Note |
|---|---|---|
| `len` | `int` |  |

### `flowgraph.bytes.to_base64`

**Bytes To Base64** — bytes を Base64 string にエンコードする。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `bytes` | `bytes` | — |  |

| Output | Type | Note |
|---|---|---|
| `text` | `string` |  |

## collection

### `flowgraph.collection.deque`

**Deque** — Stateful double-ended JSON queue with bounded capacity.

**Metadata:** contract: inputs=7 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `push_back` | `exec` (in) | — |  |
| `push_front` | `exec` (in) | — |  |
| `pop_back` | `exec` (in) | — |  |
| `pop_front` | `exec` (in) | — |  |
| `clear` | `exec` (in) | — |  |
| `item` | `json` | `null` |  |
| `capacity` | `int` | `1000` |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<json>` |  |
| `count` | `int` |  |
| `popped` | `json` |  |
| `has_popped` | `bool` |  |
| `updated` | `exec` (out) |  |
| `popped_exec` | `exec` (out) |  |
| `cleared` | `exec` (out) |  |

### `flowgraph.collection.priority_queue`

**Priority Queue** — Stateful bounded max-priority queue for JSON items.

**Metadata:** contract: inputs=6 / outputs=8 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `push` | `exec` (in) | — |  |
| `pop` | `exec` (in) | — |  |
| `clear` | `exec` (in) | — |  |
| `item` | `json` | `null` |  |
| `priority` | `float` | `0.0` |  |
| `capacity` | `int` | `1000` |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<json>` |  |
| `count` | `int` |  |
| `popped` | `json` |  |
| `popped_priority` | `float` |  |
| `has_popped` | `bool` |  |
| `updated` | `exec` (out) |  |
| `popped_exec` | `exec` (out) |  |
| `cleared` | `exec` (out) |  |

### `flowgraph.collection.reservoir_sample`

**Reservoir Sample** — Stateful deterministic reservoir sampler for JSON event streams.

**Metadata:** contract: inputs=5 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `observe` | `exec` (in) | — |  |
| `clear` | `exec` (in) | — |  |
| `item` | `json` | `null` |  |
| `capacity` | `int` | `32` |  |
| `seed` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `sample` | `list<json>` |  |
| `count_total` | `int` |  |
| `count_sampled` | `int` |  |
| `accepted` | `bool` |  |
| `index` | `int` |  |
| `updated` | `exec` (out) |  |
| `cleared` | `exec` (out) |  |

### `flowgraph.collection.ring_buffer`

**Ring Buffer** — Stateful fixed-capacity JSON ring buffer. push drops oldest when full.

**Metadata:** contract: inputs=4 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `push` | `exec` (in) | — |  |
| `clear` | `exec` (in) | — |  |
| `item` | `json` | `null` |  |
| `capacity` | `int` | `32` |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<json>` |  |
| `count` | `int` |  |
| `dropped_count` | `int` |  |
| `latest` | `json` |  |
| `updated` | `exec` (out) |  |
| `cleared` | `exec` (out) |  |

## command

### `flowgraph.command.authorize`

**Command Authorize** — 信頼できる入力元のlogin/flagsを許可一覧とOR照合。loginは大小文字を区別しない。許可一覧は両方を明示接続し、両方空なら全許可。本文中の名前を認証情報として使わない。

**Metadata:** contract: inputs=5 / outputs=3 / properties=0 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `login` | `string` | `""` |  |
| `flags` | `list<string>` | `[]` |  |
| `allowed_logins` | `list<string>` | — |  |
| `allowed_flags` | `list<string>` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_allowed` | `exec` (out) |  |
| `on_denied` | `exec` (out) |  |
| `allowed` | `bool` |  |

### `flowgraph.command.match`

**Command Match** — prefix 付きコマンド文字列を verb + args にパースして exec を分岐

**Metadata:** contract: inputs=3 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `content` | `string` | — |  |
| `prefix` | `string` | `"/"` |  |

| Output | Type | Note |
|---|---|---|
| `on_command` | `exec` (out) |  |
| `on_other` | `exec` (out) |  |
| `command` | `string` |  |
| `args` | `list<string>` |  |
| `original` | `string` |  |

### `flowgraph.command.set`

**Command Set** — command_name に一致する set を sets プロパティから引き、pre → channel_contents → post の順でチャンネルへ push する

**Metadata:** contract: inputs=2 / outputs=4 / properties=3 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `command_name` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_set` | `exec` (out) |  |
| `on_none` | `exec` (out) |  |
| `matched_name` | `string` |  |
| `entry_count` | `int` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `sets` | `json` | `[]` |  | コマンドセット配列。各要素は `{ name, pre?, post?, channel_contents? }` を持つ JSON。 |
| `pre_post_channel` | `string` | `""` |  | `pre` / `post` を流すチャンネル名。空なら pre/post を無視する。 |
| `source_actor` | `string` | `""` |  | ChannelDatum の source_actor。空なら `flowgraph:<node_id>` を自動スタンプ。 |

## compare

### `flowgraph.compare.eq`

**Equal** — Json 値同士を比較

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.compare.float_gt`

**Float >**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `float` | — |  |
| `b` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.compare.float_lt`

**Float <**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `float` | — |  |
| `b` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.compare.int_ge`

**Int >=**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.compare.int_gt`

**Int >**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.compare.int_le`

**Int <=**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.compare.int_lt`

**Int <**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.compare.neq`

**Not Equal**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

## constants

### `flowgraph.constants.math`

**Math Constants** — Common dimensionless mathematical constants.

**Metadata:** contract: inputs=0 / outputs=6 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `pi` | `float` |  |
| `tau` | `float` |  |
| `e` | `float` |  |
| `sqrt2` | `float` |  |
| `ln2` | `float` |  |
| `golden_ratio` | `float` |  |

### `flowgraph.constants.physics`

**Physics Constants** — Common SI physical constants as Float values with unit-bearing port names.

**Metadata:** contract: inputs=0 / outputs=10 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `c_m_s` | `float` |  |
| `G_m3_kg_s2` | `float` |  |
| `h_J_s` | `float` |  |
| `hbar_J_s` | `float` |  |
| `k_B_J_K` | `float` |  |
| `e_C` | `float` |  |
| `N_A_mol` | `float` |  |
| `R_J_mol_K` | `float` |  |
| `epsilon0_F_m` | `float` |  |
| `mu0_N_A2` | `float` |  |

## convert

### `flowgraph.convert.bool_to_string`

**Bool → String**

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |

### `flowgraph.convert.float_to_int`

**Float → Int** — 切り捨て（trunc）

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.convert.float_to_string`

**Float → String**

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |

### `flowgraph.convert.int_to_float`

**Int → Float**

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `float` |  |

### `flowgraph.convert.int_to_string`

**Int → String**

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |

### `flowgraph.convert.json_to_msgpack`

**JSON → MessagePack** — JSON 値を MessagePack bytes へ変換する。

**Metadata:** contract: inputs=1 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `json` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `bytes` | `bytes` |  |
| `byte_count` | `int` |  |

### `flowgraph.convert.json_to_toml`

**JSON → TOML** — JSON 値を TOML 文字列へ変換する。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `json` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `toml` | `string` |  |

### `flowgraph.convert.msgpack_to_json`

**MessagePack → JSON** — MessagePack bytes を JSON 値へ変換する。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `bytes` | `bytes` | — |  |

| Output | Type | Note |
|---|---|---|
| `json` | `json` |  |

### `flowgraph.convert.ron_to_toml`

**RON → TOML** — RON 文字列を JSON 値経由で TOML 文字列へ変換する。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `ron` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `toml` | `string` |  |

### `flowgraph.convert.string_to_float`

**String → Float** — パース失敗はエラー halt

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `float` |  |

### `flowgraph.convert.string_to_int`

**String → Int** — パース失敗はエラー halt

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.convert.toml_to_json`

**TOML → JSON** — TOML 文字列を JSON 値へ変換する。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `toml` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `json` | `json` |  |

### `flowgraph.convert.toml_to_ron`

**TOML → RON** — TOML 文字列を JSON 値経由で RON 文字列へ変換する。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `toml` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `ron` | `string` |  |

## datetime

### `flowgraph.datetime.add_duration`

**DateTime + Duration** — Add a duration (Quantity<time>) to a DateTime. Dimensionless Quantity (Float の ξ-3 coerce 経由) は「秒」と解釈される。次元不一致 (例: length) は error。

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `datetime` | `datetime` | — |  |
| `duration` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `datetime` |  |

### `flowgraph.datetime.diff`

**DateTime - DateTime** — Compute `lhs - rhs` as a Quantity<time> (unit: seconds, nanosecond precision). 結果は正負 OK。`flowgraph.unit.convert` で ms / us / ns に変換可能。

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `lhs` | `datetime` | — |  |
| `rhs` | `datetime` | — |  |

| Output | Type | Note |
|---|---|---|
| `duration` | `quantity` |  |

### `flowgraph.datetime.epoch_ms`

**DateTime -> Epoch ms** — Return Unix epoch milliseconds as a Quantity (unit: ms, dim: time). Negative for pre-1970 timestamps. 他の時間単位へは `flowgraph.unit.convert` で変換。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `datetime` | `datetime` | — |  |

| Output | Type | Note |
|---|---|---|
| `millis` | `quantity` |  |

### `flowgraph.datetime.format`

**DateTime Format** — Format a DateTime as a string. `rfc3339`: `"2026-04-24T12:34:56.123Z"` 形式 (UTC または `timezone` 指定時は offset 表示)。 `iso8601_compact`: `"20260424T123456Z"` 形式 (ファイル名向け)。 `unix_seconds` / `unix_millis`: 整数文字列。 `custom`: `custom_format` プロパティの strftime パターンを適用 (jiff::Zoned::strftime)。

**Metadata:** contract: inputs=1 / outputs=1 / properties=3 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `datetime` | `datetime` | — |  |

| Output | Type | Note |
|---|---|---|
| `text` | `string` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `format` | `string` | `"rfc3339"` |  | choices: `rfc3339`, `iso8601_compact`, `unix_seconds`, `unix_millis`, `custom`; Output shape. `rfc3339` / `iso8601_compact` / `unix_seconds` / `unix_millis` / `custom`. |
| `custom_format` | `string` | `""` |  | strftime pattern used when `format = "custom"`. See jiff::fmt::strtime. Example: `"%Y-%m-%d %H:%M:%S"`. |
| `timezone` | `string` | `""` |  | Fixed offset for display (`""` / `"Z"` / `"UTC"` = UTC, `"+09:00"` etc.). Applies to rfc3339 / iso8601_compact / custom. unix_* are always UTC-absolute and ignore this. |

### `flowgraph.datetime.from_epoch_ms`

**Epoch ms -> DateTime** — Construct a DateTime from Unix epoch milliseconds. Quantity<time> (任意の時間単位) は SI 秒 → ms に正規化されて受理される。 Dimensionless Quantity (Float の ξ-3 coerce 経由) は「ms の数値」として解釈される。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `millis` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `datetime` | `datetime` |  |

### `flowgraph.datetime.now`

**DateTime Now** — Emit the current wall-clock time as a DateTime (UTC absolute, nanosecond precision). 非決定性 (呼び出しごとに異なる結果) なので、すなゆく sample する用途では状態化ノード (prev_value 等) と組み合わせること。

**Metadata:** contract: inputs=0 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `datetime` | `datetime` |  |

### `flowgraph.datetime.parse`

**DateTime Parse** — Parse an RFC3339 / ISO 8601 string into a DateTime. Accepts both aware ("...Z" / "...+09:00") and naive ("2026-04-24T12:34:56") inputs. Naive 入力は `default_timezone` プロパティ (空なら UTC) で解釈される。`require_timezone = true` のときは naive を拒否する strict モード。

**Metadata:** contract: inputs=1 / outputs=1 / properties=2 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `s` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `datetime` | `datetime` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `require_timezone` | `bool` | `false` |  | When true, naive (no-timezone) inputs are rejected. Default false: naive inputs are interpreted with `default_timezone` (or UTC). |
| `default_timezone` | `string` | `""` |  | Fixed offset to apply when the input has no timezone info. Accepts `""` / `"Z"` / `"UTC"` (= UTC), `"+09:00"`, `"-05:30"`. IANA zones (`"Asia/Tokyo"`) are rejected (v0 is fixed-offset only). |

### `flowgraph.datetime.sub_duration`

**DateTime - Duration** — Subtract a duration (Quantity<time>) from a DateTime. Dimensionless Quantity は「秒」と解釈される (ξ-3 coerce)。

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `datetime` | `datetime` | — |  |
| `duration` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `datetime` |  |

### `flowgraph.datetime.try_parse`

**DateTime Try Parse** — Parse a datetime string and return failure as result<datetime> instead of halting.

**Metadata:** contract: inputs=1 / outputs=4 / properties=2 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `s` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `ok` | `bool` |  |
| `datetime` | `datetime` |  |
| `error` | `string` |  |
| `result` | `result<datetime>` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `require_timezone` | `bool` | `false` |  | When true, naive (no-timezone) inputs are rejected. Default false: naive inputs are interpreted with `default_timezone` (or UTC). |
| `default_timezone` | `string` | `""` |  | Fixed offset to apply when the input has no timezone info. Accepts `""` / `"Z"` / `"UTC"` (= UTC), `"+09:00"`, `"-05:30"`. IANA zones (`"Asia/Tokyo"`) are rejected (v0 is fixed-offset only). |

## db

### `flowgraph.db.sqlite.query`

**SQLite Query** — SQLite database を read-only 接続で query し、結果を Table として返す。

**Metadata:** contract: inputs=5 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_read`, `db_read`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |
| `sql` | `string` | — |  |
| `params` | `json` | `[]` |  |
| `timeout_ms` | `int` | `8000` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `table` | `table` |  |
| `row_count` | `int` |  |
| `column_count` | `int` |  |
| `error` | `string` |  |
| `result` | `result<table>` |  |

## dictionary

### `flowgraph.dictionary.from_entries`

**Dictionary From Entries** — Build a generic string-key Dictionary from List<Json> object entries.

**Metadata:** contract: inputs=3 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `entries` | `list<json>` | — |  |
| `key_field` | `string` | `"key"` |  |
| `value_field` | `string` | `"value"` |  |

| Output | Type | Note |
|---|---|---|
| `dictionary` | `map<json>` |  |
| `count` | `int` |  |

### `flowgraph.dictionary.get`

**Dictionary Get** — Get a value from a generic string-key Dictionary.

**Metadata:** contract: inputs=3 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `dictionary` | `map<json>` | — |  |
| `key` | `string` | — |  |
| `default` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `json` |  |
| `found` | `bool` |  |

### `flowgraph.dictionary.has`

**Dictionary Has** — Check whether a generic string-key Dictionary contains a key.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `dictionary` | `map<json>` | — |  |
| `key` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.dictionary.keys`

**Dictionary Keys** — Return sorted keys from a generic string-key Dictionary.

**Metadata:** contract: inputs=1 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `dictionary` | `map<json>` | — |  |

| Output | Type | Note |
|---|---|---|
| `keys` | `list<string>` |  |
| `count` | `int` |  |

### `flowgraph.dictionary.remove`

**Dictionary Remove** — Return a Dictionary with one key removed.

**Metadata:** contract: inputs=2 / outputs=4 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `dictionary` | `map<json>` | — |  |
| `key` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `dictionary` | `map<json>` |  |
| `count` | `int` |  |
| `removed` | `bool` |  |
| `value` | `json` |  |

### `flowgraph.dictionary.set`

**Dictionary Set** — Return a Dictionary with one key set to a JSON value.

**Metadata:** contract: inputs=3 / outputs=3 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `dictionary` | `map<json>` | `{}` |  |
| `key` | `string` | — |  |
| `value` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `dictionary` | `map<json>` |  |
| `count` | `int` |  |
| `replaced` | `bool` |  |

## easing

### `flowgraph.easing.apply`

**Easing apply** — Map a scalar t (conventionally in [0, 1]) through an easing curve. Single node with a `curve` enum property (19 variants: linear, quad/cubic/sine/expo/elastic/bounce × in/out/inout). `clamp_t = true` (default) clamps input to [0, 1] before evaluation — set false to let elastic/bounce overshoot naturally.

**Metadata:** contract: inputs=1 / outputs=1 / properties=2 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `t` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `curve` | `string` | `"linear"` |  | choices: `linear`, `quad_in`, `quad_out`, `quad_inout`, `cubic_in`, `cubic_out`, `cubic_inout`, `sine_in`, `sine_out`, `sine_inout`, `expo_in`, `expo_out`, `expo_inout`, `elastic_in`, `elastic_out`, `elastic_inout`, `bounce_in`, `bounce_out`, `bounce_inout`; Easing curve name. One of: linear, quad_in/out/inout, cubic_in/out/inout, sine_in/out/inout, expo_in/out/inout, elastic_in/out/inout, bounce_in/out/inout. |
| `clamp_t` | `bool` | `true` |  | When true, t is clamped to [0, 1] before the curve is applied. Default true. |

## file

### `flowgraph.file.read_bytes`

**File Read Bytes** — ファイルを bytes として読み込む。Effectful。同梱素材は graph-file:assets/example を指定すると、このノードの定義元グラフから相対参照できます。

**Metadata:** contract: inputs=2 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_read`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes` | `bytes` |  |
| `bytes_read` | `int` |  |
| `error` | `string` |  |
| `result` | `result<bytes>` |  |

### `flowgraph.file.read_json`

**File Read JSON** — JSON ファイルを読み込む。Effectful。同梱素材は graph-file:assets/example を指定すると、このノードの定義元グラフから相対参照できます。

**Metadata:** contract: inputs=2 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_read`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `json` | `json` |  |
| `bytes_read` | `int` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.file.read_ron`

**File Read RON** — RON ファイルを JSON 値へ変換して読み込む。Effectful。同梱素材は graph-file:assets/example を指定すると、このノードの定義元グラフから相対参照できます。

**Metadata:** contract: inputs=2 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_read`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `json` | `json` |  |
| `bytes_read` | `int` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.file.read_string`

**File Read String** — テキストファイルを UTF-8 文字列として読み込む。Effectful。同梱素材は graph-file:assets/example を指定すると、このノードの定義元グラフから相対参照できます。

**Metadata:** contract: inputs=2 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_read`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `text` | `string` |  |
| `bytes_read` | `int` |  |
| `error` | `string` |  |
| `result` | `result<string>` |  |

### `flowgraph.file.read_toml`

**File Read TOML** — TOML ファイルを JSON 値へ変換して読み込む。Effectful。同梱素材は graph-file:assets/example を指定すると、このノードの定義元グラフから相対参照できます。

**Metadata:** contract: inputs=2 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_read`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `json` | `json` |  |
| `bytes_read` | `int` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.file.write_bytes`

**File Write Bytes** — bytes をファイルへ atomic rename で書き込む。Effectful

**Metadata:** contract: inputs=3 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_write`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |
| `bytes` | `bytes` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_written` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.file.write_json`

**File Write JSON** — JSON 値をファイルへ atomic rename で書き込む。Effectful

**Metadata:** contract: inputs=4 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_write`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |
| `json` | `json` | — |  |
| `pretty` | `bool` | `true` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_written` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.file.write_ron`

**File Write RON** — JSON 値を RON としてファイルへ atomic rename で書き込む。Effectful

**Metadata:** contract: inputs=4 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_write`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |
| `json` | `json` | — |  |
| `pretty` | `bool` | `true` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_written` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.file.write_string`

**File Write String** — 文字列を UTF-8 ファイルへ atomic rename で書き込む。Effectful

**Metadata:** contract: inputs=3 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_write`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |
| `text` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_written` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.file.write_toml`

**File Write TOML** — JSON 値を TOML としてファイルへ atomic rename で書き込む。Effectful

**Metadata:** contract: inputs=3 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_write`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |
| `json` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_written` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

## flow

### `flowgraph.flow.branch`

**Branch** — cond が true なら then を、false なら else を発火

**Metadata:** contract: inputs=2 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `cond` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `then` | `exec` (out) |  |
| `else` | `exec` (out) |  |

### `flowgraph.flow.gate`

**Gate** — open=true の間だけ exec を通す（ステートレス）

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `open` | `bool` | `true` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |

### `flowgraph.flow.merge_bool`

**Merge Events (bool)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `bool` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `bool` |  |

### `flowgraph.flow.merge_bytes`

**Merge Events (bytes)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `bytes` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `bytes` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `bytes` |  |

### `flowgraph.flow.merge_datetime`

**Merge Events (datetime)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `datetime` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `datetime` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `datetime` |  |

### `flowgraph.flow.merge_float`

**Merge Events (float)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `float` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `float` |  |

### `flowgraph.flow.merge_int`

**Merge Events (int)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `int` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `int` |  |

### `flowgraph.flow.merge_json`

**Merge Events (json)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `json` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `json` |  |

### `flowgraph.flow.merge_motion_frame`

**Merge Events (motion_frame)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `motion_frame` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `motion_frame` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `motion_frame` |  |

### `flowgraph.flow.merge_quantity`

**Merge Events (quantity)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `quantity` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `quantity` |  |

### `flowgraph.flow.merge_string`

**Merge Events (string)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `string` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `string` |  |

### `flowgraph.flow.merge_table`

**Merge Events (table)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `table` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `table` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `table` |  |

### `flowgraph.flow.merge_unmf`

**Merge Events (unmf)** — 発火した側の値だけを読み、共通の後続へ1件ずつ渡す。両側同時発火はエラー。3経路以上は直列に接続する。

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_a` | `exec` (in) | — |  |
| `value_a` | `unmf` | — |  |
| `exec_b` | `exec` (in) | — |  |
| `value_b` | `unmf` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `unmf` |  |

## glossary

### `flowgraph.glossary.forget`

**Glossary Forget** — Glossary Table から surface (+ reading) 一致行を削除。mode=latest/all/exact、is_locked 保護

**Metadata:** contract: inputs=5 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: control; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `dictionary` | `table` | `[]` |  |
| `surface` | `string` | — |  |
| `reading` | `string` | `""` |  |
| `mode` | `string` | `"latest"` |  |

| Output | Type | Note |
|---|---|---|
| `on_forgotten` | `exec` (out) |  |
| `on_nothing` | `exec` (out) |  |
| `on_locked` | `exec` (out) |  |
| `updated_dictionary` | `table` |  |
| `removed_count` | `int` |  |
| `locked_count` | `int` |  |
| `feedback` | `string` |  |

### `flowgraph.glossary.learn`

**Glossary Learn** — Glossary Table に 11 カラムエントリを append。同値エントリは duplicate 検出して no-op

**Metadata:** contract: inputs=10 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: control; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `dictionary` | `table` | `[]` |  |
| `reading` | `string` | — |  |
| `surface` | `string` | — |  |
| `part_of_speech` | `string` | `"名詞"` |  |
| `comment` | `string` | `""` |  |
| `priority` | `int` | `0` |  |
| `by` | `string` | `""` |  |
| `tags` | `string` | `""` |  |
| `expires_at` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_learned` | `exec` (out) |  |
| `on_duplicate` | `exec` (out) |  |
| `updated_dictionary` | `table` |  |
| `added_entry` | `json` |  |
| `feedback` | `string` |  |

### `flowgraph.glossary.match`

**Glossary Match** — Glossary Table で text を照合し、一致エントリと captures を取り出す。exec 分岐可能。Stateful

**Metadata:** contract: inputs=3 / outputs=7 / properties=2 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `text` | `string` | — |  |
| `dictionary` | `table` | `[]` |  |

| Output | Type | Note |
|---|---|---|
| `on_match` | `exec` (out) |  |
| `on_no_match` | `exec` (out) |  |
| `matched_entries` | `list<json>` |  |
| `matched_count` | `int` |  |
| `captures` | `list<list<string>>` |  |
| `first_reading` | `string` |  |
| `first_captures` | `list<string>` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `match_policy` | `string` | `"first"` |  | first / all / longest |
| `anchor` | `string` | `"anywhere"` |  | anywhere / prefix / full |

### `flowgraph.glossary.replace`

**Glossary Replace** — Glossary Table（11 カラム）で content を literal(AC) + regex 統合で逐次置換。Stateful（AC/Regex キャッシュ）

**Metadata:** contract: inputs=2 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `content` | `string` | — |  |
| `dictionary` | `table` | `[]` |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |
| `applied_count` | `int` |  |

## http

### `flowgraph.http.request`

**HTTP Request** — 汎用 HTTP request。method / headers / JSON body / timeout / retry_count を指定できる。

**Metadata:** contract: inputs=8 / outputs=8 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `method` | `string` | `"GET"` |  |
| `url` | `string` | — |  |
| `headers` | `json` | `{}` |  |
| `body` | `json` | `null` |  |
| `timeout_ms` | `int` | `5000` |  |
| `retry_count` | `int` | `0` |  |
| `retry_delay_ms` | `int` | `250` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status` | `int` |  |
| `body` | `string` |  |
| `json` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## ingress

### `flowgraph.ingress.control`

**Control Input** — Control APIでこのノードを明示指定して投入。本文・送信者・フラグを後続へ渡す。送信者とフラグはAPI呼出者が提供する情報であり、このノードは本人確認を行わない。

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: control; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `content` | `string` | `""` |  |
| `source_actor` | `string` | `""` |  |
| `flags` | `list<string>` | `[]` |  |
| `meta` | `json` | `{}` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `source_actor` | `string` |  |
| `flags` | `list<string>` |  |
| `meta` | `json` |  |

### `flowgraph.ingress.file_watch`

**File Watch Ingress** — ファイルまたはディレクトリ更新を polling bridge から受け取る ingress。

**Metadata:** contract: inputs=6 / outputs=6 / properties=4 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: `file_watch`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__path__` | `string` | `""` |  |
| `__event_kind__` | `string` | `""` |  |
| `__source_actor__` | `string` | `""` |  |
| `__source_kind__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `path` | `string` |  |
| `event_kind` | `string` |  |
| `source_actor` | `string` |  |
| `source_kind` | `string` |  |
| `meta` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `path` | `string` | `""` |  | 監視対象ファイルまたはディレクトリ。空なら bridge は起動しない。 |
| `recursive` | `bool` | `false` |  | ディレクトリ監視時にサブディレクトリも走査する。 |
| `poll_ms` | `int` | `500` |  | polling 間隔。100ms 未満は 100ms に丸める。 |
| `fixed_channel` | `string` | `"file_watch"` |  | ingress が echo で出す `source_kind`。空なら `file_watch`。 |

### `flowgraph.ingress.osc_udp`

**OSC UDP Ingress** — 汎用 OSC（UDP データグラム）を受信し、ingress echo で下流へ流す。`content` は Base64、`content_bytes` は bytes。`__meta__.profile` は `osc_udp`。

**Metadata:** contract: inputs=7 / outputs=7 / properties=2 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__source_actor__` | `string` | `""` |  |
| `__source_kind__` | `string` | `""` |  |
| `__trace_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |
| `__content_bytes__` | `bytes` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `source_actor` | `string` |  |
| `source_kind` | `string` |  |
| `trace_id` | `string` |  |
| `meta` | `json` |  |
| `content_bytes` | `bytes` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `bind` | `string` | `""` |  | 受信 UDP の "host:port"。空のときブリッジは起動しない。 |
| `fixed_channel` | `string` | `""` |  | 空なら `source_kind` は `osc_udp`。任意のラベルに上書き可能。 |

### `flowgraph.ingress.twitch`

**Twitch Ingress** — Twitch チャット / EventSub 経由の入力

**Metadata:** contract: inputs=6 / outputs=7 / properties=9 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__source_actor__` | `string` | `""` |  |
| `__source_kind__` | `string` | `""` |  |
| `__trace_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `source_actor` | `string` |  |
| `source_kind` | `string` |  |
| `trace_id` | `string` |  |
| `meta` | `json` |  |
| `message_text` | `string` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `label` | `string` | `""` |  | GUIへ表示するTwitch入力名。空ならFlowgraphノードIDを表示する。 |
| `mode` | `string` | `"irc"` |  | "irc"（チャット受信）か "eventsub"（Helix/EventSub）。 |
| `channels` | `list<string>` | `[]` |  | IRC 購読するチャンネル名（`#` 不要）。 |
| `access_token` | `string` | `""` |  | OAuth トークン（空なら `token_key` による解決を試みる）。 |
| `token_key` | `string` | `""` |  | `conf.twitch.token_keys` で定義したキー。空なら `access_token` をそのまま使用。 |
| `client_id` | `string` | `""` |  | Twitch App の Client ID。EventSub で必須。 |
| `broadcaster_id` | `string` | `""` |  | EventSub subscribe 対象の broadcaster_user_id。 |
| `login` | `string` | `""` |  | IRC ログイン名（小文字・`#` 不要）。 |
| `fixed_channel` | `string` | `""` |  | ingress が echo で出す `source_kind`。空なら `twitch:chat` または `twitch:eventsub`。 |

### `flowgraph.ingress.twitch_eventsub`

**Twitch EventSub Ingress** — Twitch EventSub (WebSocket) の通知を受ける Flowgraph-native ingress。`token_key` で conf.twitch.token_keys の OAuth トークンを引き、`event_types` に挙げた sub_type を購読する。`payload` は生 event JSON、`meta` はサブスクリプション情報を含む補助マップ。

**Metadata:** contract: inputs=7 / outputs=7 / properties=5 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__event_type__` | `string` | `""` |  |
| `__actor_name__` | `string` | `""` |  |
| `__actor_login__` | `string` | `""` |  |
| `__broadcaster_login__` | `string` | `""` |  |
| `__payload__` | `json` | `null` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `event_type` | `string` |  |
| `actor_name` | `string` |  |
| `actor_login` | `string` |  |
| `broadcaster_login` | `string` |  |
| `payload` | `json` |  |
| `meta` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `token_key` | `string` | `"broadcaster"` |  | `conf.twitch.token_keys` で定義した OAuth key。EventSub 購読には user access token が必須。 |
| `broadcaster_login` | `string` | `""` |  | 購読対象の配信者ログイン名。空なら `conf.twitch.username`、それも無ければ `token_key` の OAuth login が使われる。 |
| `event_types` | `list<string>` | `[]` |  | 購読する sub_type リスト（例: `["channel.cheer", "channel.raid"]`）。空なら conf.twitch.eventsub の 各 bool トグル（stream.online / channel.cheer ...）をフォールバックとして使う。 |
| `channel_points_reward_id` | `string` | `""` |  | `channel.channel_points_custom_reward_redemption.add` を個別 reward に絞りたい時の UUID。空なら報酬を自動列挙してすべて購読する。 |
| `fixed_channel` | `string` | `""` |  | （予約）将来 V1 channel に押し戻すブリッジ用。今はコメントのみ。 |

### `flowgraph.ingress.unmfz`

**UNMF/Z Ingress** — UNMF/Z Zenoh topic を常時購読し、受信 frame ごとに `unmf` / `content_bytes` / `key_expr` を出力する。

**Metadata:** contract: inputs=9 / outputs=9 / properties=5 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__source_actor__` | `string` | `""` |  |
| `__source_kind__` | `string` | `""` |  |
| `__trace_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |
| `__content_bytes__` | `bytes` | `""` |  |
| `__unmf__` | `unmf` | `{"body":null,"eyes":null,"face":null,"header":{"capture_timestamp_ns":0,"coordinate_space":"Unknown","expected_dt_ns":null,"frame_timestamp_ns":0,"handedness":"Unknown","length_unit":"Unknown","magic":[85,78,77,70],"processed_timestamp_ns":0,"sequence":0,"stream_id":null,"timestamp_basis":"Unknown","version_major":1,"version_minor":1},"left_hand":null,"metadata":{"extensions":[],"notes":[],"producer":null,"schema_name":"UNMotionFrame","schema_version":"1.1.0"},"right_hand":null,"signals":[],"sources":[]}` |  |
| `__key_expr__` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `source_actor` | `string` |  |
| `source_kind` | `string` |  |
| `trace_id` | `string` |  |
| `meta` | `json` |  |
| `content_bytes` | `bytes` |  |
| `unmf` | `unmf` |  |
| `key_expr` | `string` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `base_key_expr` | `string` | `"un-motion/frame"` |  | UNMF/Z の base key。末尾 `/` は取り除かれる。 |
| `topic_mode` | `string` | `"frame"` |  | "frame" / "primary_source" / "stream_id"。 |
| `schema_major` | `int` | `1` |  | 購読する UNMF/Z schema major。key の `/vN` に使う。 |
| `poll_ms` | `int` | `10` |  | bridge が receiver を確認する間隔。1..1000ms に丸める。 |
| `fixed_channel` | `string` | `""` |  | 空なら `source_kind` は `unmfz`。任意のラベルに上書き可能。 |

### `flowgraph.ingress.vmc_udp`

**VMC UDP Ingress** — VMC 互換の生 UDP を受信し、各データグラムを ingress echo で下流へ流す。`content` は Base64 文字列、`content_bytes` は bytes。

**Metadata:** contract: inputs=7 / outputs=7 / properties=2 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__source_actor__` | `string` | `""` |  |
| `__source_kind__` | `string` | `""` |  |
| `__trace_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |
| `__content_bytes__` | `bytes` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `source_actor` | `string` |  |
| `source_kind` | `string` |  |
| `trace_id` | `string` |  |
| `meta` | `json` |  |
| `content_bytes` | `bytes` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `bind` | `string` | `""` |  | 受信 UDP の "host:port"（例 "0.0.0.0:39539"）。空のときブリッジは起動しない。 |
| `fixed_channel` | `string` | `""` |  | 空なら `source_kind` は `vmc_udp`。任意のラベルに上書き可能。 |

### `flowgraph.ingress.voice`

**Voice Ingress** — 音声認識（Vosk / Whisper）経由の入力

**Metadata:** contract: inputs=6 / outputs=6 / properties=19 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__source_actor__` | `string` | `""` |  |
| `__source_kind__` | `string` | `""` |  |
| `__trace_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `source_actor` | `string` |  |
| `source_kind` | `string` |  |
| `trace_id` | `string` |  |
| `meta` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `label` | `string` | `""` |  | GUIへ表示する音声入力名。空ならFlowgraphノードIDを表示する。 |
| `engine` | `string` | `"vosk"` | ✔ | "vosk"または"whisper"。1ノードにつき1エンジンだけを起動する。 |
| `model_path` | `string` | `"vosk-model-small-ja-0.22"` | ✔ | VoskのモデルID/ディレクトリ、またはWhisper GGMLモデルのパス。Voskの既定値は初回に取得する。 |
| `source` | `string` | `""` |  | Whisperの入力デバイス名（部分一致 / 空＝既定デバイス）。Voskは現在、既定入力を使う。 |
| `sample_rate` | `int` | `16000` |  | サンプリングレート（Hz）。vosk は通常 16000、whisper は任意。 |
| `language_code` | `string` | `"ja"` |  | 言語コード（whisper で使用）。例 "ja" / "en"。 |
| `whisper_backend` | `string` | `"auto"` |  | Whisper workerを auto / vulkan / cpu から選択。autoはVulkan、CPUの順に試す。 |
| `whisper_gpu_device` | `string` | `""` |  | VulkanのGPU index。空なら単一dGPUを優先して自動選択する。 |
| `grammar_json` | `string` | `""` |  | vosk の constrained grammar を JSON 文字列で指定（省略可）。 |
| `fixed_channel` | `string` | `""` |  | ingress が echo で出す `source_kind`。空なら `voice`。 |
| `partial_channel` | `string` | `""` |  | 途中認識をquiet datumとして更新する字幕チャンネル。空なら途中字幕を保存しない。 |
| `partial_interval_ms` | `int` | `800` |  | Whisper発話中に途中認識を更新する間隔（ms）。0でWhisper途中認識を無効化。 |
| `partial_min_audio_ms` | `int` | `1300` |  | Whisper途中認識へ渡す最小音声長（ms）。短すぎるprefixによる遅延と誤認識を抑える。 |
| `speech_floor_key` | `string` | `""` |  | 発話開始からfinal後の猶予まで占有する発話権キー。AIのrespect_speech_floorと対応させる。 |
| `speech_floor_release_delay_ms` | `int` | `700` |  | final 後も発話権を保持する時間（ms）。続く文を別の AI 応答へ分割しにくくする。 |
| `vad_threshold_db` | `int` | `-45` |  | Whisperが発話とみなすRMS閾値（dBFS）。 |
| `vad_endpoint_ms` | `int` | `600` |  | Whisperが発話終了とみなす連続無音時間（ms）。 |
| `vad_pre_roll_ms` | `int` | `400` |  | 発話冒頭を欠落させないため Whisper に含める先行音声（ms）。 |
| `vad_min_speech_ms` | `int` | `100` |  | ノイズを発話開始と誤認しないために必要な連続音声時間（ms）。 |

### `flowgraph.ingress.web_input`

**Web Input Ingress** — HTTP エンドポイント経由の入力（V1 の /input/* 相当）

**Metadata:** contract: inputs=6 / outputs=6 / properties=4 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: control; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__source_actor__` | `string` | `""` |  |
| `__source_kind__` | `string` | `""` |  |
| `__trace_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `source_actor` | `string` |  |
| `source_kind` | `string` |  |
| `trace_id` | `string` |  |
| `meta` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `path` | `string` | `""` |  | HTTP パス（例 "/input/chat"）。空なら `/input/flowgraph/<node_id>` を自動採番。 |
| `method` | `string` | `"POST"` |  | HTTP メソッド。"POST"/"GET"/"PUT" 等（大文字小文字不問）。 |
| `body_format` | `string` | `"plain"` |  | リクエストボディの解釈。"plain"（本文をそのまま content）/"json"（`content` フィールド）/"form"（`content` パラメータ）/"toml"・"msgpack"（`content` フィールド）。 |
| `fixed_channel` | `string` | `""` |  | ingress が echo で出す `source_kind`（V1 の channel 相当）。空なら `web_input`。 |

## json

### `flowgraph.json.get`

**JSON Get** — ドットパス（foo.bar[0].baz 等）で値を抜き出す。存在しない場合は Null

**Metadata:** contract: inputs=2 / outputs=6 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `json` | — |  |
| `path` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |
| `exists` | `bool` |  |
| `string` | `string` |  |
| `int` | `int` |  |
| `float` | `float` |  |
| `bool` | `bool` |  |

### `flowgraph.json.parse`

**JSON Parse** — String を JSON にパース（失敗時はエラー halt）

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `text` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `value` | `json` |  |

### `flowgraph.json.set_string`

**JSON Set String** — JSON objectの指定キーへ文字列を設定。キーはパスではなく文字通り扱う。元の他の項目は保持。

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `object` | `json` | `{}` |  |
| `key` | `string` | — |  |
| `value` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.json.stringify`

**JSON Stringify** — Json → String（pretty=true で整形出力）

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `json` | — |  |
| `pretty` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `text` | `string` |  |

### `flowgraph.json.try_parse`

**JSON Try Parse** — String を JSON にパースし、失敗を result<json> として返す。

**Metadata:** contract: inputs=1 / outputs=4 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `text` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `ok` | `bool` |  |
| `value` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## list

### `flowgraph.list.aggregate_numbers`

**List Aggregate Numbers** — List<Json> の数値を count/sum/min/max/avg へ集約。key 空なら item 全体。

**Metadata:** contract: inputs=2 / outputs=5 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |
| `key` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `count` | `int` |  |
| `sum` | `float` |  |
| `min` | `float` |  |
| `max` | `float` |  |
| `avg` | `float` |  |

### `flowgraph.list.binary_search_json`

**List Binary Search JSON** — Binary search in an ascending List<Json>. key 空なら item 全体。

**Metadata:** contract: inputs=3 / outputs=3 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |
| `target` | `json` | — |  |
| `key` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `found` | `bool` |  |
| `index` | `int` |  |
| `item` | `json` |  |

### `flowgraph.list.filter_json_eq`

**List Filter JSON Eq** — List<Json> を key の値または item 全体の JSON 等価で絞る。key 空なら item 全体。

**Metadata:** contract: inputs=3 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |
| `key` | `string` | `""` |  |
| `equals` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<json>` |  |
| `count` | `int` |  |

### `flowgraph.list.get`

**List Get** — index が範囲外なら Json::Null

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |
| `index` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `item` | `json` |  |

### `flowgraph.list.is_empty`

**List Is Empty**

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.list.len`

**List Length**

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |

| Output | Type | Note |
|---|---|---|
| `len` | `int` |  |

### `flowgraph.list.map_json_get`

**List Map JSON Get** — List<Json> の各 object から key を取り出す。欠損は null。

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |
| `key` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<json>` |  |

### `flowgraph.list.range`

**List Range** — 半開区間 [start, end) の Int list を作る。step=0 は空、最大 10000 要素。

**Metadata:** contract: inputs=3 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `start` | `int` | `0` |  |
| `end` | `int` | — |  |
| `step` | `int` | `1` |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<int>` |  |
| `count` | `int` |  |

### `flowgraph.list.slice`

**List Slice** — List<Json> から start/count で部分列を取り出す。count < 0 は末尾まで。

**Metadata:** contract: inputs=3 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |
| `start` | `int` | `0` |  |
| `count` | `int` | `-1` |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<json>` |  |
| `count` | `int` |  |

### `flowgraph.list.sort_json`

**List Sort JSON** — Stable sort for List<Json>. key 空なら item 全体、descending=true で降順。

**Metadata:** contract: inputs=3 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |
| `key` | `string` | `""` |  |
| `descending` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<json>` |  |
| `count` | `int` |  |

### `flowgraph.list.string_get`

**String List Get** — 文字列リストの指定位置を取得。範囲外は空文字とfound=false。

**Metadata:** contract: inputs=2 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<string>` | — |  |
| `index` | `int` | `0` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `string` |  |
| `found` | `bool` |  |

### `flowgraph.list.top_k_json`

**List Top-K JSON** — Sort List<Json> by key and return the first k items. descending=true returns largest values.

**Metadata:** contract: inputs=4 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |
| `key` | `string` | `""` |  |
| `k` | `int` | `10` |  |
| `descending` | `bool` | `true` |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<json>` |  |
| `count` | `int` |  |

### `flowgraph.list.window`

**List Window** — List<Json> から sliding window を作る。size/step は 1 以上。

**Metadata:** contract: inputs=3 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `items` | `list<json>` | — |  |
| `size` | `int` | `2` |  |
| `step` | `int` | `1` |  |

| Output | Type | Note |
|---|---|---|
| `windows` | `list<list<json>>` |  |
| `count` | `int` |  |

## literal

### `flowgraph.literal.bool`

**Bool Literal** — BoolLiteralNode literal

**Metadata:** contract: inputs=0 / outputs=1 / properties=1 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `value` | `bool` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `value` | `bool` | `false` |  |  |

### `flowgraph.literal.float`

**Float Literal** — FloatLiteralNode literal

**Metadata:** contract: inputs=0 / outputs=1 / properties=1 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `value` | `float` | `0.0` |  |  |

### `flowgraph.literal.int`

**Int Literal** — IntLiteralNode literal

**Metadata:** contract: inputs=0 / outputs=1 / properties=1 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `value` | `int` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `value` | `int` | `0` |  |  |

### `flowgraph.literal.json`

**JSON Literal** — JsonLiteralNode literal

**Metadata:** contract: inputs=0 / outputs=1 / properties=1 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `value` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `value` | `json` | `null` |  |  |

### `flowgraph.literal.string`

**String Literal** — StringLiteralNode literal

**Metadata:** contract: inputs=0 / outputs=1 / properties=1 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `value` | `string` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `value` | `string` | `""` |  |  |

### `flowgraph.literal.strings`

**String List Literal** — StringListLiteralNode literal

**Metadata:** contract: inputs=0 / outputs=1 / properties=1 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `value` | `list<string>` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `value` | `list<string>` | `[]` |  |  |

## logic

### `flowgraph.logic.and`

**And**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `bool` | — |  |
| `b` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.logic.not`

**Not**

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.logic.or`

**Or**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `bool` | — |  |
| `b` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.logic.xor`

**Xor**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `bool` | — |  |
| `b` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

## map

### `flowgraph.map.get`

**Map Get** — キーが存在しない場合は Json::Null

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `m` | `map<json>` | — |  |
| `key` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `value` | `json` |  |

### `flowgraph.map.has`

**Map Has**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `m` | `map<json>` | — |  |
| `key` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.map.keys`

**Map Keys**

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `m` | `map<json>` | — |  |

| Output | Type | Note |
|---|---|---|
| `keys` | `list<string>` |  |

## math

### `flowgraph.math.abs_float`

**Float abs** — Absolute value. Unit is preserved (abs(-5 m) = 5 m). NaN input yields NaN output.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.abs_int`

**Int abs** — Integer absolute value. i64::MIN overflow is handled via wrapping_abs (returns i64::MIN).

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.acos`

**Float acos** — Inverse cosine. Input dimensionless. Output Angle (rad). |x| > 1 yields NaN.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.acosh`

**Float acosh** — Inverse hyperbolic cosine. Input must be dimensionless. x < 1 yields NaN (std::f64).

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.asin`

**Float asin** — Inverse sine. Input dimensionless. Output Angle (rad). |x| > 1 yields NaN (std::f64).

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.asinh`

**Float asinh** — Inverse hyperbolic sine. Input must be dimensionless. Defined for all real x.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.atan`

**Float atan** — Inverse tangent. Input dimensionless. Output Angle (rad) in (-π/2, π/2).

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.atan2`

**Float atan2** — Two-argument arctangent. y and x must share a dimension (so their ratio is dimensionless). Output is Angle (rad) in (-π, π].

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `y` | `quantity` | — |  |
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.atanh`

**Float atanh** — Inverse hyperbolic tangent. Input must be dimensionless. |x| ≥ 1 yields ±inf/NaN (std::f64).

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.ceil`

**Float ceil** — Smallest integer ≥ x (f64::ceil). Unit is preserved.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.clamp_float`

**Float clamp** — Clamp value to [lo, hi] on Quantity. All three inputs must share a dimension. If lo > hi after unit-normalizing into value's unit, they are swapped. Result unit follows the value input.

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `quantity` | — |  |
| `lo` | `quantity` | — |  |
| `hi` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.clamp_int`

**Int clamp** — Clamp value to [lo, hi]. If lo > hi, they are swapped before clamping.

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `int` | — |  |
| `lo` | `int` | — |  |
| `hi` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.cos`

**Float cos** — Cosine. Input is Angle (rad/deg) or dimensionless (treated as radians). Output is dimensionless.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.cosh`

**Float cosh** — Hyperbolic cosine. Input must be dimensionless.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.deg_to_rad`

**Float deg → rad** — Convert degrees to radians. Angle-dimensioned input is converted via flowgraph.unit.convert semantics. Dimensionless input is scaled by π/180 and tagged with rad unit. Other dimensions are rejected.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.exp`

**Float exp** — e^x. Input must be dimensionless.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.float_add`

**Float +** — Quantity addition. Dimension mismatch is an error. ΔK + K(abs) is allowed; K + K is rejected.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `quantity` | — |  |
| `b` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.float_div`

**Float /** — Quantity division. Dimensions are composed (m / s = m·s⁻¹). Absolute-temperature division and division by zero are rejected.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `quantity` | — |  |
| `b` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.float_mul`

**Float *** — Quantity multiplication. Dimensions are composed (m * s = m·s). Absolute-temperature multiplication is rejected.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `quantity` | — |  |
| `b` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.float_sub`

**Float -** — Quantity subtraction. Dimension mismatch is an error. K - K yields ΔK.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `quantity` | — |  |
| `b` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.floor`

**Float floor** — Largest integer ≤ x (f64::floor). Unit is preserved.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.int_add`

**Int +**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.int_div`

**Int /**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.int_max`

**Int max**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.int_min`

**Int min**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.int_mod`

**Int %**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.int_mul`

**Int ***

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.int_sub`

**Int -**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `int` | — |  |
| `b` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.inverse_lerp`

**Float inverse_lerp** — Inverse of lerp: (v - a) / (b - a). All three inputs share a dimension. If a == b (after unit normalization), returns 0.0. Result is dimensionless.

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `quantity` | — |  |
| `b` | `quantity` | — |  |
| `v` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.lerp`

**Float lerp** — Linear interpolation: a + (b - a) * t. a and b share a dimension; t is dimensionless. t is not clamped (extrapolation allowed). Result unit follows a.

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `quantity` | — |  |
| `b` | `quantity` | — |  |
| `t` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.ln`

**Float ln** — Natural logarithm. Input must be dimensionless. x ≤ 0 yields -inf/NaN per std::f64.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.log10`

**Float log10** — Base-10 logarithm. Input must be dimensionless.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.log2`

**Float log2** — Base-2 logarithm. Input must be dimensionless.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.max_float`

**Float max** — Maximum of two same-dimension Quantity values. Result keeps A's unit. NaN follows f64::max semantics.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `quantity` | — |  |
| `b` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.min_float`

**Float min** — Minimum of two same-dimension Quantity values. Result keeps A's unit. NaN follows f64::min semantics (NaN propagates to other operand).

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `quantity` | — |  |
| `b` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.normalize_angle_deg_0_360`

**Normalize angle [0, 360) deg** — Normalize Angle into [0, 360) degrees. 1357.33 → 277.33. Angle-dim input is converted to deg first; dimensionless is treated as deg.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.normalize_angle_deg_signed`

**Normalize angle [-180, 180) deg** — Normalize Angle into [-180, +180) degrees. 277.33 → -82.67. Angle-dim input is converted to deg first; dimensionless is treated as deg.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.normalize_angle_rad_0_2pi`

**Normalize angle [0, 2π) rad** — Normalize Angle into [0, 2π) radians. Angle-dim input is converted to rad first; dimensionless is treated as rad.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.normalize_angle_rad_signed`

**Normalize angle [-π, π) rad** — Normalize Angle into [-π, +π) radians. Angle-dim input is converted to rad first; dimensionless is treated as rad.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.pow`

**Float pow** — base^exp. Both base and exp must be dimensionless (general Quantity pow requires an integer exponent for dimension algebra; for that use flowgraph.unit.* + custom). NaN and ±inf follow f64::powf semantics.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `base` | `quantity` | — |  |
| `exp` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.rad_to_deg`

**Float rad → deg** — Convert radians to degrees. Angle-dimensioned input is converted via flowgraph.unit.convert semantics. Dimensionless input is scaled by 180/π and tagged with deg unit. Other dimensions are rejected.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.remap`

**Float remap** — Remap value from [in_lo, in_hi] to [out_lo, out_hi]. value/in_lo/in_hi share a dimension; out_lo/out_hi share another dimension. If in_lo == in_hi, out_lo is returned. Result unit follows out_lo.

**Metadata:** contract: inputs=5 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `quantity` | — |  |
| `in_lo` | `quantity` | — |  |
| `in_hi` | `quantity` | — |  |
| `out_lo` | `quantity` | — |  |
| `out_hi` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.round`

**Float round** — Round half away from zero (f64::round std default). Unit is preserved.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.sign_float`

**Float sign** — Sign classifier: -1 / 0 / +1 (value only, unit preserved). NaN yields 0. Note: the SI meaning of a unit-bearing sign is unusual but kept for pass-through consistency.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.sign_int`

**Int sign** — Integer sign: -1 for x<0, 0 for x==0, +1 for x>0.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `int` |  |

### `flowgraph.math.sin`

**Float sin** — Sine. Input is Angle (rad/deg) or dimensionless (treated as radians for pre-ξ compatibility). Output is dimensionless.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.sinh`

**Float sinh** — Hyperbolic sine. Input must be dimensionless (hyperbolic functions take unitless arguments in their SI-compatible form).

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.smoothstep`

**Float smoothstep** — GLSL smoothstep: t = clamp((x - edge0) / (edge1 - edge0), 0, 1); returns t*t*(3 - 2*t). All three inputs share a dimension. If edge0 == edge1, returns 0.0. Result is dimensionless in [0, 1].

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `edge0` | `quantity` | — |  |
| `edge1` | `quantity` | — |  |
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.sqrt`

**Float sqrt** — Square root. Dimension-aware: sqrt(m²) = m, sqrt(m²/s²) = m/s. All atom exponents must be even (the current type system only represents integer dimensions), so sqrt(m) is rejected — use flowgraph.unit.strip first if that was intentional. Absolute temperature (K) is rejected. Negative value yields NaN.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.tan`

**Float tan** — Tangent. Input is Angle (rad/deg) or dimensionless (treated as radians). Output is dimensionless. ±(π/2) yields large finite values per std::f64.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

### `flowgraph.math.tanh`

**Float tanh** — Hyperbolic tangent. Input must be dimensionless.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

## mode

### `flowgraph.mode.equals`

**Mode Equals** — 実効 Runtime Mode が expected と一致するか（前後空白は無視）。未上書き時は default_runtime_mode 相当と比較。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `expected` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `equals` | `bool` |  |

### `flowgraph.mode.get`

**Mode Get** — 現在の実効 Runtime Mode ID。Control API で上書きした値。未上書き時は conf.default_runtime_mode に従う（空のことあり）。

**Metadata:** contract: inputs=0 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Output | Type | Note |
|---|---|---|
| `mode` | `string` |  |

### `flowgraph.mode.transit`

**Mode Transit** — 指定 mode へ Runtime Mode を切り替える（`State.runtime_mode_id` + TriggerGate 再計算）。`mode` が空なら default_runtime_mode 相当。`State` 未接続・conf 再読込失敗・未知 mode では on_reject。

**Metadata:** contract: inputs=3 / outputs=4 / properties=1 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `mode` | `string` | `""` |  |
| `reason` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_reject` | `exec` (out) |  |
| `accepted` | `bool` |  |
| `message` | `string` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `noop_message` | `string` | `"noop"` |  | 実効 mode が変わらなかったときの `message` 文字列。 |

## motion

### `flowgraph.motion.filter`

**Motion: Filter OSC Messages** — `osc_messages` を `address_prefix` と `address_substring`（両方省略可）でフィルタする。入出力は `motion_frame`（`json` へ coerce 可）

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `frame` | `motion_frame` | — |  |
| `address_prefix` | `string` | `""` |  |
| `address_substring` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `frame_out` | `motion_frame` |  |

### `flowgraph.motion.map`

**Motion: Map Numeric Args** — 各 `osc_messages[].args` の JSON 数値を再帰的に `float_scale` 倍する。入出力は `motion_frame`（`json` へ coerce 可）

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `frame` | `motion_frame` | — |  |
| `float_scale` | `float` | `1.0` |  |

| Output | Type | Note |
|---|---|---|
| `frame_out` | `motion_frame` |  |

### `flowgraph.motion.vmc_parse`

**Motion: VMC OSC Parse** — UDP ペイロードを OSC として解釈し、[`MotionFrame`]（`motion_frame`）を出力。`payload` bytes が非空なら優先し、空なら互換用 `payload_b64` を読む。`json` へ接続時は自動変換

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `payload` | `bytes` | `""` |  |
| `payload_b64` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `frame` | `motion_frame` |  |

## noise

### `flowgraph.noise.perlin_1d`

**Perlin 1D** — 1D Perlin noise at coordinate `t` with integer `seed` (cached Perlin per seed).

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `t` | `float` | — |  |
| `seed` | `int` | `0` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

### `flowgraph.noise.perlin_2d`

**Perlin 2D** — 2D Perlin noise at `(x, y)` with integer `seed`.

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `float` | — |  |
| `y` | `float` | — |  |
| `seed` | `int` | `0` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

## notify

### `flowgraph.notify.desktop`

**Notify: Desktop** — OS の desktop notification / Toast を表示する。Runtime Mode の通知 policy と接続する出口。

**Metadata:** contract: inputs=8 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `desktop_notification`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `title` | `string` | — |  |
| `body` | `string` | `""` |  |
| `urgency` | `string` | `"normal"` |  |
| `group` | `string` | `""` |  |
| `icon` | `string` | `""` |  |
| `timeout_ms` | `int` | `5000` |  |
| `dry_run` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `shown` | `bool` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## obs

### `flowgraph.obs.get_current_program_scene`

**OBS: Get Current Program Scene** — OBS WebSocket v5 の `GetCurrentProgramScene` を呼び、現在の番組シーン名を返す。

**Metadata:** contract: inputs=4 / outputs=8 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `timeout_ms` | `int` | `3000` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |
| `scene_name` | `string` |  |

### `flowgraph.obs.get_source_screenshot`

**OBS: Get Explicit Source Screenshot** — 明示したscene/sourceの所属を確認してから静止画を取得する。Program出力やデスクトップ全体は推定しない。

**Metadata:** contract: inputs=9 / outputs=9 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `timeout_ms` | `int` | `3000` |  |
| `scene_name` | `string` | — |  |
| `source_name` | `string` | — |  |
| `image_format` | `string` | `"png"` |  |
| `width` | `int` | `1280` |  |
| `height` | `int` | `720` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |
| `image_data_url` | `string` |  |
| `capture_metadata` | `json` |  |

### `flowgraph.obs.request`

**OBS: Request** — OBS WebSocket v5 に 1 request を送る汎用ノード。OBS 側で WebSocket Server を有効化しておく。

**Metadata:** contract: inputs=6 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `request_type` | `string` | — |  |
| `request_data` | `json` | `{}` |  |
| `timeout_ms` | `int` | `3000` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.obs.set_current_program_scene`

**OBS: Set Current Program Scene** — OBS WebSocket v5 の `SetCurrentProgramScene` を呼び、現在の番組シーンを切り替える。

**Metadata:** contract: inputs=5 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `scene_name` | `string` | — |  |
| `timeout_ms` | `int` | `3000` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.obs.set_scene_item_enabled`

**OBS: Set Scene Item Enabled** — OBS WebSocket v5 の `SetSceneItemEnabled` を呼び、scene item の表示/非表示を切り替える。`scene_item_id` が負なら `source_name` から ID を解決する。

**Metadata:** contract: inputs=8 / outputs=8 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `timeout_ms` | `int` | `3000` |  |
| `scene_name` | `string` | — |  |
| `source_name` | `string` | `""` |  |
| `scene_item_id` | `int` | `-1` |  |
| `enabled` | `bool` | `true` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |
| `scene_item_id` | `int` |  |

### `flowgraph.obs.start_record`

**OBS: Start Record** — OBS WebSocket v5 の `StartRecord` を呼び、録画を開始する。

**Metadata:** contract: inputs=4 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `timeout_ms` | `int` | `3000` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.obs.start_stream`

**OBS: Start Stream** — OBS WebSocket v5 の `StartStream` を呼び、配信を開始する。

**Metadata:** contract: inputs=4 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `timeout_ms` | `int` | `3000` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.obs.stop_record`

**OBS: Stop Record** — OBS WebSocket v5 の `StopRecord` を呼び、録画を停止する。

**Metadata:** contract: inputs=4 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `timeout_ms` | `int` | `3000` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.obs.stop_stream`

**OBS: Stop Stream** — OBS WebSocket v5 の `StopStream` を呼び、配信を停止する。

**Metadata:** contract: inputs=4 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `timeout_ms` | `int` | `3000` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.obs.trigger_studio_mode_transition`

**OBS: Trigger Studio Mode Transition** — OBS WebSocket v5 の `TriggerStudioModeTransition` を呼び、Studio Mode の transition を実行する。

**Metadata:** contract: inputs=4 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `obs_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | `"ws://127.0.0.1:4455"` |  |
| `password` | `string` | `""` |  |
| `timeout_ms` | `int` | `3000` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `ok` | `bool` |  |
| `status_code` | `int` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## ocr

### `flowgraph.ocr.recognize`

**OCR Recognize** — 画像ソースから文字列を OCR 抽出する（Windows のみ）

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `desktop_capture`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `source` | `string` | — |  |
| `lang` | `string` | — |  |
| `lines` | `bool` | `false` |  |
| `check_result_lang` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `text` | `string` |  |
| `error` | `string` |  |
| `result` | `result<string>` |  |

## osc

### `flowgraph.osc.send`

**OSC: UDP Send** — 単一 OSC メッセージを UDP で送信する。args は JSON 配列（数値・文字列・真偽・null・ネスト配列）

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `host` | `string` | — |  |
| `port` | `int` | — |  |
| `path` | `string` | — |  |
| `args` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_sent` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

## output

### `flowgraph.bos.output`

**BOS Output** — State.channel_data に ChannelDatum を push。WS クライアント / browser-output に届く終端ノード。`content` / `destination` / `source_actor` は `String`。`Quantity` を配線した場合は engine 側で`"{value} {unit}"` 形式に自動文字列化される。単位を含めたくない場合は手前で`flowgraph.util.format` (include_unit=false) か `flowgraph.unit.strip` を挟む。

**Metadata:** contract: inputs=7 / outputs=3 / properties=2 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `destination` | `string` | `""` |  |
| `content` | `string` | `""` |  |
| `is_final` | `bool` | `true` |  |
| `source_actor` | `string` | `""` |  |
| `flags` | `json` | `[]` |  |
| `meta` | `json` | `{}` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `result` | `result<bool>` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `default_destination` | `string` | `""` |  | `destination`入力が空のときに使うBOS出力先。両方空ならon_error発火。 |
| `auto_tag_source_actor` | `bool` | `true` |  | `source_actor`入力が空のとき、自動で`flowgraph:<node_id>`をstampする。falseで無効化。 |

## physics

### `flowgraph.physics.damper`

**Physics Damper** — Pure scalar exponential damper from current toward target.

**Metadata:** contract: inputs=4 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `current` | `float` | `0.0` |  |
| `target` | `float` | `0.0` |  |
| `dt` | `float` | `0.016666666666666666` |  |
| `damping` | `float` | `10.0` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

### `flowgraph.physics.gravity`

**Physics Gravity** — Pure scalar velocity integration with constant acceleration.

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `velocity` | `float` | `0.0` |  |
| `gravity` | `float` | `-9.80665` |  |
| `dt` | `float` | `0.016666666666666666` |  |

| Output | Type | Note |
|---|---|---|
| `velocity` | `float` |  |

### `flowgraph.physics.integrator`

**Physics Integrator** — Stateful scalar integrator. `step` adds velocity * dt to value.

**Metadata:** contract: inputs=5 / outputs=4 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `step` | `exec` (in) | — |  |
| `reset` | `exec` (in) | — |  |
| `initial` | `float` | `0.0` |  |
| `velocity` | `float` | `0.0` |  |
| `dt` | `float` | `0.016666666666666666` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |
| `initialized` | `bool` |  |
| `updated` | `exec` (out) |  |
| `reset_done` | `exec` (out) |  |

### `flowgraph.physics.spring`

**Physics Spring** — Stateful scalar spring integrator. `step` advances value toward target.

**Metadata:** contract: inputs=7 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `step` | `exec` (in) | — |  |
| `reset` | `exec` (in) | — |  |
| `target` | `float` | `0.0` |  |
| `initial` | `float` | `0.0` |  |
| `dt` | `float` | `0.016666666666666666` |  |
| `stiffness` | `float` | `120.0` |  |
| `damping` | `float` | `20.0` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |
| `velocity` | `float` |  |
| `initialized` | `bool` |  |
| `updated` | `exec` (out) |  |
| `reset_done` | `exec` (out) |  |

## process

### `flowgraph.process.kill`

**Process: Kill** — pid または name_filter に一致する process を終了する。

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `process_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `pid` | `int` | `0` |  |
| `name_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |
| `force` | `bool` | `true` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `killed_count` | `int` |  |
| `pids` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.process.running`

**Process: Running** — pid または name_filter で process の生存状態を確認する。

**Metadata:** contract: inputs=4 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `process_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `pid` | `int` | `0` |  |
| `name_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `running` | `bool` |  |
| `count` | `int` |  |
| `pids` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.process.spawn`

**Process: Spawn** — 外部 process を起動する。shell は介さず command + args を直接実行する。

**Metadata:** contract: inputs=7 / outputs=8 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `process_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `command` | `string` | — |  |
| `args` | `list<string>` | `[]` |  |
| `working_dir` | `string` | `""` |  |
| `env` | `json` | `{}` |  |
| `wait` | `bool` | `false` |  |
| `timeout_ms` | `int` | `0` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `pid` | `int` |  |
| `exit_code` | `int` |  |
| `stdout` | `string` |  |
| `stderr` | `string` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.process.wait`

**Process: Wait** — 指定 pid が終了するまで polling で待つ。exit_code は未取得時 -1。

**Metadata:** contract: inputs=4 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `process_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `pid` | `int` | — |  |
| `timeout_ms` | `int` | `30000` |  |
| `poll_interval_ms` | `int` | `250` |  |

| Output | Type | Note |
|---|---|---|
| `on_exit` | `exec` (out) |  |
| `on_timeout` | `exec` (out) |  |
| `exited` | `bool` |  |
| `exit_code` | `int` |  |
| `error` | `string` |  |
| `result` | `result<bool>` |  |

## random

### `flowgraph.dist.categorical`

**Distribution categorical** — Seeded categorical sample from non-negative weights. Optional values maps index to JSON value.

**Metadata:** contract: inputs=5 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `seed` | `string` | `""` |  |
| `algorithm` | `string` | `"splitmix64"` |  |
| `offset` | `int` | `0` |  |
| `weights` | `list<json>` | — |  |
| `values` | `list<json>` | `[]` |  |

| Output | Type | Note |
|---|---|---|
| `index` | `int` |  |
| `value` | `json` |  |

### `flowgraph.dist.exponential`

**Distribution exponential** — Seeded exponential distribution sample. lambda must be > 0.

**Metadata:** contract: inputs=4 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `seed` | `string` | `""` |  |
| `algorithm` | `string` | `"splitmix64"` |  |
| `offset` | `int` | `0` |  |
| `lambda` | `float` | `1.0` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

### `flowgraph.dist.normal`

**Distribution normal** — Seeded normal distribution sample using Box-Muller.

**Metadata:** contract: inputs=5 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `seed` | `string` | `""` |  |
| `algorithm` | `string` | `"splitmix64"` |  |
| `offset` | `int` | `0` |  |
| `mean` | `float` | `0.0` |  |
| `stddev` | `float` | `1.0` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

### `flowgraph.dist.poisson`

**Distribution poisson** — Seeded Poisson distribution sample. Knuth method, lambda <= 1000.

**Metadata:** contract: inputs=4 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `seed` | `string` | `""` |  |
| `algorithm` | `string` | `"splitmix64"` |  |
| `offset` | `int` | `0` |  |
| `lambda` | `float` | `1.0` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `int` |  |

### `flowgraph.prng.sequence`

**PRNG Sequence** — Seeded deterministic PRNG sequence. algorithms: splitmix64, lcg64, pcg32, xoshiro256ss.

**Metadata:** contract: inputs=4 / outputs=3 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `seed` | `string` | `""` |  |
| `algorithm` | `string` | `"splitmix64"` |  |
| `count` | `int` | `1` |  |
| `offset` | `int` | `0` |  |

| Output | Type | Note |
|---|---|---|
| `hex` | `list<string>` |  |
| `unit` | `list<float>` |  |
| `count` | `int` |  |

### `flowgraph.prng.uniform_float`

**PRNG uniform (float)** — Seeded deterministic uniform float in [lo, hi).

**Metadata:** contract: inputs=5 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `seed` | `string` | `""` |  |
| `algorithm` | `string` | `"splitmix64"` |  |
| `offset` | `int` | `0` |  |
| `lo` | `float` | — |  |
| `hi` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

### `flowgraph.prng.uniform_int`

**PRNG uniform (int)** — Seeded deterministic uniform integer in [lo, hi] inclusive.

**Metadata:** contract: inputs=5 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `seed` | `string` | `""` |  |
| `algorithm` | `string` | `"splitmix64"` |  |
| `offset` | `int` | `0` |  |
| `lo` | `int` | — |  |
| `hi` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `value` | `int` |  |

### `flowgraph.random.normal`

**Random normal** — Gaussian sample (Box–Muller) with given mean and stddev. stddev must be non-negative; 0 yields mean.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `mean` | `float` | — |  |
| `stddev` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

### `flowgraph.random.uniform_float`

**Random uniform (float)** — Uniform random float in [lo, hi) half-open interval.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `lo` | `float` | — |  |
| `hi` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `value` | `float` |  |

### `flowgraph.random.uniform_int`

**Random uniform (int)** — Uniform random integer in [lo, hi] inclusive.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `lo` | `int` | — |  |
| `hi` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `value` | `int` |  |

## regex

### `flowgraph.regex.replace`

**Regex Replace** — 正規表現ルール（{pattern, replacement}）で content を逐次置換する

**Metadata:** contract: inputs=2 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `content` | `string` | — |  |
| `rules` | `list<json>` | `[]` |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |
| `errors` | `list<string>` |  |

## screenshot

### `flowgraph.screenshot.capture`

**Screenshot Capture** — ウィンドウ/デスクトップのスクリーンショットを PNG data URL として取得

**Metadata:** contract: inputs=7 / outputs=8 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `desktop_capture`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `target_title` | `string` | `""` |  |
| `target_title_regex` | `string` | `""` |  |
| `client_only` | `bool` | `false` |  |
| `use_bitblt` | `bool` | `false` |  |
| `crop` | `json` | `null` |  |
| `save_path` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `data_url` | `string` |  |
| `saved_path` | `string` |  |
| `width` | `int` |  |
| `height` | `int` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## state

### `flowgraph.state.accumulator`

**Accumulator** — push で input を List に蓄積、clear で空に戻す

**Metadata:** contract: inputs=3 / outputs=4 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `push` | `exec` (in) | — |  |
| `clear` | `exec` (in) | — |  |
| `input` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `items` | `list<json>` |  |
| `count` | `int` |  |
| `updated` | `exec` (out) |  |
| `cleared` | `exec` (out) |  |

### `flowgraph.state.bool`

**Bool State** — set_true/set_false/toggle でブール値を切り替える

**Metadata:** contract: inputs=3 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `set_true` | `exec` (in) | — |  |
| `set_false` | `exec` (in) | — |  |
| `toggle` | `exec` (in) | — |  |

| Output | Type | Note |
|---|---|---|
| `value` | `bool` |  |
| `changed` | `exec` (out) |  |

### `flowgraph.state.int_counter`

**Int Counter** — step だけ増減/リセット可能な整数カウンタ

**Metadata:** contract: inputs=5 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `increment` | `exec` (in) | — |  |
| `decrement` | `exec` (in) | — |  |
| `reset` | `exec` (in) | — |  |
| `step` | `int` | `1` |  |
| `reset_value` | `int` | `0` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `int` |  |
| `changed` | `exec` (out) |  |

### `flowgraph.state.latch`

**Latch** — exec_in で input を保持。以後 value 出力は保持値を返す

**Metadata:** contract: inputs=2 / outputs=3 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `input` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `value` | `json` |  |
| `has_value` | `bool` |  |
| `updated` | `exec` (out) |  |

## string

### `flowgraph.string.concat`

**String Concat** — a と b を連結

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `string` | — |  |
| `b` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |

### `flowgraph.string.contains`

**String Contains**

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `haystack` | `string` | — |  |
| `needle` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.string.english_to_katakana`

**English to Katakana (ONNX)** — Rust製workerのenglish2kana ONNXでASCII英単語をカタカナ化。失敗時は原文を維持

**Metadata:** contract: inputs=4 / outputs=5 / properties=0 / exec_in=no / exec_out=no; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `content` | `string` | — |  |
| `model_dir` | `string` | `""` |  |
| `worker_path` | `string` | `""` |  |
| `min_confidence` | `float` | `0.55` |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |
| `converted_count` | `int` |  |
| `available` | `bool` |  |
| `error` | `string` |  |
| `trace` | `json` |  |

### `flowgraph.string.join`

**String Join** — List<String> を sep で連結

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `parts` | `list<string>` | — |  |
| `sep` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |

### `flowgraph.string.len`

**String Length** — UTF-8 文字数（chars().count()）

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `s` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `len` | `int` |  |

### `flowgraph.string.replace`

**String Replace** — pattern (リテラル) を replacement に全置換

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `s` | `string` | — |  |
| `pattern` | `string` | — |  |
| `replacement` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |

### `flowgraph.string.split`

**String Split** — sep で split して List<String> を返す

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `s` | `string` | — |  |
| `sep` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `parts` | `list<string>` |  |

## system

### `flowgraph.system.cpu_usage`

**System: CPU Usage** — sysinfo で全体 CPU 使用率と logical CPU ごとの使用率を取得する。

**Metadata:** contract: inputs=1 / outputs=4 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `usage_percent` | `float` |  |
| `core_count` | `int` |  |
| `per_cpu` | `json` |  |

### `flowgraph.system.load_avg`

**System: Load Average** — OS の load average を取得する。未対応OSでは sysinfo の値をそのまま返す。

**Metadata:** contract: inputs=1 / outputs=4 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `one` | `float` |  |
| `five` | `float` |  |
| `fifteen` | `float` |  |

### `flowgraph.system.memory`

**System: Memory** — sysinfo で RAM / swap の使用量を byte 単位で取得する。

**Metadata:** contract: inputs=1 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `mem_used` | `int` |  |
| `mem_total` | `int` |  |
| `mem_available` | `int` |  |
| `mem_usage_percent` | `float` |  |
| `swap_used` | `int` |  |
| `swap_total` | `int` |  |

### `flowgraph.system.process_list`

**System: Process List** — sysinfo で process 一覧を JSON 配列として取得する。name_filter と limit で絞り込める。

**Metadata:** contract: inputs=3 / outputs=3 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `name_filter` | `string` | `""` |  |
| `limit` | `int` | `100` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `count` | `int` |  |
| `processes` | `json` |  |

## table

### `flowgraph.table.concat`

**Table Concat** — 同じ列定義の表を先頭・末尾の順に連結。重複行と列の型を保持し、異なる列定義は拒否する。

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `first` | `table` | — |  |
| `second` | `table` | — |  |

| Output | Type | Note |
|---|---|---|
| `table` | `table` |  |

### `flowgraph.table.draw`

**Table Draw** — Table から行を抽選。重み列、seed、重複あり / なしを扱う

**Metadata:** contract: inputs=5 / outputs=3 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `table` | `table` | `[]` |  |
| `count` | `int` | `1` |  |
| `weight_column` | `string` | `""` |  |
| `seed` | `string` | `""` |  |
| `replacement` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `rows` | `list<json>` |  |
| `row` | `json` |  |
| `indices` | `list<int>` |  |

### `flowgraph.table.from_json`

**Table From JSON** — List<Json> (object の配列) を Table に変換。スキーマは先頭 object から推論

**Metadata:** contract: inputs=1 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `json` | `list<json>` | `[]` |  |

| Output | Type | Note |
|---|---|---|
| `table` | `table` |  |
| `row_count` | `int` |  |

### `flowgraph.table.load_google_sheet_auth`

**Table Load Google Sheet Auth** — Google Sheets API v4 values.get を OAuth access_token 付きで呼び、private Sheet を Table に読み込む。Effectful

**Metadata:** contract: inputs=9 / outputs=9 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `credential_access`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `spreadsheet` | `string` | — |  |
| `access_token` | `string` | `""` |  |
| `token_env` | `string` | `"UNVAC_GOOGLE_SHEETS_ACCESS_TOKEN"` |  |
| `token_file` | `string` | `""` |  |
| `sheet` | `string` | `""` |  |
| `range` | `string` | `"A:ZZ"` |  |
| `header_mode` | `string` | `"auto"` |  |
| `timeout_ms` | `int` | `8000` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `table` | `table` |  |
| `sheet_name` | `string` |  |
| `row_count` | `int` |  |
| `column_count` | `int` |  |
| `source_url` | `string` |  |
| `error` | `string` |  |
| `result` | `result<table>` |  |

### `flowgraph.table.load_google_sheet_public`

**Table Load Google Sheet Public** — 公開 Google Sheets を CSV export 経由で Table に読み込む。OAuth は使わない。Effectful

**Metadata:** contract: inputs=6 / outputs=9 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `spreadsheet` | `string` | — |  |
| `sheet` | `string` | `""` |  |
| `range` | `string` | `""` |  |
| `header_mode` | `string` | `"auto"` |  |
| `timeout_ms` | `int` | `8000` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `table` | `table` |  |
| `sheet_name` | `string` |  |
| `row_count` | `int` |  |
| `column_count` | `int` |  |
| `source_url` | `string` |  |
| `error` | `string` |  |
| `result` | `result<table>` |  |

### `flowgraph.table.load_tsv`

**Table Load TSV** — TSV ファイルを Table に読み込む（auto / google_ime / headerful / legacy_loose）。Effectful

**Metadata:** contract: inputs=3 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_read`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |
| `mode` | `string` | `"auto"` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `table` | `table` |  |
| `row_count` | `int` |  |
| `revision` | `string` |  |
| `error` | `string` |  |
| `result` | `result<table>` |  |

### `flowgraph.table.load_xlsx`

**Table Load XLSX** — XLSX ファイルの sheet / range を Table に読み込む。Effectful

**Metadata:** contract: inputs=5 / outputs=8 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_read`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `path` | `string` | — |  |
| `sheet` | `string` | `""` |  |
| `range` | `string` | `""` |  |
| `header_mode` | `string` | `"auto"` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `table` | `table` |  |
| `sheet_name` | `string` |  |
| `row_count` | `int` |  |
| `column_count` | `int` |  |
| `error` | `string` |  |
| `result` | `result<table>` |  |

### `flowgraph.table.to_json`

**Table To JSON** — Table を List<Json> (object の配列) に変換

**Metadata:** contract: inputs=1 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `table` | `table` | `[]` |  |

| Output | Type | Note |
|---|---|---|
| `json` | `list<json>` |  |
| `row_count` | `int` |  |

### `flowgraph.table.write_tsv`

**Table Write TSV** — Table を TSV ファイルに書き出す（atomic rename）。Effectful

**Metadata:** contract: inputs=4 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `file_write`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `table` | `table` | — |  |
| `path` | `string` | — |  |
| `expected_revision` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `on_conflict` | `exec` (out) |  |
| `bytes_written` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

## translate

### `flowgraph.translate.gas`

**Translate (GAS)** — Google Apps Script 経由の翻訳 API 呼び出し

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `text` | `string` | — |  |
| `translate_to` | `string` | — |  |
| `translate_from` | `string` | `""` |  |
| `script_id` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `translated` | `string` |  |
| `detected_lang` | `string` |  |
| `error` | `string` |  |
| `result` | `result<string>` |  |

### `flowgraph.translate.libre`

**Translate (LibreTranslate)** — LibreTranslate REST API (/translate) を叩く

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `text` | `string` | — |  |
| `translate_to` | `string` | — |  |
| `translate_from` | `string` | `""` |  |
| `base_url` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `translated` | `string` |  |
| `detected_lang` | `string` |  |
| `error` | `string` |  |
| `result` | `result<string>` |  |

## tts

### `flowgraph.tts.speak`

**TTS: Speak** — engine 入力で選んだ TTS ドライバに text を合成させ、VAC 共有 audio_sink で再生する。save_path 指定時は WAV も保存。

**Metadata:** contract: inputs=22 / outputs=10 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `audio_output`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `__tts_playback_result__` | `json` | `null` |  |
| `engine` | `string` | — | enum: `aivis_speech`, `bouyomichan`, `coeiroink`, `os`, `voicepeak`, `voicevox` |
| `text` | `string` | — |  |
| `voice` | `string` | `""` |  |
| `speed` | `float` | `1.0` |  |
| `pitch` | `float` | `0.0` |  |
| `volume` | `float` | `1.0` |  |
| `endpoint` | `string` | `""` |  |
| `extra` | `map<json>` | `{}` |  |
| `save_path` | `string` | `""` |  |
| `lane` | `string` | `"chat"` | enum: `assistant`, `chat`, `system` |
| `utterance_id` | `string` | `""` |  |
| `segment_index` | `int` | `0` |  |
| `non_blocking` | `bool` | `false` |  |
| `respect_speech_floor` | `string` | `""` |  |
| `max_queue_wait_ms` | `int` | `0` | 0はlane既定（chat=30000ms、assistant/system=無制限）、負数は無制限、正数は明示上限。 |
| `max_pending` | `int` | `0` | 0はlane既定（chat=20件、assistant/system=無制限）、負数は無制限、正数は明示上限。 |
| `lip_sync_base_key` | `string` | `""` |  |
| `lip_sync_expression` | `string` | `"JawOpen"` |  |
| `lip_sync_target_link_id` | `string` | `"avatar_lipsync"` |  |
| `lip_sync_gain` | `float` | `1.0` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `on_played` | `exec` (out) | played=trueの結果だけを通知。schedulerでは共有sinkへの投入時。non_blockingでも通知する。実聴取や音声終了の保証ではない。 |
| `on_playback_error` | `exec` (out) |  |
| `spoken_text` | `string` |  |
| `spoken_utterance_id` | `string` |  |
| `played` | `bool` |  |
| `audio_path` | `string` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## twitch

### `flowgraph.ingress.twitch_stream_state`

**Twitch Stream State** — Helix/EventSubで維持する宣言済み配信状態を明示edgeへ公開する。

**Metadata:** contract: inputs=6 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `__trigger__` | `exec` (in) | — |  |
| `__content__` | `string` | `""` |  |
| `__source_actor__` | `string` | `""` |  |
| `__source_kind__` | `string` | `""` |  |
| `__trace_id__` | `string` | `""` |  |
| `__meta__` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `content` | `string` |  |
| `source_actor` | `string` |  |
| `source_kind` | `string` |  |
| `trace_id` | `string` |  |
| `meta` | `json` |  |

### `flowgraph.twitch.ad_run`

**Twitch: Run Ad** — Helix POST /channels/commercial で広告を実行する。length_seconds は通常 30/60/90/120/150/180。

**Metadata:** contract: inputs=6 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `length_seconds` | `int` | `60` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.ban`

**Twitch: Ban User** — Helix POST /moderation/bans（永久 ban）。`duration` なしで送る。timeout は `twitch.timeout` ノードを使うこと

**Metadata:** contract: inputs=8 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `user_id` | `string` | — |  |
| `broadcaster_id` | `string` | — |  |
| `moderator_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `reason` | `string` | `""` |  |
| `endpoint` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `end_time` | `string` |  |
| `error` | `string` |  |
| `result` | `result<string>` |  |

### `flowgraph.twitch.channel_info_update`

**Twitch: Update Channel Info** — Helix PATCH /channels で配信タイトル、カテゴリ、言語、タグを更新する。空入力は送信しない。

**Metadata:** contract: inputs=9 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `game_id` | `string` | `""` |  |
| `title` | `string` | `""` |  |
| `broadcaster_language` | `string` | `""` |  |
| `tags` | `list<string>` | `[]` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.chat_clear`

**Twitch: Clear Chat** — Helix DELETE /moderation/chat でチャット全体または指定messageを削除する。

**Metadata:** contract: inputs=7 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `moderator_id` | `string` | — |  |
| `message_id` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.chat_send`

**Twitch: Chat Send** — Helix POST /chat/messages でチャット送信。max_chars/strip_substrings は optional で V1 互換。レート制限は flowgraph.util.rate_limit を上流に挿入して実現する

**Metadata:** contract: inputs=9 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `text` | `string` | — |  |
| `broadcaster_id` | `string` | — |  |
| `sender_user_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `max_chars` | `int` | `500` |  |
| `strip_substrings` | `list<string>` | `[]` |  |
| `endpoint` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `on_skipped` | `exec` (out) |  |
| `sent_text` | `string` |  |
| `error` | `string` |  |
| `result` | `result<string>` |  |

### `flowgraph.twitch.chat_settings_update`

**Twitch: Update Chat Settings** — Helix PATCH /chat/settings で emote/subscriber/follower/slow/unique chat などの設定を更新する。接続された入力だけ送信する。

**Metadata:** contract: inputs=15 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `moderator_id` | `string` | — |  |
| `emote_mode` | `bool` | — |  |
| `subscriber_mode` | `bool` | — |  |
| `unique_chat_mode` | `bool` | — |  |
| `follower_mode` | `bool` | — |  |
| `follower_mode_duration` | `int` | — |  |
| `slow_mode` | `bool` | — |  |
| `slow_mode_wait_time` | `int` | — |  |
| `non_moderator_chat_delay` | `bool` | — |  |
| `non_moderator_chat_delay_duration` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.clip_command`

**Twitch: Moderator Clip Command** — Twitch IRCの配信者/モデレーターによる !rc / !randomclip を受付。本文から権限を推定しない。ノード内でID重複とチャンネル毎5秒の連打を抑止。

**Metadata:** contract: inputs=2 / outputs=4 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `meta` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_command` | `exec` (out) |  |
| `on_skip` | `exec` (out) |  |
| `login` | `string` |  |
| `broadcaster_id` | `string` |  |

### `flowgraph.twitch.clip_create`

**Twitch: Create Clip** — Helix POST /clips で現在の配信からclip作成を開始する。

**Metadata:** contract: inputs=6 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `has_delay` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |
| `clip_id` | `string` |  |
| `edit_url` | `string` |  |

### `flowgraph.twitch.event_notice`

**Twitch: Event Notice** — EventSubから通知文とBOS演出を生成。既定は日本語。テンプレートの空文字で通知を無効化。重複抑止はノード存続中10分/4096件。

**Metadata:** contract: inputs=4 / outputs=7 / properties=1 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `event_type` | `string` | — |  |
| `payload` | `json` | — |  |
| `meta` | `json` | `{}` |  |

| Output | Type | Note |
|---|---|---|
| `on_notice` | `exec` (out) |  |
| `on_skip` | `exec` (out) |  |
| `content` | `string` |  |
| `broadcaster_id` | `string` |  |
| `raid_login` | `string` |  |
| `flags` | `json` |  |
| `meta` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `templates` | `json` | `{}` |  | event_type → 文面。{user}, {reward}, {count}, {months}, {url}。 |

### `flowgraph.twitch.get_token`

**Twitch: Get Token** — conf.twitch で定義された token_key から保存済み OAuth トークンを取り出し、on_success / on_failure で分岐する

**Metadata:** contract: inputs=2 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`, `credential_access`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `token_key` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_failure` | `exec` (out) |  |
| `access_token` | `string` |  |
| `client_id` | `string` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.goals_get`

**Twitch: Get Goals** — Helix GET /goals でチャンネルのcreator goals一覧を取得する。

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |
| `goals` | `json` |  |

### `flowgraph.twitch.poll_create`

**Twitch: Create Poll** — Helix POST /polls でpollを作成する。choices は文字列配列または {title} 配列。

**Metadata:** contract: inputs=10 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `title` | `string` | — |  |
| `choices` | `json` | — |  |
| `duration_seconds` | `int` | `60` |  |
| `channel_points_voting_enabled` | `bool` | `false` |  |
| `channel_points_per_vote` | `int` | `0` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |
| `poll_id` | `string` |  |

### `flowgraph.twitch.poll_end`

**Twitch: End Poll** — Helix PATCH /polls でpollを終了する。status は TERMINATED または ARCHIVED。

**Metadata:** contract: inputs=7 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `poll_id` | `string` | — |  |
| `status` | `string` | `"TERMINATED"` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.prediction_create`

**Twitch: Create Prediction** — Helix POST /predictions でChannel Points predictionを作成する。outcomes は文字列配列または {title} 配列。

**Metadata:** contract: inputs=8 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `title` | `string` | — |  |
| `outcomes` | `json` | — |  |
| `prediction_window_seconds` | `int` | `120` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |
| `prediction_id` | `string` |  |

### `flowgraph.twitch.prediction_end`

**Twitch: End Prediction** — Helix PATCH /predictions でpredictionを LOCKED / RESOLVED / CANCELED にする。RESOLVED は winning_outcome_id 必須。

**Metadata:** contract: inputs=8 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `prediction_id` | `string` | — |  |
| `status` | `string` | `"CANCELED"` |  |
| `winning_outcome_id` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.raid_cancel`

**Twitch: Cancel Raid** — Helix DELETE /raids で保留中の raid をキャンセルする。

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.raid_start`

**Twitch: Start Raid** — Helix POST /raids で raid を開始する。実際のraidはTwitch側の90秒カウントダウン後に行われる。

**Metadata:** contract: inputs=6 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `to_broadcaster_id` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.random_clip`

**Twitch: Random Clip** — 指定配信者のクリップを取得し候補内から均等抽選。既定は視聴数順の最大100件、max_pagesで最大1000件。送信・再生は下流へ明示配線する。

**Metadata:** contract: inputs=5 / outputs=8 / properties=1 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `login` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `content` | `string` |  |
| `chat_text` | `string` |  |
| `error` | `string` |  |
| `clip` | `json` |  |
| `meta` | `json` |  |
| `flags` | `json` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `max_pages` | `int` | `1` |  |  |

### `flowgraph.twitch.shield_mode_update`

**Twitch: Update Shield Mode** — Helix PUT /moderation/shield_mode で Shield Mode を有効化/無効化する。

**Metadata:** contract: inputs=7 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `moderator_id` | `string` | — |  |
| `is_active` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.twitch.stream_marker_create`

**Twitch: Create Stream Marker** — Helix POST /streams/markers で配信マーカーを追加する。

**Metadata:** contract: inputs=6 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `broadcaster_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |
| `description` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |
| `marker_id` | `string` |  |

### `flowgraph.twitch.timeout`

**Twitch: Timeout User** — Helix POST /moderation/bans（時間制限 ban）。`duration_secs` は 1..=1209600（最大 2 週間）。永久 ban は `twitch.ban`

**Metadata:** contract: inputs=9 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `user_id` | `string` | — |  |
| `broadcaster_id` | `string` | — |  |
| `moderator_id` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `reason` | `string` | `""` |  |
| `endpoint` | `string` | `""` |  |
| `duration_secs` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `end_time` | `string` |  |
| `error` | `string` |  |
| `result` | `result<string>` |  |

### `flowgraph.twitch.user_id_by_login`

**Twitch: User ID by Login** — Helix GET /users?login=... で login から user_id を解決する

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `login` | `string` | — |  |
| `access_token` | `string` | — |  |
| `client_id` | `string` | — |  |
| `endpoint` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `user_id` | `string` |  |
| `error` | `string` |  |
| `result` | `result<string>` |  |

### `flowgraph.twitch.validate_token`

**Twitch: Validate Token** — GET https://id.twitch.tv/oauth2/validate で token 所有者の user_id / login / client_id を取得する

**Metadata:** contract: inputs=3 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `twitch_api`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `access_token` | `string` | — |  |
| `endpoint` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `user_id` | `string` |  |
| `login` | `string` |  |
| `client_id` | `string` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## un_app

### `flowgraph.un.app.command`

**UN App: Command** — UNAA/Z safe command を app command key へ publish する。dry_run では送信せず envelope だけ作る。

**Metadata:** contract: inputs=9 / outputs=12 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `app_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `snapshot` | `json` | `null` |  |
| `app_id` | `string` | — |  |
| `instance_id` | `string` | `"main"` |  |
| `command` | `string` | `"open_gui"` | enum: `focus`, `open_gui`, `reload.preview`, `profile.switch.preview`, `camera.effect` |
| `args` | `json` | `{}` |  |
| `timeout_ms` | `int` | `800` |  |
| `wait_response_ms` | `int` | `0` |  |
| `dry_run` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_accepted` | `exec` (out) |  |
| `on_response` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `accepted` | `bool` |  |
| `response_observed` | `bool` |  |
| `request_id` | `string` |  |
| `key_expr` | `string` |  |
| `response_key_expr` | `string` |  |
| `event_key_expr` | `string` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.un.app.event`

**UN App: Event** — UNAA/Z command response / event key を bounded wait で読む polling node。

**Metadata:** contract: inputs=6 / outputs=11 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`, `app_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `app_id` | `string` | — |  |
| `instance_id` | `string` | `"main"` |  |
| `request_id` | `string` | — |  |
| `wait_response_ms` | `int` | `800` |  |
| `dry_run` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_event` | `exec` (out) |  |
| `on_timeout` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `observed` | `bool` |  |
| `source` | `string` |  |
| `key_expr` | `string` |  |
| `response_key_expr` | `string` |  |
| `event_key_expr` | `string` |  |
| `payload` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

### `flowgraph.un.app.status`

**UN App: Status** — UNAA/Z discovery snapshot JSON から app readiness / capabilities / endpoint count を読む。

**Metadata:** contract: inputs=3 / outputs=15 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `snapshot` | `json` | — |  |
| `app_id` | `string` | `""` |  |
| `instance_id` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `found` | `bool` |  |
| `ready` | `bool` |  |
| `running` | `bool` |  |
| `stale` | `bool` |  |
| `app_id` | `string` |  |
| `instance_id` | `string` |  |
| `session_id` | `string` |  |
| `display_name` | `string` |  |
| `discovery_source` | `string` |  |
| `capability_count` | `int` |  |
| `command_count` | `int` |  |
| `endpoint_count` | `int` |  |
| `capabilities` | `list<json>` |  |
| `commands` | `list<json>` |  |
| `app` | `json` |  |

### `flowgraph.unavatar.camera_effect`

**UNAvatar: Camera Effect** — AvatarLinkに対応する単一Rendererへ、進入・保持・開始時状態への復帰を原子的なUNAA/Z camera.effectとして送る。

**Metadata:** contract: inputs=14 / outputs=15 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `snapshot` | `json` | `null` |  |
| `target_link_id` | `string` | — |  |
| `pose` | `json` | — |  |
| `diagonal_fov_deg` | `float` | `28.0` |  |
| `enter_duration_ms` | `int` | `450` |  |
| `enter_easing` | `string` | `"ease_in_out_cubic"` | enum: `none`, `linear`, `ease_in_sine`, `ease_out_sine`, `ease_in_out_sine`, `ease_in_quad`, `ease_out_quad`, `ease_in_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_out_cubic`, `ease_in_quart`, `ease_out_quart`, `ease_in_out_quart`, `ease_in_quint`, `ease_out_quint`, `ease_in_out_quint` |
| `hold_ms` | `int` | `5000` |  |
| `leave_duration_ms` | `int` | `650` |  |
| `leave_easing` | `string` | `"ease_in_out_cubic"` | enum: `none`, `linear`, `ease_in_sine`, `ease_out_sine`, `ease_in_out_sine`, `ease_in_quad`, `ease_out_quad`, `ease_in_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_out_cubic`, `ease_in_quart`, `ease_out_quart`, `ease_in_out_quart`, `ease_in_quint`, `ease_out_quint`, `ease_in_out_quint` |
| `conflict_policy` | `string` | `"replace"` | enum: `replace`, `queue`, `ignore_if_active` |
| `timeout_ms` | `int` | `800` |  |
| `wait_response_ms` | `int` | `800` |  |
| `dry_run` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_accepted` | `exec` (out) |  |
| `on_response` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `accepted` | `bool` |  |
| `response_observed` | `bool` |  |
| `app_id` | `string` |  |
| `instance_id` | `string` |  |
| `session_id` | `string` |  |
| `request_id` | `string` |  |
| `key_expr` | `string` |  |
| `response_key_expr` | `string` |  |
| `event_key_expr` | `string` |  |
| `response` | `json` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## unit

### `flowgraph.unit.assign`

**Unit Assign** — Attach a unit to a dimensionless Float and produce a Quantity.

**Metadata:** contract: inputs=1 / outputs=1 / properties=1 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `unit` | `string` | `""` |  | SI-compatible unit string, e.g. "m/s^2", "Hz", "kg". Empty = dimensionless. |

### `flowgraph.unit.convert`

**Unit Convert** — Convert a Quantity to the target unit. Errors if dimensions differ or K/ΔK semantics mismatch.

**Metadata:** contract: inputs=1 / outputs=1 / properties=1 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `quantity` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `target_unit` | `string` | `""` | ✔ | Target unit string. Must match the input dimension. |

### `flowgraph.unit.get_dim_string`

**Dimension -> String** — Return the canonical dimension string (e.g. "L·T^-2").

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `dim` | `string` |  |

### `flowgraph.unit.get_unit_string`

**Unit -> String** — Return the canonical unit string (e.g. "m/s^2", "Hz").

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `name` | `string` |  |

### `flowgraph.unit.same_dimension`

**Same Dimension?** — True iff both inputs carry the same physical dimension.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `quantity` | — |  |
| `b` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `bool` |  |

### `flowgraph.unit.strip`

**Unit Strip** — Explicit escape hatch: discard the unit and emit the raw numeric value as Float.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `float` |  |

### `flowgraph.unit.to_json`

**Unit -> JSON** — Serialize Quantity to JSON with `value`, `unit`, `dimension` fields (internal form).

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `json` | `json` |  |

### `flowgraph.unit.try_parse`

**Unit Try Parse** — Parse a quantity string and return failure as result<quantity> instead of halting.

**Metadata:** contract: inputs=1 / outputs=4 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `text` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `ok` | `bool` |  |
| `quantity` | `quantity` |  |
| `error` | `string` |  |
| `result` | `result<quantity>` |  |

## unmf

### `flowgraph.unmf.extract_blendshape`

**UNMF: Extract BlendShape** — UNMotionFrameからexpression sampleを取り出す。nameは生の`Joy`を正とし、旧UNVAC互換として`vmc:Joy`も受け付ける。

**Metadata:** contract: inputs=2 / outputs=4 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `unmf` | `unmf` | — |  |
| `name` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `found` | `bool` |  |
| `name` | `string` |  |
| `value` | `float` |  |
| `expression` | `json` |  |

### `flowgraph.unmf.extract_bone`

**UNMF: Extract Bone** — UNMotionFrameからhumanoid bone transformを取り出す。bone名はVMC/Unity HumanBodyBones名を受け付ける。

**Metadata:** contract: inputs=2 / outputs=10 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `unmf` | `unmf` | — |  |
| `bone` | `string` | `"Head"` |  |

| Output | Type | Note |
|---|---|---|
| `found` | `bool` |  |
| `bone` | `string` |  |
| `pose` | `json` |  |
| `px` | `float` |  |
| `py` | `float` |  |
| `pz` | `float` |  |
| `rx` | `float` |  |
| `ry` | `float` |  |
| `rz` | `float` |  |
| `rw` | `float` |  |

### `flowgraph.unmf.filter`

**UNMF: Filter** — UNMotionFrameのhumanoid bonesとexpressionsを絞り込む。bone_namesはカンマ区切り、空なら全bone維持。

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `unmf` | `unmf` | — |  |
| `bone_names` | `string` | `""` |  |
| `expression_contains` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `unmf` | `unmf` |  |

### `flowgraph.unmf.from_vmc`

**UNMF: From VMC** — VMC互換 `motion_frame` をUNMotionFrameへ変換する。VMCはここで互換adapterになる。

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `frame` | `motion_frame` | — |  |
| `sequence` | `int` | `0` |  |

| Output | Type | Note |
|---|---|---|
| `unmf` | `unmf` |  |

### `flowgraph.unmf.map`

**UNMF: Map** — UNMotionFrameの位置translationとexpression valueをスケールする。expressionは既定で0..1にclampする。

**Metadata:** contract: inputs=4 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `unmf` | `unmf` | — |  |
| `translation_scale` | `float` | `1.0` |  |
| `expression_scale` | `float` | `1.0` |  |
| `clamp_expressions` | `bool` | `true` |  |

| Output | Type | Note |
|---|---|---|
| `unmf` | `unmf` |  |

### `flowgraph.unmf.to_vmc`

**UNMF: To VMC** — UNMotionFrameをVMC互換 `motion_frame` へ変換する。既存VMC送出ノードとの接続用。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `unmf` | `unmf` | — |  |

| Output | Type | Note |
|---|---|---|
| `frame` | `motion_frame` |  |

### `flowgraph.unmfz.decode`

**UNMF/Z: Decode** — UNMF/Z MessagePack payload bytesをUNMotionFrameへデコードする。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `payload` | `bytes` | — |  |

| Output | Type | Note |
|---|---|---|
| `unmf` | `unmf` |  |

### `flowgraph.unmfz.encode`

**UNMF/Z: Encode** — UNMotionFrameをUNMF/Z MessagePack payload bytesへエンコードする。

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `unmf` | `unmf` | — |  |

| Output | Type | Note |
|---|---|---|
| `payload` | `bytes` |  |

### `flowgraph.unmfz.publish`

**UNMF/Z: Publish** — UNMotionFrame を UNMF/Z MessagePack として Zenoh に publish する。

**Metadata:** contract: inputs=7 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `unmf` | `unmf` | — |  |
| `base_key_expr` | `string` | `""` |  |
| `topic_mode` | `string` | `""` | enum: ``, `frame`, `primary_source`, `stream_id` |
| `schema_major` | `int` | `0` |  |
| `endpoint` | `json` | `null` |  |
| `dry_run` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `key_expr` | `string` |  |
| `bytes_sent` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.unmfz.resolve_endpoint`

**UNMF/Z: Resolve Endpoint** — UNAA/Z app endpoint metadata から UNMF/Z の base key と topic mode を解決する。

**Metadata:** contract: inputs=4 / outputs=5 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `endpoints` | `json` | `null` |  |
| `app_id` | `string` | `""` |  |
| `endpoint_id` | `string` | `""` |  |
| `direction` | `string` | `"output"` | enum: `input`, `output`, `both` |

| Output | Type | Note |
|---|---|---|
| `found` | `bool` |  |
| `base_key_expr` | `string` |  |
| `topic_mode` | `string` |  |
| `schema_major` | `int` |  |
| `endpoint` | `json` |  |

### `flowgraph.unmfz.subscribe`

**UNMF/Z: Subscribe Once** — Zenoh の UNMF/Z topic を購読し、exec 発火ごとに最大 1 フレーム受信する。

**Metadata:** contract: inputs=6 / outputs=8 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `base_key_expr` | `string` | `""` |  |
| `topic_mode` | `string` | `""` | enum: ``, `frame`, `primary_source`, `stream_id` |
| `schema_major` | `int` | `0` |  |
| `endpoint` | `json` | `null` |  |
| `timeout_ms` | `int` | `0` |  |

| Output | Type | Note |
|---|---|---|
| `on_frame` | `exec` (out) |  |
| `on_timeout` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `found` | `bool` |  |
| `unmf` | `unmf` |  |
| `key_expr` | `string` |  |
| `error` | `string` |  |
| `result` | `result<unmf>` |  |

## util

### `flowgraph.util.debounce`

**Debounce** — After `value` JSON stops changing for `deadtime_ms`, updates `value_out` to the stable value. Each change restarts the timer (`ctx.trigger` + `__resume__`). Requires `run_forever` for async debounce.

**Metadata:** contract: inputs=4 / outputs=1 / properties=0 / exec_in=yes / exec_out=no; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `json` | `null` |  |
| `deadtime_ms` | `int` | `100` |  |
| `__resume__` | `exec` (in) | — |  |
| `__pending_id__` | `int` | `-1` |  |

| Output | Type | Note |
|---|---|---|
| `value_out` | `json` |  |

### `flowgraph.util.delay`

**Delay** — exec_in 発火で value を保持し、delay_ms 後に exec_out を発火

**Metadata:** contract: inputs=5 / outputs=2 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `value` | `json` | `null` |  |
| `delay_ms` | `int` | `1000` |  |
| `__resume__` | `exec` (in) | — |  |
| `__pending_id__` | `int` | `-1` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `value` | `json` |  |

### `flowgraph.util.edge_detect`

**Edge detect** — On each `exec_in`, compares `value` to the previous sample and may fire `on_edge` with `edge_type` (`rising` / `falling`). Property `mode`: `rising` | `falling` | `both` (default). Pull-only reads return the last `edge_type` string (initially empty). Wire `exec_in` together with the signal you sample (e.g. `state.bool` `changed` exec).

**Metadata:** contract: inputs=2 / outputs=2 / properties=1 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `value` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `edge_type` | `string` |  |
| `on_edge` | `exec` (out) |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `mode` | `string` | `"both"` |  | choices: `rising`, `falling`, `both`; rising: low→high only, falling: high→low only, both: either transition. |

### `flowgraph.util.format`

**Format Quantity** — Quantity → String with explicit include_unit / precision / unit_override control. Default output is "{value} {unit}" matching the engine-level Quantity → String coerce.

**Metadata:** contract: inputs=1 / outputs=1 / properties=3 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `quantity` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `string` |  |

| Property | Type | Default | Required | Note |
|---|---|---|---|---|
| `include_unit` | `bool` | `true` |  | Append " {unit}" suffix when the value is non-dimensionless. |
| `precision` | `int` | `-1` |  | Decimal places for the numeric part. -1 means use the default Display formatter (no forced precision). |
| `unit_override` | `string` | `""` |  | If non-empty, convert the Quantity to this unit before formatting (same-dimension only, errors otherwise). Useful for rendering in a different unit than upstream. |

### `flowgraph.util.log`

**Log** — value 入力を trace に書き出し exec_out を発火。`Quantity` を流した場合は engine 側で `"{value} {unit}"` 形式に自動文字列化される（dimensionless は数値のみ）。精度や単位の ON/OFF を制御したい場合は `flowgraph.util.format` を挟む。

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `trace_write`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `value` | `string` | — |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |

### `flowgraph.util.prev_value`

**Previous value** — Each time `value` is evaluated, outputs `prev`: the **previous** input JSON. First sample: `prev` equals the current `value`.

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `prev` | `json` |  |

### `flowgraph.util.rate_limit`

**Rate Limit** — N 回 / X ms のトークンバケットで exec_in をゲートする（超過時は on_deny）

**Metadata:** contract: inputs=3 / outputs=3 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=explicit/json; restore=explicit; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `max_count` | `int` | — |  |
| `window_ms` | `int` | `30000` |  |

| Output | Type | Note |
|---|---|---|
| `on_allow` | `exec` (out) |  |
| `on_deny` | `exec` (out) |  |
| `remaining` | `int` |  |

### `flowgraph.util.sample_hold`

**Sample & hold** — While `sample_exec` fires, captures the current `value` JSON into `held`. Between samples, `held` stays constant.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=yes / exec_out=no; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `sample_exec` | `exec` (in) | — |  |
| `value` | `json` | `null` |  |

| Output | Type | Note |
|---|---|---|
| `held` | `json` |  |

### `flowgraph.util.throttle`

**Throttle** — Leading-edge throttle on JSON `value`: when the value **differs** from the last emitted one, emit immediately only if at least `interval_ms` has passed since the last emit; otherwise drop the update. Identical consecutive values are passed through without resetting the timer.

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `value` | `json` | `null` |  |
| `interval_ms` | `int` | `100` |  |

| Output | Type | Note |
|---|---|---|
| `value_out` | `json` |  |

### `flowgraph.util.timer_interval`

**Timer Interval** — Periodic timer: while `enabled` and `run_forever` trigger bus is active, fires `on_tick` every `interval_sec` (min 0.01s, sleep min 10ms). Outputs `count` (total ticks) and `elapsed_sec` (wall time since previous tick, or `interval_sec` on first tick). Stale wakeups are dropped. `execute()` one-shot mode does not arm (same as `util.delay`).

**Metadata:** contract: inputs=4 / outputs=3 / properties=0 / exec_in=yes / exec_out=yes; effect: `stateful`; capabilities: —; trigger: —; state: stateful; scope=`node_instance`; storage=`volatile`; lifetime=`program_instance`; reload=reinitialized; snapshot=unsupported; restore=unsupported; migration=`none`; persistence=`none`

| Input | Type | Default | Note |
|---|---|---|---|
| `enabled` | `bool` | `true` |  |
| `interval_sec` | `float` | `1.0` |  |
| `__tick__` | `exec` (in) | — |  |
| `__pending_id__` | `int` | `-1` |  |

| Output | Type | Note |
|---|---|---|
| `on_tick` | `exec` (out) |  |
| `count` | `int` |  |
| `elapsed_sec` | `float` |  |

## vec

### `flowgraph.vec2.add`

**Vec2 add** — Componentwise vec2 addition

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec2.distance`

**Vec2 distance** — Euclidean distance between two vec2s

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `float` |  |

### `flowgraph.vec2.dot`

**Vec2 dot** — Vec2 dot product (returns scalar)

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `float` |  |

### `flowgraph.vec2.length`

**Vec2 length** — Euclidean magnitude of a vec2: sqrt(x^2 + y^2)

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `v` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `float` |  |

### `flowgraph.vec2.lerp`

**Vec2 lerp** — Componentwise linear interpolation: a + (b - a) * t for 2-vectors. t is not clamped.

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |
| `t` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec2.make`

**Vec2 make** — Pack 2 floats into a JSON array [x, y]

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `float` | — |  |
| `y` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `v` | `json` |  |

### `flowgraph.vec2.normalize`

**Vec2 normalize** — Scale a vec2 to unit length. Zero vector returns [0, 0] (not an error).

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `v` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec2.scale`

**Vec2 scale** — Multiply every component of a 2-vector by scalar `k`

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `k` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec2.sub`

**Vec2 sub** — Componentwise vec2 subtraction

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec2.unpack`

**Vec2 unpack** — Unpack a 2-component JSON array into separate Float outputs

**Metadata:** contract: inputs=1 / outputs=2 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `v` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `x` | `float` |  |
| `y` | `float` |  |

### `flowgraph.vec3.add`

**Vec3 add** — Componentwise vec3 addition

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec3.distance`

**Vec3 distance** — Euclidean distance between two vec3s

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `float` |  |

### `flowgraph.vec3.dot`

**Vec3 dot** — Vec3 dot product (returns scalar)

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `float` |  |

### `flowgraph.vec3.length`

**Vec3 length** — Euclidean magnitude of a vec3: sqrt(x^2 + y^2 + z^2)

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `v` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `float` |  |

### `flowgraph.vec3.lerp`

**Vec3 lerp** — Componentwise linear interpolation: a + (b - a) * t for 3-vectors. t is not clamped.

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |
| `t` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec3.make`

**Vec3 make** — Pack 3 floats into a JSON array [x, y, z]

**Metadata:** contract: inputs=3 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `x` | `float` | — |  |
| `y` | `float` | — |  |
| `z` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `v` | `json` |  |

### `flowgraph.vec3.normalize`

**Vec3 normalize** — Scale a vec3 to unit length. Zero vector returns [0, 0, 0] (not an error).

**Metadata:** contract: inputs=1 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `v` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec3.scale`

**Vec3 scale** — Multiply every component of a 3-vector by scalar `k`

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `k` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec3.sub`

**Vec3 sub** — Componentwise vec3 subtraction

**Metadata:** contract: inputs=2 / outputs=1 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `a` | `json` | — |  |
| `b` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `result` | `json` |  |

### `flowgraph.vec3.unpack`

**Vec3 unpack** — Unpack a 3-component JSON array into separate Float outputs

**Metadata:** contract: inputs=1 / outputs=3 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `v` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `x` | `float` |  |
| `y` | `float` |  |
| `z` | `float` |  |

## vmc

### `flowgraph.vmc.extract_blendshape`

**VMC: Extract BlendShape** — `MotionFrame` から `/VMC/Ext/Blend/Val` を探し、JSON と型付きの `found/name/value` を返す。無ければ JSON は null、typed output は既定値。

**Metadata:** contract: inputs=2 / outputs=4 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `frame` | `motion_frame` | — |  |
| `blendshape_name` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `blendshape` | `json` |  |
| `found` | `bool` |  |
| `name` | `string` |  |
| `value` | `float` |  |

### `flowgraph.vmc.extract_bone_pos`

**VMC: Extract Bone Pos** — `MotionFrame` から `/VMC/Ext/Bone/Pos` を探し、JSON と型付きの `found/bone/px/py/pz/rx/ry/rz/rw` を返す。無ければ JSON は null、typed output は既定値。

**Metadata:** contract: inputs=2 / outputs=10 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `frame` | `motion_frame` | — |  |
| `bone_name` | `string` | `""` |  |

| Output | Type | Note |
|---|---|---|
| `pose` | `json` |  |
| `found` | `bool` |  |
| `bone` | `string` |  |
| `px` | `float` |  |
| `py` | `float` |  |
| `pz` | `float` |  |
| `rx` | `float` |  |
| `ry` | `float` |  |
| `rz` | `float` |  |
| `rw` | `float` |  |

### `flowgraph.vmc.extract_root_pos`

**VMC: Extract Root Pos** — `MotionFrame` から `/VMC/Ext/Root/Pos` を探し、JSON と型付きの `found/bone/px/py/pz/rx/ry/rz/rw` を返す。無ければ JSON は null、typed output は既定値。

**Metadata:** contract: inputs=1 / outputs=10 / properties=0 / exec_in=no / exec_out=no; effect: `pure`; capabilities: —; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `frame` | `motion_frame` | — |  |

| Output | Type | Note |
|---|---|---|
| `pose` | `json` |  |
| `found` | `bool` |  |
| `bone` | `string` |  |
| `px` | `float` |  |
| `py` | `float` |  |
| `pz` | `float` |  |
| `rx` | `float` |  |
| `ry` | `float` |  |
| `rz` | `float` |  |
| `rw` | `float` |  |

### `flowgraph.vmc.send_bone_pos`

**VMC: Send Bone Pos** — VMC `/VMC/Ext/Bone/Pos` を 1 回 UDP 送信。`position` は [x,y,z]、`rotation` は [qx,qy,qz,qw] の JSON 配列

**Metadata:** contract: inputs=6 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `host` | `string` | — |  |
| `port` | `int` | — |  |
| `bone_name` | `string` | — |  |
| `position` | `json` | — |  |
| `rotation` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_sent` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.vmc.send_root_pos`

**VMC: Send Root Pos** — VMC `/VMC/Ext/Root/Pos` を 1 回 UDP 送信（骨名は常に `root`）。`position` / `rotation` は JSON 配列

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `host` | `string` | — |  |
| `port` | `int` | — |  |
| `position` | `json` | — |  |
| `rotation` | `json` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_sent` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

## vrchat

### `flowgraph.vrchat.avatar_parameter_bool`

**VRChat: Avatar Parameter (Bool)** — OSC `/avatar/parameters/<name>` に bool を 1 つ送信

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `host` | `string` | — |  |
| `port` | `int` | — |  |
| `parameter_name` | `string` | — |  |
| `value` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_sent` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.vrchat.avatar_parameter_float`

**VRChat: Avatar Parameter (Float)** — OSC `/avatar/parameters/<name>` に float を 1 つ送信（VRChat OSC Avatar Parameters）

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `host` | `string` | — |  |
| `port` | `int` | — |  |
| `parameter_name` | `string` | — |  |
| `value` | `float` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_sent` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.vrchat.avatar_parameter_int`

**VRChat: Avatar Parameter (Int)** — OSC `/avatar/parameters/<name>` に int を 1 つ送信

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `host` | `string` | — |  |
| `port` | `int` | — |  |
| `parameter_name` | `string` | — |  |
| `value` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_sent` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.vrchat.chatbox_input`

**VRChat: Chatbox Input** — `/chatbox/input` に (text, send_immediately, play_notification_sfx)。テキストは最大 144 文字に切り詰め

**Metadata:** contract: inputs=6 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `host` | `string` | — |  |
| `port` | `int` | — |  |
| `text` | `string` | — |  |
| `send_immediately` | `bool` | `true` |  |
| `play_notification_sfx` | `bool` | `true` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_sent` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.vrchat.chatbox_typing`

**VRChat: Chatbox Typing** — `/chatbox/typing` に bool を 1 つ送信

**Metadata:** contract: inputs=4 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `host` | `string` | — |  |
| `port` | `int` | — |  |
| `typing` | `bool` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `bytes_sent` | `int` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

## web

### `flowgraph.web.rss_fetch`

**Web RSS Fetch** — RSS / Atom / JSON Feed を取得して normalized item JSON と Table に変換する。Effectful

**Metadata:** contract: inputs=6 / outputs=7 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `network`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `url` | `string` | — |  |
| `limit` | `int` | `20` |  |
| `since` | `string` | `""` |  |
| `include_content` | `bool` | `false` |  |
| `timeout_ms` | `int` | `5000` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `items_json` | `list<json>` |  |
| `items_table` | `table` |  |
| `latest_updated_at` | `string` |  |
| `error` | `string` |  |
| `result` | `result<json>` |  |

## window

### `flowgraph.window.close`

**Window: Close** — 対象ウィンドウへ WM_CLOSE を送る。

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `hwnd` | `int` | `0` |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `affected_count` | `int` |  |
| `windows` | `table` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.window.enum`

**Window: Enum** — トップレベルウィンドウ一覧を Table として取得する。Windows first。

**Metadata:** contract: inputs=5 / outputs=5 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |
| `visible_only` | `bool` | `true` |  |

| Output | Type | Note |
|---|---|---|
| `exec_out` | `exec` (out) |  |
| `windows` | `table` |  |
| `count` | `int` |  |
| `error` | `string` |  |
| `result` | `result<table>` |  |

### `flowgraph.window.foreground`

**Window: Foreground** — 対象ウィンドウを前面化する。

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `hwnd` | `int` | `0` |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `affected_count` | `int` |  |
| `windows` | `table` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.window.maximize`

**Window: Maximize** — 対象ウィンドウを最大化する。

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `hwnd` | `int` | `0` |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `affected_count` | `int` |  |
| `windows` | `table` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.window.minimize`

**Window: Minimize** — 対象ウィンドウを最小化する。

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `hwnd` | `int` | `0` |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `affected_count` | `int` |  |
| `windows` | `table` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.window.move`

**Window: Move** — 対象ウィンドウの左上座標を変更する。

**Metadata:** contract: inputs=7 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `hwnd` | `int` | `0` |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |
| `x` | `int` | — |  |
| `y` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `affected_count` | `int` |  |
| `windows` | `table` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.window.pseudo_fullscreen`

**Window: Pseudo Fullscreen** — 対象ウィンドウの表示状態を保存して最大化する first slice。

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `hwnd` | `int` | `0` |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `affected_count` | `int` |  |
| `windows` | `table` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.window.pseudo_fullscreen_exit`

**Window: Pseudo Fullscreen Exit** — pseudo_fullscreen 前の表示状態を復元する。

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `hwnd` | `int` | `0` |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `affected_count` | `int` |  |
| `windows` | `table` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.window.resize`

**Window: Resize** — 対象ウィンドウのサイズを変更する。

**Metadata:** contract: inputs=7 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `hwnd` | `int` | `0` |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |
| `width` | `int` | — |  |
| `height` | `int` | — |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `affected_count` | `int` |  |
| `windows` | `table` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

### `flowgraph.window.restore`

**Window: Restore** — 対象ウィンドウを通常表示へ戻す。

**Metadata:** contract: inputs=5 / outputs=6 / properties=0 / exec_in=yes / exec_out=yes; effect: `effectful`; capabilities: `window_control`; trigger: —; state: stateless

| Input | Type | Default | Note |
|---|---|---|---|
| `exec_in` | `exec` (in) | — |  |
| `hwnd` | `int` | `0` |  |
| `pid` | `int` | `0` |  |
| `title_filter` | `string` | `""` |  |
| `exact` | `bool` | `false` |  |

| Output | Type | Note |
|---|---|---|
| `on_success` | `exec` (out) |  |
| `on_error` | `exec` (out) |  |
| `affected_count` | `int` |  |
| `windows` | `table` |  |
| `error` | `string` |  |
| `result` | `result<int>` |  |

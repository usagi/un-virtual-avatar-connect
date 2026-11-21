# Flowgraph の構成・境界・インスタンス正本

> Status: v2 canonical design. 本書は Flowgraph の file / module / instance / cross-file 接続と、公開接続から文字列 channel を撤去する規則の正本である。旧 Phase δ / λ 文書と矛盾する場合は本書を優先する。

## 実装状態

| 区分 | 内容 |
|---|---|
| implemented | 型付きgraph I/O、load-time graph-as-node展開、複数instance namespace、package境界、cross-file portal、profile composition overview、現在modeのactive/inactive表示とactivation境界診断、bounded runtime trace、activity drill-down、state snapshot/reload、ZIP共有、公開channel撤去。 |
| required before v2 release | 本章の完了試験をrelease候補で再実行し、実用profileのmode設定と外部I/O資格情報を用いた手動smokeを記録する。言語・runtime・Studioの未実装項目は残さない。 |
| future | 外部package registryとversion range solver、traceの永続保存・remote collector、大規模profile overviewの階層collapse。現行の接続意味論を変更しない拡張だけをここへ置く。 |

## 1. 設計原則

Visual モードだけで、処理順序、data I/O、fan-out、外部 I/O、実行中か否かを追跡できなければならない。ユーザーが扱う接続はすべて型付き port と明示 edge で表し、producer と consumer が文字列名だけで結び付く暗黙 pub/sub は公開言語に置かない。

ランタイム内部は broadcast queue や `ChannelDatum` を利用してよい。ただしそれは AI、WebSocket、BOS、OBS 字幕など各 adapter の実装詳細であり、Flowgraph には目的を示す型付き source / sink node として公開する。内部 bus の topic 名をユーザーが接続契約として入力してはならない。

## 2. 用語

| 用語 | 意味 |
|---|---|
| file | `*.flowgraph.toml` という保存・差分・Studio navigation の物理単位。 |
| graph | 1 file に記述された node / edge / interface / instance の論理DAG。 |
| entry graph | 自身が runtime へ一度だけ配置される graph。`[graph].kind = "entry"`。省略時の既定。 |
| module graph | 型付き interface を通して明示的に instance 化されたときだけ配置される graph。`[graph].kind = "module"`。 |
| instance | module graph を親 graph に置いた graph-as-node。親 Canvas では1 node、実行時は名前空間付きの内部DAG。 |
| subgraph | graph 内部の構造を graph-as-nodeへ抽出したもの。保存後は module graph + instanceであり、別の実行概念ではない。 |
| group | Canvas 上の視覚的整理だけを行う `[[groups]]`。scope、型、activation、stateを変えない。 |
| library | 再利用を意図して export された module graph の集合。実行概念ではない。 |
| package | library の配布・version・dependency・lockを管理する単位。 |

1 file に複数の独立 graph を埋め込まない。graphを分けるならfileを分ける。この制約により、fq、Studio navigation、診断位置、Git差分を一意にする。

### Canvas での部品とグループの操作

部品（instance）は通常ノードと一緒に選択し、移動・整列・複製・削除できる。グループにまとめた場合も、部品を含む所属要素をまとめて移動・選択できる。グループからの除外やグループ枠の削除では、所属するノードや部品自体は削除しない。

部品の複製は、同じ module ファイルを参照する別の instance を作る操作だ。参照先ファイルのコピーは作らない。module の内容を編集すると、それを参照する各 instance に反映される。複製時に既存の接続線はコピーしないため、必要な入力・出力は別途接続する。

部品自体を削除すると、その部品への接続線も除かれる。取り消しでは位置・接続・グループ所属を復元する。これらの編集は保存するまで下書きであり、別のファイルやプロファイルの内容を同時に書き換えるものではない。

## 3. ファイル種別と型付き interface

```toml
[graph]
kind = "module" # "entry" | "module"

[[graph.inputs]]
name = "exec_in"
type = "exec"
to = "normalize:exec_in"

[[graph.inputs]]
name = "text"
type = "string"
to = "normalize:value"

[[graph.outputs]]
name = "exec_out"
type = "exec"
from = "normalize:exec_out"

[[graph.outputs]]
name = "text"
type = "string"
from = "normalize:value"
```

- interface port名はgraph内で方向ごとに一意。
- `type` は `SocketType::parse` が受理するcanonical型または `exec`。
- input の `to` は同じfile内の実node入力、output の `from` は実node出力を指す。
- 宣言型と実port型は通常edgeと同じ互換規則で検証する。exec/data混在は禁止。
- 未接続の必須interface、存在しないnode/port、重複名、型不一致はload error。
- moduleはinterface外から内部nodeを参照できない。entry同士のcross-file edgeは後述のportalとして許可する。
- entryにinterfaceを宣言してもよいが、それは外部tooling向け署名であり、instance化はできない。

旧 `flowgraph.library.input/output` nodeは廃止する。境界は通常nodeのふりをするスタブではなく、graph署名そのものだからだ。

## 4. graph-as-node

```toml
[[instances]]
id = "conversation"
graph = "./modules/conversation"
position = { x = 640.0, y = 220.0 }

[[edges]]
from = "voice:exec_out"
to = "conversation:exec_in"

[[edges]]
from = "voice:text"
to = "conversation:text"

[[edges]]
from = "conversation:reply"
to = "tts:text"
```

`graph` は既存fq解決と同じ規則でmodule fileへ解決する。`include`によるテキスト展開は採用しない。依存はinstance宣言から導出でき、同じ対象を別経路で宣言しない。

loaderはinstanceを実行前に内部DAGへ展開する。runtime node IDは概念上 `<owner-fq>::@<instance-path>/<local-node>` とし、同じmoduleの複数instanceはnode、state、memoization、diagnostics、traceを共有しない。nested instanceはinstance pathを連結する。

- data/execはいずれもinterface mappingを介して通常edgeへ展開する。
- instance cycleはload error。診断には `parent.instance -> module` の閉路全体を載せる。
- entry、親module、対象moduleの`[meta].mode_groups` / `default_enabled`は、展開時に上書きせずactivation制約列として保持する。各層を現在modeで個別評価し、すべてactiveの場合だけinstance配下のexecを許可する。従って親またはmodule自身のどちらかがinactiveならexecを抑止する。data pullは既存Runtime Mode規則どおり参照可能。
- module内のstateはinstance scope。snapshot keyには完全なruntime node IDを用いる。
- module内のingress / timer等の自律sourceはinstanceごとに生成される。意図しない多重購読を避けるため、Studioは自律sourceを持つmoduleの複数instanceへ警告を表示する。

## 5. cross-file edge と Studio portal

単なる大規模graphのファイル分割には既存のcross-file edgeを使える。

```toml
[[edges]]
from = "in:content"
to = "./tts::speaker:text"
```

ただしStudioは相手nodeをCanvas外へ消してはならない。両fileにread-onlyの境界portalを表示する。

- 送信側: `speaker:text → tts/main` のoutgoing portal。
- 受信側: `chat/main::in:content → speaker:text` のincoming portal。
- portalはdata/exec、型、相手fq、node、port、active/inactiveを表示する。
- click / Enterで相手fileを開き、対象nodeを選択する。
- profile overviewは全file portalとinstanceを一枚の上位graphとして表示できる。
- edgeの所有者はTOMLに宣言したfile。反対側portalから削除する場合も所有fileを明示して編集する。

再利用・カプセル化が必要ならcross-file edgeではなくmodule instanceを使う。cross-file edgeはfile分割、instanceは抽象化という区別。

## 6. import / include / library_uses / package

- `include`: 不採用。folder walkとfq解決に加えてテキスト展開を持つと、所有権・診断・循環・Studio保存先が曖昧になる。
- `import`: 言語directiveとしては設けない。`[[instances]].graph` が実行依存と配置を同時に宣言する。
- `library_uses`: 廃止。存在・循環だけを検査する重複宣言であり、実際の合成を表さない。
- `[package]`: 宣言fileの親folderをpackage scopeとする。入れ子packageでは最も深いscopeが所有者になる。root直下の宣言はroot全体を所有する。
- `[package].dependencies`: 別packageを参照できるpackage idとversionを宣言する。local catalogでも依存宣言を強制し、root外/配布packageでは解決とversion固定も担う。
- `[package].exports`: package scope内にある外部公開module fqだけを列挙する。entryやscope外fileはexportできず、package外から非export moduleをinstance化するとload error。packageに属さないworkspace entryはapplication rootとしてexport済みmoduleを参照できる。
- StudioのZIP Import/Export: file/packageを共有する操作。言語のinclude/importではない。

ZIP書き出しAPI `POST /api/v1/control/flowgraph/export/zip` は、既存の `scope`・`targets`・`origin` に加えて、省略可能な `companions`（Flowgraphルートからの相対ファイル名の配列）を受け取る。例: `"companions": ["my-feature/README.md", "my-feature/images/notice.png"]`。指定した補助ファイルだけを元の相対位置で同梱し、周辺フォルダやノードの参照先を自動収集しない。省略時は従来どおりグラフのみ。グラフ自体は `targets` で指定する。外部パス、リンク、実行ファイル、存在しないファイルなどが含まれる場合は、ZIP全体の書き出しを失敗として返す。

通常版のStudioの「ZIP書き出し」はグラフのみを対象とする。素材を同梱するには「機能を追加・管理」から対象ファイルの「共有に必要なものを確認」を開き、「候補のファイルと素材を書き出す」へ進む。「素材と一緒に書き出す」で一覧から必要なファイルを明示選択する。未選択の素材は含まれない。名前を付けた用途は、用途管理の「共有・取込」から構成と素材をまとめて書き出せる。

グラフファイルのバックアップ復元APIは、`GET /api/v1/control/flowgraph/backups/{fq}` で対象ファイルのバックアップ名を取得し、`POST /api/v1/control/flowgraph/recovery-plan/{fq}` に `{"backup":"対象のバックアップ名"}` を渡して内容を確認する。返却される計画全体を `POST /api/v1/control/flowgraph/restore-file/{fq}` に渡すと、保存先・現在の内容（またはファイルが存在しない状態）・バックアップ内容を再確認して復元する。確認後の変更は競合として拒否する。現在のファイルがある場合は、それも別のバックアップへ残す。復元元のバックアップは削除しない。

これはグラフ定義ファイルの復元であり、実行中ノードのstate snapshot復元とは別の操作。通常版の機能目次「Flowgraphを履歴から復元」から開ける。ファイル復元後の再読み込み結果は `ok` と `diagnostics` で確認する。比較用の配置案の採用判断と、通常版の復元機能の有無は区別する。

グラフの新規作成・通常保存・バックアップ復元の要求本文は、Flowgraphの64 MiB上限を共通に使う。通常保存の `expected_content` や復元の比較用テキストも含めたJSON全体の上限であり、1ファイル当たりの容量保証ではない。不正なJSONや必須項目の欠落は書き込み前に拒否する。

## 7. runtime activation、capability、reload

activationの単位はentry fileとinstance。module definition単体には実行状態を持たせない。

1. entryの`mode_groups/default_enabled`から基底状態を求める。
2. instanceは親の状態を継承する。
3. inactiveなentry/instanceは新規execを受け取らない。data pullの規則は既存RM-3を維持する。
4. cross-file edgeの両端でactivationが異なる場合、Studioとdiagnosticsにinactive境界を表示する。暗黙dropにしない。

capability summaryは展開後のinstanceごとにsource moduleを保持して集計する。modeのdenyはpreviewだけでなく実行拒否へ進め、拒否時はnode、instance path、capabilityを診断する。

通常reloadは再load・再展開し、instance追加削除を反映する。state-preserving reloadは完全runtime node IDが一致するstateだけをrestoreする。module versionまたはinstance id変更は別identityであり、暗黙移行しない。

### 再読込・状態復元の対象

再読込は、操作中の画面が接続しているVACのグラフ全体を対象にする。エディターで選んでいる1ファイルだけの操作ではない。未保存の編集内容は再読込の入力に含まれないため、変更を反映したい場合は先に保存する。

複数のVACを起動している場合は、一覧の「専用画面を開く」から対象VACの画面へ移る。保存プロファイルの選択や次回起動の既定変更だけで、今の画面の接続先が別VACへ切り替わるわけではない。

状態を保持する再読込は、稼働中のノード状態を保存してから再読込する。保存済み状態の復元は、対象プロファイルとグラフ保存先に対応する状態ファイルを使う。これはグラフファイルそのもののバックアップ復元とは別の操作だ。グラフファイルの削除を戻したい場合は、ファイルの復元履歴を使う。

機能目次の「処理状態の保存・復元」は、Studioの既存の状態操作を開く。

| 操作 | 保存・反映する内容 |
|---|---|
| 読込時点の状態を保存 | 最後にグラフを読み込んだ時点の状態。稼働後に増えたカウンター値などは含まない |
| 現在の状態を保存 | 稼働中の処理から取得した状態。保存だけではグラフを再読込しない |
| 状態を保持して再読込 | 現在の状態を保存し、ディスク上のグラフをその状態で再読込する |
| 保存した状態を復元 | 既に保存されている状態を使ってグラフを再読込する |

保存対象は状態保存に対応したノードであり、アプリ全体や配信サービス全体のバックアップではない。
上の保存操作は画面に表示された同じ状態ファイルを更新する。世代別の履歴保存ではないため、
以前の保存状態へ戻したいときは、新たに保存する前に「保存した状態を復元」を使う。
状態の保存は同じフォルダー内の一時ファイルへ書き終えてから置き換える。検証や書込みに失敗した場合は、既存の保存ファイルを上書きしない。
保存先が未設定の場合や現在状態を取得できない場合は、画面の説明と診断を確認する。

復元候補のファイルが欠落・破損している場合や、状態をグラフへ復元できない場合は、
稼働中の構成を停止せずに失敗理由を返す。編集中の入力も保持する。
正常な復元は処理の再起動を伴うため、外部イベントを受信している配信中などは実行時点を選ぶ。

同じ保存先で保存・取込・再読込などが進行中の場合は、競合を知らせて今回の操作を止める。先の操作が終わってから再操作する。再読込時に保存先フォルダーが見つからない場合も処理を進めない。表示された保存先の移動・削除やドライブの接続を確認し、元の構成を利用できる状態へ戻す。再読込だけで空のフォルダーを作り直すことはない。

## 8. diagnostics と debug trace

load時に最低限次を検査する。

- module未解決、entryのinstance化、instance/library循環、ID衝突。
- interface mappingのnode/port未解決、型不一致、exec/data混在、重複port。
- module内部への直接参照、package非export参照。
- 通常edgeと同じcycle、fan-in、closed-set、record schema規則。
- inactive境界、自律sourceの多重instance、未使用interfaceはwarning。

trace eventは `generation`, `runtime_node_id`, `source_fq`, `instance_path`, `port`, `phase`, `value_summary`, `active` を持つ。直近2048件をメモリ上のbounded bufferに保持し、Control API diagnosticsからStudioへ渡す。通常の`value_summary`は型またはexecだけで値本体を含めない。明示的に配置した`flowgraph.util.log`の`exec_in`を発火した場合だけ、`log`イベントとして入力本文を記録する（最大4096文字、超過時は省略表示）。記録したくない内容はLogノードへ接続しない。Studioは親instance上の入出力activityから子Canvasの該当node/edgeへdrill downできる。

## 9. 公開channelの廃止

v2公開言語から次を削除する。

- `flowgraph.channel.emit`
- `flowgraph.ingress.channel_subscribe`
- `[meta].library_uses`
- `flowgraph.library.input/output`
- Flowgraph間接続を目的にした任意文字列channel設定

置換は用途別の明示nodeで行う。

| 旧用途 | 公開する接続 |
|---|---|
| Flowgraph間fan-out | 通常edge / cross-file portal / module output |
| AI observe / speech | 型付きAI input/output node |
| TTS | `flowgraph.tts.speak`へのedge |
| 字幕/BOS/WS | destinationを明示するBOS output node |
| Twitch event | 型付きTwitch ingress node |
| Avatar | AvatarIntent / UNMF node |
| OBS | 型付きOBS node |

内部`ChannelDatum` busは当面サービスadapter内に残せるが、公開node間の接続判定、Studio配線、profile構成には使わない。

## 10. 移行と削除条件

v2は未リリースのため公開channelに互換猶予期間を設けない。移行は同一開発系列内で一括して行う。

1. composition schema、loader展開、診断、Studio portal/instanceを実装。
2. AI/BOS等の目的別source/sinkを実装。
3. `flowgraph.local`、`profile.template`、examples、fixturesを明示edgeへ移行。
4. node catalog、Control API、GUI serializer、manualから旧4機能を削除。
5. `rg`で公開設定に旧feature/channel routeが0件、全profileがload成功、GUI E2Eがdrill-downを確認した時点で削除完了。

V1互換APIやWS payloadに`ChannelDatum`が残ることは、この完了判定を妨げない。公開Flowgraph言語とVisual接続モデルから見えないことが境界。

## 11. 比較した代替案

| 案 | 利点 | 却下理由 |
|---|---|---|
| channel routeを上位図に表示 | 既存実装を温存しやすい | edgeとbusの二重接続モデル、filter/final/flagsまで追う必要、型検査不能。 |
| includeでfileを展開 | syntaxが短い | ownership、複数instance、state、debug位置、循環が曖昧。 |
| cross-file edgeだけで統一 | 現engineに近い | 再利用署名、カプセル化、複数instanceを表せない。 |
| runtimeに入れ子engineを置く | module隔離が強い | exec/data pull、capability、reload、traceをengine間protocolへ重複実装する。 |
| load時にinstanceをDAG展開 | 既存型検査と実行意味論を再利用、複数instance/state分離、Visual対応が明快 | 展開metadataが必要だが、最小で一貫した変更。採用。 |

## 12. 完了試験

Flowgraph基盤の自動試験完了は、製品のrelease-ready判定とは分離する。active managed profileの
release gateは[`v2-release-gate.md`](./v2-release-gate.md)を正本とし、daily/streaming実動作、
実Twitch/チャネルポイント、Renderer到達、restart/reload、Studio調査、長時間負荷の有人証跡が
揃うまでrelease blockerとする。

- loader unit: interface型、未解決、entry/module規則、nested/multiple instance、instance cycle、namespace/state分離。
- engine integration: data pull、exec fan-out、error path、state snapshot/reload、inactive instance。
- API/schema: tree/file/diagnosticsがkind、interface、instance、portal、展開originを返す。
- Studio E2E: 親Canvasのinstanceを開く、子の該当portへ移動、cross-file portal往復、型不一致表示、inactive表示。
- Studio E2E: profile composition overviewで全graph、instance、portalとactive/inactiveを一枚に表示し、runtime activityから子edgeへdrill downする。
- persistence: save/reload、ZIP import/export後もinstance positionとinterfaceが保持される。
- persistence: 同一stateful moduleの複数instanceを異なる値でsnapshotし、reload後もruntime node ID別に復元される。
- migration:実用profileが旧channel nodeなしでloadし、AI→TTS/字幕、Twitch→TTS、Avatar、OBS経路を静的に追跡できる。

## 13. 同梱素材をグラフから読み込む

`flowgraph.file.read_string/read_bytes/read_json/read_toml/read_ron` と
`flowgraph.audio.play`、`flowgraph.table.load_tsv` の `path` は、明示的に `graph-file:` を付けると、
そのノードを定義した `.flowgraph.toml` のあるフォルダーを基準にする。
例えば `features/alerts/main.flowgraph.toml` から
`graph-file:assets/notice.wav` を指定すると、同じフォルダーの
`assets/notice.wav` を読む。モジュールのインスタンスでも呼出元ではなく
定義元のフォルダーを使うため、構成と素材を一緒に移動できる。

実行時に文字列を組み立てて `graph-file:` のパスを渡す場合も、基準は読み込みノードの定義元になる。共有の素材候補は設定に直接書かれた参照から探すため、組み立て後のパスは自動検出されない。共有するときは素材一覧から必要なファイルを明示選択し、グラフとの相対位置を保って同梱する。外部フォルダーの素材や、実行中に新しく生成するファイルは別途用意する。

辞書などのTSVも `graph-file:assets/dictionary.tsv` と指定して構成と一緒に共有できる。
読み込んだ表は従来と同じTable出力になる。辞書を更新・保存する用途では、
読込専用の同梱素材と、書込可能な保存先を区別して設定する。

`flowgraph.file.write_string/write_bytes/write_json/write_toml/write_ron` と
`flowgraph.table.write_tsv` にも `graph-file:` を明示指定できる。保存先の親フォルダーは
存在している必要がある。保存は処理ごとに別の一時ファイルを作り、完成後に置換する。
同じ定義元の素材を指定するモジュール個体は同じファイルを共有するため、
個体ごとに独立させたい場合は保存先も分ける。

辞書などを読み取って更新する場合は、`table.load_tsv` の `revision` を
同じファイルへの `table.write_tsv` の `expected_revision` に接続する。
読込後にファイルが変更・削除されていれば上書きせず、`on_conflict` と `on_error` を
出力する。再読込して変更内容を見直してから保存する。空の `expected_revision` は
従来どおり無条件の保存なので、競合確認が必要な処理では接続を省略しない。

Flowgraphのファイル・TSV保存は、同じフォルダーの `.vac-data-write.lock` を使って
確認と置換の間の同時書込を防ぐ。使用中にこの調整用ファイルを削除しないこと。
このロックを使わない外部エディターの書込まで排他制御するものではない。

通常の絶対パス・相対パスの既存動作は変わらない。区切りは `/`、解決後の実体は
Flowgraphルート内に限る。素材がない読込や、ルート外を指す読込・書込は、
そのノードのエラーになる。

共有時はグラフだけでなく必要な素材も書出画面で明示選択する。素材の自動収集や、
任意の文字列・URLを新しい参照形式へ自動変換する機能ではない。


## イベントと値の合流

複数経路から同じ後続へ値を渡す場合は、`flowgraph.flow.merge_string`などの型別Merge Eventsノードを使う。各経路の実行線を`exec_a`/`exec_b`、対応する値を`value_a`/`value_b`へ接続し、共通の`exec_out`と`value`を後続へ接続する。文字列のほか、JSON・整数・浮動小数・真偽値・表・バイト列・単位付き数値・日時・MotionFrame・UNMFを用意している。

発火した側の値だけを読み出す。もう一方のAPI呼出しやファイル読込がまだ実行されていなくても、その結果を要求しない。3経路以上は合流ノードをつなぎ、後続の状態を持つノードは1個を共有できる。全経路の完了待ちやまとめ処理ではなく、各イベントを到着した実行順に個別配送する。同一イベントで`exec_a`と`exec_b`の両方を指定した場合は、どちらかを捨てずにエラーにする。

実行で得た値はそのイベントとともに引き継がれるため、待機中に同じ生成元が次の値を出しても置き換わらない。未接続の選択側や接続型の不一致を許容する機能ではない。接続は通常どおり型検査され、選択側に必要な値がなければ実行は失敗する。

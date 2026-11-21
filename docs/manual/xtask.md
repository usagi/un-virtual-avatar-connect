# xtask

`xtask/` は、UNVAC repository 内で再現する生成、検証、実験、運用補助 command の正式な置き場所。

`.cargo/config.toml` が `cargo xtask` を `cargo run -p xtask --` へ展開する。docs と開発手順では短い形式を使う。

## リリースビルド

```console
cargo xtask release-build
```

`gui/` で `npm ci` と `npm run build` を実行した後、desktop / CLIと必要なSTT workerを生成する。
生成後は各バイナリのサイズとSHA-256を表示し、成果物が存在しない場合は失敗する。

成功時には成果物フォルダーへ `unvac-release-files.json` を保存する。本体・worker・
WindowsのVosk DLL・英語読みモデルと辞書の必要ファイルに加え、開始案内 `START-HERE.txt`、本体の `LICENSE`、配布版と一致する `manual/quickstart.md` と `manual/v1-to-v2-migration.md` を、サイズとSHA-256で記録する。
本体のビルド開始前に古い記録を無効化し、必要ファイルが空または欠落していれば新しい記録を作らない。
移動やコピー後は、アプリを起動せず次で照合できる。

```console
cargo xtask release-files-check target/build-portable/release
```

記録から必要ファイルを省いた場合も拒否する。この記録はビルド出力の整合性確認であり、
全配布物のZIP作成、署名、起動成功、最新ソースとの一致、公開承認を意味しない。
配布フォルダーへ資格情報や個人設定が混入していないかの確認は別途行う。

Windowsでは、Voskの実行に同梱する `libvosk.dll`、`libgcc_s_seh-1.dll`、
`libstdc++-6.dll`、`libwinpthread-1.dll` も確認する。欠落・空ファイル・同名フォルダーがあれば
リリースビルドは失敗する。`build.rs` のコピー警告だけで配布可能とは判断しない。
この検査は同梱ファイルの存在確認であり、DLLのロード成功やクリーンなPCでの起動確認は別途必要。

既定variantは`portable`。Vosk、Vulkan版Whisper、CPUフォールバックを含む。
用途に応じて次のvariantを明示できる。

```console
cargo xtask release-build --variant portable
cargo xtask release-build --variant minimal
```

- `portable`: Voskを含む本体 + Vulkan Whisper worker + 汎用CPU Whisper worker + AVX2 CPU Whisper worker。Vulkanを利用できなければ、CPU機能に応じたworkerへ自動降格する。
- `minimal`: Voskのみ。Whisperを使わない軽量構成。

一般配布パッケージの入口は必ず`portable`とする。GPUなしの環境でも本体を起動でき、
VoskとCPU Whisperを選択できることを配布要件とする。本体はwhisper.cppやVulkanへlinkしない。
Vulkan workerの初期化またはpre-warmに失敗した場合はCPU workerへ自動降格する。

CPUの高速版 `vac-stt-worker-cpu-avx2.exe` はSSE4.2・AVX2・FMA・F16Cが利用可能な場合だけ候補に入り、
その後に汎用版 `vac-stt-worker-cpu.exe` を試します。高速版が未同梱でも汎用版を使用できます。
配布ビルドは `GGML_NATIVE=OFF` と命令セットを明示し、開発PCのAVX-512等を自動採用しません。
Vulkan版に含まれるCPU処理にも汎用設定を使います。旧ビルドのキャッシュを混用しないよう、
Windowsの短いtargetを汎用版 `b1`、AVX2版 `a2`、Vulkan版 `v1` に分けます。
この構成の採用だけで配布検証が完了するわけではありません。生成された命令設定、各workerの
保存音声認識、対応CPUでの起動・速度を確認してから配布します。明示指定された独自workerの互換性は別途確認が必要です。

旧オプション`--voice-whisper`は`portable`の別名として当面利用できる。
依存関係を更新せずGUIだけ再ビルドする場合は `--skip-npm-ci` を指定する。

リリースビルドでは `VITE_UN_DESIGN_PREVIEW=0` を指定し、未採用の比較UIを含めない。
配布用ビルドへ開発端末固有の命令や設定が混入しないよう、非空の`RUSTFLAGS`、
`CARGO_ENCODED_RUSTFLAGS`、target別Rust flags、`CL`／`_CL_`、`CFLAGS`／`CXXFLAGS`、
`CMAKE_*`／`GGML_*`／`WHISPER_*`等の外部指定がある場合は開始前に停止する。
エラーに示された変数をその実行環境から外して再実行する。変数の値はログへ出さない。
成果物の場所は`CARGO_TARGET_DIR`ではなく`--target-dir`で指定し、`VULKAN_SDK`はそのまま利用できる。
これは環境変数の検査であり、Cargo設定ファイルや独自ツールチェーンの互換性監査を代替しない。
GUI出力の `unvac-ui-build.json` に通常版／比較版とHTML・JavaScript・CSSのSHA-256を記録し、
本体ビルド前に照合する。`--reuse-gui-dist` はこの記録がある通常版だけを再利用できる。
古い出力で記録がない場合、比較版の場合、記録後にファイルが変わった場合は再ビルドする。
記録にないHTML・JavaScript・CSSが出力フォルダー内に残っている場合も拒否する。
古い比較画面を混在させず、通常UIを再ビルドすること。シンボリックリンクを含む出力も再利用しない。

```console
cargo xtask release-gui-check gui/dist
```

この確認は画面の種類と記録済みファイルの一致を検査する。操作性・全機能・最新ソースとの一致や
ネイティブ起動の合格を意味しない。実行してもアプリ・ブラウザー・音声デバイスは起動しない。

Windows配布物には、Vosk本体および確認したGCC/MinGWランタイムの通知を`licenses/`へ同梱する。
`Vosk-0.3.45.source.json`に出典と通知・DLLのハッシュを記録し、通知の欠落・変更、
記録と異なるDLLをビルド時に拒否する。`VOSK_LIB_PATH`や`VOSK_WIN64_VERSION`で別のDLLを使う場合も、
その実物に対応する通知と出典を確認してから記録を更新する。ファイル一致の検査だけで
全依存ライブラリーの配布条件を確認したことにはならない。

フロント側の通知は`collect-frontend-notices.mjs`で通常GUIのソースマップと`app.css`の
直接参照から収集し、`THIRD-PARTY-FRONTEND.txt`と出典ハッシュの`frontend-notices.json`を
配布先の`licenses/`へコピーする。収集結果はtarget配下の専用フォルダーに残る。
生成物の識別記録・依存固定ファイル・CSS入力・通知本文の一致を本体ビルド前後に確認する。
再利用ビルドでもNodeと対応するインストール済み依存が必要。画像などの公開資産と
ソースマップ外の組込コードはこの自動収集の範囲外で、別途確認する。


複数variantを反復開発する場合は、variantごとに`--target-dir`を固定するとRust本体のcacheを維持できる。
Windowsでは各workerを`C:\u\<repo-hash>`配下の短い専用targetへ分離して構築する。CMake/MSVCの長い一時pathを避けつつnative依存のcacheを維持するため、本体だけの変更ではwhisper.cppを再構築しない。

```console
cargo xtask release-build --variant portable --target-dir target/build-portable --skip-npm-ci
cargo xtask release-build --variant portable --target-dir target/build-portable --skip-npm-ci
```

Vulkan SDKを持たない開発環境では通常の`cargo build`を使えるが、一般配布パッケージはVulkan SDKを用意した環境で`portable`を生成する。

## STT評価

本体やGUIを再構築せず、配布用と同じworkerビルド処理を確認する場合は、出力先を明示します。

```console
cargo xtask release-worker-build cpu target/worker-review-baseline
cargo xtask release-worker-build cpu-avx2 target/worker-review-avx2
cargo xtask release-worker-build vulkan target/worker-review-vulkan
```

指定先に同名workerがあれば上書きせず停止します。短い専用targetで構築し、命令設定の検査後に
指定先へコピーしてSHA-256を表示します。ビルドだけではworkerの認識動作やGPU動作は確認されません。

配布用CPU workerの単体確認では、音声デバイスを開かず保存済みWAVをIPCへ送れます。
Pythonの`msgpack`が必要です。入力はmono・PCM16・16kHzに限り、変換やVADによる切り出しは行いません。

```console
python scripts/check-stt-worker-offline.py --worker <worker.exe> --model <ggml-small.bin> --audio <sample.wav> --reference "正解文" --output <evidence.json>
```

既存の証跡ファイルは上書きしません。workerのSHA-256、音声のSHA-256、入力時間、認識結果と処理時間、
終了コードを記録します。比較時はモデル・入力音声・スレッド数（この試験では4）を揃えます。
1回の測定は実配信での継続性能や、別CPUでの起動成功の証明にはなりません。

### 入出力デバイスと再生

```powershell
cargo xtask stt-eval devices
cargo xtask stt-eval output-devices
cargo xtask stt-eval play --input tmp/stt/reference.wav --device "CABLE Input"
```

`play`は保存済みWAVを指定した再生デバイスへ直接流す。VB-CABLEを使うprofileでは、
一度収録した同じ発話をVACの実運用STT経路へ繰り返し入力できるため、既定デバイスやVCの状態を変更せず回帰試験できる。

既定マイクから比較用WAVを録音する。

```console
cargo xtask stt-eval devices
cargo xtask stt-eval record --output .local/stt-eval/01.wav --seconds 8
```

`devices`は利用可能な入力デバイス、既定デバイス、既定formatをJSON表示する。
既定入力以外を使う場合は`record --device "デバイス名"`を指定する。大文字小文字を無視した完全一致を優先し、
一意であれば名前の一部でも選択できる。同名デバイスが複数ある場合は列挙結果の`index`を
`record --device-index <n>`へ指定する。indexはその実行時の列挙順であり、永続的なデバイスIDではない。

既定では3秒のカウントダウン後に指定秒数の発話時間へ入る。通知遅延で冒頭を失わないよう音声capture自体はカウントダウン前から行い、
出力WAVの長さはカウントダウンと`--seconds`の合計になる。発話後にも約1秒の無音を残す。
出力は16 kHz、mono、16-bit PCM WAV。カウントダウンは`--countdown-seconds`で変更できる。

STTモデルを読み込まず、録音レベルと簡易VADの結果だけを確認できる。

```console
cargo xtask stt-eval inspect --input .local/stt-eval/01.wav
```

`peak_dbfs`、全体の`rms_dbfs`、20 ms frameの`max_frame_rms_dbfs`、
VAD閾値を超えたframe数を出力する。認識精度を比較する前に録音条件が揃っているか確認する。

同じWAVを現行Voskへ入力し、partial/finalの音声時刻、処理時間、認識結果、CERを記録する。

```console
cargo xtask stt-eval vosk \
  --input .local/stt-eval/01.wav \
  --reference "まずは右側を確認します" \
  --json-out .local/stt-eval/01.vosk.json
```

`--model`にはローカルディレクトリまたはVoskモデルIDを指定できる。既定値は`vosk-model-small-ja-0.22`。
評価は実時間待機せずにWAVを200 ms単位で投入するため、`audio_ms`は発話タイムライン、`processing_ms`は実処理時間を表す。
`vad`は20 ms frame、-45 dBFS、100 ms連続発声による発話区間の簡易推定であり、
`first_partial_latency_ms`は発話開始から最初のpartial、`final_latency_ms`は発話終了から最後のfinalまでの差分。
複数finalへの分割は`first_final_audio_ms`、`last_final_audio_ms`、`final_count`で確認する。
後続の共通VAD実験ではこの推定条件も比較対象とする。

同じWAVのVAD発話区間をWhisperへ入力する。`xtask`が`voice-whisper` featureを自動で有効にする。

```console
cargo xtask stt-eval whisper \
  --input .local/stt-eval/01.wav \
  --model small \
  --reference "まずは右側を確認します" \
  --json-out .local/stt-eval/01.whisper-small.json
```

既定では推定発話区間の前後500 msを含め、言語`ja`、Greedy `best_of=5`、最大8 threadで処理する。
`processing_ms`はモデル読込後の推論時間、`realtime_factor`は推論時間を入力した発話音声長で割った値。
本コマンドは配布workerを経由せず、評価用featureでCPU版whisper.cppを直接呼ぶ。
Vulkan / CPU workerの性能確認には、リリース成果物を常駐起動したまま同一WAVのprefixを反復するworker probeを使う。

```console
cargo xtask stt-eval worker \
  --input .local/stt-eval/01.wav \
  --worker target/release/vac-stt-worker-vulkan.exe \
  --model small \
  --reference "まずは右側を確認します" \
  --json-out .local/stt-eval/01.worker-vulkan.json
```

既定では実運用と同じ`partial_interval_ms=650`、`partial_min_audio_ms=1300`、VAD設定を使う。各prefix推論は`events`へ
入力上の`audio_ms`、推論へ渡した`inference_audio_ms`、`inference_ms`、`emitted`、認識文を記録する。`partial_deadline_miss_count`が0なら各途中推論が
650 ms周期内に完了している。CPU比較では`--worker target/release/vac-stt-worker-cpu.exe`だけを変える。
直接評価用`stt-eval whisper`の結果JSONは`backend=cpu`となり、配布workerの測定値ではない。

### Web Speech API

インストール済みChrome、Edge、Firefoxについて、マイクを使わずWeb Speech APIの能力を調べる。

```console
cargo xtask stt-eval web-speech \
  --browser all \
  --json-out .local/stt-eval/web-speech-capabilities.json
```

`SpeechRecognition`、interim、`continuous`、`processLocally`、`available()`、
`install()`、`start(audioTrack)`を調べる。`available()`は日本語の
`command`、`dictation`、`conversation`を端末内限定とリモート許容に分けて記録する。
このコマンドは言語パックをインストールせず、マイク権限も要求しない。

保存済みWAVの一部をWeb Audioで`MediaStreamTrack`へ変換し、実認識も比較できる。
`--start-ms`と`--end-ms`には同じWAVを`inspect`またはWhisper評価した発話区間を使う。

```console
cargo xtask stt-eval web-speech \
  --browser chrome \
  --audio .local/stt-eval/01.wav \
  --start-ms 9640 \
  --end-ms 11990 \
  --reference "まずは右側を確認します" \
  --json-out .local/stt-eval/01.web-speech-chrome.json
```

音声の前に2秒の無音を置いて認識サービスの開始を待ち、`continuous=true`で
partialとfinalを記録する。`summary`には発話開始から最初のpartial、
発話終了から最後のfinal、認識本文、CERを出す。
これは外部ブラウザーの評価であり、Tauriが利用するWebView2の動作保証にはならない。

## AI アバター制御

AIによるポーズ選択は、起動済みの隔離評価用VACに対して次の合成fixtureで確認できる。

```console
cargo xtask ai-turn-eval --fixture tests/fixtures/ai-turn/avatar-pose.synthetic.ja.json --dry-run
cargo xtask ai-turn-eval --fixture tests/fixtures/ai-turn/avatar-pose.synthetic.ja.json --base-url http://127.0.0.1:46081 --output target/ai-turn-eval/avatar-pose.json
```

会話fixtureの各caseには任意の`actor`を指定でき、Control ingressの発話元メタデータとして送る。省略時は従来の`local`。`speech_includes`は回答本文に必要な文字列の配列で、発話なしも不合格となる。これは`episode_memory_includes`（要約への保持）とは別の判定。人物名を本文へ埋め込まずに発話元の保持を調べる例は`tests/fixtures/ai-turn/source-actor.synthetic.ja.json`。文字列一致だけで人物関係の意味全体を保証するものではない。

実行には`kaltsit` personaが`open_hands`を許可する評価設定が必要。`avatar_pose`期待値は`any`（既定）、`required`、`forbidden`。`required`は要求とemitの両方を必要とし、エラーがあれば期待値にかかわらず不合格になる。レポートは要求数・emit数・エラー数と、各イベントのpreset・targetを保持する。emitはAvatarPose datumの発行であり、後段Flowgraphの成功やRendererでの適用を証明しない。本文もTurn Managerの生のspeakなので、最終字幕・音声とは別に照合する。

fixtureは明示的な両手の動作依頼、通常の紹介、沈黙依頼の3例。通常の紹介ではポーズを強制せず、沈黙時はポーズを禁止する。実配信の発言を転記したものではない。

```console
cargo xtask avatar-control prepare --streaming-persona --enable-publish --force
cargo xtask avatar-control start --validate-only
cargo xtask avatar-control preflight
cargo xtask avatar-control audit --json
cargo xtask avatar-control smoke
cargo xtask avatar-control send --preset smile --print-only
cargo xtask avatar-control send-sequence --sequence smile-then-neutral --print-only
cargo xtask avatar-control reaction --validate-all
cargo xtask avatar-control detect --json
cargo xtask avatar-control find-executables --json
cargo xtask avatar-control wait-renderer
cargo xtask avatar-control renderer-status --pid <PID>
cargo xtask avatar-control renderer-screenshot --pid <PID> --path tmp/avatar/renderer.png
cargo xtask avatar-control renderer-screenshot --pid <PID> --count 60 --interval-ms 100 --path tmp/avatar/blink-burst
cargo xtask avatar-control start-supervisor
cargo xtask avatar-control live-smoke --sequence smile-then-neutral
cargo xtask avatar-control new-live-note
cargo xtask avatar-control start-live-test
```

`detect` は `sysinfo` で process と引数を取得する。Renderer の `profile_hint` と `unmfz_key_hint` は command line からの best-effort 推定であり、確定的な discovery contract ではない。

`renderer-status` は受信・適用数などのtelemetry確認、`renderer-screenshot` はRenderer自身のoffscreen captureによる視覚証跡取得に使う。
既定は1枚だけ取得する。瞬きや遷移を調べる場合は`--count`と`--interval-ms`を指定し、同じZenoh sessionで連続画像を取得する。複数枚の場合、`--path`は出力ディレクトリとして扱う。

`prepare` は local conf と Flowgraph を生成する。既存出力は `--force` なしでは上書きしない。`--enable-publish` を付けない場合は `dry_run.value = true`。

`reaction` は `vac-motion` の canonical AvatarIntent mapperを直接使って preset を検証する。PowerShell と Rust の別々の validation rule は持たない。

## 配信前チェック

起動中VACを変更せず、現在モード、Flowgraph、AI、音声認識、Twitch IRC JOIN、OBS字幕テンプレート、字幕`/output`契約をまとめて確認する。

```console
cargo xtask stream-check
```

既定では`streaming`モードでない場合をblockする。設定調整中にdailyモードのまま他の検査だけ行う場合は`--allow-non-streaming`、字幕channelが異なる場合は`--channels user,ai-synth`のように指定する。

このcommandは配信へ音を出さない。TTS nodeが構成済みでも実合成・再生前は`Attention`として残り、起動後に再生完了telemetryが届けば`Ready`になる。字幕も`/output`のchannel keyだけでは成功扱いせず、指定した各channelに実データが届くまでは`Attention`。`Ready`は「設定がある」だけではなく、Twitch workerのJOIN、TTS再生完了、字幕データ到達などの実行証拠を確認できた項目に限る。

次回配信の縦断証拠は`--evidence-out`でJSONへ保存する。snapshot、字幕channelの到達、最大4000件のControl Event、Quick Effects状態を同じ時点で記録する。サーバー側は4096件を保持するため、音声partialやprocessor発火が続く10分程度の試験でも開始時の証拠を残せる。通常GUIの履歴取得量は従来どおりで、証跡保存時だけ大きな履歴を取得する。会話本文、音声認識文、AI発話本文、episode memoryは文字数だけ残して伏せ、Bearer tokenやOBSパスワードは記録しない。既存ファイルは`--force`なしでは上書きしない。

```console
cargo xtask stream-check --evidence-out target/stream-evidence/preflight.json
cargo xtask stream-check --evidence-out target/stream-evidence/post-stream.json
```

10分以上の実配信試験後は、preflight証跡を比較起点としてpost-streamを検査する。既定では`user` / `ai-synth`字幕、音声認識partial/final、AI判断、発話と沈黙の両方、会話終端、Twitch実コメント、`assistant-output/main::speak` / `twitch-chat/main::speak`の再生完了を必須とする。AIの連続発話判断が4件以上なら文脈を人が確認する`Attention`とする。TTS失敗、Control Eventのlagged、`playback_started`から`completed`までの音声重複、履歴上限によるpreflight以降の証拠欠落はblockする。

```console
cargo xtask stream-check --expect-live-from target/stream-evidence/preflight.json --expect-live-minutes 10 --evidence-out target/stream-evidence/post-stream.json
```

FlowgraphのTTS node IDが異なる場合だけ`--expect-tts-nodes package/file::node,...`、字幕channelが異なる場合は既存の`--channels`で明示する。`--expect-live-minutes`と`--expect-tts-nodes`は`--expect-live-from`なしでは受理しない。

チャネルポイント交換直後は期待する報酬と外部BOS到達先を指定する。`TwitchEventSubReceived`の実受信に加え、同じreward IDを持つ`ChannelDatum`が全指定destinationへ届かなければblockする。既定は`effect`。AI observeとAvatarIntentは公開channelではないため、Studio runtime trace、AI decision、Renderer受信・適用増分を有人release gateで照合する。

```console
cargo xtask stream-check --expect-reward-id <reward-id> --expect-reward-title "報酬名" --expect-reward-channels effect --evidence-out target/stream-evidence/after-channel-point.json
```

これによりIRC上のbot通知や無関係な演出を成功と誤認せず、EventSubとOBS effectを同一報酬IDで照合できる。AI入力とAvatarIntentは同一exec分岐のruntime traceと外部到達を別途記録する。イベント本文や会話内容は従来どおり証跡で伏せられる。

## Flowgraph検証

任意のFlowgraphディレクトリを、VAC本体と同じloaderで明示検証する。音声・Twitch・AI出力を
含む構成ではstreaming contractも同時監査し、経路欠落が`blocked`ならexit 1になる。

```console
cargo xtask flowgraph-check --root flowgraph.local
cargo xtask flowgraph-check --root "C:\Users\name\AppData\Roaming\UN Virtual Avatar Connect\profiles\profile-id\flowgraph.local" --require-streaming-contract
```

検査は`target/release/virtual-avatar-connect-cli`、次に`target/debug/virtual-avatar-connect-cli`を直接使う。xtask内部からCargoを再帰起動しないため、検査だけで本体の大規模再ビルドは発生しない。別の成果物を使う場合は`--cli <path>`を指定する。一般Flowgraphのloadではcontractは情報表示だけ、配信用profileは`--require-streaming-contract`で`blocked`をexit 1にする。

Git管理外の個人設定は通常の`cargo test`から暗黙に読み込まない。開発用設定やAPPDATA管理プロファイルを確認するときだけ、このcommandへ対象を明示する。

## 文書生成

```console
cargo xtask docs bless-node-catalog
```

Flowgraph registry から `docs/manual/node-catalog.md` を再生成し、一致テストを実行する。

## english2kana ONNX

日本語TTS向け英単語発音モデルを、不変revisionとSHA-256で取得し、隔離したKeras環境からONNXへ変換する。

```console
cargo xtask model english2kana prepare
cargo xtask model english2kana convert
cargo xtask model english2kana verify
```

`prepare` はモデル、学習データ、JMdict、CMUdict、変換規則とライセンスの取得・検査だけを行う。`convert` は固定長LSTMを展開したONNX生成とKeras/ONNXの代表語一致検証に加え、実行用語彙TSVを生成する。`verify` は生成済み資産のrelease gate。生成物は `.vendors/english2kana/` に置かれGit管理外となる。通常のUNVAC build/runtimeはPythonを使わない。

## OCR

既存 OCR command も同じ入口から実行する。

```console
cargo xtask paddleocr-native plan
cargo xtask ocr-onnx prepare-paddle --preset ppocrv5-server
cargo xtask ocr-onnx convert-paddle --preset ppocrv5-server
cargo xtask ocr-onnx eval --model <path>
```


### 配布候補ZIP

`release-files-check`が通った成果物は、次の手順で必要ファイルだけ梱包できます。

```powershell
python scripts/package-release-candidate.py --source target/build-portable/release --output target/UNVAC-portable-candidate.zip
```

ビルドディレクトリー全体は圧縮しません。検査表に列挙されたファイルと検査表そのものだけを入れ、圧縮後に全内容を展開読取してサイズ・SHA-256を再照合します。同名ZIPは上書きしません。成功時は隣に `.verification.json` を保存します。失敗時のZIPには成功記録がないため、配布に使わず原因を確認してください。この確認は導入・実動作・公開承認の代わりではありません。

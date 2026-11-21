# UNVAC v2 active managed profile release gate

この文書は、実配信で使用している構成の受入確認を定義する。一般ユーザーにAI、特定の
キャラクター、特定の音声エンジンの使用を要求するものではない。AIなしの初回導入や
UNAvatar・UNMotionを含むUI移行の公開判定は、[シリーズ共通の公開確認](un-series-release-gate.md)
も併せて行う。個人環境への候補版導入と一般公開の承認は別に扱う。

UNVAC v2をrelease-readyと判定するには、Dr.USAGIのactive managed profileで、offline検証と
有人実機検証の両方を同一revision・同一profileで完了しなければならない。loader成功、fixture成功、
個別nodeのself-testだけではrelease gateを通過しない。

## 1. Offline gate

- profile全体を日時付きbackupし、資格情報、AI persona、OAuth参照、辞書、resource、Avatar pose catalog、
  Twitch reward設定が保持されている。
- activeな`*.flowgraph.toml`に`flowgraph.channel.emit`、
  `flowgraph.ingress.channel_subscribe`、`flowgraph.library.*`、`[meta].library_uses`が0件。
- managed profile移行は先に`--dry-run`を通し、profile全体backup後のtransactional directory swapで
  適用する。失敗fixtureでactive tree不変性を検証する。
- `cargo xtask flowgraph-check --root <flowgraph_dir> --require-streaming-contract`がloader診断0件かつstreaming contract
  `ready`。型、未解決参照、循環、重複entryを検出した場合はblock。
- `virtual-avatar-connect-cli <conf.toml> --validate-conf`がconf、AI参照、AvatarLink、Flowgraphを
  副作用なしで検証する。
- Studio E2Eがprofile overview、module instance、portal、active/inactive、runtime trace、子Canvas
  drill-down、save/reload/import/exportを検証する。

## 2. 有人実機 gate

次を同一active managed profileで記録する。未実施、別profile、別revision、単体self-testの代用はblock。

- **daily mode**: OBS等streaming専用managed appを起動せず、マイク入力、AI会話、`user` /
  `ai-synth`字幕、AI TTSが継続動作する。
- **streaming mode**: Twitch実PRIVMSG、字幕、COEIROINK chat読み上げ、AI会話、assistant TTS、
  speech floorが競合せず、欠落・二重再生なしで動作する。
- **AI Avatar**: 瞬き、lip sync、名前付きpose、AvatarIntent reactionがUNMF/Z経由で
  Kalt'sit Pseudo Rendererへ到達し、neutralへ復帰する。
- **channel points**: 対象rewardの実交換について、同一時刻・同一reward IDでAI入力、BOS、
  OBS effect、Avatar reactionの到達を確認する。IRC bot通知は交換成功の証拠にしない。
- **lifecycle**: daily/streaming切替、Flowgraph reload、VAC再起動後も同じprofileとmodeが復元される。
  VACが所有していないOBS、Renderer、Supervisor、Capturer等を停止しない。
- **Studio**: profile overviewから上記各経路を辿り、故障をinactive、runtime trace、diagnosticsから
  特定できる。
- **長時間配信**: 10分を最低自動判定窓とし、実際の配信時間を通じて過剰発話、TTS混線、chat欠落、
  STT停止、Control Event欠落、CPU/GPU過負荷が許容範囲内である。

## 3. 証跡と判定

配信前に`cargo xtask stream-check --evidence-out <preflight.json>`、終了前に
`cargo xtask stream-check --expect-live-from <preflight.json> --expect-live-minutes 10
--evidence-out <post-stream.json>`を実行する。channel pointsは`--expect-reward-id`、
`--expect-reward-title`、明示接続後の期待routeを併記する。

配信前記録の `report.base_url` が今回の接続先と異なる場合、比較を拒否する。
接続先のない古い記録も、今回の配信前記録として使わない。同じURLでもプロセスや
設定が入れ替わる可能性があるため、URL一致だけでは同一revision・同一profileの証明にならない。
実行ファイルのハッシュ、実行中PIDと起動時刻、設定・グラフの識別記録を併せて確認する。

開発側の記録補助として `scripts/capture-stream-identity.py` を用意している。
`--base-url`、`--executable`、`--conf`、`--output` を明示し、配信前後の
`stream-check` と同じ時点に実行する。ローカルの稼働PID・起動時刻・実行ファイル・
現在の設定パスを照合し、EXE・設定・グラフは内容ではなくハッシュを保存する。
既存出力の上書き、別プロファイル・別EXE、収集中の変更を拒否する。
認証が必要なControl APIには `VAC_CONTROL_TOKEN` を環境変数で渡す。
アプリの起動・終了・設定変更やTwitch投稿は行わない。
これはディスク上のファイルの識別であり、未適用編集がないことや実際の配信成功は
別途確認する。外部辞書・関連資産・認証ファイルの内容は収集対象に含めない。

Control API `GET /api/v1/control/preflight/self-test`の`v2_release_gate`は、offline contractと
未完了の有人項目を分離して返す。有人実機記録が正本へ反映されるまでは`release_ready=false`。

最終判定はlive run noteへrevision、profile ID、mode、開始終了時刻、preflight/post-stream証跡、
各項目のPass/Blockと理由を記録する。全項目Passになるまでv2 release-readyと表現しない。

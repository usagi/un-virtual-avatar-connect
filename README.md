# U.N. Virtual Avatar Connect 2.0

VACは、声を字幕にして翻訳したり、チャネポ交換でアバターをズームしたり、ゲーム画面を見て話すAIを共演者にしたりできる、Windows向けの統合配信支援アプリです。使いたい用途から始めて、必要な機能をFlowgraph Studioで追加できます。

[公式サイト](https://usagi.github.io/un-virtual-avatar-connect/) · [Windows x64版のダウンロード](https://github.com/usagi/un-virtual-avatar-connect/releases/latest) · [使い始める](docs/manual/quickstart.md)

1. Releasesの配布ZIPをすべて展開し、`virtual-avatar-connect-desktop.exe` を起動します。
2. 通知とクリップを使うなら「Twitch配信支援（AIなし）」、共演者を使うなら「配信AIと始める」を選び、「作成して開始」を押します。
3. 用途に応じてTwitchやOBSを接続します。AIなしの構成はOpenAIの契約やAPIキーなしで使えます。

## 開発・不具合の報告

ソースからビルドする方は[開発ガイド](docs/development-guidelines.md)へ。

不具合やご要望は [Issues] へどうぞ。動作環境と再現手順、期待した動作を添えてください。設定やスクリーンショットを共有する際は認証情報を伏せてください。

[Issues]: https://github.com/usagi/un-virtual-avatar-connect/issues

## v1 → v2 移行

v0.10.0 で **V1 processor 層は撤去**されました。`[[processors]]` は TOML に残っても無視
されます。Flowgraph への移行は [`docs/manual/v1-to-v2-migration.md`](docs/manual/v1-to-v2-migration.md)
を参照してください。主要な変更点の一覧は [`CHANGELOG.md`](CHANGELOG.md) にあります。

## Roadmap (抜粋)

v2 は **Flowgraph + Control API + GUI Runtime Cockpit** を中心にしています。

- `/api/v1/control/*` は GUI / 外部ツール向けの認証付き control plane です。
  - `snapshot` / `events` / pause / reload / restart / profile edit
  - Flowgraph tree / node catalog / diagnostics / fragment copy-paste / ZIP import-export
  - Managed App / OBS Browser Source / Quick Effects / AI persona config
  - UN App API over Zenoh (UNAA/Z) discovery / command / diagnostics
- GUI は `gui/` の Svelte 5 + Vite 実装です。Now、Live、Modes、Flowgraph Studio、Resources、Observability、Settings を持ちます。
- Motion は **UNMF / UNMF/Z** を core data plane とし、VMC は互換 adapter として扱います。
- U.N. シリーズアプリ連携の control plane は **UN App API over Zenoh (UNAA/Z)** です。

主要ドキュメント:

- 開発引き継ぎ（2026-09-13）: [`docs/maintenance/handoff-2026-09-13.md`](docs/maintenance/handoff-2026-09-13.md)
- 設定リファレンス: [`docs/manual/conf-reference.md`](docs/manual/conf-reference.md)
- GUI 開発手順: [`gui/README.md`](gui/README.md)
- UNAA/Z protocol: [`docs/manual/un-app-api-over-zenoh.md`](docs/manual/un-app-api-over-zenoh.md)
- Motion / VMC / UNMF 方針: [`docs/roadmap/v2-vmc-and-restructure.md`](docs/roadmap/v2-vmc-and-restructure.md)
- Control API 動作確認用 [Bruno](https://www.usebruno.com/) collection: [`dev/bruno/vac-control-api/`](dev/bruno/vac-control-api/README.md)

v2 は `[[processors]]` から Flowgraph へ移行済みです。古い設定からの移行は [`docs/manual/v1-to-v2-migration.md`](docs/manual/v1-to-v2-migration.md) を参照してください。

## LICENSE

Virtual Avatar Connect は **MIT ライセンス**のオープンソースソフトウェアです。

- [MIT License](LICENSE)

third-party なライブラリ・外部サービスはそれぞれのライセンス/利用規約に従います（主要な依存は `Cargo.toml` 参照）。

## Contributing

バグ報告・機能要望・PR はいずれも [Issues] / Pull Requests からどうぞ。設計方針や大きな破壊的変更の相談は Issue 起票 → 合意 → PR の順が行き違いが少なく助かります。

## Author

- Usagi Ito / USAGI.NETWORK
  - [Twitch](https://www.twitch.tv/usaginetwork)
  - [X(Twitter)](https://twitter.com/usagi_network)
  - [BOOTH](https://usagi-network.booth.pm/)

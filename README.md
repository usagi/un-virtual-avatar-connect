# U.N. Virtual Avatar Connect 2.0

VACは、声を字幕にして翻訳したり、チャネポ交換でアバターをズームしたり、ゲーム画面を見て話すAIを共演者にしたりできる、Windows向けの統合配信支援アプリです。使いたい用途から始めて、必要な機能をFlowgraph Studioで追加できます。

[公式サイト](https://usagi.github.io/un-virtual-avatar-connect/) · [Windows x64版のダウンロード](https://github.com/usagi/un-virtual-avatar-connect/releases/latest) · [使い始める](docs/manual/quickstart.md)

1. Releasesの配布ZIPをすべて展開し、`virtual-avatar-connect-desktop.exe` を起動します。
2. 通知とクリップを使うなら「Twitch配信支援（AIなし）」、共演者を使うなら「配信AIと始める」を選び、「作成して開始」を押します。
3. 用途に応じてTwitchやOBSを接続します。AIなしの構成はOpenAIの契約やAPIキーなしで使えます。

[機能別マニュアル](docs/manual/index.md) · [v1からの移行](docs/manual/v1-to-v2-migration.md) · [更新履歴](CHANGELOG.md) · [開発ガイド](docs/development-guidelines.md)

## LICENSE

Virtual Avatar Connect は **MIT ライセンス**のオープンソースソフトウェアです。

- [MIT License](LICENSE)

third-party なライブラリ・外部サービスはそれぞれのライセンス/利用規約に従います（主要な依存は `Cargo.toml` 参照）。

## Contributing

バグ報告・機能要望・PR はいずれも [Issues](https://github.com/usagi/un-virtual-avatar-connect/issues) / Pull Requests からどうぞ。設計方針や大きな破壊的変更の相談は Issue 起票 → 合意 → PR の順が行き違いが少なく助かります。

## Author

- Usagi Ito / USAGI.NETWORK
  - [Twitch](https://www.twitch.tv/usaginetwork)
  - [X(Twitter)](https://twitter.com/usagi_network)
  - [BOOTH](https://usagi-network.booth.pm/)

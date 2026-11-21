# 開発ガイドライン

UNVAC v2 の開発と検証で使う基本ルール。

## 変更とコミット

- 変更は小さく焦点を絞る。
- 1 commit は 1 つの機能目的を原則とする。
- 文書とコードコメントは日本語を基準とする。
- テキストファイルは UTF-8、LF を使う。

## リポジトリ内補助コマンド

リポジトリ内で再現する生成、検証、実験、複数段階の orchestration は `xtask` に置く。

- 標準導線は `cargo xtask <command>` とする。
- PowerShell script は公式導線に追加しない。
- OS 固有処理が必要な場合も、可能な限り Rust 内の小さい platform adapter に閉じ込める。
- `.ps1` を採用するのは、Rust 実装より明確な優位性があり、文字コード、実行ポリシー、対象 platform を含む理由を文書化できる場合に限る。
- JSON / TOML は構造化 parser を使い、shell 固有の暗黙 encoding や文字列展開へ依存しない。

利用可能な補助コマンドは [`manual/xtask.md`](./manual/xtask.md) を参照する。

## ローカル確認

変更範囲に応じて Rust test、Flowgraph fixture、生成物一致検査を実行する。node catalog を変更した場合は次を実行する。

```console
cargo xtask docs bless-node-catalog
```

## Rust ビルド環境

ビルドには **Rust 1.100.0** を使用します。`rust-toolchain.toml` に従って rustup が選択します（最低対応バージョン: 1.100、プロジェクト内コードの Edition: 2024）。依存関係は `Cargo.lock` を維持し、検証時は `cargo check --locked` / `cargo test --locked` を使用してください。 Cargoの中間生成先 `build-dir` を実行ファイルの出力先と分離する場合は、`UN_BUILD_PROFILE_DIR` に実行ファイルを配置するディレクトリの絶対パスを指定してください。

## v2の構成

v2 は **Flowgraph + Control API + GUI Runtime Cockpit** を中心にしています。

- `/api/v1/control/*` は GUI / 外部ツール向けの認証付き control plane です。
  - `snapshot` / `events` / pause / reload / restart / profile edit
  - Flowgraph tree / node catalog / diagnostics / fragment copy-paste / ZIP import-export
  - Managed App / OBS Browser Source / Quick Effects / AI persona config
  - UN App API over Zenoh (UNAA/Z) discovery / command / diagnostics
- GUI は `gui/` の Svelte 5 + Vite 実装です。Now、Live、Modes、Flowgraph Studio、Resources、Observability、Settings を持ちます。
- Motion は **UNMF / UNMF/Z** を core data plane とし、VMC は互換 adapter として扱います。
- U.N. シリーズアプリ連携の control plane は **UN App API over Zenoh (UNAA/Z)** です。

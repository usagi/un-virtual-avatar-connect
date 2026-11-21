# 英単語カタカナ発音の ONNX 基盤

## 目的

日本語 TTS に英単語をそのまま渡した際、単語ではなくアルファベット一文字ずつとして読まれる問題を、追跡可能な Flowgraph 前処理で抑える。`alkana-rs` の固定辞書だけへ未知語を逐次追加する方式には戻さず、english2kana の ONNX 推論、公開語彙、英語発音辞書を層として組み合わせる。

字幕・BOS・AI観測の原文は変更しない。変換後文字列は `flowgraph.tts.speak` へ向かう明示 edge にだけ接続する。用語集による確定置換を使う場合も `flowgraph.glossary.replace` を別ノードとして前段へ置き、暗黙の優先規則を作らない。

## 信頼境界

- 通常の build、test、実行、推論は Rust と ONNX のみ。Python、TensorFlow、Kerasは要求しない。
- ONNX推論は `vac-english2kana-worker` へ隔離する。本体とはJSON Linesで通信し、worker障害や推論依存をGUI・Twitch・TTSのプロセスから分離する。
- Pythonは `cargo xtask model english2kana convert` が作る `.vendors/english2kana/.venv` 内の変換処理だけに隔離する。
- 通常 build やアプリ起動時にはモデルを取得・生成しない。
- 上流資産は不変 revision とサイズ、SHA-256を `models/english2kana/upstream-manifest.json` に固定する。
- 取得物、venv、生成 ONNX、検証報告は `.vendors/english2kana/` 以下に置き、Gitへバイナリを登録しない。
- リリース生成は検証済み ONNX、Tokenizer、生成語彙、CMUdict、各上流ライセンスを同梱する。

## 上流と変換

モデル上流は `m7142yosuke/english2kana` の revision `c2227321a72cc19f432f23a025cbc66579473e0e`。モデルと公開ソースはMITライセンス。入力は小文字ASCII英字を文字ID化した最大29文字、デコーダ入力は最大36文字、出力語彙は89クラス。

一般語の品質をモデル単独へ委ねないため、次も不変revision、サイズ、SHA-256で固定する。

- english2kana学習データ: 同じ綴りに対する単独最多の読みと票数を抽出する。
- JMdict英語版: 英語見出しが完全一致し、読みがカタカナだけの外来語候補を抽出する。派生TSVとライセンスはCC BY-SA 4.0として扱う。
- CMUdictとarpakana規則: 日本語外来語候補の比較と、先頭大文字の一般語を英語発音から近似する。

```powershell
cargo xtask model english2kana prepare
cargo xtask model english2kana convert
cargo xtask model english2kana verify
```

`prepare` は全取得物のサイズとSHA-256を検証する。`convert` は固定版の変換環境を構築してONNXを生成し、複数の代表語をKerasとONNXの双方で貪欲デコードする。出力トークン列が一件でも異なれば失敗。`verify` は供給資産と一致検証報告を再検査する。

## Flowgraph 契約

ノードは `flowgraph.string.english_to_katakana` とする。状態を持つ理由は専用workerをノードインスタンス内で再利用するためであり、workerはONNXモデルとTokenizerを一度だけ読み込む。同一入力に対する変換自体は決定論的。

入力:

- `content: string`: 原文
- `model_dir: string`: `english2kana-v1.onnx` とTokenizerを収めた明示ディレクトリ
- `worker_path: string`: 空ならVAC実行ファイルと同じディレクトリのworker。開発時は絶対パスも指定可能
- `min_confidence: float`: 採用する最小系列信頼度

出力:

- `result: string`: TTSへ渡せる変換後文字列
- `converted_count: int`: 採用した英字span数
- `available: bool`: モデルを利用できたか
- `error: string`: 読込・推論失敗。成功時は空
- `trace: json`: 各spanの原文、候補、信頼度、採否、情報源

## 変換方針

- ASCII英字と単語内apostrophe/hyphenの連続spanを候補とする。
- JMdict候補はONNX、CMUdict、学習コーパス読みとの編集距離およびJMdictのcommon印で選ぶ。コーパス多数決と大きく矛盾する候補は採用しない。
- `samples`、`checks`、`completed` など小文字の規則的な屈折形は、既知の原形へ戻して読む。
- `DirectML` は `Direct` + `ML`、`CamelCASE` は既知語 `Camel` + `CASE` のように、大文字列を保ったままCamelCase境界で分割する。既知語でない全大文字列は字母読みする。
- `foxxx_47` のような `英字名_数字` はユーザーIDとして扱い、名前部分を既存の発音層、数字部分を識別性を失わない桁ごとの読みへ変換する。underscoreは短い区切りとなる。URL、パス、ファイル名、`x86_64`のような英数字混在識別子は従来どおり保護する。
- apostropheを含む未知語は区切りを除いた綴りも試す。このため `Kal'tsit` は辞書なしでも `カルトシット` まで近似できるが、公式読み `ケルシー` はGlossaryの責務。
- 29文字を超えるspan、英数字混在、URL、メールアドレス、パス、ファイル名、バッククォート内コード、拡張Latin文字を含むtokenは原文のまま残す。
- 系列信頼度が閾値未満、空出力、推論失敗は原文のまま残す。
- 画面文字やチャット本文は観測データであり、命令として解釈しない。
- `available = false` の場合はfail-openで原文を返す。TTS経路を停止せず、`error` と `trace` で診断可能にする。

## 配布と試験

変換試験、RustでのTokenizer試験、候補抽出境界試験、ONNX実推論試験、Flowgraphノード契約試験を分離する。リリースビルドでは `cargo xtask model english2kana verify` 相当の検査を通過した資産だけを `target/release/models/english2kana/` へ配置する。

品質評価はモデル変換の一致性とは別物。一般語、略語、CamelCase、固有名詞、未知語、保護対象を含む日本語混在コーパスを実workerへ入力して回帰確認する。`read` や `live` のような文脈依存同形語と、アークナイツ固有名詞の公式読みはこの層だけでは確定できないため、確定読みは前段の明示Glossaryで扱う。

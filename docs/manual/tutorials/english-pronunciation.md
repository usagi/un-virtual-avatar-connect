# Tutorial: 英単語のカタカナ発音

日本語TTSが英単語をアルファベット一文字ずつ読む場合、`flowgraph.string.english_to_katakana` をTTS経路だけに挿入する。

## グラフ構造

```text
ingress.twitch:content ─┬─→ bos.output:content（原文字幕）
                        └─→ english_to_katakana:content ─→ tts.speak:text
```

字幕、AI観測、ログの原文は変換しない。確定した固有名詞の読みは `flowgraph.glossary.replace` を前段へ明示配線し、未知の一般英単語だけをモデルに任せる。

## モデルを準備する

リポジトリから開発実行する場合:

```powershell
cargo xtask model english2kana convert
```

取得元revision、サイズ、SHA-256を検査し、隔離した変換環境で `.vendors/english2kana/onnx/` を生成する。同時にenglish2kana学習データとJMdictから実行用語彙を生成し、CMUdictも配置する。通常のbuild、test、実行にPythonは不要。リリースビルドは検証済み資産を `target/release/models/english2kana/`、純Rust推論workerを `target/release/vac-english2kana-worker.exe` へ同梱する。

## ノード設定

```toml
[[nodes]]
id = "english_pronunciation"
feature = "flowgraph.string.english_to_katakana"
position = [420.0, 360.0]

[[edges]]
from = "twitch_in:content"
to = "english_pronunciation:content"

[[edges]]
from = "english_pronunciation:result"
to = "speak:text"
```

例は `model_dir = ""` と `min_confidence = 0.55` のポート既定値を使う。空の `model_dir` はVAC実行ファイルと同じディレクトリの `models/english2kana` を参照するため、desktopが作業ディレクトリを個人profileへ移しても同梱モデルを利用できる。値を変える場合はstring/float literalから各入力ポートへedgeを明示する。明示した相対パスは従来通りVACの作業ディレクトリ基準、絶対パスはそのまま使う。従来の既定文字列 `models/english2kana` を明示配線していた場合は、その配線を外すか空文字にすると同梱モデルの自動解決になる。明示したパスが見つからなくても別モデルへ自動置換しない。

`worker_path` は空ならVAC実行ファイルと同じディレクトリの同梱workerを使う。開発用workerを別の場所に置く場合だけ明示する。

`min_confidence` 未満、29文字超、英数字混在、URL、メール、パス、ファイル名、バッククォート内コード、拡張Latin文字、推論失敗は原文を維持する。モデルを読めない場合も `result` は原文を返し、`available = false`、`error`、`trace` へ理由を出す。

一般的な外来語はJMdict、学習コーパス、CMUdict、ONNXの候補を比較する。`DirectML` や `CamelCASE` はCamelCase境界で分割し、未知の全大文字部分だけを字母読みする。アークナイツ固有名詞の公式読みは推測させず、前段のGlossaryへ `Kal'tsit → ケルシー` のように登録する。

`foxxx_47` のような `英字名_数字` はユーザーIDとして認識し、`フォックス・ヨン・ナナ` のように読む。数字は数値化せず一桁ずつ読むため、`name_007` の先頭ゼロも維持される。`x86_64`のように名前側も英数字混在となる識別子は誤判定を避けて原文を維持する。

## 診断

- `converted_count`: 採用した英字span数
- `available`: ONNXモデルとTokenizerを利用できたか
- `trace`: 原文span、候補、信頼度、採否理由と採用層（JMdict、学習コーパス、CMUdict、ONNX、字母読みなど）

モデルの高信頼出力が常に望ましい固有名詞読みとは限らない。`GitHub`など期待読みが確定している語はGlossaryで明示する。

## 関連ノード

[`flowgraph.string.english_to_katakana`](../node-catalog.md#flowgraph-string-english-to-katakana) / [`flowgraph.glossary.replace`](../node-catalog.md#flowgraph-glossary-replace) / [`flowgraph.tts.speak`](../node-catalog.md#flowgraph-tts-speak)

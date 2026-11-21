# UNVAC v2 OCR backend 評価

評価日: 2026-08-02

## 結論

`flowgraph.ocr.recognize` の v2.0 既定backendには Windows Media OCR を採用する。
このノードの責務は `source` の画像を `text` へ変換するところまでであり、AI Observation、TTS、字幕へ自動接続しない。利用者が Flowgraph で必要な後段を明示的に接続する。

PaddleOCR ONNX probe は精度比較に使えるため experimental code として維持する。ただし現状のPP-OCRv5 server構成はmodel配布、後処理、常駐session、license bundleが製品化されておらず、v2.0既定にはしない。Paddle native C++ wrapperは認識stubのままで、Paddle Inference / OpenCV / native DLLを追加する優位を確認できなかったため製品統合を進めない。

## 評価環境と画像

- Windows 11 Pro 10.0.26200
- AMD Ryzen 9 7950X3D、32 logical processors、約64 GiB RAM
- Rust 1.94.1
- 英語合成画像: `win_ocr 0.1.3` の `sample.png`、正解 `Sample Text`
- 日本語合成画像: 同 `sample_ja.png`、正解 `サンプルテキスト`
- 日本語UI画像: `docs/image/m05/ss-ocr-result.png`。完全な正解transcriptがないためCERは算出しない

利用者提供のゲーム画面等はない。したがって、この評価は小さな合成画像とrepository内UI画像に限られる。

## 比較結果

| 条件 | Windows Media OCR | PaddleOCR ONNX probe |
|---|---:|---:|
| 追加導入 | Windows言語pack。runtime/modelの同梱なし | ONNX Runtime、det/cls/rec model、辞書が必要 |
| 英語合成 | `Sample Text`、CER 0/11 | ppocrv3-en detectorが0 box、pipeline失敗 |
| 日本語合成 | `サンプルテキスト`、CER 0/8 | `サンプルテキスト`、CER 0/8 |
| 初回 | 63 ms | process全体 1557 ms（session初期化を含む） |
| warm | 21 ms/回（20回平均） | detだけで517 ms。製品相当の常駐pipeline未実装 |
| 連続実行 | 21/21成功、同一結果 | 製品runtime未統合のため未評価 |
| memory | 21回後 +21.67 MiB | peak observed working set 958.09 MiB |
| CPU占有 | 21回でCPU 500 ms / wall約483 ms、約1.04 logical core相当 | CPU 6688 ms / wall 1557 ms、約4.30 logical cores相当 |
| model/runtime容量 | UNVAC追加配布 0 B | PP-OCRv5 server 3 model + 辞書 165.25 MiB。評価用venv等を含むlocal treeは約1.25 GB |
| license | `win_ocr` はMIT、Windows OCRはOS機能 | `ort` はMIT OR Apache-2.0。Paddle modelを含むrelease用license bundleは未確定 |
| 障害隔離 | blocking worker、panic捕捉、`on_error/result.err` | probeのみ。製品process内の障害隔離は未実装 |

時間、memory、CPUはdebug buildの単一host測定であり、hardwareを跨ぐ保証値ではない。Windows側は呼出しごとに`OcrEngine`を生成する現在実装、Paddle側は開発probeの単回processで測定した。

## 製品契約

- `source`: local path、`file:///`、HTTP(S)、base64 data URL。内容から画像形式を判定する
- Windows backendがPNG decoder固定のため、JPEG等は一時PNGへ安全に変換してから渡す
- `lang`: BCP-47。対応するWindows OCR言語packがなければerror
- `lines = true`: 空行を除いた認識行を `\n` で返す
- `lines = false`: 認識行を空白で連結した1行を返す
- Windows OCRが日本語文字間へ挿入する空白だけを除去する。英単語間等の空白は保持する
- source取得、画像decode、言語、backend、workerの失敗は `on_error` と `result.err(code = "ocr.recognize")` へ返す
- 非Windowsは従来どおりrecoverable error。アプリprocessを停止しない

## 再現コマンド

Windows backendの実画像反復評価:

```powershell
$env:UNVAC_OCR_TEST_IMAGE = 'C:\path\to\image.png'
$env:UNVAC_OCR_TEST_LANG = 'ja-JP'
cargo test -p virtual-avatar-connect processor::ocr::tests::evaluate_real_image_repeatedly --no-default-features -- --ignored --nocapture --test-threads=1
```

PaddleOCR ONNX probe:

```powershell
cargo xtask ocr-onnx eval --pipeline --image <image> --det-model .vendors/paddleocr-onnx/onnx/ppocrv5-server/det.onnx --rec-model .vendors/paddleocr-onnx/onnx/ppocrv5-server/rec.onnx --cls-model .vendors/paddleocr-onnx/onnx/ppocrv5-server/cls.onnx --dict .vendors/paddleocr-onnx/dicts/ppocrv5-server/ppocrv5_dict.txt --width 960 --height 960 --ctc-class-offset 2
```

Paddle native evalは `native-link` featureをxtaskだけが有効化する。通常の `cargo test --workspace` は未生成の `unvac_paddleocr.lib` を要求しない。

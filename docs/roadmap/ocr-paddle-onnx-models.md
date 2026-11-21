# OCR PaddleOCR ONNX Model Preparation

PaddleOCR ONNX 版の評価では、まず公式ドキュメントの PP-OCRv3 English example と同じ Paddle inference model を取得し、Paddle2ONNX で ONNX へ変換する。

公式 ONNX 取得手順は「Paddle inference model を取得して `paddle2onnx` で変換する」形で案内されている。UNVAC は変換済み ONNX を repository に含めない。

## Initial preset

`ppocrv3-en`:

- detector: `en_PP-OCRv3_det_infer.tar`
- recognizer: `en_PP-OCRv3_rec_infer.tar`
- angle classifier: `ch_ppocr_mobile_v2.0_cls_infer.tar`

`ppocrv5-server`:

- detector: `PP-OCRv5_server_det_infer.tar`
- recognizer: `PP-OCRv5_server_rec_infer.tar`
- angle classifier: `ch_ppocr_mobile_v2.0_cls_infer.tar`
- dictionary: `ppocrv5_dict.txt`

PP-OCRv5 server recognizer は公式 docs 上で Simplified Chinese / Traditional Chinese / English / Japanese などを単一モデルで扱う現行世代として案内されている。UNVAC の日本語/多言語評価本線はこの preset とし、旧 `japan_PP-OCRv3_mobile_rec` は後続の比較対象に留める。

取得先は PaddleOCR の公式 ONNX 変換ドキュメントまたは model list で使われている model URL と同じものを `xtask` に hardcode する。取得物は `.vendors/paddleocr-onnx/` 配下に保存し、repository には含めない。

## Commands

```console
cargo xtask ocr-onnx prepare-paddle --preset ppocrv3-en
cargo xtask ocr-onnx prepare-paddle --preset ppocrv5-server
```

このコマンドは以下を行う。

- model tar を `.vendors/paddleocr-onnx/downloads/` へ download
- recognition dictionary を `.vendors/paddleocr-onnx/dicts/ppocrv3-en/` へ download
- sha256 を `.vendors/paddleocr-onnx/SHA256SUMS.txt` へ記録
- `.vendors/paddleocr-onnx/paddle/ppocrv3-en/` へ展開
- `.vendors/paddleocr-onnx/README.local.md` へ次の変換手順を記録

Paddle2ONNX 変換は `.vendors/paddleocr-onnx/.venv` に隔離した Python 環境で行う。

```console
cargo xtask ocr-onnx convert-paddle --preset ppocrv3-en
cargo xtask ocr-onnx convert-paddle --preset ppocrv5-server
```

Windows / Python 3.11 では `paddle2onnx 2.1.0` + stable `paddlepaddle 3.3.1` が `paddle2onnx_cpp2py_export` の DLL load で失敗した。PaddlePaddle 自体の `paddle.utils.run_check()` と `onnx` import は成功しているため、失敗箇所は Paddle2ONNX wheel と PaddlePaddle binary の互換問題として扱う。

最新版優先のため、`xtask` は最新 `paddle2onnx` を使う。Windows / Python 3.11 では、PaddleX 最新ドキュメントの Windows 向け案内に従って nightly PaddlePaddle を使うだけでは不十分で、最新 nightly `paddlepaddle 3.4.0.dev20260407` でも同じ DLL procedure mismatch が出た。現時点で `paddle2onnx 2.1.0` と変換成功した組み合わせは `paddlepaddle-3.0.0.dev20250426-cp311-cp311-win_amd64.whl` の直 URL指定である。

`paddle2onnx==1.3.1` は変換確認済みの fallback に留め、UNVAC / U.N. ブランドの既定経路にはしない。

PP-OCRv5 の Paddle inference model は `inference.pdmodel` ではなく `inference.json + inference.pdiparams` 形式なので、`xtask` は両形式を検出して変換する。

既定で det / rec / cls を変換する。angle classifier は配信画面内の回転文字や上下逆の切り出しを補正するため、UNVAC では必要な構成要素として扱う。

`paddle2onnx 2.1.0` が生成する cls model は、reshape shape vector を作る `Concat` に scalar と vector が混在し、ONNX Runtime load 時に shape inference error になる。`xtask` は変換後に scalar input のみ `Unsqueeze(axis=0)` してから `Concat` する修復を適用する。修復後の cls model は `ort` probe で load 成功済み。

変換後は probe で load metadata を確認する。

```console
cargo xtask ocr-onnx eval --model .vendors/paddleocr-onnx/onnx/ppocrv3-en/det.onnx
cargo xtask ocr-onnx eval --model .vendors/paddleocr-onnx/onnx/ppocrv3-en/rec.onnx
cargo xtask ocr-onnx eval --model .vendors/paddleocr-onnx/onnx/ppocrv3-en/cls.onnx
```

実画像入力の評価では、PaddleOCR と同じ RGB → CHW → `(x / 255 - 0.5) / 0.5` 正規化を評価 binary 側で行う。
detector は ImageNet 系 mean/std、recognizer / angle classifier は `0.5 / 0.5` 系の正規化を使うため、評価 binary は `--decode-det` の単独指定時に detector profile を自動選択する。

```console
cargo xtask ocr-onnx eval --model .vendors/paddleocr-onnx/onnx/ppocrv3-en/rec.onnx --image .vendors/paddleocr-onnx/probes/hello.png --width 320 --height 48 --decode-rec --dict .vendors/paddleocr-onnx/dicts/ppocrv3-en/en_dict.txt
cargo xtask ocr-onnx eval --model .vendors/paddleocr-onnx/onnx/ppocrv5-server/rec.onnx --image .vendors/paddleocr-onnx/probes/hello.png --width 320 --height 48 --decode-rec --dict .vendors/paddleocr-onnx/dicts/ppocrv5-server/ppocrv5_dict.txt --ctc-class-offset 2
cargo xtask ocr-onnx eval --model .vendors/paddleocr-onnx/onnx/ppocrv3-en/cls.onnx --image .vendors/paddleocr-onnx/probes/hello.png --width 320 --height 48 --decode-cls
cargo xtask ocr-onnx eval --model .vendors/paddleocr-onnx/onnx/ppocrv3-en/det.onnx --image docs/image/m05/ss-ocr-result.png --width 960 --height 960 --decode-det --det-threshold 0.3 --det-min-area 16
cargo xtask ocr-onnx eval --pipeline --image docs/image/m05/ss-ocr-result.png --det-model .vendors/paddleocr-onnx/onnx/ppocrv3-en/det.onnx --rec-model .vendors/paddleocr-onnx/onnx/ppocrv3-en/rec.onnx --cls-model .vendors/paddleocr-onnx/onnx/ppocrv3-en/cls.onnx --dict .vendors/paddleocr-onnx/dicts/ppocrv3-en/en_dict.txt --width 960 --height 960 --max-boxes 8
cargo xtask ocr-onnx eval --pipeline --image docs/image/m05/ss-ocr-result.png --det-model .vendors/paddleocr-onnx/onnx/ppocrv5-server/det.onnx --rec-model .vendors/paddleocr-onnx/onnx/ppocrv5-server/rec.onnx --cls-model .vendors/paddleocr-onnx/onnx/ppocrv5-server/cls.onnx --dict .vendors/paddleocr-onnx/dicts/ppocrv5-server/ppocrv5_dict.txt --width 960 --height 960 --max-boxes 8 --ctc-class-offset 2
```

## Current probe result

2026-05-31 の Windows 開発環境では以下を確認済み。

- `ocr-onnx prepare-paddle --preset ppocrv3-en`: 公式 example model 3 点の download / tar extract 成功。
- `prepare-paddle` は English recognition dictionary (`en_dict.txt`) も `.vendors/paddleocr-onnx/dicts/ppocrv3-en/` へ取得し、checksum に含める。
- `ocr-onnx prepare-paddle --preset ppocrv5-server`: PP-OCRv5 server det / rec、共通 cls、`ppocrv5_dict.txt` の download / extract 成功。
- `paddle2onnx 2.1.0` + `paddlepaddle 3.0.0.dev20250426` で det / rec / cls の ONNX 変換成功。`paddle2onnx 2.1.0` は constant folding 時に `onnxruntime`, `onnx_graphsurgeon`, `sympy` を venv へ追加導入する。
- `ppocrv5-server` は `inference.json` 形式から det / rec 変換成功。server det は node 数が多く、CPU dummy inference は `960x960` で約 `517 ms`。
- cls は `paddle2onnx 2.1.0` 生成直後の model では ONNX Runtime load 時に `Concat.0` の shape inference error で失敗する。`xtask` の scalar Concat repair 後は load 成功。
- fallback 確認: `paddle2onnx==1.3.1` で det / rec / cls の ONNX 変換成功。各 model は `hard_swish` のため opset 14 へ自動引き上げ。
- `ocr-onnx eval --model .../det.onnx`: load 成功。input `x: Tensor<f32>(N,3,H,W)`, output `fetch_name_0: Tensor<f32>(N,1,H,W)`, session load 約 82 ms。
- `ocr-onnx eval --model .../rec.onnx`: load 成功。input `x: Tensor<f32>(N,3,48,W)`, output `fetch_name_0: Tensor<f32>(N,T,97)`, session load 約 97 ms。
- `ocr-onnx eval --model .../cls.onnx`: repair 後 load 成功。input `x: Tensor<f32>(N,3,H,W)`, output `fetch_name_0: Tensor<f32>(N,2)`, session load 約 79 ms。
- dummy inference:
  - det: `--run-dummy --width 960 --height 960` で成功、約 66 ms、output `(1,1,960,960)`。`48x320` は model 内 `Resize.0` の shape mismatch で失敗するため、det input は PaddleOCR preprocessing と同じく適切な resize / padding が必要。
  - rec: `--run-dummy --width 320 --height 48` で成功、約 11 ms、output `(1,40,97)`。
  - cls: `--run-dummy --width 320 --height 48` で成功、約 4 ms、output `(1,2)`。
- image inference:
  - 合成 `HELLO` crop (`320x48`) を rec に入力し、CTC decode で `HELLO`、confidence 約 `0.948`。
  - PP-OCRv5 server rec は class offset `2` で CTC decode する。合成 `HELLO` crop (`320x48`) で `HELLO`、confidence 約 `0.994`。
  - 同じ crop を cls に入力し、angle `0`、confidence 約 `0.965`。
  - `docs/image/m05/ss-ocr-result.png` を det に `960x960` resize 入力し、output `(1,1,960,960)`、約 `65 ms`。簡易 connected-component postprocess で 20 bbox を抽出。
  - 合成 `HELLO` crop は det では全ゼロ出力だった。det は文書/画面全体から text region を拾うモデルなので、単体 crop の評価には使わず rec / cls の確認用とする。
  - `--pipeline` は det / cls / rec の 3 ONNX session を接続し、det bbox を source image へ逆写像して crop したうえで cls / rec へ渡す。rec input width は crop aspect ratio から `height=48` 基準で動的に決める。`docs/image/m05/ss-ocr-result.png` の上位 boxes で `VAC/output` と `127.0.0.1:57000/output` を読めることを確認。English preset なので日本語 UI 部分の精度評価には使わない。
  - `ppocrv5-server` pipeline では同じ画像から `VAC/output`、`127.0.0.1:57000/output`、`シークレット(2)` を確認。CPU では server det が重いため、後続で PP-OCRv5 mobile preset も速度比較する。
  - `--merge-lines --line-gap <px>` は connected-component bbox を同一行として結合する実験用オプション。現状は過結合しやすく、recognizer に長すぎる crop を渡して精度を落とすことがあるため既定では無効。

これにより、`ort` crate + ONNX Runtime で PaddleOCR model を読み、det bbox 抽出、cls angle 判定、rec CTC decode、そして最小 pipeline 接続まで到達した。次の実装対象は DB postprocess の polygon / unclip 精度改善、det bbox の過分割/過結合を抑える grouping、resize/padding の PaddleOCR 互換化、日本語 preset 評価。

## Later presets

日本語実運用向けには PP-OCRv4/v5 Japanese recognition model を追加評価する。まず English preset で ONNX Runtime と PaddleOCR postprocess の実装境界を確認してから、Japanese model に広げる。

## References

- PaddleOCR ONNX model guide: <https://www.paddleocr.ai/latest/en/version3.x/deployment/obtaining_onnx_models.html>
- PaddleOCR model list: <https://www.paddleocr.ai/main/en/version3.x/model_list.html>
- PaddleOCR text recognition module model list: <https://www.paddleocr.ai/main/en/version3.x/module_usage/text_recognition.html>

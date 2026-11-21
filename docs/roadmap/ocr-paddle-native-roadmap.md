# OCR PaddleOCR Native Roadmap

> **2026-08-02 評価結果**: v2.0既定backendはWindows Media OCRに決定した。native wrapperはrecognize未接続のexperimental stubとして維持し、製品統合は停止する。比較根拠は [`ocr-backend-evaluation.md`](ocr-backend-evaluation.md)。

以下はPaddleOCR nativeを比較候補として調査した時点の実験記録。

## 経緯

V1 の OCR は Windows Media OCR を直接利用していたが、`windows` crate 0.62 migration に伴い WinRT async API の扱いが崩れ、現在は `flowgraph.ocr.recognize` から呼ばれる helper が明示的に `on_error` へ落ちる状態になっている。

Windows Media OCR は軽量・高速で有力だが、Windows 専用、API の制御幅が狭い、model / parameter の融通が利かない、WinRT binding の追従コストが高いという欠点がある。UNVAC v2 の OCR は「配信中の画面を低遅延に読む」機能として扱うため、model と前処理・後処理をこちらで管理できる PaddleOCR 系へ舵を切る。

最初は worker process / Zenoh shared memory も検討したが、v2 の開発コストと実効効率を優先し、まずは in-process native DLL 版のみを評価する。UNM で MediaPipe native dependency を repository に直接含めずビルド・同梱した経験を踏まえ、PaddleOCR native も同じ配布モデルで実験する。

ただし PaddleOCR native DLL だけでは OCR は動かない。detector / recognizer / optional classifier の model、dictionary / label file、runtime parameter、Paddle Inference / OpenCV runtime DLL、third-party license bundle が必要になる。これらは `.vendors/paddleocr-native/` に生成・収集し、release package へ同梱できるかを評価する。

ONNX Runtime 版も候補に残す。PaddleOCR の model と手法を採用しつつ、Paddle Inference ではなく ONNX Runtime で動かせるなら、配布物・API・provider 選択の見通しが良くなる可能性がある。一方で DB postprocess、CTC decode、classifier、前処理 parameter を UNVAC 側で持つ必要があるため、native DLL 版を先に開発用 reference として評価する。

戻し先の基点は `ab2f3f7 feat(ocr): scaffold PaddleOCR native experiment` の直前。PaddleOCR 方向が不適切だと判断した場合は、この実験 series を戻し、Windows Media OCR 復旧または OCR paused の方針へ戻れるようにする。

## 方針

- PaddleOCR upstream repository は本 repository に含めない。
- UNVAC 側に含めるのは、C ABI wrapper、Rust FFI crate、`xtask` による取得・ビルド・ライセンス収集の足場。
- まず Windows x64 / CPU inference / 同梱 model を対象にする。
- worker process / Zenoh shared memory / Tesseract / Windows Media OCR fallback は、この実験が不安定だった場合の後続候補に退避する。
- FlowGraph の外向き contract は `flowgraph.ocr.recognize` を維持する。内部実装は `paddleocr-inproc` 固定から始め、過剰な backend abstraction は避ける。

## Evaluation tools

PaddleOCR native DLL 版は、UNVAC 本体へ組み込む前に開発環境向けの評価バイナリで検証する。

- default build target には含めない。
- generated DLL / import lib / model / license bundle は `.vendors/paddleocr-native/` から読む。
- engine create / destroy、single image recognition、repeated recognition、elapsed time、failure status、last error を確認する。
- ONNX Runtime 版実装時の reference として、同じ画像・同じ model set で比較できるようにする。

評価対象:

- stub DLL が Rust FFI から link / load できること。
- PaddleOCR upstream wiring 後、1,000 repeated calls で crash しないこと。
- 30 minute stream-like loop で memory が増え続けないこと。
- file I/O を hot path から外せる in-memory frame API になること。
- OCR 失敗時に UNVAC 本体を落とさず error status / last error で返せること。

## ONNX Runtime branch

ONNX 版は PaddleOCR native DLL 版の reference 結果を見ながら進める。UNVAC 側で独自 C FFI を増やさず、Rust crate 経由で ONNX Runtime を使う方針を優先する。

Model preparation notes: [`ocr-paddle-onnx-models.md`](ocr-paddle-onnx-models.md)

検討軸:

- Rust crate の保守状況と ONNX Runtime version 追従。
- Windows x64 CPU provider の配布容易性。
- DirectML / CUDA / OpenVINO 等の execution provider を後から選べるか。
- model download / checksum / license manifest を `xtask` で管理できるか。
- PaddleOCR の preprocessing / DB postprocess / CTC decode を Rust 側で持つ実装コスト。

ONNX 版を採用する場合でも、FlowGraph の外向き contract は `flowgraph.ocr.recognize` のまま維持する。

初期候補は `ort` crate。`ort` は Microsoft ONNX Runtime の Rust binding で、旧 `onnxruntime-rs` の流れを汲む。`onnxruntime` crate 系は保守状況の見極めが必要なため、まずは `ort` で CPU provider の最小 inference を作る。ONNX Runtime 自体は Execution Provider framework を持ち、CPU のほか DirectML 等へ展開できる。DirectML は Windows GPU 向け候補だが、v2 初期では CPU 版を先に固定する。

開発用 probe として `vac-ocr-onnx-eval` を追加する。これは default build target には含めず、`cargo xtask ocr-onnx eval --model <path>` で任意の ONNX model を load し、session load time と input/output metadata を確認する。PaddleOCR ONNX 版では、det / rec / cls model を個別に load し、native DLL 版の結果と比較する。

## Shared ONNX runtime boundary

ONNX Runtime は OCR 以外にも、TTS、VAD、classifier、embedding、簡易 vision model などで使う可能性がある。ただし現段階で汎用 runtime framework を実装しない。まず OCR で ONNX model load / inference / packaging が成立することを確認し、その過程で domain 非依存の境界だけを記録する。

将来共通化する候補:

- `OnnxRuntimeConfig`: runtime root、execution provider、thread 数、graph optimization level、DLL search path。
- `OnnxModelManifest`: model id、domain、source URL、version、checksum、license、required files。
- `OnnxSessionOptions`: session ごとの provider override、intra/inter threads、memory arena、profiling。
- `OnnxRuntimeDiagnostics`: loaded runtime version、provider list、model metadata、session load time。
- `xtask onnx-runtime`: model download、checksum verify、license manifest、runtime DLL staging。

共通化しないもの:

- OCR の image preprocessing / DB postprocess / CTC decode。
- TTS の text normalization / phoneme / accent / vocoder pipeline。
- domain ごとの FlowGraph node contract。

切り出し条件:

- OCR ONNX probe が det / rec / cls model を実際に load し、推論結果を native DLL reference と比較できる。
- 2 つ目の domain、たとえば TTS ONNX または VAD ONNX、が具体的に必要になる。
- model / runtime 配布 manifest が OCR 固有でなくなったと判断できる。

つまり、今は `vac-ocr-onnx-eval` を実験名のまま維持する。OCR で実用性を確認してから、必要に応じて `vac-onnx-runtime` / `vac-onnx-eval` へ昇格する。

参考:

- `ort` crate docs: <https://docs.rs/ort/latest/ort/>
- ONNX Runtime Execution Providers: <https://onnxruntime.ai/docs/execution-providers/>
- ONNX Runtime DirectML EP: <https://onnxruntime.ai/docs/execution-providers/DirectML-ExecutionProvider.html>

## Native artifact layout

`cargo xtask paddleocr-native ...` は以下の ignored workspace を使う。

```text
.vendors/paddleocr-native/
  src/PaddleOCR/          # upstream checkout
  build/                  # CMake build tree
  install/bin/            # redistributable DLLs
  install/lib/            # import libs
  install/include/        # C ABI headers
  models/                 # selected OCR models
  licenses/               # third-party notices for release audit
```

Repository tracked files:

- `xtask/`: orchestration
- `native/paddleocr-inproc/`: UNVAC-owned C ABI wrapper
- `crates/vac-ocr-paddle-sys/`: Rust FFI declarations and optional link hints

## Redistribution checklist

- PaddleOCR license copied into release notice bundle.
- Paddle Inference license notices copied into release notice bundle.
- OpenCV license notices copied into release notice bundle.
- Model URL, version, checksum, and license recorded.
- Generated native DLLs and models are not committed to git.
- Release package contains the wrapper DLL, required runtime DLLs, model files, and `THIRD_PARTY_MANIFEST.md`.

## Build notes

PaddleOCR C++ local deployment expects PaddleOCR `deploy/cpp_infer`, OpenCV, and Paddle Inference paths. Official Windows deployment notes describe configuring OpenCV and Paddle Inference paths before compiling the C++ inference demo.

Current UNVAC wrapper defaults to a stub build. The real upstream integration is enabled later with:

```console
cargo xtask paddleocr-native prepare --ref main
cargo xtask paddleocr-native configure --with-upstream --opencv-dir C:\path\to\opencv --paddle-inference-dir C:\path\to\paddle_inference
cargo xtask paddleocr-native build --config Release
```

The first implementation target is a narrow C ABI:

- create/destroy engine from `model_dir`
- recognize an in-memory RGB/BGR frame
- return UTF-8 text and later region metadata
- expose last error string

## Release gate

- 1,000 repeated OCR calls without crash.
- 30 minute stream-like OCR loop without unbounded memory growth.
- No file I/O on the hot path once image capture integration is wired.
- Failure returns `on_error` / `result.err` without bringing down FlowGraph.
- Release package resolves DLLs from the packaged runtime directory.

画面の情報をAIで抽出する（任意導入・常用設定は変更しない）

main.flowgraph.toml は「Web入力 → 指定OBSソースの撮影 → AI Analyze → ログ」の例。
AI persona、音声、Twitchへの投稿、アバター操作は行わない。
scene/sourceのSET_...を実際の名称へ変更し、必要ならOBSのurl/passwordを接続する。
OpenAI認証はVAC_OPENAI_API_KEY、またはanalysisノードのapi_key_file（絶対パス）を使用する。
キー本文をグラフに保存しない。配布時は認証ファイルも含めない。
起動だけではAPIを呼ばず、Web入力から質問を送った時に1回呼ぶ。

用途別の使い分け
同じAI Analyzeを用途ごとに配置、または既存のgraph compositionで再利用する。
model / reasoning_effort / instructions / 出力上限 / timeoutを固定propertyで設定する。
本文や視聴者からモデル・認証情報を指定させる入力ではない。
6L: gpt-6-luna / none または low
6.1T: gpt-6.1-terra / none または low（例の初期値）
6.2S: gpt-6.2-sol / low
6.2A: gpt-6.2-astra / low
6.1S: gpt-6.1-sol / low
6A: gpt-6-astra / low
これは用途の固定割当ではない。Sol/Astraにnoneを指定するとエラーになる。

入力と出力
content: 質問・資料。
image_data_url: 画像1枚。PNG/JPEG/WebPのbase64 data URL。空ならテキスト処理。
context: 取得元・取得時刻など。AIへの指示にはせず、receiptへそのまま戻す。
on_started: 非同期処理開始。解析成功を意味しない。
on_success: 完了し、JSONの形式も検証した結果だけ。
analysis: {summary: string, observations: string[], uncertainties: string[]}。
receipt: analysisに加え、要求／応答モデル、effort、応答ID、使用token、処理時間、purpose、context。
on_error / error: 入力不正、認証不備、API失敗、時間切れ、出力不正。
cancel / on_cancelled: 進行中の処理を止め、結果を捨てる。
同じノードへ新しいexec_inが来ると古い処理は取り消す。別ノードは独立。
解析待ちで他のFlowgraph処理を止めない。取消前にAPIが受信済みなら料金が発生する場合がある。
単発のmock実行では外部APIへ接続しない。実行中グラフで使用する。

共演者へ渡すには
analysisの観測事実・不明点とreceiptの取得元・時刻を明示edgeで整形し、AI Observeのcontentへ接続する。summaryだけでは具体的な観測が欠ける場合がある。
on_successだけをObserveのexec_inへ接続し、対象persona_idを明示する。
認識事実と不明点、取得時刻を残し、「画面からの観測資料」として渡す。
既存の画像を直接共演者へ渡す経路と併用する際は、二重の観測・二重発話を避ける。
この例には共演者への接続を含めていない。

現時点で含まないもの
用途設定の一括編集UI、任意JSON Schema、モデルの自動格付け／失敗時の自動乗換え。
生成内容の正しさは形式検証とは別。保存画面比較の評価結果を参照する。

共演者へ渡す専用出力（追加）
observation_textはanalysisの全項目とcontext・解析モデルをJSONで保持した文字列。summaryだけを使う場合の情報欠落を避ける。AI Observeのcontentへ接続し、on_successからだけ発火する。元の人間の発言は別の会話入力として保持する。常用グラフへは未接続。
分析の完了は別の非同期イベントとなる。完了後のノードから元のWeb入力のcontentを直接引き直してはならない。質問も結果と一緒に渡す場合は、分析開始前にjson.set_stringでcontextへoriginal_question等のキーとして入れ、完了後はreceiptのcontext.original_questionをjson.getで取り出す。これにより、質問はその分析結果に結び付いた値となる。別途通常の会話入力にも同じ質問を送る構成では二重発話に注意する。
画像撮影→Terra→共演者を直列実行する場合、Terraの処理時間が加算される。取得時刻は保持されるが、この出力単体で古さの上限や返答直前の再撮影を保証するものではない。

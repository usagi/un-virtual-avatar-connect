mod sm_with_conf;
mod sm_without_conf;
mod speaker_catalog;
mod os_tts_test;

use clap::{parser::ValueSource, CommandFactory, FromArgMatches, Parser};

const DEFAULT_CONF_PATH: &str = "conf.toml";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
	/// 個別の設定ファイルのパスを指定することもできます。
	#[arg(name = "CONF", default_value = DEFAULT_CONF_PATH)]
	pub conf: String,

	/// `CONF` がコマンドラインで明示されたか。desktop の初回設定解決に使う。
	#[arg(skip)]
	pub conf_is_explicit: bool,

	/// 自己再起動時、旧プロセスが終了してportやdeviceを解放するまでbootを待つ。
	#[arg(long, hide = true)]
	pub restart_parent_pid: Option<u32>,

	/// 通常起動したデスクトップの再起動引継ぎ。
	#[arg(long, hide = true)]
	pub desktop_primary: bool,

	/// デバッグモードを有効にします。ログの出力レベルが Trace に設定されログが滝のように流れ出します。
	#[arg(short = 'D', long)]
	pub debug: bool,

	/// CoeiroInk の Speakers を表示します。使用可能な Speakers の一覧を確認する用途で使用できます。
	#[arg(long)]
	pub coeiroink_speakers: bool,

	/// AivisSpeech Engine の `/speakers` を表示します（既定 URL: `http://127.0.0.1:10101`）。エンジンを起動した状態で実行してください。
	#[arg(long)]
	pub aivisspeech_speakers: bool,

	/// VOICEVOX ENGINE の `/speakers` を表示します（既定 URL: `http://127.0.0.1:50021`）。エンジンを起動した状態で実行してください。
	#[arg(long)]
	pub voicevox_speakers: bool,

	/// OS-TTSのテストを実行します。使用可能なOS-TTSの一覧を確認する用途でも使用できます。
	#[arg(long)]
	pub test_os_tts: bool,

	/// AI ペルソナ（`[[ai.personas]]`）のファインチューニングを実行します。
	/// --persona-id で persona を指定でき、未指定時は設定ファイルで最初に定義されている AI persona が使用されます。
	#[arg(long)]
	pub openai_chat_fine_tuning: bool,

	/// 特定の AI persona（`[[ai.personas]]`）を指定したい場合に使用します。主に `--openai-chat-fine-tuning` と併用します。
	#[arg(long)]
	pub persona_id: Option<String>,

	/// OpenAI API でアップロードされている全てのファイルを削除します。
	/// 処理の失敗などで既に不要なファイルが API サービス側に残ってしまっている場合など、それらを削除したい場合に使用できます。
	#[arg(long)]
	pub openai_api_clear_files: bool,

	/// 実験的な機能を有効にします。主に開発用で、動作内容は何かと開発者の都合にあわせて変化します。
	#[arg(long)]
	pub experimental: bool,

	/// v1 `conf.toml` を best-effort で v2（Flowgraph 中心）に変換して書き出します。
	/// 既存ファイルは上書きせず、`--migrate-out-dir` 配下にすべて出力されます。
	#[arg(long)]
	pub migrate: bool,

	/// `--migrate` の出力先ディレクトリ（省略時は `./migrated/`）。
	#[arg(long, default_value = "migrated")]
	pub migrate_out_dir: String,

	/// `--migrate` で warning も error 扱いとし、1 件でも出たら exit 1 する。
	#[arg(long)]
	pub migrate_strict: bool,

	/// conf、AvatarLink、指定Flowgraphを副作用なしで検証して終了します。
	#[arg(long)]
	pub validate_conf: bool,

	/// Flowgraph ディレクトリをロードし、1-shot fixture 実行して終了します（LF-3a）。
	#[arg(long)]
	pub flowgraph_test_dir: Option<String>,

	/// Flowgraph ディレクトリをロード検証して終了します。fixture test file は不要です。
	#[arg(long)]
	pub flowgraph_load_dir: Option<String>,

	/// `--flowgraph-load-dir`でstreaming contractのblockedもexit 1にする。
	#[arg(long, requires = "flowgraph_load_dir")]
	pub flowgraph_require_streaming_contract: bool,

	/// 指定 root 配下の `*.flowgraph.test.toml` を持つ Flowgraph fixture をまとめて実行して終了します。
	#[arg(long)]
	pub flowgraph_test_root: Option<String>,

	/// `--flowgraph-test-dir` / `--flowgraph-test-root` / `--flowgraph-load-dir` の結果を JSON で出力します。
	#[arg(long)]
	pub flowgraph_test_json: bool,

	/// UNAA/Z camera.effectのrelease統合診断。exact Renderer instanceへ一回だけ送信する。
	#[arg(long, hide = true, value_name = "INSTANCE_ID", requires = "unaa_z_camera_effect_args_json")]
	pub unaa_z_camera_effect_probe_instance: Option<String>,

	/// target_session_idを除くcamera.effect args JSON。discoveryした現在sessionを自動注入する。
	#[arg(long, hide = true, value_name = "JSON", requires = "unaa_z_camera_effect_probe_instance")]
	pub unaa_z_camera_effect_args_json: Option<String>,
}

// note: Args の impl 群はサブモジュールに分離されています。
impl Args {
	pub fn new() -> Args {
		log::info!("コマンドライン引数をパースします。");
		let matches = Args::command().get_matches();
		let conf_is_explicit = matches.value_source("CONF") == Some(ValueSource::CommandLine);
		let mut args = Args::from_arg_matches(&matches).expect("clap definition and Args must agree");
		args.conf_is_explicit = conf_is_explicit;
		args
	}
}

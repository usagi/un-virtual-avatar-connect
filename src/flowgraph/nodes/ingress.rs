//! δ-3d: Ingress ノードのスケルトン。
//!
//! 3 種（WebInput / Voice / Twitch）を同一パターンで表現する:
//!
//! - 外部コード（HTTP ハンドラ / voice エンジン / Twitch IRC クライアント）が
//!   `TriggerHandle::send(TriggerEvent)` でエンジンを叩く
//! - `fired_exec = ["__trigger__"]`、`data_overrides` に各フィールド値を載せる
//! - エンジンは fire_node で overrides を inputs に merge → ノードの compute が
//!   対応する outputs を echo して `exec_out` を発火
//!
//! ## 設計メモ
//!
//! - これらの「echo 型」ノードは `PureNode` として実装して十分（compute は決定論的）。
//! - 実際の外部 I/O は engine の外側（V1 Twitch/Voice/WebInput のブリッジレイヤー、δ-4d）が担う。
//! - 各 ingress の出力フィールドは V1 の `ChannelDatum` を参考に代表的なものを揃える:
//!   `content`（本文）/ `source_actor`（発信者識別子）/ `source_kind`（種別）/ `meta`（自由）
//! - **Phase M1**: `flowgraph.ingress.vmc_udp` + `bridges::vmc_ingress` — VMC 互換の生 UDP。
//! - **Phase ρ**: `flowgraph.ingress.osc_udp` + `bridges::osc_ingress` — 汎用 OSC/任意 UDP（`__meta__.profile = "osc_udp"`）。
//! - **Phase M7**: `flowgraph.ingress.unmfz` + `bridges::unmfz_ingress` — UNMF/Z Zenoh を常時購読。
//!
//! ## 入力ポートが `__` プレフィックス始まりの理由
//!
//! ユーザがエディタで誤接続しないように、内部ポートであることを命名規則で示す。
//! δ-6 GUI で `__` 始まりポートを自動的に hide する予定。

use crate::flowgraph::node::{
	ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, PropertySpec, PureNode,
};
use crate::flowgraph::socket::{SocketType, SocketValue};
use async_trait::async_trait;
use serde_json::Value as JsonValue;

// ---------------------------------------------------------------------
// ヘルパ: 共通の ingress ポート群
// ---------------------------------------------------------------------

fn ingress_inputs() -> Vec<PortSpec> {
	vec![
		PortSpec::exec_input("__trigger__", "(internal trigger)"),
		PortSpec::input("__content__", "(internal) content", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__source_actor__", "(internal) actor", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__source_kind__", "(internal) kind", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__trace_id__", "(internal) trace id", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__meta__", "(internal) meta", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
	]
}

fn binary_ingress_inputs() -> Vec<PortSpec> {
	let mut inputs = ingress_inputs();
	inputs.push(
		PortSpec::input("__content_bytes__", "(internal) content bytes", SocketType::Bytes).with_default(SocketValue::Bytes(Vec::new())),
	);
	inputs
}

fn ingress_outputs() -> Vec<PortSpec> {
	vec![
		PortSpec::exec_output("exec_out", "Exec Out"),
		PortSpec::output("content", "Content", SocketType::String),
		PortSpec::output("source_actor", "Source Actor", SocketType::String),
		PortSpec::output("source_kind", "Source Kind", SocketType::String),
		PortSpec::output("trace_id", "Trace ID", SocketType::String),
		PortSpec::output("meta", "Meta", SocketType::Json),
	]
}

fn binary_ingress_outputs() -> Vec<PortSpec> {
	let mut outputs = ingress_outputs();
	outputs.push(PortSpec::output("content_bytes", "Content Bytes", SocketType::Bytes));
	outputs
}

fn unmfz_ingress_inputs() -> Vec<PortSpec> {
	let mut inputs = binary_ingress_inputs();
	inputs.push(
		PortSpec::input("__unmf__", "(internal) UNMF", SocketType::Unmf)
			.with_default(SocketValue::Unmf(vac_motion::unmf::UnMotionFrame::new(0))),
	);
	inputs
		.push(PortSpec::input("__key_expr__", "(internal) key expr", SocketType::String).with_default(SocketValue::String(String::new())));
	inputs
}

fn unmfz_ingress_outputs() -> Vec<PortSpec> {
	let mut outputs = binary_ingress_outputs();
	outputs.push(PortSpec::output("unmf", "UNMF", SocketType::Unmf));
	outputs.push(PortSpec::output("key_expr", "Key Expr", SocketType::String));
	outputs
}

fn file_watch_inputs() -> Vec<PortSpec> {
	vec![
		PortSpec::exec_input("__trigger__", "(internal trigger)"),
		PortSpec::input("__path__", "(internal) path", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__event_kind__", "(internal) event kind", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__source_actor__", "(internal) actor", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__source_kind__", "(internal) kind", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__meta__", "(internal) meta", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
	]
}

fn file_watch_outputs() -> Vec<PortSpec> {
	vec![
		PortSpec::exec_output("exec_out", "Exec Out"),
		PortSpec::output("path", "Path", SocketType::String),
		PortSpec::output("event_kind", "Event Kind", SocketType::String),
		PortSpec::output("source_actor", "Source Actor", SocketType::String),
		PortSpec::output("source_kind", "Source Kind", SocketType::String),
		PortSpec::output("meta", "Meta", SocketType::Json),
	]
}

async fn ingress_compute(inputs: &InputMap, fired_exec: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
	if !fired_exec.contains("__trigger__") {
		return Ok(NodeOutput::new());
	}
	let content = inputs.get("__content__").cloned().unwrap_or(SocketValue::String(String::new()));
	let actor = inputs
		.get("__source_actor__")
		.cloned()
		.unwrap_or(SocketValue::String(String::new()));
	let kind = inputs.get("__source_kind__").cloned().unwrap_or(SocketValue::String(String::new()));
	let trace_id = inputs.get("__trace_id__").cloned().unwrap_or(SocketValue::String(String::new()));
	let meta = inputs.get("__meta__").cloned().unwrap_or(SocketValue::Json(JsonValue::Null));
	Ok(NodeOutput::new()
		.set_data("content", content)
		.set_data("source_actor", actor)
		.set_data("source_kind", kind)
		.set_data("trace_id", trace_id)
		.set_data("meta", meta)
		.fire_exec("exec_out"))
}

async fn binary_ingress_compute(inputs: &InputMap, fired_exec: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
	let mut out = ingress_compute(inputs, fired_exec).await?;
	if !fired_exec.contains("__trigger__") {
		return Ok(out);
	}
	let content_bytes = inputs.get("__content_bytes__").cloned().unwrap_or(SocketValue::Bytes(Vec::new()));
	out = out.set_data("content_bytes", content_bytes);
	Ok(out)
}

async fn unmfz_ingress_compute(inputs: &InputMap, fired_exec: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
	let mut out = binary_ingress_compute(inputs, fired_exec).await?;
	if !fired_exec.contains("__trigger__") {
		return Ok(out);
	}
	let unmf = inputs
		.get("__unmf__")
		.cloned()
		.unwrap_or_else(|| SocketValue::Unmf(vac_motion::unmf::UnMotionFrame::new(0)));
	let key_expr = inputs.get("__key_expr__").cloned().unwrap_or(SocketValue::String(String::new()));
	out = out.set_data("unmf", unmf).set_data("key_expr", key_expr);
	Ok(out)
}

async fn file_watch_compute(inputs: &InputMap, fired_exec: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
	if !fired_exec.contains("__trigger__") {
		return Ok(NodeOutput::new());
	}
	let path = inputs.get("__path__").cloned().unwrap_or(SocketValue::String(String::new()));
	let event_kind = inputs.get("__event_kind__").cloned().unwrap_or(SocketValue::String(String::new()));
	let actor = inputs
		.get("__source_actor__")
		.cloned()
		.unwrap_or(SocketValue::String(String::new()));
	let kind = inputs.get("__source_kind__").cloned().unwrap_or(SocketValue::String(String::new()));
	let meta = inputs.get("__meta__").cloned().unwrap_or(SocketValue::Json(JsonValue::Null));
	Ok(NodeOutput::new()
		.set_data("path", path)
		.set_data("event_kind", event_kind)
		.set_data("source_actor", actor)
		.set_data("source_kind", kind)
		.set_data("meta", meta)
		.fire_exec("exec_out"))
}

// ---------------------------------------------------------------------
// ingress.web_input
// ---------------------------------------------------------------------

pub struct WebInputIngressNode;

impl NodeDescriptor for WebInputIngressNode {
	fn control_triggerable(&self) -> bool { true }
	fn describe(&self) -> NodeSpec {
		NodeSpec {
   feature: "flowgraph.ingress.web_input".into(),
   title: "Web Input Ingress".into(),
   category: "ingress".into(),
   description: Some("HTTP エンドポイント経由の入力（V1 の /input/* 相当）".into()),
   inputs: ingress_inputs(),
   outputs: ingress_outputs(),
   properties: vec![
    PropertySpec::new("path", "Path", SocketType::String, SocketValue::String(String::new()))
     .description("HTTP パス（例 \"/input/chat\"）。空なら `/input/flowgraph/<node_id>` を自動採番。"),
    PropertySpec::new("method", "Method", SocketType::String, SocketValue::String("POST".into()))
     .description("HTTP メソッド。\"POST\"/\"GET\"/\"PUT\" 等（大文字小文字不問）。"),
    PropertySpec::new(
     "body_format",
     "Body Format",
     SocketType::String,
     SocketValue::String("plain".into()),
    )
    .description("リクエストボディの解釈。\"plain\"（本文をそのまま content）/\"json\"（`content` フィールド）/\"form\"（`content` パラメータ）/\"toml\"・\"msgpack\"（`content` フィールド）。"),
    PropertySpec::new(
     "fixed_channel",
     "Fixed Channel",
     SocketType::String,
     SocketValue::String(String::new()),
    )
    .description("ingress が echo で出す `source_kind`（V1 の channel 相当）。空なら `web_input`。"),
   ],
  }
	}
}

#[async_trait]
impl PureNode for WebInputIngressNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		ingress_compute(inputs, fired_exec).await
	}
}

// ---------------------------------------------------------------------
// ingress.file_watch
// ---------------------------------------------------------------------

pub struct FileWatchIngressNode;

impl NodeDescriptor for FileWatchIngressNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.ingress.file_watch".into(),
			title: "File Watch Ingress".into(),
			category: "ingress".into(),
			description: Some("ファイルまたはディレクトリ更新を polling bridge から受け取る ingress。".into()),
			inputs: file_watch_inputs(),
			outputs: file_watch_outputs(),
			properties: vec![
				PropertySpec::new("path", "Path", SocketType::String, SocketValue::String(String::new()))
					.description("監視対象ファイルまたはディレクトリ。空なら bridge は起動しない。"),
				PropertySpec::new("recursive", "Recursive", SocketType::Bool, SocketValue::Bool(false))
					.description("ディレクトリ監視時にサブディレクトリも走査する。"),
				PropertySpec::new("poll_ms", "Poll ms", SocketType::Int, SocketValue::Int(500))
					.description("polling 間隔。100ms 未満は 100ms に丸める。"),
				PropertySpec::new(
					"fixed_channel",
					"Fixed Channel",
					SocketType::String,
					SocketValue::String("file_watch".into()),
				)
				.description("ingress が echo で出す `source_kind`。空なら `file_watch`。"),
			],
		}
	}
}

#[async_trait]
impl PureNode for FileWatchIngressNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		file_watch_compute(inputs, fired_exec).await
	}
}

// ---------------------------------------------------------------------
// ingress.voice
// ---------------------------------------------------------------------

pub struct VoiceIngressNode;

impl NodeDescriptor for VoiceIngressNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.ingress.voice".into(),
			title: "Voice Ingress".into(),
			category: "ingress".into(),
			description: Some("音声認識（Vosk / Whisper）経由の入力".into()),
			inputs: ingress_inputs(),
			outputs: ingress_outputs(),
			properties: vec![
				PropertySpec::new("label", "Label", SocketType::String, SocketValue::String(String::new()))
					.description("GUIへ表示する音声入力名。空ならFlowgraphノードIDを表示する。"),
				PropertySpec::new("engine", "Engine", SocketType::String, SocketValue::String("vosk".into()))
					.required()
					.description("\"vosk\"または\"whisper\"。1ノードにつき1エンジンだけを起動する。"),
				PropertySpec::new(
					"model_path",
					"Model Path",
					SocketType::String,
					SocketValue::String("vosk-model-small-ja-0.22".into()),
				)
				.required()
				.description("VoskのモデルID/ディレクトリ、またはWhisper GGMLモデルのパス。Voskの既定値は初回に取得する。"),
				PropertySpec::new("source", "Audio Source", SocketType::String, SocketValue::String(String::new()))
					.description("Whisperの入力デバイス名（部分一致 / 空＝既定デバイス）。Voskは現在、既定入力を使う。"),
				PropertySpec::new("sample_rate", "Sample Rate", SocketType::Int, SocketValue::Int(16000))
					.description("サンプリングレート（Hz）。vosk は通常 16000、whisper は任意。"),
				PropertySpec::new("language_code", "Language", SocketType::String, SocketValue::String("ja".into()))
					.description("言語コード（whisper で使用）。例 \"ja\" / \"en\"。"),
				PropertySpec::new(
					"whisper_backend",
					"Whisper Backend",
					SocketType::String,
					SocketValue::String("auto".into()),
				)
				.description("Whisper workerを auto / vulkan / cpu から選択。autoはVulkan、CPUの順に試す。"),
				PropertySpec::new(
					"whisper_gpu_device",
					"Whisper GPU Device",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("VulkanのGPU index。空なら単一dGPUを優先して自動選択する。"),
				PropertySpec::new(
					"grammar_json",
					"Grammar (JSON)",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("vosk の constrained grammar を JSON 文字列で指定（省略可）。"),
				PropertySpec::new(
					"fixed_channel",
					"Fixed Channel",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("ingress が echo で出す `source_kind`。空なら `voice`。"),
				PropertySpec::new(
					"partial_channel",
					"Partial Subtitle Channel",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("途中認識をquiet datumとして更新する字幕チャンネル。空なら途中字幕を保存しない。"),
				PropertySpec::new(
					"partial_interval_ms",
					"Whisper Partial Interval",
					SocketType::Int,
					SocketValue::Int(800),
				)
				.description("Whisper発話中に途中認識を更新する間隔（ms）。0でWhisper途中認識を無効化。"),
				PropertySpec::new(
					"partial_min_audio_ms",
					"Whisper Partial Minimum Audio",
					SocketType::Int,
					SocketValue::Int(1300),
				)
				.description("Whisper途中認識へ渡す最小音声長（ms）。短すぎるprefixによる遅延と誤認識を抑える。"),
				PropertySpec::new(
					"speech_floor_key",
					"Speech Floor Key",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("発話開始からfinal後の猶予まで占有する発話権キー。AIのrespect_speech_floorと対応させる。"),
				PropertySpec::new(
					"speech_floor_release_delay_ms",
					"Speech Floor Release Delay",
					SocketType::Int,
					SocketValue::Int(700),
				)
				.description("final 後も発話権を保持する時間（ms）。続く文を別の AI 応答へ分割しにくくする。"),
				PropertySpec::new("vad_threshold_db", "VAD Threshold", SocketType::Int, SocketValue::Int(-45))
					.description("Whisperが発話とみなすRMS閾値（dBFS）。"),
				PropertySpec::new("vad_endpoint_ms", "VAD Endpoint", SocketType::Int, SocketValue::Int(600))
					.description("Whisperが発話終了とみなす連続無音時間（ms）。"),
				PropertySpec::new("vad_pre_roll_ms", "VAD Pre-roll", SocketType::Int, SocketValue::Int(400))
					.description("発話冒頭を欠落させないため Whisper に含める先行音声（ms）。"),
				PropertySpec::new("vad_min_speech_ms", "VAD Min Speech", SocketType::Int, SocketValue::Int(100))
					.description("ノイズを発話開始と誤認しないために必要な連続音声時間（ms）。"),
			],
		}
	}
}

#[async_trait]
impl PureNode for VoiceIngressNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		ingress_compute(inputs, fired_exec).await
	}
}

// ---------------------------------------------------------------------
// ingress.twitch
// ---------------------------------------------------------------------

pub struct TwitchIngressNode;

pub struct TwitchStreamStateIngressNode;

impl NodeDescriptor for TwitchStreamStateIngressNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.ingress.twitch_stream_state".into(),
			title: "Twitch Stream State".into(),
			category: "twitch".into(),
			description: Some("Helix/EventSubで維持する宣言済み配信状態を明示edgeへ公開する。".into()),
			inputs: ingress_inputs(),
			outputs: ingress_outputs(),
			properties: Vec::new(),
		}
	}
}

#[async_trait]
impl PureNode for TwitchStreamStateIngressNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		ingress_compute(inputs, fired_exec).await
	}
}

impl NodeDescriptor for TwitchIngressNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.ingress.twitch".into(),
			title: "Twitch Ingress".into(),
			category: "ingress".into(),
			description: Some("Twitch チャット / EventSub 経由の入力".into()),
			inputs: ingress_inputs(),
			outputs: {
				let mut outputs = ingress_outputs();
				outputs.push(PortSpec::output("message_text", "Message Text", SocketType::String));
				outputs
			},
			properties: vec![
				PropertySpec::new("label", "Label", SocketType::String, SocketValue::String(String::new()))
					.description("GUIへ表示するTwitch入力名。空ならFlowgraphノードIDを表示する。"),
				PropertySpec::new("mode", "Mode", SocketType::String, SocketValue::String("irc".into()))
					.description("\"irc\"（チャット受信）か \"eventsub\"（Helix/EventSub）。"),
				PropertySpec::new(
					"channels",
					"Channels",
					SocketType::List(Box::new(SocketType::String)),
					SocketValue::List(Vec::new()),
				)
				.description("IRC 購読するチャンネル名（`#` 不要）。"),
				PropertySpec::new(
					"access_token",
					"Access Token",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("OAuth トークン（空なら `token_key` による解決を試みる）。"),
				PropertySpec::new("token_key", "Token Key", SocketType::String, SocketValue::String(String::new()))
					.description("`conf.twitch.token_keys` で定義したキー。空なら `access_token` をそのまま使用。"),
				PropertySpec::new("client_id", "Client ID", SocketType::String, SocketValue::String(String::new()))
					.description("Twitch App の Client ID。EventSub で必須。"),
				PropertySpec::new(
					"broadcaster_id",
					"Broadcaster ID",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("EventSub subscribe 対象の broadcaster_user_id。"),
				PropertySpec::new("login", "Bot Login", SocketType::String, SocketValue::String(String::new()))
					.description("IRC ログイン名（小文字・`#` 不要）。"),
				PropertySpec::new(
					"fixed_channel",
					"Fixed Channel",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("ingress が echo で出す `source_kind`。空なら `twitch:chat` または `twitch:eventsub`。"),
			],
		}
	}
}

#[async_trait]
impl PureNode for TwitchIngressNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		let mut output = ingress_compute(inputs, fired_exec).await?;
		if fired_exec.contains("__trigger__") {
			let text = inputs.get("__meta__").and_then(|v| match v {
				SocketValue::Json(meta) => meta.get("message_text").and_then(|v| v.as_str()),
				_ => None,
			}).unwrap_or("");
			output = output.set_data("message_text", SocketValue::String(text.into()));
		}
		Ok(output)
	}
}

// ---------------------------------------------------------------------
// ingress.twitch_eventsub (ζ-2)
// ---------------------------------------------------------------------
//
// Twitch EventSub (WebSocket) の通知を **Flowgraph-native に** 受ける ingress。
// `src/bridges/twitch_eventsub.rs` が WS セッションを持ち、Helix で購読を張り、
// `notification` が届くたびに TriggerEvent を発火する。V1 `src/twitch/eventsub.rs`
// 経由の ChannelDatum パイプラインとは **独立** に動く:
//   - V1 = 人間可読 1 行を `channel_datum_tx` に push（既存 [[processors]] 用）
//   - V2 = 生 event JSON + 正規化メタを Flowgraph data output に流す（こっちは raw payload を持つ）
//
// 同一 broadcaster に対して両方を enable すると WS が 2 本張られる。不要なら
// `[twitch.eventsub].enabled = false` で V1 を落とすか、ingress ノード側を消す。

pub struct TwitchEventsubIngressNode;

impl NodeDescriptor for TwitchEventsubIngressNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.ingress.twitch_eventsub".into(),
			title: "Twitch EventSub Ingress".into(),
			category: "ingress".into(),
			description: Some(
				"Twitch EventSub (WebSocket) の通知を受ける Flowgraph-native ingress。`token_key` で \
     conf.twitch.token_keys の OAuth トークンを引き、`event_types` に挙げた sub_type を購読する。\
     `payload` は生 event JSON、`meta` はサブスクリプション情報を含む補助マップ。"
					.into(),
			),
			inputs: vec![
				PortSpec::exec_input("__trigger__", "(internal trigger)"),
				PortSpec::input("__event_type__", "(internal) event_type", SocketType::String)
					.with_default(SocketValue::String(String::new())),
				PortSpec::input("__actor_name__", "(internal) actor_name", SocketType::String)
					.with_default(SocketValue::String(String::new())),
				PortSpec::input("__actor_login__", "(internal) actor_login", SocketType::String)
					.with_default(SocketValue::String(String::new())),
				PortSpec::input("__broadcaster_login__", "(internal) broadcaster_login", SocketType::String)
					.with_default(SocketValue::String(String::new())),
				PortSpec::input("__payload__", "(internal) payload", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
				PortSpec::input("__meta__", "(internal) meta", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)),
			],
			outputs: vec![
				PortSpec::exec_output("exec_out", "Exec Out"),
				PortSpec::output("event_type", "Event Type", SocketType::String),
				PortSpec::output("actor_name", "Actor Name", SocketType::String),
				PortSpec::output("actor_login", "Actor Login", SocketType::String),
				PortSpec::output("broadcaster_login", "Broadcaster Login", SocketType::String),
				PortSpec::output("payload", "Raw Payload", SocketType::Json),
				PortSpec::output("meta", "Meta", SocketType::Json),
			],
			properties: vec![
				PropertySpec::new(
					"token_key",
					"Token Key",
					SocketType::String,
					SocketValue::String("broadcaster".into()),
				)
				.description("`conf.twitch.token_keys` で定義した OAuth key。EventSub 購読には user access token が必須。"),
				PropertySpec::new(
					"broadcaster_login",
					"Broadcaster Login",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description(
					"購読対象の配信者ログイン名。空なら `conf.twitch.username`、それも無ければ `token_key` の OAuth login が使われる。",
				),
				PropertySpec::new(
					"event_types",
					"Event Types",
					SocketType::List(Box::new(SocketType::String)),
					SocketValue::List(Vec::new()),
				)
				.description(
					"購読する sub_type リスト（例: `[\"channel.cheer\", \"channel.raid\"]`）。空なら conf.twitch.eventsub の \
      各 bool トグル（stream.online / channel.cheer ...）をフォールバックとして使う。",
				),
				PropertySpec::new(
					"channel_points_reward_id",
					"Channel Points Reward ID",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description(
					"`channel.channel_points_custom_reward_redemption.add` を個別 reward に絞りたい時の UUID。\
      空なら報酬を自動列挙してすべて購読する。",
				),
				PropertySpec::new(
					"fixed_channel",
					"Fixed Channel",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("（予約）将来 V1 channel に押し戻すブリッジ用。今はコメントのみ。"),
			],
		}
	}
}

async fn twitch_eventsub_compute(inputs: &InputMap, fired_exec: &ExecFireSet) -> Result<NodeOutput, NodeExecError> {
	if !fired_exec.contains("__trigger__") {
		return Ok(NodeOutput::new());
	}
	let event_type = inputs.get("__event_type__").cloned().unwrap_or(SocketValue::String(String::new()));
	let actor_name = inputs.get("__actor_name__").cloned().unwrap_or(SocketValue::String(String::new()));
	let actor_login = inputs.get("__actor_login__").cloned().unwrap_or(SocketValue::String(String::new()));
	let broadcaster_login = inputs
		.get("__broadcaster_login__")
		.cloned()
		.unwrap_or(SocketValue::String(String::new()));
	let payload = inputs.get("__payload__").cloned().unwrap_or(SocketValue::Json(JsonValue::Null));
	let meta = inputs.get("__meta__").cloned().unwrap_or(SocketValue::Json(JsonValue::Null));
	Ok(NodeOutput::new()
		.set_data("event_type", event_type)
		.set_data("actor_name", actor_name)
		.set_data("actor_login", actor_login)
		.set_data("broadcaster_login", broadcaster_login)
		.set_data("payload", payload)
		.set_data("meta", meta)
		.fire_exec("exec_out"))
}

#[async_trait]
impl PureNode for TwitchEventsubIngressNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		twitch_eventsub_compute(inputs, fired_exec).await
	}
}

// ---------------------------------------------------------------------
// AI output ingress
// ---------------------------------------------------------------------

pub struct AiResponseIngressNode;
pub struct AiSpeechIngressNode;
pub struct AiAvatarIntentIngressNode;
pub struct AiAvatarPoseIngressNode;

fn ai_output_spec(feature: &str, title: &str, description: &str, exposes_utterance_content: bool) -> NodeSpec {
	let mut inputs = vec![
		PortSpec::exec_input("__trigger__", "(internal trigger)"),
		PortSpec::input("__content__", "(internal) content", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__persona_id__", "(internal) persona", SocketType::String).with_default(SocketValue::String(String::new())),
		PortSpec::input("__utterance_id__", "(internal) utterance", SocketType::String).with_default(SocketValue::String(String::new())),
	];
	let mut outputs = vec![
		PortSpec::exec_output("exec_out", "Exec Out"),
		PortSpec::output("content", "Content", SocketType::String),
		PortSpec::output("persona_id", "Persona", SocketType::String),
		PortSpec::output("utterance_id", "Utterance", SocketType::String),
	];
	if exposes_utterance_content {
		inputs.push(
			PortSpec::input("__utterance_content__", "(internal) utterance content", SocketType::String)
				.with_default(SocketValue::String(String::new())),
		);
		outputs.push(PortSpec::output("utterance_content", "Utterance Content", SocketType::String));
	}
	inputs.push(PortSpec::input("__meta__", "(internal) meta", SocketType::Json).with_default(SocketValue::Json(JsonValue::Null)));
	outputs.push(PortSpec::output("meta", "Meta", SocketType::Json));
	NodeSpec {
		feature: feature.into(),
		title: title.into(),
		category: "ai".into(),
		description: Some(description.into()),
		inputs,
		outputs,
		properties: vec![
			PropertySpec::new("persona_id", "Persona", SocketType::String, SocketValue::String(String::new()))
				.description("空なら全persona。AIサービス境界の識別子であり、公開channel名ではない。"),
		],
	}
}

fn ai_output_compute(inputs: &InputMap, fired_exec: &ExecFireSet, exposes_utterance_content: bool) -> NodeOutput {
	if !fired_exec.contains("__trigger__") {
		return NodeOutput::new();
	}
	let mut output = NodeOutput::new()
		.set_data(
			"content",
			inputs.get("__content__").cloned().unwrap_or(SocketValue::String(String::new())),
		)
		.set_data(
			"persona_id",
			inputs.get("__persona_id__").cloned().unwrap_or(SocketValue::String(String::new())),
		)
		.set_data(
			"utterance_id",
			inputs
				.get("__utterance_id__")
				.cloned()
				.unwrap_or(SocketValue::String(String::new())),
		)
		.set_data(
			"meta",
			inputs.get("__meta__").cloned().unwrap_or(SocketValue::Json(JsonValue::Null)),
		)
		.fire_exec("exec_out");
	if exposes_utterance_content {
		output = output.set_data(
			"utterance_content",
			inputs
				.get("__utterance_content__")
				.cloned()
				.unwrap_or(SocketValue::String(String::new())),
		);
	}
	output
}

macro_rules! impl_ai_output_ingress {
	($ty:ty, $feature:literal, $title:literal, $description:literal, $exposes_utterance_content:literal $(,)?) => {
		impl NodeDescriptor for $ty {
			fn describe(&self) -> NodeSpec {
				ai_output_spec($feature, $title, $description, $exposes_utterance_content)
			}
		}

		#[async_trait]
		impl PureNode for $ty {
			async fn compute(
				&self,
				_host: &crate::flowgraph::node::PureEvalHost,
				_props: &InputMap,
				inputs: &InputMap,
				fired_exec: &ExecFireSet,
			) -> Result<NodeOutput, NodeExecError> {
				Ok(ai_output_compute(inputs, fired_exec, $exposes_utterance_content))
			}
		}
	};
}

impl_ai_output_ingress!(
	AiResponseIngressNode,
	"flowgraph.ingress.ai_response",
	"AI Response",
	"AI personaの確定応答全文を明示edgeへ公開する。",
	false
);
impl_ai_output_ingress!(
	AiAvatarIntentIngressNode,
	"flowgraph.ingress.ai_avatar_intent",
	"AI Avatar Intent",
	"AIが確定したAvatarIntent JSONを明示edgeへ公開する。",
	false
);
impl_ai_output_ingress!(
	AiAvatarPoseIngressNode,
	"flowgraph.ingress.ai_avatar_pose",
	"AI Avatar Pose",
	"AIが確定した名前付きAvatarPoseを明示edgeへ公開する。",
	false
);
impl_ai_output_ingress!(
	AiSpeechIngressNode,
	"flowgraph.ingress.ai_speech",
	"AI Speech",
	"AI personaが読み上げ対象として確定した発話segmentと、その時点までの累積発話を明示edgeへ公開する。",
	true
);

// ---------------------------------------------------------------------
// ingress.vmc_udp（Phase M1）
// ---------------------------------------------------------------------
//
// VMC 互換の **生 UDP** を [`crate::bridges::vmc_ingress`] が受信し、各データグラムごとに
// `TriggerEvent` を投入する。`content` にはペイロードの **Base64**（`__content__` 経由で echo）、
// `content_bytes` には同じペイロードの `bytes` を流す。
// `meta` に `remote` / `byte_len` / `encoding` を載せる。

pub struct VmcUdpIngressNode;

impl NodeDescriptor for VmcUdpIngressNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.ingress.vmc_udp".into(),
			title: "VMC UDP Ingress".into(),
			category: "ingress".into(),
			description: Some("VMC 互換の生 UDP を受信し、各データグラムを ingress echo で下流へ流す。`content` は Base64 文字列、`content_bytes` は bytes。".into()),
			inputs: binary_ingress_inputs(),
			outputs: binary_ingress_outputs(),
			properties: vec![
				PropertySpec::new("bind", "Bind", SocketType::String, SocketValue::String(String::new()))
					.description("受信 UDP の \"host:port\"（例 \"0.0.0.0:39539\"）。空のときブリッジは起動しない。"),
				PropertySpec::new(
					"fixed_channel",
					"Source Kind Override",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("空なら `source_kind` は `vmc_udp`。任意のラベルに上書き可能。"),
			],
		}
	}
}

#[async_trait]
impl PureNode for VmcUdpIngressNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		binary_ingress_compute(inputs, fired_exec).await
	}
}

// ---------------------------------------------------------------------
// ingress.osc_udp（Phase ρ）
// ---------------------------------------------------------------------
//
// 汎用 **OSC over UDP**（および任意バイナリ）の入口。[`crate::bridges::osc_ingress`] が受信し、
// `__meta__` に `profile: "osc_udp"` を載せる（VMC ingress のメタと区別）。

pub struct OscUdpIngressNode;

impl NodeDescriptor for OscUdpIngressNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.ingress.osc_udp".into(),
			title: "OSC UDP Ingress".into(),
			category: "ingress".into(),
			description: Some(
				"汎用 OSC（UDP データグラム）を受信し、ingress echo で下流へ流す。`content` は Base64、`content_bytes` は bytes。`__meta__.profile` は `osc_udp`。".into(),
			),
			inputs: binary_ingress_inputs(),
			outputs: binary_ingress_outputs(),
			properties: vec![
				PropertySpec::new("bind", "Bind", SocketType::String, SocketValue::String(String::new()))
					.description("受信 UDP の \"host:port\"。空のときブリッジは起動しない。"),
				PropertySpec::new(
					"fixed_channel",
					"Source Kind Override",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("空なら `source_kind` は `osc_udp`。任意のラベルに上書き可能。"),
			],
		}
	}
}

#[async_trait]
impl PureNode for OscUdpIngressNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		binary_ingress_compute(inputs, fired_exec).await
	}
}

// ---------------------------------------------------------------------
// ingress.unmfz（Phase M7）
// ---------------------------------------------------------------------
//
// UNMF/Z を Zenoh から常時購読する motion data plane の入口。
// `flowgraph.unmfz.subscribe` は exec ごとの一回受信、こちらは bridge が常駐して
// frame ごとに `TriggerEvent` を投入する。

pub struct UnmfzIngressNode;

impl NodeDescriptor for UnmfzIngressNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.ingress.unmfz".into(),
			title: "UNMF/Z Ingress".into(),
			category: "ingress".into(),
			description: Some(
				"UNMF/Z Zenoh topic を常時購読し、受信 frame ごとに `unmf` / `content_bytes` / `key_expr` を出力する。".into(),
			),
			inputs: unmfz_ingress_inputs(),
			outputs: unmfz_ingress_outputs(),
			properties: vec![
				PropertySpec::new(
					"base_key_expr",
					"Base Key",
					SocketType::String,
					SocketValue::String("un-motion/frame".into()),
				)
				.description("UNMF/Z の base key。末尾 `/` は取り除かれる。"),
				PropertySpec::new("topic_mode", "Topic Mode", SocketType::String, SocketValue::String("frame".into()))
					.description("\"frame\" / \"primary_source\" / \"stream_id\"。"),
				PropertySpec::new("schema_major", "Schema Major", SocketType::Int, SocketValue::Int(1))
					.description("購読する UNMF/Z schema major。key の `/vN` に使う。"),
				PropertySpec::new("poll_ms", "Poll ms", SocketType::Int, SocketValue::Int(10))
					.description("bridge が receiver を確認する間隔。1..1000ms に丸める。"),
				PropertySpec::new(
					"fixed_channel",
					"Source Kind Override",
					SocketType::String,
					SocketValue::String(String::new()),
				)
				.description("空なら `source_kind` は `unmfz`。任意のラベルに上書き可能。"),
			],
		}
	}
}

#[async_trait]
impl PureNode for UnmfzIngressNode {
	async fn compute(
		&self,
		_host: &crate::flowgraph::node::PureEvalHost,
		_props: &InputMap,
		inputs: &InputMap,
		fired_exec: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		unmfz_ingress_compute(inputs, fired_exec).await
	}
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::engine::{create_trigger_bus, FlowgraphBuilder, PortRef};
	use crate::flowgraph::node::{ExecCtx, NodeImpl, TriggerEvent};
	use crate::flowgraph::nodes::log::LogNode;
	use std::sync::Arc;
	use std::time::Duration;

	#[test]
	fn voice_ingress_has_usable_vosk_defaults() {
		let spec = VoiceIngressNode.describe();
		let engine = spec.properties.iter().find(|property| property.name == "engine").unwrap();
		let model = spec.properties.iter().find(|property| property.name == "model_path").unwrap();
		assert!(engine.required);
		assert_eq!(engine.default.0, serde_json::json!("vosk"));
		assert!(model.required);
		assert_eq!(model.default.0, serde_json::json!("vosk-model-small-ja-0.22"));
	}

	#[tokio::test]
	async fn web_input_ingress_forwards_content_to_log_via_external_trigger() {
		// graph: WebInputIngress --content--> Log
		//                         --exec_out--> Log.exec_in
		let mut b = FlowgraphBuilder::new();
		b.add_node("in", NodeImpl::pure(Arc::new(WebInputIngressNode)), InputMap::new());
		b.add_node("log", NodeImpl::effectful(Arc::new(LogNode)), InputMap::new());
		b.connect_exec(PortRef::new("in", "exec_out"), PortRef::new("log", "exec_in"));
		b.connect(PortRef::new("in", "content"), PortRef::new("log", "value"));
		let mut prog = b.build().expect("build");

		let (handle, rx) = create_trigger_bus();
		let external = handle.clone();
		let mut ctx = ExecCtx::default();

		// 250ms 後に shutdown、その前に 1 回 trigger を打つ
		let sender = tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(30)).await;
			let _ = external.send(
				TriggerEvent::new("in")
					.with_exec("__trigger__")
					.with_override("__content__", SocketValue::String("hello via web".into()))
					.with_override("__source_actor__", SocketValue::String("alice".into())),
			);
		});
		let shutdown = tokio::time::sleep(Duration::from_millis(150));
		prog.run_forever_with_bus(&mut ctx, handle, rx, shutdown, None)
			.await
			.expect("run_forever_with_bus");
		sender.await.unwrap();

		assert_eq!(ctx.trace.len(), 1);
		assert!(ctx.trace[0].contains("hello via web"), "trace: {:?}", ctx.trace);
	}

	#[tokio::test]
	async fn multiple_external_triggers_process_sequentially() {
		let mut b = FlowgraphBuilder::new();
		b.add_node("in", NodeImpl::pure(Arc::new(TwitchIngressNode)), InputMap::new());
		b.add_node("log", NodeImpl::effectful(Arc::new(LogNode)), InputMap::new());
		b.connect_exec(PortRef::new("in", "exec_out"), PortRef::new("log", "exec_in"));
		b.connect(PortRef::new("in", "content"), PortRef::new("log", "value"));
		let mut prog = b.build().expect("build");

		let (handle, rx) = create_trigger_bus();
		let external = handle.clone();
		let mut ctx = ExecCtx::default();

		let sender = tokio::spawn(async move {
			for i in 0..3 {
				tokio::time::sleep(Duration::from_millis(10)).await;
				let _ = external.send(
					TriggerEvent::new("in")
						.with_exec("__trigger__")
						.with_override("__content__", SocketValue::String(format!("chat {i}"))),
				);
			}
		});
		let shutdown = tokio::time::sleep(Duration::from_millis(150));
		prog.run_forever_with_bus(&mut ctx, handle, rx, shutdown, None)
			.await
			.expect("run_forever_with_bus");
		sender.await.unwrap();

		assert_eq!(ctx.trace.len(), 3);
		assert!(ctx.trace[0].contains("chat 0"));
		assert!(ctx.trace[1].contains("chat 1"));
		assert!(ctx.trace[2].contains("chat 2"));
	}

	#[tokio::test]
	async fn twitch_ingress_accepts_json_meta_without_stopping_worker() {
		let mut b = FlowgraphBuilder::new();
		b.add_node("in", NodeImpl::pure(Arc::new(TwitchIngressNode)), InputMap::new());
		b.add_node("log", NodeImpl::effectful(Arc::new(LogNode)), InputMap::new());
		b.connect_exec(PortRef::new("in", "exec_out"), PortRef::new("log", "exec_in"));
		b.connect(PortRef::new("in", "content"), PortRef::new("log", "value"));
		let mut prog = b.build().expect("build");

		let (handle, rx) = create_trigger_bus();
		let external = handle.clone();
		let mut ctx = ExecCtx::default();
		let sender = tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(30)).await;
			external
				.send(
					TriggerEvent::new("in")
						.with_exec("__trigger__")
						.with_override("__content__", SocketValue::String("first".into()))
						.with_override("__meta__", SocketValue::Json(serde_json::json!({ "channel_login": "mychan" }))),
				)
				.expect("first trigger");
			tokio::time::sleep(Duration::from_millis(30)).await;
			external
				.send(
					TriggerEvent::new("in")
						.with_exec("__trigger__")
						.with_override("__content__", SocketValue::String("second".into()))
						.with_override("__meta__", SocketValue::Json(serde_json::json!({ "channel_login": "mychan" }))),
				)
				.expect("second trigger");
		});
		let shutdown = tokio::time::sleep(Duration::from_millis(150));
		prog.run_forever_with_bus(&mut ctx, handle, rx, shutdown, None)
			.await
			.expect("worker remains healthy");
		sender.await.unwrap();

		assert_eq!(ctx.trace.len(), 2);
		assert!(ctx.trace[0].contains("first"));
		assert!(ctx.trace[1].contains("second"));
	}

	#[tokio::test]
	async fn ingress_without_trigger_is_noop() {
		let node = VoiceIngressNode;
		let out = node
			.compute(
				&crate::flowgraph::node::PureEvalHost::default(),
				&InputMap::new(),
				&InputMap::new(),
				&ExecFireSet::new(),
			)
			.await
			.unwrap();
		assert!(out.fired_exec.is_empty());
		assert!(out.data.is_empty());
	}

	#[tokio::test]
	async fn vmc_udp_ingress_echoes_base64_content() {
		let mut b = FlowgraphBuilder::new();
		b.add_node("vmc", NodeImpl::pure(Arc::new(VmcUdpIngressNode)), InputMap::new());
		b.add_node("log", NodeImpl::effectful(Arc::new(LogNode)), InputMap::new());
		b.connect_exec(PortRef::new("vmc", "exec_out"), PortRef::new("log", "exec_in"));
		b.connect(PortRef::new("vmc", "content"), PortRef::new("log", "value"));
		let mut prog = b.build().expect("build");

		let (handle, rx) = create_trigger_bus();
		let external = handle.clone();
		let mut ctx = ExecCtx::default();

		let sender = tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(30)).await;
			let _ = external.send(
				TriggerEvent::new("vmc")
					.with_exec("__trigger__")
					.with_override("__content__", SocketValue::String("QUJD".into()))
					.with_override("__source_actor__", SocketValue::String("127.0.0.1:39539".into()))
					.with_override("__source_kind__", SocketValue::String("vmc_udp".into())),
			);
		});
		let shutdown = tokio::time::sleep(Duration::from_millis(150));
		prog.run_forever_with_bus(&mut ctx, handle, rx, shutdown, None)
			.await
			.expect("run_forever_with_bus");
		sender.await.unwrap();

		assert_eq!(ctx.trace.len(), 1);
		assert!(ctx.trace[0].contains("QUJD"), "trace: {:?}", ctx.trace);
	}

	#[tokio::test]
	async fn vmc_udp_ingress_echoes_content_bytes() {
		let mut inputs = InputMap::new();
		inputs.insert("__content__".into(), SocketValue::String("QUJD".into()));
		inputs.insert("__content_bytes__".into(), SocketValue::Bytes(vec![65, 66, 67]));
		inputs.insert("__source_kind__".into(), SocketValue::String("vmc_udp".into()));
		let mut fired = ExecFireSet::new();
		fired.insert("__trigger__");
		let out = VmcUdpIngressNode
			.compute(&crate::flowgraph::node::PureEvalHost::default(), &InputMap::new(), &inputs, &fired)
			.await
			.unwrap();
		assert_eq!(out.data.get("content"), Some(&SocketValue::String("QUJD".into())));
		assert_eq!(out.data.get("content_bytes"), Some(&SocketValue::Bytes(vec![65, 66, 67])));
	}

	#[tokio::test]
	async fn unmfz_ingress_echoes_unmf_and_key() {
		let mut frame = vac_motion::unmf::UnMotionFrame::new(42);
		frame.header.sequence = 42;
		let mut inputs = InputMap::new();
		inputs.insert("__unmf__".into(), SocketValue::Unmf(frame.clone()));
		inputs.insert("__content__".into(), SocketValue::String("{}".into()));
		inputs.insert("__content_bytes__".into(), SocketValue::Bytes(vec![1, 2, 3]));
		inputs.insert("__key_expr__".into(), SocketValue::String("un-motion/frame/v1".into()));
		let mut fired = ExecFireSet::new();
		fired.insert("__trigger__");
		let out = UnmfzIngressNode
			.compute(&crate::flowgraph::node::PureEvalHost::default(), &InputMap::new(), &inputs, &fired)
			.await
			.unwrap();
		assert_eq!(out.data.get("unmf"), Some(&SocketValue::Unmf(frame)));
		assert_eq!(out.data.get("key_expr"), Some(&SocketValue::String("un-motion/frame/v1".into())));
		assert_eq!(out.data.get("content_bytes"), Some(&SocketValue::Bytes(vec![1, 2, 3])));
	}
}

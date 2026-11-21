//! δ-9 Part B: Flowgraph ランタイムの外部ブリッジ層。
//!
//! ## 責務
//!
//! - `flowgraph.ingress.*` ノードの `properties` を走査し、実行中の program に
//!   `TriggerEvent` を送り込む「入口」を提供する。
//! - 対応 ingress:
//!   - [`web_input`]: HTTP POST/GET を actix-web にぶら下げる。
//!   - [`voice`] (δ-9 Part E.2): Vosk / Whisper を起動し、確定テキストを TriggerEvent で流す。
//!     実装は V1 と共用の `processor::voice::VoiceSink` 抽象を経由する。
//!   - [`twitch`] (stub): IRC / EventSub 起動（実装は V1 processor 側を再利用する予定）。
//!   - [`ai_output`]: AI内部datumをpersonaと用途で絞り、専用ingressへ投入する。
//!   - `file_watch` (RI-0): file / directory polling watcher。create / modify / delete を `TriggerEvent` として投入。
//!   - `vmc_ingress` (Phase M1): `flowgraph.ingress.vmc_udp` — 生 UDP を受信し Base64 化して `TriggerEvent` 投入。
//!   - `osc_ingress` (Phase ρ): `flowgraph.ingress.osc_udp` — 汎用 OSC/UDP。メタに `profile: "osc_udp"`（VMC と分離）。
//!   - `unmfz_ingress` (Phase M7): `flowgraph.ingress.unmfz` — UNMF/Z Zenoh frame を常時購読して `unmf` として投入。
//!
//! ## 典型的な使い方
//!
//! 1. [`collect_all`] で `FlowgraphRuntime::node_meta` から ingress エントリ一覧を作る。
//! 2. actix-web の `configure` コールバック内で [`web_input::register_routes`] を呼ぶ。
//! 3. voice / twitch ingress は起動時に個別に spawn する（今後実装）。
//!
//! 全ブリッジは `TriggerHandle` を clone して所有するだけなので、program ワーカーとは
//! 非同期疎結合。program 停止時に `TriggerHandle.send` が失敗したら warn ログを出して落ちる。
//!
//! ## Crate 分割時の依存契約（Step 4）
//!
//! - **許容**: `flowgraph`（`loader` / `node` / `socket` 等）、`state` の共有型、`shutdown`（ingress の寿命）、`processor`（voice）。
//! - **禁止**: `motion` への参照（UDP パススルーは motion 層が単独で完結）。`web_interface` へも直接依存しない。
//! 詳細表は [`docs/architecture.md`](../../docs/architecture.md)「レイヤ境界（Step 4）」。

pub mod ai_output;
pub mod file_watch;
pub mod osc_ingress;
pub mod twitch;
pub mod twitch_eventsub;
pub mod twitch_stream_state;
pub mod unmfz_ingress;
pub mod vmc_ingress;
pub mod voice;
pub mod web_input;

use crate::flowgraph::loader::LoadedNodeMeta;
use crate::state::ChannelDatum;
use crate::SharedState;
use std::collections::HashMap;
use tokio::sync::broadcast;

/// ingress ノード種別ごとに整形した「接続先メタデータ」の束。
#[derive(Debug, Default, Clone)]
pub struct BridgeCatalog {
	pub web_input: Vec<web_input::FlowgraphWebInputEndpoint>,
	pub voice: Vec<voice::FlowgraphVoiceIngress>,
	pub twitch: Vec<twitch::FlowgraphTwitchIngress>,
	pub twitch_eventsub: Vec<twitch_eventsub::FlowgraphTwitchEventsubIngress>,
	pub twitch_stream_state: Vec<twitch_stream_state::FlowgraphTwitchStreamState>,
	pub ai_output: Vec<ai_output::FlowgraphAiOutput>,
	pub file_watch: Vec<file_watch::FlowgraphFileWatchIngress>,
	pub vmc_udp: Vec<vmc_ingress::FlowgraphVmcUdpIngress>,
	pub osc_udp: Vec<osc_ingress::FlowgraphOscUdpIngress>,
	pub unmfz: Vec<unmfz_ingress::FlowgraphUnmfzIngress>,
}

impl BridgeCatalog {
	pub fn is_empty(&self) -> bool {
		self.web_input.is_empty()
			&& self.voice.is_empty()
			&& self.twitch.is_empty()
			&& self.twitch_eventsub.is_empty()
			&& self.twitch_stream_state.is_empty()
			&& self.ai_output.is_empty()
			&& self.file_watch.is_empty()
			&& self.vmc_udp.is_empty()
			&& self.osc_udp.is_empty()
			&& self.unmfz.is_empty()
	}

	pub fn len(&self) -> usize {
		self.web_input.len()
			+ self.voice.len()
			+ self.twitch.len()
			+ self.twitch_eventsub.len()
			+ self.twitch_stream_state.len()
			+ self.ai_output.len()
			+ self.file_watch.len()
			+ self.vmc_udp.len()
			+ self.osc_udp.len()
			+ self.unmfz.len()
	}
}

/// ζ-3: Flowgraph 実行中に走っている bridge ワーカーたちをまとめた lifecycle 管理用ハンドル。
///
/// - 初回起動時: `app_core::boot` が [`spawn_all_from_state`] を呼び、戻りを `State` に格納する。
/// - reload 時: [`web_interface::control::flowgraph::reload::reload_runtime_with_restart_status`] が旧 handles を取り出して
///   [`BridgeHandles::finish_all`] で停止 → 新 runtime 上で再 spawn → 入れ替え、という流れ。
///
/// `web_input_snapshot` は actix HTTP server を再起動できない制約上、差分検出用にだけ持つ。
/// 差分があれば GUI 側に [`crate::control_events::ControlEvent::RestartRecommended`]
/// をブロードキャストして再起動を促す。
pub struct BridgeHandles {
	pub(crate) twitch: Vec<twitch::TwitchBridgeHandle>,
	pub(crate) twitch_eventsub: Vec<twitch_eventsub::TwitchEventsubBridgeHandle>,
	pub(crate) twitch_stream_state: Vec<tokio::task::JoinHandle<()>>,
	pub(crate) voice: Vec<crate::processor::voice::VoiceIngress>,
	pub(crate) ai_output: Vec<tokio::task::JoinHandle<()>>,
	pub(crate) file_watch: Vec<tokio::task::JoinHandle<()>>,
	pub(crate) vmc_udp: Vec<tokio::task::JoinHandle<()>>,
	pub(crate) osc_udp: Vec<tokio::task::JoinHandle<()>>,
	pub(crate) unmfz: Vec<tokio::task::JoinHandle<()>>,
	/// actix に登録済みの web_input エンドポイントのスナップショット。reload 差分検出専用。
	pub(crate) web_input_snapshot: Vec<web_input::FlowgraphWebInputEndpoint>,
}

impl Default for BridgeHandles {
	fn default() -> Self {
		Self::empty()
	}
}

impl std::fmt::Debug for BridgeHandles {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("BridgeHandles")
			.field("twitch", &self.twitch.len())
			.field("twitch_eventsub", &self.twitch_eventsub.len())
			.field("twitch_stream_state", &self.twitch_stream_state.len())
			.field("voice", &self.voice.len())
			.field("ai_output", &self.ai_output.len())
			.field("file_watch", &self.file_watch.len())
			.field("vmc_udp", &self.vmc_udp.len())
			.field("osc_udp", &self.osc_udp.len())
			.field("unmfz", &self.unmfz.len())
			.field("web_input_snapshot", &self.web_input_snapshot.len())
			.finish()
	}
}

impl BridgeHandles {
	pub fn empty() -> Self {
		Self {
			twitch: Vec::new(),
			twitch_eventsub: Vec::new(),
			twitch_stream_state: Vec::new(),
			voice: Vec::new(),
			ai_output: Vec::new(),
			file_watch: Vec::new(),
			vmc_udp: Vec::new(),
			osc_udp: Vec::new(),
			unmfz: Vec::new(),
			web_input_snapshot: Vec::new(),
		}
	}

	pub(crate) fn voice_worker_count(&self) -> usize {
		self.voice.len()
	}

	pub(crate) fn voice_worker_statuses(&self) -> Vec<crate::processor::voice::VoiceWorkerStatus> {
		self.voice.iter().map(crate::processor::voice::VoiceIngress::status).collect()
	}

	pub(crate) fn twitch_irc_worker_count(&self) -> usize {
		self.twitch.len()
	}

	pub(crate) fn twitch_irc_ready_worker_count(&self) -> usize {
		self.twitch.iter().filter(|handle| handle.is_ready()).count()
	}

	pub(crate) fn twitch_irc_channel_counts(&self) -> (usize, usize) {
		self.twitch.iter().fold((0, 0), |(joined, expected), handle| {
			(joined + handle.joined_channel_count(), expected + handle.expected_channel_count())
		})
	}

	pub(crate) fn twitch_eventsub_worker_count(&self) -> usize {
		self.twitch_eventsub.len()
	}

	pub(crate) fn release_voice_speech_floors(&self) {
		for voice in &self.voice {
			voice.release_speech_floor();
		}
	}

	/// 所有しているハンドルを全て graceful shutdown する。個々の finish は 5 秒タイムアウト。
	pub async fn finish_all(self) {
		for h in self.twitch_eventsub {
			let node_id = h.node_id().to_string();
			if tokio::time::timeout(std::time::Duration::from_secs(5), h.finish()).await.is_err() {
				log::warn!("《Bridges》 twitch_eventsub finish タイムアウト node={}", node_id);
			}
		}
		for h in self.twitch {
			if tokio::time::timeout(std::time::Duration::from_secs(5), h.finish()).await.is_err() {
				log::warn!("《Bridges》 twitch IRC finish タイムアウト");
			}
		}
		for h in self.voice {
			if tokio::time::timeout(std::time::Duration::from_secs(5), h.finish()).await.is_err() {
				log::warn!("《Bridges》 voice finish タイムアウト");
			}
		}
		for h in self.ai_output {
			h.abort();
		}
		for h in self.twitch_stream_state {
			h.abort();
		}
		for h in self.file_watch {
			h.abort();
		}
		for h in self.vmc_udp {
			h.abort();
		}
		for h in self.osc_udp {
			h.abort();
		}
		for h in self.unmfz {
			h.abort();
		}
	}
}

/// OAuth 更新後に、認証情報へ依存する Twitch bridge だけを再起動する。
///
/// Flowgraph 全体の reload は音声認識や AI 出力まで中断するため、DCF 完了時には使わない。
pub async fn restart_twitch_from_state(state: &SharedState) {
	let (catalog, trigger, bridge_handles, runtime_health) = {
		let s = state.read().await;
		let fg = s.flowgraph.read().await;
		let (catalog, trigger) = match fg.as_ref() {
			Some(rt) => (collect_all(&rt.node_meta), rt.trigger()),
			None => (BridgeCatalog::default(), None),
		};
		(catalog, trigger, s.bridge_handles.clone(), s.runtime_health.clone())
	};

	let mut handles = bridge_handles.lock().await;
	let old_eventsub = std::mem::take(&mut handles.twitch_eventsub);
	let old_irc = std::mem::take(&mut handles.twitch);

	for handle in old_eventsub {
		let node_id = handle.node_id().to_string();
		if tokio::time::timeout(std::time::Duration::from_secs(5), handle.finish())
			.await
			.is_err()
		{
			log::warn!("《Bridges》 OAuth反映時の twitch_eventsub finish タイムアウト node={}", node_id);
		}
	}
	for handle in old_irc {
		if tokio::time::timeout(std::time::Duration::from_secs(5), handle.finish())
			.await
			.is_err()
		{
			log::warn!("《Bridges》 OAuth反映時の twitch IRC finish タイムアウト");
		}
	}

	runtime_health.replace_component("twitch_irc", Vec::new()).await;
	handles.twitch = twitch::spawn(&catalog.twitch, trigger.clone(), state.clone()).await;
	runtime_health.replace_component("twitch_eventsub", Vec::new()).await;
	handles.twitch_eventsub = twitch_eventsub::spawn(&catalog.twitch_eventsub, trigger, state.clone());

	log::info!(
		"《Bridges》 OAuth更新を反映: twitch IRC={} / EventSub={}",
		handles.twitch.len(),
		handles.twitch_eventsub.len()
	);
}

fn voice_start_failure_details(entry: Option<&voice::FlowgraphVoiceIngress>) -> (&'static str, &'static str, &'static str) {
	match entry {
		Some(entry) if entry.model_path.trim().is_empty() && entry.engine == "vosk" => (
			"voice_vosk_model_missing",
			"Voskの音声認識モデルが設定されていない。",
			"右側のInspectorでModel PathのSetを押し、`vosk-model-small-ja-0.22`を入力する。続けてSave、Reloadの順に実行する。",
		),
		Some(entry) if entry.model_path.trim().is_empty() && matches!(entry.engine.as_str(), "whisper" | "whisper-rs") => (
			"voice_whisper_model_missing",
			"Whisperの音声認識モデルが設定されていない。",
			"右側のInspectorでModel PathにWhisper GGMLモデルのパスを設定する。続けてSave、Reloadの順に実行する。",
		),
		Some(entry) if entry.engine == "vosk" && !cfg!(feature = "voice-vosk") => (
			"voice_vosk_backend_unavailable",
			"このUNVACビルドにはVosk音声認識が含まれていない。",
			"Voskを含むrelease variantを使用する。公式のportableとminimalはどちらもVoskを含む。",
		),
		Some(entry) if matches!(entry.engine.as_str(), "whisper" | "whisper-rs") && !cfg!(feature = "voice-whisper") => (
			"voice_whisper_backend_unavailable",
			"このUNVACビルドにはWhisper音声認識が含まれていない。",
			"Vulkan workerとCPU fallbackを含むportable release variantを使用する。",
		),
		Some(entry) if !matches!(entry.engine.as_str(), "vosk" | "whisper" | "whisper-rs") => (
			"voice_engine_unknown",
			"音声認識エンジンの指定を認識できない。",
			"右側のInspectorでEngineを`vosk`または`whisper`に設定する。続けてSave、Reloadの順に実行する。",
		),
		_ => (
			"voice_worker_start_failed",
			"音声認識ワーカーを開始できない。",
			"右側のInspectorでEngineとModel Pathを確認する。変更後はSave、Reloadの順に実行する。",
		),
	}
}

/// ζ-3: `SharedState` 配下の flowgraph runtime を読み、全 bridge を新しい trigger で spawn する。
///
/// 戻りには web_input のスナップショットも入るが、actix HTTP server 側の route は
/// 起動時に固定されるため、新しいエンドポイントが追加された場合は呼び出し側で差分を見て
/// `RestartRecommended` 通知を出すこと（ここでは投げない）。
pub async fn spawn_all_from_state(state: &SharedState, channel_datum_tx: &broadcast::Sender<ChannelDatum>) -> BridgeHandles {
	let (catalog, trigger) = {
		let s = state.read().await;
		let fg = s.flowgraph.read().await;
		match fg.as_ref() {
			Some(rt) => (collect_all(&rt.node_meta), rt.trigger()),
			None => (BridgeCatalog::default(), None),
		}
	};

	if !catalog.is_empty() {
		log::info!(
			"《Flowgraph/Bridges》 ingress 合計 {} 件: web_input={}, voice={}, twitch={}, twitch_eventsub={}, twitch_stream_state={}, ai_output={}, file_watch={}, vmc_udp={}, osc_udp={}, unmfz={}",
			catalog.len(),
			catalog.web_input.len(),
			catalog.voice.len(),
			catalog.twitch.len(),
			catalog.twitch_eventsub.len(),
			catalog.twitch_stream_state.len(),
			catalog.ai_output.len(),
			catalog.file_watch.len(),
			catalog.vmc_udp.len(),
			catalog.osc_udp.len(),
			catalog.unmfz.len()
		);
	}

	let (shutdown, unmfz_transport) = {
		let s = state.read().await;
		(s.shutdown.clone(), s.unmfz_transport.clone())
	};

	let tokio_handle = tokio::runtime::Handle::current();
	let (control_event_tx, speech_floor, voice_input) = {
		let s = state.read().await;
		(s.control_event_tx.clone(), s.speech_floor.clone(), s.voice_input.clone())
	};
	let (voice, failed_voice_nodes) = voice::spawn(
		&catalog.voice,
		trigger.clone(),
		tokio_handle,
		control_event_tx,
		speech_floor,
		voice_input,
		std::sync::Arc::downgrade(state),
	);
	let runtime_health = state.read().await.runtime_health.clone();
	let voice_issues = failed_voice_nodes
		.into_iter()
		.map(|node| {
			let entry = catalog.voice.iter().find(|entry| entry.node_id == node);
			let (code, message, resolution) = voice_start_failure_details(entry);
			crate::runtime_health::RuntimeIssue::error(
				format!("voice:{node}"),
				"voice",
				code,
				message,
				"この音声入力から Flowgraph へ発話を渡せない。",
				"flowgraph",
				Some(node),
			)
			.with_resolution(resolution)
		})
		.collect();
	runtime_health.replace_component("voice", voice_issues).await;
	runtime_health.replace_component("twitch_irc", Vec::new()).await;
	let twitch = twitch::spawn(&catalog.twitch, trigger.clone(), state.clone()).await;
	runtime_health.replace_component("twitch_eventsub", Vec::new()).await;
	let twitch_eventsub = twitch_eventsub::spawn(&catalog.twitch_eventsub, trigger.clone(), state.clone());
	let twitch_stream_state = twitch_stream_state::spawn(&catalog.twitch_stream_state, trigger.clone(), channel_datum_tx);
	let ai_output = ai_output::spawn(&catalog.ai_output, trigger.clone(), channel_datum_tx);
	let file_watch = file_watch::spawn(&catalog.file_watch, trigger.clone(), shutdown.clone());
	let vmc_udp = vmc_ingress::spawn(&catalog.vmc_udp, trigger.clone(), shutdown.clone());
	let osc_udp = osc_ingress::spawn(&catalog.osc_udp, trigger.clone(), shutdown.clone());
	let unmfz = unmfz_ingress::spawn(&catalog.unmfz, trigger.clone(), shutdown, unmfz_transport);

	BridgeHandles {
		twitch,
		twitch_eventsub,
		twitch_stream_state,
		voice,
		ai_output,
		file_watch,
		vmc_udp,
		osc_udp,
		unmfz,
		web_input_snapshot: catalog.web_input,
	}
}

/// ζ-3: `web_input_snapshot` 間の差分（method + path の集合）が実質的に変化したかを返す。
///
/// actix-web は route 登録を再起動無しに差し替えられないため、reload 時に本判定で変化を検出したら
/// `RestartRecommended` 通知を出して再起動を促す。
pub fn web_input_changed(old: &[web_input::FlowgraphWebInputEndpoint], new: &[web_input::FlowgraphWebInputEndpoint]) -> bool {
	fn key(ep: &web_input::FlowgraphWebInputEndpoint) -> (String, web_input::Method) {
		(ep.path.clone(), ep.method)
	}
	let mut old_set: std::collections::BTreeSet<_> = old.iter().map(key).collect();
	let mut new_set: std::collections::BTreeSet<_> = new.iter().map(key).collect();
	// 対称差が空ならば変化なし。
	old_set.retain(|k| !new_set.remove(k));
	!(old_set.is_empty() && new_set.is_empty())
}

/// `FlowgraphRuntime::node_meta` を走査して ingress エントリを全部集める。
pub fn collect_all(node_meta: &HashMap<String, LoadedNodeMeta>) -> BridgeCatalog {
	let mut cat = BridgeCatalog::default();
	for (fq, meta) in node_meta {
		match meta.feature.as_str() {
			"flowgraph.ingress.web_input" => {
				if let Some(ep) = web_input::FlowgraphWebInputEndpoint::from_meta(fq, meta) {
					cat.web_input.push(ep);
				}
			}
			"flowgraph.ingress.voice" => {
				if let Some(v) = voice::FlowgraphVoiceIngress::from_meta(fq, meta) {
					cat.voice.push(v);
				}
			}
			"flowgraph.ingress.twitch" => {
				if let Some(t) = twitch::FlowgraphTwitchIngress::from_meta(fq, meta) {
					cat.twitch.push(t);
				}
			}
			"flowgraph.ingress.twitch_eventsub" => {
				if let Some(t) = twitch_eventsub::FlowgraphTwitchEventsubIngress::from_meta(fq, meta) {
					cat.twitch_eventsub.push(t);
				}
			}
			"flowgraph.ingress.twitch_stream_state" => {
				cat.twitch_stream_state
					.push(twitch_stream_state::FlowgraphTwitchStreamState::from_meta(fq, meta));
			}
			"flowgraph.ingress.ai_response" => {
				cat.ai_output
					.push(ai_output::FlowgraphAiOutput::from_meta(fq, meta, ai_output::AiOutputKind::Response));
			}
			"flowgraph.ingress.ai_speech" => {
				cat.ai_output
					.push(ai_output::FlowgraphAiOutput::from_meta(fq, meta, ai_output::AiOutputKind::Speech));
			}
			"flowgraph.ingress.ai_avatar_intent" => {
				cat.ai_output.push(ai_output::FlowgraphAiOutput::from_meta(
					fq,
					meta,
					ai_output::AiOutputKind::AvatarIntent,
				));
			}
			"flowgraph.ingress.ai_avatar_pose" => {
				cat.ai_output.push(ai_output::FlowgraphAiOutput::from_meta(
					fq,
					meta,
					ai_output::AiOutputKind::AvatarPose,
				));
			}
			"flowgraph.ingress.file_watch" => {
				if let Some(w) = file_watch::FlowgraphFileWatchIngress::from_meta(fq, meta) {
					cat.file_watch.push(w);
				}
			}
			"flowgraph.ingress.vmc_udp" => {
				if let Some(v) = vmc_ingress::FlowgraphVmcUdpIngress::from_meta(fq, meta) {
					cat.vmc_udp.push(v);
				}
			}
			"flowgraph.ingress.osc_udp" => {
				if let Some(v) = osc_ingress::FlowgraphOscUdpIngress::from_meta(fq, meta) {
					cat.osc_udp.push(v);
				}
			}
			"flowgraph.ingress.unmfz" => {
				if let Some(v) = unmfz_ingress::FlowgraphUnmfzIngress::from_meta(fq, meta) {
					cat.unmfz.push(v);
				}
			}
			_ => {}
		}
	}
	cat
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::bridges::web_input::{BodyFormat, FlowgraphWebInputEndpoint, Method};

	fn ep(path: &str, method: Method) -> FlowgraphWebInputEndpoint {
		FlowgraphWebInputEndpoint {
			node_id: format!("n{}", path),
			path: path.to_string(),
			method,
			body_format: BodyFormat::Plain,
			fixed_channel: String::new(),
		}
	}

	#[test]
	fn web_input_changed_detects_add_remove_and_ignores_order() {
		let a = vec![ep("/x", Method::Post), ep("/y", Method::Get)];
		let b_reorder = vec![ep("/y", Method::Get), ep("/x", Method::Post)];
		assert!(!web_input_changed(&a, &b_reorder), "順序入れ替えは同一扱い");

		let b_add = vec![ep("/x", Method::Post), ep("/y", Method::Get), ep("/z", Method::Put)];
		assert!(web_input_changed(&a, &b_add), "追加は差分");

		let b_rm = vec![ep("/x", Method::Post)];
		assert!(web_input_changed(&a, &b_rm), "削除は差分");

		let b_method = vec![ep("/x", Method::Get), ep("/y", Method::Get)];
		assert!(web_input_changed(&a, &b_method), "method 違いは差分");

		assert!(!web_input_changed(&a, &a), "恒等は同一扱い");
		assert!(!web_input_changed(&[], &[]), "両空は同一扱い");
	}

	#[test]
	fn bridge_handles_default_is_empty() {
		let h = BridgeHandles::default();
		assert!(h.twitch.is_empty());
		assert!(h.twitch_eventsub.is_empty());
		assert!(h.twitch_stream_state.is_empty());
		assert!(h.voice.is_empty());
		assert!(h.ai_output.is_empty());
		assert!(h.file_watch.is_empty());
		assert!(h.vmc_udp.is_empty());
		assert!(h.osc_udp.is_empty());
		assert!(h.unmfz.is_empty());
		assert!(h.web_input_snapshot.is_empty());
	}

	#[test]
	fn bridge_handles_debug_shows_counts() {
		let h = BridgeHandles::empty();
		let s = format!("{h:?}");
		assert!(s.contains("BridgeHandles"));
		assert!(s.contains("twitch"));
	}

	#[test]
	fn missing_vosk_model_has_actionable_diagnostic() {
		let entry = voice::FlowgraphVoiceIngress {
			node_id: "voice/main::in".into(),
			label: String::new(),
			engine: "vosk".into(),
			model_path: String::new(),
			source: String::new(),
			sample_rate: 16000,
			language_code: "ja".into(),
			whisper_backend: "auto".into(),
			whisper_gpu_device: String::new(),
			grammar_json: String::new(),
			fixed_channel: "voice".into(),
			partial_channel: String::new(),
			partial_interval_ms: 800,
			partial_min_audio_ms: 1_300,
			speech_floor_key: String::new(),
			speech_floor_release_delay_ms: 700,
			vad_threshold_db: -45.0,
			vad_endpoint_ms: 600,
			vad_pre_roll_ms: 400,
			vad_min_speech_ms: 100,
		};
		let (code, message, resolution) = voice_start_failure_details(Some(&entry));
		assert_eq!(code, "voice_vosk_model_missing");
		assert!(message.contains("Vosk"));
		assert!(resolution.contains("vosk-model-small-ja-0.22"));
	}

	#[cfg(not(feature = "voice-whisper"))]
	#[test]
	fn whisper_without_feature_names_release_variants() {
		let entry = voice::FlowgraphVoiceIngress {
			node_id: "voice/main::in".into(),
			label: String::new(),
			engine: "whisper".into(),
			model_path: "ggml-medium.bin".into(),
			source: String::new(),
			sample_rate: 16000,
			language_code: "ja".into(),
			whisper_backend: "auto".into(),
			whisper_gpu_device: String::new(),
			grammar_json: String::new(),
			fixed_channel: "voice".into(),
			partial_channel: String::new(),
			partial_interval_ms: 800,
			partial_min_audio_ms: 1_300,
			speech_floor_key: String::new(),
			speech_floor_release_delay_ms: 700,
			vad_threshold_db: -45.0,
			vad_endpoint_ms: 600,
			vad_pre_roll_ms: 400,
			vad_min_speech_ms: 100,
		};
		let (code, message, resolution) = voice_start_failure_details(Some(&entry));
		assert_eq!(code, "voice_whisper_backend_unavailable");
		assert!(message.contains("Whisper"));
		assert!(resolution.contains("portable"));
	}
}

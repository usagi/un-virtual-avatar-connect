//! `flowgraph.ingress.unmfz` 用ブリッジ。
//!
//! UNMF/Z は motion data plane の本線なので、`flowgraph.unmfz.subscribe` の一回購読とは別に、
//! Zenoh subscriber を常駐させて受信フレームごとに `TriggerEvent` を投入する。

use crate::flowgraph::loader::LoadedNodeMeta;
use crate::flowgraph::node::{TriggerEvent, TriggerHandle};
use crate::flowgraph::socket::SocketValue;
use crate::shutdown::ShutdownBroker;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use un_motion_frame_zenoh::{Subscriber, TopicMode, ZenohTopicStrategy};

const DEFAULT_UNMFZ_BASE_KEY: &str = "un-motion/frame";
const DEFAULT_UNMFZ_SCHEMA_MAJOR: i64 = 1;

#[derive(Debug, Clone)]
pub struct FlowgraphUnmfzIngress {
	pub node_id: String,
	pub base_key_expr: String,
	pub topic_mode: String,
	pub schema_major: i64,
	pub poll_ms: i64,
	pub fixed_channel: String,
}

impl FlowgraphUnmfzIngress {
	pub fn from_meta(fq: &str, meta: &LoadedNodeMeta) -> Option<Self> {
		let p = &meta.properties;
		let get_str = |k: &str| -> String {
			p.get(k)
				.and_then(|v| v.as_str().ok())
				.map(str::trim)
				.map(str::to_string)
				.unwrap_or_default()
		};
		let get_int = |k: &str, default: i64| -> i64 { p.get(k).and_then(|v| v.as_i64().ok()).unwrap_or(default) };
		let base_key_expr = {
			let s = get_str("base_key_expr");
			if s.is_empty() {
				DEFAULT_UNMFZ_BASE_KEY.to_string()
			} else {
				s.trim_end_matches('/').to_string()
			}
		};
		Some(Self {
			node_id: fq.to_string(),
			base_key_expr,
			topic_mode: get_str("topic_mode"),
			schema_major: get_int("schema_major", DEFAULT_UNMFZ_SCHEMA_MAJOR),
			poll_ms: get_int("poll_ms", 10).clamp(1, 1000),
			fixed_channel: get_str("fixed_channel"),
		})
	}

	fn strategy(&self) -> Result<ZenohTopicStrategy, String> {
		let mode = match self.topic_mode.trim().to_ascii_lowercase().as_str() {
			"" | "frame" => TopicMode::Frame,
			"primary_source" | "by_primary_source" | "source" => TopicMode::ByPrimarySource,
			"stream_id" | "by_stream_id" | "stream" => TopicMode::ByStreamId,
			other => return Err(format!("unknown UNMF/Z topic_mode: {other}")),
		};
		let schema_major = u16::try_from(self.schema_major).map_err(|_| format!("schema_major out of u16 range: {}", self.schema_major))?;
		Ok(ZenohTopicStrategy::new(self.base_key_expr.clone(), mode).with_schema_major(schema_major))
	}
}

fn spawn_one(
	entry: FlowgraphUnmfzIngress,
	trigger: TriggerHandle,
	shutdown: Arc<ShutdownBroker>,
	transport: crate::unmfz_transport::UnmfzTransportRuntime,
) -> JoinHandle<()> {
	let node_id = entry.node_id.clone();
	let kind = if entry.fixed_channel.trim().is_empty() {
		"unmfz".to_string()
	} else {
		entry.fixed_channel.trim().to_string()
	};
	tokio::spawn(async move {
		let strategy = match entry.strategy() {
			Ok(strategy) => strategy,
			Err(e) => {
				log::error!("《Flowgraph/UNMFZ》 strategy 作成失敗 node_id={}: {}", node_id, e);
				return;
			}
		};
		let subscribe_key_expr = strategy.subscribe_key_expr();
		let mut backend = match transport.subscriber_backend() {
			Ok(backend) => backend,
			Err(e) => {
				log::error!("《Flowgraph/UNMFZ》 Zenoh open 失敗 node_id={}: {}", node_id, e);
				return;
			}
		};
		let subscriber = match Subscriber::declare(&mut backend, strategy) {
			Ok(subscriber) => subscriber,
			Err(e) => {
				log::error!(
					"《Flowgraph/UNMFZ》 subscriber 作成失敗 node_id={} key={}: {}",
					node_id,
					subscribe_key_expr,
					e
				);
				return;
			}
		};
		log::info!("《Flowgraph/UNMFZ》 ingress 起動 node_id={} key={}", node_id, subscribe_key_expr);
		let poll = Duration::from_millis(entry.poll_ms as u64);
		loop {
			tokio::select! {
				biased;
				_ = shutdown.wait() => {
					log::info!("《Flowgraph/UNMFZ》 ingress 終了 node_id={} key={}", node_id, subscribe_key_expr);
					break;
				}
				_ = tokio::time::sleep(poll) => {
					while let Some(message) = subscriber.try_recv_message() {
						let frame = match vac_motion::unmf::decode_unmfz_payload(&message.payload) {
							Ok(frame) => frame,
							Err(e) => {
								log::warn!("《Flowgraph/UNMFZ》 decode 失敗 node_id={} key={}: {}", node_id, message.key_expr, e);
								continue;
							}
						};
						let frame_json = vac_motion::unmf::unmf_to_json_value(&frame);
						let content = serde_json::to_string(&frame_json).unwrap_or_else(|_| "null".to_string());
						let meta = json!({
							"profile": "unmfz",
							"key_expr": message.key_expr,
							"subscribe_key_expr": subscribe_key_expr,
							"byte_len": message.payload.len(),
							"encoding": "unmfz_msgpack",
						});
						let ev = TriggerEvent::new(&node_id)
							.with_exec("__trigger__")
							.with_override("__unmf__", SocketValue::Unmf(frame))
							.with_override("__content__", SocketValue::String(content))
							.with_override("__content_bytes__", SocketValue::Bytes(message.payload))
							.with_override("__key_expr__", SocketValue::String(message.key_expr.clone()))
							.with_override("__source_actor__", SocketValue::String(message.key_expr))
							.with_override("__source_kind__", SocketValue::String(kind.clone()))
							.with_override("__meta__", SocketValue::Json(meta));
						if let Err(e) = trigger.send(ev) {
							log::warn!("《Flowgraph/UNMFZ》 TriggerEvent 送信失敗 node_id={}: {}", node_id, e);
						}
					}
				}
			}
		}
	})
}

pub fn spawn(
	entries: &[FlowgraphUnmfzIngress],
	trigger: Option<TriggerHandle>,
	shutdown: Arc<ShutdownBroker>,
	transport: crate::unmfz_transport::UnmfzTransportRuntime,
) -> Vec<JoinHandle<()>> {
	if entries.is_empty() {
		return Vec::new();
	}
	let Some(trigger) = trigger else {
		log::warn!(
			"《Flowgraph/UNMFZ》 ingress ノード {} 件を検出しましたが、Flowgraph ランタイムが無いため UNMF/Z ブリッジを開始できません。",
			entries.len()
		);
		return Vec::new();
	};
	entries
		.iter()
		.cloned()
		.map(|entry| spawn_one(entry, trigger.clone(), shutdown.clone(), transport.clone()))
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::flowgraph::loader::LoadedNodeMeta;
	use crate::flowgraph::node::InputMap;
	use crate::flowgraph::socket::SocketValue;
	use std::path::PathBuf;

	fn meta(props: Vec<(&str, SocketValue)>) -> LoadedNodeMeta {
		let mut m = InputMap::new();
		for (k, v) in props {
			m.insert(k.to_string(), v);
		}
		LoadedNodeMeta {
			feature: "flowgraph.ingress.unmfz".into(),
			file: PathBuf::from("t.flowgraph.toml"),
			position: None,
			activation_chain: Vec::new(),
			properties: m,
		}
	}

	#[test]
	fn from_meta_defaults_and_clamps() {
		let m = meta(vec![("poll_ms", SocketValue::Int(0))]);
		let v = FlowgraphUnmfzIngress::from_meta("n1", &m).expect("ingress");
		assert_eq!(v.node_id, "n1");
		assert_eq!(v.base_key_expr, "un-motion/frame");
		assert_eq!(v.poll_ms, 1);
		assert_eq!(v.schema_major, 1);
	}

	#[test]
	fn strategy_builds_subscribe_key() {
		let m = meta(vec![
			("base_key_expr", SocketValue::String("un-motion/frame/custom/".into())),
			("topic_mode", SocketValue::String("stream_id".into())),
			("schema_major", SocketValue::Int(2)),
		]);
		let v = FlowgraphUnmfzIngress::from_meta("n1", &m).expect("ingress");
		let strategy = v.strategy().expect("strategy");
		assert_eq!(strategy.subscribe_key_expr(), "un-motion/frame/custom/v2/**");
	}
}

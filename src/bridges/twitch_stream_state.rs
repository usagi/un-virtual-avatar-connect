//! 宣言済みTwitch配信状態を専用ingressへ渡す内部bridge。

use crate::flowgraph::loader::LoadedNodeMeta;
use crate::flowgraph::node::{TriggerEvent, TriggerHandle};
use crate::flowgraph::socket::SocketValue;
use crate::state::ChannelDatum;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct FlowgraphTwitchStreamState {
	pub node_id: String,
}

impl FlowgraphTwitchStreamState {
	pub fn from_meta(fq: &str, _meta: &LoadedNodeMeta) -> Self {
		Self { node_id: fq.to_owned() }
	}

	fn matches(datum: &ChannelDatum) -> bool {
		datum.channel == "twitch:stream_state" && datum.has_flag(ChannelDatum::FLAG_IS_FINAL)
	}

	fn build_trigger(&self, datum: &ChannelDatum) -> TriggerEvent {
		let meta = serde_json::to_value(&datum.meta).unwrap_or(serde_json::Value::Null);
		TriggerEvent::new(self.node_id.clone())
			.with_exec("__trigger__")
			.with_override("__content__", SocketValue::String(datum.content.clone()))
			.with_override("__source_kind__", SocketValue::String("twitch.stream_state".into()))
			.with_override("__meta__", SocketValue::Json(meta))
	}
}

pub fn spawn(
	entries: &[FlowgraphTwitchStreamState],
	trigger: Option<TriggerHandle>,
	channel_datum_tx: &broadcast::Sender<ChannelDatum>,
) -> Vec<tokio::task::JoinHandle<()>> {
	let Some(trigger) = trigger.filter(|_| !entries.is_empty()) else {
		return Vec::new();
	};
	entries
		.iter()
		.cloned()
		.map(|entry| {
			let trigger = trigger.clone();
			let mut rx = channel_datum_tx.subscribe();
			tokio::spawn(async move {
				loop {
					match rx.recv().await {
						Ok(datum) if FlowgraphTwitchStreamState::matches(&datum) => {
							if trigger.send(entry.build_trigger(&datum)).is_err() {
								return;
							}
						}
						Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
						Err(broadcast::error::RecvError::Closed) => return,
					}
				}
			})
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn accepts_only_final_declared_stream_state() {
		let final_state = ChannelDatum::new("twitch:stream_state".into(), "{}".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
		let partial = ChannelDatum::new("twitch:stream_state".into(), "{}".into());
		let chat = ChannelDatum::new("chat".into(), "hello".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
		assert!(FlowgraphTwitchStreamState::matches(&final_state));
		assert!(!FlowgraphTwitchStreamState::matches(&partial));
		assert!(!FlowgraphTwitchStreamState::matches(&chat));
	}

	#[test]
	fn trigger_forwards_structured_state_metadata() {
		let datum = ChannelDatum::new("twitch:stream_state".into(), "配信中".into())
			.with_flag(ChannelDatum::FLAG_IS_FINAL)
			.with_meta(
				"state",
				serde_json::json!({
					"title": {
						"value": "配信試験",
						"observed_at": "2026-08-10T00:00:00Z",
						"source": "helix_startup",
						"age_ms": 10,
						"fresh": true
					}
				}),
			);
		let trigger = FlowgraphTwitchStreamState { node_id: "state".into() }.build_trigger(&datum);
		let meta = trigger.data_overrides["__meta__"].as_json().unwrap();
		assert_eq!(meta["state"]["title"]["value"], "配信試験");
		assert_eq!(meta["state"]["title"]["fresh"], true);
	}
}

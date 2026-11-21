//! AI service の内部 `ChannelDatum` を目的別の明示 ingress へ変換する境界。

use crate::flowgraph::loader::LoadedNodeMeta;
use crate::flowgraph::node::{TriggerEvent, TriggerHandle};
use crate::flowgraph::socket::SocketValue;
use crate::state::ChannelDatum;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct FlowgraphAiOutput {
	pub node_id: String,
	pub persona_id: Option<String>,
	pub kind: AiOutputKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiOutputKind {
	Response,
	Speech,
	AvatarIntent,
	AvatarPose,
}

impl FlowgraphAiOutput {
	pub fn from_meta(fq: &str, meta: &LoadedNodeMeta, kind: AiOutputKind) -> Self {
		let persona_id = meta
			.properties
			.get("persona_id")
			.and_then(|value| value.as_str().ok())
			.map(str::trim)
			.filter(|value| !value.is_empty())
			.map(ToOwned::to_owned);
		Self {
			node_id: fq.to_owned(),
			persona_id,
			kind,
		}
	}

	pub fn matches(&self, datum: &ChannelDatum) -> bool {
		if !datum.has_flag(ChannelDatum::FLAG_IS_FINAL) {
			return false;
		}
		let expected_source_kind = match self.kind {
			AiOutputKind::Response => Some(ChannelDatum::SOURCE_AI_RESPONSE),
			AiOutputKind::Speech => Some(ChannelDatum::SOURCE_AI_SPEECH),
			AiOutputKind::AvatarIntent | AiOutputKind::AvatarPose => None,
		};
		if expected_source_kind.is_some_and(|expected| datum.source().is_none_or(|source| source.kind != expected)) {
			return false;
		}
		let persona_id = match self.kind {
			AiOutputKind::Response | AiOutputKind::Speech => datum.meta.get("persona_id"),
			AiOutputKind::AvatarIntent | AiOutputKind::AvatarPose => datum.meta.get("source_actor"),
		}
		.and_then(|value| value.as_str())
		.unwrap_or_default();
		if self.persona_id.as_deref().is_some_and(|expected| expected != persona_id) {
			return false;
		}
		match self.kind {
			AiOutputKind::Response => !datum.has_flag(ChannelDatum::FLAG_SPEECH_SEGMENT) && datum.meta.contains_key("persona_id"),
			AiOutputKind::Speech => datum.has_flag(ChannelDatum::FLAG_SPEECH_SEGMENT) && datum.meta.contains_key("persona_id"),
			AiOutputKind::AvatarIntent => datum.channel == "avatar_intent",
			AiOutputKind::AvatarPose => datum.channel == "avatar_pose",
		}
	}

	pub fn build_trigger(&self, datum: &ChannelDatum) -> TriggerEvent {
		let persona_id = datum
			.meta
			.get("persona_id")
			.or_else(|| datum.meta.get("source_actor"))
			.and_then(|value| value.as_str())
			.unwrap_or_default();
		let utterance_id = datum.meta.get("utterance_id").and_then(|value| value.as_str()).unwrap_or_default();
		let utterance_content = datum
			.meta
			.get("utterance_content")
			.and_then(|value| value.as_str())
			.unwrap_or(&datum.content);
		let meta = serde_json::to_value(&datum.meta).unwrap_or(serde_json::Value::Null);
		let mut trigger = TriggerEvent::new(self.node_id.clone())
			.with_exec("__trigger__")
			.with_override("__content__", SocketValue::String(datum.content.clone()))
			.with_override("__persona_id__", SocketValue::String(persona_id.to_owned()))
			.with_override("__utterance_id__", SocketValue::String(utterance_id.to_owned()))
			.with_override("__meta__", SocketValue::Json(meta));
		if self.kind == AiOutputKind::Speech {
			trigger = trigger.with_override("__utterance_content__", SocketValue::String(utterance_content.to_owned()));
		}
		trigger
	}
}

pub fn spawn(
	entries: &[FlowgraphAiOutput],
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
						Ok(datum) if entry.matches(&datum) => {
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
	fn response_and_speech_are_separate_and_persona_scoped() {
		let response = FlowgraphAiOutput {
			node_id: "response".into(),
			persona_id: Some("kaltsit".into()),
			kind: AiOutputKind::Response,
		};
		let speech = FlowgraphAiOutput {
			node_id: "speech".into(),
			persona_id: Some("kaltsit".into()),
			kind: AiOutputKind::Speech,
		};
		let full = ChannelDatum::new("ai".into(), "full".into())
			.with_flag(ChannelDatum::FLAG_IS_FINAL)
			.with_source(crate::state::DataSource::new(ChannelDatum::SOURCE_AI_RESPONSE).with_actor("kaltsit"))
			.with_meta("persona_id", "kaltsit");
		let segment = ChannelDatum::new("ai.speech".into(), "line".into())
			.with_flag(ChannelDatum::FLAG_IS_FINAL)
			.with_flag(ChannelDatum::FLAG_SPEECH_SEGMENT)
			.with_source(crate::state::DataSource::new(ChannelDatum::SOURCE_AI_SPEECH).with_actor("kaltsit"))
			.with_meta("persona_id", "kaltsit");
		assert!(response.matches(&full));
		assert!(!response.matches(&segment));
		assert!(!speech.matches(&full));
		assert!(speech.matches(&segment));
	}

	#[test]
	fn response_rejects_flowgraph_output_even_when_persona_meta_is_preserved() {
		let response = FlowgraphAiOutput {
			node_id: "response".into(),
			persona_id: Some("kaltsit".into()),
			kind: AiOutputKind::Response,
		};
		let transformed = ChannelDatum::new("ai-synth".into(), "translated".into())
			.with_flag(ChannelDatum::FLAG_IS_FINAL)
			.with_source(crate::state::DataSource::new("flowgraph.bos").with_actor("kaltsit"))
			.with_meta("persona_id", "kaltsit");
		assert!(!response.matches(&transformed));
	}

	#[test]
	fn speech_trigger_exposes_accumulated_utterance_for_live_subtitles() {
		let speech = FlowgraphAiOutput {
			node_id: "speech".into(),
			persona_id: Some("kaltsit".into()),
			kind: AiOutputKind::Speech,
		};
		let segment = ChannelDatum::new("ai.speech".into(), "二文目。".into())
			.with_flag(ChannelDatum::FLAG_IS_FINAL)
			.with_flag(ChannelDatum::FLAG_SPEECH_SEGMENT)
			.with_source(crate::state::DataSource::new(ChannelDatum::SOURCE_AI_SPEECH).with_actor("kaltsit"))
			.with_meta("persona_id", "kaltsit")
			.with_meta("utterance_content", "一文目。二文目。");
		let trigger = speech.build_trigger(&segment);
		assert_eq!(
			trigger.data_overrides.get("__utterance_content__"),
			Some(&SocketValue::String("一文目。二文目。".into()))
		);
	}
}

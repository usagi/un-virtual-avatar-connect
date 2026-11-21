//! EventSub facts -> configurable moderator notices. No AI or network calls.
use crate::flowgraph::node::{
	get_required_json, get_required_string, ExecFireSet, InputMap, NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec,
	PropertySpec, StatefulCtx, StatefulNode,
};
use crate::flowgraph::socket::{SocketType, SocketValue};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
	any::Any,
	collections::VecDeque,
	time::{Duration, Instant},
};

pub struct TwitchNoticeNode;
#[derive(Default)]
struct Seen(VecDeque<(String, Instant)>);
impl Seen {
	fn duplicate(&mut self, key: &str) -> bool {
		let now = Instant::now();
		self.0.retain(|(_, at)| now.duration_since(*at) < Duration::from_secs(600));
		if self.0.iter().any(|(id, _)| id == key) {
			return true;
		}
		if self.0.len() >= 4096 {
			self.0.pop_front();
		}
		self.0.push_back((key.into(), now));
		false
	}
}
fn text(v: &Value, key: &str) -> String {
	v.get(key)
		.and_then(Value::as_str)
		.unwrap_or("")
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
		.chars()
		.take(160)
		.collect()
}
fn notice(kind: &str, e: &Value, templates: &Value) -> Option<(String, String, String)> {
	let anonymous = e["is_anonymous"].as_bool() == Some(true);
	let actor = if anonymous {
		"匿名の方".into()
	} else {
		let n = text(e, "user_name");
		if n.is_empty() {
			text(e, "user_login")
		} else {
			n
		}
	};
	let (label, default, actor) = match kind {
		"channel.channel_points_custom_reward_redemption.add" => ("CHANNEL POINTS", "{user}さんが「{reward}」を交換した。", actor),
		"channel.follow" => ("FOLLOW", "{user}さん、フォローありがとう！", actor),
		"channel.subscribe" if e["is_gift"].as_bool() != Some(true) => ("SUBSCRIBE", "{user}さん、サブスクありがとう！", actor),
		"channel.subscription.message" => ("RESUBSCRIBE", "{user}さん、{months}か月のサブスクありがとう！", actor),
		"channel.subscription.gift" => ("GIFT SUBS", "{user}から{count}件のサブギフト。ありがとう！", actor),
		"channel.raid" => (
			"RAID",
			"{user}さんが{count}人とRaidに来てくれた。ようこそ！ 配信はこちら → {url}",
			text(e, "from_broadcaster_user_name"),
		),
		_ => return None,
	};
	if actor.is_empty() {
		return None;
	}
	let reward = text(&e["reward"], "title");
	if kind.ends_with("redemption.add") && reward.is_empty() {
		return None;
	}
	let login = text(e, "from_broadcaster_user_login");
	let url = if !login.is_empty() && login.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
		format!("https://www.twitch.tv/{login}")
	} else {
		String::new()
	};
	if kind == "channel.raid" && url.is_empty() {
		return None;
	}
	let count = e[if kind == "channel.raid" { "viewers" } else { "total" }].as_u64();
	let months = e["cumulative_months"].as_u64();
	if matches!(kind, "channel.raid" | "channel.subscription.gift") && count.is_none() {
		return None;
	}
	if kind == "channel.subscription.message" && months.is_none() {
		return None;
	}
	let values = [
		("user", actor),
		("reward", reward),
		("count", count.unwrap_or(0).to_string()),
		("months", months.unwrap_or(0).to_string()),
		("url", url),
	];
	let template = templates.get(kind).and_then(Value::as_str).unwrap_or(default);
	// Expand only template tokens: braces in viewer-controlled names stay literal.
	let mut result = String::new();
	let mut rest = template;
	while let Some(start) = rest.find('{') {
		result.push_str(&rest[..start]);
		let Some(end) = rest[start..].find('}') else {
			result.push_str(&rest[start..]);
			rest = "";
			break;
		};
		let key = &rest[start + 1..start + end];
		if let Some((_, value)) = values.iter().find(|(name, _)| *name == key) {
			result.push_str(value);
		} else {
			result.push_str(&rest[start..=start + end]);
		}
		rest = &rest[start + end + 1..];
	}
	result.push_str(rest);
	let result = result
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
		.chars()
		.take(450)
		.collect::<String>();
	if result.is_empty() {
		return None;
	}
	Some((result, label.into(), login))
}
impl NodeDescriptor for TwitchNoticeNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.twitch.event_notice".into(),
			title: "Twitch: Event Notice".into(),
			category: "twitch".into(),
			description: Some(
				"EventSubから通知文とBOS演出を生成。既定は日本語。テンプレートの空文字で通知を無効化。重複抑止はノード存続中10分/4096件。"
					.into(),
			),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("event_type", "Event Type", SocketType::String),
				PortSpec::input("payload", "Payload", SocketType::Json),
				PortSpec::input("meta", "Meta", SocketType::Json).with_default(SocketValue::Json(json!({}))),
			],
			outputs: vec![
				PortSpec::exec_output("on_notice", "On Notice"),
				PortSpec::exec_output("on_skip", "On Skip"),
				PortSpec::output("content", "Content", SocketType::String),
				PortSpec::output("broadcaster_id", "Broadcaster ID", SocketType::String),
				PortSpec::output("raid_login", "Raid Login", SocketType::String),
				PortSpec::output("flags", "Flags", SocketType::Json),
				PortSpec::output("meta", "Meta", SocketType::Json),
			],
			properties: vec![
				PropertySpec::new("templates", "Templates", SocketType::Json, SocketValue::Json(json!({})))
					.description("event_type → 文面。{user}, {reward}, {count}, {months}, {url}。"),
			],
		}
	}
}
#[async_trait]
impl StatefulNode for TwitchNoticeNode {
	fn init_state(&self) -> Box<dyn Any + Send> {
		Box::<Seen>::default()
	}
	async fn compute(
		&self,
		state: &mut (dyn Any + Send),
		props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
		_ctx: &StatefulCtx<'_>,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let kind = get_required_string(inputs, "event_type")?;
		let event = get_required_json(inputs, "payload")?;
		let meta = inputs.get("meta").and_then(|v| v.as_json().ok()).cloned().unwrap_or(json!({}));
		let templates = props.get("templates").and_then(|v| v.as_json().ok()).cloned().unwrap_or(json!({}));
		let Some((content, label, login)) = notice(&kind, &event, &templates) else {
			return Ok(NodeOutput::new().fire_exec("on_skip"));
		};
		let id = if kind.ends_with("redemption.add") {
			text(&event, "id")
		} else {
			text(&meta, "message_id")
		};
		// Missing event identity cannot safely authorize an external notice.
		if id.is_empty() || state.downcast_mut::<Seen>().unwrap().duplicate(&format!("{kind}:{id}")) {
			return Ok(NodeOutput::new().fire_exec("on_skip"));
		}
		let broadcaster = if kind == "channel.raid" {
			text(&event, "to_broadcaster_user_id")
		} else {
			text(&event, "broadcaster_user_id")
		};
		Ok(NodeOutput::new()
			.set_data("content", SocketValue::String(content))
			.set_data("broadcaster_id", SocketValue::String(broadcaster))
			.set_data("raid_login", SocketValue::String(login))
			.set_data("flags", SocketValue::Json(json!(["obs.event.notice"])))
			.set_data(
				"meta",
				SocketValue::Json(json!({"quick_effect_label":label,"quick_effect_duration_ms":6500,"event_type":kind,"event_id":id})),
			)
			.fire_exec("on_notice"))
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn redemption_is_receipt_not_execution_claim() {
		let (s, _, _) = notice(
			"channel.channel_points_custom_reward_redemption.add",
			&json!({"user_name":"Alice {url}","reward":{"title":"ズーム"}}),
			&json!({}),
		)
		.unwrap();
		assert_eq!(s, "Alice {url}さんが「ズーム」を交換した。");
	}
	#[test]
	fn gift_privacy_and_fanout() {
		let e = json!({"is_anonymous":true,"user_name":"private","total":10});
		let (s, _, _) = notice("channel.subscription.gift", &e, &json!({})).unwrap();
		assert!(!s.contains("private"));
		assert!(s.contains("10件"));
		assert!(notice("channel.subscribe", &json!({"user_name":"recipient","is_gift":true}), &json!({})).is_none());
	}
	#[test]
	fn raid_uses_source_and_does_not_invent_game() {
		let e = json!({"from_broadcaster_user_name":"Guest","from_broadcaster_user_login":"guest_1","viewers":12});
		let (s, _, _) = notice("channel.raid", &e, &json!({})).unwrap();
		assert!(s.contains("12人"));
		assert!(s.contains("https://www.twitch.tv/guest_1"));
		assert!(notice("channel.raid", &json!({"from_broadcaster_user_name":"Guest"}), &json!({})).is_none());
	}
	#[test]
	fn templates_and_dedup() {
		assert!(notice("channel.follow", &json!({"user_name":"A"}), &json!({"channel.follow":""})).is_none());
		let mut seen = Seen::default();
		assert!(!seen.duplicate("x"));
		assert!(seen.duplicate("x"));
		assert!(!seen.duplicate("y"));
	}
	#[tokio::test]
	async fn repeated_notification_fires_once_and_requires_identity() {
		let node = TwitchNoticeNode;
		let mut state = node.init_state();
		let ctx = StatefulCtx {
			node_id: "notice",
			trigger: None,
		};
		let mut input = InputMap::new();
		input.insert("event_type".into(), SocketValue::String("channel.follow".into()));
		input.insert(
			"payload".into(),
			SocketValue::Json(json!({"user_name":"A","broadcaster_user_id":"1"})),
		);
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		let out = node.compute(state.as_mut(), &InputMap::new(), &input, &fired, &ctx).await.unwrap();
		assert!(out.fired_exec.contains("on_skip"));
		input.insert("meta".into(), SocketValue::Json(json!({"message_id":"m1"})));
		let out = node.compute(state.as_mut(), &InputMap::new(), &input, &fired, &ctx).await.unwrap();
		assert!(out.fired_exec.contains("on_notice"));
		assert_eq!(out.data["broadcaster_id"], SocketValue::String("1".into()));
		let out = node.compute(state.as_mut(), &InputMap::new(), &input, &fired, &ctx).await.unwrap();
		assert!(out.fired_exec.contains("on_skip"));
	}
}

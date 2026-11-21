//! `flowgraph.notify.*`: desktop notification output nodes.

use crate::flowgraph::node::{
	get_optional_bool, get_optional_int, get_optional_string, get_required_string, EffectfulNode, ExecCtx, ExecFireSet, InputMap,
	NodeDescriptor, NodeExecError, NodeOutput, NodeSpec, PortSpec, RecordedEffect,
};
use crate::flowgraph::socket::{FlowResult, SocketType, SocketValue};
use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};

pub struct DesktopNotificationNode;

impl NodeDescriptor for DesktopNotificationNode {
	fn describe(&self) -> NodeSpec {
		NodeSpec {
			feature: "flowgraph.notify.desktop".into(),
			title: "Notify: Desktop".into(),
			category: "notify".into(),
			description: Some("OS の desktop notification / Toast を表示する。Runtime Mode の通知 policy と接続する出口。".into()),
			inputs: vec![
				PortSpec::exec_input("exec_in", "Exec"),
				PortSpec::input("title", "Title", SocketType::String),
				PortSpec::input("body", "Body", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("urgency", "Urgency", SocketType::String).with_default(SocketValue::String("normal".into())),
				PortSpec::input("group", "Group", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("icon", "Icon", SocketType::String).with_default(SocketValue::String(String::new())),
				PortSpec::input("timeout_ms", "Timeout ms", SocketType::Int).with_default(SocketValue::Int(5000)),
				PortSpec::input("dry_run", "Dry Run", SocketType::Bool).with_default(SocketValue::Bool(false)),
			],
			outputs: vec![
				PortSpec::exec_output("on_success", "On Success"),
				PortSpec::exec_output("on_error", "On Error"),
				PortSpec::output("shown", "Shown", SocketType::Bool),
				PortSpec::output("error", "Error", SocketType::String),
				PortSpec::output("result", "Result", SocketType::Result(Box::new(SocketType::Json))),
			],
			properties: vec![],
		}
	}
}

#[async_trait]
impl EffectfulNode for DesktopNotificationNode {
	async fn execute(
		&self,
		ctx: &mut ExecCtx,
		_props: &InputMap,
		inputs: &InputMap,
		fired: &ExecFireSet,
	) -> Result<NodeOutput, NodeExecError> {
		if !fired.contains("exec_in") {
			return Ok(NodeOutput::new());
		}
		let spec = NotificationSpec {
			title: get_required_string(inputs, "title")?,
			body: get_optional_string(inputs, "body", "")?,
			urgency: NotificationUrgency::parse(&get_optional_string(inputs, "urgency", "normal")?),
			group: get_optional_string(inputs, "group", "")?,
			icon: get_optional_string(inputs, "icon", "")?,
			timeout_ms: get_optional_int(inputs, "timeout_ms", 5000)?.clamp(0, 86_400_000),
			dry_run: get_optional_bool(inputs, "dry_run", false)?,
		};

		let payload = spec.to_payload();
		if spec.dry_run {
			ctx.record_effect(RecordedEffect::desktop_notification(ctx.node_id.clone(), payload.clone(), None));
			ctx.log(format!("notify.desktop dry-run: {}", spec.title));
			return Ok(notification_success_output(true, payload));
		}

		match show_desktop_notification(&spec).await {
			Ok(()) => {
				ctx.record_effect(RecordedEffect::desktop_notification(ctx.node_id.clone(), payload.clone(), None));
				ctx.log(format!("notify.desktop: {}", spec.title));
				Ok(notification_success_output(true, payload))
			}
			Err(error) => {
				ctx.record_effect(RecordedEffect::desktop_notification(
					ctx.node_id.clone(),
					payload,
					Some(error.clone()),
				));
				ctx.log(format!("notify.desktop error: {error}"));
				Ok(notification_error_output(error))
			}
		}
	}
}

#[derive(Debug, Clone)]
struct NotificationSpec {
	title: String,
	body: String,
	urgency: NotificationUrgency,
	group: String,
	icon: String,
	timeout_ms: i64,
	dry_run: bool,
}

impl NotificationSpec {
	fn to_payload(&self) -> JsonValue {
		json!({
			"title": self.title,
			"body": self.body,
			"urgency": self.urgency.as_str(),
			"group": self.group,
			"icon": self.icon,
			"timeout_ms": self.timeout_ms,
			"dry_run": self.dry_run,
		})
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotificationUrgency {
	Low,
	Normal,
	Critical,
}

impl NotificationUrgency {
	fn parse(value: &str) -> Self {
		match value.trim().to_ascii_lowercase().as_str() {
			"low" | "silent" | "info" => Self::Low,
			"critical" | "high" | "urgent" | "error" => Self::Critical,
			_ => Self::Normal,
		}
	}

	fn as_str(self) -> &'static str {
		match self {
			Self::Low => "low",
			Self::Normal => "normal",
			Self::Critical => "critical",
		}
	}
}

#[cfg(windows)]
async fn show_desktop_notification(spec: &NotificationSpec) -> Result<(), String> {
	let spec = spec.clone();
	tokio::task::spawn_blocking(move || show_desktop_notification_blocking(&spec))
		.await
		.map_err(|e| format!("notification task failed: {e}"))?
}

#[cfg(windows)]
fn show_desktop_notification_blocking(spec: &NotificationSpec) -> Result<(), String> {
	use std::path::Path;
	use winrt_notification::{Duration, IconCrop, Sound, Toast};

	let duration = match spec.urgency {
		NotificationUrgency::Critical => Duration::Long,
		NotificationUrgency::Low | NotificationUrgency::Normal => Duration::Short,
	};
	let mut toast = Toast::new(Toast::POWERSHELL_APP_ID)
		.title(&spec.title)
		.text1(&spec.body)
		.duration(duration);
	toast = if spec.urgency == NotificationUrgency::Low {
		toast.sound(None)
	} else {
		toast.sound(Some(Sound::Default))
	};
	if !spec.icon.trim().is_empty() {
		toast = toast.icon(Path::new(&spec.icon), IconCrop::Square, "notification icon");
	}
	toast.show().map_err(|e| e.to_string())
}

#[cfg(not(windows))]
async fn show_desktop_notification(_spec: &NotificationSpec) -> Result<(), String> {
	Err("desktop notification is not implemented on this platform".into())
}

fn notification_success_output(shown: bool, payload: JsonValue) -> NodeOutput {
	NodeOutput::new()
		.set_data("shown", SocketValue::Bool(shown))
		.set_data("error", SocketValue::String(String::new()))
		.set_data("result", SocketValue::Result(FlowResult::ok(SocketValue::Json(payload))))
		.fire_exec("on_success")
}

fn notification_error_output(error: impl Into<String>) -> NodeOutput {
	let error = error.into();
	NodeOutput::new()
		.set_data("shown", SocketValue::Bool(false))
		.set_data("error", SocketValue::String(error.clone()))
		.set_data("result", SocketValue::Result(FlowResult::err(error).with_code("notify.desktop")))
		.fire_exec("on_error")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn fired() -> ExecFireSet {
		let mut fired = ExecFireSet::new();
		fired.insert("exec_in");
		fired
	}

	#[tokio::test]
	async fn desktop_notification_dry_run_records_effect_and_succeeds() {
		let mut ctx = ExecCtx {
			node_id: "notify".into(),
			..Default::default()
		};
		let inputs: InputMap = [
			("title".into(), SocketValue::String("UNVAC".into())),
			("body".into(), SocketValue::String("Ready".into())),
			("urgency".into(), SocketValue::String("low".into())),
			("group".into(), SocketValue::String("runtime".into())),
			("dry_run".into(), SocketValue::Bool(true)),
		]
		.into_iter()
		.collect();

		let out = DesktopNotificationNode
			.execute(&mut ctx, &InputMap::new(), &inputs, &fired())
			.await
			.unwrap();
		assert!(out.fired_exec.contains("on_success"));
		assert_eq!(out.data.get("shown"), Some(&SocketValue::Bool(true)));
		assert_eq!(ctx.recorded_effects.len(), 1);
		assert_eq!(ctx.recorded_effects[0].kind, "desktop_notification");
		assert!(ctx.recorded_effects[0].contents.as_deref().unwrap().contains("\"urgency\":\"low\""));
	}

	#[test]
	fn urgency_parser_normalizes_aliases() {
		assert_eq!(NotificationUrgency::parse("silent"), NotificationUrgency::Low);
		assert_eq!(NotificationUrgency::parse("urgent"), NotificationUrgency::Critical);
		assert_eq!(NotificationUrgency::parse("other"), NotificationUrgency::Normal);
	}
}

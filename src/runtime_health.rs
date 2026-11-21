//! GUI と診断 API が参照するランタイム問題レジストリ。

use serde::Serialize;
use std::collections::BTreeMap;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeIssueSeverity {
	Warning,
	Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeIssue {
	pub key: String,
	pub severity: RuntimeIssueSeverity,
	pub component: String,
	pub code: String,
	pub message: String,
	pub impact: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub resolution: Option<String>,
	pub action: String,
	pub node: Option<String>,
	pub updated_at: String,
}

impl RuntimeIssue {
	pub fn error(
		key: impl Into<String>,
		component: impl Into<String>,
		code: impl Into<String>,
		message: impl Into<String>,
		impact: impl Into<String>,
		action: impl Into<String>,
		node: Option<String>,
	) -> Self {
		Self {
			key: key.into(),
			severity: RuntimeIssueSeverity::Error,
			component: component.into(),
			code: code.into(),
			message: message.into(),
			impact: impact.into(),
			resolution: None,
			action: action.into(),
			node,
			updated_at: jiff::Timestamp::now().to_string(),
		}
	}

	pub fn with_resolution(mut self, resolution: impl Into<String>) -> Self {
		self.resolution = Some(resolution.into());
		self
	}
}

#[derive(Debug, Default)]
pub struct RuntimeHealthRegistry {
	issues: RwLock<BTreeMap<String, RuntimeIssue>>,
}

impl RuntimeHealthRegistry {
	pub async fn report(&self, issue: RuntimeIssue) {
		self.issues.write().await.insert(issue.key.clone(), issue);
	}

	pub async fn clear(&self, key: &str) {
		self.issues.write().await.remove(key);
	}

	pub async fn replace_component(&self, component: &str, issues: Vec<RuntimeIssue>) {
		let mut current = self.issues.write().await;
		current.retain(|_, issue| issue.component != component);
		for issue in issues {
			current.insert(issue.key.clone(), issue);
		}
	}

	pub async fn snapshot(&self) -> Vec<RuntimeIssue> {
		self.issues.read().await.values().cloned().collect()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn issue(key: &str, component: &str) -> RuntimeIssue {
		RuntimeIssue::error(key, component, "failed", "失敗", "停止", "resources", None)
	}

	#[test]
	fn resolution_is_serialized_when_present() {
		let value = serde_json::to_value(issue("voice:a", "voice").with_resolution("Model Pathを設定する。")).unwrap();
		assert_eq!(value["resolution"], "Model Pathを設定する。");
	}

	#[tokio::test]
	async fn report_replaces_and_clear_removes_issue() {
		let registry = RuntimeHealthRegistry::default();
		registry.report(issue("voice:a", "voice")).await;
		registry.report(issue("voice:a", "voice")).await;
		assert_eq!(registry.snapshot().await.len(), 1);
		registry.clear("voice:a").await;
		assert!(registry.snapshot().await.is_empty());
	}

	#[tokio::test]
	async fn replace_component_preserves_other_components() {
		let registry = RuntimeHealthRegistry::default();
		registry.report(issue("eventsub:a", "twitch_eventsub")).await;
		registry.report(issue("voice:old", "voice")).await;
		registry.replace_component("voice", vec![issue("voice:new", "voice")]).await;
		let keys = registry.snapshot().await.into_iter().map(|item| item.key).collect::<Vec<_>>();
		assert_eq!(keys, vec!["eventsub:a", "voice:new"]);
	}
}

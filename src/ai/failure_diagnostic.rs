//! Persist categories only: provider messages can contain prompts, URLs or secrets.
use super::openai_responses::ResponsesClientError;

pub(super) fn classify(error: &anyhow::Error) -> (&'static str, Option<u16>) {
	for cause in error.chain() {
		if let Some(error) = cause.downcast_ref::<ResponsesClientError>() {
			return match error {
				ResponsesClientError::Api { status, body } => (api_category(*status, body), Some(*status)),
				ResponsesClientError::Transport(_) => ("transport", None),
				ResponsesClientError::Decode { .. } => ("response_decode", None),
				ResponsesClientError::Encode(_) => ("request_encode", None),
				ResponsesClientError::Build(_) => ("client_build", None),
			};
		}
	}
	("other", None)
}

// Only structured provider error codes are recognized; never copy provider text.
fn api_category(status: u16, body: &str) -> &'static str {
	if status == 429 {
		let value = serde_json::from_str::<serde_json::Value>(body).ok();
		let code = value.as_ref().and_then(|v| v.pointer("/error/code")).and_then(|v| v.as_str());
		if matches!(code, Some("credit_balance_exhausted" | "insufficient_quota")) {
			return "quota_exhausted";
		}
		return "rate_limited";
	}
	match status {
		401 => "authentication",
		403 => "permission",
		500..=599 => "provider_unavailable",
		_ => "api",
	}
}

pub(super) fn issue(persona: &str, stage: &str, error: &anyhow::Error) -> crate::runtime_health::RuntimeIssue {
	let (category, _) = classify(error);
	let (message, resolution) = match category {
		"quota_exhausted" => (
			"AIのAPI利用残高または利用枠が不足している。",
			"API提供元の残高・利用上限を確認する。解消後、改めて話しかけて応答を確認する。",
		),
		"authentication" => (
			"AIのAPI認証に失敗した。",
			"AI接続に設定したAPIキーを確認する。変更した設定を保存・適用してから応答を確認する。",
		),
		"permission" => ("AIのAPI利用権限が拒否された。", "APIキーとモデルの利用権限を確認する。"),
		"rate_limited" => (
			"AIのAPI要求が一時的に制限された。",
			"少し待ってから応答を確認する。繰り返す場合はAPIの利用制限を確認する。",
		),
		"provider_unavailable" => ("AI提供元で要求を処理できなかった。", "少し待ってから応答を確認する。"),
		"transport" => ("AI提供元との通信に失敗した。", "ネットワークとAIの接続先を確認する。"),
		_ if stage == "turn_output_parse" => (
			"AIの応答を会話制御として読み取れなかった。",
			"再度の応答でも続く場合は、使用モデルと診断記録を添えて報告する。",
		),
		_ => (
			"AIの応答処理に失敗した。",
			"診断記録の失敗分類とHTTP状態を確認する。繰り返す場合は使用モデルとともに報告する。",
		),
	};
	crate::runtime_health::RuntimeIssue::error(
		format!("ai_reaction:{persona}:{stage}"),
		"ai_reaction",
		category,
		format!("{persona}: {message}"),
		"このAI共演者の直近の応答処理が失敗した。",
		"observability",
		None,
	)
	.with_resolution(resolution)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn failure_diagnostic_preserves_status_through_context_without_body() {
		let error = anyhow::Error::new(ResponsesClientError::Api {
			status: 401,
			body: "secret token and private prompt".into(),
		})
		.context("request failed with private URL");
		assert_eq!(classify(&error), ("authentication", Some(401)));
	}

	#[test]
	fn failure_diagnostic_does_not_guess_from_untrusted_text() {
		assert_eq!(classify(&anyhow::anyhow!("401 model_not_found secret")), ("other", None));
		assert_eq!(
			classify(&anyhow::Error::new(ResponsesClientError::Decode {
				error: "private".into(),
				body: "private".into(),
			})),
			("response_decode", None)
		);
	}

	#[test]
	fn failure_diagnostic_distinguishes_quota_from_transient_limits() {
		for code in ["credit_balance_exhausted", "insufficient_quota"] {
			let body = serde_json::json!({"error":{"code":code,"message":"private secret"}}).to_string();
			assert_eq!(api_category(429, &body), "quota_exhausted");
			assert_eq!(api_category(403, &body), "permission");
		}
		for body in [
			"credit_balance_exhausted",
			r#"{"error":{"message":"insufficient_quota"}}"#,
			r#"{"code":"insufficient_quota"}"#,
			"{}",
		] {
			assert_eq!(api_category(429, body), "rate_limited");
		}
	}

	#[test]
	fn failure_diagnostic_user_guidance_never_contains_provider_body() {
		let error = anyhow::Error::new(ResponsesClientError::Api {
			status: 429,
			body: r#"{"error":{"code":"credit_balance_exhausted","message":"PRIVATE_TOKEN"}}"#.into(),
		});
		let issue = issue("partner-a", "request_or_tool_loop", &error);
		assert_eq!(issue.code, "quota_exhausted");
		assert!(issue.message.contains("残高"));
		assert!(issue.resolution.is_some());
		assert!(!serde_json::to_string(&issue).unwrap().contains("PRIVATE_TOKEN"));
	}
}

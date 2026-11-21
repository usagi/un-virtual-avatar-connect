//! Responses API リクエストで **モデル系列ごとに異なる既定** を一箇所にまとめる。
//! UI やプロンプト組み立て（`context`）とは分離し、将来モデルが増えてもここを拡張する。

use anyhow::{Context, Result};
use regex::Regex;

use crate::ai::config::{AiPersonaConf, OpenAiModelPolicyConf, OpenAiReasoningEffortConf};
use crate::ai::openai_responses::types::request::{CreateResponseRequest, Reasoning, ReasoningEffort, TextConfig, TextFormat};

#[derive(Debug)]
pub(crate) struct ModelSelection<'a> {
	pub model: &'a str,
	pub max_output_tokens: Option<u32>,
	pub reasoning_effort: Option<ReasoningEffort>,
	pub escalated: bool,
}

fn reasoning_effort_from_conf(value: OpenAiReasoningEffortConf) -> ReasoningEffort {
	match value {
		OpenAiReasoningEffortConf::None => ReasoningEffort::None,
		OpenAiReasoningEffortConf::Minimal => ReasoningEffort::Minimal,
		OpenAiReasoningEffortConf::Low => ReasoningEffort::Low,
		OpenAiReasoningEffortConf::Medium => ReasoningEffort::Medium,
		OpenAiReasoningEffortConf::High => ReasoningEffort::High,
		OpenAiReasoningEffortConf::Xhigh => ReasoningEffort::Xhigh,
		OpenAiReasoningEffortConf::Max => ReasoningEffort::Max,
	}
}

fn escalation_matches(policy: &OpenAiModelPolicyConf, channel: &str, content: &str, request_class: Option<&str>) -> Result<bool> {
	if request_class.is_some_and(|value| policy.trigger_request_classes.iter().any(|candidate| candidate == value)) {
		return Ok(true);
	}
	if policy.trigger_channels.iter().any(|candidate| candidate == channel) {
		return Ok(true);
	}
	match policy.trigger_regex_pattern.as_deref() {
		Some(pattern) => Ok(Regex::new(pattern)
			.with_context(|| format!("openai_model_policy.trigger_regex_pattern が不正です: {pattern:?}"))?
			.is_match(content)),
		None => Ok(false),
	}
}

pub(crate) fn validate_model_policy(persona: &AiPersonaConf) -> Result<()> {
	if let Some(policy) = persona.openai_model_policy.as_ref() {
		if policy.model.trim().is_empty() {
			anyhow::bail!("openai_model_policy.model は空にできません");
		}
		if policy.trigger_channels.is_empty() && policy.trigger_request_classes.is_empty() && policy.trigger_regex_pattern.is_none() {
			anyhow::bail!("openai_model_policy には trigger_channels、trigger_request_classes、trigger_regex_pattern のいずれかが必要です");
		}
		if let Some(pattern) = policy.trigger_regex_pattern.as_deref() {
			Regex::new(pattern).with_context(|| format!("openai_model_policy.trigger_regex_pattern が不正です: {pattern:?}"))?;
		}
	}
	Ok(())
}

pub(crate) fn select_model<'a>(
	persona: &'a AiPersonaConf,
	channel: &str,
	content: &str,
	request_class: Option<&str>,
	routine_max_output_tokens: Option<u32>,
) -> Result<ModelSelection<'a>> {
	let routine_model = persona.model.as_deref().unwrap_or("gpt-5.6-luna");
	if let Some(policy) = persona.openai_model_policy.as_ref() {
		if escalation_matches(policy, channel, content, request_class)? {
			return Ok(ModelSelection {
				model: policy.model.trim(),
				max_output_tokens: policy.max_output_tokens.or(routine_max_output_tokens),
				reasoning_effort: policy.reasoning_effort.map(reasoning_effort_from_conf),
				escalated: true,
			});
		}
	}
	Ok(ModelSelection {
		model: routine_model,
		max_output_tokens: routine_max_output_tokens,
		reasoning_effort: persona.openai_reasoning_effort.map(reasoning_effort_from_conf),
		escalated: false,
	})
}

#[inline]
pub(crate) fn is_gpt5_family(model_id: Option<&str>) -> bool {
	model_id.is_some_and(|m| m.starts_with("gpt-5"))
}

#[inline]
pub(crate) fn supports_explicit_prompt_cache(model_id: Option<&str>) -> bool {
	model_id.is_some_and(|m| m.starts_with("gpt-5.6"))
}

fn is_sol_astra_62(model: &str) -> bool {
	is_model_or_snapshot(model, "gpt-6.2-sol") || is_model_or_snapshot(model, "gpt-6.2-astra")
}

pub(super) fn reasoning_effort_supported(model: &str, effort: ReasoningEffort) -> bool {
	// OpenAI model docs checked 2026-10-21: Luna permits none; Astra/Sol start at low.
	// Keep request compatibility separate from GPT-5-specific retry/cache behavior.
	if is_model_or_snapshot(model, "gpt-6-luna") || is_model_or_snapshot(model, "gpt-6.1-terra") {
		return !matches!(effort, ReasoningEffort::Minimal);
	}
	if is_sol_astra_62(model) || is_model_or_snapshot(model, "gpt-6-astra") || is_model_or_snapshot(model, "gpt-6.1-sol") {
		return !matches!(effort, ReasoningEffort::None | ReasoningEffort::Minimal);
	}
	if model.starts_with("gpt-5.6") {
		return !matches!(effort, ReasoningEffort::Minimal);
	}
	if model.starts_with("gpt-5.5") || model.starts_with("gpt-5.4") {
		return matches!(
			effort,
			ReasoningEffort::None | ReasoningEffort::Low | ReasoningEffort::Medium | ReasoningEffort::High | ReasoningEffort::Xhigh
		);
	}
	if model == "gpt-5" || model.starts_with("gpt-5-") {
		return matches!(
			effort,
			ReasoningEffort::Minimal | ReasoningEffort::Low | ReasoningEffort::Medium | ReasoningEffort::High
		);
	}
	// 未知の将来gpt-5系列はAPI側の検証を正とする。
	true
}

fn is_model_or_snapshot(model: &str, name: &str) -> bool {
	model == name || model.strip_prefix(name).is_some_and(|suffix| suffix.starts_with('-'))
}

fn supports_reasoning_options(model: &str) -> bool {
	is_gpt5_family(Some(model))
		|| is_sol_astra_62(model)
		|| ["gpt-6-luna", "gpt-6-astra", "gpt-6.1-sol", "gpt-6.1-terra"].iter().any(|name| is_model_or_snapshot(model, name))
}

/// Terra supports reasoning history even though it is not in the GPT-5 family.
pub(crate) fn supports_reasoning_history(model: Option<&str>) -> bool {
	is_gpt5_family(model) || model.is_some_and(|m| is_model_or_snapshot(m, "gpt-6.1-terra"))
}

#[inline]
pub(crate) fn needs_default_system_when_missing(model_id: Option<&str>) -> bool {
	is_gpt5_family(model_id)
}

#[inline]
pub(crate) fn should_retry_on_empty_assistant(model_id: Option<&str>) -> bool {
	is_gpt5_family(model_id)
}

/// Responses API へのリクエストに、モデル族に応じた既定値を適用する。
///
/// - `max_output_tokens`: モデル族を問わず `request.max_output_tokens` に代入する
///   （Chat Completions の `max_tokens` / `max_completion_tokens` の分岐は Responses では不要）。
/// - `reasoning.effort`: gpt-5 系と対応を確認した gpt-6 モデルに設定する。その他モデルでは API 側が
///   無視するか `invalid_request_error` を返す可能性があるので、ここでは設定しない。
/// - `text.format`: 旧 Chat Completions `ResponseFormat::Text` の Responses 等価。Responses の既定は
///   そもそも自由文なので明示は不要だが、gpt-5 系で structured outputs の誤起動を防ぐため
///   `TextFormat::Text` を明示する（Chat Completions 時代の挙動を維持）。
///
/// いずれも persona 設定の値で **常に** 上書きする（既存値は保持しない）。
pub(crate) fn apply_model_responses_options(
	request: &mut CreateResponseRequest,
	model: &str,
	max_output_tokens: Option<u32>,
	reasoning_effort: Option<ReasoningEffort>,
) {
	if is_gpt5_family(Some(model)) {
		let text_cfg = request.text.get_or_insert_with(TextConfig::default);
		if text_cfg.format.is_none() {
			text_cfg.format = Some(TextFormat::Text);
		}
	}

	if let Some(mt) = max_output_tokens {
		request.max_output_tokens = Some(mt);
	}

	if let Some(effort) = reasoning_effort {
		if supports_reasoning_options(model) {
			if reasoning_effort_supported(model, effort) {
				let r = request.reasoning.get_or_insert_with(Reasoning::default);
				r.effort = Some(effort);
			} else {
				log::warn!("reasoning.effort={effort:?} はモデル {model} で未対応のため無視します。");
			}
		} else {
			log::warn!("openai_reasoning_effort の対応を確認していないモデルです（指定モデル: {model}）。無視します。");
		}
	}
	if is_sol_astra_62(model) {
		request.temperature = None;
		request.top_p = None;
	}
	// Terra defaults to medium when omitted; sampling controls require explicit none.
	// Clear stale effort when the caller switches from an unsupported model setting.
	if is_model_or_snapshot(model, "gpt-6.1-terra") {
		let valid_effort = reasoning_effort.filter(|e| reasoning_effort_supported(model, *e));
		if let Some(r) = request.reasoning.as_mut() { r.effort = valid_effort; }
		if valid_effort != Some(ReasoningEffort::None) {
			request.temperature = None;
			request.top_p = None;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn gpt6_explicit_reasoning_survives_request_serialization() {
		for model in ["gpt-6-luna", "gpt-6-luna-2026-10-01"] {
			let mut req = CreateResponseRequest { model: model.into(), ..Default::default() };
			apply_model_responses_options(&mut req, model, Some(512), Some(ReasoningEffort::None));
			let wire = serde_json::to_value(&req).unwrap();
			assert_eq!(wire["reasoning"]["effort"], "none");
			assert_eq!(wire["max_output_tokens"], 512);
		}
		for model in ["gpt-6-astra", "gpt-6.1-sol", "gpt-6.2-sol", "gpt-6.2-astra"] {
			let mut req = CreateResponseRequest { model: model.into(), ..Default::default() };
			apply_model_responses_options(&mut req, model, None, Some(ReasoningEffort::Low));
			assert_eq!(serde_json::to_value(&req).unwrap()["reasoning"]["effort"], "low");
			assert!(!reasoning_effort_supported(model, ReasoningEffort::None));
		}
		assert!(!reasoning_effort_supported("gpt-6-luna", ReasoningEffort::Minimal));
		assert!(!supports_reasoning_options("gpt-6-lunar-unrelated"));
	}

	#[test]
	fn sol_astra_62_omit_unsupported_sampling() {
		for model in ["gpt-6.2-sol", "gpt-6.2-astra", "gpt-6.2-sol-2026-11-19"] {
			for effort in [None, Some(ReasoningEffort::Low), Some(ReasoningEffort::Max)] {
				let mut req = CreateResponseRequest { model: model.into(), temperature: Some(0.7), top_p: Some(0.9), ..Default::default() };
				apply_model_responses_options(&mut req, model, Some(256), effort);
				let wire = serde_json::to_value(&req).unwrap();
				assert!(wire.get("temperature").is_none() && wire.get("top_p").is_none());
				assert_eq!(wire["reasoning"]["effort"], serde_json::to_value(effort).unwrap());
			}
		}
	}

	#[test]
	fn terra_wire_parameters_cover_none_low_default_and_invalid() {
		for model in ["gpt-6.1-terra", "gpt-6.1-terra-2026-10-27"] {
			for effort in [Some(ReasoningEffort::None), Some(ReasoningEffort::Low), None, Some(ReasoningEffort::Minimal)] {
				let mut req = CreateResponseRequest { model: model.into(), temperature: Some(0.7), top_p: Some(0.9), ..Default::default() };
				apply_model_responses_options(&mut req, model, Some(1600), effort);
				let wire=serde_json::to_value(&req).unwrap();
				assert_eq!(wire["max_output_tokens"], 1600);
				if effort == Some(ReasoningEffort::None) {
					assert_eq!(wire["reasoning"]["effort"], "none");
					assert!(wire.get("temperature").is_some());
				} else {
					assert!(wire.get("temperature").is_none() && wire.get("top_p").is_none());
					if effort == Some(ReasoningEffort::Low) { assert_eq!(wire["reasoning"]["effort"], "low"); }
				}
			}
		}
		assert!(supports_reasoning_history(Some("gpt-6.1-terra")));
		assert!(!supports_reasoning_history(Some("gpt-6.1-terracotta")));
	}

	#[test]
	fn missing_persona_model_defaults_to_gpt56_luna() {
		let persona: AiPersonaConf = toml::from_str("id = \"default-model\"").unwrap();
		let selected = select_model(&persona, "user", "hello", None, Some(512)).unwrap();
		assert_eq!(selected.model, "gpt-5.6-luna");
		assert_eq!(selected.max_output_tokens, Some(512));
		assert!(!selected.escalated);
	}

	#[test]
	fn gpt5_family_detection() {
		assert!(is_gpt5_family(Some("gpt-5")));
		assert!(is_gpt5_family(Some("gpt-5-mini")));
		assert!(is_gpt5_family(Some("gpt-5.4-nano")));
		assert!(is_gpt5_family(Some("gpt-5.4")));
		assert!(!is_gpt5_family(Some("gpt-4o")));
		assert!(!is_gpt5_family(Some("gpt-4o-mini")));
		assert!(!is_gpt5_family(None));
	}

	#[test]
	fn policy_aliases_track_gpt5_family() {
		for id in ["gpt-5", "gpt-5.4-nano", "gpt-5.4"] {
			let m = Some(id);
			assert_eq!(needs_default_system_when_missing(m), is_gpt5_family(m));
			assert_eq!(should_retry_on_empty_assistant(m), is_gpt5_family(m));
		}
		assert!(!should_retry_on_empty_assistant(Some("gpt-4o-mini")));
	}

	#[test]
	fn explicit_prompt_cache_is_limited_to_gpt56_family() {
		assert!(supports_explicit_prompt_cache(Some("gpt-5.6-luna")));
		assert!(supports_explicit_prompt_cache(Some("gpt-5.6")));
		assert!(!supports_explicit_prompt_cache(Some("gpt-5.4-mini")));
		assert!(!supports_explicit_prompt_cache(Some("gpt-4o")));
		assert!(!supports_explicit_prompt_cache(None));
	}

	#[test]
	fn responses_options_apply_max_output_tokens_for_any_model() {
		let mut req = CreateResponseRequest {
			model: "gpt-4o-mini".to_string(),
			..Default::default()
		};
		apply_model_responses_options(&mut req, "gpt-4o-mini", Some(512), None);
		assert_eq!(req.max_output_tokens, Some(512));
		assert!(req.reasoning.is_none());
		assert!(req.text.is_none(), "non-gpt5 は text.format を埋めない");
	}

	#[test]
	fn responses_options_apply_reasoning_effort_for_gpt5() {
		let mut req = CreateResponseRequest {
			model: "gpt-5-mini".to_string(),
			..Default::default()
		};
		apply_model_responses_options(&mut req, "gpt-5-mini", None, Some(ReasoningEffort::High));
		assert_eq!(req.reasoning.as_ref().and_then(|r| r.effort), Some(ReasoningEffort::High));
		assert!(matches!(req.text.as_ref().and_then(|t| t.format.clone()), Some(TextFormat::Text)));
	}

	#[test]
	fn responses_options_ignore_reasoning_effort_for_non_gpt5() {
		let mut req = CreateResponseRequest {
			model: "gpt-4o-mini".to_string(),
			..Default::default()
		};
		apply_model_responses_options(&mut req, "gpt-4o-mini", None, Some(ReasoningEffort::Low));
		assert!(req.reasoning.is_none());
	}

	#[test]
	fn responses_options_preserve_existing_text_format() {
		let mut req = CreateResponseRequest {
			model: "gpt-5-mini".to_string(),
			text: Some(TextConfig {
				format: Some(TextFormat::JsonObject),
			}),
			..Default::default()
		};
		apply_model_responses_options(&mut req, "gpt-5-mini", None, None);
		assert!(matches!(
			req.text.as_ref().and_then(|t| t.format.clone()),
			Some(TextFormat::JsonObject)
		));
	}

	#[test]
	fn flowgraph_model_routing_preserves_routine_and_explicit_effort() {
		use crate::flowgraph::{node::InputMap, socket::SocketValue};
		use crate::flowgraph::nodes::ai::build_observation_datum;
		// Each configuration has one alternate route; this does not imply multi-route support.
		for (model, effort) in [
			("gpt-6-luna", "none"),
			("gpt-6.1-terra", "none"),
			("gpt-6.1-terra", "low"),
			("gpt-6.1-sol", "low"),
			("gpt-6.2-sol", "low"),
			("gpt-6.2-astra", "low"),
			("gpt-6-astra", "low"),
		] {
			let conf = format!(r#"
id = "review"
model = "gpt-5.6-luna"
openai_reasoning_effort = "none"
[openai_model_policy]
model = "{model}"
reasoning_effort = "{effort}"
max_output_tokens = 1600
trigger_request_classes = ["screen_detail"]
"#);
			let persona: AiPersonaConf = toml::from_str(&conf).unwrap();
			validate_model_policy(&persona).unwrap();
			let mut inputs = InputMap::new();
			inputs.insert("meta".into(), SocketValue::Json(serde_json::json!({"ai_request_class":"screen_detail"})));
			for trusted in [false, true] {
				let mut properties = InputMap::new();
				if trusted { properties.insert("request_class".into(), SocketValue::String("screen_detail".into())); }
				let datum = build_observation_datum("review", "screen_detail gpt-6-astra".into(), "review-node", &properties, &inputs).unwrap();
				let selected = select_model(&persona, &datum.channel, &datum.content, crate::ai::context::request_class(&datum), Some(500)).unwrap();
				assert_eq!(selected.model, if trusted { model } else { "gpt-5.6-luna" });
				assert_eq!(selected.escalated, trusted);
				assert_eq!(selected.max_output_tokens, Some(if trusted { 1600 } else { 500 }));
				let mut request = CreateResponseRequest { model: selected.model.into(), ..Default::default() };
				apply_model_responses_options(&mut request, selected.model, selected.max_output_tokens, selected.reasoning_effort);
				let wire = serde_json::to_value(request).unwrap();
				assert_eq!(wire["reasoning"]["effort"], if trusted { effort } else { "none" });
			}
			// A purpose-specific call must not change the persona's next routine call.
			let next = select_model(&persona, "user", "hello", None, Some(500)).unwrap();
			assert_eq!(next.model, "gpt-5.6-luna");
			assert!(!next.escalated);
		}
	}

	#[test]
	fn escalation_policy_matches_channel_or_regex() {
		let policy = OpenAiModelPolicyConf {
			model: "gpt-5.6-terra".into(),
			reasoning_effort: Some(OpenAiReasoningEffortConf::Medium),
			max_output_tokens: Some(2048),
			trigger_channels: vec!["twitch.moderation".into()],
			trigger_request_classes: vec!["twitch_channel_points_reward".into()],
			trigger_regex_pattern: Some("(?i)(調査|verify)".into()),
		};
		assert!(escalation_matches(&policy, "twitch.moderation", "通常", None).unwrap());
		assert!(escalation_matches(&policy, "user", "verify this", None).unwrap());
		assert!(escalation_matches(&policy, "user", "hello", Some("twitch_channel_points_reward")).unwrap());
		assert!(!escalation_matches(&policy, "user", "hello", None).unwrap());
	}

	#[test]
	fn reasoning_capability_tracks_model_generations() {
		assert!(reasoning_effort_supported("gpt-5.6-luna", ReasoningEffort::Max));
		assert!(!reasoning_effort_supported("gpt-5.6-luna", ReasoningEffort::Minimal));
		assert!(reasoning_effort_supported("gpt-5.4-mini", ReasoningEffort::Xhigh));
		assert!(!reasoning_effort_supported("gpt-5.4-mini", ReasoningEffort::Max));
		assert!(reasoning_effort_supported("gpt-5-mini", ReasoningEffort::Minimal));
		assert!(!reasoning_effort_supported("gpt-5-mini", ReasoningEffort::None));
	}
}

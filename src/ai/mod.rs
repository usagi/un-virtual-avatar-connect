//! 常駐 AI サービス（旧 `feature = "openai-chat"` プロセッサー）。
//!
//! Phase I (2026-04-17): プロセッサー列から切り離し、`State` の `broadcast` から流れる [`Observation`] を
//! 聞き続ける **常駐タスク**（[`service::AiService`]）へ昇格させた。
//!
//! # 構造
//!
//! - [`config`] — `[[ai.personas]]` の設定型
//! - [`observe`] — [`Observation`] と [`ObserveSet`]（triggers / include_all / include_additional / exclude）
//! - [`service`] — [`AiService`] 本体と `spawn_all` エントリ
//! - `context` / `completion` / `model_policy` / `tools` — OpenAI Chat API 周辺（テスト対象含む）
//! - [`fine_tuning`] — CLI の fine-tune / OpenAI Files 一括削除（persona_id 指定）

pub(crate) mod config;
pub(crate) mod observe;

mod completion;
mod failure_diagnostic;
mod context;
#[cfg(test)]
pub(crate) use context::{assemble_openai_responses_dynamic_input, assemble_openai_responses_static_input};
#[cfg(test)]
pub(crate) use model_policy::apply_model_responses_options;
mod decision;
mod episode;
mod initiative;
mod model_policy;
pub(crate) mod task;
pub(crate) mod reload;
mod service;
mod streaming_turn;
mod visual_history;
mod past_screen;
mod tools;

// Phase χ: OpenAI Responses API（`/v1/responses`）の自前 `reqwest + 自前 DTO` 実装。
// `crate::*` には依存せず（`SharedState` / `ChannelDatum` 非参照）、
// 将来 `vac-openai-responses` crate 等に切り出せる境界を保つ。
// 詳細: docs/roadmap/phase-chi-openai-responses.md §3。
pub(crate) mod openai_responses;

pub mod fine_tuning;

pub use config::AiConf;
pub use observe::Observation;
pub use reload::{AiReloadHandle, AiReloadReport, AiReloadRequest};
pub use service::spawn_all;

pub(crate) const ENV_OPENAI_API_KEY: &str = "VAC_OPENAI_API_KEY";
/// Flowgraph が信頼済みの経路設定として付与する AI 依頼種別。
/// 外部入力本文から推測せず、`flowgraph.ai.observe` の property だけが設定する。
pub(crate) const META_REQUEST_CLASS: &str = "ai_request_class";
pub(crate) const REQUEST_CLASS_TWITCH_CHANNEL_POINTS_REWARD: &str = "twitch_channel_points_reward";

pub(crate) fn validate_profile_references(conf: &crate::Conf, profile_dir: &std::path::Path) -> anyhow::Result<()> {
	for persona in &conf.ai.personas {
		if let Some(path) = persona.api_key_file.as_deref() {
			let path = resolve_profile_path(profile_dir, path);
			let value = std::fs::read_to_string(&path)
				.map_err(|error| anyhow::anyhow!("AI credential fileを読めません: {}: {error}", path.display()))?;
			if value.trim().is_empty() {
				return Err(anyhow::anyhow!("AI credential fileが空です: {}", path.display()));
			}
		}
		let Some(configured_path) = persona.openai_tools_json_path.as_deref() else {
			continue;
		};
		let path = resolve_profile_path(profile_dir, configured_path);
		let source = std::fs::read_to_string(&path)
			.map_err(|error| anyhow::anyhow!("OpenAI tools fileを読めません: {}: {error}", path.display()))?;
		let mut declared = tools::parse_tools_json(&source)
			.map_err(|error| anyhow::anyhow!("OpenAI tools fileが不正です: {}: {error}", path.display()))?;
		tools::constrain_effect_channels(&mut declared, persona.openai_tool_policy.as_ref().and_then(|policy| policy.effect_allowed_channels.as_deref()), persona.channel_effect.as_deref().unwrap_or("effect"))?;
		let require_strict = persona.openai_tool_policy.as_ref().is_some_and(|policy| policy.require_strict);
		tools::validate_function_tool_schemas(&declared, require_strict)?;
		if let Some(policy) = persona.openai_tool_policy.as_ref() {
			if let Some(allowed) = policy.default_allowed.as_ref() {
				tools::apply_allowed_tools(&declared, allowed)?;
			}
			for (channel, allowed) in &policy.by_channel {
				tools::apply_allowed_tools(&declared, allowed)
					.map_err(|error| anyhow::anyhow!("openai_tool_policy.by_channel[{channel:?}]が不正です: {error}"))?;
			}
			for (request_class, allowed) in &policy.by_request_class {
				tools::apply_allowed_tools(&declared, allowed)
					.map_err(|error| anyhow::anyhow!("openai_tool_policy.by_request_class[{request_class:?}]が不正です: {error}"))?;
			}
		}
	}
	Ok(())
}

fn resolve_profile_path(profile_dir: &std::path::Path, path: &std::path::Path) -> std::path::PathBuf {
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		profile_dir.join(path)
	}
}

pub(crate) fn resolve_openai_api_key(persona: &config::AiPersonaConf) -> anyhow::Result<Option<String>> {
	resolve_openai_api_key_with_env(persona, std::env::var(ENV_OPENAI_API_KEY).ok())
}

fn resolve_openai_api_key_with_env(persona: &config::AiPersonaConf, environment: Option<String>) -> anyhow::Result<Option<String>> {
	if let Some(value) = environment.filter(|value| !value.trim().is_empty()) {
		return Ok(Some(value));
	}
	if let Some(path) = persona.api_key_file.as_deref() {
		let value = std::fs::read_to_string(path)
			.map_err(|error| anyhow::anyhow!("OpenAI API key fileを読めません: {}: {error}", path.display()))?;
		let value = value.trim();
		if value.is_empty() {
			return Err(anyhow::anyhow!("OpenAI API key fileが空です: {}", path.display()));
		}
		return Ok(Some(value.to_string()));
	}
	Ok(persona.api_key.clone().filter(|value| !value.trim().is_empty()))
}

#[cfg(test)]
mod api_key_tests {
	use super::*;

	fn persona(source: &str) -> config::AiPersonaConf {
		toml::from_str::<config::AiConf>(source).unwrap().personas.remove(0)
	}

	#[test]
	fn api_key_resolution_prefers_environment_then_file_then_inline() {
		let root = std::env::temp_dir().join(format!("unvac-api-key-resolver-{}", std::process::id()));
		std::fs::write(&root, "file-secret\n").unwrap();
		let escaped = root.to_string_lossy().replace('\\', "\\\\");
		let configured = persona(&format!(
			"[[personas]]\nid = \"test\"\napi_key = \"inline-secret\"\napi_key_file = \"{escaped}\"\n"
		));
		assert_eq!(
			resolve_openai_api_key_with_env(&configured, Some("env-secret".into()))
				.unwrap()
				.as_deref(),
			Some("env-secret")
		);
		assert_eq!(
			resolve_openai_api_key_with_env(&configured, None).unwrap().as_deref(),
			Some("file-secret")
		);
		let inline = persona("[[personas]]\nid = \"test\"\napi_key = \"inline-secret\"\n");
		assert_eq!(
			resolve_openai_api_key_with_env(&inline, None).unwrap().as_deref(),
			Some("inline-secret")
		);
		std::fs::remove_file(root).unwrap();
	}
}

/// χ-6: Responses API の `max_output_tokens` を env で上書きするためのキー。
/// conf `[[ai.personas]].openai_max_output_tokens` / legacy `max_tokens` より優先される。
pub(crate) const ENV_OPENAI_MAX_OUTPUT_TOKENS: &str = "VAC_OPENAI_MAX_OUTPUT_TOKENS";

#[cfg(any(test, feature="desktop-native-review"))]
mod review_provider;

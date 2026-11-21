//! Chat Completions 縺ｮ `tools` JSON 隱ｭ縺ｿ霎ｼ縺ｿ縺ｨ縲∫ｵ・∩霎ｼ縺ｿ繝・・繝ｫ縺ｮ螳溯｡後・
//!
//! Phase IV 縺ｧ **Actions**・亥憶菴懃畑縺ｮ縺ゅｋ function-call・峨ｒ蟆主・縺励◆縲ゅヤ繝ｼ繝ｫ譛ｬ菴薙・ `run_function_tool` 縺ｫ繝・ぅ繧ｹ繝代ャ繝√＆繧後・
//! [`ToolContext`] 邨檎罰縺ｧ繧｢繝励Μ迥ｶ諷具ｼ・SharedState`・峨ｄ Twitch 隱崎ｨｼ諠・ｱ縺ｫ繧｢繧ｯ繧ｻ繧ｹ縺吶ｋ縲・
//!
//! - `vac_ping`: 逍朱夂｢ｺ隱搾ｼ亥憶菴懃畑縺ｪ縺暦ｼ・
//! - `vac_emit_effect`: 莉ｻ諢上・ VAC 繝√Ε繝ｳ繝阪Ν縺ｸ `ChannelDatum` 繧・push 縺吶ｋ縲・rowser Source 蛛ｴ縺ｧ諡ｾ縺｣縺ｦ貍泌・縺ｫ菴ｿ縺・Φ螳壹・
//! - `vac_twitch_set_category`: Helix `PATCH /channels` 縺ｧ驟堺ｿ｡繧ｫ繝・ざ繝ｪ繝ｼ繧貞､画峩縺吶ｋ縲りｦ・`channel:manage:broadcast` 繧ｹ繧ｳ繝ｼ繝励・
//! - `vac_twitch_chat_say`: Helix `POST /chat/messages` 縺ｧ繝√Ε繝・ヨ逋ｺ隧ｱ・医Δ繝・Ξ繝ｼ繧ｿ繝ｼ/繝懊ャ繝亥哨・峨・
//! - `vac_twitch_ban_user`: Helix `POST /moderation/bans` 縺ｧ BAN / timeout・・duration_secs` 謖・ｮ壹〒 timeout・峨・
//! - `vac_twitch_unban_user`: Helix `DELETE /moderation/bans`縲・
//! - `vac_twitch_delete_message`: Helix `DELETE /moderation/chat` 縺ｧ迚ｹ螳壹Γ繝・そ繝ｼ繧ｸ蜑企勁縲・

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use crate::ai::openai_responses::types::{
	input::InputItem,
	request::{AllowedToolRef, AllowedToolsChoice, NamedToolChoice, Tool, ToolChoice, ToolChoiceMode},
	response::FunctionCallView,
};
use crate::conf::{TwitchEventSubConfig, TwitchModeratorConfig};
use crate::control_events::{AiToolCallPhase, ControlEvent};
use crate::state::{ChannelDatum, DataSource};
use crate::SharedState;
use vac_motion::avatar_intent::{avatar_intent_to_unmf, AvatarIntentMapperOptions, AvatarIntentRequest};

const DEFAULT_AVATAR_INTENT_CHANNEL: &str = "avatar_intent";
pub(crate) const AVATAR_INTENT_TOOL_NAME: &str = "vac_emit_avatar_intent";

/// Phase IV: 蜑ｯ菴懃畑繧剃ｼｴ縺・ヤ繝ｼ繝ｫ縺ｫ貂｡縺吝ｮ溯｡後さ繝ｳ繝・く繧ｹ繝医・
#[derive(Clone)]
pub(crate) struct ToolContext {
	pub(crate) state: SharedState,
	pub(crate) persona_label: String,
	/// `vac_emit_effect` 縺ｧ `channel` 蠑墓焚縺檎┌縺・→縺阪↓菴ｿ縺・里螳壹・ VAC 繝√Ε繝ｳ繝阪Ν蜷阪・
	pub(crate) effect_channel: String,
	pub(crate) effect_allowed_channels: Option<Vec<String>>,
	/// Helix 蜻ｼ縺ｳ蜃ｺ縺礼ｳｻ繝・・繝ｫ逕ｨ縲よ悴險ｭ螳壹・繝壹Ν繧ｽ繝翫〒縺ｯ Twitch 邉ｻ繝・・繝ｫ繧堤┌蜉ｹ蛹悶☆繧九・
	pub(crate) twitch_eventsub: Option<Arc<TwitchEventSubConfig>>,
	/// Phase V-c: 繝｢繝・Ξ繝ｼ繧ｷ繝ｧ繝ｳ邉ｻ・・AN / timeout / delete / chat・峨ヤ繝ｼ繝ｫ縺悟ｿ・ｦ√→縺吶ｋ繝懊ャ繝医い繧ｫ繧ｦ繝ｳ繝郁ｨｭ螳壹・
	pub(crate) twitch_moderator: Option<Arc<TwitchModeratorConfig>>,
	/// 繝・ヵ繧ｩ繝ｫ繝医・騾∽ｿ｡蜈磯・菫｡閠・login縲Ｄhat_say / ban 邉ｻ縺ｧ `broadcaster_login` 蠑墓焚縺檎怐逡･縺輔ｌ縺溘→縺阪・繝輔か繝ｼ繝ｫ繝舌ャ繧ｯ縲・
	pub(crate) twitch_default_broadcaster_login: Option<String>,
}

/// `openai_tools_json` の JSON をパースして Responses API の [`Tool`] 配列に変換する。
///
/// # 受け付ける書式（両対応）
///
/// 1. **Chat Completions 形式**（旧来、後方互換）
///    ```json
///    [{"type":"function","function":{"name":"vac_ping","description":"...","parameters":{...},"strict":true}}]
///    ```
/// 2. **Responses 形式**（推奨、χ 以降）
///    ```json
///    [{"type":"function","name":"vac_ping","description":"...","parameters":{...},"strict":true}]
///    ```
/// 3. **Hosted tools**（Responses 新機能）
///    ```json
///    [{"type":"web_search"}, {"type":"file_search","vector_store_ids":["vs_..."]}]
///    ```
///
/// 1 件ごとに自動判定（`type:"function"` で `function` sub-object を持つなら 1 番と見做す）
/// するため、ユーザが既存 conf を書き換えずに χ 移行できる。
pub(crate) fn parse_tools_json(text: &str) -> Result<Vec<Tool>> {
	let t = text.trim();
	if t.is_empty() {
		return Ok(Vec::new());
	}
	let raw: Vec<Value> = serde_json::from_str(t).context("openai_tools_json の JSON を配列としてパースできませんでした")?;
	let mut out = Vec::with_capacity(raw.len());
	for (i, v) in raw.into_iter().enumerate() {
		out.push(tool_from_json_any_format(v).with_context(|| format!("openai_tools_json[{i}] の解釈に失敗"))?);
	}
	Ok(out)
}

/// 1 件分の JSON を受けて、Chat Completions / Responses どちらの形式でも Responses `Tool` に正規化する。
fn tool_from_json_any_format(v: Value) -> Result<Tool> {
	// Chat Completions 形式判定: `type:"function"` かつ `function` sub-object 持ち
	if let Value::Object(map) = &v {
		if map.get("type").and_then(|t| t.as_str()) == Some("function") && map.get("function").map(|f| f.is_object()).unwrap_or(false) {
			let function = map.get("function").and_then(|f| f.as_object()).unwrap();
			let name = function
				.get("name")
				.and_then(|n| n.as_str())
				.context("function.name が未指定")?
				.to_string();
			let description = function.get("description").and_then(|d| d.as_str()).map(str::to_string);
			let parameters = function.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object"}));
			let strict = function.get("strict").and_then(|s| s.as_bool());
			return Ok(Tool::Function {
				name,
				description,
				parameters,
				strict,
			});
		}
		if map.get("type").and_then(|t| t.as_str()) == Some("custom") && map.get("custom").map(|f| f.is_object()).unwrap_or(false) {
			let custom = map.get("custom").and_then(|f| f.as_object()).unwrap();
			let name = custom
				.get("name")
				.and_then(|n| n.as_str())
				.context("custom.name が未指定")?
				.to_string();
			let description = custom.get("description").and_then(|d| d.as_str()).map(str::to_string);
			let format = custom.get("format").cloned();
			return Ok(Tool::Custom { name, description, format });
		}
	}
	serde_json::from_value::<Tool>(v).context("Tool として解釈できない JSON")
}

/// 宣言された tool 名の集合を返す。hosted tools は固定名（`"web_search"` 等）。
#[allow(dead_code)]
pub(crate) fn declared_tool_names(tools: &[Tool]) -> HashSet<String> {
	tools.iter().map(|t| t.name().to_string()).collect()
}

/// VAC が **ローカル実行する** tool 名（`Function` / `Custom`）の集合。
/// hosted tools（web_search / file_search / code_interpreter）は除外。
pub(crate) fn locally_dispatched_tool_names(tools: &[Tool]) -> HashSet<String> {
	tools
		.iter()
		.filter(|t| t.is_locally_dispatched())
		.map(|t| t.name().to_string())
		.collect()
}

pub(crate) fn validate_function_tool_schemas(tools: &[Tool], require_strict: bool) -> Result<()> {
	for tool in tools {
		let Tool::Function {
			name, parameters, strict, ..
		} = tool
		else {
			continue;
		};
		if require_strict && *strict != Some(true) {
			bail!("function tool {name:?} は strict: true が必要です");
		}
		if *strict == Some(true) {
			validate_strict_schema(parameters, &format!("tool[{name}].parameters"))?;
		}
	}
	Ok(())
}

fn validate_strict_schema(schema: &Value, path: &str) -> Result<()> {
	let Some(object) = schema.as_object() else {
		bail!("{path} はJSON object schemaである必要があります");
	};
	let is_object = object.get("type").is_some_and(|value| match value {
		Value::String(value) => value == "object",
		Value::Array(values) => values.iter().any(|value| value.as_str() == Some("object")),
		_ => false,
	});
	if is_object {
		if object.get("additionalProperties") != Some(&Value::Bool(false)) {
			bail!("{path}.additionalProperties は false が必要です");
		}
		let properties = object.get("properties").and_then(Value::as_object);
		let required = object
			.get("required")
			.and_then(Value::as_array)
			.map(|values| values.iter().filter_map(Value::as_str).collect::<HashSet<_>>())
			.unwrap_or_default();
		if let Some(properties) = properties {
			for (name, child) in properties {
				if !required.contains(name.as_str()) {
					bail!("{path}.required に {name:?} がありません");
				}
				validate_strict_schema(child, &format!("{path}.properties.{name}"))?;
			}
		}
	}
	validate_strict_schema_children(schema, path)
}

fn validate_strict_schema_children(schema: &Value, path: &str) -> Result<()> {
	let Some(object) = schema.as_object() else {
		return Ok(());
	};
	if let Some(items) = object.get("items") {
		validate_strict_schema(items, &format!("{path}.items"))?;
	}
	for keyword in ["anyOf", "oneOf", "allOf"] {
		if let Some(items) = object.get(keyword).and_then(Value::as_array) {
			for (index, item) in items.iter().enumerate() {
				validate_strict_schema(item, &format!("{path}.{keyword}[{index}]"))?;
			}
		}
	}
	Ok(())
}

pub(crate) fn apply_allowed_tools(tools: &[Tool], allowed_names: &[String]) -> Result<(ToolChoice, HashSet<String>)> {
	let by_name = tools
		.iter()
		.map(|tool| (tool.name(), tool))
		.collect::<std::collections::HashMap<_, _>>();
	let mut refs = Vec::with_capacity(allowed_names.len());
	let mut local = HashSet::new();
	for name in allowed_names {
		let tool = by_name
			.get(name.as_str())
			.ok_or_else(|| anyhow::anyhow!("allowed_tools に未宣言tool {name:?} が指定されています"))?;
		let (kind, ref_name) = match tool {
			Tool::Function { .. } => ("function", Some(name.clone())),
			Tool::Custom { .. } => ("custom", Some(name.clone())),
			Tool::WebSearch { .. } => ("web_search", None),
			Tool::FileSearch { .. } => ("file_search", None),
			Tool::CodeInterpreter { .. } => ("code_interpreter", None),
		};
		refs.push(AllowedToolRef {
			kind: kind.to_string(),
			name: ref_name,
		});
		if tool.is_locally_dispatched() {
			local.insert(name.clone());
		}
	}
	Ok((ToolChoice::Allowed(AllowedToolsChoice::auto(refs)), local))
}

/// `openai_tool_choice` 文字列を [`ToolChoice`] に変換する。
pub(crate) fn parse_tool_choice(s: &str) -> Result<ToolChoice> {
	match s.trim().to_lowercase().as_str() {
		"none" => Ok(ToolChoice::Mode(ToolChoiceMode::None)),
		"auto" => Ok(ToolChoice::Mode(ToolChoiceMode::Auto)),
		"required" => Ok(ToolChoice::Mode(ToolChoiceMode::Required)),
		other => {
			// `function:<name>` / `custom:<name>` の named 指定に対応。
			if let Some(rest) = other.strip_prefix("function:") {
				let name = rest.trim();
				if name.is_empty() {
					bail!("openai_tool_choice の function: 後に名前がありません");
				}
				return Ok(ToolChoice::Named(NamedToolChoice::Function { name: name.to_string() }));
			}
			if let Some(rest) = other.strip_prefix("custom:") {
				let name = rest.trim();
				if name.is_empty() {
					bail!("openai_tool_choice の custom: 後に名前がありません");
				}
				return Ok(ToolChoice::Named(NamedToolChoice::Custom { name: name.to_string() }));
			}
			bail!(
				"openai_tool_choice は none / auto / required / function:<name> / custom:<name> のいずれかにしてください: {:?}",
				s
			)
		}
	}
}

pub(crate) async fn run_function_tool(name: &str, arguments: &str, ctx: &ToolContext) -> String {
	match name {
		"vac_ping" => {
			let _ = arguments;
			r#"{"ok":true,"tool":"vac_ping"}"#.to_string()
		}
		"vac_emit_effect" => action_emit_effect(arguments, ctx).await,
		"vac_emit_avatar_intent" => action_emit_avatar_intent(arguments, ctx).await,
		"vac_twitch_set_category" => action_twitch_set_category(arguments, ctx).await,
		"vac_twitch_chat_say" => action_twitch_chat_say(arguments, ctx).await,
		"vac_twitch_ban_user" => action_twitch_ban_user(arguments, ctx).await,
		"vac_twitch_unban_user" => action_twitch_unban_user(arguments, ctx).await,
		"vac_twitch_delete_message" => action_twitch_delete_message(arguments, ctx).await,
		_ => json_tool_error("unknown tool", name),
	}
}

fn json_tool_error(key: &str, name: &str) -> String {
	let esc = name.replace('\\', "\\\\").replace('"', "\\\"");
	format!(r#"{{"error":"{}","name":"{}"}}"#, key, esc)
}

/// tool 縺ｮ謌ｻ繧雁､繧・`serde_json::Value` 縺九ｉ譁・ｭ怜・蛹悶☆繧九・繝ｫ繝代・縲・
fn json_ok(value: Value) -> String {
	serde_json::to_string(&value).unwrap_or_else(|_| r#"{"ok":true}"#.to_string())
}

fn json_err(message: impl Into<String>) -> String {
	let m = message.into();
	serde_json::to_string(&json!({"ok": false, "error": m}))
		.unwrap_or_else(|_| r#"{"ok":false,"error":"serialization_failed"}"#.to_string())
}

/// Responses API ネイティブな tool call ディスパッチ。
///
/// 入力は `OutputItem::FunctionCall` から取った [`FunctionCallView`]（非所有参照）。
/// 出力は次ラウンドの `input[]` に積むための [`InputItem::FunctionCallOutput`]。
///
/// - `declared` に入っていない名前は即座に `{"error":"tool not declared...","name":"..."}` を返す
/// - 宣言済みなら [`run_function_tool`] に委譲
pub(crate) async fn dispatch_tool_call(call: FunctionCallView<'_>, declared: &HashSet<String>, ctx: &ToolContext) -> InputItem {
	let argument_chars = call.arguments.chars().count();
	emit_tool_call_event(ctx, AiToolCallPhase::Started, call.name, argument_chars, 0, None, None).await;
	let started = Instant::now();
	let output = if !declared.contains(call.name) {
		json_tool_error("tool not declared in openai_tools_json", call.name)
	} else {
		run_function_tool(call.name, call.arguments, ctx).await
	};
	let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
	let (success, error) = tool_output_status(&output);
	emit_tool_call_event(
		ctx,
		AiToolCallPhase::Completed,
		call.name,
		argument_chars,
		elapsed_ms,
		Some(success),
		error,
	)
	.await;
	InputItem::function_call_output(call.call_id, output)
}

async fn emit_tool_call_event(
	ctx: &ToolContext,
	phase: AiToolCallPhase,
	tool_name: &str,
	argument_chars: usize,
	elapsed_ms: u64,
	success: Option<bool>,
	error: Option<String>,
) {
	let control_event_tx = ctx.state.read().await.control_event_tx.clone();
	let _ = control_event_tx.send(ControlEvent::AiToolCall {
		phase,
		persona_id: ctx.persona_label.clone(),
		tool_name: tool_name.to_string(),
		argument_chars,
		elapsed_ms,
		success,
		error,
		observed_at: jiff::Timestamp::now().to_string(),
	});
}

fn tool_output_status(output: &str) -> (bool, Option<String>) {
	let Ok(value) = serde_json::from_str::<Value>(output) else {
		return (false, Some("tool returned non-JSON output".to_string()));
	};
	let error = value
		.get("error")
		.and_then(Value::as_str)
		.map(|message| message.chars().take(240).collect::<String>());
	let success = value.get("ok").and_then(Value::as_bool).unwrap_or(error.is_none());
	(success, error)
}

// ---------- Phase IV: Action impls ----------

/// `vac_emit_effect`: 莉ｻ諢上・ VAC 繝√Ε繝ｳ繝阪Ν縺ｸ `ChannelDatum` 繧・push 縺吶ｋ縲・
///
/// 蠑墓焚 JSON:
/// ```json
/// {"channel":"effect","content":"sparkle","meta":{"intensity":3}}
/// ```
/// - `channel`: 逵∫払譎ゅ・ `ToolContext::effect_channel`
/// - `content`: 蠢・医よｼ泌・隴伜挨蟄舌ｄ繝・く繧ｹ繝医ゅヶ繝ｩ繧ｦ繧ｶ繧ｽ繝ｼ繧ｹ蛛ｴ縺ｧ隗｣驥医☆繧九・
/// - `meta`: 莉ｻ諢上ＡChannelDatum::meta` 縺ｫ key=value 縺ｨ縺励※繧ｳ繝斐・縺吶ｋ・・Value::Object` 縺ｮ縺ｿ・峨・
pub(crate) fn constrain_effect_channels(tools: &mut [Tool], allowed: Option<&[String]>, default_channel: &str) -> Result<()> {
    let Some(allowed) = allowed else { return Ok(()); };
    if allowed.is_empty() || allowed.iter().any(|c| c.trim().is_empty() || c.trim() != c) {
        bail!("effect_allowed_channels must contain non-empty channel names without surrounding whitespace");
    }
    if !allowed.iter().any(|c| c == default_channel) {
        bail!("effect_allowed_channels must include configured channel_effect {default_channel:?}");
    }
    for tool in tools {
        let Tool::Function { name, parameters, description, .. } = tool else { continue; };
        if name != "vac_emit_effect" { continue; }
        if let Some(channel) = parameters.pointer_mut("/properties/channel") {
            let nullable = channel["type"] == "null" || channel["type"].as_array().is_some_and(|types| types.iter().any(|t| t == "null"));
            let mut choices: Vec<Value> = allowed.iter().map(|c| json!(c)).collect();
            if nullable { choices.push(Value::Null); }
            // A custom schema may already restrict destinations: never widen it.
            if let Some(existing) = channel.get("enum").and_then(Value::as_array) {
                choices.retain(|choice| existing.contains(choice));
            }
            if choices.is_empty() { bail!("vac_emit_effect channel schema and effect_allowed_channels do not overlap"); }
            channel["enum"] = json!(choices);
            channel["description"] = json!(format!("演出の送り先。許可先: {}。null/省略は{default_channel}。画面観測や発話の入力先へは送らない。",allowed.join(", ")));
        }
        let suffix = format!(" 設定された演出先: {}。既定: {default_channel}。観測情報の記録には使わない。", allowed.join(", "));
        description.get_or_insert_with(String::new).push_str(&suffix);
    }
    Ok(())
}

#[cfg(test)]
mod effect_destination_tests {
    use super::*;
    #[test]
    fn effect_destination_schema_is_explicit_and_does_not_widen_custom_enum() {
        let original: Tool = serde_json::from_value(json!({"type":"function","name":"vac_emit_effect","strict":true,
            "parameters":{"type":"object","additionalProperties":false,"properties":{"channel":{"type":["string","null"]}},"required":["channel"]}})).unwrap();
        let mut tools=vec![original.clone()];
        constrain_effect_channels(&mut tools,None,"effect").unwrap();
        assert_eq!(tools[0],original);
        let allowed=vec!["effect".into(),"reaction".into()];
        constrain_effect_channels(&mut tools,Some(&allowed),"effect").unwrap();
        assert_eq!(serde_json::to_value(&tools[0]).unwrap()["parameters"]["properties"]["channel"]["enum"],json!(["effect","reaction",null]));
        let narrower=vec!["effect".into()];
        constrain_effect_channels(&mut tools,Some(&narrower),"effect").unwrap();
        constrain_effect_channels(&mut tools,Some(&allowed),"effect").unwrap();
        assert_eq!(serde_json::to_value(&tools[0]).unwrap()["parameters"]["properties"]["channel"]["enum"],json!(["effect",null]));
        assert!(constrain_effect_channels(&mut tools,Some(&[]),"effect").is_err());
        assert!(constrain_effect_channels(&mut tools,Some(&["reaction".into()]),"effect").is_err());
        assert!(constrain_effect_channels(&mut tools,Some(&["effect".into()," reaction".into()]),"effect").is_err());
    }
    #[tokio::test]
    async fn effect_destination_rejects_observation_writes_and_keeps_multiple_outputs() {
        let mut conf: crate::Conf=toml::from_str("").unwrap();
        conf.runtime_dir=Some(std::env::temp_dir().join(format!("vac_effect_dest_{}",rand::random::<u64>())));
        let state=crate::State::new(&conf,Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap())),crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let ctx=ToolContext {state:state.clone(),persona_label:"test".into(),effect_channel:"effect".into(),effect_allowed_channels:Some(vec!["effect".into(),"reaction".into()]),twitch_eventsub:None,twitch_moderator:None,twitch_default_broadcaster_login:None};
        for channel in ["obs:game_frame","user","ai"] {
            let result=action_emit_effect(&json!({"channel":channel,"content":"not an observation"}).to_string(),&ctx).await;
            assert!(serde_json::from_str::<Value>(&result).unwrap().get("error").is_some());
        }
        assert!(state.read().await.channel_data.read().await.is_empty());
        for channel in [Value::Null,json!("reaction")] {
            let result=action_emit_effect(&json!({"channel":channel,"content":"sparkle","meta":{"intensity":0.3}}).to_string(),&ctx).await;
            assert_eq!(serde_json::from_str::<Value>(&result).unwrap()["ok"],true);
        }
        let shared=state.read().await;
        let data=shared.channel_data.read().await;
        assert_eq!(data.iter().map(|d|d.channel.as_str()).collect::<Vec<_>>(),vec!["effect","reaction"]);
        assert!(data.iter().all(|d|d.source.as_ref().unwrap().kind=="ai"));
    }
}

async fn action_emit_effect(arguments: &str, ctx: &ToolContext) -> String {
	let args: Value = match serde_json::from_str(arguments) {
		Ok(v) => v,
		Err(_) if arguments.trim().is_empty() => Value::Object(Default::default()),
		Err(e) => return json_err(format!("invalid arguments JSON: {}", e)),
	};

	let content = match args.get("content").and_then(|v| v.as_str()) {
		Some(s) if !s.trim().is_empty() => s.to_string(),
		_ => return json_err("content is required (non-empty string)"),
	};
	let channel = args
		.get("channel")
		.and_then(|v| v.as_str())
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.unwrap_or_else(|| ctx.effect_channel.clone());

    if ctx.effect_allowed_channels.as_ref().is_some_and(|allowed| !allowed.contains(&channel)) {
        return json_err(format!("effect channel {channel:?} is not allowed; allowed destinations: {}", ctx.effect_allowed_channels.as_ref().unwrap().join(", ")));
    }

	let mut cd = ChannelDatum::new(channel.clone(), content.clone())
		.with_flag(ChannelDatum::FLAG_IS_FINAL)
		.with_source(
			DataSource::new("ai")
				.with_subtype("action.emit_effect")
				.with_actor(&ctx.persona_label),
		);

	if let Some(meta) = args.get("meta").and_then(|v| v.as_object()) {
		for (k, v) in meta {
			let vs = match v {
				Value::String(s) => s.clone(),
				other => other.to_string(),
			};
			cd = cd.with_meta(k, vs);
		}
	}

	let id = cd.get_id();
	log::info!(
		"縲晦I[{}]縲・action vac_emit_effect 竊・channel={} content={:?} (datum_id={})",
		ctx.persona_label,
		channel,
		content,
		id
	);
	ctx.state.read().await.push_channel_datum(cd).await;
	json_ok(json!({"ok": true, "tool": "vac_emit_effect", "channel": channel, "datum_id": id}))
}

async fn action_emit_avatar_intent(arguments: &str, ctx: &ToolContext) -> String {
	let (channel, target_link_id, intent_count, cd) = match build_avatar_intent_datum(arguments, &ctx.persona_label) {
		Ok(v) => v,
		Err(e) => return json_err(e),
	};
	let id = cd.get_id();
	log::info!(
		"《AI[{}]》 action vac_emit_avatar_intent -> channel={} target_link_id={} intents={} (datum_id={})",
		ctx.persona_label,
		channel,
		target_link_id,
		intent_count,
		id
	);
	ctx.state.read().await.push_channel_datum(cd).await;
	json_ok(json!({
		"ok": true,
		"tool": "vac_emit_avatar_intent",
		"channel": channel,
		"target_link_id": target_link_id,
		"intent_count": intent_count,
		"datum_id": id,
	}))
}

pub(crate) fn build_avatar_intent_datum(
	arguments: &str,
	persona_label: &str,
) -> std::result::Result<(String, String, usize, ChannelDatum), String> {
	let args: Value = serde_json::from_str(arguments).map_err(|e| format!("invalid arguments JSON: {e}"))?;
	let channel = args
		.get("channel")
		.and_then(|v| v.as_str())
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty())
		.unwrap_or_else(|| DEFAULT_AVATAR_INTENT_CHANNEL.to_string());
	let request: AvatarIntentRequest = serde_json::from_value(args.clone()).map_err(|e| format!("invalid AvatarIntent JSON: {e}"))?;
	let Some(target_link_id) = request
		.target_link_id
		.as_deref()
		.map(str::trim)
		.filter(|s| !s.is_empty())
		.map(str::to_string)
	else {
		return Err("target_link_id is required for vac_emit_avatar_intent".to_string());
	};
	if request.intents.is_empty() {
		return Err("intents must contain at least one avatar intent".to_string());
	}
	if let Err(e) = avatar_intent_to_unmf(&request, &AvatarIntentMapperOptions::default()) {
		return Err(e);
	}
	let content = serde_json::to_string(&request).map_err(|e| format!("failed to serialize AvatarIntent: {e}"))?;
	let mut cd = ChannelDatum::new(channel.clone(), content)
		.with_flag(ChannelDatum::FLAG_IS_FINAL)
		.with_source(
			DataSource::new("ai")
				.with_subtype("action.emit_avatar_intent")
				.with_actor(persona_label),
		)
		.with_meta("source_actor", persona_label.to_string())
		.with_meta("target_link_id", target_link_id.clone())
		.with_meta("intent_count", request.intents.len() as i64)
		.with_meta("schema", "unvac.avatar_intent.v1");
	if let Some(meta) = args.get("meta").and_then(|v| v.as_object()) {
		for (k, v) in meta {
			if matches!(k.as_str(), "source_actor" | "target_link_id" | "intent_count" | "schema") {
				continue;
			}
			cd = cd.with_meta(k, v.clone());
		}
	}
	Ok((channel, target_link_id, request.intents.len(), cd))
}

/// `vac_twitch_set_category`: Helix `PATCH /channels` 縺ｧ驟堺ｿ｡繧ｫ繝・ざ繝ｪ繝ｼ繧貞､画峩縺吶ｋ縲・
///
/// 蠑墓焚 JSON:
/// ```json
/// {"game_name":"Just Chatting"}
/// ```
/// 縺ｾ縺溘・ `{"game_id":"509658"}`縲ゆｸ｡譁ｹ謖・ｮ壽凾縺ｯ `game_id` 繧貞━蜈医☆繧九・
///
/// 隕√せ繧ｳ繝ｼ繝・ `channel:manage:broadcast`・・twitch_oauth` 縺ｮ譌｢螳壹せ繧ｳ繝ｼ繝励↓蜷ｫ繧√※縺ゅｋ・峨・
async fn action_twitch_set_category(arguments: &str, ctx: &ToolContext) -> String {
	let Some(es) = ctx.twitch_eventsub.clone() else {
		return json_err("Twitch eventsub config is not available for this persona");
	};

	let args: Value = match serde_json::from_str(arguments) {
		Ok(v) => v,
		Err(e) => return json_err(format!("invalid arguments JSON: {}", e)),
	};

	let game_id_arg = args
		.get("game_id")
		.and_then(|v| v.as_str())
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty());
	let game_name_arg = args
		.get("game_name")
		.and_then(|v| v.as_str())
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty());
	if game_id_arg.is_none() && game_name_arg.is_none() {
		return json_err("either game_id or game_name is required");
	}

	let access_token = match crate::twitch::oauth::ensure_user_access_token(&es).await {
		Ok(t) => t,
		Err(e) => return json_err(format!("failed to get Twitch user access token: {}", e)),
	};
	let client_id = crate::twitch::oauth::resolve_twitch_client_id(&es);
	let broadcaster_login = es.broadcaster_login.clone().filter(|s| !s.trim().is_empty()).unwrap_or_default();
	if broadcaster_login.is_empty() {
		return json_err("twitch.eventsub.broadcaster_login is not configured");
	}

	let broadcaster_id = match helix_user_id(&client_id, &access_token, &broadcaster_login).await {
		Ok(id) => id,
		Err(e) => return json_err(format!("failed to resolve broadcaster_id: {}", e)),
	};

	let game_id = match (game_id_arg.clone(), game_name_arg.clone()) {
		(Some(id), _) => id,
		(None, Some(name)) => match helix_resolve_game_id(&client_id, &access_token, &name).await {
			Ok(Some(id)) => id,
			Ok(None) => return json_err(format!("game not found: {}", name)),
			Err(e) => return json_err(format!("failed to resolve game_id: {}", e)),
		},
		(None, None) => unreachable!(),
	};

	match helix_patch_channel_game_id(&client_id, &access_token, &broadcaster_id, &game_id).await {
		Ok(_) => {
			log::info!(
				"縲晦I[{}]縲・action vac_twitch_set_category 竊・broadcaster_id={} game_id={} (requested game_name={:?})",
				ctx.persona_label,
				broadcaster_id,
				game_id,
				game_name_arg
			);
			json_ok(json!({
			 "ok": true,
			 "tool": "vac_twitch_set_category",
			 "broadcaster_id": broadcaster_id,
			 "game_id": game_id,
			 "game_name": game_name_arg,
			}))
		}
		Err(e) => json_err(format!("helix patch channels failed: {}", e)),
	}
}

// ---------- Phase V-c: Moderation actions ----------

/// 繝｢繝・Ξ繝ｼ繧ｿ繝ｼ邉ｻ繝・・繝ｫ蜈ｱ騾壹・蜑榊・逅・Ａ(eventsub, moderator, client_id, moderator_access_token, sender_user_id, broadcaster_login, broadcaster_id)` 繧定ｿ斐☆縲・
async fn resolve_twitch_moderation(
	args: &Value,
	ctx: &ToolContext,
) -> std::result::Result<(String, String, String, String, String), String> {
	let Some(es) = ctx.twitch_eventsub.clone() else {
		return Err("Twitch eventsub config is not available for this persona".into());
	};
	let Some(mc) = ctx.twitch_moderator.clone() else {
		return Err("Twitch moderator (bot) account config is not available for this persona".into());
	};
	if !mc.enabled {
		return Err("Twitch moderator account is disabled (twitch.moderator.enabled = false)".into());
	}

	let client_id = crate::twitch::oauth::resolve_twitch_client_id(&es);
	let access_token = match crate::twitch::oauth::ensure_moderator_access_token(&es, &mc).await {
		Ok(t) => t,
		Err(e) => return Err(format!("failed to get moderator access token: {}", e)),
	};
	let sender_user_id = match helix_validate_user_id(&access_token).await {
		Ok(id) => id,
		Err(e) => return Err(format!("failed to resolve moderator user_id via validate: {}", e)),
	};

	let broadcaster_login = args
		.get("broadcaster_login")
		.and_then(|v| v.as_str())
		.map(|s| s.trim().trim_start_matches('#').to_lowercase())
		.filter(|s| !s.is_empty())
		.or_else(|| {
			ctx.twitch_default_broadcaster_login
				.as_deref()
				.map(|s| s.trim().trim_start_matches('#').to_lowercase())
				.filter(|s| !s.is_empty())
		})
		.ok_or_else(|| "broadcaster_login is required (or set [twitch.eventsub] / [twitch].username as fallback)".to_string())?;

	let broadcaster_id = match helix_user_id(&client_id, &access_token, &broadcaster_login).await {
		Ok(id) => id,
		Err(e) => return Err(format!("failed to resolve broadcaster_id: {}", e)),
	};

	Ok((client_id, access_token, sender_user_id, broadcaster_login, broadcaster_id))
}

/// 蠑墓焚 `{user_id? | user_login?}` 縺九ｉ Twitch user_id 繧定ｧ｣豎ｺ縺吶ｋ縲・
async fn resolve_target_user_id(args: &Value, client_id: &str, bearer: &str) -> std::result::Result<String, String> {
	if let Some(id) = args
		.get("user_id")
		.and_then(|v| v.as_str())
		.map(|s| s.trim())
		.filter(|s| !s.is_empty())
	{
		return Ok(id.to_string());
	}
	if let Some(login) = args
		.get("user_login")
		.and_then(|v| v.as_str())
		.map(|s| s.trim().trim_start_matches('@').to_lowercase())
		.filter(|s| !s.is_empty())
	{
		return helix_user_id(client_id, bearer, &login)
			.await
			.map_err(|e| format!("failed to resolve user_id from user_login={}: {}", login, e));
	}
	Err("either user_id or user_login is required".into())
}

/// `vac_twitch_chat_say`: Helix `POST /chat/messages` 縺ｧ繝懊ャ繝亥哨逋ｺ隧ｱ縲・
async fn action_twitch_chat_say(arguments: &str, ctx: &ToolContext) -> String {
	let args: Value = match serde_json::from_str(arguments) {
		Ok(v) => v,
		Err(_) if arguments.trim().is_empty() => Value::Object(Default::default()),
		Err(e) => return json_err(format!("invalid arguments JSON: {}", e)),
	};
	let content = match args.get("content").and_then(|v| v.as_str()) {
		Some(s) if !s.trim().is_empty() => s.trim().to_string(),
		_ => return json_err("content is required (non-empty string)"),
	};
	let reply_parent_message_id = args
		.get("reply_parent_message_id")
		.and_then(|v| v.as_str())
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty());

	let (client_id, bearer, sender_id, _login, broadcaster_id) = match resolve_twitch_moderation(&args, ctx).await {
		Ok(v) => v,
		Err(e) => return json_err(e),
	};

	let url = "https://api.twitch.tv/helix/chat/messages";
	let mut body = json!({
	 "broadcaster_id": broadcaster_id,
	 "sender_id": sender_id,
	 "message": content,
	});
	if let Some(rp) = &reply_parent_message_id {
		body["reply_parent_message_id"] = Value::String(rp.clone());
	}
	let resp = match reqwest::Client::new()
		.post(url)
		.header("Client-Id", &client_id)
		.bearer_auth(&bearer)
		.json(&body)
		.send()
		.await
	{
		Ok(r) => r,
		Err(e) => return json_err(format!("request failed: {}", e)),
	};
	let status = resp.status();
	let text = resp.text().await.unwrap_or_default();
	if !status.is_success() {
		return json_err(format!("POST /chat/messages failed: {} {}", status, text));
	}
	log::info!(
		"縲晦I[{}]縲・action vac_twitch_chat_say 竊・broadcaster_id={} sender_id={} len={}",
		ctx.persona_label,
		broadcaster_id,
		sender_id,
		content.chars().count()
	);
	json_ok(json!({"ok": true, "tool": "vac_twitch_chat_say", "broadcaster_id": broadcaster_id, "sender_id": sender_id}))
}

/// `vac_twitch_ban_user`: BAN 縺ｾ縺溘・ timeout・・duration_secs` 謖・ｮ壽凾・峨・
async fn action_twitch_ban_user(arguments: &str, ctx: &ToolContext) -> String {
	let args: Value = match serde_json::from_str(arguments) {
		Ok(v) => v,
		Err(e) => return json_err(format!("invalid arguments JSON: {}", e)),
	};
	let reason = args
		.get("reason")
		.and_then(|v| v.as_str())
		.map(|s| s.trim().to_string())
		.filter(|s| !s.is_empty());
	let duration_secs = args.get("duration_secs").and_then(|v| v.as_u64());

	let (client_id, bearer, moderator_id, _login, broadcaster_id) = match resolve_twitch_moderation(&args, ctx).await {
		Ok(v) => v,
		Err(e) => return json_err(e),
	};
	let target_id = match resolve_target_user_id(&args, &client_id, &bearer).await {
		Ok(id) => id,
		Err(e) => return json_err(e),
	};

	let url = format!(
		"https://api.twitch.tv/helix/moderation/bans?broadcaster_id={}&moderator_id={}",
		urlencoding::encode(&broadcaster_id),
		urlencoding::encode(&moderator_id)
	);
	let mut inner = serde_json::Map::new();
	inner.insert("user_id".to_string(), Value::String(target_id.clone()));
	if let Some(d) = duration_secs {
		inner.insert("duration".to_string(), Value::from(d));
	}
	if let Some(r) = reason.clone() {
		inner.insert("reason".to_string(), Value::String(r));
	}
	let body = json!({"data": Value::Object(inner)});

	let resp = match reqwest::Client::new()
		.post(&url)
		.header("Client-Id", &client_id)
		.bearer_auth(&bearer)
		.json(&body)
		.send()
		.await
	{
		Ok(r) => r,
		Err(e) => return json_err(format!("request failed: {}", e)),
	};
	let status = resp.status();
	let text = resp.text().await.unwrap_or_default();
	if !status.is_success() {
		return json_err(format!("POST /moderation/bans failed: {} {}", status, text));
	}
	log::info!(
		"縲晦I[{}]縲・action vac_twitch_ban_user 竊・broadcaster_id={} target={} duration_secs={:?} reason={:?}",
		ctx.persona_label,
		broadcaster_id,
		target_id,
		duration_secs,
		reason
	);
	json_ok(json!({
	 "ok": true,
	 "tool": "vac_twitch_ban_user",
	 "broadcaster_id": broadcaster_id,
	 "user_id": target_id,
	 "duration_secs": duration_secs,
	}))
}

/// `vac_twitch_unban_user`: BAN 隗｣髯､縲Ｕimeout 荳ｭ縺ｮ隗｣髯､縺ｫ繧ゆｽｿ縺医ｋ縲・
async fn action_twitch_unban_user(arguments: &str, ctx: &ToolContext) -> String {
	let args: Value = match serde_json::from_str(arguments) {
		Ok(v) => v,
		Err(e) => return json_err(format!("invalid arguments JSON: {}", e)),
	};
	let (client_id, bearer, moderator_id, _login, broadcaster_id) = match resolve_twitch_moderation(&args, ctx).await {
		Ok(v) => v,
		Err(e) => return json_err(e),
	};
	let target_id = match resolve_target_user_id(&args, &client_id, &bearer).await {
		Ok(id) => id,
		Err(e) => return json_err(e),
	};

	let url = format!(
		"https://api.twitch.tv/helix/moderation/bans?broadcaster_id={}&moderator_id={}&user_id={}",
		urlencoding::encode(&broadcaster_id),
		urlencoding::encode(&moderator_id),
		urlencoding::encode(&target_id)
	);
	let resp = match reqwest::Client::new()
		.delete(&url)
		.header("Client-Id", &client_id)
		.bearer_auth(&bearer)
		.send()
		.await
	{
		Ok(r) => r,
		Err(e) => return json_err(format!("request failed: {}", e)),
	};
	let status = resp.status();
	if !status.is_success() {
		let text = resp.text().await.unwrap_or_default();
		return json_err(format!("DELETE /moderation/bans failed: {} {}", status, text));
	}
	log::info!(
		"縲晦I[{}]縲・action vac_twitch_unban_user 竊・broadcaster_id={} target={}",
		ctx.persona_label,
		broadcaster_id,
		target_id,
	);
	json_ok(json!({"ok": true, "tool": "vac_twitch_unban_user", "broadcaster_id": broadcaster_id, "user_id": target_id}))
}

/// `vac_twitch_delete_message`: 迚ｹ螳壹Γ繝・そ繝ｼ繧ｸ縺ｮ蜑企勁縲Ａmessage_id` 蠢・医・
async fn action_twitch_delete_message(arguments: &str, ctx: &ToolContext) -> String {
	let args: Value = match serde_json::from_str(arguments) {
		Ok(v) => v,
		Err(e) => return json_err(format!("invalid arguments JSON: {}", e)),
	};
	let message_id = match args
		.get("message_id")
		.and_then(|v| v.as_str())
		.map(|s| s.trim())
		.filter(|s| !s.is_empty())
	{
		Some(m) => m.to_string(),
		None => return json_err("message_id is required"),
	};
	let (client_id, bearer, moderator_id, _login, broadcaster_id) = match resolve_twitch_moderation(&args, ctx).await {
		Ok(v) => v,
		Err(e) => return json_err(e),
	};

	let url = format!(
		"https://api.twitch.tv/helix/moderation/chat?broadcaster_id={}&moderator_id={}&message_id={}",
		urlencoding::encode(&broadcaster_id),
		urlencoding::encode(&moderator_id),
		urlencoding::encode(&message_id)
	);
	let resp = match reqwest::Client::new()
		.delete(&url)
		.header("Client-Id", &client_id)
		.bearer_auth(&bearer)
		.send()
		.await
	{
		Ok(r) => r,
		Err(e) => return json_err(format!("request failed: {}", e)),
	};
	let status = resp.status();
	if !status.is_success() {
		let text = resp.text().await.unwrap_or_default();
		return json_err(format!("DELETE /moderation/chat failed: {} {}", status, text));
	}
	log::info!(
		"縲晦I[{}]縲・action vac_twitch_delete_message 竊・broadcaster_id={} message_id={}",
		ctx.persona_label,
		broadcaster_id,
		message_id
	);
	json_ok(json!({"ok": true, "tool": "vac_twitch_delete_message", "broadcaster_id": broadcaster_id, "message_id": message_id}))
}

/// 繝医・繧ｯ繝ｳ謇譛芽・・ user_id 繧貞叙蠕励☆繧具ｼ・alidate 繝ｬ繧ｹ繝昴Φ繧ｹ縺ｫ蜷ｫ縺ｾ繧後ｋ・峨・
async fn helix_validate_user_id(bearer: &str) -> Result<String> {
	let resp = reqwest::Client::new()
		.get("https://id.twitch.tv/oauth2/validate")
		.header("Authorization", format!("Bearer {}", bearer.trim()))
		.send()
		.await?;
	let status = resp.status();
	let text = resp.text().await.unwrap_or_default();
	if !status.is_success() {
		bail!("validate failed: {} {}", status, text);
	}
	let v: Value = serde_json::from_str(&text).context("validate response JSON parse failed")?;
	v.get("user_id")
		.and_then(|u| u.as_str())
		.map(|s| s.to_string())
		.ok_or_else(|| anyhow::anyhow!("validate: user_id not found"))
}

async fn helix_user_id(client_id: &str, bearer: &str, login: &str) -> Result<String> {
	let url = format!("https://api.twitch.tv/helix/users?login={}", urlencoding::encode(login));
	let resp = reqwest::Client::new()
		.get(&url)
		.header("Client-Id", client_id)
		.bearer_auth(bearer)
		.send()
		.await?
		.error_for_status()?;
	let v: Value = resp.json().await?;
	v.get("data")
		.and_then(|d| d.as_array())
		.and_then(|a| a.first())
		.and_then(|u| u.get("id"))
		.and_then(|i| i.as_str())
		.map(|s| s.to_string())
		.context("helix users: id not found")
}

async fn helix_resolve_game_id(client_id: &str, bearer: &str, name: &str) -> Result<Option<String>> {
	let url = format!("https://api.twitch.tv/helix/games?name={}", urlencoding::encode(name));
	let resp = reqwest::Client::new()
		.get(&url)
		.header("Client-Id", client_id)
		.bearer_auth(bearer)
		.send()
		.await?
		.error_for_status()?;
	let v: Value = resp.json().await?;
	Ok(v.get("data")
		.and_then(|d| d.as_array())
		.and_then(|a| a.first())
		.and_then(|g| g.get("id"))
		.and_then(|i| i.as_str())
		.map(|s| s.to_string()))
}

async fn helix_patch_channel_game_id(client_id: &str, bearer: &str, broadcaster_id: &str, game_id: &str) -> Result<()> {
	let url = format!(
		"https://api.twitch.tv/helix/channels?broadcaster_id={}",
		urlencoding::encode(broadcaster_id)
	);
	let body = json!({"game_id": game_id});
	let resp = reqwest::Client::new()
		.patch(&url)
		.header("Client-Id", client_id)
		.bearer_auth(bearer)
		.json(&body)
		.send()
		.await?;
	if !resp.status().is_success() {
		let status = resp.status();
		let text = resp.text().await.unwrap_or_default();
		bail!("helix PATCH /channels failed: {} {}", status, text);
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	// ---------- parse_tools_json ----------

	#[test]
	fn parse_tools_accepts_chat_completions_legacy_format() {
		let j =
			r#"[{"type":"function","function":{"name":"vac_ping","description":"ping","parameters":{"type":"object","properties":{}}}}]"#;
		let v = parse_tools_json(j).unwrap();
		assert_eq!(v.len(), 1);
		match &v[0] {
			Tool::Function { name, description, .. } => {
				assert_eq!(name, "vac_ping");
				assert_eq!(description.as_deref(), Some("ping"));
			}
			other => panic!("expected Function, got {other:?}"),
		}
		assert!(declared_tool_names(&v).contains("vac_ping"));
	}

	#[test]
	fn parse_tools_accepts_responses_flat_format() {
		let j =
			r#"[{"type":"function","name":"vac_ping","description":"ping","parameters":{"type":"object","properties":{}},"strict":true}]"#;
		let v = parse_tools_json(j).unwrap();
		assert_eq!(v.len(), 1);
		match &v[0] {
			Tool::Function { name, strict, .. } => {
				assert_eq!(name, "vac_ping");
				assert_eq!(*strict, Some(true));
			}
			other => panic!("expected Function, got {other:?}"),
		}
	}

	#[test]
	fn parse_tools_preserves_strict_flag_from_legacy_format() {
		let j = r#"[{"type":"function","function":{"name":"vac_ping","parameters":{"type":"object"},"strict":true}}]"#;
		let v = parse_tools_json(j).unwrap();
		match &v[0] {
			Tool::Function { strict, .. } => assert_eq!(*strict, Some(true)),
			_ => panic!("expected Function"),
		}
	}

	#[test]
	fn parse_tools_accepts_hosted_web_search() {
		let j = r#"[{"type":"web_search","search_context_size":"medium"}]"#;
		let v = parse_tools_json(j).unwrap();
		match &v[0] {
			Tool::WebSearch { search_context_size, .. } => {
				assert_eq!(search_context_size.as_deref(), Some("medium"));
			}
			other => panic!("expected WebSearch, got {other:?}"),
		}
		assert!(declared_tool_names(&v).contains("web_search"));
		assert!(!v[0].is_locally_dispatched());
	}

	#[test]
	fn parse_tools_accepts_hosted_file_search() {
		let j = r#"[{"type":"file_search","vector_store_ids":["vs_1","vs_2"],"max_num_results":5}]"#;
		let v = parse_tools_json(j).unwrap();
		match &v[0] {
			Tool::FileSearch {
				vector_store_ids,
				max_num_results,
				..
			} => {
				assert_eq!(vector_store_ids, &vec!["vs_1".to_string(), "vs_2".to_string()]);
				assert_eq!(*max_num_results, Some(5));
			}
			other => panic!("expected FileSearch, got {other:?}"),
		}
	}

	#[test]
	fn parse_tools_accepts_mixed_local_and_hosted() {
		let j = r#"[
   {"type":"function","function":{"name":"vac_ping","parameters":{"type":"object"}}},
   {"type":"web_search"},
   {"type":"function","name":"vac_other","parameters":{"type":"object"}}
  ]"#;
		let v = parse_tools_json(j).unwrap();
		assert_eq!(v.len(), 3);
		let local = locally_dispatched_tool_names(&v);
		assert!(local.contains("vac_ping"));
		assert!(local.contains("vac_other"));
		assert!(!local.contains("web_search"));
		assert!(declared_tool_names(&v).contains("web_search"));
	}

	#[test]
	fn parse_tools_accepts_distributed_example_tools_json() {
		let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("conf.example-openai-tools.json");
		let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} を読めない: {e}", path.display()));
		let tools = parse_tools_json(&text).unwrap_or_else(|e| panic!("conf.example-openai-tools.json を parse できない: {e}"));
		validate_function_tool_schemas(&tools, true)
			.unwrap_or_else(|e| panic!("conf.example-openai-tools.json がstrict schemaではない: {e}"));
		let names = declared_tool_names(&tools);
		assert!(names.contains("vac_ping"));
		assert!(names.contains("vac_emit_effect"));
		assert!(names.contains("vac_emit_avatar_intent"));
	}

	#[test]
	fn strict_schema_validation_rejects_optional_property_shape() {
		let tools = vec![Tool::function(
			"unsafe_shape",
			json!({
				"type": "object",
				"properties": {"optional": {"type": "string"}},
				"required": [],
				"additionalProperties": false
			}),
			None,
			Some(true),
		)];
		assert!(validate_function_tool_schemas(&tools, true).is_err());
	}

	#[test]
	fn allowed_tools_builds_api_choice_and_local_defense_set() {
		let tools = vec![
			Tool::function(
				"vac_ping",
				json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
				None,
				Some(true),
			),
			Tool::WebSearch {
				user_location: None,
				search_context_size: None,
			},
		];
		let allowed = vec!["vac_ping".to_string(), "web_search".to_string()];
		let (choice, local) = apply_allowed_tools(&tools, &allowed).unwrap();
		let value = serde_json::to_value(choice).unwrap();
		assert_eq!(value["type"], "allowed_tools");
		assert_eq!(value["mode"], "auto");
		assert_eq!(value["tools"][0]["name"], "vac_ping");
		assert_eq!(value["tools"][1]["type"], "web_search");
		assert!(local.contains("vac_ping"));
		assert!(!local.contains("web_search"));
	}

	#[test]
	fn build_avatar_intent_datum_outputs_final_channel_datum() {
		let raw = r#"{
		 "target_link_id": "kaltsit_ai",
		 "intents": [
		  {"kind": "expression", "name": "concerned", "intensity": 0.45, "duration_ms": 1200}
		 ],
		 "meta": {"reason": "comment_reaction"}
		}"#;
		let (channel, target, count, cd) = build_avatar_intent_datum(raw, "Kaltsit").unwrap();
		assert_eq!(channel, "avatar_intent");
		assert_eq!(target, "kaltsit_ai");
		assert_eq!(count, 1);
		assert_eq!(cd.channel, "avatar_intent");
		assert!(cd.has_flag(ChannelDatum::FLAG_IS_FINAL));
		assert_eq!(cd.meta_get("source_actor").and_then(|v| v.as_str()), Some("Kaltsit"));
		assert_eq!(cd.meta_get("target_link_id").and_then(|v| v.as_str()), Some("kaltsit_ai"));
		assert_eq!(cd.meta_get("reason").and_then(|v| v.as_str()), Some("comment_reaction"));
		let request: AvatarIntentRequest = serde_json::from_str(&cd.content).unwrap();
		assert_eq!(request.target_link_id.as_deref(), Some("kaltsit_ai"));
		assert_eq!(request.intents.len(), 1);
	}

	#[test]
	fn build_avatar_intent_datum_protects_reserved_meta() {
		let raw = r#"{
		 "target_link_id": "kaltsit_ai",
		 "intents": [
		  {"kind": "expression", "name": "concerned", "intensity": 0.45}
		 ],
		 "meta": {
		  "source_actor": "spoofed",
		  "target_link_id": "other",
		  "schema": "spoofed",
		  "intent_count": 999
		 }
		}"#;
		let (_, _, _, cd) = build_avatar_intent_datum(raw, "Kaltsit").unwrap();
		assert_eq!(cd.meta_get("source_actor").and_then(|v| v.as_str()), Some("Kaltsit"));
		assert_eq!(cd.meta_get("target_link_id").and_then(|v| v.as_str()), Some("kaltsit_ai"));
		assert_eq!(cd.meta_get("schema").and_then(|v| v.as_str()), Some("unvac.avatar_intent.v1"));
		assert_eq!(cd.meta_get("intent_count").and_then(|v| v.as_i64()), Some(1));
	}

	#[test]
	fn avatar_reaction_presets_example_maps_to_unmf() {
		let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("avatar-reaction-presets.example.json");
		let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} を読めない: {e}", path.display()));
		let root: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("avatar reaction preset JSON を parse できない: {e}"));
		assert_eq!(
			root.get("schema").and_then(|v| v.as_str()),
			Some("unvac.avatar_reaction_presets.v1")
		);
		let target_link_id = root
			.get("target_link_id")
			.and_then(|v| v.as_str())
			.unwrap_or("kaltsit_ai")
			.to_string();
		let presets = root
			.get("presets")
			.and_then(|v| v.as_object())
			.expect("avatar reaction presets must contain object `presets`");
		assert!(presets.contains_key("channel_point_surprise"));
		for (name, preset) in presets {
			let intents = preset
				.get("intents")
				.cloned()
				.unwrap_or_else(|| panic!("preset `{name}` has no intents"));
			let request = AvatarIntentRequest {
				target_link_id: Some(target_link_id.clone()),
				intents: serde_json::from_value(intents).unwrap_or_else(|e| panic!("preset `{name}` intents are invalid: {e}")),
			};
			assert!(!request.intents.is_empty(), "preset `{name}` has no intents");
			avatar_intent_to_unmf(&request, &AvatarIntentMapperOptions::default())
				.unwrap_or_else(|e| panic!("preset `{name}` does not map to UNMF: {e}"));
		}
	}

	#[test]
	fn parse_tools_rejects_malformed_function() {
		let j = r#"[{"type":"function","function":{}}]"#;
		assert!(parse_tools_json(j).is_err());
	}

	#[test]
	fn parse_tools_empty_string_returns_empty_vec() {
		assert!(parse_tools_json("").unwrap().is_empty());
		assert!(parse_tools_json("   ").unwrap().is_empty());
	}

	// ---------- parse_tool_choice ----------

	#[test]
	fn parse_tool_choice_auto_none_required() {
		assert!(matches!(parse_tool_choice("auto").unwrap(), ToolChoice::Mode(ToolChoiceMode::Auto)));
		assert!(matches!(parse_tool_choice("none").unwrap(), ToolChoice::Mode(ToolChoiceMode::None)));
		assert!(matches!(
			parse_tool_choice("required").unwrap(),
			ToolChoice::Mode(ToolChoiceMode::Required)
		));
	}

	#[test]
	fn parse_tool_choice_named_function() {
		match parse_tool_choice("function:vac_ping").unwrap() {
			ToolChoice::Named(NamedToolChoice::Function { name }) => assert_eq!(name, "vac_ping"),
			other => panic!("expected Named Function, got {other:?}"),
		}
	}

	#[test]
	fn parse_tool_choice_named_custom() {
		match parse_tool_choice("custom:freeform").unwrap() {
			ToolChoice::Named(NamedToolChoice::Custom { name }) => assert_eq!(name, "freeform"),
			other => panic!("expected Named Custom, got {other:?}"),
		}
	}

	#[test]
	fn parse_tool_choice_rejects_unknown() {
		assert!(parse_tool_choice("xyz").is_err());
		assert!(parse_tool_choice("function:").is_err());
	}

	// ---------- dispatch_tool_call 周辺（State 依存のないロジックのみ） ----------

	// dispatch_tool_call 本体の async test は SharedState 構築コストが高いため
	// χ-5 の service/completion 統合後に integration test として追加する。
	// ここでは json_tool_error が declared 外呼び出しで返す形状だけ検証する。

	#[test]
	fn json_tool_error_is_valid_json_with_name() {
		let s = json_tool_error("tool not declared in openai_tools_json", "unauthorized_fn");
		let v: Value = serde_json::from_str(&s).unwrap();
		assert_eq!(v["error"], "tool not declared in openai_tools_json");
		assert_eq!(v["name"], "unauthorized_fn");
	}

	#[test]
	fn tool_output_status_reports_json_success_and_error_without_exposing_output() {
		assert_eq!(tool_output_status(r#"{"ok":true,"datum_id":42}"#), (true, None));
		assert_eq!(
			tool_output_status(r#"{"ok":false,"error":"target_link_id is required"}"#),
			(false, Some("target_link_id is required".to_string()))
		);
		assert_eq!(
			tool_output_status(r#"{"error":"tool not declared","name":"unsafe"}"#),
			(false, Some("tool not declared".to_string()))
		);
	}

	#[test]
	fn tool_name_helper() {
		assert_eq!(
			Tool::Function {
				name: "f".into(),
				description: None,
				parameters: json!({}),
				strict: None,
			}
			.name(),
			"f"
		);
		assert_eq!(
			Tool::WebSearch {
				user_location: None,
				search_context_size: None,
			}
			.name(),
			"web_search"
		);
	}

	#[test]
	fn json_err_is_valid_json() {
		let s = json_err("boom");
		let v: Value = serde_json::from_str(&s).unwrap();
		assert_eq!(v["ok"], false);
		assert_eq!(v["error"], "boom");
	}
}

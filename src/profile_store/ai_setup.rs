use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table};

use super::{read_profile_manifest, validate_profile_id};
#[cfg(test)]
use super::{read_settings, PROFILES_DIR, PROFILE_CONF_FILE};

const CREDENTIAL_RELATIVE_PATH: &str = "credentials/openai-api-key.txt";
const TOOLS_RELATIVE_PATH: &str = "resources/ai-tools.json";
const AVATAR_FLOWGRAPH_FILE: &str = "avatar-reaction.flowgraph.toml";
const SAFE_TOOL_NAMES: &[&str] = &["vac_ping", "vac_emit_effect", "vac_emit_avatar_intent"];
const AVATAR_GUIDANCE_BEGIN: &str = "<!-- UNVAC_MANAGED_AVATAR_GUIDANCE_BEGIN -->";
const AVATAR_GUIDANCE_END: &str = "<!-- UNVAC_MANAGED_AVATAR_GUIDANCE_END -->";

#[derive(Debug, Clone)]
pub struct AiSetupInput {
	pub expected_revision: Option<String>,
	pub persona_id: String,
	pub display_name: String,
	pub model: String,
	pub api_key: Option<String>,
	pub custom_instructions: String,
	pub observe_triggers: Vec<String>,
	pub avatar_actions: bool,
	pub avatar_link_id: Option<String>,
	pub avatar_base_key: Option<String>,
	pub avatar_publish: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiSetupStatus {
	pub revision: String,
	pub configured: bool,
	pub enabled: bool,
	pub persona_id: Option<String>,
	pub display_name: Option<String>,
	pub model: Option<String>,
	pub custom_instructions: Option<String>,
	pub observe_triggers: Vec<String>,
	pub avatar_actions: bool,
	pub avatar_link_id: Option<String>,
	pub avatar_base_key: Option<String>,
	pub avatar_publish: bool,
	pub credential_source: String,
	pub template_id: Option<String>,
}

#[cfg(test)]
fn read_active_profile_ai_at(root: PathBuf, environment_credential: bool) -> Result<AiSetupStatus> {
	let (profile_dir, conf_path) = active_profile_paths(&root)?;
	read_profile_ai_paths(profile_dir, conf_path, environment_credential)
}

pub fn read_profile_ai(conf_path: &Path) -> Result<AiSetupStatus> {
	read_profile_ai_paths(
		conf_path.parent().context("profile directory missing")?.to_path_buf(),
		conf_path.to_path_buf(),
		environment_credential_available(),
	)
}

fn read_profile_ai_paths(profile_dir: PathBuf, conf_path: PathBuf, environment_credential: bool) -> Result<AiSetupStatus> {
	let source =
		std::fs::read_to_string(&conf_path).with_context(|| format!("active profile confを読めません: {}", conf_path.display()))?;
	let conf = toml::from_str::<crate::Conf>(&source).context("active profile confを解釈できません")?;
	let revision = ai_setup_revision(&profile_dir, &source)?;
	let persona = conf.ai.personas.first();
	let credential_source = if environment_credential {
		"environment"
	} else if let Some(path) = persona.and_then(|item| item.api_key_file.as_deref()) {
		if resolve_profile_path(&profile_dir, path).is_file() {
			"profile_file"
		} else {
			"missing"
		}
	} else if persona
		.and_then(|item| item.api_key.as_deref())
		.is_some_and(|value| !value.trim().is_empty())
	{
		"conf"
	} else {
		"missing"
	};
	let template_id = read_profile_manifest(&profile_dir).ok().and_then(|manifest| manifest.template_id);
	let avatar_link = conf
		.un_avatar
		.as_ref()
		.and_then(|un_avatar| un_avatar.links.iter().find(|(_, link)| link.has_capability("ai_control")));
	let avatar_link_id = avatar_link.map(|(id, _)| id.clone());
	let avatar_base_key = avatar_link.and_then(|(_, link)| link.unmfz.as_ref().map(|unmfz| unmfz.base_key_expr.clone()));
	let avatar_flowgraph = avatar_flowgraph_path(&profile_dir, conf.flowgraph_dir.as_deref()).ok();
	let avatar_flowgraph_state = avatar_flowgraph
		.as_deref()
		.and_then(|path| std::fs::read_to_string(path).ok())
		.and_then(|source| avatar_flowgraph_publish_state(&source));
	let tools_enabled = setup_tools_are_unmodified(&profile_dir, &source, persona.and_then(|item| item.id.as_deref()), false);
	if ai_setup_revision(&profile_dir, &source)? != revision {
		return Err(super::lifecycle::StaleRevision.into());
	}
	Ok(AiSetupStatus {
		revision,
		configured: persona.is_some(),
		enabled: persona.is_some_and(|item| item.is_enabled),
		persona_id: persona.and_then(|item| item.id.clone()),
		display_name: persona.and_then(|item| item.display_name.clone()),
		model: persona.and_then(|item| item.model.clone()),
		custom_instructions: persona.and_then(|item| item.custom_instructions.clone()),
		observe_triggers: persona.map(|item| item.observe.triggers.clone()).unwrap_or_default(),
		avatar_actions: (tools_enabled || persona.and_then(|item| item.system_instructions_extra.as_deref()).is_some_and(|text| text.contains(AVATAR_GUIDANCE_BEGIN)))
			&& avatar_link.is_some() && avatar_flowgraph_state.is_some(),
		avatar_link_id,
		avatar_base_key,
		avatar_publish: avatar_flowgraph_state.unwrap_or(false),
		credential_source: credential_source.to_string(),
		template_id,
	})
}

#[cfg(test)]
fn configure_active_profile_ai_at(root: PathBuf, input: &AiSetupInput, environment_credential: bool) -> Result<AiSetupStatus> {
	let (profile_dir, conf_path) = active_profile_paths(&root)?;
	configure_profile_ai_paths(profile_dir, conf_path, input, environment_credential)
}

pub fn configure_profile_ai(conf_path: &Path, input: &AiSetupInput) -> Result<AiSetupStatus> {
	let _operation = super::lifecycle::operation_lock(&super::user_config_root()?)?;
	configure_profile_ai_paths(
		conf_path.parent().context("profile directory missing")?.to_path_buf(),
		conf_path.to_path_buf(),
		input,
		environment_credential_available(),
	)
}

fn configure_profile_ai_paths(
	profile_dir: PathBuf,
	conf_path: PathBuf,
	input: &AiSetupInput,
	environment_credential: bool,
) -> Result<AiSetupStatus> {
	validate_input(input)?;
	let source =
		std::fs::read_to_string(&conf_path).with_context(|| format!("active profile confを読めません: {}", conf_path.display()))?;
	let current_revision = ai_setup_revision(&profile_dir, &source)?;
	if input.expected_revision.as_ref().is_some_and(|expected| expected != &current_revision) {
		return Err(super::lifecycle::StaleRevision.into());
	}
	let owns_tools = setup_tools_are_unmodified(&profile_dir, &source, Some(&input.persona_id), true);
	let mut document = source
		.parse::<DocumentMut>()
		.context("active profile confを編集形式で解釈できません")?;
	if input.avatar_actions {
		let link_id = input.avatar_link_id.as_deref().expect("validated avatar link id");
		let base_key = input.avatar_base_key.as_deref().expect("validated avatar base key");
		if document.get("flowgraph_dir").is_none() {
			document["flowgraph_dir"] = toml_edit::value("flowgraph");
		}
		upsert_avatar_link(&mut document, link_id, base_key)?;
	}
	let persona = persona_table(&mut document, &input.persona_id)?;
	persona["id"] = toml_edit::value(&input.persona_id);
	persona["display_name"] = toml_edit::value(&input.display_name);
	persona["is_enabled"] = toml_edit::value(true);
	persona["channel_utterance"] = toml_edit::value("ai");
	persona["channel_effect"] = toml_edit::value("effect");
	persona["model"] = toml_edit::value(&input.model);
	persona["custom_instructions"] = toml_edit::value(&input.custom_instructions);
	if persona.get("openai_store").is_none() {
		persona["openai_store"] = toml_edit::value(false);
	}
	if persona.get("openai_stream").is_none() {
		persona["openai_stream"] = toml_edit::value(true);
	}
	if persona.get("openai_max_output_tokens").is_none() {
		persona["openai_max_output_tokens"] = toml_edit::value(1024);
	}
	let mut turn_management = persona
		.get("turn_management")
		.and_then(Item::as_table)
		.cloned()
		.unwrap_or_else(Table::new);
	turn_management["enabled"] = toml_edit::value(true);
	persona["turn_management"] = Item::Table(turn_management);
	let mut observation_buffer = persona
		.get("observation_buffer")
		.and_then(Item::as_table)
		.cloned()
		.unwrap_or_else(Table::new);
	if observation_buffer.get("settle_ms").is_none() {
		observation_buffer["settle_ms"] = toml_edit::value(750);
	}
	if observation_buffer.get("max_wait_ms").is_none() {
		observation_buffer["max_wait_ms"] = toml_edit::value(5000);
	}
	persona["observation_buffer"] = Item::Table(observation_buffer);
	let mut observe = persona.get("observe").and_then(Item::as_table).cloned().unwrap_or_else(Table::new);
	let mut triggers = Array::new();
	for channel in &input.observe_triggers {
		triggers.push(channel.as_str());
	}
	observe["triggers"] = toml_edit::value(triggers);
	observe["include_all"] = toml_edit::value(true);
	persona["observe"] = Item::Table(observe);

	let credential_path = profile_dir.join(CREDENTIAL_RELATIVE_PATH);
	let existing_credential = credential_path.is_file();
	let inline_credential = persona.get("api_key_file").is_none()
		&& persona
			.get("api_key")
			.and_then(Item::as_str)
			.is_some_and(|value| !value.trim().is_empty());
	if input.api_key.as_deref().is_none_or(|value| value.trim().is_empty())
		&& !existing_credential
		&& !environment_credential
		&& !inline_credential
	{
		return Err(anyhow!("OpenAI API keyを入力するか、VAC_OPENAI_API_KEYを設定してください"));
	}
	let credential_contents = input
		.api_key
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.map(|value| value.as_bytes().to_vec());
	if credential_contents.is_some() {
		persona.remove("api_key");
		persona["api_key_file"] = toml_edit::value(CREDENTIAL_RELATIVE_PATH);
	} else if existing_credential {
		persona["api_key_file"] = toml_edit::value(CREDENTIAL_RELATIVE_PATH);
	}

	let mut avatar_flowgraph_write = None;
	// Keep the shared directory lock through the complete multi-file transaction.
	let mut _avatar_graph_guard = None;
	let tools_contents = if input.avatar_actions {
		let link_id = input.avatar_link_id.as_deref().expect("validated avatar link id");
		let base_key = input.avatar_base_key.as_deref().expect("validated avatar base key");
		// Structured turn avatar output does not require replacing a custom tool set.
		let uses_custom_tools = !owns_tools && (persona.get("openai_tools_json_path").is_some()
			|| persona.get("openai_tool_policy").is_some() || profile_dir.join(TOOLS_RELATIVE_PATH).exists());
		if !uses_custom_tools {
			persona["openai_tools_json_path"] = toml_edit::value(TOOLS_RELATIVE_PATH);
			let mut policy = persona
				.get("openai_tool_policy")
				.and_then(Item::as_table)
				.cloned()
				.unwrap_or_else(Table::new);
			policy["require_strict"] = toml_edit::value(true);
			let mut allowed = Array::new();
			for name in SAFE_TOOL_NAMES {
				allowed.push(*name);
			}
			policy["default_allowed"] = toml_edit::value(allowed);
			persona["openai_tool_policy"] = Item::Table(policy);
		}
		update_avatar_guidance(persona, Some(link_id));
		let conf_for_paths = toml::from_str::<crate::Conf>(&document.to_string()).context("AvatarLink追加後のconfが不正です")?;
		let flowgraph_path = avatar_flowgraph_path(&profile_dir, conf_for_paths.flowgraph_dir.as_deref())?;
		let graph_dir = flowgraph_path.parent().context("身体表現の保存先がありません")?;
		std::fs::create_dir_all(graph_dir)?;
		_avatar_graph_guard = Some(crate::flowgraph::write_lock::acquire(graph_dir)
			.context("身体表現の保存先を別の操作が変更中、または使用できません。入力を保持したまま再操作してください")?);
		// Studio may finish a save between the first revision check and this lock.
		if ai_setup_revision(&profile_dir, &source)? != current_revision {
			return Err(super::lifecycle::StaleRevision.into());
		}
		let graph_contents = match std::fs::read_to_string(&flowgraph_path) {
			Ok(existing) => update_avatar_flowgraph(&existing, base_key, input.avatar_publish)?,
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => avatar_flowgraph_source(base_key, input.avatar_publish),
			Err(error) => return Err(error).context("身体表現の既存Flowgraphを読み込めません"),
		};
		avatar_flowgraph_write = Some(PendingWrite::new(flowgraph_path, graph_contents.into_bytes(), false));
		if uses_custom_tools { None } else { Some(safe_tools_json(link_id)?.into_bytes()) }
	} else {
		// Turning off avatar setup must not detach unrelated user-defined tools.
		if owns_tools {
			persona.remove("openai_tools_json_path");
			persona.remove("openai_tool_policy");
		}
		update_avatar_guidance(persona, None);
		None
	};

	let next = document.to_string();
	toml::from_str::<crate::Conf>(&next).context("更新後のactive profile confが不正です")?;
	let mut writes = vec![PendingWrite::new(conf_path.clone(), next.into_bytes(), false)];
	if let Some(contents) = credential_contents {
		writes.push(PendingWrite::new(credential_path, contents, true));
	}
	if let Some(contents) = tools_contents {
		writes.push(PendingWrite::new(profile_dir.join(TOOLS_RELATIVE_PATH), contents, false));
	}
	if let Some(write) = avatar_flowgraph_write {
		writes.push(write);
	}
	write_files_transaction(writes)?;
	read_profile_ai_paths(profile_dir, conf_path, environment_credential)
}

// The basic editor also owns generated tools and avatar graph controls. A conf-only
// revision cannot detect edits made in Studio while this form remains open.
fn ai_setup_revision(profile_dir: &Path, source: &str) -> Result<String> {
	let conf = toml::from_str::<crate::Conf>(source).context("AI設定のrevisionを確認できません")?;
	let mut hash = blake3::Hasher::new();
	hash.update(b"unvac-ai-setup-v2\0");
	hash.update(&(source.len() as u64).to_le_bytes());
	hash.update(source.as_bytes());
	let graph = avatar_flowgraph_path(profile_dir, conf.flowgraph_dir.as_deref()).ok();
	for path in [Some(profile_dir.join(TOOLS_RELATIVE_PATH)), graph] {
		match path.map(std::fs::read).transpose() {
			Ok(Some(bytes)) => {
				hash.update(&[1]);
				hash.update(&(bytes.len() as u64).to_le_bytes());
				hash.update(&bytes);
			}
			Ok(None) => { hash.update(&[0]); }
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => { hash.update(&[0]); }
			Err(error) => return Err(error).context("AI設定に関連するファイルを確認できません"),
		}
	}
	Ok(hash.finalize().to_hex().to_string())
}

fn environment_credential_available() -> bool {
	std::env::var(crate::ai::ENV_OPENAI_API_KEY)
		.ok()
		.is_some_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
fn active_profile_paths(root: &Path) -> Result<(PathBuf, PathBuf)> {
	let settings = read_settings(&root.join("settings.toml"))?.ok_or_else(|| anyhow!("desktop settingsがありません"))?;
	validate_profile_id(&settings.active_profile_id)?;
	let profile_dir = root.join(PROFILES_DIR).join(settings.active_profile_id);
	let conf_path = profile_dir.join(PROFILE_CONF_FILE);
	if !conf_path.is_file() {
		return Err(anyhow!("active profile confがありません: {}", conf_path.display()));
	}
	Ok((profile_dir, conf_path))
}

fn resolve_profile_path(profile_dir: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		profile_dir.join(path)
	}
}

fn validate_input(input: &AiSetupInput) -> Result<()> {
	validate_profile_id(&input.persona_id).context("persona idが不正です")?;
	if input.display_name.trim().is_empty() || input.display_name.chars().count() > 80 {
		return Err(anyhow!("AI表示名は1〜80文字で指定してください"));
	}
	if input.model.trim().is_empty() || input.model.chars().count() > 120 {
		return Err(anyhow!("modelは1〜120文字で指定してください"));
	}
	if input.custom_instructions.trim().is_empty() || input.custom_instructions.chars().count() > 20_000 {
		return Err(anyhow!("AIの役割は1〜20000文字で指定してください"));
	}
	if input.observe_triggers.is_empty() || input.observe_triggers.len() > 16 {
		return Err(anyhow!("observe triggerは1〜16件で指定してください"));
	}
	for channel in &input.observe_triggers {
		if channel.is_empty() || channel.len() > 64 || !channel.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte)) {
			return Err(anyhow!("observe triggerが不正です: {channel:?}"));
		}
	}
	if input.avatar_actions {
		let link_id = input.avatar_link_id.as_deref().map(str::trim).unwrap_or_default();
		if link_id.is_empty()
			|| link_id.len() > 64
			|| !link_id
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
		{
			return Err(anyhow!("AvatarLink IDが不正です"));
		}
		let base_key = input.avatar_base_key.as_deref().map(str::trim).unwrap_or_default();
		if base_key.is_empty()
			|| base_key.len() > 240
			|| !base_key
				.bytes()
				.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'.'))
		{
			return Err(anyhow!("UNMF/Z base keyが不正です"));
		}
	}
	Ok(())
}

fn persona_table<'a>(document: &'a mut DocumentMut, persona_id: &str) -> Result<&'a mut Table> {
	if document.get("ai").is_none() {
		document["ai"] = Item::Table(Table::new());
	}
	let ai = document["ai"].as_table_mut().ok_or_else(|| anyhow!("[ai]がtableではありません"))?;
	if ai.get("personas").is_none() {
		ai["personas"] = Item::ArrayOfTables(ArrayOfTables::new());
	}
	let personas = ai["personas"]
		.as_array_of_tables_mut()
		.ok_or_else(|| anyhow!("[[ai.personas]]がarray of tablesではありません"))?;
	let existing_index = {
		personas
			.iter()
			.position(|table| table.get("id").and_then(Item::as_str) == Some(persona_id))
	};
	if let Some(index) = existing_index {
		return personas.get_mut(index).ok_or_else(|| anyhow!("AI personaを取得できません"));
	}
	let mut table = Table::new();
	table.set_implicit(false);
	personas.push(table);
	personas.iter_mut().last().ok_or_else(|| anyhow!("AI personaを追加できません"))
}

// Only definitions and policy still equal to our generated values are ours to replace.
// A shared filename alone is not proof of ownership; preserve user changes and orphan files.
fn setup_tools_are_unmodified(profile_dir: &Path, source: &str, persona_id: Option<&str>, allow_unreferenced: bool) -> bool {
	let Ok(conf) = toml::from_str::<toml::Value>(source) else { return false; };
	let Some(personas) = conf.get("ai").and_then(|v| v.get("personas")).and_then(toml::Value::as_array) else { return false; };
	let Some(persona) = personas.iter().find(|p| persona_id.is_none_or(|id| p.get("id").and_then(toml::Value::as_str) == Some(id))) else { return false; };
	let unreferenced = allow_unreferenced && persona.get("openai_tools_json_path").is_none() && persona.get("openai_tool_policy").is_none();
	if !unreferenced && persona.get("openai_tools_json_path").and_then(toml::Value::as_str) != Some(TOOLS_RELATIVE_PATH) { return false; }
	if !unreferenced {
	let Some(policy) = persona.get("openai_tool_policy").and_then(toml::Value::as_table) else { return false; };
	if policy.len() != 2 || policy.get("require_strict").and_then(toml::Value::as_bool) != Some(true) { return false; }
	let Some(allowed) = policy.get("default_allowed").and_then(toml::Value::as_array) else { return false; };
	if allowed.len() != SAFE_TOOL_NAMES.len() || !SAFE_TOOL_NAMES.iter().all(|name| allowed.iter().any(|v| v.as_str() == Some(name))) { return false; }
	}
	let Ok(bytes) = std::fs::read(profile_dir.join(TOOLS_RELATIVE_PATH)) else { return false; };
	let Ok(actual) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return false; };
	let Some(links) = conf.get("un_avatar").and_then(|v| v.get("links")).and_then(toml::Value::as_table) else { return false; };
	links.keys().any(|id| safe_tools_json(id).ok().and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok()).is_some_and(|expected| expected == actual))
}

fn safe_tools_json(link_id: &str) -> Result<String> {
	let source = include_str!("../../conf.example-openai-tools.json").replace("kaltsit_ai", link_id);
	let mut value: serde_json::Value = serde_json::from_str(&source)?;
	let entries = value
		.as_array_mut()
		.ok_or_else(|| anyhow!("embedded OpenAI toolsがarrayではありません"))?;
	entries.retain(|entry| {
		entry
			.get("function")
			.and_then(|function| function.get("name"))
			.and_then(serde_json::Value::as_str)
			.is_some_and(|name| SAFE_TOOL_NAMES.contains(&name))
	});
	serde_json::to_string_pretty(&value).context("safe OpenAI toolsをJSON化できません")
}

fn update_avatar_guidance(persona: &mut Table, link_id: Option<&str>) {
	let current = persona.get("system_instructions_extra").and_then(Item::as_str).unwrap_or_default();
	let without_managed = remove_managed_guidance(current).trim().to_string();
	let next = if let Some(link_id) = link_id {
		let guidance = format!(
			"{AVATAR_GUIDANCE_BEGIN}\nTurn Managerのavatar_intentで身体反応を返し、target_link_idは{link_id}とする。observeでは必ずnull。中立的なrespondでは原則null。強い感情をreactとして発話する場合は意味に合う小さなexpressionを1個返し、明示的な合意をcloseする場合は小さなhead_nodを1個使ってよい。1回につき1〜2個の小さなexpression、head_nod、head_shake、look_atに限り、沈黙を動作で埋めず、不要ならnullとする。\n{AVATAR_GUIDANCE_END}"
		);
		if without_managed.is_empty() {
			guidance
		} else {
			format!("{without_managed}\n\n{guidance}")
		}
	} else {
		without_managed
	};
	if next.is_empty() {
		persona.remove("system_instructions_extra");
	} else {
		persona["system_instructions_extra"] = toml_edit::value(next);
	}
}

fn upsert_avatar_link(document: &mut DocumentMut, link_id: &str, base_key: &str) -> Result<()> {
	if document.get("un_avatar").is_none() {
		document["un_avatar"] = Item::Table(Table::new());
	}
	let un_avatar = document["un_avatar"]
		.as_table_mut()
		.ok_or_else(|| anyhow!("[un_avatar]がtableではありません"))?;
	if un_avatar.get("links").is_none() {
		un_avatar["links"] = Item::Table(Table::new());
	}
	let links = un_avatar["links"]
		.as_table_mut()
		.ok_or_else(|| anyhow!("[un_avatar.links]がtableではありません"))?;
	if links.get(link_id).is_none() {
		links[link_id] = Item::Table(Table::new());
	}
	let link = links[link_id]
		.as_table_mut()
		.ok_or_else(|| anyhow!("AvatarLinkがtableではありません"))?;
	let mut capabilities = link.get("capabilities").and_then(Item::as_array).cloned().unwrap_or_default();
	for required in ["reference", "ai_control"] {
		if !capabilities.iter().any(|value| value.as_str() == Some(required)) {
			capabilities.push(required);
		}
	}
	link["capabilities"] = toml_edit::value(capabilities);
	let mut unmfz = link.get("unmfz").and_then(Item::as_table).cloned().unwrap_or_else(Table::new);
	unmfz["base_key_expr"] = toml_edit::value(base_key);
	unmfz["topic_mode"] = toml_edit::value("frame");
	unmfz["schema_major"] = toml_edit::value(1);
	link["unmfz"] = Item::Table(unmfz);
	Ok(())
}

fn avatar_flowgraph_path(profile_dir: &Path, configured_dir: Option<&Path>) -> Result<PathBuf> {
	let configured_dir = configured_dir.ok_or_else(|| anyhow!("アバター動作にはflowgraph_dirが必要です"))?;
	if configured_dir.as_os_str().is_empty()
		|| configured_dir.is_absolute()
		|| !configured_dir
			.components()
			.all(|component| matches!(component, Component::Normal(_)))
	{
		return Err(anyhow!("managed profileのflowgraph_dirはprofile内の相対pathで指定してください"));
	}
	Ok(profile_dir.join(configured_dir).join(AVATAR_FLOWGRAPH_FILE))
}

fn avatar_flowgraph_source(base_key: &str, publish: bool) -> String {
	include_str!("../../profile.template/streaming-companion/avatar-reaction.flowgraph.toml")
		.replace(
			"properties.value = \"un-motion/frame/ai-avatar\"",
			&format!("properties.value = \"{base_key}\""),
		)
		.replace("properties.value = true", &format!("properties.value = {}", !publish))
}

fn update_avatar_flowgraph(source: &str, base_key: &str, publish: bool) -> Result<String> {
	let mut graph = source.parse::<DocumentMut>().context("身体表現のFlowgraphが不正です。Studioで確認してください")?;
	let error = || anyhow!("身体表現のFlowgraphの接続が変更されています。既存の構成は保持しました。Studioで送信先とdry_runを確認してください");
	let nodes = graph.get("nodes").and_then(Item::as_array_of_tables).ok_or_else(error)?;
	let players: Vec<_> = nodes.iter().filter(|node| node.get("id").and_then(Item::as_str) == Some("play_intent")).collect();
	if players.len() != 1 || players[0].get("feature").and_then(Item::as_str) != Some("flowgraph.avatar.intent_play_unmfz") { return Err(error()); }
	let mut indices = Vec::new();
	for (id, feature, target) in [("unmfz_base_key", "flowgraph.literal.string", "play_intent:base_key_expr"), ("dry_run", "flowgraph.literal.bool", "play_intent:dry_run")] {
		let matches: Vec<_> = nodes.iter().enumerate().filter(|(_, node)| node.get("id").and_then(Item::as_str) == Some(id)).collect();
		if matches.len() != 1 || matches[0].1.get("feature").and_then(Item::as_str) != Some(feature) { return Err(error()); }
		let value = matches[0].1.get("properties").and_then(|p| p.get("value")).ok_or_else(error)?;
		if (id == "dry_run" && value.as_bool().is_none()) || (id != "dry_run" && value.as_str().is_none()) { return Err(error()); }
		let unchanged = if id == "dry_run" { value.as_bool() == Some(!publish) } else { value.as_str() == Some(base_key) };
		if unchanged { continue; }
		let edges = graph.get("edges").and_then(Item::as_array_of_tables).ok_or_else(error)?;
		let incoming: Vec<_> = edges.iter().filter(|edge| edge.get("to").and_then(Item::as_str) == Some(target)).collect();
		if incoming.len() != 1 || incoming[0].get("from").and_then(Item::as_str) != Some(format!("{id}:value").as_str()) { return Err(error()); }
		indices.push((matches[0].0, if id == "dry_run" { toml_edit::Value::from(!publish) } else { toml_edit::Value::from(base_key) }));
	}
	if indices.is_empty() { return Ok(source.to_owned()); }
	let nodes = graph["nodes"].as_array_of_tables_mut().ok_or_else(error)?;
	for (index, mut value) in indices {
		let slot = &mut nodes.get_mut(index).ok_or_else(error)?["properties"]["value"];
		if let Some(old) = slot.as_value() { *value.decor_mut() = old.decor().clone(); }
		*slot = Item::Value(value);
	}
	Ok(graph.to_string())
}

fn avatar_flowgraph_publish_state(source: &str) -> Option<bool> {
	let root: toml::Value = toml::from_str(source).ok()?;
	let nodes = root.get("nodes")?.as_array()?;
	if !nodes
		.iter()
		.any(|node| node.get("feature").and_then(toml::Value::as_str) == Some("flowgraph.avatar.intent_play_unmfz"))
	{
		return None;
	}
	let dry_run = nodes
		.iter()
		.find(|node| node.get("id").and_then(toml::Value::as_str) == Some("dry_run"))?;
	Some(!dry_run.get("properties")?.get("value")?.as_bool()?)
}

fn remove_managed_guidance(value: &str) -> String {
	let Some(start) = value.find(AVATAR_GUIDANCE_BEGIN) else {
		return value.to_string();
	};
	let Some(relative_end) = value[start..].find(AVATAR_GUIDANCE_END) else {
		return value.to_string();
	};
	let end = start + relative_end + AVATAR_GUIDANCE_END.len();
	format!("{}{}", &value[..start], &value[end..])
}

struct PendingWrite {
	target: PathBuf,
	contents: Vec<u8>,
	private: bool,
}

impl PendingWrite {
	fn new(target: PathBuf, contents: Vec<u8>, private: bool) -> Self {
		Self { target, contents, private }
	}
}

struct PreparedWrite {
	target: PathBuf,
	temporary: PathBuf,
	previous: PathBuf,
	had_previous: bool,
	promoted: bool,
}

pub(crate) fn write_config_transaction(path: &Path, contents: &[u8]) -> Result<()> {
    write_config_replacement(path, contents, |_| Ok(()))
}

/// For a single configuration, keep the original pathname present until the
/// complete staged file replaces it. Multi-file AI installation below retains
/// its separate rollback protocol; this is not a multi-file atomic commit.
fn write_config_replacement(path: &Path, contents: &[u8], checkpoint: impl Fn(bool) -> Result<()>) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| anyhow!("設定ファイル名が不正です"))?;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let temporary = parent.join(format!(".{name}.unvac-tmp-{}-{nonce}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).context("設定の一時ファイルを作成できません")?;
    struct Temporary(PathBuf);
    impl Drop for Temporary { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }
    let _temporary = Temporary(temporary.clone());
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    checkpoint(false)?;
    std::fs::rename(&temporary, path).context("設定を置換できません。元の設定は退避していません")?;
    checkpoint(true)?;
    Ok(())
}

fn write_files_transaction(writes: Vec<PendingWrite>) -> Result<()> {
	let nanos = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_nanos();
	let nonce = format!("{}-{nanos}", std::process::id());
	let mut prepared = Vec::with_capacity(writes.len());
	for write in writes {
		if let Some(parent) = write.target.parent() {
			if let Err(error) = std::fs::create_dir_all(parent) {
				cleanup_temporaries(&prepared);
				return Err(error).with_context(|| format!("directoryを作成できません: {}", parent.display()));
			}
		}
		let filename = write.target.file_name().and_then(|value| value.to_str()).unwrap_or("file");
		let temporary = write.target.with_file_name(format!(".{filename}.unvac-tmp-{nonce}"));
		let previous = write.target.with_file_name(format!(".{filename}.unvac-previous-{nonce}"));
		if let Err(error) = write_staged_file(&temporary, &write.contents, write.private) {
			let _ = std::fs::remove_file(&temporary);
			cleanup_temporaries(&prepared);
			return Err(error);
		}
		prepared.push(PreparedWrite {
			target: write.target,
			temporary,
			previous,
			had_previous: false,
			promoted: false,
		});
	}

	for index in 0..prepared.len() {
		if prepared[index].previous.exists() {
			if let Err(error) = std::fs::remove_file(&prepared[index].previous) {
				rollback_prepared(&mut prepared);
				return Err(error).context("古いtransaction backupを削除できません");
			}
		}
		if prepared[index].target.exists() {
			if let Err(error) = std::fs::rename(&prepared[index].target, &prepared[index].previous) {
				rollback_prepared(&mut prepared);
				return Err(error).with_context(|| format!("既存fileを退避できません: {}", prepared[index].target.display()));
			}
			prepared[index].had_previous = true;
		}
	}

	for index in 0..prepared.len() {
		if let Err(error) = std::fs::rename(&prepared[index].temporary, &prepared[index].target) {
			rollback_prepared(&mut prepared);
			return Err(error).with_context(|| format!("更新fileを確定できません: {}", prepared[index].target.display()));
		}
		prepared[index].promoted = true;
	}
	for write in prepared {
		if write.had_previous {
			let _ = std::fs::remove_file(write.previous);
		}
	}
	Ok(())
}

fn write_staged_file(path: &Path, contents: &[u8], private: bool) -> Result<()> {
	let mut options = std::fs::OpenOptions::new();
	options.write(true).create(true).truncate(true);
	#[cfg(unix)]
	if private {
		use std::os::unix::fs::OpenOptionsExt;
		options.mode(0o600);
	}
	let _ = private;
	use std::io::Write;
	let mut file = options
		.open(path)
		.with_context(|| format!("一時fileを書けません: {}", path.display()))?;
	file.write_all(contents)?;
	file.sync_all()?;
	Ok(())
}

fn cleanup_temporaries(prepared: &[PreparedWrite]) {
	for write in prepared {
		let _ = std::fs::remove_file(&write.temporary);
	}
}

fn rollback_prepared(prepared: &mut [PreparedWrite]) {
	for write in prepared.iter_mut().rev() {
		if write.promoted {
			let _ = std::fs::remove_file(&write.target);
			write.promoted = false;
		}
		if write.had_previous {
			let _ = std::fs::rename(&write.previous, &write.target);
			write.had_previous = false;
		}
		let _ = std::fs::remove_file(&write.temporary);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::profile_store::{ensure_active_profile_at, write_settings, DesktopSettings};
	use std::time::{SystemTime, UNIX_EPOCH};

	fn root(label: &str) -> PathBuf {
		let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		std::env::temp_dir().join(format!("unvac-ai-setup-{label}-{}-{nonce}", std::process::id()))
	}

    #[test]
    fn single_config_replacement_preserves_original_on_prepublication_error() {
        let root=root("single-config-error");std::fs::create_dir(&root).unwrap();
        let conf=root.join("conf.toml");std::fs::write(&conf,"workers=1\n").unwrap();
        assert!(write_config_replacement(&conf,b"workers=2\n",|published| {
            assert!(!published);assert_eq!(std::fs::read_to_string(&conf).unwrap(),"workers=1\n");
            Err(anyhow!("injected before replacement"))
        }).is_err());
        assert_eq!(std::fs::read_to_string(&conf).unwrap(),"workers=1\n");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(),1);
        write_config_transaction(&conf,b"workers=2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&conf).unwrap(),"workers=2\n");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(),1);
        std::fs::remove_file(conf).unwrap();std::fs::remove_dir(root).unwrap();
    }

    #[test]
    #[ignore = "child entry; requires parent-owned isolated fixture"]
    fn single_config_interruption_child() {
        let path=PathBuf::from(std::env::var_os("UNVAC_CONFIG_INTERRUPT_FIXTURE").expect("parent fixture required"));
        let root=path.parent().unwrap().canonicalize().unwrap();
        assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("unvac-ai-setup-single-config-interrupt-"));
        assert_eq!(path.file_name().unwrap(),"conf.toml");
        let after=std::env::var("UNVAC_CONFIG_INTERRUPT_AFTER").unwrap()=="1";
        write_config_replacement(&path,b"workers=2\n",|published| {
            if published==after {std::process::exit(81);}
            Ok(())
        }).unwrap();
        panic!("interruption checkpoint not reached");
    }

    #[test]
    fn single_config_replacement_survives_process_exit_before_and_after_publication() {
        for after in [false,true] {
            let root=root("single-config-interrupt");std::fs::create_dir(&root).unwrap();
            let conf=root.join("conf.toml");std::fs::write(&conf,"workers=1\n").unwrap();
            let mut child=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","profile_store::ai_setup::tests::single_config_interruption_child","--ignored"])
                .env("UNVAC_CONFIG_INTERRUPT_FIXTURE",&conf).env("UNVAC_CONFIG_INTERRUPT_AFTER",if after {"1"} else {"0"})
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
            let status=loop {
                if let Some(status)=child.try_wait().unwrap(){break status;}
                if std::time::Instant::now()>=deadline {let _=child.kill();let _=child.wait();panic!("owned child exceeded deadline");}
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            assert_eq!(status.code(),Some(81));
            assert_eq!(std::fs::read_to_string(&conf).unwrap(),if after {"workers=2\n"} else {"workers=1\n"});
            // A future save does not mistake an interrupted staging file for
            // the current configuration, nor remove unrelated staged evidence.
            write_config_transaction(&conf,b"workers=3\n").unwrap();
            assert_eq!(std::fs::read_to_string(conf).unwrap(),"workers=3\n");
            let resolved=root.canonicalize().unwrap();
            assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
            assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("unvac-ai-setup-single-config-interrupt-"));
            std::fs::remove_dir_all(resolved).unwrap();
        }
    }

	fn input() -> AiSetupInput {
		AiSetupInput {
			expected_revision: None,
			persona_id: "companion".into(),
			display_name: "AIパートナー".into(),
			model: "test-model".into(),
			api_key: Some("secret-value".into()),
			custom_instructions: "配信を支援する。".into(),
			observe_triggers: vec!["user".into(), "chat".into()],
			avatar_actions: true,
			avatar_link_id: Some("companion_avatar".into()),
			avatar_base_key: Some("un-motion/frame/companion-avatar".into()),
			avatar_publish: false,
		}
	}

	#[test]
	fn explicit_profile_ai_write_does_not_follow_changed_launch_default() {
		let root = root("explicit-target");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let current_dir = root.join("profiles/default");
		let current_conf = current_dir.join("conf.toml");
		let next_dir = root.join("profiles/next");
		std::fs::create_dir_all(&next_dir).unwrap();
		let next_conf = next_dir.join("conf.toml");
		std::fs::write(&next_conf, "workers = 2\n").unwrap();
		write_settings(
			&root.join("settings.toml"),
			&DesktopSettings {
				schema_version: 1,
				active_profile_id: "next".into(),
				onboarding_completed: true,
				voice_input_muted: false,
			},
		)
		.unwrap();
		let status = configure_profile_ai_paths(current_dir.clone(), current_conf.clone(), &input(), false).unwrap();
		assert_eq!(status.persona_id.as_deref(), Some("companion"));
		assert!(read_profile_ai_paths(current_dir, current_conf, false).unwrap().configured);
		assert!(!read_active_profile_ai_at(root.clone(), false).unwrap().configured);
		assert_eq!(std::fs::read_to_string(next_conf).unwrap(), "workers = 2\n");
		assert!(!next_dir.join(CREDENTIAL_RELATIVE_PATH).exists());
		assert_eq!(
			read_settings(&root.join("settings.toml")).unwrap().unwrap().active_profile_id,
			"next"
		);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn configures_persona_without_putting_secret_in_conf() {
		let root = root("configure");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		write_settings(
			&root.join("settings.toml"),
			&DesktopSettings {
				schema_version: 1,
				active_profile_id: "default".into(),
				onboarding_completed: true,
				voice_input_muted: false,
			},
		)
		.unwrap();
		let status = configure_active_profile_ai_at(root.clone(), &input(), false).unwrap();
		assert!(status.configured);
		assert_eq!(status.credential_source, "profile_file");
		let conf = std::fs::read_to_string(root.join("profiles/default/conf.toml")).unwrap();
		assert!(!conf.contains("secret-value"));
		assert!(conf.contains("api_key_file = \"credentials/openai-api-key.txt\""));
		let parsed = toml::from_str::<crate::Conf>(&conf).unwrap();
		let persona = parsed.ai.personas.first().unwrap();
		assert!(persona.turn_management.as_ref().is_some_and(|value| value.enabled));
		let observation_buffer = persona.observation_buffer.as_ref().unwrap();
		assert_eq!(observation_buffer.settle_ms, 750);
		assert_eq!(observation_buffer.max_wait_ms, 5000);
		assert!(conf.contains("target_link_idはcompanion_avatar"));
		assert!(conf.contains("Turn Managerのavatar_intent"));
		assert!(conf.contains("observeでは必ずnull"));
		assert!(conf.contains("強い感情をreactとして発話する場合"));
		assert!(conf.contains("base_key_expr = \"un-motion/frame/companion-avatar\""));
		let key = std::fs::read_to_string(root.join("profiles/default/credentials/openai-api-key.txt")).unwrap();
		assert_eq!(key, "secret-value");
		let tools: serde_json::Value =
			serde_json::from_str(&std::fs::read_to_string(root.join("profiles/default/resources/ai-tools.json")).unwrap()).unwrap();
		assert_eq!(tools.as_array().unwrap().len(), SAFE_TOOL_NAMES.len());
		let tools_text = tools.to_string();
		assert!(tools_text.contains("companion_avatar"));
		assert!(!tools_text.contains("kaltsit_ai"));
		let flowgraph = std::fs::read_to_string(root.join("profiles/default/flowgraph/avatar-reaction.flowgraph.toml")).unwrap();
		assert!(flowgraph.contains("flowgraph.avatar.intent_play_unmfz"));
		assert!(flowgraph.contains("properties.value = \"un-motion/frame/companion-avatar\""));
		assert!(flowgraph.contains("properties.value = true"));
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn custom_avatar_wiring_allows_name_edit_but_rejects_ambiguous_update_atomically() {
		let root = root("custom-wiring-transaction");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let mut request = input();
		configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		let profile = root.join("profiles/default");
		let graph = profile.join("flowgraph/avatar-reaction.flowgraph.toml");
		let original = std::fs::read_to_string(&graph).unwrap();
		let custom = original.replace("from = \"dry_run:value\"", "from = \"custom_dry_run:value\"")
			+ "\n# independent publish control\n[[nodes]]\nid = \"custom_dry_run\"\nfeature = \"flowgraph.literal.bool\"\nproperties.value = true\n";
		std::fs::write(&graph, &custom).unwrap();
		request.expected_revision = Some(read_active_profile_ai_at(root.clone(), false).unwrap().revision);
		request.display_name = "Name edit with custom wiring".into();
		request.api_key = None;
		configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		assert_eq!(std::fs::read_to_string(&graph).unwrap(), custom);
		let saved: toml::Value = toml::from_str(&std::fs::read_to_string(profile.join("conf.toml")).unwrap()).unwrap();
		assert_eq!(saved["ai"]["personas"][0]["display_name"].as_str(), Some("Name edit with custom wiring"));
		let paths = [profile.join("conf.toml"), profile.join(CREDENTIAL_RELATIVE_PATH), profile.join(TOOLS_RELATIVE_PATH), graph];
		let before: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
		request.expected_revision = Some(read_active_profile_ai_at(root.clone(), false).unwrap().revision);
		request.avatar_publish = true;
		request.display_name = "Must not persist".into();
		request.api_key = Some("replacement-test-key-must-not-persist".into());
		let error = configure_active_profile_ai_at(root.clone(), &request, false).unwrap_err();
		assert!(error.to_string().contains("接続が変更"), "{error:#}");
		for (path, expected) in paths.iter().zip(before) {
			assert_eq!(std::fs::read(path).unwrap(), expected, "Changed on rejected save: {}", path.display());
		}
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn studio_graph_write_blocks_ai_save_without_partial_changes_and_can_retry() {
		let root = root("studio-lock");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let mut request = input();
		configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		let profile = root.join("profiles/default");
		let paths = [profile.join("conf.toml"), profile.join(CREDENTIAL_RELATIVE_PATH), profile.join(TOOLS_RELATIVE_PATH), profile.join("flowgraph/avatar-reaction.flowgraph.toml")];
		let before: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
		request.expected_revision = Some(read_active_profile_ai_at(root.clone(), false).unwrap().revision);
		request.display_name = "Saved after Studio finishes".into();
		request.api_key = Some("isolated-new-credential".into());
		let studio = crate::flowgraph::write_lock::acquire(&profile.join("flowgraph")).unwrap();
		let error = configure_active_profile_ai_at(root.clone(), &request, false).unwrap_err();
		assert_eq!(error.downcast_ref::<std::io::Error>().unwrap().kind(), std::io::ErrorKind::WouldBlock);
		for (path, expected) in paths.iter().zip(before) { assert_eq!(std::fs::read(path).unwrap(), expected); }
		drop(studio);
		let status = configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		assert_eq!(status.display_name.as_deref(), Some("Saved after Studio finishes"));
		assert_eq!(std::fs::read_to_string(profile.join(CREDENTIAL_RELATIVE_PATH)).unwrap(), "isolated-new-credential");
		// A completed AI transaction must release the same lock Studio uses.
		drop(crate::flowgraph::write_lock::acquire(&profile.join("flowgraph")).unwrap());
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn stale_ai_form_detects_graph_and_tool_changes_without_partial_writes() {
		for change in ["graph-edit", "graph-delete", "tools-edit"] {
			let root = root(change);
			ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
			let mut request = input();
			let opened = configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
			let profile = root.join("profiles/default");
			let graph = profile.join("flowgraph/avatar-reaction.flowgraph.toml");
			let tools = profile.join(TOOLS_RELATIVE_PATH);
			match change {
				"graph-edit" => std::fs::write(&graph, std::fs::read_to_string(&graph).unwrap() + "\n# Studio edit\n").unwrap(),
				"graph-delete" => std::fs::remove_file(&graph).unwrap(),
				_ => std::fs::write(&tools, "[]\n").unwrap(),
			}
			let paths = [profile.join("conf.toml"), profile.join(CREDENTIAL_RELATIVE_PATH), tools, graph];
			let before: Vec<_> = paths.iter().map(|p| std::fs::read(p).ok()).collect();
			request.expected_revision = Some(opened.revision.clone());
			request.display_name = "Stale form must not save".into();
			request.api_key = Some("stale-replacement-key".into());
			let error = configure_active_profile_ai_at(root.clone(), &request, false).unwrap_err();
			assert!(error.is::<super::super::lifecycle::StaleRevision>(), "{change}: {error:#}");
			for (path, expected) in paths.iter().zip(before) { assert_eq!(std::fs::read(path).ok(), expected); }
			let fresh = read_active_profile_ai_at(root.clone(), false).unwrap();
			assert_ne!(fresh.revision, opened.revision);
			request.expected_revision = Some(fresh.revision);
			request.api_key = None;
			request.display_name = "Reviewed and saved".into();
			assert_eq!(configure_active_profile_ai_at(root.clone(), &request, false).unwrap().display_name.as_deref(), Some("Reviewed and saved"));
			if change == "graph-edit" { assert!(std::fs::read_to_string(&paths[3]).unwrap().contains("# Studio edit")); }
			if change == "tools-edit" { assert_eq!(std::fs::read_to_string(&paths[2]).unwrap(), "[]\n"); }
			std::fs::remove_dir_all(root).unwrap();
		}
	}

	#[test]
	fn basic_ai_save_preserves_avatar_camera_launch_and_model_bindings() {
		let root = root("avatar-capabilities");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let mut request = input();
		configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		let path = root.join("profiles/default/conf.toml");
		let mut document = std::fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
		let link = &mut document["un_avatar"]["links"]["companion_avatar"];
		let mut caps = Array::new();
		for value in ["reference", "ai_control", "camera_control", "launch"] { caps.push(value); }
		link["capabilities"] = toml_edit::value(caps);
		link["label"] = toml_edit::value("Existing camera and avatar");
		link["manifest_path"] = toml_edit::value("profiles/existing.toml");
		link["unaa_z"] = Item::Table(Table::new());
		link["unaa_z"]["app_id"] = toml_edit::value("un-avatar");
		link["unaa_z"]["instance_id"] = toml_edit::value("existing-renderer");
		link["expression_bindings"] = Item::Table(Table::new());
		link["expression_bindings"]["smile"] = Item::Table(Table::new());
		link["expression_bindings"]["smile"]["smile"] = toml_edit::value(0.8);
		std::fs::write(&path, document.to_string()).unwrap();
		let before: toml::Value = toml::from_str(&document.to_string()).unwrap();
		for name in ["First rename", "Second rename"] {
			request.expected_revision = Some(read_active_profile_ai_at(root.clone(), false).unwrap().revision);
			request.display_name = name.into();
			request.api_key = None;
			configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
			let source = std::fs::read_to_string(&path).unwrap();
			let after: toml::Value = toml::from_str(&source).unwrap();
			assert_eq!(after["un_avatar"]["links"], before["un_avatar"]["links"]);
			let conf: crate::Conf = toml::from_str(&source).unwrap();
			conf.un_avatar.unwrap().links["companion_avatar"].validate("companion_avatar").unwrap();
		}
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn basic_ai_edit_preserves_existing_response_and_buffer_options() {
		let root = root("retain-advanced");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let mut request = input();
		request.avatar_actions = false;
		configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		let path = root.join("profiles/default/conf.toml");
		let mut document = std::fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
		let persona = persona_table(&mut document, "companion").unwrap();
		persona["openai_store"] = toml_edit::value(true);
		persona["openai_stream"] = toml_edit::value(false);
		persona["openai_max_output_tokens"] = toml_edit::value(4096);
		persona["openai_reasoning_effort"] = toml_edit::value("low");
		persona["observation_buffer"]["settle_ms"] = toml_edit::value(250);
		persona["observation_buffer"]["max_wait_ms"] = toml_edit::value(1000);
		std::fs::write(&path, document.to_string()).unwrap();
		request.expected_revision = Some(read_active_profile_ai_at(root.clone(), false).unwrap().revision);
		request.display_name = "Updated name".into();
		request.api_key = None;
		configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		let parsed = std::fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
		let persona = &parsed["ai"]["personas"][0];
		assert_eq!(persona["display_name"].as_str(), Some("Updated name"));
		assert_eq!(persona["openai_store"].as_bool(), Some(true));
		assert_eq!(persona["openai_stream"].as_bool(), Some(false));
		assert_eq!(persona["openai_max_output_tokens"].as_integer(), Some(4096));
		assert_eq!(persona["openai_reasoning_effort"].as_str(), Some("low"));
		assert_eq!(persona["observation_buffer"]["settle_ms"].as_integer(), Some(250));
		assert_eq!(persona["observation_buffer"]["max_wait_ms"].as_integer(), Some(1000));
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn basic_edit_without_avatar_retains_custom_tool_reference_and_policy() {
		let root = root("custom-tools");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let mut request = input();
		request.avatar_actions = false;
		configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		let path = root.join("profiles/default/conf.toml");
		let tools = root.join("profiles/default/custom-tools.json");
		std::fs::write(&tools, "[]\n").unwrap();
		let mut document = std::fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
		let persona = persona_table(&mut document, "companion").unwrap();
		persona["openai_tools_json_path"] = toml_edit::value("custom-tools.json");
		let mut policy = Table::new();
		policy["require_strict"] = toml_edit::value(false);
		let mut allowed = Array::new(); allowed.push("my_custom_tool");
		policy["default_allowed"] = toml_edit::value(allowed);
		persona["openai_tool_policy"] = Item::Table(policy);
		std::fs::write(&path, document.to_string()).unwrap();
		request.expected_revision = Some(read_active_profile_ai_at(root.clone(), false).unwrap().revision);
		request.display_name = "Edited basic name".into();
		request.api_key = None;
		configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		let parsed = std::fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
		let persona = &parsed["ai"]["personas"][0];
		assert_eq!(persona["openai_tools_json_path"].as_str(), Some("custom-tools.json"));
		assert_eq!(persona["openai_tool_policy"]["require_strict"].as_bool(), Some(false));
		assert_eq!(persona["openai_tool_policy"]["default_allowed"][0].as_str(), Some("my_custom_tool"));
		assert_eq!(std::fs::read_to_string(&tools).unwrap(), "[]\n");
		for enabled in [true, true, false] {
			request.avatar_actions = enabled;
			request.expected_revision = Some(read_active_profile_ai_at(root.clone(), false).unwrap().revision);
			let result = configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
			assert_eq!(result.avatar_actions, enabled);
			let parsed = std::fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
			let persona = &parsed["ai"]["personas"][0];
			assert_eq!(persona["openai_tools_json_path"].as_str(), Some("custom-tools.json"));
			assert_eq!(persona["openai_tool_policy"]["require_strict"].as_bool(), Some(false));
			assert_eq!(persona["openai_tool_policy"]["default_allowed"][0].as_str(), Some("my_custom_tool"));
			assert_eq!(std::fs::read_to_string(&tools).unwrap(), "[]\n");
			assert!(!root.join("profiles/default").join(TOOLS_RELATIVE_PATH).exists());
		}
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn unchanged_generated_tools_can_be_disabled_and_reenabled() {
		let root = root("reenable-generated-tools");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let mut request = input();
		let first = configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		assert!(first.avatar_actions);
		let path = root.join("profiles/default/conf.toml");
		request.avatar_actions = false;
		request.expected_revision = Some(first.revision);
		let disabled = configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
		assert!(!disabled.avatar_actions);
		let value: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
		assert!(value["ai"]["personas"][0].get("openai_tools_json_path").is_none());
		request.avatar_actions = true;
		request.expected_revision = Some(disabled.revision);
		assert!(configure_active_profile_ai_at(root.clone(), &request, false).unwrap().avatar_actions);
		let value: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
		assert_eq!(value["ai"]["personas"][0]["openai_tools_json_path"].as_str(), Some(TOOLS_RELATIVE_PATH));
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn customized_generated_tools_and_policy_survive_avatar_toggle() {
		for edit_file in [true, false] {
			let root = root("edited-generated-tools");
			ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
			let mut request = input();
			configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
			let profile = root.join("profiles/default");
			let path = profile.join("conf.toml");
			let tools = profile.join(TOOLS_RELATIVE_PATH);
			if edit_file {
				let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&tools).unwrap()).unwrap();
				value[0]["function"]["description"] = serde_json::json!("User-authored description");
				std::fs::write(&tools, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
			} else {
				let mut document = std::fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
				persona_table(&mut document, "companion").unwrap()["openai_tool_policy"]["require_strict"] = toml_edit::value(false);
				std::fs::write(&path, document.to_string()).unwrap();
			}
			let original_tools = std::fs::read(&tools).unwrap();
			let original: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
			for enabled in [true, false, true] {
				request.avatar_actions = enabled;
				request.api_key = None;
				request.expected_revision = Some(read_active_profile_ai_at(root.clone(), false).unwrap().revision);
				let result = configure_active_profile_ai_at(root.clone(), &request, false).unwrap();
				assert_eq!(result.avatar_actions, enabled);
				assert_eq!(std::fs::read(&tools).unwrap(), original_tools);
				let saved: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
				assert_eq!(saved["ai"]["personas"][0]["openai_tool_policy"], original["ai"]["personas"][0]["openai_tool_policy"]);
				assert_eq!(saved["ai"]["personas"][0]["openai_tools_json_path"].as_str(), Some(TOOLS_RELATIVE_PATH));
			}
			std::fs::remove_dir_all(root).unwrap();
		}
	}

	#[test]
	fn rejects_missing_credential_without_changing_conf() {
		let root = root("missing-key");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let path = root.join("profiles/default/conf.toml");
		let before = std::fs::read_to_string(&path).unwrap();
		let mut input = input();
		input.api_key = None;
		assert!(configure_active_profile_ai_at(root.clone(), &input, false).is_err());
		assert_eq!(std::fs::read_to_string(path).unwrap(), before);
		std::fs::remove_dir_all(root).unwrap();
	}
	#[test]
	fn stale_ai_form_cannot_overwrite_conf_or_credentials() {
		let root = root("stale-form");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let path = root.join("profiles/default/conf.toml");
		let status = read_active_profile_ai_at(root.clone(), false).unwrap();
		let mut request = input();
		request.expected_revision = Some(status.revision);
		std::fs::write(&path, "workers = 2\n").unwrap();
		let error = configure_active_profile_ai_at(root.clone(), &request, false).unwrap_err();
		assert!(error.is::<super::super::lifecycle::StaleRevision>());
		assert_eq!(std::fs::read_to_string(&path).unwrap(), "workers = 2\n");
		assert!(!root.join("profiles/default/credentials").exists());
		request.expected_revision = Some(read_active_profile_ai_at(root.clone(), false).unwrap().revision);
		assert!(configure_active_profile_ai_at(root.clone(), &request, false).unwrap().configured);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn rejects_obstructed_resource_directory_without_partial_writes() {
		let root = root("rollback");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let profile = root.join("profiles/default");
		std::fs::write(profile.join("resources"), "blocks directory creation").unwrap();
		let conf_path = profile.join("conf.toml");
		let before = std::fs::read_to_string(&conf_path).unwrap();
		assert!(configure_active_profile_ai_at(root.clone(), &input(), false).is_err());
		assert_eq!(std::fs::read_to_string(conf_path).unwrap(), before);
		assert!(!profile.join(CREDENTIAL_RELATIVE_PATH).exists());
		assert!(!profile.join(TOOLS_RELATIVE_PATH).exists());
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn managed_avatar_guidance_preserves_user_text_and_can_be_removed() {
		let mut persona = Table::new();
		persona["system_instructions_extra"] = toml_edit::value("ユーザー固有の追加指示");
		update_avatar_guidance(&mut persona, Some("companion_avatar"));
		let enabled = persona["system_instructions_extra"].as_str().unwrap();
		assert!(enabled.contains("ユーザー固有の追加指示"));
		assert!(enabled.contains(AVATAR_GUIDANCE_BEGIN));
		assert!(enabled.contains("Turn Managerのavatar_intent"));
		assert!(enabled.contains("observeでは必ずnull"));
		update_avatar_guidance(&mut persona, None);
		assert_eq!(persona["system_instructions_extra"].as_str(), Some("ユーザー固有の追加指示"));
	}

	#[test]
	fn avatar_graph_update_preserves_custom_nodes_comments_and_timing() {
		let original = avatar_flowgraph_source("old/key", false).replace("properties.value = 30", "properties.value = 60 # user frame rate");
		let original = format!("{original}\n# user extension\n[[nodes]]\nid = \"user_node\"\nfeature = \"flowgraph.literal.string\"\nproperties.value = \"keep me\"\n");
		let updated = update_avatar_flowgraph(&original, "new/key", true).unwrap();
		assert!(updated.contains("60 # user frame rate"));
		assert!(updated.contains("# user extension"));
		let parsed: toml::Value = toml::from_str(&updated).unwrap();
		let nodes = parsed["nodes"].as_array().unwrap();
		assert_eq!(nodes.iter().find(|n| n["id"].as_str() == Some("user_node")).unwrap()["properties"]["value"].as_str(), Some("keep me"));
		assert_eq!(nodes.iter().find(|n| n["id"].as_str() == Some("unmfz_base_key")).unwrap()["properties"]["value"].as_str(), Some("new/key"));
		assert_eq!(avatar_flowgraph_publish_state(&updated), Some(true));
		let rewired = original.replace("from = \"dry_run:value\"", "from = \"user_node:value\"");
		assert!(update_avatar_flowgraph(&rewired, "new/key", true).is_err());
		assert_eq!(update_avatar_flowgraph(&rewired, "old/key", false).unwrap(), rewired);
		let destination_only = update_avatar_flowgraph(&rewired, "new/key", false).unwrap();
		assert!(destination_only.contains("from = \"user_node:value\""));
		assert!(destination_only.contains("properties.value = \"new/key\""));
		assert_eq!(avatar_flowgraph_publish_state(&destination_only), Some(false));
	}

	#[test]
	fn managed_avatar_flowgraph_reports_publish_state_only_for_timed_player() {
		let dry_run = avatar_flowgraph_source("un-motion/frame/test", false);
		assert_eq!(avatar_flowgraph_publish_state(&dry_run), Some(false));
		let publishing = avatar_flowgraph_source("un-motion/frame/test", true);
		assert_eq!(avatar_flowgraph_publish_state(&publishing), Some(true));
		assert_eq!(
			avatar_flowgraph_publish_state("[[nodes]]\nid = \"dry_run\"\nfeature = \"flowgraph.literal.bool\"\nproperties.value = false\n"),
			None
		);
	}
}

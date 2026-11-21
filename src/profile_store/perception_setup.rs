//! Configuration editing for the pending perception UI. No capture or apply.
//! Pure preparation is separate from locked, transactional file publication.
use anyhow::{bail, Context, Result};
use std::path::Path;
use toml_edit::{Array, DocumentMut, Item};

#[derive(serde::Serialize)]
pub struct PerceptionView {
    pub revision: String,
    pub sources: Vec<SourceView>,
    pub archived_sources: Vec<SourceView>,
    pub history: Vec<HistoryView>,
}
#[derive(serde::Serialize)]
pub struct HistoryView {
    pub persona_index: usize,
    pub persona_id: Option<String>,
    pub display_name: Option<String>,
    pub turn_management_enabled: bool,
    pub visual_history_enabled: bool,
    pub past_screen_image_enabled: bool,
}
#[derive(serde::Serialize)]
pub struct SourceView {
    pub id: String,
    pub scene_name: String,
    pub source_name: String,
    pub enabled: bool,
    pub refresh_before_reply: bool,
    pub active_modes: Vec<String>,
    pub channel: String,
    pub shared_source_ids: Vec<String>,
    pub observers: Vec<ObserverView>,
    pub connection: ConnectionView,
}
#[derive(serde::Serialize)]
pub struct ConnectionView {
    pub server: Option<String>,
    pub secure: bool,
    pub custom_path_or_query: bool,
    pub environment_password: bool,
    pub saved_password: bool,
    pub local_obs_password: bool,
}
fn connection_view(s:&crate::stream_perception::ObsPerceptionSourceConf) -> ConnectionView {
    let url=reqwest::Url::parse(&s.url).ok();
    ConnectionView {
        server:url.as_ref().and_then(|u|u.host().map(|host|format!("{host}:{}",u.port_or_known_default().unwrap_or(4455)))),
        secure:url.as_ref().is_some_and(|u|u.scheme()=="wss"),
        custom_path_or_query:url.as_ref().is_some_and(|u|u.path()!="/" || u.query().is_some() || u.fragment().is_some() || !u.username().is_empty() || u.password().is_some()),
        environment_password:s.password_env.is_some(),saved_password:!s.password.is_empty(),local_obs_password:s.use_local_obs_config_password,
    }
}
#[derive(serde::Serialize)]
pub struct ObserverView {
    pub persona_index: usize,
    pub persona_id: Option<String>,
    pub display_name: Option<String>,
    pub enabled: bool,
    /// None means the persona lacks its required output channel. Do not
    /// present a configured observation rule as a running AI connection.
    pub rule: Option<crate::ai::observe::ObservationRule>,
}

/// Saved configuration only, not live connection/capture status. No credentials,
/// prompts, or connection URLs are serialized into the presentation model.
pub fn perception_view(source: &str) -> Result<PerceptionView> {
    let conf: crate::Conf = toml::from_str(source)?;
    conf.perception.validate()?;
    let project = |s:&crate::stream_perception::ObsPerceptionSourceConf| SourceView {
        id:s.id.clone(), scene_name:s.scene_name.clone(), source_name:s.source_name.clone(),
        enabled:s.enabled, refresh_before_reply:s.refresh_before_reply, active_modes:s.active_modes.clone(),
        channel:s.channel.clone(),
        connection:connection_view(s),
        shared_source_ids:conf.perception.obs_sources.iter().filter(|other|other.channel==s.channel).map(|other|other.id.clone()).collect(),
        observers:conf.ai.personas.iter().enumerate().map(|(index,p)|ObserverView {
            persona_index:index,persona_id:p.id.clone(),display_name:p.display_name.clone(),enabled:p.is_enabled,
            rule:p.channel_utterance.as_deref().map(|channel|crate::ai::observe::ObserveSet::from_conf(channel,&p.observe).rule_for(&s.channel)),
        }).collect(),
    };
    let sources=conf.perception.obs_sources.iter().map(&project).collect();
    let archived_sources=conf.perception.archived_obs_sources.iter().map(project).collect();
    let history=conf.ai.personas.iter().enumerate().map(|(index,p)|HistoryView {
        persona_index:index,persona_id:p.id.clone(),display_name:p.display_name.clone(),
        turn_management_enabled:p.turn_management.as_ref().is_some_and(|t|t.enabled),
        visual_history_enabled:p.turn_management.as_ref().is_some_and(|t|t.visual_history_enabled),
        past_screen_image_enabled:p.turn_management.as_ref().is_some_and(|t|t.past_screen_image_enabled),
    }).collect();
    Ok(PerceptionView {revision:blake3::hash(source.as_bytes()).to_hex().to_string(),sources,archived_sources,history})
}

/// Only explicitly edited fields are replaced. Credentials, capture dimensions,
/// channel routing, archive settings, personas and unknown fields are preserved.
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePatch {
    pub enabled: Option<bool>,
    pub refresh_before_reply: Option<bool>,
    pub scene_name: Option<String>,
    pub source_name: Option<String>,
    pub active_modes: Option<Vec<String>>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCreate {
    pub id:String,
    pub scene_name:String,
    pub source_name:String,
    pub url:String,
    pub password_env:Option<String>,
    #[serde(default)]
    pub use_local_obs_config_password:bool,
}

#[derive(serde::Deserialize)]
#[serde(tag="kind", rename_all="snake_case", deny_unknown_fields)]
pub enum CredentialReplacement {
    Environment {name:String},
    LocalObs,
    Password {value:String},
    None,
}

#[derive(serde::Deserialize)]
#[serde(tag="kind", rename_all="snake_case", deny_unknown_fields)]
pub enum PerceptionEdit {
    Source { source_id: String, patch: SourcePatch },
    Assignment { source_id: String, persona_index: usize, shared_source_ids: Vec<String>, observe: bool },
    History { persona_index: usize, visual_history_enabled: Option<bool>, past_screen_image_enabled: Option<bool> },
    Create { source: SourceCreate },
    SourceLifecycle {source_id:String, restore:bool, shared_source_ids:Vec<String>, observer_indices:Vec<usize>},
    Connection {source_id:String, url:Option<String>, credential:Option<CredentialReplacement>},
}

pub fn read_managed_perception(id: &str) -> Result<PerceptionView> {
    perception_view(&super::lifecycle::read_content(id)?.content)
}

pub fn save_managed_perception(id: &str, revision: &str, edit: &PerceptionEdit) -> Result<SourceSaveResult> {
    save_managed_at(&super::user_config_root()?,id,revision,edit)
}

fn save_managed_at(root: &Path, id: &str, revision: &str, edit: &PerceptionEdit) -> Result<SourceSaveResult> {
    super::lifecycle::edit_managed_config(root,id,|source| {
        let prepared=match edit {
            PerceptionEdit::Source {source_id,patch} => prepare_source_edit(source,revision,source_id,patch)?,
            PerceptionEdit::Assignment {source_id,persona_index,shared_source_ids,observe} =>
                prepare_assignment_edit(source,revision,*persona_index,source_id,shared_source_ids,*observe)?,
            PerceptionEdit::History {persona_index,visual_history_enabled,past_screen_image_enabled} =>
                prepare_history_edit(source,revision,*persona_index,*visual_history_enabled,*past_screen_image_enabled)?,
            PerceptionEdit::Create {source:input} => prepare_source_create(source,revision,input)?,
            PerceptionEdit::SourceLifecycle {source_id,restore,shared_source_ids,observer_indices} =>
                prepare_source_lifecycle(source,revision,source_id,*restore,shared_source_ids,observer_indices)?,
            PerceptionEdit::Connection {source_id,url,credential} =>
                prepare_connection_edit(source,revision,source_id,url.as_deref(),credential.as_ref())?,
        };
        let result=SourceSaveResult {revision:blake3::hash(prepared.as_bytes()).to_hex().to_string(),restart_required:true};
        Ok((prepared,result))
    })
}

#[derive(Debug, serde::Serialize)]
pub struct SourceSaveResult {
    pub revision: String,
    pub restart_required: bool,
}

/// `conf_path` must be resolved by the API's existing profile/path boundary;
/// never accept a browser-supplied filesystem path directly.
pub fn save_source_edit(conf_path: &Path, expected_revision: &str, source_id: &str, patch: &SourcePatch) -> Result<SourceSaveResult> {
    let _operation = super::lifecycle::lock_config_write(conf_path)?;
    // Reread while locked, not the document obtained when the form was opened.
    let source = std::fs::read_to_string(conf_path).context("設定を読み込めません")?;
    let prepared = prepare_source_edit(&source, expected_revision, source_id, patch)?;
    toml::from_str::<crate::Conf>(&prepared).context("設定全体を検証できません。保存していません")?;
    super::write_config_transaction(conf_path, prepared.as_bytes())?;
    Ok(SourceSaveResult { revision: blake3::hash(prepared.as_bytes()).to_hex().to_string(), restart_required: true })
}

/// Same resolved-path boundary as save_source_edit. Save only; no live apply.
pub fn save_assignment_edit(conf_path: &Path, expected_revision: &str, persona_index: usize,
    source_id: &str, expected_shared_ids: &[String], observe: bool) -> Result<SourceSaveResult> {
    let _operation = super::lifecycle::lock_config_write(conf_path)?;
    let source = std::fs::read_to_string(conf_path).context("設定を読み込めません")?;
    let prepared = prepare_assignment_edit(&source, expected_revision, persona_index, source_id, expected_shared_ids, observe)?;
    if prepared != source { super::write_config_transaction(conf_path,prepared.as_bytes())?; }
    Ok(SourceSaveResult { revision:blake3::hash(prepared.as_bytes()).to_hex().to_string(),restart_required:true })
}

pub fn prepare_source_edit(source: &str, expected_revision: &str, source_id: &str, patch: &SourcePatch) -> Result<String> {
    if expected_revision != blake3::hash(source.as_bytes()).to_hex().as_str() {
        return Err(super::lifecycle::StaleRevision.into());
    }
    if source_id.trim().is_empty() { bail!("取得元IDが必要です"); }
    let mut document: DocumentMut = source.parse().context("設定を解析できません")?;
    // Validate before locating the target, so duplicate IDs cannot select an
    // arbitrary source. Validation is local and performs no OBS connection.
    validate(&document.to_string())?;
    let sources = document.get_mut("perception").and_then(Item::as_table_mut)
        .and_then(|p| p.get_mut("obs_sources")).and_then(Item::as_array_of_tables_mut)
        .context("編集する取得元がありません")?;
    let target = sources.iter_mut().find(|table| table.get("id").and_then(Item::as_str) == Some(source_id))
        .context("指定した取得元がありません。別の取得元へは保存しません")?;
    if let Some(value) = patch.enabled { target["enabled"] = toml_edit::value(value); }
    if let Some(value) = patch.refresh_before_reply { target["refresh_before_reply"] = toml_edit::value(value); }
    if let Some(value) = &patch.scene_name { target["scene_name"] = toml_edit::value(value); }
    if let Some(value) = &patch.source_name { target["source_name"] = toml_edit::value(value); }
    if let Some(values) = &patch.active_modes {
        if values.iter().any(|v| v.trim().is_empty()) { bail!("空のモードIDは指定できません。全モードは空配列で指定します"); }
        let mut array = Array::new();
        for value in values { array.push(value.as_str()); }
        target["active_modes"] = toml_edit::value(array);
    }
    let prepared = document.to_string();
    validate(&prepared)?;
    Ok(prepared)
}

/// Change observation for a complete shared-channel group, never trigger policy.
/// The index is scoped to the exact document revision, including unnamed personas.
pub fn prepare_assignment_edit(source: &str, expected_revision: &str, persona_index: usize,
    source_id: &str, expected_shared_ids: &[String], observe: bool) -> Result<String> {
    if expected_revision != blake3::hash(source.as_bytes()).to_hex().as_str() {
        return Err(super::lifecycle::StaleRevision.into());
    }
    let conf: crate::Conf = toml::from_str(source)?;
    conf.perception.validate()?;
    let capture = conf.perception.obs_sources.iter().find(|s|s.id == source_id)
        .context("指定した取得元がありません")?;
    let mut actual: Vec<_> = conf.perception.obs_sources.iter().filter(|s|s.channel == capture.channel).map(|s|s.id.clone()).collect();
    actual.sort();
    let mut expected = expected_shared_ids.to_vec(); expected.sort();
    if actual != expected { bail!("共有する取得元の範囲が一致しません。変更していません"); }
    let persona = conf.ai.personas.get(persona_index).context("指定した共演者がありません")?;
    let utterance = persona.channel_utterance.as_deref().context("共演者の出力チャンネルを先に設定してください")?;
    let mut policy = crate::ai::observe::ObserveSet::from_conf(utterance,&persona.observe);
    if policy.is_observed(&capture.channel) == observe { return Ok(source.to_owned()); }
    let channel = &capture.channel;
    let mut document: DocumentMut = source.parse()?;
    let target = document.get_mut("ai").and_then(Item::as_table_mut)
        .and_then(|ai|ai.get_mut("personas")).and_then(Item::as_array_of_tables_mut)
        .and_then(|items|items.get_mut(persona_index)).context("共演者の設定形式を編集できません")?;
    if !target.contains_key("observe") { target["observe"] = Item::Table(toml_edit::Table::new()); }
    let table = target.get_mut("observe").and_then(Item::as_table_like_mut).context("観測設定を編集できません")?;
    if observe { policy.exclude.retain(|c|c != channel); }
    else if !policy.exclude.contains(channel) { policy.exclude.push(channel.clone()); }
    let mut exclusions = Array::new();
    for c in &policy.exclude { exclusions.push(c.as_str()); }
    table.insert("exclude",toml_edit::value(exclusions));
    // Existing triggers / include_all / own-channel inclusion already suffice.
    // Add a context-only channel only if removing the exclusion did not suffice.
    if observe && !policy.is_observed(channel) {
        let mut additional = Array::new();
        for c in &policy.include_additional { additional.push(c.as_str()); }
        additional.push(channel.as_str());
        table.insert("include_additional",toml_edit::value(additional));
    }
    let prepared = document.to_string();
    let result: crate::Conf = toml::from_str(&prepared)?;
    let updated = &result.ai.personas[persona_index];
    let effective = crate::ai::observe::ObserveSet::from_conf(utterance,&updated.observe);
    if effective.is_observed(channel) != observe { bail!("観測設定の検証に失敗しました"); }
    Ok(prepared)
}

pub fn prepare_history_edit(source:&str, revision:&str, persona_index:usize, notes:Option<bool>, image:Option<bool>) -> Result<String> {
    if revision != blake3::hash(source.as_bytes()).to_hex().as_str() {return Err(super::lifecycle::StaleRevision.into());}
    let conf:crate::Conf=toml::from_str(source)?;
    conf.ai.personas.get(persona_index).context("指定した共演者がありません")?;
    if notes.is_none() && image.is_none() {return Ok(source.to_owned());}
    let mut document:DocumentMut=source.parse()?;
    let persona=document.get_mut("ai").and_then(Item::as_table_mut)
        .and_then(|ai|ai.get_mut("personas")).and_then(Item::as_array_of_tables_mut)
        .and_then(|items|items.get_mut(persona_index)).context("共演者の設定形式を編集できません")?;
    if !persona.contains_key("turn_management") {persona["turn_management"]=Item::Table(toml_edit::Table::new());}
    let table=persona.get_mut("turn_management").and_then(Item::as_table_like_mut).context("履歴設定を編集できません")?;
    if let Some(value)=notes {table.insert("visual_history_enabled",toml_edit::value(value));}
    if let Some(value)=image {table.insert("past_screen_image_enabled",toml_edit::value(value));}
    let prepared=document.to_string();
    toml::from_str::<crate::Conf>(&prepared)?;
    Ok(prepared)
}

fn validate_connection_url(value:&str) -> Result<()> {
    let url=reqwest::Url::parse(value).context("OBS接続先URLが不正です")?;
    if !matches!(url.scheme(),"ws"|"wss") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        bail!("OBS接続先は認証情報を含まないwsまたはwss URLで指定してください");
    }
    Ok(())
}

fn validate_credential_environment(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.contains(['=', '\0']) {
        bail!("有効な環境変数名を指定してください");
    }
    Ok(())
}

/// None means retain every existing credential/fallback byte. An explicit
/// replacement removes all old fallbacks so stale credentials cannot win.
pub fn prepare_connection_edit(source:&str,revision:&str,source_id:&str,url:Option<&str>,credential:Option<&CredentialReplacement>) -> Result<String> {
    if revision != blake3::hash(source.as_bytes()).to_hex().as_str() {return Err(super::lifecycle::StaleRevision.into());}
    if let Some(value)=url {validate_connection_url(value)?;}
    match credential {
        Some(CredentialReplacement::Environment {name}) => validate_credential_environment(name)?,
        Some(CredentialReplacement::Password {value}) if value.is_empty() => bail!("空のパスワードは保存できません。認証なしを明示的に選択してください"),
        _ => (),
    }
    let mut document:DocumentMut=source.parse()?;
    let sources=document.get_mut("perception").and_then(Item::as_table_mut)
        .and_then(|p|p.get_mut("obs_sources")).and_then(Item::as_array_of_tables_mut).context("取得元がありません")?;
    let target=sources.iter_mut().find(|s|s.get("id").and_then(Item::as_str)==Some(source_id)).context("指定した取得元がありません")?;
    if url.is_none() && credential.is_none() {return Ok(source.to_owned());}
    if let Some(value)=url {target["url"]=toml_edit::value(value);}
    if let Some(replacement)=credential {
        for key in ["password","password_env","use_local_obs_config_password"] {target.remove(key);}
        match replacement {
            CredentialReplacement::Environment {name} => {target["password_env"]=toml_edit::value(name);},
            CredentialReplacement::LocalObs => {target["use_local_obs_config_password"]=toml_edit::value(true);},
            CredentialReplacement::Password {value} => {target["password"]=toml_edit::value(value);},
            CredentialReplacement::None => (),
        }
    }
    let prepared=document.to_string();validate(&prepared)?;Ok(prepared)
}

pub fn prepare_source_create(source:&str, revision:&str, input:&SourceCreate) -> Result<String> {
    if revision != blake3::hash(source.as_bytes()).to_hex().as_str() {return Err(super::lifecycle::StaleRevision.into());}
    if input.id.is_empty() || input.id.len()>96 || !input.id.bytes().all(|c|c.is_ascii_alphanumeric() || c==b'_' || c==b'-') {
        bail!("新しい取得元IDは96文字以内の英数字・ハイフン・アンダースコアで指定してください");
    }
    validate_connection_url(&input.url)?;
    if let Some(name) = &input.password_env { validate_credential_environment(name)?; }
    let conf:crate::Conf=toml::from_str(source)?;conf.perception.validate()?;
    if conf.perception.obs_sources.iter().chain(&conf.perception.archived_obs_sources).any(|s|s.id==input.id) {bail!("同じ取得元IDが存在します。削除済みなら復元してください");}
    // Do not accidentally attach a new capture to an existing trigger/context.
    let mut occupied=std::collections::HashSet::new();
    for s in conf.perception.obs_sources.iter().chain(&conf.perception.archived_obs_sources) {occupied.insert(s.channel.as_str());}
    for p in &conf.ai.personas {
        if let Some(channel)=&p.channel_utterance {occupied.insert(channel.as_str());}
        for c in p.observe.triggers.iter().chain(&p.observe.include_additional).chain(&p.observe.exclude) {occupied.insert(c.as_str());}
    }
    let base=format!("obs:source:{}",input.id);let mut channel=base.clone();let mut suffix=1;
    while occupied.contains(channel.as_str()) {channel=format!("{base}-{suffix}");suffix+=1;}
    let mut document:DocumentMut=source.parse()?;
    if !document.contains_key("perception") {document["perception"]=Item::Table(toml_edit::Table::new());}
    let perception=document.get_mut("perception").and_then(Item::as_table_mut).context("画面取得設定を編集できません")?;
    if !perception.contains_key("obs_sources") || perception.get("obs_sources").and_then(Item::as_array).is_some_and(|a|a.is_empty()) {
        perception["obs_sources"]=Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let sources=perception.get_mut("obs_sources").and_then(Item::as_array_of_tables_mut).context("取得元一覧を編集できません")?;
    let mut entry=toml_edit::Table::new();
    for (name,value) in [("id",&input.id),("scene_name",&input.scene_name),("source_name",&input.source_name),("url",&input.url),("channel",&channel)] {entry[name]=toml_edit::value(value.as_str());}
    entry["enabled"]=toml_edit::value(false);
    entry["refresh_before_reply"]=toml_edit::value(false);
    entry["use_local_obs_config_password"]=toml_edit::value(input.use_local_obs_config_password);
    if let Some(env)=&input.password_env {entry["password_env"]=toml_edit::value(env.as_str());}
    let mut modes=Array::new();modes.push("streaming");entry["active_modes"]=toml_edit::value(modes);
    sources.push(entry);
    let prepared=document.to_string();validate(&prepared)?;Ok(prepared)
}

pub fn prepare_source_lifecycle(source:&str, revision:&str, source_id:&str, restore:bool, shared_ids:&[String], observers:&[usize]) -> Result<String> {
    let view=perception_view(source)?;
    if view.revision!=revision {return Err(super::lifecycle::StaleRevision.into());}
    let list=if restore {&view.archived_sources} else {&view.sources};
    let target=list.iter().find(|s|s.id==source_id).context("指定した取得元がありません")?;
    let mut shared=target.shared_source_ids.clone();shared.sort();let mut expected=shared_ids.to_vec();expected.sort();
    let actual:Vec<_>=target.observers.iter().filter(|p|p.rule.is_some_and(|r|!matches!(r,crate::ai::observe::ObservationRule::Excluded|crate::ai::observe::ObservationRule::Unobserved))).map(|p|p.persona_index).collect();
    let mut expected_observers=observers.to_vec();expected_observers.sort();
    if shared!=expected || actual!=expected_observers {bail!("共有範囲または利用する共演者が一致しません。変更していません");}
    let mut document:DocumentMut=source.parse()?;
    let perception=document.get_mut("perception").and_then(Item::as_table_mut).context("画面取得設定を編集できません")?;
    let (from,to)=if restore {("archived_obs_sources","obs_sources")} else {("obs_sources","archived_obs_sources")};
    let origin=perception.get_mut(from).and_then(Item::as_array_of_tables_mut).context("取得元一覧を編集できません")?;
    let index=origin.iter().position(|s|s.get("id").and_then(Item::as_str)==Some(source_id)).context("取得元がありません")?;
    let entry=origin.get(index).unwrap().clone();origin.remove(index);
    if !perception.contains_key(to) || perception.get(to).and_then(Item::as_array).is_some_and(|a|a.is_empty()) {
        perception[to]=Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    perception.get_mut(to).and_then(Item::as_array_of_tables_mut).context("復元先を編集できません")?.push(entry);
    let prepared=document.to_string();validate(&prepared)?;Ok(prepared)
}

fn validate(source: &str) -> Result<()> {
    #[derive(serde::Deserialize)]
    struct PerceptionOnly { #[serde(default)] perception: crate::stream_perception::StreamPerceptionConf }
    let conf: PerceptionOnly = toml::from_str(source)?;
    conf.perception.validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    const CONFIG: &str = r#"# keep this profile comment
workers = 2
[unrelated]
future_setting = "preserve"
[[ai.personas]]
id = "one"
custom_instructions = "KEEP_ONE"
[[ai.personas]]
id = "two"
custom_instructions = "KEEP_TWO"
[[perception.obs_sources]]
id = "game"
scene_name = "Game"
source_name = "Capture"
enabled = true
password_env = "TEST_OBS_PASSWORD"
width = 640
future_setting = "keep source extension"
[[perception.obs_sources]]
id = "notes"
scene_name = "Notes"
source_name = "Board"
password = "synthetic-fixture-only"
"#;
    fn revision(source: &str) -> String { blake3::hash(source.as_bytes()).to_hex().to_string() }

    #[test]
    fn connection_summary_reports_configuration_without_secret_url_parts() {
        let source=CONFIG.replace("width = 640","width = 640\nurl='wss://user:URL_SECRET@localhost:4456/private-path?token=QUERY_SECRET#FRAGMENT_SECRET'\npassword='PASSWORD_SECRET'\nuse_local_obs_config_password=true");
        let view=perception_view(&source).unwrap();
        let connection=&view.sources[0].connection;
        assert_eq!(connection.server.as_deref(),Some("localhost:4456"));
        assert!(connection.secure && connection.custom_path_or_query);
        assert!(connection.environment_password && connection.saved_password && connection.local_obs_password);
        let json=serde_json::to_string(&view).unwrap();
        for hidden in ["URL_SECRET","QUERY_SECRET","FRAGMENT_SECRET","PASSWORD_SECRET","private-path","TEST_OBS_PASSWORD"] {assert!(!json.contains(hidden));}
        let standard=perception_view(CONFIG).unwrap();
        assert_eq!(standard.sources[0].connection.server.as_deref(),Some("127.0.0.1:4455"));
        assert!(!standard.sources[0].connection.secure);
    }

    #[test]
    fn connection_replacement_is_explicit_preserves_capture_and_hides_credentials() {
        let original=CONFIG.replace("width = 640","width = 640\npassword='old-secret-fixture'\nuse_local_obs_config_password=true");
        assert_eq!(prepare_connection_edit(&original,&revision(&original),"game",None,None).unwrap(),original);
        let changed=prepare_connection_edit(&original,&revision(&original),"game",Some("ws://127.0.0.1:4456"),None).unwrap();
        let before:toml::Value=toml::from_str(&original).unwrap();
        let after:toml::Value=toml::from_str(&changed).unwrap();
        let mut expected=before.clone();expected["perception"]["obs_sources"][0].as_table_mut().unwrap().insert("url".into(),"ws://127.0.0.1:4456".into());
        assert_eq!(after,expected);
        for replacement in [CredentialReplacement::Environment{name:"NEW_OBS_SECRET_REFERENCE".into()},CredentialReplacement::LocalObs,CredentialReplacement::Password{value:"new-secret-fixture".into()},CredentialReplacement::None] {
            let edited=prepare_connection_edit(&changed,&revision(&changed),"game",None,Some(&replacement)).unwrap();
            let mut actual:toml::Value=toml::from_str(&edited).unwrap();
            let table=actual["perception"]["obs_sources"][0].as_table_mut().unwrap();
            let keys=["password","password_env","use_local_obs_config_password"];
            let present=keys.iter().filter(|key|table.contains_key(**key)).count();
            assert_eq!(present,if matches!(replacement,CredentialReplacement::None){0}else{1});
            match replacement {
                CredentialReplacement::Environment{ref name}=>assert_eq!(table["password_env"].as_str(),Some(name.as_str())),
                CredentialReplacement::LocalObs=>assert_eq!(table["use_local_obs_config_password"].as_bool(),Some(true)),
                CredentialReplacement::Password{ref value}=>assert_eq!(table["password"].as_str(),Some(value.as_str())),
                CredentialReplacement::None=>(),
            }
            for key in keys {table.remove(key);}
            let mut baseline=after.clone();for key in keys {baseline["perception"]["obs_sources"][0].as_table_mut().unwrap().remove(key);}
            assert_eq!(actual,baseline);
            let view=serde_json::to_string(&perception_view(&edited).unwrap()).unwrap();
            for secret in ["old-secret-fixture","new-secret-fixture","NEW_OBS_SECRET_REFERENCE","ws://127.0.0.1:4456"] {assert!(!view.contains(secret));}
        }
        for url in ["http://localhost:4455","ws://user:secret@localhost:4455",""] {assert!(prepare_connection_edit(&original,&revision(&original),"game",Some(url),None).is_err());}
        assert!(prepare_connection_edit(&original,&revision(&original),"missing",None,Some(&CredentialReplacement::None)).is_err());
        assert!(prepare_connection_edit(&changed,&revision(&original),"game",None,Some(&CredentialReplacement::None)).unwrap_err().is::<super::super::lifecycle::StaleRevision>());
        assert!(prepare_connection_edit(&original,&revision(&original),"game",None,Some(&CredentialReplacement::Password{value:String::new()})).is_err());
    }

    #[test]
    fn source_archive_restore_preserves_full_record_and_requires_scope() {
        let original=CONFIG.replace("id = \"one\"","id='one'\nchannel_utterance='ai:one'\nobserve={include_all=true}");
        let shared=vec!["game".into(),"notes".into()];
        assert!(prepare_source_lifecycle(&original,&revision(&original),"game",false,&shared,&[]).is_err());
        assert!(prepare_source_lifecycle(&original,&revision(&original),"game",false,&["game".into()],&[0]).is_err());
        let archived=prepare_source_lifecycle(&original,&revision(&original),"game",false,&shared,&[0]).unwrap();
        let before:toml::Value=toml::from_str(&original).unwrap();let after:toml::Value=toml::from_str(&archived).unwrap();
        assert_eq!(before["perception"]["obs_sources"][0],after["perception"]["archived_obs_sources"][0]);
        assert_eq!(before["perception"]["obs_sources"][1],after["perception"]["obs_sources"][0]);
        assert_eq!(before["ai"],after["ai"]);
        let view=perception_view(&archived).unwrap();
        assert_eq!(view.sources.len(),1);assert_eq!(view.archived_sources.len(),1);
        let json=serde_json::to_string(&view).unwrap();assert!(!json.contains("TEST_OBS_PASSWORD"));
        let duplicate=SourceCreate {id:"game".into(),scene_name:"Other".into(),source_name:"Other".into(),url:"ws://127.0.0.1:4455".into(),password_env:None,use_local_obs_config_password:false};
        assert!(prepare_source_create(&archived,&revision(&archived),&duplicate).is_err());
        assert!(prepare_source_lifecycle(&archived,&revision(&original),"game",true,&["notes".into()],&[0]).unwrap_err().is::<super::super::lifecycle::StaleRevision>());
        let restored=prepare_source_lifecycle(&archived,&revision(&archived),"game",true,&["notes".into()],&[0]).unwrap();
        let restored:toml::Value=toml::from_str(&restored).unwrap();
        // toml_edit retains document positions when moving tables. Match by
        // stable ID, rather than assuming restored sources serialize last.
        let restored_sources=restored["perception"]["obs_sources"].as_array().unwrap();
        assert_eq!(restored_sources.len(),2);
        for expected in before["perception"]["obs_sources"].as_array().unwrap() {
            let actual=restored_sources.iter().find(|s|s["id"]==expected["id"]).unwrap();
            assert_eq!(actual,expected);
        }
        assert_eq!(restored["ai"],before["ai"]);
    }

    #[test]
    fn source_creation_starts_disabled_and_avoids_existing_routing() {
        let input=SourceCreate {id:"new".into(),scene_name:"Scene".into(),source_name:"Selected game".into(),url:"ws://127.0.0.1:4455".into(),password_env:Some("OBS_SECRET_REFERENCE".into()),use_local_obs_config_password:false};
        let original=CONFIG.replace("id = \"one\"","id='one'\nobserve={triggers=['obs:source:new'],exclude=['obs:source:new-1']}");
        let prepared=prepare_source_create(&original,&revision(&original),&input).unwrap();
        let before:toml::Value=toml::from_str(&original).unwrap();let after:toml::Value=toml::from_str(&prepared).unwrap();
        assert_eq!(before["ai"],after["ai"]);
        for i in 0..2 {assert_eq!(before["perception"]["obs_sources"][i],after["perception"]["obs_sources"][i]);}
        let conf:crate::Conf=toml::from_str(&prepared).unwrap();let added=&conf.perception.obs_sources[2];
        assert!(!added.enabled && !added.refresh_before_reply);
        assert_eq!(added.channel,"obs:source:new-2");
        assert_eq!(added.active_modes,vec!["streaming"]);
        assert_eq!(added.password_env.as_deref(),Some("OBS_SECRET_REFERENCE"));
        assert!(prepare_source_create(&prepared,&revision(&prepared),&input).is_err());
        assert!(prepare_source_create(&prepared,&revision(&original),&input).unwrap_err().is::<super::super::lifecycle::StaleRevision>());
        for original in ["workers=1\n","workers=1\n[perception]\nobs_sources=[]\n"] {
            assert_eq!(perception_view(&prepare_source_create(original,&revision(original),&input).unwrap()).unwrap().sources.len(),1);
        }
        for url in ["http://example.com","ws://user:secret@example.com",""] {
            let bad=SourceCreate {url:url.into(),..SourceCreate {id:"new".into(),scene_name:"Scene".into(),source_name:"Selected".into(),url:String::new(),password_env:None,use_local_obs_config_password:false}};
            assert!(prepare_source_create(CONFIG,&revision(CONFIG),&bad).is_err());
        }
    }

    #[test]
    fn source_creation_rejects_invalid_credential_reference_before_writing() {
        for name in ["", " ", "OBS=PASSWORD", "OBS\0PASSWORD"] {
            let input=SourceCreate {id:"new".into(),scene_name:"Scene".into(),source_name:"Game".into(),url:"ws://127.0.0.1:4455".into(),password_env:Some(name.into()),use_local_obs_config_password:false};
            assert!(prepare_source_create(CONFIG,&revision(CONFIG),&input).is_err(), "Invalid environment name must not produce a saveable document: {name:?}");
            assert!(prepare_connection_edit(CONFIG,&revision(CONFIG),"game",None,Some(&CredentialReplacement::Environment {name:name.into()})).is_err());
        }
    }

    #[test]
    fn history_edit_retains_management_and_disabled_image_preference() {
        for setup in ["", "turn_management={enabled=true,avatar_sync_with_speech=true,past_screen_image_enabled=true,future_history='keep'}"] {
            let original=CONFIG.replace("id = \"one\"",&format!("id='one'\n{setup}"));
            let prepared=prepare_history_edit(&original,&revision(&original),0,Some(false),None).unwrap();
            let before:toml::Value=toml::from_str(&original).unwrap();
            let after:toml::Value=toml::from_str(&prepared).unwrap();
            assert_eq!(before["perception"],after["perception"]);
            assert_eq!(before["ai"]["personas"][1],after["ai"]["personas"][1]);
            let history=&perception_view(&prepared).unwrap().history[0];
            assert!(!history.visual_history_enabled);
            assert_eq!(history.turn_management_enabled,!setup.is_empty());
            assert_eq!(history.past_screen_image_enabled,!setup.is_empty());
            if !setup.is_empty() {
                assert_eq!(after["ai"]["personas"][0]["turn_management"]["future_history"].as_str(),Some("keep"));
                assert_eq!(after["ai"]["personas"][0]["turn_management"]["avatar_sync_with_speech"].as_bool(),Some(true));
            }
            let enabled=prepare_history_edit(&prepared,&revision(&prepared),0,Some(true),Some(true)).unwrap();
            let view=perception_view(&enabled).unwrap();
            assert!(view.history[0].visual_history_enabled && view.history[0].past_screen_image_enabled);
            assert_eq!(view.history[0].turn_management_enabled,!setup.is_empty(),"history edit must not silently enable turn management");
            assert!(prepare_history_edit(&enabled,&revision(&original),0,Some(false),None).unwrap_err().is::<super::super::lifecycle::StaleRevision>());
            assert!(prepare_history_edit(&enabled,&revision(&enabled),99,Some(false),None).is_err());
        }
    }

    #[test]
    fn managed_edit_keeps_same_content_peer_separate_and_rejects_path_escape() {
        let root=std::env::temp_dir().join(format!("unvac-perception-managed-{}",rand::random::<u64>()));
        for id in ["one","two"] {
            let dir=root.join("profiles").join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("conf.toml"),CONFIG).unwrap();
        }
        let edit=PerceptionEdit::Source {source_id:"game".into(),patch:SourcePatch {source_name:Some("Managed edit".into()),..Default::default()}};
        let held=super::super::lifecycle::operation_lock(&root).unwrap();
        assert!(save_managed_at(&root,"one",&revision(CONFIG),&edit).is_err());
        drop(held);
        for invalid in ["../two","one/../two","one\\..\\two","C:\\other",""] {
            assert!(save_managed_at(&root,invalid,&revision(CONFIG),&edit).is_err());
        }
        let result=save_managed_at(&root,"one",&revision(CONFIG),&edit).unwrap();
        let one=root.join("profiles/one/conf.toml");
        let two=root.join("profiles/two/conf.toml");
        let changed=std::fs::read_to_string(&one).unwrap();
        assert_eq!(revision(&changed),result.revision);
        assert!(changed.contains("Managed edit"));
        assert_eq!(std::fs::read_to_string(&two).unwrap(),CONFIG);
        assert!(save_managed_at(&root,"one",&revision(CONFIG),&edit).unwrap_err().is::<super::super::lifecycle::StaleRevision>());
        assert_eq!(std::fs::read_to_string(&one).unwrap(),changed);
        let archive=PerceptionEdit::SourceLifecycle {source_id:"game".into(),restore:false,shared_source_ids:vec!["game".into(),"notes".into()],observer_indices:vec![]};
        let held=super::super::lifecycle::operation_lock(&root).unwrap();
        assert!(save_managed_at(&root,"one",&result.revision,&archive).is_err());
        assert_eq!(std::fs::read_to_string(&one).unwrap(),changed);
        drop(held);
        let archived=save_managed_at(&root,"one",&result.revision,&archive).unwrap();
        assert!(archived.restart_required);
        let archived_text=std::fs::read_to_string(&one).unwrap();
        let view=perception_view(&archived_text).unwrap();
        assert_eq!(view.sources.len(),1);
        assert_eq!(view.archived_sources[0].id,"game");
        let restore=PerceptionEdit::SourceLifecycle {source_id:"game".into(),restore:true,shared_source_ids:vec!["notes".into()],observer_indices:vec![]};
        assert!(save_managed_at(&root,"one",&result.revision,&restore).unwrap_err().is::<super::super::lifecycle::StaleRevision>());
        assert_eq!(std::fs::read_to_string(&one).unwrap(),archived_text);
        let restored=save_managed_at(&root,"one",&archived.revision,&restore).unwrap();
        let restored_text=std::fs::read_to_string(&one).unwrap();
        assert_eq!(revision(&restored_text),restored.revision);
        let original_value:toml::Value=toml::from_str(&changed).unwrap();
        let restored_value:toml::Value=toml::from_str(&restored_text).unwrap();
        assert_eq!(original_value,restored_value);
        assert_eq!(std::fs::read_to_string(&two).unwrap(),CONFIG);
        for path in [&one,&two,&root.join(".profiles-operation.lock")] {std::fs::remove_file(path).unwrap();}
        for dir in [root.join("profiles/one"),root.join("profiles/two"),root.join("profiles"),root] {std::fs::remove_dir(dir).unwrap();}
    }

    #[test]
    fn assignment_preserves_triggers_and_other_personas_for_shared_sources() {
        let group = vec!["game".into(),"notes".into()];
        for initial in [
            "observe={include_all=true,triggers=['obs:game_frame'],exclude=['unrelated']}",
            "observe={triggers=['obs:game_frame'],exclude=['obs:game_frame','unrelated']}",
            "observe={include_additional=['other'],exclude=['unrelated']}",
            "observe={include_additional=['obs:game_frame','other']}",
            "",
        ] {
            let original = CONFIG.replace("id = \"one\"", &format!("id='one'\nchannel_utterance='ai:one'\n{initial}"));
            let before: toml::Value=toml::from_str(&original).unwrap();
            let before_conf:crate::Conf=toml::from_str(&original).unwrap();
            let mut current=original.clone();
            for enabled in [false,true,false,true] {
                current=prepare_assignment_edit(&current,&revision(&current),0,"game",&group,enabled).unwrap();
                let after:toml::Value=toml::from_str(&current).unwrap();
                assert_eq!(before["perception"],after["perception"]);
                assert_eq!(before["ai"]["personas"][1],after["ai"]["personas"][1]);
                assert_eq!(before["unrelated"],after["unrelated"]);
                let conf:crate::Conf=toml::from_str(&current).unwrap();
                let p=&conf.ai.personas[0];
                assert_eq!(p.observe.triggers,before_conf.ai.personas[0].observe.triggers);
                assert_eq!(p.observe.include_all,before_conf.ai.personas[0].observe.include_all);
                assert_eq!(p.custom_instructions,before_conf.ai.personas[0].custom_instructions);
                let observed=crate::ai::observe::ObserveSet::from_conf("ai:one",&p.observe);
                for s in &conf.perception.obs_sources { assert_eq!(observed.is_observed(&s.channel),enabled); }
                assert_eq!(prepare_assignment_edit(&current,&revision(&current),0,"notes",&group,enabled).unwrap(),current);
            }
            assert!(prepare_assignment_edit(&current,&revision(&original),0,"game",&group,false).unwrap_err().is::<super::super::lifecycle::StaleRevision>());
            assert!(prepare_assignment_edit(&current,&revision(&current),0,"game",&["game".into()],false).is_err());
            assert!(prepare_assignment_edit(&current,&revision(&current),2,"game",&group,false).is_err());
        }
    }

    #[test]
    fn assignment_file_save_supports_unnamed_persona_and_table_policy_with_conflicts() {
        let root = std::env::temp_dir().join(format!("unvac-assignment-save-{}",rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let path=root.join("conf.toml");
        // Unnamed personas remain addressable within the exact document revision.
        let original=CONFIG.replace("id = \"one\"", "channel_utterance='ai:one'")
            .replace("custom_instructions = \"KEEP_ONE\"", "custom_instructions='KEEP_ONE'\n[ai.personas.observe]\ninclude_all=true\ntriggers=['obs:game_frame']\nfuture_policy='preserved'");
        std::fs::write(&path,&original).unwrap();
        let group=vec!["game".into(),"notes".into()];
        let held=super::super::lifecycle::lock_config_write(&path).unwrap();
        assert!(save_assignment_edit(&path,&revision(&original),0,"game",&group,false).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(),original);
        drop(held);
        assert!(save_assignment_edit(&path,&revision(&original),0,"game",&["game".into()],false).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(),original);
        let saved=save_assignment_edit(&path,&revision(&original),0,"game",&group,false).unwrap();
        assert!(saved.restart_required);
        let reopened=std::fs::read_to_string(&path).unwrap();
        assert_eq!(saved.revision,revision(&reopened));
        let parsed:toml::Value=toml::from_str(&reopened).unwrap();
        assert_eq!(parsed["ai"]["personas"][0]["observe"]["future_policy"].as_str(),Some("preserved"));
        let view=perception_view(&reopened).unwrap();
        assert!(view.sources.iter().all(|s|s.observers[0].rule==Some(crate::ai::observe::ObservationRule::Excluded)));
        let stale=save_assignment_edit(&path,&revision(&original),0,"game",&group,true).unwrap_err();
        assert!(stale.is::<super::super::lifecycle::StaleRevision>());
        assert_eq!(std::fs::read_to_string(&path).unwrap(),reopened);
        let restored=save_assignment_edit(&path,&saved.revision,0,"notes",&group,true).unwrap();
        let restored_text=std::fs::read_to_string(&path).unwrap();
        assert_eq!(restored.revision,revision(&restored_text));
        let conf:crate::Conf=toml::from_str(&restored_text).unwrap();
        assert_eq!(conf.ai.personas[0].observe.triggers,vec!["obs:game_frame"]);
        assert!(conf.ai.personas[0].observe.include_additional.is_empty());
        assert_eq!(perception_view(&restored_text).unwrap().sources[0].observers[0].rule,Some(crate::ai::observe::ObservationRule::All));
        for file in [&path,&root.join(".unvac-config-write.lock")] {std::fs::remove_file(file).unwrap();}
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn view_exposes_shared_routing_and_exclusion_without_exporting_secrets() {
        use crate::ai::observe::ObservationRule;
        let conf = CONFIG.replace("id = \"one\"", "id = \"one\"\nchannel_utterance='ai:one'\nobserve={include_all=true,exclude=['obs:game_frame']}")
            .replace("id = \"two\"", "id = \"two\"\nchannel_utterance='ai:two'\nobserve={include_additional=['obs:game_frame']}");
        let view = perception_view(&conf).unwrap();
        assert_eq!(view.sources.len(),2);
        for source in &view.sources {
            assert_eq!(source.shared_source_ids,vec!["game","notes"]);
            assert_eq!(source.observers[0].rule,Some(ObservationRule::Excluded));
            assert_eq!(source.observers[1].rule,Some(ObservationRule::Additional));
        }
        let json=serde_json::to_string(&view).unwrap();
        for secret in ["synthetic-fixture-only","TEST_OBS_PASSWORD","KEEP_ONE","KEEP_TWO"] { assert!(!json.contains(secret)); }
        let missing=perception_view(CONFIG).unwrap();
        assert!(missing.sources[0].observers.iter().all(|p|p.rule.is_none()));
    }

    #[test]
    fn source_patch_keeps_other_sources_personas_credentials_and_unknown_fields() {
        let output = prepare_source_edit(CONFIG, &revision(CONFIG), "game", &SourcePatch {
            refresh_before_reply: Some(true), source_name: Some("New capture".into()),
            active_modes: Some(vec!["streaming".into()]), ..Default::default()
        }).unwrap();
        let before: toml::Value = toml::from_str(CONFIG).unwrap();
        let after: toml::Value = toml::from_str(&output).unwrap();
        assert_eq!(before["ai"], after["ai"]);
        assert_eq!(before["unrelated"], after["unrelated"]);
        assert_eq!(before["perception"]["obs_sources"][1], after["perception"]["obs_sources"][1]);
        let mut expected = before["perception"]["obs_sources"][0].clone();
        let expected_table = expected.as_table_mut().unwrap();
        expected_table.insert("refresh_before_reply".into(), true.into());
        expected_table.insert("source_name".into(), "New capture".into());
        expected_table.insert("active_modes".into(), toml::Value::Array(vec!["streaming".into()]));
        assert_eq!(expected, after["perception"]["obs_sources"][0]);
        assert!(output.starts_with("# keep this profile comment"));
    }

    #[test]
    fn stale_revision_missing_target_and_invalid_scope_return_no_prepared_config() {
        let conflict = prepare_source_edit(CONFIG, "stale", "game", &SourcePatch::default()).unwrap_err();
        assert!(conflict.downcast_ref::<super::super::lifecycle::StaleRevision>().is_some());
        assert!(prepare_source_edit(CONFIG, &revision(CONFIG), "absent", &SourcePatch::default()).is_err());
        for patch in [SourcePatch {scene_name:Some(" ".into()),..Default::default()},
            SourcePatch {active_modes:Some(vec!["".into()]),..Default::default()}] {
            assert!(prepare_source_edit(CONFIG, &revision(CONFIG), "game", &patch).is_err());
        }
        let duplicate = CONFIG.replace("id = \"notes\"", "id = \"game\"");
        assert!(prepare_source_edit(&duplicate, &revision(&duplicate), "game", &SourcePatch::default()).is_err());
        let empty_modes = prepare_source_edit(CONFIG, &revision(CONFIG), "game", &SourcePatch {active_modes:Some(vec![]),..Default::default()}).unwrap();
        let parsed: toml::Value = toml::from_str(&empty_modes).unwrap();
        assert!(parsed["perception"]["obs_sources"][0]["active_modes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn locked_file_save_reopens_and_rejects_stale_or_busy_writes_without_changing_peer() {
        let root = std::env::temp_dir().join(format!("unvac-perception-save-{}",rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let conf = root.join("conf.toml");
        let peer = root.join("peer.toml");
        let original = "workers=2\n[[perception.obs_sources]]\nid='game'\nscene_name='Game'\nsource_name='Capture'\npassword_env='KEEP_SECRET_REFERENCE'\n";
        std::fs::write(&conf, original).unwrap();
        std::fs::write(&peer, original).unwrap();
        let patch = SourcePatch { source_name: Some("Selected window".into()), refresh_before_reply:Some(true), ..Default::default() };
        let held = super::super::lifecycle::lock_config_write(&conf).unwrap();
        assert!(save_source_edit(&conf,&revision(original),"game",&patch).is_err());
        assert_eq!(std::fs::read_to_string(&conf).unwrap(),original);
        drop(held);
        let result=save_source_edit(&conf,&revision(original),"game",&patch).unwrap();
        let reopened=std::fs::read_to_string(&conf).unwrap();
        assert_eq!(result.revision,revision(&reopened));
        assert!(result.restart_required);
        let parsed:crate::Conf=toml::from_str(&reopened).unwrap();
        assert_eq!(parsed.perception.obs_sources[0].source_name,"Selected window");
        assert_eq!(parsed.perception.obs_sources[0].password_env.as_deref(),Some("KEEP_SECRET_REFERENCE"));
        let error=save_source_edit(&conf,&revision(original),"game",&patch).unwrap_err();
        assert!(error.is::<super::super::lifecycle::StaleRevision>());
        let invalid=SourcePatch {scene_name:Some("".into()),..Default::default()};
        assert!(save_source_edit(&conf,&result.revision,"game",&invalid).is_err());
        assert_eq!(std::fs::read_to_string(&conf).unwrap(),reopened);
        assert_eq!(std::fs::read_to_string(&peer).unwrap(),original);
        assert!(!std::fs::read_dir(&root).unwrap().any(|e|e.unwrap().file_name().to_string_lossy().contains("unvac-tmp")));
        for path in [&conf,&peer,&root.join(".unvac-config-write.lock")] { std::fs::remove_file(path).unwrap(); }
        std::fs::remove_dir(root).unwrap();
    }
}

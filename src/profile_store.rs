use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

mod ai_setup;
pub mod perception_setup;
pub mod dictionary_setup;
pub mod dictionary_connection;
pub(crate) use ai_setup::write_config_transaction;
mod bundle;
pub mod lifecycle;
pub use ai_setup::{configure_profile_ai, read_profile_ai, AiSetupInput, AiSetupStatus};
pub use bundle::{export_profile_bundle, import_profile_bundle};

const CONFIG_DIR_ENV: &str = "UN_VAC_CONFIG_DIR";
const WINDOWS_APP_DIR: &str = "UN Virtual Avatar Connect";
const PORTABLE_APP_DIR: &str = "un-virtual-avatar-connect";
const SETTINGS_FILE: &str = "settings.toml";
const PROFILES_DIR: &str = "profiles";
const PROFILE_CONF_FILE: &str = "conf.toml";
const PROFILE_MANIFEST_FILE: &str = "profile.toml";
const STARTER_PROFILE_ID: &str = "default";
const DESKTOP_DEFAULT_WEB_UI_ADDRESS: &str = "127.0.0.1:46000";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopSettings {
	#[serde(default = "default_schema_version")]
	pub schema_version: u32,
	pub active_profile_id: String,
	#[serde(default)]
	pub onboarding_completed: bool,
	/// 全Voice Ingressに対するソフトミュート。デスクトップ再起動後も維持する。
	#[serde(default)]
	pub voice_input_muted: bool,
}

#[derive(Debug, Clone)]
pub struct ActiveProfile {
	pub root: PathBuf,
	pub profiles_dir: PathBuf,
	pub profile_id: String,
	pub profile_dir: PathBuf,
	pub conf_path: PathBuf,
	pub settings: DesktopSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileManifest {
	#[serde(default = "default_schema_version")]
	pub schema_version: u32,
	pub id: String,
	pub label: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub template_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StoredProfile {
	pub id: String,
	pub label: String,
	pub conf_path: PathBuf,
	pub is_active: bool,
	pub template_id: Option<String>,
}

pub struct ProfileSeedFile<'a> {
	pub path: &'a str,
	pub contents: &'a [u8],
}

#[derive(Debug, Clone)]
pub struct ProfileStoreSnapshot {
	pub root: PathBuf,
	pub onboarding_completed: bool,
	pub active_profile_id: String,
	pub profiles: Vec<StoredProfile>,
}

fn default_schema_version() -> u32 {
	1
}

impl DesktopSettings {
	fn starter() -> Self {
		Self {
			schema_version: default_schema_version(),
			active_profile_id: STARTER_PROFILE_ID.to_string(),
			onboarding_completed: false,
			voice_input_muted: false,
		}
	}
}

pub fn voice_input_muted_for_conf(conf_path: Option<&Path>) -> Result<bool> {
    let Ok(root) = user_config_root() else { return Ok(false); };
    voice_input_muted_at(&root, conf_path)
}

fn voice_input_muted_at(root: &Path, conf_path: Option<&Path>) -> Result<bool> {
    if !conf_path.is_some_and(|path| is_managed_conf_path(root, path)) { return Ok(false); }
    let _operation = lifecycle::startup_operation_lock(root, std::time::Duration::from_secs(2))?;
    Ok(read_settings(&root.join(SETTINGS_FILE))?.is_some_and(|settings| settings.voice_input_muted))
}

pub fn set_voice_input_muted_for_conf(conf_path: Option<&Path>, muted: bool) -> Result<bool> {
	set_voice_input_muted_at(&user_config_root()?, conf_path, muted)
}

fn set_voice_input_muted_at(root: &Path, conf_path: Option<&Path>, muted: bool) -> Result<bool> {
	if !conf_path.is_some_and(|path| is_managed_conf_path(&root, path)) {
		return Ok(false);
	}
	let _operation = lifecycle::operation_lock(root)?;
	let settings_path = root.join(SETTINGS_FILE);
	let mut settings = read_settings(&settings_path)?.unwrap_or_else(DesktopSettings::starter);
	settings.voice_input_muted = muted;
	write_settings(&settings_path, &settings)?;
	Ok(true)
}

fn is_managed_conf_path(root: &Path, conf_path: &Path) -> bool {
	let profiles = root.join(PROFILES_DIR);
	let profiles = std::fs::canonicalize(&profiles).unwrap_or(profiles);
	let conf_path = std::fs::canonicalize(conf_path).unwrap_or_else(|_| conf_path.to_path_buf());
	conf_path.starts_with(profiles)
}

pub fn is_managed_metadata_path(path: &Path) -> bool {
	let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
	resolved.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.eq_ignore_ascii_case("profile.toml"))
		&& user_config_root().is_ok_and(|root| is_managed_conf_path(&root, &resolved))
}

pub fn user_config_root() -> Result<PathBuf> {
	if let Some(path) = std::env::var_os(CONFIG_DIR_ENV) {
		return Ok(PathBuf::from(path));
	}
	#[cfg(windows)]
	if let Some(path) = std::env::var_os("APPDATA") {
		return Ok(PathBuf::from(path).join(WINDOWS_APP_DIR));
	}
	if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
		return Ok(PathBuf::from(path).join(PORTABLE_APP_DIR));
	}
	if let Some(path) = std::env::var_os("HOME") {
		return Ok(PathBuf::from(path).join(".config").join(PORTABLE_APP_DIR));
	}
	Err(anyhow!("UNVAC のユーザー設定ディレクトリを取得できませんでした"))
}

pub fn desktop_default_conf(template: &str) -> Result<String> {
	desktop_managed_conf(template)
}

pub(crate) fn desktop_managed_conf(source: &str) -> Result<String> {
	let mut document = source.parse::<DocumentMut>().context("managed profile confを解釈できません")?;
	document["web_ui_address"] = toml_edit::value(DESKTOP_DEFAULT_WEB_UI_ADDRESS);
	Ok(document.to_string())
}

pub fn ensure_active_profile(default_conf_template: &str) -> Result<ActiveProfile> {
	ensure_active_profile_at(user_config_root()?, default_conf_template)
}

fn ensure_active_profile_at(root: PathBuf, default_conf_template: &str) -> Result<ActiveProfile> {
	let _operation = lifecycle::startup_operation_lock(&root, std::time::Duration::from_secs(2))?;
	let profiles_dir = root.join(PROFILES_DIR);
	std::fs::create_dir_all(&profiles_dir).with_context(|| format!("profile directoryを作成できません: {}", profiles_dir.display()))?;

	let settings_path = root.join(SETTINGS_FILE);
	let mut settings = read_settings(&settings_path)?.unwrap_or_else(DesktopSettings::starter);
	validate_profile_id(&settings.active_profile_id)?;

	let mut profile_id = settings.active_profile_id.clone();
	let mut profile_dir = profiles_dir.join(&profile_id);
	let mut conf_path = profile_dir.join(PROFILE_CONF_FILE);
	if !conf_path.is_file() {
		profile_id = STARTER_PROFILE_ID.to_string();
		settings.active_profile_id = profile_id.clone();
		settings.onboarding_completed = false;
		profile_dir = profiles_dir.join(&profile_id);
		conf_path = profile_dir.join(PROFILE_CONF_FILE);
		std::fs::create_dir_all(&profile_dir)
			.with_context(|| format!("starter profile directoryを作成できません: {}", profile_dir.display()))?;
		if !conf_path.exists() {
			prepare_new_blank_graph(&profile_dir, default_conf_template)?;
			std::fs::write(&conf_path, default_conf_template)
				.with_context(|| format!("starter profileを作成できません: {}", conf_path.display()))?;
		}
		write_profile_manifest(
			&profile_dir,
			&ProfileManifest {
				schema_version: default_schema_version(),
				id: profile_id.clone(),
				label: "Default".to_string(),
				template_id: Some("blank".to_string()),
			},
		)?;
	}
	write_settings(&settings_path, &settings)?;

	Ok(ActiveProfile {
		root,
		profiles_dir,
		profile_id,
		profile_dir,
		conf_path,
		settings,
	})
}

pub fn snapshot() -> Result<ProfileStoreSnapshot> {
	snapshot_at(user_config_root()?)
}

fn snapshot_at(root: PathBuf) -> Result<ProfileStoreSnapshot> {
	let settings = read_settings(&root.join(SETTINGS_FILE))?.unwrap_or_else(DesktopSettings::starter);
	let profiles_dir = root.join(PROFILES_DIR);
	let mut profiles = Vec::new();
	if profiles_dir.is_dir() {
		for entry in
			std::fs::read_dir(&profiles_dir).with_context(|| format!("profile directoryを列挙できません: {}", profiles_dir.display()))?
		{
			let entry = entry?;
			let profile_dir = entry.path();
			if !profile_dir.is_dir() {
				continue;
			}
			let Some(id) = profile_dir.file_name().and_then(|value| value.to_str()).map(str::to_string) else {
				continue;
			};
			if validate_profile_id(&id).is_err() {
				continue;
			}
			let conf_path = profile_dir.join(PROFILE_CONF_FILE);
			if !conf_path.is_file() {
				continue;
			}
			let manifest = read_profile_manifest(&profile_dir).unwrap_or(ProfileManifest {
				schema_version: default_schema_version(),
				id: id.clone(),
				label: profile_label_from_id(&id),
				template_id: None,
			});
			profiles.push(StoredProfile {
				id: id.clone(),
				label: manifest.label,
				conf_path,
				is_active: id == settings.active_profile_id,
				template_id: manifest.template_id,
			});
		}
	}
	profiles.sort_by(|left, right| {
		left.label
			.to_lowercase()
			.cmp(&right.label.to_lowercase())
			.then(left.id.cmp(&right.id))
	});
	Ok(ProfileStoreSnapshot {
		root,
		onboarding_completed: settings.onboarding_completed,
		active_profile_id: settings.active_profile_id,
		profiles,
	})
}

pub fn create_profile_with_files(
	id: &str,
	label: &str,
	conf_source: &str,
	template_id: Option<&str>,
	files: &[ProfileSeedFile<'_>],
) -> Result<StoredProfile> {
	create_profile_at(user_config_root()?, id, label, conf_source, template_id, files)
}

fn create_profile_at(
	root: PathBuf,
	id: &str,
	label: &str,
	conf_source: &str,
	template_id: Option<&str>,
	files: &[ProfileSeedFile<'_>],
) -> Result<StoredProfile> {
	let _operation = lifecycle::operation_lock(&root)?;
	validate_profile_id(id)?;
	let label = validate_profile_label(label)?;
	let conf_source = desktop_managed_conf(conf_source)?;
	let profiles_dir = root.join(PROFILES_DIR);
	std::fs::create_dir_all(&profiles_dir).with_context(|| format!("profile directoryを作成できません: {}", profiles_dir.display()))?;
	let profile_dir = profiles_dir.join(id);
	if profile_dir.exists() {
		return Err(anyhow!("profileは既に存在します: {id}"));
	}
	let staging = staging_profile_dir(&profiles_dir, id, "create");
	let result = (|| -> Result<()> {
		std::fs::create_dir(&staging).with_context(|| format!("profile staging directoryを作成できません: {}", staging.display()))?;
		std::fs::write(staging.join(PROFILE_CONF_FILE), &conf_source).context("profile confを書けません")?;
		if template_id == Some("blank") {
			prepare_new_blank_graph(&staging, &conf_source)?;
		}
		for file in files {
			let relative = validate_profile_relative_path(file.path)?;
			let path = staging.join(relative);
			if let Some(parent) = path.parent() {
				std::fs::create_dir_all(parent).with_context(|| format!("profile file directoryを作成できません: {}", parent.display()))?;
			}
			std::fs::write(&path, file.contents).with_context(|| format!("profile fileを書けません: {}", path.display()))?;
		}
		write_profile_manifest(
			&staging,
			&ProfileManifest {
				schema_version: default_schema_version(),
				id: id.to_string(),
				label: label.to_string(),
				template_id: template_id.map(str::to_string),
			},
		)?;
		std::fs::rename(&staging, &profile_dir).with_context(|| format!("profileを確定できません: {}", profile_dir.display()))?;
		Ok(())
	})();
	if let Err(error) = result {
		let _ = std::fs::remove_dir_all(&staging);
		return Err(error);
	}
	let conf_path = profile_dir.join(PROFILE_CONF_FILE);
	Ok(StoredProfile {
		id: id.to_string(),
		label: label.to_string(),
		conf_path,
		is_active: false,
		template_id: template_id.map(str::to_string),
	})
}

// Only seed the standard workspace of a newly created blank profile. Existing
// profiles and explicit alternative roots must retain missing-root diagnostics.
fn prepare_new_blank_graph(profile_dir: &Path, source: &str) -> Result<()> {
	let document = source.parse::<DocumentMut>()?;
	let standard = match document.get("flowgraph_dir") {
		None => true,
		Some(value) => value.as_str() == Some("flowgraph"),
	};
	if standard {
		std::fs::create_dir_all(profile_dir.join("flowgraph"))
			.context("空のFlowgraph保存先を作成できません")?;
	}
	Ok(())
}

fn validate_profile_label(value: &str) -> Result<&str> {
	let value = value.trim();
	if value.is_empty() || value.chars().count() > 80 {
		Err(anyhow!("profile labelは1〜80文字で指定してください"))
	} else {
		Ok(value)
	}
}

fn staging_profile_dir(profiles_dir: &Path, id: &str, operation: &str) -> PathBuf {
	use std::time::{SystemTime, UNIX_EPOCH};
	let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
	profiles_dir.join(format!(".{id}.{operation}-{}-{nonce}", std::process::id()))
}

pub fn activate_profile(id: &str) -> Result<StoredProfile> {
	activate_profile_at(user_config_root()?, id)
}

pub fn rename_profile_label(id: &str, label: &str) -> Result<StoredProfile> {
	rename_profile_label_at(user_config_root()?, id, label)
}

fn rename_profile_label_at(root: PathBuf, id: &str, label: &str) -> Result<StoredProfile> {
	let _operation = lifecycle::operation_lock(&root)?;
	validate_profile_id(id)?;
	let label = validate_profile_label(label)?;
	let profile_dir = root.join(PROFILES_DIR).join(id);
	let mut manifest = read_profile_manifest(&profile_dir)?;
	if !profile_dir.join(PROFILE_CONF_FILE).is_file() {
		return Err(anyhow!("profile confがありません: {id}"));
	}
	manifest.label = label.to_owned();
	write_profile_manifest(&profile_dir, &manifest)?;
	snapshot_at(root)?
		.profiles
		.into_iter()
		.find(|profile| profile.id == id)
		.context("renamed profile missing")
}

fn activate_profile_at(root: PathBuf, id: &str) -> Result<StoredProfile> {
	let _operation = lifecycle::operation_lock(&root)?;
	validate_profile_id(id)?;
	let profile_dir = root.join(PROFILES_DIR).join(id);
	let conf_path = profile_dir.join(PROFILE_CONF_FILE);
	if !conf_path.is_file() {
		return Err(anyhow!("profile confが存在しません: {}", conf_path.display()));
	}
	let manifest = read_profile_manifest(&profile_dir).unwrap_or(ProfileManifest {
		schema_version: default_schema_version(),
		id: id.to_string(),
		label: profile_label_from_id(id),
		template_id: None,
	});
	let settings_path = root.join(SETTINGS_FILE);
	let mut settings = read_settings(&settings_path)?.unwrap_or_else(DesktopSettings::starter);
	settings.active_profile_id = id.to_string();
	settings.onboarding_completed = true;
	write_settings(&settings_path, &settings)?;
	Ok(StoredProfile {
		id: id.to_string(),
		label: manifest.label,
		conf_path,
		is_active: true,
		template_id: manifest.template_id,
	})
}

fn read_profile_manifest(profile_dir: &Path) -> Result<ProfileManifest> {
	let path = profile_dir.join(PROFILE_MANIFEST_FILE);
	let source = std::fs::read_to_string(&path).with_context(|| format!("profile manifestを読めません: {}", path.display()))?;
	toml::from_str(&source).with_context(|| format!("profile manifestを解釈できません: {}", path.display()))
}

fn write_profile_manifest(profile_dir: &Path, manifest: &ProfileManifest) -> Result<()> {
	let source = toml::to_string_pretty(manifest).context("profile manifestをTOMLへ変換できません")?;
	let path = profile_dir.join(PROFILE_MANIFEST_FILE);
	std::fs::write(&path, source).with_context(|| format!("profile manifestを書けません: {}", path.display()))
}

fn profile_label_from_id(id: &str) -> String {
	id.split(['-', '_'])
		.filter(|part| !part.is_empty())
		.map(|part| {
			let mut chars = part.chars();
			match chars.next() {
				Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
				None => String::new(),
			}
		})
		.collect::<Vec<_>>()
		.join(" ")
}

fn read_settings(path: &Path) -> Result<Option<DesktopSettings>> {
	if !path.is_file() {
		return Ok(None);
	}
	let source = std::fs::read_to_string(path).with_context(|| format!("settingsを読めません: {}", path.display()))?;
	let settings = toml::from_str(&source).with_context(|| format!("settingsを解釈できません: {}", path.display()))?;
	Ok(Some(settings))
}

fn write_settings(path: &Path, settings: &DesktopSettings) -> Result<()> {
	let source = toml::to_string_pretty(settings).context("desktop settingsをTOMLへ変換できません")?;
	let temporary = path.with_extension("toml.tmp");
	let previous = path.with_extension("toml.previous");
	std::fs::write(&temporary, source).with_context(|| format!("settings一時ファイルを書けません: {}", temporary.display()))?;
	let had_previous = path.exists();
	if previous.exists() {
		std::fs::remove_file(&previous).with_context(|| format!("古いsettings退避を削除できません: {}", previous.display()))?;
	}
	if path.exists() {
		std::fs::rename(path, &previous).with_context(|| format!("旧settingsを退避できません: {}", path.display()))?;
	}
	if let Err(error) = std::fs::rename(&temporary, path) {
		if had_previous {
			let _ = std::fs::rename(&previous, path);
		}
		return Err(error).with_context(|| format!("settingsを確定できません: {}", path.display()));
	}
	if had_previous {
		let _ = std::fs::remove_file(previous);
	}
	Ok(())
}

fn validate_profile_id(value: &str) -> Result<()> {
	let valid = !value.is_empty()
		&& value.len() <= 64
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_');
	if valid {
		Ok(())
	} else {
		Err(anyhow!("profile idが不正です: {value:?}"))
	}
}

fn validate_profile_relative_path(value: &str) -> Result<PathBuf> {
	let path = Path::new(value);
	let valid = !value.is_empty()
		&& !path.is_absolute()
		&& path
			.components()
			.all(|component| matches!(component, std::path::Component::Normal(_)));
	if valid {
		Ok(path.to_path_buf())
	} else {
		Err(anyhow!("profile file pathが不正です: {value:?}"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::{SystemTime, UNIX_EPOCH};

	fn temp_root(label: &str) -> PathBuf {
		let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		std::env::temp_dir().join(format!("unvac-profile-store-{label}-{}-{nonce}", std::process::id()))
	}

	#[test]
	fn initial_voice_acceptance_waits_and_does_not_hide_corrupt_settings() {
		let root = temp_root("initial-voice-lock");
		let active = ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let lock = lifecycle::operation_lock(&root).unwrap();
		let (tx, rx) = std::sync::mpsc::channel();
		let worker_root = root.clone(); let conf = active.conf_path.clone();
		let worker = std::thread::spawn(move || { tx.send(voice_input_muted_at(&worker_root, Some(&conf))).unwrap(); });
		assert!(matches!(rx.recv_timeout(std::time::Duration::from_millis(80)), Err(std::sync::mpsc::RecvTimeoutError::Timeout)));
		let path = root.join(SETTINGS_FILE);
		let mut settings = read_settings(&path).unwrap().unwrap(); settings.voice_input_muted = true;
		write_settings(&path, &settings).unwrap(); drop(lock);
		assert!(rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap().unwrap()); worker.join().unwrap();
		std::fs::write(&path, "schemaVersion = [broken").unwrap();
		assert!(voice_input_muted_at(&root, Some(&active.conf_path)).is_err());
		assert_eq!(std::fs::read_to_string(&path).unwrap(), "schemaVersion = [broken");
		assert!(!voice_input_muted_at(&root, Some(&root.join("external.toml"))).unwrap());
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn startup_waits_for_settings_owner_then_reads_latest_selection() {
		let root = temp_root("startup-lock");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		create_profile_at(root.clone(), "other", "Other", "workers = 1\n", Some("blank"), &[]).unwrap();
		let lock = lifecycle::operation_lock(&root).unwrap();
		let (tx, rx) = std::sync::mpsc::channel();
		let worker_root = root.clone();
		let worker = std::thread::spawn(move || { tx.send(ensure_active_profile_at(worker_root, "workers = 1\n")).unwrap(); });
		assert!(matches!(rx.recv_timeout(std::time::Duration::from_millis(80)), Err(std::sync::mpsc::RecvTimeoutError::Timeout)));
		let path = root.join(SETTINGS_FILE);
		let mut settings = read_settings(&path).unwrap().unwrap();
		settings.active_profile_id = "other".into(); settings.voice_input_muted = true;
		write_settings(&path, &settings).unwrap();
		drop(lock);
		let started = rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap().unwrap();
		worker.join().unwrap();
		assert_eq!(started.profile_id, "other"); assert!(started.settings.voice_input_muted);
		let lock = lifecycle::operation_lock(&root).unwrap();
		let before = std::fs::read(&path).unwrap();
		let at = std::time::Instant::now();
		assert!(lifecycle::startup_operation_lock(&root, std::time::Duration::from_millis(40)).is_err());
		assert!(at.elapsed() < std::time::Duration::from_secs(1));
		assert_eq!(std::fs::read(&path).unwrap(), before);
		drop(lock); std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn voice_acceptance_save_respects_profile_operation_and_preserves_selection() {
		let root = temp_root("voice-settings-lock");
		let active = ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		create_profile_at(root.clone(), "other", "Other", "workers = 1\n", Some("blank"), &[]).unwrap();
		let before = std::fs::read(root.join(SETTINGS_FILE)).unwrap();
		let lock = lifecycle::operation_lock(&root).unwrap();
		assert!(set_voice_input_muted_at(&root, Some(&active.conf_path), true).is_err(), "must reject while another profile operation owns settings");
		assert_eq!(std::fs::read(root.join(SETTINGS_FILE)).unwrap(), before);
		drop(lock);
		activate_profile_at(root.clone(), "other").unwrap();
		assert!(set_voice_input_muted_at(&root, Some(&active.conf_path), true).unwrap());
		let settings = read_settings(&root.join(SETTINGS_FILE)).unwrap().unwrap();
		assert_eq!(settings.active_profile_id, "other");
		assert!(settings.voice_input_muted && settings.onboarding_completed);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn new_blank_profiles_load_but_existing_missing_roots_stay_missing() {
		let root = temp_root("blank-graph");
		let active = ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let graphs = active.profile_dir.join("flowgraph");
		let report = crate::flowgraph::loader::load_flowgraph_dir(&graphs).unwrap();
		assert_eq!(report.graph_signature.file_count, 0);
		std::fs::remove_dir(&graphs).unwrap();
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		assert!(!graphs.exists(), "reopening must not hide an existing missing root");
		let blank = create_profile_at(root.clone(), "blank", "Blank", "workers = 1\n", Some("blank"), &[]).unwrap();
		assert!(crate::flowgraph::loader::load_flowgraph_dir(&blank.conf_path.parent().unwrap().join("flowgraph")).is_ok());
		let custom = create_profile_at(root.clone(), "custom", "Custom", "flowgraph_dir = '../external'\n", Some("blank"), &[]).unwrap();
		assert!(!custom.conf_path.parent().unwrap().join("flowgraph").exists());
		assert!(!root.join("profiles/external").exists());
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn label_change_preserves_identity_settings_and_profile_contents() {
		let root = temp_root("label");
		let active = ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let original = std::fs::read(&active.conf_path).unwrap();
		let settings = std::fs::read(root.join(SETTINGS_FILE)).unwrap();
		let result = rename_profile_label_at(root.clone(), "default", "新しい表示名").unwrap();
		assert_eq!(result.id, "default");
		assert_eq!(result.label, "新しい表示名");
		assert_eq!(std::fs::read(&active.conf_path).unwrap(), original);
		assert_eq!(std::fs::read(root.join(SETTINGS_FILE)).unwrap(), settings);
		assert!(rename_profile_label_at(root.clone(), "default", " ").is_err());
		assert_eq!(read_profile_manifest(&active.profile_dir).unwrap().label, "新しい表示名");
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn first_run_creates_starter_profile_and_settings() {
		let root = temp_root("first-run");
		let active = ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		assert_eq!(active.profile_id, "default");
		assert!(!active.settings.onboarding_completed);
		assert_eq!(std::fs::read_to_string(active.conf_path).unwrap(), "workers = 1\n");
		assert!(root.join("settings.toml").is_file());
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn missing_active_profile_falls_back_without_overwriting_starter() {
		let root = temp_root("fallback");
		std::fs::create_dir_all(&root).unwrap();
		write_settings(
			&root.join("settings.toml"),
			&DesktopSettings {
				schema_version: 1,
				active_profile_id: "missing".into(),
				onboarding_completed: true,
				voice_input_muted: false,
			},
		)
		.unwrap();
		let active = ensure_active_profile_at(root.clone(), "workers = 2\n").unwrap();
		assert_eq!(active.profile_id, "default");
		assert!(!active.settings.onboarding_completed);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn managed_conf_path_accepts_canonical_windows_path() {
		let root = temp_root("managed-path");
		let conf = root.join("profiles/default/conf.toml");
		std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
		std::fs::write(&conf, "workers = 1\n").unwrap();
		let canonical = std::fs::canonicalize(&conf).unwrap();
		assert!(is_managed_conf_path(&root, &canonical));
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn profile_id_rejects_path_syntax() {
		assert!(validate_profile_id("../escape").is_err());
		assert!(validate_profile_id("Valid").is_err());
		assert!(validate_profile_id("streaming-ai").is_ok());
	}

	#[test]
	fn desktop_default_conf_uses_port_below_windows_dynamic_range() {
		let source = desktop_default_conf("workers = 1\n").unwrap();
		let parsed = source.parse::<DocumentMut>().unwrap();
		assert_eq!(parsed["web_ui_address"].as_str(), Some("127.0.0.1:46000"));
		assert_eq!(parsed["workers"].as_integer(), Some(1));
	}

	#[test]
	fn template_creation_preserves_named_purposes_and_rejects_existing_profile() {
		let root = temp_root("template-purposes");
		let template = crate::profile_templates::materialize("streaming-tools", "workers = 1\n").unwrap();
		let created = create_profile_at(root.clone(), "tools", "配信支援", &template.conf, Some("streaming-tools"), &template.files).unwrap();
		let graphs = created.conf_path.parent().unwrap().join("flowgraph");
		let catalogue = crate::web_interface::control::flowgraph::purposes::read(&graphs).unwrap();
		assert_eq!(catalogue.document.purposes.len(), 2);
		for purpose in &catalogue.document.purposes {
			for fq in &purpose.roots { assert!(graphs.join(format!("{fq}.flowgraph.toml")).is_file()); }
		}
		let metadata = graphs.join(".flowgraph-purposes.json");
		let before = std::fs::read(&metadata).unwrap();
		let conf_before = std::fs::read(&created.conf_path).unwrap();
		assert!(create_profile_at(root.clone(), "tools", "置換しない", "workers = 2\n", None, &[]).is_err());
		assert_eq!(std::fs::read(metadata).unwrap(), before);
		assert_eq!(std::fs::read(created.conf_path).unwrap(), conf_before);
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn create_snapshot_and_activate_profile() {
		let root = temp_root("lifecycle");
		ensure_active_profile_at(root.clone(), "workers = 1\n").unwrap();
		let created = create_profile_at(root.clone(), "streaming-ai", "Streaming AI", "workers = 4\n", None, &[]).unwrap();
		assert!(!created.is_active);
		let conf = std::fs::read_to_string(&created.conf_path).unwrap();
		assert!(conf.contains("web_ui_address = \"127.0.0.1:46000\""));
		let active = activate_profile_at(root.clone(), "streaming-ai").unwrap();
		assert!(active.is_active);
		let snapshot = snapshot_at(root.clone()).unwrap();
		assert!(snapshot.onboarding_completed);
		assert_eq!(snapshot.active_profile_id, "streaming-ai");
		assert_eq!(snapshot.profiles.len(), 2);
		std::fs::remove_dir_all(root).unwrap();
	}
}

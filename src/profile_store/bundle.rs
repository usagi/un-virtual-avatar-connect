use anyhow::{anyhow, Context, Result};
use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use super::{
	default_schema_version, staging_profile_dir, validate_profile_id, validate_profile_label, write_profile_manifest, ProfileManifest,
	StoredProfile, PROFILES_DIR, PROFILE_CONF_FILE, PROFILE_MANIFEST_FILE, SETTINGS_FILE,
};

const MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;
const MAX_FILE_COUNT: usize = 1024;
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

struct BundleFile {
	path: PathBuf,
	contents: Vec<u8>,
}

pub fn import_profile_bundle(id: &str, label: &str, bytes: &[u8]) -> Result<StoredProfile> {
	import_profile_bundle_at(super::user_config_root()?, id, label, bytes)
}

pub fn export_profile_bundle(id: &str) -> Result<Vec<u8>> {
	export_profile_bundle_at(super::user_config_root()?, id)
}

fn export_profile_bundle_at(root: PathBuf, id: &str) -> Result<Vec<u8>> {
	let _operation = super::lifecycle::operation_lock(&root)?;
	validate_profile_id(id)?;
	let profile_dir = root.join(PROFILES_DIR).join(id);
	if !profile_dir.join(PROFILE_CONF_FILE).is_file() {
		return Err(anyhow!("profileがありません: {id}"));
	}
	let mut paths = Vec::new();
	collect_export_files(&profile_dir, &profile_dir, &mut paths)?;
	paths.sort();
	if paths.len() > MAX_FILE_COUNT {
		return Err(anyhow!("profile bundleのfile数が上限を超えています: {}", paths.len()));
	}
	let cursor = Cursor::new(Vec::new());
	let mut writer = zip::ZipWriter::new(cursor);
	let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
	let mut total = 0_u64;
	for path in paths {
		let relative = path.strip_prefix(&profile_dir).expect("collected path must be inside profile");
		validate_bundle_path(relative)?;
		let contents = std::fs::read(&path).with_context(|| format!("profile fileを読めません: {}", path.display()))?;
		if contents.len() as u64 > MAX_FILE_BYTES {
			return Err(anyhow!("profile bundle内のfileが大きすぎます: {}", relative.display()));
		}
		total = total.saturating_add(contents.len() as u64);
		if total > MAX_TOTAL_BYTES {
			return Err(anyhow!("profile bundleの合計sizeが上限を超えています"));
		}
		if relative
			.extension()
			.and_then(|value| value.to_str())
			.is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
		{
			ensure_toml_has_no_inline_secret(relative, &contents)?;
		}
		let archive_path = relative
			.components()
			.filter_map(|component| match component {
				Component::Normal(value) => Some(value.to_string_lossy()),
				_ => None,
			})
			.collect::<Vec<_>>()
			.join("/");
		writer.start_file(archive_path, options)?;
		writer.write_all(&contents)?;
	}
	let bytes = writer.finish()?.into_inner();
	if bytes.len() > MAX_ARCHIVE_BYTES {
		return Err(anyhow!("profile bundleの圧縮sizeが32 MiBを超えています。関連ファイルを減らしてからexportしてください"));
	}
	Ok(bytes)
}

fn collect_export_files(root: &Path, directory: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
	for entry in std::fs::read_dir(directory).with_context(|| format!("profile directoryを読めません: {}", directory.display()))? {
		let entry = entry?;
		let path = entry.path();
		let relative = path.strip_prefix(root).expect("walked path must be inside profile");
		if export_ignored(relative) {
			continue;
		}
		let kind = entry.file_type()?;
		if kind.is_symlink() {
			return Err(anyhow!("profile bundleのsymlinkを拒否しました: {}", relative.display()));
		}
		if kind.is_dir() {
			collect_export_files(root, &path, output)?;
		} else if kind.is_file() {
			output.push(path);
		}
	}
	Ok(())
}

fn export_ignored(path: &Path) -> bool {
	if path == Path::new(PROFILE_MANIFEST_FILE) || path == Path::new(SETTINGS_FILE) {
		return true;
	}
	if path.components().any(|component| {
		matches!(component, Component::Normal(value) if matches!(value.to_str(), Some("credentials" | "tmp.local" | "flowgraph.disabled")))
	}) {
		return true;
	}
	let name = path
		.file_name()
		.and_then(|value| value.to_str())
		.unwrap_or_default()
		.to_ascii_lowercase();
	// This hidden file is current configuration, unlike journals/backups.
	(name.starts_with('.') && name != ".flowgraph-purposes.json")
		|| name.ends_with(".bak")
		|| name.contains(".bak-")
		|| name.contains(".pre-")
		|| name.contains(".before-")
		|| name.contains(".old.")
}

fn ensure_toml_has_no_inline_secret(path: &Path, contents: &[u8]) -> Result<()> {
	let source = std::str::from_utf8(contents).with_context(|| format!("{} はUTF-8 TOMLではありません", path.display()))?;
	let value = toml::from_str::<toml::Value>(source).with_context(|| format!("{} をTOMLとして解釈できません", path.display()))?;
	if let Some(key) = find_inline_secret(&value) {
		return Err(anyhow!(
			"{} にinline secret `{}`があります。credential fileまたはOAuth storeへ移してからexportしてください",
			path.display(),
			key
		));
	}
	Ok(())
}

fn find_inline_secret(value: &toml::Value) -> Option<&str> {
	match value {
		toml::Value::Table(table) => {
			for (key, child) in table {
				let normalized = key.to_ascii_lowercase();
				if matches!(
					normalized.as_str(),
					"api_key"
						| "access_token" | "refresh_token"
						| "oauth_token" | "bearer_token"
						| "password" | "client_secret"
						| "device_code"
				) && !matches!(child, toml::Value::String(value) if value.trim().is_empty())
				{
					return Some(key);
				}
				if let Some(found) = find_inline_secret(child) {
					return Some(found);
				}
			}
			None
		}
		toml::Value::Array(values) => values.iter().find_map(find_inline_secret),
		_ => None,
	}
}

fn import_profile_bundle_at(root: PathBuf, id: &str, label: &str, bytes: &[u8]) -> Result<StoredProfile> {
	let _operation = super::lifecycle::operation_lock(&root)?;
	validate_profile_id(id)?;
	let label = validate_profile_label(label)?;
	if bytes.len() > MAX_ARCHIVE_BYTES {
		return Err(anyhow!("profile bundleが大きすぎます: {} bytes", bytes.len()));
	}
	let mut files = read_bundle(bytes)?;
	let conf_index = files
		.iter()
		.position(|file| file.path == Path::new(PROFILE_CONF_FILE))
		.ok_or_else(|| anyhow!("profile bundleのrootにconf.tomlがありません"))?;
	let conf_source = std::str::from_utf8(&files[conf_index].contents).context("bundle conf.tomlはUTF-8である必要があります")?;
	let conf_source = super::desktop_managed_conf(conf_source)?;
	toml::from_str::<crate::Conf>(&conf_source).context("bundle conf.tomlを解釈できません")?;
	files[conf_index].contents = conf_source.into_bytes();

	let profiles_dir = root.join(PROFILES_DIR);
	std::fs::create_dir_all(&profiles_dir).with_context(|| format!("profile directoryを作成できません: {}", profiles_dir.display()))?;
	let profile_dir = profiles_dir.join(id);
	if profile_dir.exists() {
		return Err(anyhow!("profileは既に存在します: {id}"));
	}
	let staging = staging_profile_dir(&profiles_dir, id, "import");
	let result = (|| -> Result<()> {
		std::fs::create_dir(&staging).with_context(|| format!("bundle staging directoryを作成できません: {}", staging.display()))?;
		for file in &files {
			if file.path == Path::new(PROFILE_MANIFEST_FILE) || file.path == Path::new(SETTINGS_FILE) {
				continue;
			}
			let path = staging.join(&file.path);
			if let Some(parent) = path.parent() {
				std::fs::create_dir_all(parent).with_context(|| format!("bundle directoryを作成できません: {}", parent.display()))?;
			}
			std::fs::write(&path, &file.contents).with_context(|| format!("bundle fileを書けません: {}", path.display()))?;
		}
		write_profile_manifest(
			&staging,
			&ProfileManifest {
				schema_version: default_schema_version(),
				id: id.to_string(),
				label: label.to_string(),
				template_id: None,
			},
		)?;
		std::fs::rename(&staging, &profile_dir).with_context(|| format!("import profileを確定できません: {}", profile_dir.display()))?;
		Ok(())
	})();
	if let Err(error) = result {
		let _ = std::fs::remove_dir_all(&staging);
		return Err(error);
	}
	Ok(StoredProfile {
		id: id.to_string(),
		label: label.to_string(),
		conf_path: profile_dir.join(PROFILE_CONF_FILE),
		is_active: false,
		template_id: None,
	})
}

fn read_bundle(bytes: &[u8]) -> Result<Vec<BundleFile>> {
	let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).context("profile bundleをZIPとして読めません")?;
	if archive.len() > MAX_FILE_COUNT {
		return Err(anyhow!("profile bundleのfile数が上限を超えています: {}", archive.len()));
	}
	let mut files = Vec::new();
	let mut total = 0_u64;
	for index in 0..archive.len() {
		let mut entry = archive.by_index(index)?;
		if entry.is_dir() {
			continue;
		}
		if entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) {
			return Err(anyhow!("profile bundleのsymlinkを拒否しました: {}", entry.name()));
		}
		let path = entry
			.enclosed_name()
			.ok_or_else(|| anyhow!("profile bundleに危険なpathがあります: {}", entry.name()))?;
		validate_bundle_path(&path)?;
		if entry.size() > MAX_FILE_BYTES {
			return Err(anyhow!("profile bundle内のfileが大きすぎます: {}", entry.name()));
		}
		total = total.saturating_add(entry.size());
		if total > MAX_TOTAL_BYTES {
			return Err(anyhow!("profile bundleの展開sizeが上限を超えています"));
		}
		let mut contents = Vec::with_capacity(entry.size() as usize);
		entry.read_to_end(&mut contents)?;
		files.push(BundleFile { path, contents });
	}
	let files = normalize_bundle_root(files)?;
	ensure_unique_paths(&files)?;
	Ok(files)
}

fn ensure_unique_paths(files: &[BundleFile]) -> Result<()> {
	let mut paths = HashSet::with_capacity(files.len());
	for file in files {
		// Windows normally resolves differently cased archive entries to the same file.
		#[cfg(windows)]
		let key = file.path.to_string_lossy().to_lowercase();
		#[cfg(not(windows))]
		let key = file.path.clone();
		if !paths.insert(key) {
			return Err(anyhow!("profile bundleに重複pathがあります: {}", file.path.display()));
		}
	}
	Ok(())
}

fn normalize_bundle_root(mut files: Vec<BundleFile>) -> Result<Vec<BundleFile>> {
	if files.iter().any(|file| file.path == Path::new(PROFILE_CONF_FILE)) {
		return Ok(files);
	}
	let prefix = files
		.first()
		.and_then(|file| file.path.components().next())
		.and_then(|component| match component {
			Component::Normal(value) => Some(value.to_os_string()),
			_ => None,
		})
		.ok_or_else(|| anyhow!("profile bundleが空です"))?;
	if !files
		.iter()
		.all(|file| file.path.components().next() == Some(Component::Normal(prefix.as_os_str())))
	{
		return Err(anyhow!("profile bundleのrootにconf.tomlがありません"));
	}
	for file in &mut files {
		file.path = file.path.strip_prefix(&prefix).unwrap().to_path_buf();
		validate_bundle_path(&file.path)?;
	}
	Ok(files)
}

fn validate_bundle_path(path: &Path) -> Result<()> {
	if path.as_os_str().is_empty() || path.is_absolute() || !path.components().all(|component| matches!(component, Component::Normal(_))) {
		return Err(anyhow!("profile bundle pathが不正です: {}", path.display()));
	}
	if path.components().next() == Some(Component::Normal(std::ffi::OsStr::new("credentials"))) {
		return Err(anyhow!("profile bundleにcredentialを含めることはできません: {}", path.display()));
	}
	let extension = path
		.extension()
		.and_then(|value| value.to_str())
		.unwrap_or_default()
		.to_ascii_lowercase();
	if matches!(
		extension.as_str(),
		"exe" | "dll" | "com" | "bat" | "cmd" | "ps1" | "msi" | "scr" | "lnk"
	) {
		return Err(anyhow!("profile bundleの実行形式を拒否しました: {}", path.display()));
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Write;
	use std::time::{SystemTime, UNIX_EPOCH};

	fn temp_root(label: &str) -> PathBuf {
		let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		std::env::temp_dir().join(format!("unvac-profile-bundle-{label}-{}-{nonce}", std::process::id()))
	}

	fn bundle(entries: &[(&str, &[u8])]) -> Vec<u8> {
		let cursor = Cursor::new(Vec::new());
		let mut writer = zip::ZipWriter::new(cursor);
		for (path, contents) in entries {
			writer.start_file(*path, zip::write::SimpleFileOptions::default()).unwrap();
			writer.write_all(contents).unwrap();
		}
		writer.finish().unwrap().into_inner()
	}

	#[test]
	fn imports_conf_and_related_files_atomically() {
		let root = temp_root("valid");
		let bytes = bundle(&[
			("workspace/conf.toml", b"workers = 1\nflowgraph_dir = \"flowgraph\"\n"),
			("workspace/flowgraph/main.flowgraph.toml", b"[meta]\ntitle = \"Imported\"\n"),
			("workspace/resources/data.tsv", b"a\tb\n"),
		]);
		let profile = import_profile_bundle_at(root.clone(), "imported", "Imported", &bytes).unwrap();
		assert!(profile.conf_path.is_file());
		let conf = std::fs::read_to_string(&profile.conf_path).unwrap();
		assert!(conf.contains("web_ui_address = \"127.0.0.1:46000\""));
		assert!(root.join("profiles/imported/flowgraph/main.flowgraph.toml").is_file());
		assert!(root.join("profiles/imported/resources/data.tsv").is_file());
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	#[ignore = "requires ZIP generated by gui/tests/prepareProfileFolderContract.ts; no devices"]
	fn imports_browser_generated_folder_bundle() {
		let input = std::env::var_os("UNVAC_TEST_FOLDER_IMPORT_ZIP").expect("set generated ZIP path");
		let bytes = std::fs::read(input).unwrap();
		let root = temp_root("browser-folder-contract");
		struct Cleanup(PathBuf);
		impl Drop for Cleanup { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
		let _cleanup = Cleanup(root.clone());
		let other = bundle(&[("conf.toml", b"workers = 2\n")]);
		let existing = import_profile_bundle_at(root.clone(), "existing", "Existing", &other).unwrap();
		let before = std::fs::read(&existing.conf_path).unwrap();
		let imported = import_profile_bundle_at(root.clone(), "imported", "Imported", &bytes).unwrap();
		let directory = imported.conf_path.parent().unwrap();
		assert_eq!(std::fs::read(directory.join("assets/payload.bin")).unwrap(), [0, 255, 42, 128]);
		let registry = crate::web_interface::control::flowgraph::purposes::read(&directory.join("graphs")).unwrap();
		assert_eq!(registry.document.purposes[0].name, "配信支援");
		assert_eq!(registry.document.purposes[0].roots, ["main"]);
		assert!(directory.join("graphs/main.flowgraph.toml").is_file());
		assert!(!directory.join("graphs/credentials").exists());
		assert!(!directory.join("graphs/.flowgraph-purposes.json.bak").exists());
		assert!(import_profile_bundle_at(root.clone(), "existing", "Overwrite", &bytes).is_err());
		assert_eq!(std::fs::read(&existing.conf_path).unwrap(), before);
		let exported = export_profile_bundle_at(root.clone(), "imported").unwrap();
		let reopened = import_profile_bundle_at(root, "copy", "Copy", &exported).unwrap();
		let copy = reopened.conf_path.parent().unwrap();
		for relative in ["assets/payload.bin", "graphs/main.flowgraph.toml", "graphs/.flowgraph-purposes.json"] {
			assert_eq!(std::fs::read(copy.join(relative)).unwrap(), std::fs::read(directory.join(relative)).unwrap());
		}
	}

	#[test]
	fn replaces_legacy_web_ui_address_for_managed_profile() {
		let root = temp_root("managed-port");
		let bytes = bundle(&[("conf.toml", b"workers = 1\nweb_ui_address = \"127.0.0.1:57000\"\n")]);
		let profile = import_profile_bundle_at(root.clone(), "imported", "Imported", &bytes).unwrap();
		let conf = std::fs::read_to_string(profile.conf_path).unwrap();
		assert!(conf.contains("web_ui_address = \"127.0.0.1:46000\""));
		assert!(!conf.contains("127.0.0.1:57000"));
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn rejects_traversal_and_does_not_create_profile() {
		let root = temp_root("traversal");
		let bytes = bundle(&[("conf.toml", b"workers = 1\n"), ("../outside.txt", b"no")]);
		assert!(import_profile_bundle_at(root.clone(), "bad", "Bad", &bytes).is_err());
		assert!(!root.join("profiles/bad").exists());
		let _ = std::fs::remove_dir_all(root);
	}

	#[test]
	fn rejects_invalid_conf_without_partial_profile() {
		let root = temp_root("invalid-conf");
		let bytes = bundle(&[("conf.toml", b"not = [valid")]);
		assert!(import_profile_bundle_at(root.clone(), "bad", "Bad", &bytes).is_err());
		assert!(!root.join("profiles/bad").exists());
		let _ = std::fs::remove_dir_all(root);
	}

	#[test]
	fn rejects_duplicate_paths() {
		let files = vec![
			BundleFile {
				path: PathBuf::from("data.txt"),
				contents: b"first".to_vec(),
			},
			BundleFile {
				path: PathBuf::from("data.txt"),
				contents: b"second".to_vec(),
			},
		];
		assert!(ensure_unique_paths(&files).is_err());
	}

	#[test]
	fn rejects_executable_files() {
		let root = temp_root("executable");
		let bytes = bundle(&[("conf.toml", b"workers = 1\n"), ("tools/setup.ps1", b"Write-Output no")]);
		assert!(import_profile_bundle_at(root.clone(), "bad", "Bad", &bytes).is_err());
		assert!(!root.join("profiles/bad").exists());
		let _ = std::fs::remove_dir_all(root);
	}

	#[cfg(windows)]
	#[test]
	fn rejects_case_aliases_without_publishing_partial_profile() {
		let root = temp_root("case-alias");
		let bytes = bundle(&[
			("conf.toml", b"workers = 1\n"),
			("assets/Name.txt", b"first"),
			("assets/name.txt", b"second"),
		]);
		let result = import_profile_bundle_at(root.clone(), "ambiguous", "Ambiguous", &bytes);
		let published = root.join("profiles/ambiguous").exists();
		let _ = std::fs::remove_dir_all(root);
		assert!(result.is_err(), "Windows aliases must not overwrite an earlier ZIP entry");
		assert!(!published);
	}

	#[test]
	fn rejects_profile_credentials() {
		let root = temp_root("credential");
		let bytes = bundle(&[("conf.toml", b"workers = 1\n"), ("credentials/openai-api-key.txt", b"secret")]);
		assert!(import_profile_bundle_at(root.clone(), "bad", "Bad", &bytes).is_err());
		assert!(!root.join("profiles/bad").exists());
		let _ = std::fs::remove_dir_all(root);
	}

	#[test]
	fn bundle_operations_share_profile_lock_and_roundtrip_assets() {
		let root = temp_root("bundle-lock-roundtrip");
		let bytes = bundle(&[
			("conf.toml", b"workers = 1\nflowgraph_dir = \"flowgraph\"\n"),
			("flowgraph/custom/main.flowgraph.toml", "# 手作りの構成\n[meta]\ntitle = \"Custom\"\n".as_bytes()),
			("assets/test.bin", &[0, 255, 42, 128]),
		]);
		import_profile_bundle_at(root.clone(), "source", "Source", &bytes).unwrap();
		let conf = std::fs::read(root.join("profiles/source/conf.toml")).unwrap();
		let lock = super::super::lifecycle::operation_lock(&root).unwrap();
		assert!(export_profile_bundle_at(root.clone(), "source").is_err());
		assert!(import_profile_bundle_at(root.clone(), "copy", "Copy", &bytes).is_err());
		assert!(!root.join("profiles/copy").exists());
		assert_eq!(std::fs::read(root.join("profiles/source/conf.toml")).unwrap(), conf);
		drop(lock);
		let exported = export_profile_bundle_at(root.clone(), "source").unwrap();
		import_profile_bundle_at(root.clone(), "copy", "Copy", &exported).unwrap();
		for relative in ["conf.toml", "flowgraph/custom/main.flowgraph.toml", "assets/test.bin"] {
			assert_eq!(std::fs::read(root.join("profiles/copy").join(relative)).unwrap(), std::fs::read(root.join("profiles/source").join(relative)).unwrap());
		}
		let copied = super::super::read_profile_manifest(&root.join("profiles/copy")).unwrap();
		assert_eq!(copied.id, "copy");
		assert_eq!(copied.label, "Copy");
		assert!(import_profile_bundle_at(root.clone(), "source", "Overwrite", &exported).is_err());
		assert_eq!(std::fs::read(root.join("profiles/source/conf.toml")).unwrap(), conf);
		std::fs::remove_dir_all(root).unwrap();
	}
	#[test]
	fn named_purposes_survive_profile_bundle_roundtrip() {
		let root = temp_root("named-purpose-roundtrip");
		let template = crate::profile_templates::materialize("streaming-tools", "workers = 1\n").unwrap();
		super::super::create_profile_at(root.clone(), "source", "Source", &template.conf, Some("streaming-tools"), &template.files).unwrap();
		let source_graphs = root.join("profiles/source/flowgraph");
		let before = crate::web_interface::control::flowgraph::purposes::read(&source_graphs).unwrap();
		std::fs::write(source_graphs.join(".flowgraph-purposes.json.bak"), b"private old catalogue").unwrap();
		let bytes = export_profile_bundle_at(root.clone(), "source").unwrap();
		let entries = read_bundle(&bytes).unwrap();
		assert!(entries.iter().any(|file| file.path == Path::new("flowgraph/.flowgraph-purposes.json")), "named purposes omitted from profile ZIP");
		assert!(!entries.iter().any(|file| file.path == Path::new("flowgraph/.flowgraph-purposes.json.bak")));
		import_profile_bundle_at(root.clone(), "copy", "Copy", &bytes).unwrap();
		let destination = root.join("profiles/copy/flowgraph");
		let after = crate::web_interface::control::flowgraph::purposes::read(&destination).unwrap();
		assert_eq!(before.document, after.document);
		for purpose in after.document.purposes {
			for fq in purpose.roots {
				let name = format!("{fq}.flowgraph.toml");
				assert_eq!(std::fs::read(source_graphs.join(&name)).unwrap(), std::fs::read(destination.join(name)).unwrap());
			}
		}
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn exports_only_runnable_profile_files_without_credentials_or_history() {
		let root = temp_root("export");
		let profile = root.join("profiles/live");
		std::fs::create_dir_all(profile.join("credentials")).unwrap();
		std::fs::create_dir_all(profile.join("flowgraph")).unwrap();
		std::fs::create_dir_all(profile.join("flowgraph.disabled")).unwrap();
		std::fs::write(profile.join("conf.toml"), "workers = 1\nflowgraph_dir = \"flowgraph\"\n").unwrap();
		std::fs::write(profile.join("profile.toml"), "id = \"live\"\n").unwrap();
		std::fs::write(profile.join("credentials/openai-api-key.txt"), "secret").unwrap();
		std::fs::write(profile.join("conf.toml.bak-20260801"), "old").unwrap();
		std::fs::write(profile.join("flowgraph/main.flowgraph.toml"), "[meta]\ntitle = \"Live\"\n").unwrap();
		std::fs::write(profile.join("flowgraph.disabled/old.flowgraph.toml"), "[meta]\ntitle = \"Old\"\n").unwrap();
		let bytes = export_profile_bundle_at(root.clone(), "live").unwrap();
		let files = read_bundle(&bytes).unwrap();
		let paths = files.iter().map(|file| file.path.as_path()).collect::<Vec<_>>();
		assert!(paths.contains(&Path::new("conf.toml")));
		assert!(paths.contains(&Path::new("flowgraph/main.flowgraph.toml")));
		assert!(!paths.iter().any(|path| path.starts_with("credentials")));
		assert!(!paths.iter().any(|path| path.starts_with("flowgraph.disabled")));
		assert!(!paths.contains(&Path::new("profile.toml")));
		assert!(!paths.contains(&Path::new("conf.toml.bak-20260801")));
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn export_rejects_inline_secrets() {
		let root = temp_root("export-secret");
		let profile = root.join("profiles/live");
		std::fs::create_dir_all(&profile).unwrap();
		std::fs::write(profile.join("conf.toml"), "workers = 1\n[service]\napi_key = \"secret\"\n").unwrap();
		let error = export_profile_bundle_at(root.clone(), "live").unwrap_err();
		assert!(error.to_string().contains("inline secret `api_key`"));
		std::fs::write(profile.join("conf.toml"), "workers = 1\n[control_api]\nbearer_token = \"secret\"\n").unwrap();
		assert!(export_profile_bundle_at(root.clone(), "live")
			.unwrap_err()
			.to_string()
			.contains("inline secret `bearer_token`"));
		std::fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn export_rejects_archive_too_large_to_reimport() {
		let root = temp_root("export-archive-limit");
		let profile = root.join("profiles/large");
		std::fs::create_dir_all(&profile).unwrap();
		std::fs::write(profile.join("conf.toml"), "workers = 1\n").unwrap();
		// Each file and the expanded total are valid. Only the compressed ZIP
		// crosses the importer limit; deterministic noise avoids compression.
		let mut state = 0x1234_5678_u32;
		let data: Vec<u8> = (0..12 * 1024 * 1024).map(|_| {
			state ^= state << 13;
			state ^= state >> 17;
			state ^= state << 5;
			state as u8
		}).collect();
		for index in 0..3 {
			std::fs::write(profile.join(format!("asset-{index}.bin")), &data).unwrap();
		}
		let result = export_profile_bundle_at(root.clone(), "large");
		let source_unchanged = std::fs::read(profile.join("asset-0.bin")).unwrap() == data;
		std::fs::remove_dir_all(root).unwrap();
		assert!(source_unchanged);
		assert!(result.unwrap_err().to_string().contains("32 MiB"));
	}
}

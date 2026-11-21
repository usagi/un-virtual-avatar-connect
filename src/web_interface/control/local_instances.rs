//! Local process directory. Never stores or forwards another instance's bearer token.
use actix_web::{get, HttpResponse, Responder};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

#[derive(Debug, Serialize, Deserialize)]
pub struct LocalInstance {
	pub pid: u32,
	pub started_at: u64,
	pub conf_path: PathBuf,
	pub gui_url: String,
	pub executable: PathBuf,
}

fn directory() -> Result<PathBuf> {
	Ok(crate::profile_store::user_config_root()?.join("local-instances"))
}

pub fn register(conf_path: &Path, address: std::net::SocketAddr) -> Result<()> {
	let pid = std::process::id();
	let mut system = System::new();
	system.refresh_processes_specifics(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true, ProcessRefreshKind::nothing());
	let process = system.process(Pid::from_u32(pid)).context("current process unavailable")?;
	let host = if address.ip().is_unspecified() {
		if address.is_ipv4() {
			"127.0.0.1".to_string()
		} else {
			"[::1]".to_string()
		}
	} else if address.is_ipv6() {
		format!("[{}]", address.ip())
	} else {
		address.ip().to_string()
	};
	let entry = LocalInstance {
		pid,
		started_at: process.start_time(),
		conf_path: std::fs::canonicalize(conf_path)?,
		gui_url: format!("http://{host}:{}/gui/", address.port()),
		executable: std::env::current_exe()?,
	};
	let dir = directory()?;
	std::fs::create_dir_all(&dir)?;
	std::fs::write(dir.join(format!("{pid}.json")), serde_json::to_vec(&entry)?)?;
	Ok(())
}

fn matches_process(entry: &LocalInstance, started_at: u64, executable: Option<&Path>) -> bool {
	entry.started_at == started_at && executable.is_some_and(|path| path == entry.executable)
}

pub fn list() -> Result<Vec<LocalInstance>> {
	let dir = directory()?;
	if !dir.exists() {
		return Ok(Vec::new());
	}
	let mut system = System::new();
	system.refresh_processes_specifics(
		ProcessesToUpdate::All,
		true,
		ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always),
	);
	let mut entries = Vec::new();
	for file in std::fs::read_dir(dir)? {
		let file = file?;
		if file.path().extension().and_then(|s| s.to_str()) != Some("json") {
			continue;
		}
		let Ok(bytes) = std::fs::read(file.path()) else {
			continue;
		};
		let Ok(entry) = serde_json::from_slice::<LocalInstance>(&bytes) else {
			continue;
		};
		if system
			.process(Pid::from_u32(entry.pid))
			.is_some_and(|p| matches_process(&entry, p.start_time(), p.exe()))
		{
			entries.push(entry);
		}
	}
	entries.sort_by_key(|entry| entry.pid);
	Ok(entries)
}

#[get("/desktop/instances")]
pub async fn get_instances() -> impl Responder {
	match list() {
		Ok(instances) => {
			let profiles = crate::profile_store::snapshot();
			let entries: Vec<_> = instances.iter().map(|entry| {
				let profile = profiles.as_ref().ok().and_then(|snapshot| matching_profile(&entry.conf_path, &snapshot.profiles));
				let mut value = serde_json::to_value(entry).expect("LocalInstance serialization");
				value["profile"] = profile.map(|p| serde_json::json!({"id":p.id,"label":p.label,"next_start":p.is_active})).unwrap_or(serde_json::Value::Null);
				value
			}).collect();
			HttpResponse::Ok().json(serde_json::json!({"current_pid": std::process::id(), "instances": entries, "profile_lookup_ok": profiles.is_ok()}))
		},
		Err(error) => HttpResponse::InternalServerError().json(serde_json::json!({"error":error.to_string()})),
	}
}

fn matching_profile<'a>(path: &Path, profiles: &'a [crate::profile_store::StoredProfile]) -> Option<&'a crate::profile_store::StoredProfile> {
	let canonical = std::fs::canonicalize(path).ok()?;
	let mut matches = profiles.iter().filter(|p| std::fs::canonicalize(&p.conf_path).ok().as_ref() == Some(&canonical));
	let first = matches.next()?;
	// Ambiguous aliases must not silently select a saved profile.
	if matches.next().is_some() { None } else { Some(first) }
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn profile_identity_uses_existing_paths_and_rejects_ambiguity() {
		let root = std::env::temp_dir().join(format!("vac-instance-profile-{}", rand::random::<u64>()));
		std::fs::create_dir_all(&root).unwrap();
		let path = root.join("conf.toml");
		std::fs::write(&path, "").unwrap();
		let profile = crate::profile_store::StoredProfile { id:"a".into(), label:"Same label".into(), conf_path:path.clone(), is_active:true, template_id:None };
		assert_eq!(matching_profile(&root.join(".").join("conf.toml"), &[profile.clone()]).unwrap().id, "a");
		assert!(matching_profile(&root.join("missing.toml"), &[profile.clone()]).is_none());
		let mut alias = profile.clone(); alias.id = "b".into();
		assert!(matching_profile(&path, &[profile, alias]).is_none());
	}
	#[test]
	fn reused_pid_and_different_binary_are_not_the_registered_instance() {
		let entry = LocalInstance {
			pid: 1,
			started_at: 10,
			conf_path: "a.toml".into(),
			gui_url: "http://127.0.0.1:1/gui/".into(),
			executable: "vac.exe".into(),
		};
		assert!(matches_process(&entry, 10, Some(Path::new("vac.exe"))));
		assert!(!matches_process(&entry, 11, Some(Path::new("vac.exe"))));
		assert!(!matches_process(&entry, 10, Some(Path::new("other.exe"))));
		assert!(!matches_process(&entry, 10, None));
	}
}

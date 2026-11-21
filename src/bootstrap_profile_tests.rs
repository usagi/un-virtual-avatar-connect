use super::*;
use clap::{parser::ValueSource, CommandFactory, FromArgMatches};
use std::{fs, path::PathBuf, process::Command};

// Run cwd-dependent bootstrap logic in fresh children, never in the test runner.
#[test]
fn desktop_profile_selection_in_fresh_process() {
	const CHILD: &str = "UNVAC_PROFILE_SELECTION_CHILD";
	if let Ok(case) = std::env::var(CHILD) {
		let root = PathBuf::from(std::env::var_os("UN_VAC_CONFIG_DIR").unwrap());
		let before = std::env::current_dir().unwrap();
		let argv = if case == "explicit" {
			vec!["vac", "explicit.toml"]
		} else {
			vec!["vac"]
		};
		let matches = Args::command().try_get_matches_from(argv).unwrap();
		let mut args = Args::from_arg_matches(&matches).unwrap();
		args.conf_is_explicit = matches.value_source("CONF") == Some(ValueSource::CommandLine);
		prepare_desktop_conf(&mut args).unwrap();
		if case == "explicit" {
			assert_eq!(args.conf, "explicit.toml");
			assert_eq!(std::env::current_dir().unwrap(), before);
			assert!(!root.exists(), "explicit config must not initialize managed storage");
		} else {
			let profile = root.join("profiles/partner");
			assert_eq!(
				fs::canonicalize(&args.conf).unwrap(),
				fs::canonicalize(profile.join("conf.toml")).unwrap()
			);
			assert_eq!(
				fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
				fs::canonicalize(profile).unwrap()
			);
			let settings: crate::profile_store::DesktopSettings =
				toml::from_str(&fs::read_to_string(root.join("settings.toml")).unwrap()).unwrap();
			assert_eq!(settings.active_profile_id, "partner");
			assert!(settings.onboarding_completed);
			assert!(settings.voice_input_muted);
			assert!(!root.join("profiles/default").exists());
		}
		return;
	}
	let nonce = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap()
		.as_nanos();
	let fixture = std::env::temp_dir().join(format!("unvac-bootstrap-{}-{nonce}", std::process::id()));
	fs::create_dir(&fixture).unwrap();
	let managed = fixture.join("managed");
	for id in ["partner", "broadcaster"] {
		let dir = managed.join("profiles").join(id);
		fs::create_dir_all(&dir).unwrap();
		fs::write(dir.join("conf.toml"), format!("# {id}\nworkers = 1\n")).unwrap();
	}
	fs::write(
		managed.join("settings.toml"),
		"schemaVersion = 1\nactiveProfileId = 'partner'\nonboardingCompleted = true\nvoiceInputMuted = true\n",
	)
	.unwrap();
	let originals: Vec<_> = ["partner", "broadcaster"]
		.into_iter()
		.map(|id| {
			let file = managed.join("profiles").join(id).join("conf.toml");
			let bytes = fs::read(&file).unwrap();
			(file, bytes)
		})
		.collect();
	// Repeat selection in a fresh process to cover reopening persisted selection.
	for case in ["managed", "managed", "explicit"] {
		let store = if case == "explicit" {
			fixture.join("unused-store")
		} else {
			managed.clone()
		};
		let output = Command::new(std::env::current_exe().unwrap())
			.args([
				"--exact",
				"bootstrap::profile_selection_tests::desktop_profile_selection_in_fresh_process",
				"--nocapture",
			])
			.env(CHILD, case)
			.env("UN_VAC_CONFIG_DIR", store)
			.current_dir(&fixture)
			.output()
			.unwrap();
		assert!(
			output.status.success(),
			"child failed: {}\n{}",
			String::from_utf8_lossy(&output.stdout),
			String::from_utf8_lossy(&output.stderr)
		);
		assert!(
			String::from_utf8_lossy(&output.stdout).contains("1 passed"),
			"child test must execute"
		);
		for (file, bytes) in &originals {
			assert_eq!(&fs::read(file).unwrap(), bytes);
		}
	}
	fs::remove_dir_all(fixture).unwrap();
}

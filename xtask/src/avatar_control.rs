use std::{
	collections::BTreeMap,
	fs,
	io::Write,
	path::{Path, PathBuf},
	process::{Command, Stdio},
	thread,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use serde_json::{json, Value};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use vac_motion::avatar_intent::{avatar_intent_neutral_unmf, avatar_intent_to_unmf, AvatarIntentMapperOptions, AvatarIntentRequest};

use super::{repo_root, run_command, Result, DEFAULT_DESKTOP_BASE_URL as DEFAULT_BASE_URL};

const DEFAULT_BASE_KEY: &str = "un-motion/frame/kaltsit-ai";
const DEFAULT_LINK_ID: &str = "kaltsit_ai";
const DEFAULT_PROFILE_HINT: &str = "Kaltsit";
const DEFAULT_PRESET_FILE: &str = "avatar-reaction-presets.example.json";
const FIXTURES: &[&str] = &[
	"flowgraph.example/ai-avatar-unmfz",
	"flowgraph.example/ai-avatar-unmfz-invalid",
	"flowgraph.example/twitch-command-to-avatar-expression",
	"flowgraph.example/channel-point-to-avatar-reaction",
	"flowgraph.example/voice-observe-to-avatar-nod",
];

pub(crate) fn run(args: Vec<String>) -> Result<()> {
	let Some((subcommand, rest)) = args.split_first() else {
		print_help();
		return Ok(());
	};
	let args = CliArgs::new(rest);
	if help_requested(&args) {
		print_help();
		return Ok(());
	}
	match subcommand.as_str() {
		"prepare" => prepare(&args),
		"start" => start(&args),
		"preflight" => preflight(&args),
		"audit" => audit(&args),
		"smoke" => smoke(),
		"send" => send(&args),
		"preview-frame" => preview_frame(&args),
		"replay-frame" => replay_frame(&args),
		"send-sequence" => send_sequence(&args),
		"reaction" => reaction(&args),
		"live-smoke" => live_smoke(&args),
		"find-executables" => find_executables_command(&args),
		"detect" => detect_command(&args),
		"renderer-status" => renderer_status(&args),
		"renderer-screenshot" => renderer_screenshot(&args),
		"wait-renderer" => wait_renderer(&args),
		"start-supervisor" => start_supervisor(&args),
		"new-live-note" => new_live_note(&args).map(|_| ()),
		"start-live-test" => start_live_test(&args),
		"-h" | "--help" | "help" => {
			print_help();
			Ok(())
		}
		other => Err(format!("unknown avatar-control subcommand: {other}").into()),
	}
}

fn help_requested(args: &CliArgs<'_>) -> bool {
	args.flag("-h") || args.flag("--help") || args.items.first().is_some_and(|item| item == "help")
}

#[derive(Debug)]
struct CliArgs<'a> {
	items: &'a [String],
}

impl<'a> CliArgs<'a> {
	fn new(items: &'a [String]) -> Self {
		Self { items }
	}

	fn flag(&self, name: &str) -> bool {
		self.items.iter().any(|item| item == name)
	}

	fn value(&self, name: &str) -> Option<&str> {
		self.items
			.iter()
			.position(|item| item == name)
			.and_then(|index| self.items.get(index + 1))
			.map(String::as_str)
	}

	fn values(&self, name: &str) -> Vec<&str> {
		self.items
			.iter()
			.enumerate()
			.filter_map(|(index, item)| (item == name).then(|| self.items.get(index + 1)).flatten())
			.map(String::as_str)
			.collect()
	}

	fn value_or<'b>(&'b self, name: &str, default: &'b str) -> &'b str {
		self.value(name).unwrap_or(default)
	}

	fn parse_u64(&self, name: &str, default: u64) -> Result<u64> {
		match self.value(name) {
			Some(value) => Ok(value.parse().map_err(|_| format!("{name} must be an unsigned integer: {value}"))?),
			None => Ok(default),
		}
	}
}

#[derive(Debug, Clone)]
struct LivePaths {
	conf: PathBuf,
	flowgraph_dir: PathBuf,
}

fn live_paths(root: &Path, args: &CliArgs<'_>, readiness_defaults: bool) -> LivePaths {
	let default_conf = if readiness_defaults {
		"target/avatar-ai-control-readiness/conf.avatar-ai-control.local.toml"
	} else {
		"conf.avatar-ai-control.local.toml"
	};
	let default_flowgraph = if readiness_defaults {
		"target/avatar-ai-control-readiness/flowgraph.avatar-ai-control.local"
	} else {
		"flowgraph.avatar-ai-control.local"
	};
	LivePaths {
		conf: resolve_path(root, args.value_or("--conf", default_conf)),
		flowgraph_dir: resolve_path(root, args.value_or("--flowgraph-dir", default_flowgraph)),
	}
}

fn prepare(args: &CliArgs<'_>) -> Result<()> {
	let root = repo_root()?;
	let paths = live_paths(&root, args, false);
	prepare_with_paths(args, &root, &paths, false)
}

fn prepare_with_paths(args: &CliArgs<'_>, root: &Path, paths: &LivePaths, quiet: bool) -> Result<()> {
	let base_key = args.value_or("--base-key", DEFAULT_BASE_KEY);
	let link_id = args.value_or("--link-id", DEFAULT_LINK_ID);
	let profile_hint = args.value_or("--profile-hint", DEFAULT_PROFILE_HINT);
	validate_replacement("base key", base_key, false)?;
	validate_replacement("link id", link_id, true)?;
	validate_replacement("profile hint", profile_hint, false)?;

	let force = args.flag("--force");
	assert_can_write(&paths.conf, force)?;
	assert_can_write(&paths.flowgraph_dir, force)?;
	if paths.flowgraph_dir.exists() {
		ensure_safe_remove_dir(&paths.flowgraph_dir, root)?;
		fs::remove_dir_all(&paths.flowgraph_dir)?;
	}
	fs::create_dir_all(&paths.flowgraph_dir)?;

	let streaming = args.flag("--streaming-persona");
	let conf_template_name = if streaming {
		"conf.example-streaming-ai-avatar.toml"
	} else {
		"conf.example-avatar-ai-control.toml"
	};
	let conf_template = root.join(conf_template_name);
	let flowgraph_template = root.join("flowgraph.example/ai-avatar-unmfz/main.flowgraph.toml");
	let flowgraph_main = paths.flowgraph_dir.join("main.flowgraph.toml");

	let publish_value = if args.flag("--enable-publish") { "false" } else { "true" };
	let flowgraph_text = fs::read_to_string(flowgraph_template)?
		.replace(
			"properties.value = \"un-motion/frame/kaltsit-ai\"",
			&format!("properties.value = \"{base_key}\""),
		)
		.replace("properties.value = true", &format!("properties.value = {publish_value}"));
	fs::write(&flowgraph_main, flowgraph_text)?;

	let flowgraph_setting = path_for_toml(root, &paths.flowgraph_dir);
	let conf_text = fs::read_to_string(conf_template)?
		.replace(
			"flowgraph_dir = \"flowgraph.example/ai-avatar-unmfz\"",
			&format!("flowgraph_dir = \"{flowgraph_setting}\""),
		)
		.replace("[un_avatar.links.kaltsit_ai]", &format!("[un_avatar.links.{link_id}]"))
		.replace("[un_avatar.links.kaltsit_ai.unmfz]", &format!("[un_avatar.links.{link_id}.unmfz]"))
		.replace("`kaltsit_ai`", &format!("`{link_id}`"))
		.replace("profile_hint = \"Kaltsit\"", &format!("profile_hint = \"{profile_hint}\""))
		.replace(
			"base_key_expr = \"un-motion/frame/kaltsit-ai\"",
			&format!("base_key_expr = \"{base_key}\""),
		);
	if let Some(parent) = paths.conf.parent() {
		fs::create_dir_all(parent)?;
	}
	fs::write(&paths.conf, conf_text)?;

	if !quiet {
		println!("Wrote {}", paths.conf.display());
		println!("Wrote {}", flowgraph_main.display());
		println!("Conf template: {conf_template_name}");
		println!("UNMF/Z base key: {base_key}");
		println!("dry_run.value: {publish_value}");
	}
	Ok(())
}

fn start(args: &CliArgs<'_>) -> Result<()> {
	let root = repo_root()?;
	let paths = live_paths(&root, args, false);
	if args.flag("--prepare") {
		prepare_with_paths(args, &root, &paths, false)?;
	}
	validate_live_paths(&paths)?;
	if !args.flag("--skip-load-check") {
		load_check(&root, &paths.flowgraph_dir, false)?;
	}
	let link_id = args.value_or("--link-id", DEFAULT_LINK_ID);
	println!("Ready for avatar AI control live run.");
	println!("Conf: {}", paths.conf.display());
	println!("Flowgraph: {}", paths.flowgraph_dir.display());
	println!("UNMF/Z base key: {}", args.value_or("--base-key", DEFAULT_BASE_KEY));
	println!("AvatarLink id: {link_id}");
	println!("Smoke commands:");
	println!("  cargo xtask avatar-control send --preset smile --link-id {link_id}");
	println!("  cargo xtask avatar-control send-sequence --sequence smile-then-neutral --link-id {link_id}");
	if args.flag("--validate-only") {
		return Ok(());
	}
	let mut command = Command::new("cargo");
	command
		.current_dir(root)
		.arg("run")
		.arg("--bin")
		.arg("virtual-avatar-connect-cli")
		.arg("--")
		.arg(&paths.conf);
	run_command(&mut command)
}

fn load_check(root: &Path, flowgraph_dir: &Path, quiet: bool) -> Result<()> {
	let mut command = Command::new("cargo");
	command
		.current_dir(root)
		.arg("run")
		.arg("--bin")
		.arg("virtual-avatar-connect-cli")
		.arg("--")
		.arg("--flowgraph-load-dir")
		.arg(flowgraph_dir)
		.arg("--flowgraph-test-json");
	if quiet {
		run_command_quiet(&mut command)
	} else {
		run_command(&mut command)
	}
}

fn smoke() -> Result<()> {
	let root = repo_root()?;
	for fixture in FIXTURES {
		println!("==> {fixture}");
		run_fixture(&root, fixture, false)?;
	}
	Ok(())
}

fn run_fixture(root: &Path, fixture: &str, quiet: bool) -> Result<()> {
	let mut command = Command::new("cargo");
	command
		.current_dir(root)
		.arg("run")
		.arg("--bin")
		.arg("virtual-avatar-connect-cli")
		.arg("--")
		.arg("--flowgraph-test-dir")
		.arg(fixture);
	if quiet {
		run_command_quiet(&mut command)
	} else {
		run_command(&mut command)
	}
}

fn send(args: &CliArgs<'_>) -> Result<()> {
	let preset = args.value_or("--preset", "smile");
	let mut request = intent_request(args.value_or("--link-id", DEFAULT_LINK_ID), preset)?;
	if let Some(duration_ms) = args.value("--duration-ms") {
		let duration_ms = duration_ms
			.parse::<u64>()
			.map_err(|_| format!("--duration-ms must be an unsigned integer: {duration_ms}"))?;
		override_intent_duration(&mut request, duration_ms)?;
	}
	post_intent(
		args.value_or("--base-url", DEFAULT_BASE_URL),
		args.value_or("--token", ""),
		args.value_or("--channel", "avatar_intent"),
		preset,
		&request,
		args.flag("--print-only"),
		"xtask.avatar_intent_smoke",
		None,
	)
}

fn preview_frame(args: &CliArgs<'_>) -> Result<()> {
	let preset = args.value_or("--preset", "yaw-right");
	let mut request = intent_request(args.value_or("--link-id", DEFAULT_LINK_ID), preset)?;
	if let Some(duration_ms) = args.value("--duration-ms") {
		let duration_ms = duration_ms
			.parse::<u64>()
			.map_err(|_| format!("--duration-ms must be an unsigned integer: {duration_ms}"))?;
		override_intent_duration(&mut request, duration_ms)?;
	}
	let request: AvatarIntentRequest = serde_json::from_value(request)?;
	let sequence = args.parse_u64("--sequence", 0)?;
	let frame = if args.flag("--terminal") {
		avatar_intent_neutral_unmf(&request, sequence)
	} else {
		avatar_intent_to_unmf(
			&request,
			&AvatarIntentMapperOptions {
				sequence,
				elapsed_ms: args.parse_u64("--elapsed-ms", 1000)?,
				..AvatarIntentMapperOptions::default()
			},
		)
	}
	.map_err(|error| format!("avatar intent mapping failed: {error}"))?;
	let json = format!("{}\n", serde_json::to_string_pretty(&frame)?);
	if let Some(output) = args.value("--output") {
		let path = resolve_path(&repo_root()?, output);
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent)?;
		}
		fs::write(&path, json)?;
		println!("Wrote AvatarIntent frame preview to {}", path.display());
	} else {
		print!("{json}");
	}
	Ok(())
}

fn replay_frame(args: &CliArgs<'_>) -> Result<()> {
	use zenoh::Wait as _;

	let input = args.value("--input").ok_or("missing --input UNMotionFrame JSON path")?;
	let path = resolve_path(&repo_root()?, input);
	let payload = read_unmfz_payload(&path)?;
	let release = args
		.value("--release-input")
		.map(|input| {
			let path = resolve_path(&repo_root()?, input);
			let payload = read_unmfz_payload(&path)?;
			Ok::<_, Box<dyn std::error::Error>>((path, payload))
		})
		.transpose()?;
	let key = unmf_publish_key(args.value_or("--base-key", DEFAULT_BASE_KEY), args.parse_u64("--schema-major", 1)?);
	let count = args.parse_u64("--count", 30)?.clamp(1, 3_000);
	let release_count = args.parse_u64("--release-count", 30)?.clamp(1, 3_000);
	let interval = Duration::from_millis(args.parse_u64("--interval-ms", 33)?.max(1));
	if args.flag("--dry-run") {
		println!(
			"UNMF/Z replay dry-run: input={} key={} frames={} interval_ms={} bytes={} release={}",
			path.display(),
			key,
			count,
			interval.as_millis(),
			payload.len(),
			release
				.as_ref()
				.map(|(path, payload)| format!("{} ({} frames, {} bytes)", path.display(), release_count, payload.len()))
				.unwrap_or_else(|| "none".to_string())
		);
		return Ok(());
	}

	let session = zenoh::open(zenoh::Config::default())
		.wait()
		.map_err(|error| format!("open Zenoh for UNMF/Z replay: {error}"))?;
	let publisher = session
		.declare_publisher(&key)
		.wait()
		.map_err(|error| format!("declare UNMF/Z replay publisher {key}: {error}"))?;
	thread::sleep(Duration::from_millis(args.parse_u64("--discovery-wait-ms", 750)?));
	for index in 0..count {
		publisher
			.put(payload.clone())
			.wait()
			.map_err(|error| format!("publish UNMF/Z replay frame {}/{}: {error}", index + 1, count))?;
		if index + 1 < count {
			thread::sleep(interval);
		}
	}
	println!("Replayed {count} UNMotionFrame(s) from {} to {key}", path.display());
	if let Some((release_path, release_payload)) = release {
		for index in 0..release_count {
			publisher
				.put(release_payload.clone())
				.wait()
				.map_err(|error| format!("publish UNMF/Z release frame {}/{}: {error}", index + 1, release_count))?;
			if index + 1 < release_count {
				thread::sleep(interval);
			}
		}
		println!(
			"Released with {release_count} UNMotionFrame(s) from {} to {key}",
			release_path.display()
		);
	}
	Ok(())
}

fn read_unmfz_payload(path: &Path) -> Result<Vec<u8>> {
	let value: Value = serde_json::from_slice(&fs::read(path)?)?;
	let frame = vac_motion::unmf::unmf_from_json_value(&value).map_err(|error| format!("read {}: {error}", path.display()))?;
	Ok(vac_motion::unmf::encode_unmfz_payload(&frame)?)
}

fn unmf_publish_key(base_key: &str, schema_major: u64) -> String {
	let base_key = base_key.trim().trim_end_matches('/');
	let suffix = format!("/v{schema_major}");
	if base_key.ends_with(&suffix) {
		base_key.to_string()
	} else {
		format!("{base_key}{suffix}")
	}
}

fn override_intent_duration(request: &mut Value, duration_ms: u64) -> Result<()> {
	let intents = request
		.get_mut("intents")
		.and_then(Value::as_array_mut)
		.ok_or("AvatarIntent request has no intents array")?;
	for intent in intents {
		intent["duration_ms"] = Value::from(duration_ms);
	}
	Ok(())
}

fn send_sequence(args: &CliArgs<'_>) -> Result<()> {
	let sequence = args.value_or("--sequence", "smile-then-neutral");
	let presets: &[&str] = match sequence {
		"smile-then-neutral" | "smile_then_neutral" => &["smile", "neutral"],
		"concerned-then-neutral" | "concerned_then_neutral" => &["concerned-shake", "neutral"],
		"all" => &["smile", "neutral", "nod", "concerned-shake", "neutral"],
		other => return Err(format!("unknown avatar intent sequence: {other}").into()),
	};
	let delay = Duration::from_millis(args.parse_u64("--delay-ms", 1200)?);
	for (index, preset) in presets.iter().enumerate() {
		println!("==> {preset}");
		let request = intent_request(args.value_or("--link-id", DEFAULT_LINK_ID), preset)?;
		post_intent(
			args.value_or("--base-url", DEFAULT_BASE_URL),
			args.value_or("--token", ""),
			args.value_or("--channel", "avatar_intent"),
			preset,
			&request,
			args.flag("--print-only"),
			"xtask.avatar_intent_smoke",
			None,
		)?;
		if index + 1 < presets.len() && !args.flag("--print-only") {
			thread::sleep(delay);
		}
	}
	Ok(())
}

fn intent_request(link_id: &str, preset: &str) -> Result<Value> {
	let intents = match preset {
		"smile" => json!([{"kind":"expression","name":"smile","intensity":0.45,"duration_ms":1000,"curve":"hold"}]),
		"nod" => json!([{"kind":"head_nod","count":1,"intensity":0.35,"duration_ms":700,"curve":"ease_out"}]),
		"pitch-down" | "pitch_down" => json!([{"kind":"look_at","pitch_deg":20.0,"yaw_deg":0.0,"duration_ms":2000}]),
		"yaw-right" | "yaw_right" => json!([{"kind":"look_at","pitch_deg":0.0,"yaw_deg":20.0,"duration_ms":2000}]),
		"concerned-shake" | "concerned_shake" => json!([
			{"kind":"expression","name":"concerned","intensity":0.4,"duration_ms":1200,"curve":"hold"},
			{"kind":"head_shake","count":1,"intensity":0.25,"duration_ms":800,"curve":"ease_out"}
		]),
		"neutral" => json!([{"kind":"expression","name":"neutral","intensity":1.0,"duration_ms":300,"curve":"linear"}]),
		"invalid" => json!([{"kind":"expression","name":"unknown_expression","intensity":0.5,"duration_ms":1000,"curve":"hold"}]),
		other => return Err(format!("unknown avatar intent preset: {other}").into()),
	};
	Ok(json!({"target_link_id": link_id, "intents": intents}))
}

fn post_intent(
	base_url: &str,
	token: &str,
	channel: &str,
	preset: &str,
	request: &Value,
	print_only: bool,
	source_kind: &str,
	preset_schema: Option<&str>,
) -> Result<()> {
	let target = request.get("target_link_id").and_then(Value::as_str).unwrap_or_default();
	let mut meta = json!({
		"preset": preset,
		"target_link_id": target,
		"schema": "unvac.avatar_intent.v1"
	});
	if let Some(schema) = preset_schema {
		meta["preset_schema"] = Value::String(schema.to_string());
	}
	let body = json!({
		"channel": channel,
		"content": serde_json::to_string(request)?,
		"is_final": true,
		"source": {"kind": source_kind, "actor": "local"},
		"meta": meta
	});
	let uri = format!("{}/api/v1/control/ingress", base_url.trim_end_matches('/'));
	if print_only {
		println!("POST {uri}");
		println!("{}", serde_json::to_string_pretty(&body)?);
		return Ok(());
	}
	let text = serde_json::to_string(&body)?;
	let mut request_builder = ureq::post(&uri).header("Content-Type", "application/json; charset=utf-8");
	if !token.trim().is_empty() {
		request_builder = request_builder.header("Authorization", &format!("Bearer {token}"));
	}
	let response = request_builder.send(text.as_bytes()).map_err(|e| format!("POST {uri}: {e}"))?;
	let status = response.status();
	let mut reader = response.into_parts().1.into_reader();
	let mut response_text = String::new();
	use std::io::Read as _;
	reader.read_to_string(&mut response_text)?;
	println!("HTTP {status}");
	if !response_text.trim().is_empty() {
		println!("{response_text}");
	}
	Ok(())
}

#[derive(Debug)]
struct ReactionConfig {
	channel: String,
	target_link_id: String,
	presets: BTreeMap<String, Value>,
}

fn load_reactions(path: &Path) -> Result<ReactionConfig> {
	let root: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
	let schema = root.get("schema").and_then(Value::as_str).unwrap_or_default();
	if schema != "unvac.avatar_reaction_presets.v1" {
		return Err(format!("unsupported preset schema: {schema}").into());
	}
	let presets = root
		.get("presets")
		.and_then(Value::as_object)
		.ok_or("preset file must contain object `presets`")?
		.iter()
		.map(|(name, value)| (name.clone(), value.clone()))
		.collect();
	Ok(ReactionConfig {
		channel: root.get("channel").and_then(Value::as_str).unwrap_or("avatar_intent").to_string(),
		target_link_id: root.get("target_link_id").and_then(Value::as_str).unwrap_or_default().to_string(),
		presets,
	})
}

fn validate_reaction(name: &str, entry: &Value, target: &str) -> Result<Value> {
	let intents = entry
		.get("intents")
		.cloned()
		.ok_or_else(|| format!("preset `{name}` has no intents"))?;
	let request = json!({"target_link_id": target, "intents": intents});
	let typed: AvatarIntentRequest = serde_json::from_value(request.clone())?;
	avatar_intent_to_unmf(&typed, &AvatarIntentMapperOptions::default()).map_err(|e| format!("preset `{name}`: {e}"))?;
	Ok(request)
}

fn reaction(args: &CliArgs<'_>) -> Result<()> {
	let root = repo_root()?;
	let path = resolve_path(&root, args.value_or("--file", DEFAULT_PRESET_FILE));
	let config = load_reactions(&path)?;
	if args.flag("--list") {
		println!("Avatar reaction presets:");
		for (name, entry) in &config.presets {
			println!("- {name}: {}", entry.get("description").and_then(Value::as_str).unwrap_or_default());
		}
		return Ok(());
	}
	let target = args.value("--link-id").unwrap_or(&config.target_link_id);
	if target.trim().is_empty() {
		return Err("target_link_id is required in the preset file or --link-id".into());
	}
	if args.flag("--validate-all") {
		for (name, entry) in &config.presets {
			validate_reaction(name, entry, target)?;
		}
		println!("Avatar reaction preset validation OK: {}", path.display());
		return Ok(());
	}
	let name = args.value_or("--preset", "chat_agree");
	let entry = config
		.presets
		.get(name)
		.ok_or_else(|| format!("unknown avatar reaction preset: {name}"))?;
	let request = validate_reaction(name, entry, target)?;
	if args.flag("--validate-only") {
		println!("Avatar reaction preset validation OK: {name}");
		return Ok(());
	}
	post_intent(
		args.value_or("--base-url", DEFAULT_BASE_URL),
		args.value_or("--token", ""),
		args.value("--channel").unwrap_or(&config.channel),
		name,
		&request,
		args.flag("--print-only"),
		"xtask.avatar_reaction_preset",
		Some("unvac.avatar_reaction_presets.v1"),
	)
}

#[derive(Debug, Clone, Serialize)]
struct ExecutableCandidate {
	kind: String,
	path: String,
	length: u64,
	last_write_unix_secs: u64,
	search_root: String,
}

fn search_roots(root: &Path, args: &CliArgs<'_>) -> Vec<PathBuf> {
	let explicit = args.values("--search-root");
	let values = if explicit.is_empty() {
		vec!["../un-avatar", "../un-avatar-1.0.0-reference-only"]
	} else {
		explicit
	};
	values.into_iter().map(|path| resolve_path(root, path)).collect()
}

fn find_executables(root: &Path, args: &CliArgs<'_>) -> Result<Vec<ExecutableCandidate>> {
	let mut found = Vec::new();
	for search_root in search_roots(root, args) {
		if search_root.exists() {
			visit_executables(&search_root, &search_root, &mut found)?;
		}
	}
	found.sort_by(|a, b| {
		b.last_write_unix_secs
			.cmp(&a.last_write_unix_secs)
			.then_with(|| a.path.cmp(&b.path))
	});
	if !args.flag("--all") {
		let mut selected = Vec::new();
		for kind in ["renderer", "supervisor"] {
			if let Some(item) = found.iter().find(|item| item.kind == kind) {
				selected.push(item.clone());
			}
		}
		selected.sort_by(|a, b| a.kind.cmp(&b.kind));
		return Ok(selected);
	}
	Ok(found)
}

fn visit_executables(dir: &Path, search_root: &Path, out: &mut Vec<ExecutableCandidate>) -> Result<()> {
	if dir.file_name().is_some_and(|name| name.eq_ignore_ascii_case("build")) {
		return Ok(());
	}
	for entry in match fs::read_dir(dir) {
		Ok(entries) => entries,
		Err(_) => return Ok(()),
	} {
		let entry = match entry {
			Ok(entry) => entry,
			Err(_) => continue,
		};
		let path = entry.path();
		if path.is_dir() {
			visit_executables(&path, search_root, out)?;
			continue;
		}
		let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
			continue;
		};
		let executable_name = name.strip_suffix(std::env::consts::EXE_SUFFIX).unwrap_or(name);
		let kind = if executable_name.eq_ignore_ascii_case("un-avatar-renderer") {
			"renderer"
		} else if executable_name.eq_ignore_ascii_case("un-avatar-supervisor") {
			"supervisor"
		} else {
			continue;
		};
		let metadata = fs::metadata(&path)?;
		let modified = metadata
			.modified()
			.unwrap_or(UNIX_EPOCH)
			.duration_since(UNIX_EPOCH)
			.unwrap_or_default()
			.as_secs();
		out.push(ExecutableCandidate {
			kind: kind.to_string(),
			path: path.canonicalize().unwrap_or(path).display().to_string(),
			length: metadata.len(),
			last_write_unix_secs: modified,
			search_root: search_root.display().to_string(),
		});
	}
	Ok(())
}

fn find_executables_command(args: &CliArgs<'_>) -> Result<()> {
	let root = repo_root()?;
	let items = find_executables(&root, args)?;
	if args.flag("--json") {
		println!("{}", serde_json::to_string_pretty(&items)?);
	} else if items.is_empty() {
		println!("UNAvatar executable candidates: none");
	} else {
		println!("UNAvatar executable candidates:");
		for item in items {
			println!("- {}: {}", item.kind, item.path);
			println!("  last_write_unix_secs: {}", item.last_write_unix_secs);
			println!("  length: {}", item.length);
		}
	}
	Ok(())
}

#[derive(Debug, Clone, Serialize)]
struct RunningProcess {
	process_name: String,
	pid: u32,
	executable: String,
	profile_hint: Option<String>,
	unmfz_key_hint: Option<String>,
	runtime_bus_key: Option<String>,
	command_line: String,
}

fn detect_processes(include_supervisor: bool) -> Vec<RunningProcess> {
	let mut system = System::new();
	system.refresh_processes_specifics(ProcessesToUpdate::All, false, ProcessRefreshKind::everything().without_cpu());
	let mut rows = system
		.processes()
		.values()
		.filter_map(|process| {
			let name = process.name().to_string_lossy().to_string();
			let stem = name.trim_end_matches(".exe");
			if !stem.eq_ignore_ascii_case("un-avatar-renderer")
				&& !(include_supervisor && stem.eq_ignore_ascii_case("un-avatar-supervisor"))
			{
				return None;
			}
			let cmd = process
				.cmd()
				.iter()
				.map(|part| part.to_string_lossy().to_string())
				.collect::<Vec<_>>();
			Some(RunningProcess {
				process_name: name,
				pid: process.pid().as_u32(),
				executable: process.exe().map(|path| path.display().to_string()).unwrap_or_default(),
				profile_hint: arg_value(
					&cmd,
					&[
						"--gltf",
						"--profile",
						"--profile-id",
						"--profile-path",
						"--profile-file",
						"--manifest",
						"--manifest-path",
					],
				),
				unmfz_key_hint: arg_value(
					&cmd,
					&[
						"--unmotion-zenoh-key",
						"--unmfz",
						"--unmfz-key",
						"--unmfz-base-key",
						"--unmfz-base-key-expr",
					],
				),
				runtime_bus_key: arg_value(&cmd, &["--runtime-bus-key"]),
				command_line: cmd.join(" "),
			})
		})
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| a.process_name.cmp(&b.process_name).then_with(|| a.pid.cmp(&b.pid)));
	rows
}

fn arg_value(args: &[String], names: &[&str]) -> Option<String> {
	for (index, arg) in args.iter().enumerate() {
		for name in names {
			if arg.eq_ignore_ascii_case(name) {
				return args.get(index + 1).cloned();
			}
			if let Some((key, value)) = arg.split_once('=') {
				if key.eq_ignore_ascii_case(name) {
					return Some(value.to_string());
				}
			}
		}
	}
	None
}

fn detect_command(args: &CliArgs<'_>) -> Result<()> {
	let rows = detect_processes(args.flag("--include-supervisor"));
	if args.flag("--json") {
		println!("{}", serde_json::to_string_pretty(&rows)?);
	} else if rows.is_empty() {
		println!("No running UNAvatar Renderer process found.");
	} else {
		for row in rows {
			println!(
				"{} pid={} executable={} profile_hint={} unmfz_key_hint={} runtime_bus_key={}",
				row.process_name,
				row.pid,
				row.executable,
				row.profile_hint.as_deref().unwrap_or("-"),
				row.unmfz_key_hint.as_deref().unwrap_or("-"),
				row.runtime_bus_key.as_deref().unwrap_or("-")
			);
			println!("  {}", row.command_line);
		}
	}
	Ok(())
}

fn renderer_status(args: &CliArgs<'_>) -> Result<()> {
	use zenoh::Wait as _;

	let requested_pid = args
		.value("--pid")
		.map(|value| {
			value
				.parse::<u32>()
				.map_err(|_| format!("--pid must be an unsigned integer: {value}"))
		})
		.transpose()?;
	let renderers = detect_processes(false)
		.into_iter()
		.filter(|process| requested_pid.is_none_or(|pid| process.pid == pid))
		.collect::<Vec<_>>();
	let renderer = match renderers.as_slice() {
		[] => return Err("no matching UNAvatar Renderer process found".into()),
		[renderer] => renderer,
		_ => return Err("multiple UNAvatar Renderer processes found; select one with --pid".into()),
	};
	let runtime_bus_key = renderer
		.runtime_bus_key
		.as_deref()
		.ok_or("Renderer command line does not contain --runtime-bus-key")?;
	let status_key = format!("{runtime_bus_key}/status");
	let timeout = Duration::from_millis(args.parse_u64("--timeout-ms", 3000)?.max(1));
	let session = zenoh::open(zenoh::Config::default())
		.wait()
		.map_err(|error| format!("open Zenoh runtime bus: {error}"))?;
	let subscriber = session
		.declare_subscriber(&status_key)
		.wait()
		.map_err(|error| format!("subscribe Renderer status {status_key}: {error}"))?;
	let sample = subscriber
		.recv_timeout(timeout)
		.map_err(|error| format!("read Renderer status {status_key}: {error}"))?
		.ok_or_else(|| format!("timed out waiting for Renderer status on {status_key}"))?;
	let telemetry: Value =
		serde_json::from_slice(&sample.payload().to_bytes()).map_err(|error| format!("parse Renderer status {status_key}: {error}"))?;
	if args.flag("--json") {
		println!("{}", serde_json::to_string_pretty(&telemetry)?);
	} else {
		println!("Renderer pid={} status_key={status_key}", renderer.pid);
		for field in [
			"connected",
			"primary_motion_source",
			"unmotion_zenoh_enabled",
			"unmotion_zenoh_key",
			"unmotion_zenoh_received_frames",
			"motion_applied_frames",
		] {
			println!("{field}: {}", telemetry.get(field).unwrap_or(&Value::Null));
		}
		let expression_count = telemetry.get("expression_presets").and_then(Value::as_array).map_or(0, Vec::len);
		println!("expression_presets: {expression_count}");
	}
	Ok(())
}

fn renderer_screenshot(args: &CliArgs<'_>) -> Result<()> {
	use zenoh::Wait as _;

	let requested_pid = args
		.value("--pid")
		.map(|value| {
			value
				.parse::<u32>()
				.map_err(|_| format!("--pid must be an unsigned integer: {value}"))
		})
		.transpose()?;
	let renderers = detect_processes(false)
		.into_iter()
		.filter(|process| requested_pid.is_none_or(|pid| process.pid == pid))
		.collect::<Vec<_>>();
	let renderer = match renderers.as_slice() {
		[] => return Err("no matching UNAvatar Renderer process found".into()),
		[renderer] => renderer,
		_ => return Err("multiple UNAvatar Renderer processes found; select one with --pid".into()),
	};
	let runtime_bus_key = renderer
		.runtime_bus_key
		.as_deref()
		.ok_or("Renderer command line does not contain --runtime-bus-key")?;
	let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
	let count = args.parse_u64("--count", 1)?.clamp(1, 300) as usize;
	let root = repo_root()?;
	let output = args.value("--path").map(PathBuf::from).unwrap_or_else(|| {
		if count == 1 {
			PathBuf::from(format!("tmp/avatar/renderer-{timestamp}.png"))
		} else {
			PathBuf::from(format!("tmp/avatar/renderer-burst-{timestamp}"))
		}
	});
	let output = if output.is_absolute() { output } else { root.join(output) };
	if count == 1 {
		if let Some(parent) = output.parent() {
			fs::create_dir_all(parent)?;
		}
	} else {
		fs::create_dir_all(&output)?;
	}

	let control_key = format!("{runtime_bus_key}/control");
	let session = zenoh::open(zenoh::Config::default())
		.wait()
		.map_err(|error| format!("open Zenoh runtime bus: {error}"))?;
	let publisher = session
		.declare_publisher(&control_key)
		.wait()
		.map_err(|error| format!("declare Renderer control publisher {control_key}: {error}"))?;
	let discovery_wait = Duration::from_millis(args.parse_u64("--discovery-wait-ms", 750)?);
	thread::sleep(discovery_wait);
	let timeout = Duration::from_millis(args.parse_u64("--timeout-ms", 12_000)?.max(1));
	let interval = Duration::from_millis(args.parse_u64("--interval-ms", 100)?.max(1));
	for index in 0..count {
		let path = if count == 1 {
			output.clone()
		} else {
			output.join(format!("frame-{index:03}.png"))
		};
		let request_id = format!("unvac-xtask-{timestamp}-{index}");
		let response_key = format!("{runtime_bus_key}/control/response/{request_id}");
		let subscriber = session
			.declare_subscriber(&response_key)
			.wait()
			.map_err(|error| format!("subscribe Renderer response {response_key}: {error}"))?;
		let request = json!({
			"request_id": request_id,
			"command": {
				"command": "screenshot",
				"path": path.to_string_lossy(),
			}
		});
		publisher
			.put(serde_json::to_vec(&request)?)
			.wait()
			.map_err(|error| format!("send Renderer screenshot command: {error}"))?;
		let sample = subscriber
			.recv_timeout(timeout)
			.map_err(|error| format!("read Renderer screenshot response: {error}"))?
			.ok_or_else(|| format!("timed out waiting for Renderer screenshot response on {response_key}"))?;
		let response: Value =
			serde_json::from_slice(&sample.payload().to_bytes()).map_err(|error| format!("parse Renderer response: {error}"))?;
		if !response.get("ok").and_then(Value::as_bool).unwrap_or(false) {
			return Err(format!(
				"Renderer screenshot failed: {}",
				response.get("error").and_then(Value::as_str).unwrap_or("unknown error")
			)
			.into());
		}
		if !path.is_file() {
			return Err(format!("Renderer reported screenshot success but file is missing: {}", path.display()).into());
		}
		if index + 1 < count {
			thread::sleep(interval);
		}
	}
	if count == 1 {
		println!("Renderer screenshot: {}", output.display());
	} else {
		println!("Renderer screenshot burst: {} frames in {}", count, output.display());
	}
	Ok(())
}

fn wait_renderer(args: &CliArgs<'_>) -> Result<()> {
	let timeout = Duration::from_secs(args.parse_u64("--timeout-secs", 120)?);
	let interval = Duration::from_secs(args.parse_u64("--interval-secs", 2)?.max(1));
	let started = Instant::now();
	loop {
		let renderers = detect_processes(args.flag("--include-supervisor"))
			.into_iter()
			.filter(|process| {
				process
					.process_name
					.trim_end_matches(".exe")
					.eq_ignore_ascii_case("un-avatar-renderer")
			})
			.collect::<Vec<_>>();
		if !renderers.is_empty() {
			if args.flag("--json") {
				println!("{}", serde_json::to_string_pretty(&renderers)?);
			} else {
				println!("UNAvatar Renderer is running:");
				for process in renderers {
					println!("- pid={} {}", process.pid, process.executable);
				}
			}
			return Ok(());
		}
		if started.elapsed() >= timeout {
			return Err("timed out waiting for UNAvatar Renderer; start the target profile and retry".into());
		}
		println!("Waiting for UNAvatar Renderer... ({} sec)", interval.as_secs());
		thread::sleep(interval);
	}
}

fn start_supervisor(args: &CliArgs<'_>) -> Result<()> {
	let root = repo_root()?;
	let items = find_executables(&root, args)?;
	let supervisor = items
		.iter()
		.find(|item| item.kind == "supervisor")
		.ok_or("UNAvatar supervisor executable was not found")?;
	println!("UNAvatar Supervisor:\n  {}", supervisor.path);
	if !args.flag("--launch") {
		println!("To launch it:\n  cargo xtask avatar-control start-supervisor --launch");
		return Ok(());
	}
	let path = PathBuf::from(&supervisor.path);
	let mut command = Command::new(&path);
	if let Some(parent) = path.parent() {
		command.current_dir(parent);
	}
	command.spawn()?;
	println!("Started UNAvatar Supervisor.");
	Ok(())
}

fn live_smoke(args: &CliArgs<'_>) -> Result<()> {
	if !args.flag("--skip-renderer-wait") {
		let timeout = args.value_or("--renderer-timeout-secs", "120").to_string();
		let wait_args = vec!["--timeout-secs".to_string(), timeout];
		wait_renderer(&CliArgs::new(&wait_args))?;
	}
	if args.flag("--print-only") {
		println!("PrintOnly: skipping Control API preflight POST.");
	} else {
		println!("Checking UNVAC Control API ingress...");
		let preflight = intent_request("preflight", "neutral")?;
		post_intent(
			args.value_or("--base-url", DEFAULT_BASE_URL),
			args.value_or("--token", ""),
			"avatar_intent",
			"control_api_preflight",
			&preflight,
			false,
			"xtask.avatar_live_smoke_preflight",
			None,
		)?;
	}
	send_sequence(args)
}

#[derive(Debug, Serialize)]
struct AuditCheck {
	name: &'static str,
	status: &'static str,
	detail: String,
}

#[derive(Debug, Serialize)]
struct AuditSummary {
	ok: bool,
	ready_for_live_smoke: bool,
	base_key: String,
	target_link_id: String,
	preset_file: String,
	checks: Vec<AuditCheck>,
}

fn collect_audit(args: &CliArgs<'_>) -> Result<AuditSummary> {
	let root = repo_root()?;
	let paths = live_paths(&root, args, true);
	let mut checks = Vec::new();
	let local_result = (|| -> Result<()> {
		if args.flag("--prepare") {
			prepare_with_paths(args, &root, &paths, true)?;
		}
		validate_live_paths(&paths)?;
		load_check(&root, &paths.flowgraph_dir, true)
	})();
	checks.push(check("local_conf_flowgraph_load", local_result, "conf / Flowgraph load-check OK"));

	match find_executables(&root, args) {
		Ok(items) if items.iter().any(|item| item.kind == "renderer") && items.iter().any(|item| item.kind == "supervisor") => {
			checks.push(AuditCheck {
				name: "unavatar_executables",
				status: "ok",
				detail: "renderer and supervisor candidates found".into(),
			});
		}
		Ok(_) => checks.push(AuditCheck {
			name: "unavatar_executables",
			status: "warn",
			detail: "renderer or supervisor candidate missing".into(),
		}),
		Err(error) => checks.push(AuditCheck {
			name: "unavatar_executables",
			status: "fail",
			detail: error.to_string(),
		}),
	}
	let renderers = detect_processes(false);
	checks.push(if renderers.is_empty() {
		AuditCheck {
			name: "unavatar_renderer_running",
			status: "pending",
			detail: "No running UNAvatar Renderer. Start the target profile before live smoke.".into(),
		}
	} else {
		AuditCheck {
			name: "unavatar_renderer_running",
			status: "ok",
			detail: format!("{} renderer process(es) detected", renderers.len()),
		}
	});

	if args.flag("--skip-dry-run-smoke") {
		checks.push(AuditCheck {
			name: "unvac_dry_run_smoke",
			status: "skipped",
			detail: "skipped by --skip-dry-run-smoke".into(),
		});
	} else {
		let smoke_result = FIXTURES.iter().try_for_each(|fixture| run_fixture(&root, fixture, true));
		checks.push(check("unvac_dry_run_smoke", smoke_result, "all avatar AI control fixtures passed"));
	}

	let preview = intent_request(args.value_or("--link-id", DEFAULT_LINK_ID), "smile")
		.and_then(|value| serde_json::to_string(&value).map(|_| ()).map_err(Into::into));
	checks.push(check("live_smoke_payload_preview", preview, "smile payload preview OK"));

	let preset_result = (|| -> Result<()> {
		let preset_path = resolve_path(&root, args.value_or("--preset-file", DEFAULT_PRESET_FILE));
		let config = load_reactions(&preset_path)?;
		for (name, entry) in &config.presets {
			validate_reaction(name, entry, args.value_or("--link-id", DEFAULT_LINK_ID))?;
		}
		Ok(())
	})();
	checks.push(check(
		"avatar_reaction_presets",
		preset_result,
		"preset file and canonical mapper validation passed",
	));

	let fail_count = checks.iter().filter(|check| check.status == "fail").count();
	let ready = fail_count == 0 && !renderers.is_empty();
	Ok(AuditSummary {
		ok: fail_count == 0,
		ready_for_live_smoke: ready,
		base_key: args.value_or("--base-key", DEFAULT_BASE_KEY).to_string(),
		target_link_id: args.value_or("--link-id", DEFAULT_LINK_ID).to_string(),
		preset_file: args.value_or("--preset-file", DEFAULT_PRESET_FILE).to_string(),
		checks,
	})
}

fn print_audit_summary(args: &CliArgs<'_>, summary: &AuditSummary) -> Result<()> {
	if args.flag("--json") {
		println!("{}", serde_json::to_string_pretty(&summary)?);
	} else {
		println!("UNVAC 2.0 avatar AI control readiness");
		for item in &summary.checks {
			println!("[{}] {}: {}", item.status, item.name, item.detail);
		}
		if summary.ready_for_live_smoke {
			println!("Ready for live smoke:\n  cargo xtask avatar-control live-smoke --sequence smile-then-neutral");
		} else {
			println!("Not ready for live smoke yet. Start the target UNAvatar Renderer, then rerun this audit.");
		}
	}
	Ok(())
}

fn ensure_audit_passed(summary: &AuditSummary) -> Result<()> {
	let fail_count = summary.checks.iter().filter(|check| check.status == "fail").count();
	if !summary.ok {
		return Err(format!("avatar control readiness failed: {fail_count} check(s)").into());
	}
	Ok(())
}

fn audit(args: &CliArgs<'_>) -> Result<()> {
	let summary = collect_audit(args)?;
	print_audit_summary(args, &summary)?;
	ensure_audit_passed(&summary)
}

fn check(name: &'static str, result: Result<()>, success: &str) -> AuditCheck {
	match result {
		Ok(()) => AuditCheck {
			name,
			status: "ok",
			detail: success.to_string(),
		},
		Err(error) => AuditCheck {
			name,
			status: "fail",
			detail: error.to_string(),
		},
	}
}

fn preflight(args: &CliArgs<'_>) -> Result<()> {
	println!("==> prepare / load-check");
	let mut start_items = args.items.to_vec();
	start_items.push("--validate-only".into());
	start(&CliArgs::new(&start_items))?;
	if !args.flag("--skip-executable-discovery") {
		println!("\n==> UNAvatar executable discovery");
		find_executables_command(args)?;
	}
	println!("\n==> UNAvatar Renderer detection");
	let mut detect_items = args.items.to_vec();
	detect_items.push("--include-supervisor".into());
	detect_command(&CliArgs::new(&detect_items))?;
	if !args.flag("--skip-dry-run-smoke") {
		println!("\n==> UNVAC dry-run smoke");
		smoke()?;
	}
	if !args.flag("--skip-payload-preview") {
		println!("\n==> intent payload preview");
		let mut preview = vec![
			"--sequence".into(),
			"smile-then-neutral".into(),
			"--link-id".into(),
			args.value_or("--link-id", DEFAULT_LINK_ID).into(),
			"--print-only".into(),
		];
		if let Some(url) = args.value("--base-url") {
			preview.extend(["--base-url".into(), url.into()]);
		}
		send_sequence(&CliArgs::new(&preview))?;
	}
	println!("\nPreflight complete.");
	println!("Next live run:\n  cargo xtask avatar-control start --prepare --streaming-persona --enable-publish --force");
	println!("Then:\n  cargo xtask avatar-control live-smoke --sequence smile-then-neutral");
	Ok(())
}

fn new_live_note(args: &CliArgs<'_>) -> Result<PathBuf> {
	let root = repo_root()?;
	let output_dir = resolve_path(&root, args.value_or("--output-dir", "docs/roadmap/avatar-ai-control-live-runs"));
	fs::create_dir_all(&output_dir)?;
	let now = jiff::Zoned::now();
	let session_id = args
		.value("--session-id")
		.map(str::to_string)
		.unwrap_or_else(|| now.strftime("%Y%m%d-%H%M").to_string());
	let output = output_dir.join(format!("avatar-ai-control-live-{session_id}.md"));
	assert_can_write(&output, args.flag("--force"))?;
	let base_key = markdown_cell(args.value_or("--base-key", DEFAULT_BASE_KEY));
	let link_id = markdown_cell(args.value_or("--link-id", DEFAULT_LINK_ID));
	let preset_file = markdown_cell(args.value_or("--preset-file", DEFAULT_PRESET_FILE));
	let unvac_build = markdown_cell(&git_head(&root));
	let created = now.strftime("%Y-%m-%d %H:%M:%S %:z").to_string();
	let content = build_live_note_content(&created, &session_id, &unvac_build, &link_id, &base_key, &preset_file);
	fs::write(&output, content)?;
	println!("Wrote {}", output.display());
	Ok(output)
}

fn build_live_note_content(created: &str, session_id: &str, unvac_build: &str, link_id: &str, base_key: &str, preset_file: &str) -> String {
	format!(
		"# UNVAC 2.0 AIアバター制御 実配信記録\n\n状態: 記録中\n作成: {created}\n\n\
## 1. 対象\n\n| 項目 | 値 |\n|---|---|\n| セッションID | {session_id} |\n| UNVAC build / commit | {unvac_build} |\n| UNAvatar build / profile | TODO |\n| UNMotion / mocap source | TODO |\n| OBS scene / output | TODO |\n| AvatarLink id | {link_id} |\n| UNMF/Z base key | {base_key} |\n| preset file | {preset_file} |\n\n\
## 2. 配信前確認\n\n```console\ncargo xtask stream-check --evidence-out target/stream-evidence/{session_id}-preflight.json\n```\n\n実プロファイルを試験するときは、example構成を生成する`avatar-control audit --prepare --force`を実行しない。\n\n| 確認項目 | 結果 | メモ |\n|---|---|---|\n| readiness | TODO | preflight JSONを保存 |\n| UNAvatar Renderer | TODO | TODO |\n| Twitch OAuth / IRC | TODO | 実PRIVMSG到達は認可状態と分けて記録する |\n| OBS字幕 / 演出 | TODO | TODO |\n| avatar reaction presets | TODO | TODO |\n\n\
## 3. 実配信中の観察\n\n| 項目 | 結果 | メモ |\n|---|---|---|\n| AI会話量 / 会話終端 | TODO | TODO |\n| STT partial / final | TODO | TODO |\n| AI TTS / 字幕 / 口パク同期 | TODO | TODO |\n| Twitch comment / 読み上げ | TODO | TODO |\n| 瞬き / 姿勢 / AvatarIntent | TODO | TODO |\n| OBS字幕 / Quick Effects | TODO | TODO |\n\n\
## 4. 実ユースケース\n\n| ユースケース | 入力 | 期待結果 | 結果 | メモ |\n|---|---|---|---|---|\n| 短い会話反応 | Twitch chat / AI observe | smile / nod | TODO | TODO |\n| チャネルポイント交換 | channel point | channel_point_surprise | TODO | 実交換時刻を記録する |\n| 配信者発話への反応 | voice observe | listening_nod / concerned | TODO | TODO |\n| 軽いモデレーション | AI judgement | moderation_warning | TODO | TODO |\n\n\
## 5. 配信後の再現・修正\n\n配信後の成功を配信中の成功として扱わず、別行へ記録する。\n\n| 項目 | 結果 | 根拠 |\n|---|---|---|\n| TODO | TODO | TODO |\n\n\
## 6. 判定\n\n| 判定 | 理由 |\n|---|---|\n| TODO | TODO |\n\n配信終了前の縦断検査:\n\n```console\ncargo xtask stream-check --expect-live-from target/stream-evidence/{session_id}-preflight.json --expect-live-minutes 10 --evidence-out target/stream-evidence/{session_id}-post-stream.json\n```\n\n次回の確認:\n\n- TODO\n"
	)
}

fn start_live_test(args: &CliArgs<'_>) -> Result<()> {
	println!("==> Create live run note");
	let note = new_live_note(args)?;
	println!("\n==> Readiness audit");
	let mut audit_items = args.items.to_vec();
	for flag in ["--prepare", "--streaming-persona", "--enable-publish", "--force"] {
		if !audit_items.iter().any(|item| item == flag) {
			audit_items.push(flag.into());
		}
	}
	let audit_args = CliArgs::new(&audit_items);
	let summary = collect_audit(&audit_args)?;
	append_audit_to_live_note(&note, &summary)?;
	print_audit_summary(&audit_args, &summary)?;
	ensure_audit_passed(&summary)?;
	println!("\n==> UNAvatar Supervisor");
	let mut supervisor_items = Vec::new();
	for root in args.values("--search-root") {
		supervisor_items.extend(["--search-root".into(), root.into()]);
	}
	if args.flag("--launch-supervisor") {
		supervisor_items.push("--launch".into());
	}
	start_supervisor(&CliArgs::new(&supervisor_items))?;
	println!("When the target Renderer key matches, run:\n  cargo xtask avatar-control live-smoke --sequence smile-then-neutral");
	Ok(())
}

fn git_head(root: &Path) -> String {
	let output = Command::new("git")
		.args(["rev-parse", "--short", "HEAD"])
		.current_dir(root)
		.output();
	match output {
		Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).trim().to_string(),
		_ => "取得できず".to_string(),
	}
}

fn append_audit_to_live_note(path: &Path, summary: &AuditSummary) -> Result<()> {
	let recorded = jiff::Zoned::now().strftime("%Y-%m-%d %H:%M:%S %:z").to_string();
	let content = format_audit_markdown(&recorded, summary);
	let mut file = fs::OpenOptions::new().append(true).open(path)?;
	file.write_all(content.as_bytes())?;
	println!("Recorded readiness audit in {}", path.display());
	Ok(())
}

fn format_audit_markdown(recorded: &str, summary: &AuditSummary) -> String {
	let mut content = format!(
		"\n\n## 7. start-live-test 自動記録\n\n記録: {recorded}\n\n\
| 判定 | 値 |\n|---|---|\n| audit ok | {} |\n| live smoke 準備完了 | {} |\n| AvatarLink id | {} |\n| UNMF/Z base key | {} |\n| preset file | {} |\n\n\
| チェック | 状態 | 詳細 |\n|---|---|---|\n",
		summary.ok,
		summary.ready_for_live_smoke,
		markdown_cell(&summary.target_link_id),
		markdown_cell(&summary.base_key),
		markdown_cell(&summary.preset_file),
	);
	for check in &summary.checks {
		content.push_str(&format!(
			"| {} | {} | {} |\n",
			markdown_cell(check.name),
			markdown_cell(check.status),
			markdown_cell(&check.detail)
		));
	}
	content
}

fn validate_live_paths(paths: &LivePaths) -> Result<()> {
	if !paths.conf.is_file() {
		return Err(format!("conf file not found: {}; pass --prepare or create it first", paths.conf.display()).into());
	}
	if !paths.flowgraph_dir.is_dir() {
		return Err(format!(
			"Flowgraph dir not found: {}; pass --prepare or create it first",
			paths.flowgraph_dir.display()
		)
		.into());
	}
	Ok(())
}

fn validate_replacement(label: &str, value: &str, identifier: bool) -> Result<()> {
	if value.trim().is_empty() || value.contains(['\r', '\n', '"']) {
		return Err(format!("{label} must be non-empty and must not contain quotes or newlines").into());
	}
	if identifier && !value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) {
		return Err(format!("{label} may contain only ASCII letters, digits, `_`, and `-`").into());
	}
	Ok(())
}

fn assert_can_write(path: &Path, force: bool) -> Result<()> {
	if path.exists() && !force {
		return Err(format!("refusing to overwrite {}; pass --force", path.display()).into());
	}
	Ok(())
}

fn ensure_safe_remove_dir(path: &Path, root: &Path) -> Result<()> {
	let absolute = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
	if absolute == root || absolute.parent().is_none() || absolute.file_name().is_none() {
		return Err(format!("refusing unsafe directory removal: {}", absolute.display()).into());
	}
	Ok(())
}

fn resolve_path(root: &Path, path: &str) -> PathBuf {
	let path = PathBuf::from(path);
	if path.is_absolute() {
		path
	} else {
		root.join(path)
	}
}

fn path_for_toml(root: &Path, path: &Path) -> String {
	path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn markdown_cell(value: &str) -> String {
	value.replace('|', "\\|")
}

fn run_command_quiet(command: &mut Command) -> Result<()> {
	let status = command.stdout(Stdio::null()).stderr(Stdio::null()).status()?;
	if !status.success() {
		return Err(format!("command failed with status {status}").into());
	}
	Ok(())
}

fn print_help() {
	println!(
		"avatar-control subcommands:\n\
  prepare                 Generate local conf and Flowgraph files.\n\
  start                   Validate and start UNVAC with the local avatar-control conf.\n\
  preflight               Run preparation, discovery, fixture, and payload checks.\n\
  audit                   Emit readiness checks; add --json for machine output.\n\
  smoke                   Run all avatar-control Flowgraph fixtures.\n\
  send                    Send one built-in intent preset; optionally set --duration-ms.\n\
  send-sequence           Send smile/concerned sequences with explicit neutral.\n\
  reaction                List, validate, preview, or send reaction presets.\n\
  preview-frame           Map one preset to a UNMotionFrame JSON without publishing.\n\
  replay-frame            Publish a saved UNMotionFrame JSON to UNMF/Z for inspection.\n\
  live-smoke              Wait for Renderer, probe Control API, and send a sequence.\n\
  find-executables        Find UNAvatar Renderer and Supervisor candidates.\n\
  detect                  Detect running UNAvatar processes with sysinfo.\n\
  renderer-status         Read a running Renderer's UNAA/Z status telemetry.\n\
  renderer-screenshot     Capture Renderer frame(s) through its UNAA/Z control bus.\n\
  wait-renderer           Wait until a Renderer process appears.\n\
  start-supervisor        Show or launch the newest Supervisor candidate.\n\
  new-live-note           Create a live-run observation note.\n\
  start-live-test         Create a note, audit readiness, and show/start Supervisor.\n\n\
Use kebab-case options, for example:\n\
  cargo xtask avatar-control prepare --streaming-persona --enable-publish --force\n\
  cargo xtask avatar-control send --preset smile --duration-ms 5000 --print-only\n\
  cargo xtask avatar-control preview-frame --preset yaw-right --elapsed-ms 1000 --output target/yaw-right.unmf.json\n\
  cargo xtask avatar-control preview-frame --preset yaw-right --terminal --output target/head-neutral.unmf.json\n\
  cargo xtask avatar-control replay-frame --input target/pose.unmf.json --release-input target/neutral.unmf.json --base-key un-motion/frame/kaltsit-ai --count 60\n\
  cargo xtask avatar-control renderer-screenshot --count 60 --interval-ms 100\n\
  cargo xtask avatar-control audit --prepare --json"
	);
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn extracts_split_and_equals_arguments() {
		let split = vec!["renderer".into(), "--gltf".into(), "kp.unavatar".into()];
		assert_eq!(arg_value(&split, &["--gltf"]).as_deref(), Some("kp.unavatar"));
		let equals = vec!["renderer".into(), "--unmfz-key=un-motion/frame/ai".into()];
		assert_eq!(arg_value(&equals, &["--unmfz-key"]).as_deref(), Some("un-motion/frame/ai"));
	}

	#[test]
	fn nested_help_is_detected_before_subcommand_execution() {
		for items in [vec!["--help".into()], vec!["-h".into()], vec!["help".into()]] {
			assert!(help_requested(&CliArgs::new(&items)));
		}
		assert!(!help_requested(&CliArgs::new(&["--print-only".into()])));
	}

	#[test]
	fn intent_presets_follow_canonical_mapper() {
		for preset in ["smile", "nod", "pitch-down", "yaw-right", "concerned-shake", "neutral"] {
			let value = intent_request("ai", preset).unwrap();
			let request: AvatarIntentRequest = serde_json::from_value(value).unwrap();
			avatar_intent_to_unmf(&request, &AvatarIntentMapperOptions::default()).unwrap();
		}
	}

	#[test]
	fn duration_override_updates_all_intents() {
		let mut request = intent_request("ai", "concerned-shake").unwrap();
		override_intent_duration(&mut request, 5000).unwrap();
		assert!(request["intents"]
			.as_array()
			.unwrap()
			.iter()
			.all(|intent| intent["duration_ms"] == 5000));
	}

	#[test]
	fn unmf_publish_key_appends_schema_once() {
		assert_eq!(unmf_publish_key("un-motion/frame/ai", 1), "un-motion/frame/ai/v1");
		assert_eq!(unmf_publish_key("un-motion/frame/ai/v1/", 1), "un-motion/frame/ai/v1");
	}

	#[test]
	fn replacement_validation_rejects_toml_injection() {
		assert!(validate_replacement("link", "ai.good", true).is_err());
		assert!(validate_replacement("profile", "bad\"\nvalue", false).is_err());
		assert!(validate_replacement("link", "kaltsit_ai", true).is_ok());
	}

	#[test]
	fn live_note_template_is_japanese_and_separates_follow_up_evidence() {
		let note = build_live_note_content(
			"2026-08-01 12:00:00 +09:00",
			"session-1",
			"6255e1d",
			"kaltsit_ai",
			"un-motion/frame/kaltsit-ai",
			"presets.json",
		);
		assert!(note.contains("# UNVAC 2.0 AIアバター制御 実配信記録"));
		assert!(note.contains("## 3. 実配信中の観察"));
		assert!(note.contains("配信後の成功を配信中の成功として扱わず"));
		assert!(note.contains("| チャネルポイント交換 |"));
		assert!(note.contains("| UNVAC build / commit | 6255e1d |"));
		assert!(note.contains("cargo xtask stream-check --evidence-out target/stream-evidence/session-1-preflight.json"));
		assert!(note.contains("--expect-live-from target/stream-evidence/session-1-preflight.json"));
		assert!(!note.contains("cargo xtask avatar-control audit --prepare"));
	}

	#[test]
	fn audit_markdown_is_japanese_and_escapes_table_cells() {
		let summary = AuditSummary {
			ok: false,
			ready_for_live_smoke: false,
			base_key: "un-motion/frame/test".into(),
			target_link_id: "ai|partner".into(),
			preset_file: "presets.json".into(),
			checks: vec![AuditCheck {
				name: "renderer",
				status: "fail",
				detail: "missing | stopped".into(),
			}],
		};
		let note = format_audit_markdown("2026-08-01 12:01:00 +09:00", &summary);
		assert!(note.contains("## 7. start-live-test 自動記録"));
		assert!(note.contains("| audit ok | false |"));
		assert!(note.contains("ai\\|partner"));
		assert!(note.contains("missing \\| stopped"));
	}
}

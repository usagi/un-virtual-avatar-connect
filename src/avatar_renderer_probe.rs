use serde::{Deserialize, Serialize};
use std::time::Duration;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use zenoh::Wait as _;

const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AvatarRendererProbeResult {
	pub checked_at: String,
	pub expected_base_keys: Vec<String>,
	pub renderers: Vec<AvatarRendererProbe>,
}

impl AvatarRendererProbeResult {
	pub(crate) fn ready(&self) -> bool {
		!self.expected_base_keys.is_empty() && self.expected_base_keys.iter().all(|key|
			self.renderers.iter().any(|r| r.ready() && r.unmotion_zenoh_key.as_deref().is_some_and(|k| k.trim().trim_end_matches('/') == key)))
	}

	pub(crate) fn fresh_for(&self, keys: &[String]) -> bool {
		let Ok(checked) = self.checked_at.parse::<jiff::Timestamp>() else { return false; };
		let age = jiff::Timestamp::now().as_second() - checked.as_second();
		(0..=90).contains(&age) && normalize_keys(self.expected_base_keys.clone()) == normalize_keys(keys.to_vec())
	}

	pub(crate) fn needs_attention(&self) -> bool {
		if self.expected_base_keys.is_empty() { return true; }
		if self.renderers.is_empty() { return true; }
		// Missing telemetry or an idle sample is not evidence of broken motion.
		if self.renderers.iter().any(|r| r.error.is_some()) { return false; }
		self.expected_base_keys.iter().any(|key| !self.renderers.iter().any(|r|
			r.unmotion_zenoh_enabled && r.unmotion_zenoh_key.as_deref().is_some_and(|k| k.trim().trim_end_matches('/') == key)))
	}

	pub(crate) fn detail(&self) -> String {
		if self.expected_base_keys.is_empty() {
			return "AI control用AvatarLinkが構成されていない。".into();
		}
		if self.renderers.is_empty() {
			return "起動中のUNAvatar Rendererが見つからない。".into();
		}
		if self.ready() {
			return format!(
				"直近の確認で{}接続先すべてのRendererが受信・反映中。姿勢の見た目は対象画面で確認できる。",
				self.expected_base_keys.len()
			);
		}
		if let Some(error) = self.renderers.iter().find_map(|renderer| renderer.error.as_deref()) {
			return format!("Renderer状態を確認できない: {error}");
		}
		if !self.needs_attention() {
			return "対象Rendererの受信設定を確認済み。直近の観測では受信・反映の増加を確認できていない。停止や故障とは断定していない。".into();
		}
		let keys = self
			.renderers
			.iter()
			.filter_map(|renderer| renderer.unmotion_zenoh_key.as_deref())
			.collect::<Vec<_>>()
			.join(", ");
		format!(
			"RendererのUNMF/Z受信を確認できない。期待: {} / Renderer: {}",
			self.expected_base_keys.join(", "),
			if keys.is_empty() { "key不明" } else { &keys }
		)
	}
}

fn normalize_keys(keys: Vec<String>) -> Vec<String> {
	let mut keys = keys.into_iter().map(|k| k.trim().trim_end_matches('/').to_string()).filter(|k| !k.is_empty()).collect::<Vec<_>>();
	keys.sort(); keys.dedup(); keys
}

/// A single read-only probe per State, at most once per 30 seconds; never wait in snapshot.
pub(crate) fn refresh_in_background(
	evidence: std::sync::Arc<tokio::sync::RwLock<Option<AvatarRendererProbeResult>>>,
	gate: std::sync::Arc<tokio::sync::Mutex<Option<std::time::Instant>>>,
	keys: Vec<String>,
) {
	if keys.is_empty() { return; }
	let Ok(mut guard) = gate.try_lock_owned() else { return; };
	if guard.as_ref().is_some_and(|last| last.elapsed() < Duration::from_secs(30)) { return; }
	*guard = Some(std::time::Instant::now());
	tokio::spawn(async move {
		let _guard = guard;
		match crate::un_app_api::run_blocking_on_zenoh_runtime(move || Ok(probe(keys))).await {
			Ok(result) => *evidence.write().await = Some(result),
			Err(error) => log::warn!("Avatar status observation failed: {error}"),
		}
	});
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AvatarRendererProbe {
	pub pid: u32,
	pub profile_hint: Option<String>,
	pub runtime_bus_key: Option<String>,
	pub connected: bool,
	pub unmotion_zenoh_enabled: bool,
	pub unmotion_zenoh_key: Option<String>,
	pub key_matches: bool,
	pub received_before: u64,
	pub received_after: u64,
	pub received_delta: u64,
	pub applied_before: u64,
	pub applied_after: u64,
	pub applied_delta: u64,
	pub error: Option<String>,
}

impl AvatarRendererProbe {
	fn ready(&self) -> bool {
		self.error.is_none()
			&& self.connected
			&& self.unmotion_zenoh_enabled
			&& self.key_matches
			&& self.received_delta > 0
			&& self.applied_delta > 0
	}
}

#[derive(Debug)]
struct RunningRenderer {
	pid: u32,
	profile_hint: Option<String>,
	runtime_bus_key: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RendererTelemetry {
	#[serde(default)]
	connected: bool,
	#[serde(default)]
	unmotion_zenoh_enabled: bool,
	unmotion_zenoh_key: Option<String>,
	#[serde(default)]
	unmotion_zenoh_received_frames: u64,
	#[serde(default)]
	motion_applied_frames: u64,
}

pub(crate) fn probe(expected_base_keys: Vec<String>) -> AvatarRendererProbeResult {
	let mut expected_base_keys = expected_base_keys
		.into_iter()
		.map(|key| key.trim().trim_end_matches('/').to_string())
		.filter(|key| !key.is_empty())
		.collect::<Vec<_>>();
	expected_base_keys.sort();
	expected_base_keys.dedup();
	let renderers = detect_renderers();
	let mut result = AvatarRendererProbeResult {
		checked_at: jiff::Timestamp::now().to_string(),
		expected_base_keys,
		renderers: Vec::with_capacity(renderers.len()),
	};
	if result.expected_base_keys.is_empty() || renderers.is_empty() {
		return result;
	}

	let session = match zenoh::open(zenoh::Config::default()).wait() {
		Ok(session) => session,
		Err(error) => {
			for renderer in renderers {
				result
					.renderers
					.push(failed_probe(renderer, format!("Zenoh session open: {error}")));
			}
			return result;
		}
	};
	for renderer in renderers {
		result
			.renderers
			.push(probe_renderer(&session, renderer, &result.expected_base_keys));
	}
	result
}

fn probe_renderer(session: &zenoh::Session, renderer: RunningRenderer, expected_base_keys: &[String]) -> AvatarRendererProbe {
	let Some(runtime_bus_key) = renderer.runtime_bus_key.as_deref() else {
		return failed_probe(renderer, "Renderer command lineに--runtime-bus-keyがない。".into());
	};
	let status_key = format!("{runtime_bus_key}/status");
	let subscriber = match session.declare_subscriber(&status_key).wait() {
		Ok(subscriber) => subscriber,
		Err(error) => return failed_probe(renderer, format!("status subscribe {status_key}: {error}")),
	};
	let before = match subscriber.recv_timeout(STATUS_TIMEOUT) {
		Ok(Some(sample)) => match parse_telemetry(&sample.payload().to_bytes()) {
			Ok(telemetry) => telemetry,
			Err(error) => return failed_probe(renderer, format!("status parse {status_key}: {error}")),
		},
		Ok(None) => return failed_probe(renderer, format!("status timeout {status_key}")),
		Err(error) => return failed_probe(renderer, format!("status receive {status_key}: {error}")),
	};
	let after = match subscriber.recv_timeout(STATUS_TIMEOUT) {
		Ok(Some(sample)) => match parse_telemetry(&sample.payload().to_bytes()) {
			Ok(telemetry) => telemetry,
			Err(error) => return failed_probe(renderer, format!("status parse {status_key}: {error}")),
		},
		Ok(None) => return failed_probe(renderer, format!("status second sample timeout {status_key}")),
		Err(error) => return failed_probe(renderer, format!("status second sample {status_key}: {error}")),
	};
	let key = after.unmotion_zenoh_key.clone().or(before.unmotion_zenoh_key);
	let key_matches = key.as_deref().is_some_and(|key| {
		let key = key.trim().trim_end_matches('/');
		expected_base_keys.iter().any(|expected| expected == key)
	});
	AvatarRendererProbe {
		pid: renderer.pid,
		profile_hint: renderer.profile_hint,
		runtime_bus_key: renderer.runtime_bus_key,
		connected: after.connected,
		unmotion_zenoh_enabled: after.unmotion_zenoh_enabled,
		unmotion_zenoh_key: key,
		key_matches,
		received_before: before.unmotion_zenoh_received_frames,
		received_after: after.unmotion_zenoh_received_frames,
		received_delta: after
			.unmotion_zenoh_received_frames
			.saturating_sub(before.unmotion_zenoh_received_frames),
		applied_before: before.motion_applied_frames,
		applied_after: after.motion_applied_frames,
		applied_delta: after.motion_applied_frames.saturating_sub(before.motion_applied_frames),
		error: None,
	}
}

fn parse_telemetry(bytes: &[u8]) -> Result<RendererTelemetry, serde_json::Error> {
	serde_json::from_slice(bytes)
}

fn failed_probe(renderer: RunningRenderer, error: String) -> AvatarRendererProbe {
	AvatarRendererProbe {
		pid: renderer.pid,
		profile_hint: renderer.profile_hint,
		runtime_bus_key: renderer.runtime_bus_key,
		connected: false,
		unmotion_zenoh_enabled: false,
		unmotion_zenoh_key: None,
		key_matches: false,
		received_before: 0,
		received_after: 0,
		received_delta: 0,
		applied_before: 0,
		applied_after: 0,
		applied_delta: 0,
		error: Some(error),
	}
}

fn detect_renderers() -> Vec<RunningRenderer> {
	let mut system = System::new();
	system.refresh_processes_specifics(ProcessesToUpdate::All, false, ProcessRefreshKind::everything().without_cpu());
	let mut renderers = system
		.processes()
		.values()
		.filter_map(|process| {
			let name = process.name().to_string_lossy();
			if !name.trim_end_matches(".exe").eq_ignore_ascii_case("un-avatar-renderer") {
				return None;
			}
			let args = process
				.cmd()
				.iter()
				.map(|value| value.to_string_lossy().to_string())
				.collect::<Vec<_>>();
			Some(RunningRenderer {
				pid: process.pid().as_u32(),
				profile_hint: arg_value(&args, &["--manifest", "--profile", "--profile-path"]),
				runtime_bus_key: arg_value(&args, &["--runtime-bus-key"]),
			})
		})
		.collect::<Vec<_>>();
	renderers.sort_by_key(|renderer| renderer.pid);
	renderers
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn argument_value_accepts_split_and_equals_forms() {
		assert_eq!(
			arg_value(
				&["app".into(), "--runtime-bus-key".into(), "un/avatar/1".into()],
				&["--runtime-bus-key"]
			),
			Some("un/avatar/1".into())
		);
		assert_eq!(
			arg_value(&["app".into(), "--manifest=a.toml".into()], &["--manifest"]),
			Some("a.toml".into())
		);
	}

	#[test]
	fn result_requires_matching_moving_renderer() {
		let mut result = AvatarRendererProbeResult {
			checked_at: "now".into(),
			expected_base_keys: vec!["un-motion/frame/ai".into()],
			renderers: vec![AvatarRendererProbe {
				pid: 1,
				profile_hint: None,
				runtime_bus_key: Some("un/avatar/1".into()),
				connected: true,
				unmotion_zenoh_enabled: true,
				unmotion_zenoh_key: Some("un-motion/frame/ai".into()),
				key_matches: true,
				received_before: 10,
				received_after: 12,
				received_delta: 2,
				applied_before: 8,
				applied_after: 10,
				applied_delta: 2,
				error: None,
			}],
		};
		assert!(result.ready());
		result.checked_at = jiff::Timestamp::now().to_string();
		assert!(result.fresh_for(&result.expected_base_keys));
		assert!(!result.fresh_for(&["other/key".into()]));
		result.checked_at = "2020-01-01T00:00:00Z".into();
		assert!(!result.fresh_for(&result.expected_base_keys));
		result.expected_base_keys.push("other/key".into());
		assert!(!result.ready(), "One receiver must not certify every configured target");
		assert!(result.needs_attention());
		result.expected_base_keys.pop();
		result.renderers[0].applied_delta = 0;
		assert!(!result.ready());
		assert!(!result.needs_attention(), "Idle sample alone is not failure");
		result.renderers[0].unmotion_zenoh_enabled = false;
		assert!(result.needs_attention());
		result.renderers[0].error = Some("timeout".into());
		assert!(!result.needs_attention(), "Unavailable observation is not a confirmed misconfiguration");
	}
}

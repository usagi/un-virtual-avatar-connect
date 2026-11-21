//! ランタイム一時ディレクトリの生成と管理。
//!
//! - ルート: 既定で `dirs::data_local_dir()/virtual-avatar-connect/runtime`（Windows では `%LOCALAPPDATA%`）。
//!   設定で `runtime_dir` が指定されていれば優先する。
//! - セッションごとにサブディレクトリ `<root>/<session_id>/` を切る。session_id はプロセス起動時に生成。
//! - 起動時に、24 時間以上古く、生存ロックを取得できるセッションだけを整理する。
//!   稼働中の別プロセス、ロックを持たない旧版のデータ、識別できないディレクトリは残す。

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

const DEFAULT_APP_SUBDIR: &str = "virtual-avatar-connect";
const DEFAULT_RUNTIME_SUBDIR: &str = "runtime";
/// 既定のインライン添付の最大バイト数。アプリ設定で上書き可能。
pub const DEFAULT_INLINE_MAX_BYTES: u64 = 32 * 1024;
const STALE_SESSION_SECS: u64 = 24 * 60 * 60;
const SESSION_LEASE_FILE: &str = ".session-lease";

/// ランタイム一時ディレクトリ初期化に必要なアプリ設定の最小断面。
#[derive(Debug, Clone, Default)]
pub struct RuntimePathConfig {
	pub runtime_dir: Option<PathBuf>,
	pub attachment_inline_max_bytes: Option<u64>,
}

/// プロセス生存期間中に共有する一時ディレクトリ情報。
///
/// `session_dir` 配下に添付（音声・画像など）を書き出し、`href` ベースで Web UI から配信する想定。
#[derive(Debug, Clone)]
pub struct RuntimePaths {
	/// 全セッション共通のルート（例: `%LOCALAPPDATA%/virtual-avatar-connect/runtime`）。
	pub root: PathBuf,
	/// 本プロセス専用のセッションディレクトリ（`root/<session_id>`）。
	pub session_dir: PathBuf,
	/// 本プロセスのセッション ID（`YYYYMMDDTHHMMSSZ-<rand8>`）。
	pub session_id: String,
	/// インライン添付の最大バイト数。これを超えるバイナリは自動で `File` 添付に降格する運用を想定。
	pub inline_max_bytes: u64,
	// Keep the lease alive across clones and release it on process exit.
	_session_lease: Arc<std::fs::File>,
}

impl RuntimePaths {
	/// 設定から一時ディレクトリ情報を初期化し、セッションディレクトリを作成して返す。
	///
	/// 既定ルートが取得できないプラットフォームでは `./.vac-runtime/` にフォールバックする。
	pub fn init(config: RuntimePathConfig) -> Result<Self> {
		let root = resolve_root(&config);
		std::fs::create_dir_all(&root)?;

		let session_id = new_session_id();
		let session_dir = root.join(&session_id);
		std::fs::create_dir(&session_dir)?;
		let lease = std::fs::OpenOptions::new().read(true).write(true).create_new(true)
			.open(session_dir.join(SESSION_LEASE_FILE))?;
		lease.lock()?;

		let inline_max_bytes = config.attachment_inline_max_bytes.unwrap_or(DEFAULT_INLINE_MAX_BYTES);

		cleanup_stale_sessions(&root, &session_id);

		log::info!(
			"《Runtime》 一時ディレクトリを初期化しました: root={:?} session_id={} inline_max_bytes={}",
			root,
			session_id,
			inline_max_bytes
		);

		Ok(Self {
			root,
			session_dir,
			session_id,
			inline_max_bytes,
			_session_lease: Arc::new(lease),
		})
	}

	/// セッションディレクトリ配下に相対パスを解決する（ファイル自体は作成しない）。
	pub fn session_path<P: AsRef<Path>>(&self, relative: P) -> PathBuf {
		self.session_dir.join(relative)
	}
}

fn resolve_root(config: &RuntimePathConfig) -> PathBuf {
	if let Some(p) = config.runtime_dir.as_ref() {
		return p.clone();
	}
	if let Some(base) = dirs::data_local_dir() {
		return base.join(DEFAULT_APP_SUBDIR).join(DEFAULT_RUNTIME_SUBDIR);
	}
	// プラットフォーム固有 API が無ければ cwd にフォールバック。
	PathBuf::from(".vac-runtime")
}

fn new_session_id() -> String {
	let ts = jiff::Timestamp::now().strftime("%Y%m%dT%H%M%SZ").to_string();
	let rand: u32 = rand::random();
	format!("{}-{:08x}", ts, rand)
}

fn cleanup_stale_sessions(root: &Path, current_session_id: &str) {
	cleanup_stale_sessions_at(root, current_session_id, SystemTime::now());
}

fn cleanup_stale_sessions_at(root: &Path, current_session_id: &str, now: SystemTime) {
	let Ok(entries) = std::fs::read_dir(root) else {
		return;
	};
	let threshold = Duration::from_secs(STALE_SESSION_SECS);
	for ent in entries.flatten() {
		let path = ent.path();
		if !ent.file_type().is_ok_and(|kind| kind.is_dir()) {
			continue;
		}
		let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
			continue;
		};
		if name == current_session_id {
			continue;
		}
		if !is_session_directory_name(name) { continue; }
		let Ok(meta) = ent.metadata() else { continue };
		let modified = meta.modified().ok().unwrap_or(SystemTime::UNIX_EPOCH);
		let age = now.duration_since(modified).unwrap_or_default();
		if age < threshold {
			continue;
		}
		let lease_path = path.join(SESSION_LEASE_FILE);
		// Legacy sessions have no lease; their activity cannot be determined safely.
		if !std::fs::symlink_metadata(&lease_path).is_ok_and(|m| m.file_type().is_file()) { continue; }
		let Ok(lease) = std::fs::OpenOptions::new().read(true).write(true).open(&lease_path) else { continue };
		if lease.try_lock().is_err() { continue; }
		match std::fs::remove_dir_all(&path) {
			Ok(()) => log::debug!(
				"《Runtime》 古いセッションディレクトリを削除しました: {:?}（age {}s）",
				path,
				age.as_secs()
			),
			Err(e) => log::debug!("《Runtime》 セッションディレクトリの削除に失敗（スキップ）: {:?} - {}", path, e),
		}
	}
}

fn is_session_directory_name(name: &str) -> bool {
	let b = name.as_bytes();
	b.len() == 25 && b[8] == b'T' && b[15] == b'Z' && b[16] == b'-'
		&& b[..8].iter().chain(b[9..15].iter()).all(u8::is_ascii_digit)
		&& b[17..].iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	#[ignore = "subprocess worker for cleanup_respects_another_process"]
	fn session_lease_worker() {
		let root = PathBuf::from(std::env::var_os("VAC_SESSION_LEASE_TEST_ROOT").unwrap());
		let owner = RuntimePaths::init(RuntimePathConfig { runtime_dir: Some(root.join("sessions")), ..Default::default() }).unwrap();
		std::fs::write(root.join("ready"), &owner.session_id).unwrap();
		let deadline = std::time::Instant::now() + Duration::from_secs(10);
		while !root.join("release").exists() {
			assert!(std::time::Instant::now() < deadline);
			std::thread::sleep(Duration::from_millis(10));
		}
	}

	#[test]
	fn cleanup_respects_another_process() {
		let root = tempfile::tempdir().unwrap();
		let mut child = std::process::Command::new(std::env::current_exe().unwrap())
			.args(["--exact", "runtime::tests::session_lease_worker", "--ignored"])
			.env("VAC_SESSION_LEASE_TEST_ROOT", root.path())
			.stdout(std::process::Stdio::null()).spawn().unwrap();
		let deadline = std::time::Instant::now() + Duration::from_secs(8);
		while !root.path().join("ready").exists() && std::time::Instant::now() < deadline {
			std::thread::sleep(Duration::from_millis(10));
		}
		if !root.path().join("ready").exists() { let _ = child.kill(); let _ = child.wait(); panic!("worker did not become ready"); }
		let id = std::fs::read_to_string(root.path().join("ready")).unwrap();
		let sessions = root.path().join("sessions");
		let future = SystemTime::now() + Duration::from_secs(STALE_SESSION_SECS + 5);
		cleanup_stale_sessions_at(&sessions, "other-instance", future);
		let preserved = sessions.join(&id).exists();
		std::fs::write(root.path().join("release"), b"").unwrap();
		assert!(child.wait().unwrap().success());
		assert!(preserved, "active subprocess data deleted");
		cleanup_stale_sessions_at(&sessions, "other-instance", future);
		assert!(!sessions.join(id).exists());
	}

	#[test]
	fn cleanup_keeps_recent_current_legacy_and_unrecognized_directories() {
		let root = tempfile::tempdir().unwrap();
		let owner = RuntimePaths::init(RuntimePathConfig { runtime_dir: Some(root.path().into()), ..Default::default() }).unwrap();
		let recent = owner.session_dir.clone();
		let current_id = owner.session_id.clone();
		drop(owner);
		cleanup_stale_sessions_at(root.path(), "other-instance", SystemTime::now());
		assert!(recent.exists());
		let legacy = root.path().join("20000101T000000Z-1234abcd");
		let unrelated = root.path().join("user-data");
		for path in [&legacy, &unrelated] { std::fs::create_dir(path).unwrap(); std::fs::write(path.join("keep"), b"keep").unwrap(); }
		std::fs::write(unrelated.join(SESSION_LEASE_FILE), b"").unwrap();
		cleanup_stale_sessions_at(root.path(), &current_id, SystemTime::now() + Duration::from_secs(STALE_SESSION_SECS + 5));
		assert!(recent.exists());
		assert!(legacy.join("keep").exists());
		assert!(unrelated.join("keep").exists());
	}

	#[test]
	fn cleanup_preserves_live_session_until_last_owner_drops() {
		let root = tempfile::tempdir().unwrap();
		let owner = RuntimePaths::init(RuntimePathConfig { runtime_dir: Some(root.path().into()), ..Default::default() }).unwrap();
		let live = owner.session_dir.clone();
		std::fs::write(live.join("speech.wav"), b"active attachment").unwrap();
		let clone = owner.clone();
		drop(owner);
		let future = SystemTime::now() + Duration::from_secs(STALE_SESSION_SECS + 5);
		cleanup_stale_sessions_at(root.path(), "other-instance", future);
		assert_eq!(std::fs::read(live.join("speech.wav")).unwrap(), b"active attachment");
		drop(clone);
		cleanup_stale_sessions_at(root.path(), "other-instance", future);
		assert!(!live.exists(), "released stale session should be reclaimed");
	}
}

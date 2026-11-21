//! UNMF/Z data plane の Zenoh transport 設定。
//!
//! key expression と接続経路は別の責務として扱う。既定は Zenoh の自動探索を
//! そのまま維持し、LAN 利用時だけ publisher の listen endpoint と subscriber の
//! connect endpoint を追加する。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use un_motion_frame_zenoh::{ZenohSessionBackend, ZenohSessionConfig, ZenohSubscriberBackend};

pub const DEFAULT_UNMFZ_TCP_PORT: u16 = 47_447;

fn default_listen_ip() -> IpAddr {
	IpAddr::V4(Ipv4Addr::UNSPECIFIED)
}

fn default_port() -> u16 {
	DEFAULT_UNMFZ_TCP_PORT
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct UnmfzListenConf {
	#[serde(default)]
	pub enabled: bool,
	#[serde(default = "default_listen_ip")]
	pub ip: IpAddr,
	#[serde(default = "default_port")]
	pub port: u16,
}

impl Default for UnmfzListenConf {
	fn default() -> Self {
		Self {
			enabled: false,
			ip: default_listen_ip(),
			port: default_port(),
		}
	}
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct UnmfzConnectConf {
	#[serde(default = "crate::utility::bool_true")]
	pub enabled: bool,
	pub ip: IpAddr,
	#[serde(default = "default_port")]
	pub port: u16,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct UnmfzTransportConf {
	#[serde(default)]
	pub listen: UnmfzListenConf,
	#[serde(default)]
	pub connect: Vec<UnmfzConnectConf>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UnmfzTransportSnapshot {
	pub mode: &'static str,
	pub automatic_discovery: bool,
	pub listen_endpoint: Option<String>,
	pub connect_endpoints: Vec<String>,
}

/// UNVACプロセス内のUNMF/Z publisher / subscriberで共有するZenoh session。
///
/// 複数FlowgraphノードやAvatarLinkが同じlisten portを重複bindしないよう、sessionは
/// 初回利用時に一度だけ開く。各backendはcloneした同一session handleを使う。
#[derive(Clone)]
pub struct UnmfzTransportRuntime {
	config: UnmfzTransportConf,
	session: Arc<SharedSession>,
}

#[derive(Default)]
struct SharedSession(Mutex<Option<zenoh::Session>>);

impl SharedSession {
	fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, Option<zenoh::Session>>> {
		self.0.lock()
	}
}

impl Drop for SharedSession {
	fn drop(&mut self) {
		// Zenoh 1.9 Session::drop calls close().wait(), even after an explicit close.
		// Actix workers use a current-thread runtime, which cannot run that blocking close.
		let current_thread = tokio::runtime::Handle::try_current()
			.is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::CurrentThread);
		if current_thread {
			let session = self.0.get_mut().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
			if let Some(session) = session {
				if std::thread::spawn(move || drop(session)).join().is_err() {
					log::error!("UNMF/Z session disposal thread panicked");
				}
			}
		}
	}
}

impl std::fmt::Debug for UnmfzTransportRuntime {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("UnmfzTransportRuntime")
			.field("config", &self.config)
			.field(
				"session_open",
				&self.session.lock().map(|session| session.is_some()).unwrap_or(false),
			)
			.finish()
	}
}

impl UnmfzTransportRuntime {
	pub fn new(config: UnmfzTransportConf) -> Self {
		Self {
			config,
			session: Arc::new(SharedSession::default()),
		}
	}

	pub fn snapshot(&self) -> UnmfzTransportSnapshot {
		self.config.snapshot()
	}

	pub fn publisher_backend(&self) -> Result<ZenohSessionBackend, String> {
		self.session().map(ZenohSessionBackend::from_session)
	}

	pub fn subscriber_backend(&self) -> Result<ZenohSubscriberBackend, String> {
		self.session().map(ZenohSubscriberBackend::from_session)
	}

	fn session(&self) -> Result<zenoh::Session, String> {
		let mut session = self.session.lock().map_err(|_| "UNMF/Z session lock poisoned".to_string())?;
		if let Some(session) = session.as_ref() {
			return Ok(session.clone());
		}
		let backend = ZenohSessionBackend::open(&self.config.session_config()).map_err(|error| error.to_string())?;
		let opened = backend.session().clone();
		*session = Some(opened.clone());
		Ok(opened)
	}
}

impl Default for UnmfzTransportRuntime {
	fn default() -> Self {
		Self::new(UnmfzTransportConf::default())
	}
}

impl UnmfzTransportConf {
	pub fn validate(&self) -> anyhow::Result<()> {
		if self.listen.enabled && self.listen.port == 0 {
			anyhow::bail!("unmfz.listen.port must be between 1 and 65535");
		}
		let mut seen = HashSet::new();
		for (index, endpoint) in self.connect.iter().enumerate().filter(|(_, endpoint)| endpoint.enabled) {
			if endpoint.port == 0 {
				anyhow::bail!("unmfz.connect[{index}].port must be between 1 and 65535");
			}
			if endpoint.ip.is_unspecified() {
				anyhow::bail!("unmfz.connect[{index}].ip must be a destination IP address");
			}
			let address = SocketAddr::new(endpoint.ip, endpoint.port);
			if !seen.insert(address) {
				anyhow::bail!("unmfz.connect[{index}] duplicates {address}");
			}
		}
		Ok(())
	}

	pub fn publisher_session_config(&self) -> ZenohSessionConfig {
		match self.listen_endpoint() {
			Some(endpoint) => ZenohSessionConfig::default().with_listen_endpoint(endpoint),
			None => ZenohSessionConfig::default(),
		}
	}

	pub fn subscriber_session_config(&self) -> ZenohSessionConfig {
		self.connect_endpoints()
			.into_iter()
			.fold(ZenohSessionConfig::default(), |config, endpoint| {
				config.with_connect_endpoint(endpoint)
			})
	}

	pub fn session_config(&self) -> ZenohSessionConfig {
		let config = match self.listen_endpoint() {
			Some(endpoint) => ZenohSessionConfig::default().with_listen_endpoint(endpoint),
			None => ZenohSessionConfig::default(),
		};
		self.connect_endpoints()
			.into_iter()
			.fold(config, |config, endpoint| config.with_connect_endpoint(endpoint))
	}

	pub fn snapshot(&self) -> UnmfzTransportSnapshot {
		let listen_endpoint = self.listen_endpoint();
		let connect_endpoints = self.connect_endpoints();
		let mode = match (listen_endpoint.is_some(), connect_endpoints.is_empty()) {
			(false, true) => "automatic",
			(true, true) => "listen",
			(false, false) => "connect",
			(true, false) => "listen_connect",
		};
		UnmfzTransportSnapshot {
			mode,
			automatic_discovery: true,
			listen_endpoint,
			connect_endpoints,
		}
	}

	fn listen_endpoint(&self) -> Option<String> {
		self.listen
			.enabled
			.then(|| format!("tcp/{}", SocketAddr::new(self.listen.ip, self.listen.port)))
	}

	fn connect_endpoints(&self) -> Vec<String> {
		self.connect
			.iter()
			.filter(|endpoint| endpoint.enabled)
			.map(|endpoint| format!("tcp/{}", SocketAddr::new(endpoint.ip, endpoint.port)))
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test(flavor = "current_thread")]
	async fn shared_session_can_be_dropped_on_http_worker_runtime() {
		let mut config = zenoh::Config::default();
		config.insert_json5("scouting/multicast/enabled", "false").unwrap();
		config.insert_json5("listen/endpoints", "[]").unwrap();
		let session = std::thread::spawn(move || {
			use zenoh::Wait as _;
			zenoh::open(config).wait().unwrap()
		})
		.join()
		.unwrap();
		let transport = UnmfzTransportRuntime::default();
		*transport.session.lock().unwrap() = Some(session);
		let last_http_owner = transport.clone();
		drop(transport);
		drop(last_http_owner);
	}

	#[test]
	fn default_preserves_automatic_discovery() {
		let config = UnmfzTransportConf::default();
		assert_eq!(config.snapshot().mode, "automatic");
		assert_eq!(config.snapshot().listen_endpoint, None);
		assert!(config.snapshot().connect_endpoints.is_empty());
		assert_eq!(config.publisher_session_config().listen_endpoints(), None);
		assert_eq!(config.subscriber_session_config().connect_endpoints(), None);
	}

	#[test]
	fn explicit_ip_and_port_build_role_specific_endpoints() {
		let config = UnmfzTransportConf {
			listen: UnmfzListenConf {
				enabled: true,
				ip: "0.0.0.0".parse().unwrap(),
				port: DEFAULT_UNMFZ_TCP_PORT,
			},
			connect: vec![
				UnmfzConnectConf {
					enabled: true,
					ip: "192.0.2.10".parse().unwrap(),
					port: DEFAULT_UNMFZ_TCP_PORT,
				},
				UnmfzConnectConf {
					enabled: true,
					ip: "192.0.2.11".parse().unwrap(),
					port: 47_448,
				},
			],
		};
		config.validate().unwrap();
		assert_eq!(
			config.publisher_session_config().listen_endpoints(),
			Some(["tcp/0.0.0.0:47447".to_owned()].as_slice())
		);
		assert_eq!(
			config.subscriber_session_config().connect_endpoints(),
			Some(["tcp/192.0.2.10:47447".to_owned(), "tcp/192.0.2.11:47448".to_owned()].as_slice())
		);
		let shared = config.session_config();
		assert_eq!(shared.listen_endpoints(), Some(["tcp/0.0.0.0:47447".to_owned()].as_slice()));
		assert_eq!(
			shared.connect_endpoints(),
			Some(["tcp/192.0.2.10:47447".to_owned(), "tcp/192.0.2.11:47448".to_owned()].as_slice())
		);
	}

	#[test]
	fn invalid_and_duplicate_connect_endpoints_are_rejected() {
		let unspecified = UnmfzTransportConf {
			connect: vec![UnmfzConnectConf {
				enabled: true,
				ip: "0.0.0.0".parse().unwrap(),
				port: DEFAULT_UNMFZ_TCP_PORT,
			}],
			..Default::default()
		};
		assert!(unspecified.validate().unwrap_err().to_string().contains("destination IP"));

		let endpoint = UnmfzConnectConf {
			enabled: true,
			ip: "192.0.2.10".parse().unwrap(),
			port: DEFAULT_UNMFZ_TCP_PORT,
		};
		let duplicate = UnmfzTransportConf {
			connect: vec![endpoint.clone(), endpoint],
			..Default::default()
		};
		assert!(duplicate.validate().unwrap_err().to_string().contains("duplicates"));
	}

	#[test]
	fn runtime_clones_share_one_lazy_session_slot_without_opening_a_socket() {
		let runtime = UnmfzTransportRuntime::default();
		let cloned = runtime.clone();
		assert!(Arc::ptr_eq(&runtime.session, &cloned.session));
		assert!(runtime.session.lock().unwrap().is_none());
	}
}

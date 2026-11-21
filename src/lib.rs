pub(crate) mod ai;
mod app_core;
mod args;
mod avatar_hand_pose;
mod avatar_motion;
mod avatar_renderer_probe;
mod bootstrap;
pub(crate) mod bridges;
mod conf;
mod control_events;
pub(crate) mod datetime;
mod desktop;
mod diagnostic_images;
mod error;
mod libretranslate;
mod logger;
pub(crate) mod managed_app;
pub(crate) mod migrate;
mod motion;
mod processor;
pub(crate) mod profile_store;
pub(crate) mod profile_templates;
pub(crate) mod quick_effects;
mod resource;
mod runtime;
pub(crate) mod runtime_health;
pub(crate) mod shutdown;
pub(crate) mod speech_scheduler;
pub(crate) mod speech_playback;
mod state;
pub(crate) mod stream_perception;
pub(crate) mod stream_state;
mod streaming_contract;
#[cfg(all(feature = "tools", feature = "voice-vosk"))]
pub mod stt_eval;
pub(crate) mod twitch;
mod twitch_oauth_sessions;
pub(crate) mod un_app_api;
pub(crate) mod unmfz_transport;

pub(crate) mod file_publish;
pub mod flowgraph;
pub mod utility;
pub mod web_interface;

pub use crate::{
	args::Args,
	conf::Conf,
	conf::*,
	error::{Error, Result},
	processor::*,
	state::{Attachment, ChannelData, ChannelDatum, DataSource, SharedChannelData, SharedState, State},
};
pub use std::sync::Arc;
pub use tokio::sync::RwLock;

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};
use tokio::sync::Mutex;

/// `MixerDeviceSink`（ストリーム）と `Player` をまとめて保持する。rodio 0.22 では前者を先に drop してはいけない。
pub struct AudioSink {
	_device: Option<MixerDeviceSink>,
	lazy_open: bool,
	pub player: Player,
}

impl AudioSink {
	pub(crate) fn open_default() -> crate::Result<Self> {
		let device = DeviceSinkBuilder::open_default_sink()?;
		let player = Player::connect_new(device.mixer());
		Ok(Self { _device: Some(device), lazy_open: false, player })
	}

	/// Non-audio workflows must not require an output device at startup.
	pub(crate) fn deferred() -> Self {
		let (player, _output) = Player::new();
		Self { _device: None, lazy_open: true, player }
	}

	pub(crate) fn ensure_device(&mut self) -> crate::Result<()> {
		self.ensure_device_with(Self::open_default)
	}

	fn ensure_device_with(&mut self, open: impl FnOnce() -> crate::Result<Self>) -> crate::Result<()> {
		if self.lazy_open {
			// Keep the deferred state after failure so a later playback can retry.
			*self = open()?;
		} else if self._device.is_none() {
			return Err(anyhow::anyhow!("audio output is disconnected").into());
		}
		Ok(())
	}

	/// For non-audio tests only: no OS device, playback, or queue consumption.
	#[cfg(test)]
	pub(crate) fn disconnected_for_test() -> Self {
		let (player, _output) = Player::new();
		Self { _device: None, lazy_open: false, player }
	}
}

pub type SharedAudioSink = Arc<Mutex<AudioSink>>;

#[cfg(test)]
mod deferred_audio_tests {
	use super::*;

	#[test]
	fn startup_needs_no_device_and_failed_playback_can_retry() {
		let mut sink = AudioSink::deferred();
		assert!(sink._device.is_none());
		assert!(sink.player.empty());
		let mut attempts = 0;
		for _ in 0..2 {
			assert!(sink.ensure_device_with(|| {
				attempts += 1;
				Err(anyhow::anyhow!("no output device in this simulated host").into())
			}).is_err());
			assert!(sink.lazy_open);
			assert!(sink.player.empty());
		}
		assert_eq!(attempts, 2);
	}

	#[test]
	fn disconnected_test_sink_never_opens_a_real_device() {
		let mut sink = AudioSink::disconnected_for_test();
		assert!(sink.ensure_device_with(|| panic!("must not access output device")).is_err());
	}
}

impl std::fmt::Debug for AudioSink {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "AudioSink")
	}
}

/// 互換入口。移行期間中は CLI runner と同じ動きをする。
pub async fn run() -> Result<()> {
	run_cli().await
}

/// コンソール付きの玄人・開発者向け runner。
pub async fn run_cli() -> Result<()> {
	bootstrap::run_app_core_with_standard_bootstrap().await
}

/// Tauri / tray を使わない desktop runner 入口。
///
/// 非 Windows fallback として CLI と同じ runtime を起動する。
pub async fn run_desktop_headless() -> Result<()> {
	bootstrap::run_app_core_with_standard_bootstrap().await
}

/// system tray 付きの desktop runner。
///
/// Windows では tray menu から Tauri WebView GUI を開き、`ShutdownBroker` 経由で VAC を終了する。
/// それ以外の OS は Tauri shell 導入まで headless desktop runner と同じ起動にする。
pub fn run_desktop() -> Result<()> {
	desktop::run()
}

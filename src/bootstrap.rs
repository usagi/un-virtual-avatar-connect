use crate::{app_core, logger, Args, AudioSink, Conf, Result};
use anyhow::Context;
use std::sync::Arc;
use tokio::sync::Mutex;

const DEFAULT_CONF_TEMPLATE: &str = include_str!("../conf.toml");
static PROFILE_LEASE: std::sync::OnceLock<std::fs::File> = std::sync::OnceLock::new();

pub(crate) async fn run_app_core_with_standard_bootstrap() -> Result<()> {
	let core = boot_app_core_with_standard_bootstrap().await?;
	core.run().await.into_result()
}

pub(crate) async fn boot_app_core_with_standard_bootstrap() -> Result<app_core::AppCore> {
	boot_app_core(BootMode::Cli).await
}

pub(crate) async fn boot_app_core_for_desktop() -> Result<app_core::AppCore> {
	boot_app_core(BootMode::Desktop).await
}

#[derive(Clone, Copy)]
enum BootMode {
	Cli,
	Desktop,
}

async fn boot_app_core(mode: BootMode) -> Result<app_core::AppCore> {
	// ロガーの実装を初期化
	logger::init();

	// コマンドライン引数をパースし、ログレベルを更新
	let mut args = Args::new();
	wait_for_restart_parent(args.restart_parent_pid).await?;

	// 音声出力は最初の再生時に開く。通知・設定検証だけならデバイス不要。
	let audio_sink = Arc::new(Mutex::new(AudioSink::deferred()));

	if matches!(mode, BootMode::Desktop) {
		prepare_desktop_conf(&mut args)?;
	}
	// conf を必要としない特殊な動作モードを実行
	args.execute_special_modes_without_conf(audio_sink.clone()).await?;
	// 設定を読み込みし、ログレベルを更新
	if let Some(lease) = crate::profile_store::lifecycle::acquire_lease(std::path::Path::new(&args.conf))? {
		let _ = PROFILE_LEASE.set(lease);
	}
	let conf = Conf::new(&args)?;
	// conf を必要とする特殊な動作モードを実行
	args.execute_special_modes_with_conf(&conf).await?;

	// run_with 機能の実行
	conf.execute_run_with()?;

	app_core::AppCore::boot(conf, audio_sink).await
}

const RESTART_PARENT_WAIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) async fn wait_for_restart_parent(parent_pid: Option<u32>) -> Result<()> {
	let Some(parent_pid) = parent_pid else {
		return Ok(());
	};
	if parent_pid == std::process::id() {
		return Err(anyhow::anyhow!("restart parent PIDが現在のPIDと一致しています: {parent_pid}").into());
	}

	use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
	let parent_pid = Pid::from_u32(parent_pid);
	let deadline = tokio::time::Instant::now() + RESTART_PARENT_WAIT_TIMEOUT;
	let mut system = System::new();
	loop {
		system.refresh_processes_specifics(ProcessesToUpdate::Some(&[parent_pid]), true, ProcessRefreshKind::nothing());
		if system.process(parent_pid).is_none() {
			return Ok(());
		}
		if tokio::time::Instant::now() >= deadline {
			return Err(anyhow::anyhow!("再起動元プロセスの終了待機がtimeoutしました: pid={parent_pid}").into());
		}
		tokio::time::sleep(std::time::Duration::from_millis(100)).await;
	}
}

fn prepare_desktop_conf(args: &mut Args) -> Result<()> {
	if args.conf_is_explicit {
		return Ok(());
	}
	let default_conf = crate::profile_store::desktop_default_conf(DEFAULT_CONF_TEMPLATE)?;
	let active = crate::profile_store::ensure_active_profile(&default_conf)?;
	std::env::set_current_dir(&active.profile_dir)
		.with_context(|| format!("active profile directoryへ移動できません: {}", active.profile_dir.display()))?;
	args.conf = active.conf_path.to_string_lossy().into_owned();
	log::info!(
		"《Desktop》 user profileを選択しました: id={} conf={} store={} profiles={} onboarding_completed={}",
		active.profile_id,
		active.conf_path.display(),
		active.root.display(),
		active.profiles_dir.display(),
		active.settings.onboarding_completed
	);
	Ok(())
}

#[cfg(test)]
#[path = "bootstrap_profile_tests.rs"]
mod profile_selection_tests;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn embedded_desktop_conf_is_valid() {
		toml::from_str::<Conf>(DEFAULT_CONF_TEMPLATE).expect("embedded desktop conf must parse");
	}
}

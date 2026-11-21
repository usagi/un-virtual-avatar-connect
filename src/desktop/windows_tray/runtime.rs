use crate::app_core::AppCore;
use crate::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::task::JoinHandle;

pub(super) fn build_tokio_runtime() -> Result<Arc<tokio::runtime::Runtime>> {
	Ok(Arc::new(
		tokio::runtime::Builder::new_multi_thread()
			.enable_all()
			.build()
			.map_err(anyhow::Error::from)?,
	))
}

pub(super) fn spawn_vac_runtime_task(
	runtime: &Arc<tokio::runtime::Runtime>,
	core: AppCore,
	app_handle_rx: tokio::sync::oneshot::Receiver<tauri::AppHandle>,
	exit_ui_when_finished: Arc<AtomicBool>,
) -> JoinHandle<()> {
	runtime.spawn(async move {
		let app_handle = app_handle_rx.await.ok();
		let (serve_result, cleanup_result) = core.run().await.into_parts();
		if let Err(e) = serve_result {
			log::error!("《Desktop》 VAC runtime serve がエラー終了しました: {e}");
		}
		if let Err(e) = cleanup_result {
			log::error!("《Desktop》 VAC runtime cleanup がエラー終了しました: {e}");
		}
		if exit_ui_when_finished.load(Ordering::SeqCst) {
			if let Some(app_handle) = app_handle {
				// AppHandle::exit may terminate the process directly if the event
				// loop has already closed. Dispatch onto that loop first, so a
				// completed worker cannot bypass the caller's cleanup/error path.
				let exit_handle = app_handle.clone();
				let _ = app_handle.run_on_main_thread(move || {
					if exit_ui_when_finished.load(Ordering::SeqCst) {
						exit_handle.exit(0);
					}
				});
			}
		}
	})
}

pub(super) fn wait_for_vac_runtime_shutdown(runtime: &Arc<tokio::runtime::Runtime>, serve_handle: JoinHandle<()>, timeout: Duration) {
	// Construct the timer inside the runtime context. Constructing it as a
	// block_on argument panics when desktop shutdown runs on the native thread.
	match runtime.block_on(async { tokio::time::timeout(timeout, serve_handle).await }) {
		Ok(Ok(())) => {
			log::info!("《Desktop》 VAC runtime は正常に停止しました。");
		}
		Ok(Err(e)) => {
			log::error!("《Desktop》 VAC runtime task の終了待機に失敗しました: {e}");
		}
		Err(_) => {
			log::warn!("《Desktop》 VAC runtime の停止待機が timeout しました: {timeout:?}");
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn native_thread_can_wait_for_runtime_shutdown_without_entering_tokio() {
		let runtime = build_tokio_runtime().unwrap();
		assert!(tokio::runtime::Handle::try_current().is_err());
		let completed = runtime.spawn(async {});
		wait_for_vac_runtime_shutdown(&runtime, completed, Duration::from_secs(1));
		let pending = runtime.spawn(std::future::pending::<()>());
		wait_for_vac_runtime_shutdown(&runtime, pending, Duration::from_millis(1));
		assert!(tokio::runtime::Handle::try_current().is_err());
	}
}

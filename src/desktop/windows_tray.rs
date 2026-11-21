use crate::Result;
use runtime::{build_tokio_runtime, spawn_vac_runtime_task, wait_for_vac_runtime_shutdown};
use ui::setup_desktop_ui;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};

mod runtime;
mod ui;

const DESKTOP_RUNTIME_SHUTDOWN_WAIT_SECONDS: u64 = 10;

pub fn run() -> Result<()> {
	#[cfg(feature="desktop-native-review")]
	let _review_root=ui::prepare_native_review()?;
	let context=tauri::generate_context!("./tauri.conf.json");
	#[cfg(feature="desktop-native-review")]
	let context={let mut context=context;context.config_mut().identifier="network.usagi.virtual-avatar-connect.native-review".into();context};
	let runtime = build_tokio_runtime()?;

	// Parse the same grammar as bootstrap. Explicit settings and special tools
	// remain independent; ordinary launches share one primary per settings store.
	use clap::{CommandFactory, FromArgMatches, parser::ValueSource};
	let matches = crate::Args::command().get_matches();
	let args = crate::Args::from_arg_matches(&matches).expect("Args grammar");
	let ordinary = args.desktop_primary || !crate::Args::command().get_arguments().any(|arg| {
		matches.value_source(arg.get_id().as_str()) == Some(ValueSource::CommandLine)
			&& !matches!(arg.get_id().as_str(), "debug" | "restart_parent_pid")
	});
	let primary = if ordinary {
		runtime.block_on(crate::bootstrap::wait_for_restart_parent(args.restart_parent_pid))?;
		match super::primary_instance::admit(&crate::profile_store::user_config_root()?)? {
			super::primary_instance::Admission::Activated => return Ok(()),
			super::primary_instance::Admission::Primary(guard) => {
				super::PRIMARY_DESKTOP.store(true, Ordering::SeqCst);
				Some(Arc::new(guard))
			}
		}
	} else { None };
	let primary_for_setup = primary.clone();
	let activation_task = Arc::new(Mutex::new(None::<tokio::task::JoinHandle<()>>));
	let activation_for_setup = activation_task.clone();
	let runtime_for_setup = runtime.clone();
	let core = runtime.block_on(crate::bootstrap::boot_app_core_for_desktop())?;
	let (gui_url, shutdown) = core.runtime_handle().into_parts();
	let runtime_for_after_run = runtime.clone();
	let shutdown_for_setup = shutdown.clone();
	let (app_handle_tx, app_handle_rx) = tokio::sync::oneshot::channel::<tauri::AppHandle>();
	let exit_ui_when_runtime_finishes = Arc::new(AtomicBool::new(true));
	let serve_handle = spawn_vac_runtime_task(&runtime, core, app_handle_rx, exit_ui_when_runtime_finishes.clone());
	let setup_error = Arc::new(Mutex::new(None));
	let setup_error_writer = setup_error.clone();
	let exit_ui_for_setup = exit_ui_when_runtime_finishes.clone();

	let ui_result = tauri::Builder::default()
		.setup(move |app| {
			let result = setup_desktop_ui(app, gui_url.clone(), shutdown_for_setup.clone(), app_handle_tx);
			if result.is_ok() {
				if let Some(primary) = primary_for_setup {
					let handle = app.handle().clone();
					*activation_for_setup.lock().expect("activation task") = Some(runtime_for_setup.spawn(async move {
						loop {
							match primary.take_activation() {
								Ok(true) => { let show = handle.clone(); let _ = handle.run_on_main_thread(move || ui::show_gui(&show)); }
								Ok(false) => {},
								Err(error) => log::warn!("VAC activation request: {error}"),
							}
							tokio::time::sleep(std::time::Duration::from_millis(250)).await;
						}
					}));
				}
			}
			if let Err(error) = result {
				// Returning Err from Tauri's setup hook panics in the event loop.
				// Record it and return through run_return instead, so cleanup runs.
				exit_ui_for_setup.store(false, Ordering::SeqCst);
				*setup_error_writer.lock().expect("desktop setup error") = Some(error);
				shutdown_for_setup.trigger(crate::shutdown::ShutdownReason::Desktop);
				app.handle().exit(1);
			}
			Ok(())
		})
		.build(context)
		.map(|app| { app.run_return(|_, _| {}); })
		.map_err(anyhow::Error::from);
	if let Some(task) = activation_task.lock().expect("activation task").take() { task.abort(); }
	exit_ui_when_runtime_finishes.store(false, Ordering::SeqCst);

	shutdown.trigger(crate::shutdown::ShutdownReason::Desktop);
	wait_for_vac_runtime_shutdown(
		&runtime_for_after_run,
		serve_handle,
		std::time::Duration::from_secs(DESKTOP_RUNTIME_SHUTDOWN_WAIT_SECONDS),
	);
	// A failed WebView/tray setup must also stop the already-started core before
	// returning to the desktop error presentation. Otherwise its modal wait can
	// leave listeners and linked applications running without a usable window.
	ui_result?;
	if let Some(error) = setup_error.lock().expect("desktop setup error").take() {
		return Err(error.into());
	}
	Ok(())
}

pub(crate) mod primary_instance;
static PRIMARY_DESKTOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) fn is_primary_desktop() -> bool { PRIMARY_DESKTOP.load(std::sync::atomic::Ordering::SeqCst) }

#[cfg(target_os = "windows")]
mod windows_tray;

#[cfg(target_os = "windows")]
pub fn run() -> crate::Result<()> {
	windows_tray::run()
}

#[cfg(not(target_os = "windows"))]
pub fn run() -> crate::Result<()> {
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.map_err(anyhow::Error::from)?;
	runtime.block_on(crate::run_desktop_headless())
}

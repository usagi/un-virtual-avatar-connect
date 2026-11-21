use crate::shutdown::{ShutdownBroker, ShutdownReason};
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

const TRAY_ID: &str = "vac-tray";
const MAIN_WINDOW_LABEL: &str = "main";
const MENU_OPEN_GUI: &str = "vac-open-gui";
const MENU_QUIT: &str = "vac-quit";
const APP_TITLE: &str = "Virtual Avatar Connect";
const MAIN_WINDOW_WIDTH: f64 = 1280.0;
const MAIN_WINDOW_HEIGHT: f64 = 820.0;

pub(super) fn setup_desktop_ui(
	app: &mut tauri::App,
	gui_url: String,
	shutdown: Arc<ShutdownBroker>,
	app_handle_tx: tokio::sync::oneshot::Sender<tauri::AppHandle>,
) -> anyhow::Result<()> {
	let app_handle = app.handle().clone();
	let _ = app_handle_tx.send(app.handle().clone());
	#[cfg(feature = "desktop-native-review")]
	if prepare_native_review()?.join("fail-ui-setup").is_file() {
		anyhow::bail!("isolated review: injected desktop UI setup failure");
	}
	setup_tray(app, shutdown, gui_url.parse::<tauri::Url>()?.origin().ascii_serialization())?;
	let window = setup_main_window(app, gui_url)?;
	attach_hide_on_close(window, app_handle);
	log::info!("《Desktop》 Tauri system tray と WebView を初期化しました。");
	Ok(())
}

fn setup_tray(app: &mut tauri::App, shutdown: Arc<ShutdownBroker>, origin: String) -> tauri::Result<()> {
	let open_gui = MenuItem::with_id(app, MENU_OPEN_GUI, "GUI を開く", true, None::<&str>)?;
	let quit = MenuItem::with_id(app, MENU_QUIT, "終了", true, None::<&str>)?;
	let menu = Menu::with_items(app, &[&open_gui, &quit])?;
	let icon = Image::from_bytes(include_bytes!("../../../assets/brand/vac/derived/vac-tray-default-16.png"))?;

	let review = Arc::new(Mutex::new(CloseReview::default()));
	#[cfg(feature = "desktop-native-review")]
	native_review::install(app.handle().clone(), shutdown.clone(), review.clone(), origin.clone())?;
	TrayIconBuilder::with_id(TRAY_ID)
		.tooltip(APP_TITLE)
		.icon(icon)
		.menu(&menu)
		.show_menu_on_left_click(false)
		.on_menu_event(move |app, event| match event.id().as_ref() {
			MENU_OPEN_GUI => show_gui(app),
			MENU_QUIT => request_reviewed_shutdown(app, shutdown.clone(), review.clone(), &origin),
			_ => {}
		})
		.on_tray_icon_event(|tray, event| {
			if let TrayIconEvent::DoubleClick {
				button: MouseButton::Left, ..
			} = event
			{
				show_gui(tray.app_handle());
			}
		})
		.build(app)?;
	Ok(())
}

#[derive(Default)]
struct CloseReview { sequence: u64, pending: Option<u64>, closing: bool }
impl CloseReview {
 fn request(&mut self) -> Option<u64> {
  if self.closing { return None; }
  self.sequence = self.sequence.checked_add(1)?;
  self.pending = Some(self.sequence);
  Some(self.sequence)
 }
 fn resolve(&mut self, id: u64, allowed: bool) -> bool {
  if self.pending != Some(id) { return false; }
  self.pending = None;
  self.closing = allowed;
  allowed
 }
}

fn release_close_review(app: &tauri::AppHandle, id: u64) {
 if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
  let _ = window.eval(format!("(() => {{ const r=document.documentElement; if(r.__vacCloseReview?.requestId === {id}) {{ r.inert=r.__vacCloseReview.wasInert; delete r.__vacCloseReview; }} }})()"));
 }
 show_gui(app);
}

fn request_reviewed_shutdown(app: &tauri::AppHandle, shutdown: Arc<ShutdownBroker>, review: Arc<Mutex<CloseReview>>, origin: &str) {
 let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else { return; };
 let Some(id) = review.lock().expect("close review").request() else { return; };
 let script = include_str!("close_review.js")
  .replace("__REQUEST_ID__", &id.to_string())
  .replace("__EXPECTED_ORIGIN__", &serde_json::to_string(origin).expect("origin JSON"));
 let callback_app = app.clone();
 let callback_review = review.clone();
 // Existing WebView2 dependency; no remote invoke capability or HTTP shutdown endpoint.
 let scheduled = window.with_webview(move |webview| {
  let completed_app = callback_app.clone();
  let completed_review = callback_review.clone();
  let handler = webview2_com::ExecuteScriptCompletedHandler::create(Box::new(move |result, output| {
   let value = serde_json::from_str::<serde_json::Value>(&output).ok();
   let allowed = result.is_ok() && value.as_ref().is_some_and(|v| v["requestId"].as_u64()==Some(id) && v["allowed"].as_bool()==Some(true));
   if completed_review.lock().expect("close review").resolve(id, allowed) {
    request_desktop_shutdown(&shutdown);
   } else { release_close_review(&completed_app, id); }
   Ok(())
  }));
  let script = webview2_com::CoTaskMemPWSTR::from(script.as_str());
  let result = unsafe { webview.controller().CoreWebView2().and_then(|view| view.ExecuteScript(*script.as_ref().as_pcwstr(), &handler)) };
  if let Err(error) = result {
   log::error!("Close review failed: {error}");
   callback_review.lock().expect("close review").resolve(id, false);
   release_close_review(&callback_app, id);
  }
 });
 if let Err(error) = scheduled {
  log::error!("Close review scheduling failed: {error}");
  review.lock().expect("close review").resolve(id, false);
  release_close_review(app, id);
 }
}

fn request_desktop_shutdown(shutdown: &ShutdownBroker) {
	log::info!("《Desktop》 tray menu から終了を要求しました。");
	shutdown.trigger(ShutdownReason::Desktop);
}

fn setup_main_window(app: &mut tauri::App, gui_url: String) -> anyhow::Result<WebviewWindow> {
	let gui_url = gui_url.parse()?;
	let builder = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::External(gui_url))
		.title(APP_TITLE)
		.icon(Image::from_bytes(include_bytes!(
			"../../../assets/brand/vac/derived/vac-tray-default-32.png"
		))?)?
		.inner_size(MAIN_WINDOW_WIDTH, MAIN_WINDOW_HEIGHT)
		.resizable(true)
		.visible(true);
	#[cfg(feature="desktop-native-review")]
	let builder=builder.data_directory(prepare_native_review()?.join("webview"));
	let window=builder.build()?;
	Ok(window)
}

fn attach_hide_on_close(window: WebviewWindow, app_handle: tauri::AppHandle) {
	window.on_window_event(move |event| {
		if let WindowEvent::CloseRequested { api, .. } = event {
			api.prevent_close();
			if let Some(window) = app_handle.get_webview_window(MAIN_WINDOW_LABEL) {
				let _ = window.hide();
			}
		}
	});
}

pub(super) fn show_gui(app: &tauri::AppHandle) {
	match app.get_webview_window(MAIN_WINDOW_LABEL) { Some(window) => {
		let _ = window.unminimize();
		if let Err(e) = window.show() {
			log::error!("《Desktop》 GUI window を表示できませんでした: {e}");
		}
		if let Err(e) = window.set_focus() {
			log::warn!("《Desktop》 GUI window に focus できませんでした: {e}");
		}
	} _ => {
		log::error!("《Desktop》 GUI window が見つかりません。");
	}}
}

#[cfg(test)]
mod close_review_tests {
 use super::CloseReview;
 #[test]
 fn stale_or_denied_review_cannot_authorize_shutdown() {
  let mut gate=CloseReview::default();
  let old=gate.request().unwrap(); let current=gate.request().unwrap();
  assert!(!gate.resolve(old,true)); assert!(!gate.resolve(current,false));
  assert!(!gate.resolve(current,true));
  let next=gate.request().unwrap(); assert!(gate.resolve(next,true));
  assert!(gate.request().is_none()); assert!(!gate.resolve(next,true));
 }
}

#[cfg(feature = "desktop-native-review")]
mod native_review {
 use super::*;
 #[cfg(not(debug_assertions))]
 compile_error!("desktop-native-review must not be included in a release artifact");
 pub(super) fn validate()->std::io::Result<std::path::PathBuf>{
  let root=std::env::var_os("UN_VAC_DESKTOP_REVIEW_ROOT").ok_or_else(||std::io::Error::other("Explicit review root required"))?;
  let root=std::path::PathBuf::from(root).canonicalize()?;
  if std::fs::read_to_string(root.join(".vac-native-review"))?.trim()!="isolated-desktop-close-v1" {return Err(std::io::Error::other("Invalid review marker") );}
  let configured=std::env::var_os("UN_VAC_CONFIG_DIR").ok_or_else(||std::io::Error::other("Isolated config directory required"))?;
  if std::path::PathBuf::from(configured).canonicalize()?!=root.join("store").canonicalize()? {return Err(std::io::Error::other("Review store mismatch") );}
  validate_restart_args(&std::env::args_os().skip(1).collect::<Vec<_>>(), &root)?;
  Ok(root)
 }
 fn validate_restart_args(args:&[std::ffi::OsString], root:&std::path::Path)->std::io::Result<()> {
  if args.is_empty(){return Ok(());}
  // Normal desktop restart now carries its primary-instance ownership marker.
  // Keep the review gate strict about every other argument and the config root.
  if args.iter().filter(|arg| *arg == "--desktop-primary").count() > 1 {
   return Err(std::io::Error::other("Duplicate primary marker"));
  }
  let filtered:Vec<_>=args.iter().filter(|arg| *arg != "--desktop-primary").cloned().collect();
  let args=filtered.as_slice();
  if args.len()!=3 || args[1]!="--restart-parent-pid" || args[2].to_str().and_then(|s|s.parse::<u32>().ok()).filter(|id|*id>0).is_none(){
   return Err(std::io::Error::other("Review build accepts only a managed restart"));
  }
  let conf=std::path::PathBuf::from(&args[0]).canonicalize()?;
  if crate::profile_store::lifecycle::managed_conf_at(&root.join("store"),&conf).is_none(){
   return Err(std::io::Error::other("Review restart configuration is outside the managed store"));
  }
  Ok(())
 }
 #[cfg(test)]
 mod restart_tests {
  use super::*;
  #[test]
  fn review_restart_stays_inside_its_store() {
   let root=std::env::temp_dir().join(format!("vac-review-restart-{}",rand::random::<u64>()));
   let profile=root.join("store/profiles/review");std::fs::create_dir_all(&profile).unwrap();
   let conf=profile.join("conf.toml");std::fs::write(&conf,"").unwrap();
   let args=vec![conf.as_os_str().to_owned(),"--restart-parent-pid".into(),"123".into()];
   assert!(validate_restart_args(&args,&root).is_ok());
   let mut primary=args.clone();primary.insert(1,"--desktop-primary".into());assert!(validate_restart_args(&primary,&root).is_ok());
   primary.insert(1,"--desktop-primary".into());assert!(validate_restart_args(&primary,&root).is_err());
   assert!(validate_restart_args(&[],&root).is_ok());
   let outside=root.join("outside.toml");std::fs::write(&outside,"").unwrap();
   let mut bad=args.clone();bad[0]=outside.into_os_string();assert!(validate_restart_args(&bad,&root).is_err());
   bad=args.clone();bad[2]="0".into();assert!(validate_restart_args(&bad,&root).is_err());
   bad=args.clone();bad[1]="--persona-id".into();assert!(validate_restart_args(&bad,&root).is_err());
   assert!(validate_restart_args(&args[..1],&root).is_err());
   std::fs::remove_dir_all(root).unwrap();
  }
 }
 // File requests are available only in this opt-in build. They enter the exact
 // native menu shutdown handler, including WebView2 result and ShutdownBroker.
 pub(super) fn install(app:tauri::AppHandle,shutdown:Arc<ShutdownBroker>,review:Arc<Mutex<CloseReview>>,origin:String)->tauri::Result<()> {
  let root=validate()?;
  std::thread::Builder::new().name("vac-native-close-review".into()).spawn(move||{
   let deadline=std::time::Instant::now()+std::time::Duration::from_secs(120);
   let mut next=1;
   while std::time::Instant::now()<deadline && next<=2 {
    if root.join(format!("close-request-{next}")).is_file(){
     let app2=app.clone();let shutdown=shutdown.clone();let review=review.clone();let origin=origin.clone();
     if app.run_on_main_thread(move||request_reviewed_shutdown(&app2,shutdown,review,&origin)).is_err(){break;}
     next+=1;
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
   }
  })?;
  Ok(())
 }
}

#[cfg(feature="desktop-native-review")]
pub(super) fn prepare_native_review()->std::io::Result<std::path::PathBuf>{native_review::validate()}

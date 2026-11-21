use super::runtime::ServerRuntime;
use crate::{bridges, web_interface, Result};
use actix_files::Files;
use actix_web::dev::Server;
use actix_web::web::Data;

const BROWSER_OUTPUT_ROUTE: &str = "/browser-output";
const RESOURCES_ROUTE: &str = "/resources";
const HTTP_SHUTDOWN_TIMEOUT_SECONDS: u64 = 2;

// Resolve once before worker creation. Missing optional assets must not cause
// one opaque actix-files startup error per worker; other routes remain usable.
fn available_resources_path(configured: Option<&str>) -> Option<std::path::PathBuf> {
	let path = std::path::PathBuf::from(configured?);
	match std::fs::metadata(&path) {
		Ok(metadata) if metadata.is_dir() => Some(path),
		Ok(_) => {
			log::warn!("VAC static resources unavailable: {} is not a directory; /resources requests return 404. Other UI and output routes remain available.", path.display());
			None
		}
		Err(error) => {
			log::warn!("VAC static resources unavailable: {} ({error}); /resources requests return 404. Create the asset directory and restart to enable it. Other UI and output routes remain available.", path.display());
			None
		}
	}
}

pub(super) fn build_http_server(runtime: ServerRuntime) -> Result<Server> {
	let resources_path = available_resources_path(runtime.conf.web_ui_resources_path.as_deref());
	let workers = runtime.workers;
	let web_ui_address = runtime.web_ui_address.clone();
	let conf_source_path = runtime.conf.source_path.clone();
	std::fs::create_dir_all(&runtime.output_root)?;
	web_interface::gui::log_distribution_status(runtime.conf.gui_dist_path.as_deref());
	let server = actix_web::HttpServer::new(move || {
		let runtime = runtime.clone();
		let reg = runtime.web_input_registry.clone();
		let output_root = runtime.output_root.clone();
		let app = actix_web::App::new()
			.wrap(actix_web::middleware::Condition::new(
				runtime.conf.web_ui_compress,
				actix_web::middleware::Compress::default(),
			))
			.app_data(Data::new(runtime.state))
			.app_data(Data::new(web_interface::output::OutputPaths { root: output_root.clone() }))
			.app_data(Data::new(reg.clone()))
			.app_data(Data::new(runtime.control_api_runtime))
			.app_data(Data::new(runtime.flowgraph_trigger.clone()))
			.configure({
				let r = reg.clone();
				move |cfg| {
					web_interface::web_input::register_web_input_routes(cfg, &r);
				}
			})
			.configure({
				let eps = runtime.flowgraph_web_input_endpoints.clone();
				move |cfg| {
					bridges::web_input::register_routes(cfg, eps.as_ref());
				}
			})
			.configure(web_interface::control::register)
			.configure({
				let gui_dist_path = runtime.conf.gui_dist_path.clone();
				move |cfg| {
					web_interface::gui::register(cfg, gui_dist_path.as_deref());
				}
			})
			.service(web_interface::websocket)
			.service(web_interface::input::get_index)
			.service(web_interface::input::get_subfile)
			.service(web_interface::output::post)
			.service(web_interface::output::get_index)
			.service(web_interface::output::get_subfile)
			.service(web_interface::obs_template::get)
			.service(Files::new(BROWSER_OUTPUT_ROUTE, output_root.clone()))
			.service(web_interface::status::get)
			.service(web_interface::favicon);
		if let Some(web_ui_resources_path) = resources_path.clone() {
			app.service(Files::new(RESOURCES_ROUTE, web_ui_resources_path))
		} else {
			app
		}
	})
	.workers(workers)
	.bind(web_ui_address)?;
	if let (Some(path), Some(address)) = (conf_source_path.as_deref(), server.addrs().first().copied()) {
		if let Err(error) = web_interface::control::local_instances::register(path, address) {
			log::warn!("VAC local instance registration failed: {error}");
		}
	}
	let server = server.disable_signals().shutdown_timeout(HTTP_SHUTDOWN_TIMEOUT_SECONDS).run();
	Ok(server)
}


#[cfg(test)]
mod tests {
	use super::*;

	#[actix_web::test]
	async fn optional_resources_preserve_assets_and_other_routes() {
		use actix_web::{test, App, HttpResponse};
		let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
		let root = std::env::temp_dir().join(format!("unvac-resource-route-{}-{nonce}", std::process::id()));
		std::fs::create_dir(&root).unwrap();
		let missing = root.join("not-created");
		assert!(available_resources_path(None).is_none());
		assert!(available_resources_path(Some(missing.to_str().unwrap())).is_none());
		let file = root.join("asset.txt");
		std::fs::write(&file, "user asset").unwrap();
		assert!(available_resources_path(Some(file.to_str().unwrap())).is_none());
		let directory = available_resources_path(Some(root.to_str().unwrap())).unwrap();
		let app = test::init_service(App::new().service(Files::new(RESOURCES_ROUTE, directory))).await;
		let response = test::call_service(&app, test::TestRequest::get().uri("/resources/asset.txt").to_request()).await;
		assert!(response.status().is_success());
		assert_eq!(test::read_body(response).await, "user asset");
		let app = test::init_service(App::new().route("/ready", actix_web::web::get().to(|| async {HttpResponse::Ok().finish()}))).await;
		let response = test::call_service(&app, test::TestRequest::get().uri("/resources/asset.txt").to_request()).await;
		assert_eq!(response.status(), actix_web::http::StatusCode::NOT_FOUND);
		let response = test::call_service(&app, test::TestRequest::get().uri("/ready").to_request()).await;
		assert!(response.status().is_success());
		assert!(!missing.exists(), "Do not create or modify configured paths");
		std::fs::remove_file(file).unwrap();
		std::fs::remove_dir(root).unwrap();
	}
}

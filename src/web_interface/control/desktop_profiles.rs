use actix_web::http::header;
use actix_web::web::{self, Bytes, Json, Path, Query};
use actix_web::{get, post, put, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

use crate::conf::Conf;
use crate::profile_store;
use crate::SharedState;

const DEFAULT_CONF_TEMPLATE: &str = include_str!("../../../conf.toml");

#[derive(Debug, Serialize)]
struct DesktopProfileEntry {
	id: String,
	label: String,
	conf_path: String,
	is_active: bool,
	template_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct DesktopProfileTemplateEntry {
	id: &'static str,
	label: &'static str,
	description: &'static str,
	recommended: bool,
}

#[derive(Debug, Serialize)]
struct DesktopProfilesResponse {
	root: String,
	managed_mode: bool,
	onboarding_completed: bool,
	active_profile_id: String,
	current_profile_id: Option<String>,
	current_conf_path: Option<String>,
	profiles: Vec<DesktopProfileEntry>,
}

#[derive(Debug, Deserialize)]
struct CreateDesktopProfileRequest {
	id: String,
	label: String,
	#[serde(default)]
	conf: Option<String>,
	#[serde(default)]
	template_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImportDesktopProfileBundleQuery {
	id: String,
	label: String,
}

#[derive(Debug, Serialize)]
struct DesktopProfileMutationResponse {
	profile: DesktopProfileEntry,
}

#[derive(Debug, Deserialize)]
struct ProfileLabelRequest {
	label: String,
}

#[put("/desktop/profiles/{id}/label")]
async fn put_profile_label(id: Path<String>, body: Json<ProfileLabelRequest>) -> impl Responder {
	match profile_store::rename_profile_label(&id.into_inner(), &body.label) {
		Ok(profile) => HttpResponse::Ok().json(DesktopProfileMutationResponse {
			profile: profile_entry(profile),
		}),
		Err(error) => profile_error(error),
	}
}

#[derive(Debug, Deserialize)]
struct ConfigureDesktopAiRequest {
	#[serde(default)]
	expected_revision: Option<String>,
	persona_id: String,
	display_name: String,
	model: String,
	#[serde(default)]
	api_key: Option<String>,
	custom_instructions: String,
	observe_triggers: Vec<String>,
	#[serde(default)]
	avatar_actions: bool,
	#[serde(default)]
	avatar_link_id: Option<String>,
	#[serde(default)]
	avatar_base_key: Option<String>,
	#[serde(default)]
	avatar_publish: bool,
}

#[derive(Debug, Serialize)]
struct DesktopAiMutationResponse {
	status: profile_store::AiSetupStatus,
	restart_required: bool,
}

#[get("/desktop/profiles")]
async fn get_desktop_profiles(state: web::Data<SharedState>) -> impl Responder {
	match profile_store::snapshot() {
		Ok(snapshot) => {
			let current = state.read().await.conf_source_path.clone();
			let current_profile_id = current
				.as_deref()
				.and_then(|path| current_profile(path, &snapshot.profiles))
				.map(|profile| profile.id.clone());
			let managed_mode = current_profile_id.is_some();
			HttpResponse::Ok().json(DesktopProfilesResponse {
				root: snapshot.root.display().to_string(),
				managed_mode,
				onboarding_completed: snapshot.onboarding_completed,
				active_profile_id: snapshot.active_profile_id,
				current_profile_id,
				current_conf_path: current.map(|path| path.display().to_string()),
				profiles: snapshot.profiles.into_iter().map(profile_entry).collect(),
			})
		}
		Err(error) => profile_error(error),
	}
}

#[get("/desktop/profile-templates")]
async fn get_desktop_profile_templates() -> impl Responder {
	HttpResponse::Ok().json(
		crate::profile_templates::TEMPLATES
			.iter()
			.map(|template| DesktopProfileTemplateEntry {
				id: template.id,
				label: template.label,
				description: template.description,
				recommended: template.recommended,
			})
			.collect::<Vec<_>>(),
	)
}

#[post("/desktop/profiles")]
async fn post_desktop_profile(body: Json<CreateDesktopProfileRequest>) -> impl Responder {
	if body.conf.is_some() && body.template_id.is_some() {
		return HttpResponse::BadRequest().json(serde_json::json!({
			"error": "ambiguous_profile_source",
			"detail": "confとtemplate_idは同時に指定できません",
		}));
	}
	let default_conf = match profile_store::desktop_default_conf(DEFAULT_CONF_TEMPLATE) {
		Ok(conf) => conf,
		Err(error) => return profile_error(error),
	};
	let template_id = body.template_id.as_deref().unwrap_or("blank");
	let materialized;
	let (conf, files, stored_template_id) = if let Some(conf) = body.conf.as_deref() {
		(conf, &[][..], None)
	} else {
		materialized = match crate::profile_templates::materialize(template_id, &default_conf) {
			Ok(template) => template,
			Err(error) => return profile_error(error),
		};
		(materialized.conf.as_str(), materialized.files.as_slice(), Some(template_id))
	};
	if let Err(error) = toml::from_str::<Conf>(conf) {
		return HttpResponse::BadRequest().json(serde_json::json!({
			"error": "invalid_conf",
			"detail": format!("confとして解釈できません: {error}"),
		}));
	}
	match profile_store::create_profile_with_files(&body.id, &body.label, conf, stored_template_id, files) {
		Ok(profile) => HttpResponse::Created().json(DesktopProfileMutationResponse {
			profile: profile_entry(profile),
		}),
		Err(error) => profile_error(error),
	}
}

#[post("/desktop/profiles/import-bundle")]
async fn post_import_desktop_profile_bundle(query: Query<ImportDesktopProfileBundleQuery>, body: Bytes) -> impl Responder {
	match profile_store::import_profile_bundle(&query.id, &query.label, &body) {
		Ok(profile) => HttpResponse::Created().json(DesktopProfileMutationResponse {
			profile: profile_entry(profile),
		}),
		Err(error) => profile_error(error),
	}
}

#[get("/desktop/profiles/{id}/export-bundle")]
async fn get_export_desktop_profile_bundle(id: Path<String>) -> impl Responder {
	let id = id.into_inner();
	match profile_store::export_profile_bundle(&id) {
		Ok(bundle) => HttpResponse::Ok()
			.insert_header((header::CONTENT_TYPE, "application/zip"))
			.insert_header((
				header::CONTENT_DISPOSITION,
				format!("attachment; filename=\"{id}.unvac-profile.zip\""),
			))
			.body(bundle),
		Err(error) => profile_error(error),
	}
}

#[get("/desktop/active-profile/ai")]
async fn get_desktop_ai_setup(state: web::Data<SharedState>) -> impl Responder {
	let path = match current_managed_conf(&state).await {
		Ok(path) => path,
		Err(response) => return response,
	};
	match profile_store::read_profile_ai(&path) {
		Ok(status) => HttpResponse::Ok().json(status),
		Err(error) => profile_error(error),
	}
}

#[put("/desktop/active-profile/ai")]
async fn put_desktop_ai_setup(state: web::Data<SharedState>, body: Json<ConfigureDesktopAiRequest>) -> impl Responder {
	let path = match current_managed_conf(&state).await {
		Ok(path) => path,
		Err(response) => return response,
	};
	let input = profile_store::AiSetupInput {
		expected_revision: body.expected_revision.clone(),
		persona_id: body.persona_id.clone(),
		display_name: body.display_name.clone(),
		model: body.model.clone(),
		api_key: body.api_key.clone(),
		custom_instructions: body.custom_instructions.clone(),
		observe_triggers: body.observe_triggers.clone(),
		avatar_actions: body.avatar_actions,
		avatar_link_id: body.avatar_link_id.clone(),
		avatar_base_key: body.avatar_base_key.clone(),
		avatar_publish: body.avatar_publish,
	};
	match profile_store::configure_profile_ai(&path, &input) {
		Ok(status) => HttpResponse::Ok().json(DesktopAiMutationResponse {
			status,
			restart_required: true,
		}),
		Err(error) => profile_error(error),
	}
}

#[post("/desktop/profiles/{id}/activate")]
async fn post_activate_desktop_profile(id: Path<String>) -> impl Responder {
	match profile_store::activate_profile(&id.into_inner()) {
		Ok(profile) => HttpResponse::Ok().json(DesktopProfileMutationResponse {
			profile: profile_entry(profile),
		}),
		Err(error) => profile_error(error),
	}
}

fn profile_entry(profile: profile_store::StoredProfile) -> DesktopProfileEntry {
	DesktopProfileEntry {
		id: profile.id,
		label: profile.label,
		conf_path: profile.conf_path.display().to_string(),
		is_active: profile.is_active,
		template_id: profile.template_id,
	}
}

fn profile_error(error: anyhow::Error) -> HttpResponse {
	if error.downcast_ref::<std::io::Error>().is_some_and(|cause| cause.kind() == std::io::ErrorKind::WouldBlock) {
		return HttpResponse::Conflict().json(serde_json::json!({"error":"profile_busy", "detail":error.to_string()}));
	}
	if error.is::<profile_store::lifecycle::StaleRevision>() {
		return HttpResponse::Conflict().json(serde_json::json!({"error":"profile_changed", "detail":error.to_string()}));
	}
	HttpResponse::BadRequest().json(serde_json::json!({
		"error": "profile_store_error",
		"detail": error.to_string(),
	}))
}

fn current_profile<'a>(
	current: &std::path::Path,
	profiles: &'a [profile_store::StoredProfile],
) -> Option<&'a profile_store::StoredProfile> {
	let current = std::fs::canonicalize(current).ok()?;
	profiles
		.iter()
		.find(|profile| std::fs::canonicalize(&profile.conf_path).ok().as_ref() == Some(&current))
}

async fn current_managed_conf(state: &web::Data<SharedState>) -> Result<std::path::PathBuf, HttpResponse> {
	let snapshot = profile_store::snapshot().map_err(profile_error)?;
	let current = state.read().await.conf_source_path.clone();
	if let Some(profile) = current.as_deref().and_then(|path| current_profile(path, &snapshot.profiles)) {
		Ok(profile.conf_path.clone())
	} else {
		Err(HttpResponse::Conflict().json(serde_json::json!({
			"error": "managed_profile_required",
			"detail": "この操作はdesktop managed profileでのみ使用できます",
		})))
	}
}

pub fn configure(cfg: &mut web::ServiceConfig) {
	cfg.service(get_desktop_profiles)
		.service(get_profile_perception)
		.service(put_profile_perception)
		.service(get_profile_content)
		.service(put_profile_content)
		.service(post_archive_profile)
		.service(get_archived_profiles)
		.service(post_restore_profile)
		.service(get_desktop_profile_templates)
		.service(post_desktop_profile)
		.service(put_profile_label)
		.service(post_import_desktop_profile_bundle)
		.service(get_export_desktop_profile_bundle)
		.service(get_desktop_ai_setup)
		.service(put_desktop_ai_setup)
		.service(post_activate_desktop_profile);
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PerceptionEditRequest {
	revision: String,
	edit: profile_store::perception_setup::PerceptionEdit,
}

#[get("/desktop/profiles/{id}/perception")]
async fn get_profile_perception(id: Path<String>) -> impl Responder {
	match profile_store::perception_setup::read_managed_perception(&id) {
		Ok(view) => HttpResponse::Ok().json(view),
		Err(error) => profile_error(error),
	}
}

#[put("/desktop/profiles/{id}/perception")]
async fn put_profile_perception(id: Path<String>, body: Json<PerceptionEditRequest>) -> impl Responder {
	match profile_store::perception_setup::save_managed_perception(&id,&body.revision,&body.edit) {
		Ok(result) => HttpResponse::Ok().json(result),
		Err(error) => profile_error(error),
	}
}

#[derive(Deserialize)]
struct ContentEdit {
	content: String,
	revision: String,
}

#[get("/desktop/profiles/{id}/content")]
async fn get_profile_content(id: Path<String>) -> impl Responder {
	match profile_store::lifecycle::read_content(&id) {
		Ok(content) => HttpResponse::Ok().json(content),
		Err(error) => profile_error(error),
	}
}
#[put("/desktop/profiles/{id}/content")]
async fn put_profile_content(id: Path<String>, body: Json<ContentEdit>) -> impl Responder {
	match profile_store::lifecycle::save_content(&id, &body.content, &body.revision) {
		Ok(content) => HttpResponse::Ok().json(content),
		Err(error) => profile_error(error),
	}
}
#[get("/desktop/deleted-profiles")]
async fn get_archived_profiles() -> impl Responder {
	match profile_store::lifecycle::archived() {
		Ok(profiles) => HttpResponse::Ok().json(profiles),
		Err(error) => profile_error(error),
	}
}
#[post("/desktop/deleted-profiles/{id}/restore")]
async fn post_restore_profile(id: Path<String>) -> impl Responder {
	match profile_store::lifecycle::restore(&id) {
		Ok(()) => HttpResponse::Ok().json(serde_json::json!({"ok":true})),
		Err(error) => profile_error(error),
	}
}
#[post("/desktop/profiles/{id}/archive")]
async fn post_archive_profile(id: Path<String>, state: web::Data<SharedState>) -> impl Responder {
	let snapshot = match profile_store::snapshot() {
		Ok(value) => value,
		Err(error) => return profile_error(error),
	};
	let Some(profile) = snapshot.profiles.iter().find(|p| p.id == *id) else {
		return HttpResponse::NotFound().finish();
	};
	let path = match std::fs::canonicalize(&profile.conf_path) {
		Ok(path) => path,
		Err(error) => return profile_error(error.into()),
	};
	let current = state.read().await.conf_source_path.clone();
	let instances = match super::local_instances::list() {
		Ok(value) => value,
		Err(error) => return profile_error(error),
	};
	if current.and_then(|p| std::fs::canonicalize(p).ok()).as_ref() == Some(&path) || instances.iter().any(|entry| entry.conf_path == path)
	{
		return HttpResponse::Conflict()
			.json(serde_json::json!({"error":"profile_in_use", "detail":"起動中のVACが使用するプロファイルは削除できません"}));
	}
	match profile_store::lifecycle::archive(&id) {
		Ok(()) => HttpResponse::Ok().json(serde_json::json!({"ok":true})),
		Err(error) => profile_error(error),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::{SystemTime, UNIX_EPOCH};

	#[actix_web::test]
	#[ignore = "explicit bounded isolated browser review; no application/device initialization"]
	async fn perception_browser_review() {
		let root=std::path::PathBuf::from(std::env::var_os("UNVAC_PERCEPTION_BROWSER_ROOT").expect("review root required"));
		assert_eq!(profile_store::user_config_root().unwrap(),root);
		assert!(root.is_absolute() && root.is_dir());
		assert!(!root.join("profiles").exists(),"review must use fresh isolated storage");
		let source="workers=2\n[[ai.personas]]\nid='host'\nchannel_utterance='ai:host'\nobserve={include_all=true}\n[[ai.personas]]\nid='guide'\nchannel_utterance='ai:guide'\nobserve={include_all=true,exclude=['obs:game_frame']}\n[[perception.obs_sources]]\nid='game'\nscene_name='Game'\nsource_name='Capture'\npassword_env='REVIEW_ONLY_UNSET'\n";
		for id in ["stream","desk"] {
			let dir=root.join("profiles").join(id);std::fs::create_dir_all(&dir).unwrap();
			std::fs::write(dir.join("conf.toml"),source).unwrap();
		}
		let listener=std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).unwrap();
		let port=listener.local_addr().unwrap().port();
		let identity=serde_json::json!({"kind":"unvac-isolated-perception-review","root":root,"port":port,"devices":false});
		let response_identity=identity.clone();
		let server=actix_web::HttpServer::new(move || {
			let response=response_identity.clone();
			actix_web::App::new()
				.route("/review-identity",web::get().to(move || {let value=response.clone();async move {HttpResponse::Ok().json(value)}}))
				.service(web::scope("/api/v1/control").service(get_profile_perception).service(put_profile_perception))
		}).disable_signals().workers(1).listen(listener).unwrap().run();
		let handle=server.handle();
		let task=actix_web::rt::spawn(server);
		std::fs::write(root.join("ready.json"),serde_json::to_vec(&identity).unwrap()).unwrap();
		// Bound independent of the browser/controller, including abandoned runs.
		tokio::time::sleep(std::time::Duration::from_secs(150)).await;
		handle.stop(true).await;
		task.await.unwrap().unwrap();
	}

	#[test]
	fn perception_http_uses_isolated_store() {
		let root=std::env::temp_dir().join(format!("unvac-perception-http-{}",rand::random::<u64>()));
		std::fs::create_dir(&root).unwrap();
		let output=std::process::Command::new(std::env::current_exe().unwrap())
			.args(["web_interface::control::desktop_profiles::tests::perception_http_child","--exact","--ignored","--test-threads=1"])
			.env("UN_VAC_CONFIG_DIR",&root).env("UNVAC_PERCEPTION_HTTP_TEST_ROOT",&root).output().unwrap();
		assert!(output.status.success(),"{}\n{}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr));
		assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"),"child test must run");
		for id in ["one","two"] {
			let dir=root.join("profiles").join(id);
			std::fs::remove_file(dir.join("conf.toml")).unwrap();
			std::fs::remove_dir(dir).unwrap();
		}
		std::fs::remove_file(root.join(".profiles-operation.lock")).unwrap();
		std::fs::remove_dir(root.join("profiles")).unwrap();
		std::fs::remove_dir(root).unwrap();
	}

	#[actix_web::test]
	#[ignore = "isolated child; no sockets, devices or production configuration"]
	async fn perception_http_child() {
		use actix_web::test::{init_service,call_service,read_body_json,TestRequest};
		let root=std::path::PathBuf::from(std::env::var_os("UNVAC_PERCEPTION_HTTP_TEST_ROOT").expect("parent required"));
		assert_eq!(profile_store::user_config_root().unwrap(),root);
		let source="workers=2\n[[ai.personas]]\nid='partner'\nchannel_utterance='ai:partner'\ncustom_instructions='PRIVATE_PROMPT'\n[[perception.obs_sources]]\nid='game'\nscene_name='Scene'\nsource_name='Capture'\npassword='PRIVATE_PASSWORD'\n";
		for id in ["one","two"] {
			let dir=root.join("profiles").join(id);std::fs::create_dir_all(&dir).unwrap();
			std::fs::write(dir.join("conf.toml"),source).unwrap();
		}
		let app=init_service(actix_web::App::new().service(get_profile_perception).service(put_profile_perception)).await;
		let endpoint="/desktop/profiles/one/perception";
		let response=call_service(&app,TestRequest::get().uri(endpoint).to_request()).await;
		assert_eq!(response.status(),200);
		let view:serde_json::Value=read_body_json(response).await;
		assert!(!view.to_string().contains("PRIVATE_"));
		assert_eq!(view["sources"][0]["id"],"game");
		let revision=view["revision"].as_str().unwrap();
		let body=serde_json::json!({"revision":revision,"edit":{"kind":"assignment","source_id":"game","persona_index":0,"shared_source_ids":["game"],"observe":true}});
		let response=call_service(&app,TestRequest::put().uri(endpoint).set_json(&body).to_request()).await;
		assert_eq!(response.status(),200);
		let saved:serde_json::Value=read_body_json(response).await;
		assert_eq!(saved["restart_required"],true);
		assert_ne!(saved["revision"],view["revision"]);
		let response=call_service(&app,TestRequest::get().uri(endpoint).to_request()).await;
		let reopened:serde_json::Value=read_body_json(response).await;
		assert_eq!(reopened["revision"],saved["revision"]);
		assert_eq!(reopened["sources"][0]["observers"][0]["rule"],"additional");
		let bytes=std::fs::read(root.join("profiles/one/conf.toml")).unwrap();
		let stale=call_service(&app,TestRequest::put().uri(endpoint).set_json(&body).to_request()).await;
		assert_eq!(stale.status(),409);
		let failure:serde_json::Value=read_body_json(stale).await;
		assert_eq!(failure["error"],"profile_changed");
		let invalid=serde_json::json!({"revision":saved["revision"],"edit":{"kind":"source","source_id":"game","patch":{"password":"must-not-overwrite"}}});
		assert_eq!(call_service(&app,TestRequest::put().uri(endpoint).set_json(&invalid).to_request()).await.status(),400);
		assert_eq!(std::fs::read(root.join("profiles/one/conf.toml")).unwrap(),bytes);
		assert_eq!(std::fs::read_to_string(root.join("profiles/two/conf.toml")).unwrap(),source);
	}

	#[test]
	fn running_profile_is_not_the_next_launch_default_or_an_arbitrary_store_file() {
		let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
		let root = std::env::temp_dir().join(format!("unvac-managed-path-{}-{nonce}", std::process::id()));
		let profiles = root.join("profiles");
		let conf = profiles.join("default").join("conf.toml");
		std::fs::create_dir_all(conf.parent().unwrap()).unwrap();
		std::fs::write(&conf, "workers = 1\n").unwrap();
		let entries = vec![profile_store::StoredProfile {
			id: "default".into(),
			label: "Current".into(),
			conf_path: conf.clone(),
			is_active: false,
			template_id: None,
		}];
		assert_eq!(current_profile(&conf, &entries).unwrap().id, "default");
		assert!(current_profile(&root.join("outside.toml"), &entries).is_none());
		let other_file = profiles.join("default").join("other.toml");
		std::fs::write(&other_file, "workers = 1\n").unwrap();
		assert!(current_profile(&other_file, &entries).is_none());
		std::fs::remove_dir_all(root).unwrap();
	}
}

#[cfg(test)]
mod ai_save_conflict_tests {
    #[test]
    fn graph_write_contention_returns_conflict() {
        let cause = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::WouldBlock))
            .context("Shared graph directory is busy");
        assert_eq!(super::profile_error(cause).status(), actix_web::http::StatusCode::CONFLICT);
    }
}

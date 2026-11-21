//! Purpose storage operations. Graph changes are journaled and never activate/reload runtime.
use actix_web::{post, web, HttpResponse};
use super::{Document, Snapshot};
use crate::SharedState;
use super::super::{parse_graph_body, util};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveRequest {
    expected: Snapshot,
    document: Document,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RetryRequest {
    expected: Snapshot,
    id: String,
    revision: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RemovalPlanRequest {
    expected: Snapshot,
    id: String,
    files: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoveRequest {
    expected: Snapshot,
    id: String,
    files: Vec<String>,
    reviewed: serde_json::Value,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreRequest {
    expected_root: String,
    recovery: super::RemovalRecovery,
}

#[post("/flowgraph/purposes/remove-files")]
async fn remove_files(state: web::Data<SharedState>, body: web::Bytes) -> HttpResponse {
    let request: RemoveRequest = match parse_graph_body(&body) { Ok(value) => value, Err(error) => return error };
    let (root, _) = match util::flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
    match super::remove_purpose_files(&root, &request.expected, &request.id, request.files, &request.reviewed) {
        // A partial storage failure must preserve the recovery id and removed-file list.
        Ok(outcome) => HttpResponse::Ok().json(outcome),
        Err(error) => util::err_json(actix_web::http::StatusCode::CONFLICT, "purpose_remove_failed", error),
    }
}

#[post("/flowgraph/purposes/restore-removal")]
async fn restore_removal(state: web::Data<SharedState>, body: web::Bytes) -> HttpResponse {
    let request: RestoreRequest = match parse_graph_body(&body) { Ok(value) => value, Err(error) => return error };
    let (root, _) = match util::flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
    if super::super::graph_root_identity(&root) != request.expected_root {
        return util::err_json(actix_web::http::StatusCode::CONFLICT, "purpose_restore_target_changed", "purpose root changed");
    }
    match super::restore_removal(&root, &request.recovery) {
        Ok(snapshot) => HttpResponse::Ok().json(snapshot),
        Err(error) => util::err_json(actix_web::http::StatusCode::CONFLICT, "purpose_restore_failed", error),
    }
}

#[post("/flowgraph/purposes/removal-plan")]
async fn removal_plan(state: web::Data<SharedState>, body: web::Bytes) -> HttpResponse {
    let request: RemovalPlanRequest = match parse_graph_body(&body) { Ok(value) => value, Err(error) => return error };
    let (root, _) = match util::flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
    match super::removal_plan(&root, &request.expected, &request.id, request.files) {
        Ok(plan) => HttpResponse::Ok().json(plan),
        Err(error) => util::err_json(actix_web::http::StatusCode::CONFLICT, "purpose_removal_plan_failed", error),
    }
}

#[post("/flowgraph/purposes/removal-history")]
async fn removal_history(state: web::Data<SharedState>) -> HttpResponse {
    let (root, _) = match util::flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
    match super::removal_recovery_history(&root) {
        Ok(entries) => HttpResponse::Ok().json(serde_json::json!({"root": super::super::graph_root_identity(&root), "entries": entries})),
        Err(error) => util::err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "purpose_removal_history_failed", error),
    }
}

#[post("/flowgraph/purposes/history")]
async fn history(state: web::Data<SharedState>) -> HttpResponse {
    let (root, _) = match util::flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
    history_at(&root)
}

fn history_at(root: &std::path::Path) -> HttpResponse {
    match super::history(root) {
        Ok(entries) => HttpResponse::Ok().json(serde_json::json!({"root": super::super::graph_root_identity(root), "entries": entries})),
        Err(error) => util::err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "purpose_history_failed", error),
    }
}

#[post("/flowgraph/purposes/recovery")]
async fn recovery(state: web::Data<SharedState>) -> HttpResponse {
    let (root, _) = match util::flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
    match super::registration_recovery(&root) {
        Ok(entries) => HttpResponse::Ok().json(serde_json::json!({"root": super::super::graph_root_identity(&root), "entries": entries})),
        Err(error) => util::err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "purpose_recovery_failed", error),
    }
}

#[post("/flowgraph/purposes/retry")]
async fn retry(state: web::Data<SharedState>, body: web::Bytes) -> HttpResponse {
    let request: RetryRequest = match parse_graph_body(&body) { Ok(value) => value, Err(error) => return error };
    let (root, _) = match util::flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
    retry_at(&root, request)
}

fn retry_at(root: &std::path::Path, request: RetryRequest) -> HttpResponse {
    match super::retry_registration(root, &request.expected, &request.id, &request.revision) {
        Ok(snapshot) => HttpResponse::Ok().json(snapshot),
        Err(error) => util::err_json(actix_web::http::StatusCode::CONFLICT, "purpose_retry_failed", error),
    }
}

#[post("/flowgraph/purposes/read")]
async fn read(state: web::Data<SharedState>) -> HttpResponse {
    let (root, _) = match util::flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
    match super::read(&root) {
        Ok(snapshot) => HttpResponse::Ok().json(snapshot),
        Err(error) => util::err_json(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, "purpose_read_failed", error),
    }
}

#[post("/flowgraph/purposes/save")]
async fn save(state: web::Data<SharedState>, body: web::Bytes) -> HttpResponse {
    let request: SaveRequest = match parse_graph_body(&body) { Ok(value) => value, Err(error) => return error };
    let (root, _) = match util::flowgraph_dir(&state).await { Ok(value) => value, Err(error) => return error };
    save_at(&root, request)
}

fn save_at(root: &std::path::Path, request: SaveRequest) -> HttpResponse {
    match super::save(root, &request.expected, request.document) {
        Ok(snapshot) => HttpResponse::Ok().json(snapshot),
        Err(error) => util::err_json(actix_web::http::StatusCode::CONFLICT, "purpose_save_failed", error),
    }
}

pub fn configure(config: &mut web::ServiceConfig) {
    config.configure(super::transfer::configure);
    config.service(read).service(save).service(history).service(recovery).service(retry)
        .service(removal_plan).service(removal_history).service(remove_files).service(restore_removal);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[actix_web::test]
    async fn normal_removal_restores_exact_bytes_and_rejects_changed_review_or_destination() {
        use actix_web::{test as http, App};
        let directory = std::env::temp_dir().join(format!("vac-purpose-lifecycle-{}-{}", std::process::id(), rand::random::<u64>()));
        std::fs::create_dir(&directory).unwrap();
        let root = directory.canonicalize().unwrap();
        let graph = root.join("main.flowgraph.toml");
        let original = b"[meta]\ntitle='Original bytes'\n";
        std::fs::write(&graph, original).unwrap();
        let mut conf: crate::Conf = toml::from_str("").unwrap();
        conf.runtime_dir = Some(root.join("runtime"));
        let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
        let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let runtime = state.read().await.flowgraph.clone();
        *runtime.write().await = Some(crate::flowgraph::FlowgraphRuntime::load(&root));
        let app = http::init_service(App::new().app_data(web::Data::new(state.clone()))
            .service(web::scope("/api/v1/control").configure(super::super::super::configure))).await;
        let before = super::super::read(&root).unwrap();
        let saved = super::super::save(&root,&before,Document {schema_version:1,purposes:vec![
            super::super::Purpose{id:"one".into(),name:"One".into(),roots:vec!["main".into()]}
        ]}).unwrap();
        let review: serde_json::Value = http::call_and_read_body_json(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/removal-plan")
            .set_json(serde_json::json!({"expected":saved,"id":"one","files":["main"]})).to_request()).await;
        let remove_body = serde_json::json!({"expected":saved,"id":"one","files":["main"],"reviewed":review});
        std::fs::write(&graph,b"# changed after review\n").unwrap();
        let stale = http::call_service(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/remove-files").set_json(&remove_body).to_request()).await;
        assert_eq!(stale.status(),409);
        assert_eq!(std::fs::read(&graph).unwrap(),b"# changed after review\n");
        assert!(super::super::removal_recovery_history(&root).unwrap().is_empty());
        std::fs::write(&graph,original).unwrap();
        let removed: serde_json::Value = http::call_and_read_body_json(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/remove-files").set_json(&remove_body).to_request()).await;
        assert_eq!(removed["removed_files"],serde_json::json!(["main"]));
        assert!(removed["error"].is_null());
        assert!(!graph.exists());
        assert!(super::super::read(&root).unwrap().document.purposes.is_empty());
        let restore_body=serde_json::json!({"expected_root":saved.root,"recovery":removed["recovery"]});
        let wrong_target=http::call_service(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/restore-removal")
            .set_json(serde_json::json!({"expected_root":"other-profile","recovery":removed["recovery"]})).to_request()).await;
        assert_eq!(wrong_target.status(),409);
        assert!(!graph.exists());
        std::fs::write(&graph,b"# user replacement\n").unwrap();
        let conflict=http::call_service(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/restore-removal").set_json(&restore_body).to_request()).await;
        assert_eq!(conflict.status(),409);
        assert_eq!(std::fs::read(&graph).unwrap(),b"# user replacement\n");
        std::fs::remove_file(&graph).unwrap();
        for _ in 0..2 {
            let restored: Snapshot=http::call_and_read_body_json(&app,http::TestRequest::post()
                .uri("/api/v1/control/flowgraph/purposes/restore-removal").set_json(&restore_body).to_request()).await;
            assert_eq!(restored.document,saved.document);
            assert_eq!(std::fs::read(&graph).unwrap(),original);
        }
        let bridges=state.read().await.bridge_handles.clone();
        std::mem::replace(&mut *bridges.lock().await,crate::bridges::BridgeHandles::empty()).finish_all().await;
        drop(app);drop(runtime);drop(state);
        assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("vac-purpose-lifecycle-"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[actix_web::test]
    async fn normal_purpose_routes_use_current_runtime_root_and_reject_old_target() {
        use actix_web::{test as http, App};
        let directory = std::env::temp_dir().join(format!("vac-purpose-http-{}-{}", std::process::id(), rand::random::<u64>()));
        std::fs::create_dir(&directory).unwrap();
        let root = directory.canonicalize().unwrap();
        let first = root.join("first"); let second = root.join("second");
        std::fs::create_dir(&first).unwrap(); std::fs::create_dir(&second).unwrap();
        let graph_text = "[[nodes]]\nid='value'\nfeature='flowgraph.literal.string'\n[nodes.properties]\nvalue='unchanged'\n";
        std::fs::write(first.join("main.flowgraph.toml"), graph_text).unwrap();
        let mut conf: crate::Conf = toml::from_str("").unwrap();
        conf.runtime_dir = Some(root.join("runtime"));
        let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
        let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let runtime = state.read().await.flowgraph.clone();
        *runtime.write().await = Some(crate::flowgraph::FlowgraphRuntime::load(&first));
        let app = http::init_service(App::new().app_data(web::Data::new(state.clone()))
            .service(web::scope("/api/v1/control").configure(super::super::super::configure))).await;
        let before: Snapshot = http::call_and_read_body_json(&app, http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/read").to_request()).await;
        assert_eq!(before.root, super::super::super::graph_root_identity(&first));
        let document = serde_json::json!({"schema_version":1,"purposes":[
            {"id":"one","name":"One","roots":["main"]},
            {"id":"two","name":"Two","roots":["main"]}
        ]});
        let saved: Snapshot = http::call_and_read_body_json(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/save").set_json(serde_json::json!({"expected":before,"document":document})).to_request()).await;
        let bytes = std::fs::read(first.join(".flowgraph-purposes.json")).unwrap();
        let removal_request = serde_json::json!({"expected":saved,"id":"one","files":["main"]});
        let plan: serde_json::Value = http::call_and_read_body_json(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/removal-plan").set_json(&removal_request).to_request()).await;
        assert_eq!(plan["root"], saved.root);
        assert_eq!(plan["other_purpose_uses"][0]["id"], "two");
        assert_eq!(plan["other_purpose_uses"][0]["files"], serde_json::json!(["main"]));
        assert_eq!(plan["delete_ready"], false);
        assert_eq!(std::fs::read_to_string(first.join("main.flowgraph.toml")).unwrap(),graph_text);
        assert_eq!(std::fs::read(first.join(".flowgraph-purposes.json")).unwrap(),bytes);
        *runtime.write().await = Some(crate::flowgraph::FlowgraphRuntime::load(&second));
        let rejected = http::call_service(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/save").set_json(serde_json::json!({"expected":saved,"document":document})).to_request()).await;
        assert_eq!(rejected.status(),409);
        assert_eq!(std::fs::read(first.join(".flowgraph-purposes.json")).unwrap(),bytes);
        assert!(!second.join(".flowgraph-purposes.json").exists());
        let old_plan = http::call_service(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/removal-plan").set_json(removal_request).to_request()).await;
        assert_eq!(old_plan.status(),409);
        for action in ["history","recovery","removal-history"] {
            let response: serde_json::Value = http::call_and_read_body_json(&app,http::TestRequest::post()
                .uri(&format!("/api/v1/control/flowgraph/purposes/{action}")).to_request()).await;
            assert_eq!(response["root"],super::super::super::graph_root_identity(&second));
            assert!(response["entries"].as_array().unwrap().is_empty());
        }
        let malformed = http::call_service(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/retry").set_json(serde_json::json!({"id":"one"})).to_request()).await;
        assert_eq!(malformed.status(),400);
        let malformed_plan = http::call_service(&app,http::TestRequest::post()
            .uri("/api/v1/control/flowgraph/purposes/removal-plan")
            .set_json(serde_json::json!({"id":"one","files":["main"]})).to_request()).await;
        assert_eq!(malformed_plan.status(),400);
        let bridges = state.read().await.bridge_handles.clone();
        std::mem::replace(&mut *bridges.lock().await,crate::bridges::BridgeHandles::empty()).finish_all().await;
        drop(app); drop(runtime); drop(state);
        assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("vac-purpose-http-"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[actix_web::test]
    async fn metadata_save_preserves_graphs_and_rejects_stale_or_foreign_snapshot() {
        let directory = std::env::temp_dir().join(format!("vac-purpose-api-{}-{}", std::process::id(), rand::random::<u64>()));
        std::fs::create_dir(&directory).unwrap();
        let root = directory.canonicalize().unwrap();
        let graph = root.join("main.flowgraph.toml");
        std::fs::write(&graph, "# unchanged graph\n").unwrap();
        let before = super::super::read(&root).unwrap();
        let document = Document { schema_version: 1, purposes: vec![super::super::Purpose { id: "custom".into(), name: "Custom function".into(), roots: vec!["main".into()] }] };
        assert_eq!(save_at(&root, SaveRequest { expected: before.clone(), document: document.clone() }).status(), 200);
        let saved = super::super::read(&root).unwrap();
        assert_eq!(saved.document, document);
        assert_eq!(save_at(&root, SaveRequest { expected: before, document: document.clone() }).status(), 409);
        let mut foreign = saved.clone(); foreign.root = "another-profile".into();
        assert_eq!(save_at(&root, SaveRequest { expected: foreign, document }).status(), 409);
        assert_eq!(super::super::read(&root).unwrap(), saved);
        assert_eq!(std::fs::read_to_string(&graph).unwrap(), "# unchanged graph\n");
        assert!(parse_graph_body::<SaveRequest>(br#"{"document":{"schema_version":1,"purposes":[]}}"#).is_err());
        let receipt = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "root": saved.root,
            "purpose": {"id":"repair", "name":"Recovered function", "roots":["main"]},
            "graphs": {"main": blake3::hash(&std::fs::read(&graph).unwrap()).to_hex().to_string()}
        })).unwrap();
        std::fs::write(root.join(".flowgraph-purpose-import-repair.json"), &receipt).unwrap();
        let revision = blake3::hash(&receipt).to_hex().to_string();
        assert_eq!(retry_at(&root, RetryRequest {expected:saved.clone(),id:"repair".into(),revision:"stale".into()}).status(),409);
        assert_eq!(super::super::read(&root).unwrap(), saved);
        assert_eq!(retry_at(&root, RetryRequest {expected:saved.clone(),id:"repair".into(),revision:revision.clone()}).status(),200);
        let recovered = super::super::read(&root).unwrap();
        assert_eq!(recovered.document.purposes.len(),2);
        assert_eq!(retry_at(&root, RetryRequest {expected:recovered.clone(),id:"repair".into(),revision}).status(),200);
        let response = history_at(&root);
        assert_eq!(response.status(),200);
        let body = actix_web::body::to_bytes(response.into_body()).await.unwrap();
        let history_body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(history_body["root"], saved.root);
        assert!(history_body["entries"].as_array().unwrap().iter().any(|entry| entry["document"] == serde_json::to_value(&saved.document).unwrap()));
        assert_eq!(std::fs::read_to_string(&graph).unwrap(), "# unchanged graph\n");
        assert!(root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("vac-purpose-api-"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}

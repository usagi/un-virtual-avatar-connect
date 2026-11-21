//! Explicitly invoked, bounded browser/API review against retained disposable files.
//! No application bootstrap, user configuration, OAuth, capture or external bridge.
use super::*;

#[actix_web::test]
#[ignore = "manual loopback browser review; isolated storage and disconnected audio"]
async fn isolated_persistent_browser_api() {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let run = std::env::var("UNVAC_BROWSER_REVIEW_RUN").expect("explicit isolated run name required");
    assert!(!run.is_empty() && run.len() <= 64 && run.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'));
    let lifetime: u64 = std::env::var("UNVAC_BROWSER_REVIEW_SECONDS")
        .map(|value| value.parse().expect("review lifetime must be seconds")).unwrap_or(120);
    // Interactive comparison needs time for the user to read and switch between
    // alternatives. Keep an upper bound and an explicit, store-local stop path.
    assert!((1..=3600).contains(&lifetime), "review lifetime must be at most one hour");
    let parent = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/browser-api-review");
    std::fs::create_dir_all(&parent).unwrap();
    let parent = parent.canonicalize().unwrap();
    let root = parent.join(&run);
    let marker = root.join("isolated-review.marker");
    if root.exists() {
        assert_eq!(root.canonicalize().unwrap(), root, "review root must not be redirected");
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "UNVAC isolated browser API review v1");
    } else {
        std::fs::create_dir(&root).unwrap();
        std::fs::write(&marker, "UNVAC isolated browser API review v1").unwrap();
        std::fs::create_dir(root.join("graphs")).unwrap();
        std::fs::write(root.join("graphs/main.flowgraph.toml"),
            "[[nodes]]\nid='value'\nfeature='flowgraph.literal.string'\n[nodes.properties]\nvalue='isolated original'\n").unwrap();
    }
    let graphs = root.join("graphs");
    assert_eq!(graphs.canonicalize().unwrap(), graphs);
    let stop_request = root.join("stop.request");
    assert!(!stop_request.exists(), "previous review stop request remains; choose a fresh review store");
    let mut conf: crate::Conf = toml::from_str("").unwrap();
    conf.runtime_dir = Some(root.join("runtime"));
    // Storage and non-audio graph review must not acquire the user's output
    // device. This fixture cannot establish playback or audio timing correctness.
    let audio = Arc::new(Mutex::new(crate::AudioSink::disconnected_for_test()));
    let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
    let shared = state.read().await.flowgraph.clone();
    *shared.write().await = Some(FlowgraphRuntime::load(&graphs));
    // Optional state lifecycle review. Keep the existing storage-only fixture
    // unchanged unless explicitly selected by the review operator.
    if std::env::var("UNVAC_BROWSER_REVIEW_STATE_LIFECYCLE").as_deref() == Ok("1") {
        let profile = root.join("review-conf.toml");
        std::fs::write(&profile, "# Isolated state lifecycle review; no external connections\n").unwrap();
        state.write().await.conf_source_path = Some(profile.clone());
        let runtime_root = state.read().await.runtime_paths.root.clone();
        shared.write().await.as_mut().unwrap().set_profile_local_state_snapshot_path(&runtime_root, &profile);
    }
    let server_state = state.clone();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    // Production graph handlers, including explicitly opted-in graph triggers;
    // no open-external or authentication routes.
    let server = actix_web::HttpServer::new(move || actix_web::App::new()
        .app_data(Data::new(server_state.clone()))
        .app_data(web::PayloadConfig::new(GRAPH_BODY_LIMIT_BYTES))
        .service(web::scope("/api/v1/control")
            .configure(purposes::api::configure)
            .service(get_tree).service(get_diagnostics).service(get_file).service(put_file)
            .service(post_reload).service(post_reload_preserving_state)
            .service(post_save_live_state_snapshot).service(post_restore_profile_local_state_snapshot)
            .service(post_file).service(delete_file).service(get_removal_plan)
            .service(get_node_catalog).service(get_signature)
            .service(trigger::post_trigger_node)
            .service(fragment_zip::post_export_zip).service(fragment_zip::post_import_zip)
            .service(fragment_zip::get_companions).service(fragment_zip::post_share_plan)
            .service(file_recovery::recovery_history).service(file_recovery::preview_restore)
            .service(file_recovery::restore_file)))
        .workers(1).listen(listener).unwrap().run();
    let handle = server.handle();
    let task = actix_web::rt::spawn(server);
    let ready = root.join("ready.json");
    std::fs::write(&ready, serde_json::to_vec_pretty(&serde_json::json!({
        "kind":"unvac-isolated-native-browser-review", "pid":std::process::id(),
        "port":port, "root":root, "graph_root":graph_root_identity(&graphs),
        "lifetime_seconds":lifetime, "stop_request":stop_request, "runtime_applied_at_start":false,
        "audio_output":"disconnected", "audio_playback_supported":false
    })).unwrap()).unwrap();
    println!("Isolated API ready: {}", ready.display());
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(lifetime));
    tokio::pin!(deadline);
    let mut stop_poll = tokio::time::interval(std::time::Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = &mut deadline => break,
            _ = stop_poll.tick() => {
                if stop_request.is_file() { break; }
            }
        }
    }
    handle.stop(true).await;
    task.await.unwrap().unwrap();
    let bridges = state.read().await.bridge_handles.clone();
    std::mem::replace(&mut *bridges.lock().await, crate::bridges::BridgeHandles::empty()).finish_all().await;
    if let Some(worker) = shared.read().await.as_ref().and_then(|rt| rt.handle.clone()) {
        worker.shutdown().await;
    }
    std::fs::remove_file(ready).unwrap();
    // Retain only this named review's files for a second process to reopen.
}

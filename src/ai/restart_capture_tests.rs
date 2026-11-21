//! Opt-in subprocess check: persisted configuration -> fresh OBS transport -> first AI request.
//! This is a new process running production State/AiService, not desktop bootstrap.
use super::*;
use base64::Engine as _;
use futures::{SinkExt, StreamExt};
use serde_json::json;

const CHILD: &str = "ai::service::request_capture_tests::restart_capture::reopened_capture_child";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Opens the default audio output in isolated child processes; coordinate execution first"]
async fn reopened_process_captures_first_reply() {
    let root = std::env::temp_dir().join(format!("vac-reopened-capture-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let obs = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    provider.set_nonblocking(true).unwrap();
    let provider_url = format!("http://{}", provider.local_addr().unwrap());
    let mut conf: crate::Conf = toml::from_str("").unwrap();
    conf.runtime_dir = Some(root.join("runtime"));
    conf.perception = toml::from_str(&format!(r#"
[[obs_sources]]
id = "selected"
enabled = true
refresh_before_reply = true
url = "ws://{}"
use_local_obs_config_password = false
scene_name = "selected scene"
source_name = "selected game"
channel = "obs:selected"
image_format = "png"
width = 100
height = 100
"#, obs.local_addr().unwrap())).unwrap();
    let mut excluded = conf.perception.obs_sources[0].clone();
    excluded.id = "excluded".into();
    excluded.channel = "obs:excluded".into();
    excluded.scene_name = "unselected scene".into();
    excluded.source_name = "unselected game".into();
    conf.perception.obs_sources.push(excluded);
    let mut persona = super::super::chi6_resolver_tests::persona_minimal();
    persona.api_key = Some("local-capture-only".into());
    persona.openai_stream = Some(true);
    persona.observe.triggers = vec!["user".into()];
    persona.observe.include_all = false;
    persona.observe.include_additional = vec!["obs:selected".into()];
    persona.turn_management = Some(super::super::super::config::AiTurnManagementConf {
        enabled: true, visual_history_enabled: true, past_screen_image_enabled: true,
        ..Default::default()
    });
    conf.ai.personas = vec![persona];
    let config = root.join("conf.toml");
    crate::profile_store::write_config_transaction(&config, toml::to_string(&conf).unwrap().as_bytes()).unwrap();
    let initial = std::fs::read(&config).unwrap();

    // Different, decodable nonblank images prove each process requests a fresh frame.
    let frames: Vec<String> = [false, true].into_iter().map(|reverse| {
        let pixels = image::RgbImage::from_fn(100, 100, |x, _| {
            if (x < 50) ^ reverse { image::Rgb([255, 255, 255]) } else { image::Rgb([0, 0, 0]) }
        });
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(pixels).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes.into_inner()))
    }).collect();
    let served_frames = frames.clone();
    let obs_task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for frame in served_frames {
            for screenshot in [false, true] {
                let (stream, _) = tokio::time::timeout(Duration::from_secs(45), obs.accept()).await.unwrap().unwrap();
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                ws.send(tokio_tungstenite::tungstenite::Message::Text(json!({"op":0,"d":{"rpcVersion":1}}).to_string().into())).await.unwrap();
                let identify: serde_json::Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                assert_eq!(identify["op"], 1);
                ws.send(tokio_tungstenite::tungstenite::Message::Text(json!({"op":2,"d":{"negotiatedRpcVersion":1}}).to_string().into())).await.unwrap();
                let request: serde_json::Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
                let (kind, data, response) = if screenshot {
                    ("GetSourceScreenshot", json!({"sourceName":"selected game","imageFormat":"png","imageWidth":100,"imageHeight":100}), json!({"imageData":frame}))
                } else {
                    ("GetSceneItemId", json!({"sceneName":"selected scene","sourceName":"selected game"}), json!({"sceneItemId":7}))
                };
                assert_eq!(request["op"], 6);
                assert_eq!(request["d"]["requestType"], kind);
                assert_eq!(request["d"]["requestData"], data);
                ws.send(tokio_tungstenite::tungstenite::Message::Text(json!({"op":7,"d":{"requestType":kind,"requestId":request["d"]["requestId"],"requestStatus":{"result":true,"code":100},"responseData":response}}).to_string().into())).await.unwrap();
                requests.push(request);
            }
        }
        requests
    });
    let mut pids = Vec::new();
    let mut requests = Vec::new();
    for _ in 0..2 {
        let listener = provider.try_clone().unwrap();
        let capture = tokio::task::spawn_blocking(move || {
            let (connection, request) = receive(&listener);
            respond(connection, 0, "UNMATCHED_NOTE");
            request
        });
        let mut child = tokio::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--ignored", "--exact", CHILD, "--nocapture"])
            .env("VAC_REOPEN_CAPTURE_CONFIG", &config)
            .env("VAC_REOPEN_CAPTURE_PROVIDER", &provider_url)
            .kill_on_drop(true);
        let mut child = child.spawn().unwrap();
        pids.push(child.id().unwrap());
        let status = tokio::time::timeout(Duration::from_secs(40), child.wait()).await.expect("child deadline exceeded").unwrap();
        assert!(status.success(), "isolated child failed: {status}");
        requests.push(capture.await.unwrap());
    }
    let obs_requests = obs_task.await.unwrap();
    for (request, frame) in requests.iter().zip(&frames) {
        let images: Vec<_> = request["input"].as_array().unwrap().iter()
            .filter_map(|item| item["content"].as_array()).flatten()
            .filter(|part| part["type"] == "input_image").collect();
        assert_eq!(images.len(), 1, "fresh process must not inherit historical image");
        assert_eq!(images[0]["image_url"], *frame);
    }
    assert_eq!(std::fs::read(&config).unwrap(), initial, "read-only startup changed saved configuration");
    let evidence = json!({"pids":pids,"obs_requests":obs_requests,"provider_requests":requests,
        "scope":"Two fresh test processes, production config reload/State/AiService and real loopback OBS/HTTP transports. No desktop bootstrap, physical OBS, external AI, or real-game interpretation."});
    std::fs::write(root.join("evidence.json"), serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    println!("Reopened capture evidence: {}", root.join("evidence.json").display());
}

#[tokio::test]
#[ignore = "Worker for reopened_process_captures_first_reply; not a standalone test"]
async fn reopened_capture_child() {
    let config = std::env::var_os("VAC_REOPEN_CAPTURE_CONFIG").expect("parent must supply isolated configuration");
    let endpoint = std::env::var("VAC_REOPEN_CAPTURE_PROVIDER").unwrap();
    let endpoint_url = reqwest::Url::parse(&endpoint).unwrap();
    assert_eq!(endpoint_url.scheme(), "http");
    assert_eq!(endpoint_url.host_str(), Some("127.0.0.1"));
    let conf = crate::Conf::new_noop_probe(config).unwrap();
    conf.perception.validate().unwrap();
    for source in &conf.perception.obs_sources {
        let url = reqwest::Url::parse(&source.url).unwrap();
        assert_eq!(url.scheme(), "ws");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert!(!source.use_local_obs_config_password);
    }
    let state = crate::State::new(&conf, Arc::new(Mutex::new(crate::AudioSink::open_default().unwrap())), crate::shutdown::ShutdownBroker::new()).await.unwrap();
    let mut service = AiService::new(conf.ai.personas[0].clone(), state.clone(), None, None, None,
        Arc::new(AtomicBool::new(false)), Arc::new(RwLock::new(AiUsageTelemetry::default()))).await.unwrap();
    service.client = ResponsesClient::new(ResponsesClientConfig {api_key:"local-capture-only".into(),base_url:endpoint,..Default::default()}).unwrap();
    let trigger = ChannelDatum::new("user".into(), "さっきの画面と今は？".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
    let observation = Observation { datum_id:trigger.get_id(), channel:trigger.channel.clone() };
    state.read().await.push_channel_datum_quiet(trigger).await;
    tokio::time::timeout(Duration::from_secs(15), service.react(observation)).await.unwrap().unwrap();
}

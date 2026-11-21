//! Offline request capture through the production AiService request builder.
use super::*;
use std::io::{Read, Write};

#[path = "restart_capture_tests.rs"]
mod restart_capture;

#[tokio::test]
async fn concurrent_transport_completion_keeps_current_visual_scope() {
    capture_concurrent_scope(false, false).await;
}

#[tokio::test]
async fn concurrent_transport_mode_gap_rejects_old_visual_notes() {
    capture_concurrent_scope(true, false).await;
}

#[tokio::test]
async fn concurrent_transport_mode_gap_without_inactive_request_rejects_old_notes() {
    capture_concurrent_scope(true, true).await;
}

    fn receive(listener: &std::net::TcpListener) -> (std::net::TcpStream, serde_json::Value) {
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(10);
        let mut connection=loop {
            if let Ok((connection,_))=listener.accept() {break connection;}
            assert!(std::time::Instant::now()<deadline,"missing local request");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        connection.set_nonblocking(false).unwrap();
        connection.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut header=Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte=[0]; connection.read_exact(&mut byte).unwrap(); header.push(byte[0]);
            assert!(header.len()<65536);
        }
        let header=String::from_utf8(header).unwrap().to_lowercase();
        assert!(header.contains("authorization: bearer local-capture-only"));
        let length:usize=header.lines().find_map(|l|l.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
        assert!(length<1_000_000);
        let mut body=vec![0;length];connection.read_exact(&mut body).unwrap();
        (connection,serde_json::from_slice(&body).unwrap())
    }
    fn respond(mut connection:std::net::TcpStream,id:u64,evidence:&str) {
        let decision=serde_json::json!({"action":"observe","speak":null,"episode_memory":null,"avatar_intent":null,"avatar_pose":null,"visual_notes":[{"datum_id":id,"kind":"state","readouts":[],"evidence":evidence}]}).to_string();
        let event=serde_json::json!({"type":"response.completed","response":{"id":format!("local-{id}"),"status":"completed","output":[],"output_text":decision}});
        let body=format!("event: response.completed\ndata: {event}\n\n");
        write!(connection,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    }

async fn capture_concurrent_scope(mode_gap: bool, no_inactive_request: bool) {
    let frame=|scene:&str| ChannelDatum::new("obs:game_frame".into(),"synthetic local frame".into())
        .with_flag(ChannelDatum::FLAG_IS_FINAL).with_source(crate::DataSource::new("obs.scene_source"))
        .with_meta(ChannelDatum::META_STATE_KEY,"obs_source:fixture")
        .with_meta("scene_name",scene).with_meta("source_name","fixture")
        .with_meta("observed_at",jiff::Timestamp::now().to_string())
        .with_attachment(crate::Attachment::Url {url:"data:image/png;base64,iVBORw0KGgo=".into(),mime:Some("image/png".into()),title:None});
    // Provider never decodes this synthetic payload; this tests request routing,
    // transport completion and memory ownership, not image quality or inference.
    let old=frame(if mode_gap { "new-scene" } else { "old-scene" });let new=frame("new-scene");
    let old_id=old.get_id();let new_id=new.get_id();
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();let address=listener.local_addr().unwrap();
    let (received,wait)=tokio::sync::oneshot::channel();
    let (release,hold)=std::sync::mpsc::channel();
    let server=std::thread::spawn(move || {
        let (old_connection,first)=receive(&listener);received.send(()).unwrap();
        let (connection,second)=receive(&listener);respond(connection,new_id,"CURRENT_SCOPE_NOTE");
        hold.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        respond(old_connection,old_id,"OBSOLETE_SCOPE_NOTE");
        let (connection,third)=receive(&listener);respond(connection,new_id,"CURRENT_SCOPE_NOTE");
        [first,second,third]
    });
    let mut conf:crate::Conf=toml::from_str("").unwrap();
    conf.perception=toml::from_str("[[obs_sources]]\nid='fixture'\nenabled=true\nrefresh_before_reply=false\nscene_name='new-scene'\nsource_name='fixture'\n").unwrap();
    if mode_gap { conf.perception.obs_sources[0].active_modes = vec!["streaming".into()]; }
    conf.runtime_dir=Some(std::env::temp_dir().join(format!("vac-concurrent-scope-{}",rand::random::<u64>())));
    let state=crate::State::new(&conf,Arc::new(Mutex::new(crate::AudioSink::open_default().unwrap())),crate::shutdown::ShutdownBroker::new()).await.unwrap();
    let mut persona=super::chi6_resolver_tests::persona_minimal();
    persona.api_key=Some("local-capture-only".into());persona.openai_stream=Some(true);persona.openai_max_in_flight=Some(2);
    persona.observe.triggers=vec!["user".into()];persona.observe.include_all=true;
    persona.turn_management=Some(super::super::config::AiTurnManagementConf {enabled:true,visual_history_enabled:true,..Default::default()});
    let mut service=AiService::new(persona,state.clone(),None,None,None,Arc::new(AtomicBool::new(false)),Arc::new(RwLock::new(AiUsageTelemetry::default()))).await.unwrap();
    service.client=ResponsesClient::new(ResponsesClientConfig {api_key:"local-capture-only".into(),base_url:format!("http://{address}"),..Default::default()}).unwrap();
    async fn trigger(state:&crate::SharedState)->Observation {
        let datum=ChannelDatum::new("user".into(),"今の画面を見て".into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
        let observation=Observation{datum_id:datum.get_id(),channel:datum.channel.clone()};
        state.read().await.push_channel_datum_quiet(datum).await;observation
    }
    if mode_gap { crate::state::apply_runtime_mode_change(&state, &conf, Some("streaming".into()), None).await.unwrap(); }
    state.read().await.push_channel_datum_quiet(old).await;
    let first=trigger(&state).await;let pending_service=service.clone();
    let pending=tokio::spawn(async move {pending_service.react(first).await});
    tokio::time::timeout(Duration::from_secs(10),wait).await.unwrap().unwrap();
    if mode_gap { crate::state::apply_runtime_mode_change(&state, &conf, Some("maintenance".into()), None).await.unwrap(); }
    if no_inactive_request { crate::state::apply_runtime_mode_change(&state, &conf, Some("streaming".into()), None).await.unwrap(); }
    state.read().await.push_channel_datum_quiet(new).await;
    tokio::time::timeout(Duration::from_secs(10),service.react(trigger(&state).await)).await.unwrap().unwrap();
    release.send(()).unwrap();pending.await.unwrap().unwrap();
    if mode_gap { crate::state::apply_runtime_mode_change(&state, &conf, Some("streaming".into()), None).await.unwrap(); }
    tokio::time::timeout(Duration::from_secs(10),service.react(trigger(&state).await)).await.unwrap().unwrap();
    let requests=server.join().unwrap();
    let third=requests[2]["input"].to_string();
    if mode_gap && !no_inactive_request {
        assert!(requests[0]["input"].to_string().contains("data:image/png"));
        assert!(!requests[1]["input"].to_string().contains("data:image/png"), "inactive mode must exclude image from actual transport");
        assert!(third.contains("data:image/png"), "same-source image must return after reactivation");
        assert!(!third.contains("CURRENT_SCOPE_NOTE"), "note without an input image must not enter visual history");
    } else {
    assert!(third.contains("CURRENT_SCOPE_NOTE"),"current response must enter actual next request");
    }
    assert!(!third.contains("OBSOLETE_SCOPE_NOTE"),"late previous-source response contaminated next request");
}

#[tokio::test]
#[ignore = "manual saved-image continuous service test"]
async fn continuous_past_screen_requests() {
    let conf_path = std::path::PathBuf::from(std::env::var("UNVAC_CAPTURE_CONF").unwrap());
    let old_path = std::path::PathBuf::from(std::env::var("UNVAC_CAPTURE_PAST_IMAGE").unwrap());
    let new_path = std::path::PathBuf::from(std::env::var("UNVAC_CAPTURE_IMAGE").unwrap());
    let output = std::path::PathBuf::from(std::env::var("UNVAC_CAPTURE_OUTPUT").unwrap());
    assert!(!output.exists());
    let original: crate::Conf = toml::from_str(&std::fs::read_to_string(&conf_path).unwrap()).unwrap();
    let mut persona = original.ai.personas[0].clone();
    persona.api_key = Some("local-capture-only".into()); persona.api_key_file = None;
    persona.turn_management.as_mut().unwrap().past_screen_image_enabled = true;
    // Skip wall-clock scheduling only; exercise the same request/cache path.
    persona.min_interval_in_secs = None;
    if let Some(decision) = persona.decision.as_mut() { decision.min_interval_in_secs = None; }
    if let Some(path) = persona.openai_tools_json_path.as_mut() {
        if path.is_relative() { *path = conf_path.parent().unwrap().join(&*path); }
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for round in 0..11 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut connection = loop {
                if let Ok((connection, _)) = listener.accept() { break connection; }
                assert!(std::time::Instant::now() < deadline, "missing request at round {round}");
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            connection.set_nonblocking(false).unwrap();
            connection.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte=[0]; connection.read_exact(&mut byte).unwrap(); header.push(byte[0]);
                assert!(header.len()<65536);
            }
            let header=String::from_utf8(header).unwrap().to_lowercase();
            assert!(header.contains("authorization: bearer local-capture-only"));
            let length: usize=header.lines().find_map(|l| l.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
            assert!(length<16*1024*1024);
            let mut body=vec![0;length]; connection.read_exact(&mut body).unwrap();
            requests.push(serde_json::from_slice::<serde_json::Value>(&body).unwrap());
            let decision=r#"{"action":"observe","speak":null,"episode_memory":null,"avatar_intent":null,"avatar_pose":null,"visual_notes":[]}"#;
            let event=serde_json::json!({"type":"response.completed","response":{"id":format!("offline-{round}"),"status":"completed","output":[],"output_text":decision}});
            let body=format!("event: response.completed\ndata: {event}\n\n");
            write!(connection,"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
        requests
    });
    let mut conf: crate::Conf=toml::from_str("").unwrap();
    // A configured periodic source supplies only saved frames in this harness.
    // No periodic task is spawned and reply-time acquisition stays disabled.
    conf.perception = toml::from_str(r#"
[[obs_sources]]
id = "recorded-game"
enabled = true
refresh_before_reply = false
scene_name = "same-scene"
source_name = "recorded-frame"
active_modes = ["streaming"]
"#).unwrap();
    conf.runtime_dir=Some(output.parent().unwrap().join("runtime-continuous"));
    let state=crate::State::new(&conf,Arc::new(Mutex::new(crate::AudioSink::open_default().unwrap())),crate::shutdown::ShutdownBroker::new()).await.unwrap();
    *state.read().await.runtime_mode_id.write().unwrap() = Some("streaming".into());
    let mut diagnostics=state.read().await.control_event_tx.subscribe();
    let mut service=AiService::new(persona,state.clone(),None,None,None,Arc::new(AtomicBool::new(false)),Arc::new(RwLock::new(AiUsageTelemetry::default()))).await.unwrap();
    service.client=ResponsesClient::new(ResponsesClientConfig {api_key:"local-capture-only".into(),base_url:format!("http://{address}"),..Default::default()}).unwrap();
    let mut frame_ids=Vec::new();
    for (round,(path,scene,source,prompt,age_seconds)) in [
        (&old_path,"same-scene","recorded-frame","今は何の画面？",0),
        (&new_path,"same-scene","recorded-frame","さっきのスキル名は？",0),
        (&new_path,"same-scene","recorded-frame","さっきのスキルじゃなくて、今のを見て",0),
        (&new_path,"same-scene","recorded-frame","今の画面ではなく、さっきのスキルを見たい",0),
        (&new_path,"same-scene","recorded-frame","今は何の画面？",0),
        (&old_path,"different-scene","recorded-frame","さっきのスキル名は？",0),
        (&new_path,"different-scene","recorded-frame","さっきのスキル名は？",0),
        (&old_path,"different-scene","different-source","さっきのスキル名は？",0),
        (&old_path,"expiry-scene","recorded-frame","今は何の画面？",61),
        (&new_path,"expiry-scene","recorded-frame","さっきのスキル名は？",0),
        (&new_path,"expiry-scene","recorded-frame","さっきのスキル名は？",0),
    ].into_iter().enumerate() {
        if round == 10 {
            // Keep the existing state-channel frame, as a real mode switch does.
            *state.read().await.runtime_mode_id.write().unwrap() = Some("maintenance".into());
        } else {
        let frame=ChannelDatum::new("obs:game_frame".into(),"保存画像の連続再生".into()).with_flag(ChannelDatum::FLAG_IS_FINAL)
            .with_source(crate::DataSource::new("obs.scene_source"))
            .with_meta(ChannelDatum::META_STATE_KEY,"obs_source:recorded-game")
            .with_meta("scene_name",scene).with_meta("source_name",source)
            .with_meta("observed_at",(jiff::Timestamp::now() - std::time::Duration::from_secs(age_seconds)).to_string())
            .with_attachment(crate::Attachment::File {href:String::new(),bytes:Some(std::fs::metadata(path).unwrap().len()),path:path.clone(),mime:Some("image/jpeg".into()),label:Some("recorded-game".into())});
        frame_ids.push(frame.get_id());
        state.read().await.push_channel_datum_quiet(frame).await;
        }
        let trigger=ChannelDatum::new("user".into(),prompt.into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
        let observation=Observation {datum_id:trigger.get_id(),channel:trigger.channel.clone()};
        state.read().await.push_channel_datum_quiet(trigger).await;
        tokio::time::timeout(std::time::Duration::from_secs(20),service.react(observation)).await.unwrap().unwrap();
    }
    let requests=server.join().unwrap();
    let counts: Vec<_>=requests.iter().map(|r| r["input"].as_array().unwrap().iter().filter_map(|m| m["content"].as_array()).flatten().filter(|p| p["type"]=="input_image").count()).collect();
    assert_eq!(counts,vec![1,2,1,2,1,1,2,1,1,1,0]);
    let mut contexts=Vec::new();
    while let Ok(event)=diagnostics.try_recv() {
        let value=serde_json::to_value(event).unwrap();
        if value["kind"]=="ai_request_context" { contexts.push(value); }
    }
    assert_eq!(contexts.len(),11);
    assert!(contexts[10]["observations"].as_array().unwrap().iter().all(|o| o["source_kind"] != "obs.scene_source"));
    for (request,context) in requests.iter().zip(&contexts) {
        for datum in context["input_datums"].as_array().unwrap() {
            if datum["source_kind"]=="obs.scene_source" {
                let index=datum["input_index"].as_u64().unwrap() as usize;
                let label=request["input"][index]["content"][0]["text"].as_str().unwrap();
                assert!(label.contains(&format!("datum_id={}",datum["datum_id"])));
            }
        }
    }
    assert!(contexts[1]["observations"].as_array().unwrap().iter().any(|o|o["datum_id"]==frame_ids[0]));
    assert!(contexts[3]["observations"].as_array().unwrap().iter().any(|o|o["datum_id"]==frame_ids[0]));
    assert!(contexts[6]["observations"].as_array().unwrap().iter().any(|o|o["datum_id"]==frame_ids[5]));
    let evidence=serde_json::json!({"requests":requests,"contexts":contexts,"frame_ids":frame_ids,"image_counts":counts,"scope":"same AiService over eleven successful mocked observe responses; cancellation, redirection, scene and source switch, expired previous observation, streaming-to-maintenance mode switch with retained state frame; no injected cache, external API, playback or renderer"});
    std::fs::File::create_new(output).unwrap().write_all(serde_json::to_string_pretty(&evidence).unwrap().as_bytes()).unwrap();
}

#[tokio::test]
#[ignore = "manual offline replay of saved tool responses"]
async fn capture_saved_tool_continuation() {
    let input = std::env::var("UNVAC_REPLAY_INPUT").unwrap();
    let responses = std::env::var("UNVAC_REPLAY_RESPONSES").unwrap();
    let output = std::path::PathBuf::from(std::env::var("UNVAC_REPLAY_OUTPUT").unwrap());
    assert!(!output.exists());
    let mut request_json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();
    // The wire serializer omits the optional message discriminator, while the
    // Rust enum deserializer requires it. Serialization restores the original.
    for item in request_json["input"].as_array_mut().unwrap() {
        if item.get("role").is_some() && item.get("type").is_none() {
            item["type"] = serde_json::json!("message");
        }
    }
    let request: CreateResponseRequest = serde_json::from_value(request_json).unwrap();
    let replay: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(responses).unwrap()).unwrap();
    assert!(!replay.is_empty() && replay.len() < 8);
    for response in &replay {
        assert_eq!(response["status"], "completed");
        let mut calls = 0;
        for item in response["output"].as_array().unwrap() {
            if item["type"] == "function_call" {
                assert!(matches!(item["name"].as_str(), Some("vac_ping" | "vac_emit_effect")));
                calls += 1;
            }
        }
        assert!(calls > 0, "only replay unfinished local tool rounds");
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut captured = Vec::new();
        for round in 0..=replay.len() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut connection = loop {
                if let Ok((connection, _)) = listener.accept() { break connection; }
                assert!(std::time::Instant::now() < deadline, "continuation was not emitted");
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            connection.set_nonblocking(false).unwrap();
            connection.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0]; connection.read_exact(&mut byte).unwrap(); header.push(byte[0]);
                assert!(header.len() < 65536);
            }
            let header = String::from_utf8(header).unwrap().to_lowercase();
            assert!(header.contains("authorization: bearer local-capture-only"));
            let length: usize = header.lines().find_map(|l| l.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
            assert!(length < 16 * 1024 * 1024);
            let mut body = vec![0; length]; connection.read_exact(&mut body).unwrap();
            captured.push(serde_json::from_slice::<serde_json::Value>(&body).unwrap());
            if let Some(response) = replay.get(round) {
                let event = serde_json::json!({"type":"response.completed","response":response});
                let body = format!("event: response.completed\ndata: {event}\n\n");
                write!(connection, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            } else {
                let body = r#"{"error":{"message":"offline capture only","type":"invalid_request_error"}}"#;
                write!(connection, "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        }
        captured
    });
    let mut conf: crate::Conf = toml::from_str("").unwrap();
    conf.runtime_dir = Some(output.parent().unwrap().join("runtime"));
    let audio = Arc::new(Mutex::new(crate::AudioSink::open_default().unwrap()));
    let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
    // The saved fixture has prior frame 1, current frame 2 and trigger 3.
    for _ in 0..3 { ChannelDatum::new("fixture".into(), String::new()); }
    let ctx = ToolContext { state: state.clone(), persona_label: "kaltsit".into(), effect_channel: "effect".into(), effect_allowed_channels: std::env::var("UNVAC_CAPTURE_EFFECT_CHANNELS").ok().map(|value| serde_json::from_str(&value).unwrap()),
        twitch_eventsub: None, twitch_moderator: None, twitch_default_broadcaster_login: None };
    let client = ResponsesClient::new(ResponsesClientConfig {
        api_key: "local-capture-only".into(), base_url: format!("http://{address}"), ..Default::default()
    }).unwrap();
    let allowed = ["vac_ping".to_string(), "vac_emit_effect".to_string()].into_iter().collect();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(20),
        drive_responses_tool_loop(&client, request, true, true, &allowed, Some(&ctx), &state, "ai", "kaltsit", true, None, None)).await;
    let captured = server.join().unwrap();
    assert!(outcome.is_ok() && outcome.unwrap().is_err(), "terminal capture rejection expected");
    let shared = state.read().await;
    let data = shared.channel_data.read().await;
    let emitted: Vec<_> = data.iter().map(|d| serde_json::json!({"id":d.get_id(),"channel":d.channel,"content":d.content,"source":d.source})).collect();
    let evidence = serde_json::json!({"requests":captured,"emitted":emitted,"scope":"isolated production tool loop; no processors, Twitch or renderer"});
    std::fs::File::create_new(output).unwrap().write_all(serde_json::to_string_pretty(&evidence).unwrap().as_bytes()).unwrap();
}

#[tokio::test]
#[ignore = "manual saved-image evaluation; requires explicit input/output paths"]
async fn capture_saved_image_production_request() {
    let conf_path = std::env::var("UNVAC_CAPTURE_CONF").expect("UNVAC_CAPTURE_CONF");
    let image_path = std::path::PathBuf::from(std::env::var("UNVAC_CAPTURE_IMAGE").expect("UNVAC_CAPTURE_IMAGE"));
    let output = std::path::PathBuf::from(std::env::var("UNVAC_CAPTURE_OUTPUT").expect("UNVAC_CAPTURE_OUTPUT"));
    let prompt = std::env::var("UNVAC_CAPTURE_PROMPT").expect("UNVAC_CAPTURE_PROMPT");
    assert!(!output.exists(), "never overwrite captured evidence");
    let conf_path = std::path::PathBuf::from(conf_path);
    let original: crate::Conf = toml::from_str(&std::fs::read_to_string(&conf_path).unwrap()).unwrap();
    let mut persona = original.ai.personas[0].clone();
    if std::env::var_os("UNVAC_CAPTURE_PAST_IMAGE").is_some() {
        assert!(std::env::var_os("UNVAC_CAPTURE_HISTORY").is_some());
        persona.turn_management.as_mut().unwrap().past_screen_image_enabled = true;
    }
    if let Ok(channels) = std::env::var("UNVAC_CAPTURE_EFFECT_CHANNELS") {
        persona.openai_tool_policy.get_or_insert_with(Default::default).effect_allowed_channels = Some(serde_json::from_str(&channels).unwrap());
    }
    if let Some(path) = persona.openai_tools_json_path.as_mut() {
        if path.is_relative() { *path = conf_path.parent().unwrap().join(&*path); }
    }
    persona.api_key = Some("local-capture-only".into());
    persona.api_key_file = None;
    assert!(persona.observe.triggers.iter().any(|c| c == "user"));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut connection = loop {
            if let Ok((connection, _)) = listener.accept() { break connection; }
            assert!(std::time::Instant::now() < deadline, "production service did not emit a request");
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        connection.set_nonblocking(false).unwrap();
        connection.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            let mut byte = [0]; connection.read_exact(&mut byte).unwrap(); header.push(byte[0]);
            assert!(header.len() < 65536);
        }
        let header = String::from_utf8(header).unwrap().to_lowercase();
        assert!(header.contains("authorization: bearer local-capture-only"));
        let length: usize = header.lines().find_map(|l| l.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
        assert!(length < 16 * 1024 * 1024);
        let mut body = vec![0; length]; connection.read_exact(&mut body).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Deliberately reject: no synthetic AI response enters history and no
        // external API or tool call is needed to capture the exact input.
        let response = r#"{"error":{"message":"offline capture only","type":"invalid_request_error"}}"#;
        write!(connection, "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
        request
    });

    let mut conf: crate::Conf = toml::from_str("").unwrap();
    conf.runtime_dir = Some(output.parent().unwrap().join("runtime"));
    let audio = Arc::new(Mutex::new(crate::AudioSink::open_default().unwrap()));
    let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
    let mut service = AiService::new(persona, state.clone(), None, None, None,
        Arc::new(AtomicBool::new(false)), Arc::new(RwLock::new(AiUsageTelemetry::default()))).await.unwrap();
    service.client = ResponsesClient::new(ResponsesClientConfig {
        api_key: "local-capture-only".into(), base_url: format!("http://{address}"), ..Default::default()
    }).unwrap();
    let observed_at = jiff::Timestamp::now().to_string();
    // Each capture uses a fresh State: reserve an earlier identity before the
    // current frame so imported history cannot alias the current observation.
    let history_datum_id = std::env::var_os("UNVAC_CAPTURE_HISTORY").map(|_| {
        ChannelDatum::new("obs:game_frame".into(), "prior frame identity".into()).get_id()
    });
    let frame = ChannelDatum::new("obs:game_frame".into(), "保存済みゲーム画像を使った隔離再生".into())
        .with_flag(ChannelDatum::FLAG_IS_FINAL)
        .with_source(crate::DataSource::new("obs.scene_source"))
        .with_meta(ChannelDatum::META_STATE_KEY, "obs_source:recorded-game")
        .with_meta("scene_name", "VOD-fixture")
        .with_meta("source_name", "recorded-frame")
        .with_meta("observed_at", observed_at)
        .with_attachment(crate::Attachment::File {
            href: String::new(), bytes: Some(std::fs::metadata(&image_path).unwrap().len()), path: image_path,
            mime: Some("image/jpeg".into()), label: Some("recorded-game".into()),
        });
    if let Ok(seed_path) = std::env::var("UNVAC_CAPTURE_HISTORY") {
        let seed: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(seed_path).unwrap()).unwrap();
        let mut notes: Vec<super::super::visual_history::VisualNote> = serde_json::from_value(seed["notes"].clone()).unwrap();
        let previous_id = history_datum_id.unwrap();
        assert_ne!(previous_id, frame.get_id());
        for note in &mut notes {
            assert_eq!(note.datum_id, seed["datum_id"].as_u64().unwrap());
            note.datum_id = previous_id;
        }
        let age_ms = seed["age_ms"].as_i64().unwrap();
        assert!((1..60_000).contains(&age_ms));
        let now = jiff::Timestamp::now();
        let previous_at = jiff::Timestamp::from_nanosecond(now.as_nanosecond() - i128::from(age_ms) * 1_000_000).unwrap();
        let source = crate::control_events::AiContextObservation {
            datum_id: previous_id, channel: frame.channel.clone(),
            source_kind: "obs.scene_source".into(), source_subtype: None,
            observed_at: Some(previous_at.to_string()), attachment_count: 1, input_image_count: 1,
        };
        let generation = state.read().await.runtime_mode_generation.load(std::sync::atomic::Ordering::SeqCst);
        let mut scope_parts = vec![
            format!("{}:{:?}:{:?}", frame.channel, frame.meta_get("scene_name"), frame.meta_get("source_name")),
            format!("mode-generation:{generation}"),
        ];
        scope_parts.sort();
        let scope = scope_parts.join("|");
        if let Ok(previous_path) = std::env::var("UNVAC_CAPTURE_PAST_IMAGE") {
            use base64::Engine;
            let image_url = format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(std::fs::read(previous_path).unwrap()));
            let previous = super::super::past_screen::Frame { observation: source.clone(), image: serde_json::from_value(serde_json::json!({"type":"input_image","image_url":image_url,"detail":"auto"})).unwrap() };
            assert!(service.past_screen.lock().await.select(scope.clone(), Some(previous), None, now).is_none());
        }
        let mut history = service.visual_history.lock().await;
        history.prepare(scope.clone(), std::slice::from_ref(&source), now);
        history.record(&notes, std::slice::from_ref(&source), now);
        assert!(history.prepare(scope, &[source], now).is_some(), "generated notes must survive production retention rules");
    }
    state.read().await.push_channel_datum_quiet(frame).await;
    let trigger = ChannelDatum::new("user".into(), prompt.clone()).with_flag(ChannelDatum::FLAG_IS_FINAL);
    let observation = Observation { datum_id: trigger.get_id(), channel: trigger.channel.clone() };
    state.read().await.push_channel_datum_quiet(trigger).await;
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(20), service.react(observation)).await;
    let request = server.join().expect("offline capture server");
    assert!(outcome.is_ok(), "capture must finish without waiting on external services");
    let serialized = serde_json::to_string_pretty(&request).unwrap();
    assert!(serialized.contains("input_image"), "saved image must reach the production request");
    assert!(serialized.contains("visual_notes"), "production visual-note schema must be present");
    assert!(serialized.contains(&prompt));
    if std::env::var_os("UNVAC_CAPTURE_HISTORY").is_some() {
        assert!(serialized.contains("[過去画像の読取メモ・非信頼データ・現在状態ではない]"));
    }
    let image_count = request["input"].as_array().unwrap().iter()
        .filter_map(|item| item["content"].as_array())
        .flatten().filter(|part| part["type"] == "input_image").count();
    let expects_previous = std::env::var_os("UNVAC_CAPTURE_PAST_IMAGE").is_some()
        && super::super::past_screen::refers_to_past_screen(&prompt);
    assert_eq!(image_count, if expects_previous { 2 } else { 1 },
        "offline comparison must actually contain the intended one/two-image input");
    let mut file = std::fs::File::create_new(output).unwrap();
    file.write_all(serialized.as_bytes()).unwrap();
}

#[tokio::test]
async fn saved_visual_settings_drive_first_provider_input() {
    let root=std::env::temp_dir().join(format!("vac-visual-reopen-{}",rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let path=root.join("conf.toml");
    let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();let address=listener.local_addr().unwrap();
    let server=std::thread::spawn(move || {
        let mut requests=Vec::new();
        for _ in 0..4 {let (connection,request)=receive(&listener);respond(connection,0,"UNMATCHED_NOTE");requests.push(request);}
        requests
    });
    let mut conf:crate::Conf=toml::from_str("").unwrap();
    conf.runtime_dir=Some(root.join("runtime"));
    conf.perception=toml::from_str("[[obs_sources]]\nid='saved-source'\nenabled=true\nrefresh_before_reply=false\nchannel='obs:new'\nscene_name='saved-scene'\nsource_name='saved-game'\n").unwrap();
    let mut persona=super::chi6_resolver_tests::persona_minimal();
    persona.api_key=Some("local-capture-only".into());persona.openai_stream=Some(true);
    persona.observe.triggers=vec!["user".into()];persona.observe.include_all=false;
    persona.observe.include_additional=vec!["obs:new".into()];
    persona.turn_management=Some(super::super::config::AiTurnManagementConf {
        enabled:true,visual_history_enabled:true,past_screen_image_enabled:false,..Default::default()
    });
    conf.ai.personas=vec![persona];
    for enabled in [false,true] {
        conf.ai.personas[0].turn_management.as_mut().unwrap().past_screen_image_enabled=enabled;
        crate::profile_store::write_config_transaction(&path,toml::to_string(&conf).unwrap().as_bytes()).unwrap();
        let reopened=crate::Conf::new_noop_probe(path.clone()).unwrap();
        assert_eq!(reopened.ai.personas[0].turn_management.as_ref().unwrap().past_screen_image_enabled,enabled);
        let state=crate::State::new(&reopened,Arc::new(Mutex::new(crate::AudioSink::open_default().unwrap())),crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let mut service=AiService::new(reopened.ai.personas[0].clone(),state.clone(),None,None,None,
            Arc::new(AtomicBool::new(false)),Arc::new(RwLock::new(AiUsageTelemetry::default()))).await.unwrap();
        service.client=ResponsesClient::new(ResponsesClientConfig {api_key:"local-capture-only".into(),base_url:format!("http://{address}"),..Default::default()}).unwrap();
        for (round,pixel) in ["bmV3LWZpcnN0","bmV3LXNlY29uZA=="].into_iter().enumerate() {
            for (channel,source,image) in [("obs:old","old-game","T0xEX1NPVVJDRQ=="),("obs:new","saved-game",pixel)] {
                let frame=ChannelDatum::new(channel.into(),"synthetic frame".into()).with_flag(ChannelDatum::FLAG_IS_FINAL)
                    .with_source(crate::DataSource::new("obs.scene_source"))
                    .with_meta(ChannelDatum::META_STATE_KEY,"obs_source:saved-source")
                    .with_meta("scene_name","saved-scene").with_meta("source_name",source)
                    .with_meta("observed_at",jiff::Timestamp::now().to_string())
                    .with_attachment(crate::Attachment::Url{url:format!("data:image/png;base64,{image}"),mime:Some("image/png".into()),title:None});
                state.read().await.push_channel_datum_quiet(frame).await;
            }
            let trigger=ChannelDatum::new("user".into(),if round==0 {"今の画面は？"}else{"さっきのスキル名は？"}.into()).with_flag(ChannelDatum::FLAG_IS_FINAL);
            let observation=Observation{datum_id:trigger.get_id(),channel:trigger.channel.clone()};
            state.read().await.push_channel_datum_quiet(trigger).await;
            tokio::time::timeout(Duration::from_secs(10),service.react(observation)).await.unwrap().unwrap();
        }
    }
    let requests=server.join().unwrap();
    let counts:Vec<_>=requests.iter().map(|r|r["input"].as_array().unwrap().iter().filter_map(|m|m["content"].as_array()).flatten().filter(|p|p["type"]=="input_image").count()).collect();
    assert_eq!(counts,vec![1,1,1,2],"Saved history setting must govern new service, whose initial cache is empty");
    for request in &requests {
        let text=request["input"].to_string();
        assert!(!text.contains("T0xEX1NPVVJDRQ=="),"Excluded channel reached provider");
        assert!(text.contains("data:image/png;base64,bmV3L"),"Saved channel missing from provider");
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn failure_diagnostic_real_transport_recovers_quota_then_parse_failure() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for round in 0..3 {
            let (mut connection, request) = receive(&listener);
            assert_eq!(request["stream"], true);
            if round == 0 {
                let body = r#"{"error":{"code":"credit_balance_exhausted","type":"insufficient_quota","message":"PRIVATE_PROVIDER_SENTINEL"}}"#;
                write!(connection, "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            } else if round == 1 {
                let event = serde_json::json!({"type":"response.completed","response":{"id":"invalid-turn","status":"completed","output":[],"output_text":"not turn JSON"}});
                let body = format!("event: response.completed\ndata: {event}\n\n");
                write!(connection, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            } else {
                respond(connection, 0, "UNMATCHED_NOTE");
            }
        }
    });
    let mut conf: crate::Conf = toml::from_str("").unwrap();
    conf.runtime_dir = Some(std::env::temp_dir().join(format!("vac-health-recovery-{}", rand::random::<u64>())));
    let audio = Arc::new(Mutex::new(crate::AudioSink::disconnected_for_test()));
    let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
    let mut persona = super::chi6_resolver_tests::persona_minimal();
    persona.api_key = Some("local-capture-only".into());
    persona.openai_stream = Some(true);
    persona.observe.triggers = vec!["user".into()];
    persona.observe.include_all = true;
    persona.turn_management = Some(super::super::config::AiTurnManagementConf {enabled:true,..Default::default()});
    let mut service = AiService::new(persona, state.clone(), None, None, None,
        Arc::new(AtomicBool::new(false)), Arc::new(RwLock::new(AiUsageTelemetry::default()))).await.unwrap();
    service.client = ResponsesClient::new(ResponsesClientConfig {api_key:"local-capture-only".into(),base_url:format!("http://{address}"),..Default::default()}).unwrap();
    let health = state.read().await.runtime_health.clone();
    health.report(crate::runtime_health::RuntimeIssue::error("other-feature", "fixture", "fixture", "unrelated", "fixture", "observability", None)).await;
    for round in 0..3 {
        let datum = ChannelDatum::new("user".into(), format!("local recovery request {round}")).with_flag(ChannelDatum::FLAG_IS_FINAL);
        let observation = Observation {datum_id:datum.get_id(),channel:datum.channel.clone()};
        state.read().await.push_channel_datum_quiet(datum).await;
        let result = tokio::time::timeout(Duration::from_secs(10), service.react(observation)).await.unwrap();
        let issues = health.snapshot().await;
        assert!(issues.iter().any(|i| i.key == "other-feature"));
        assert!(!serde_json::to_string(&issues).unwrap().contains("PRIVATE_PROVIDER_SENTINEL"));
        let own:Vec<_> = issues.iter().filter(|i| i.component == "ai_reaction").collect();
        match round {
            0 => { assert!(result.is_err()); assert_eq!(own.len(),1); assert_eq!(own[0].code,"quota_exhausted"); }
            1 => { assert!(result.is_err()); assert_eq!(own.len(),1); assert!(own[0].key.ends_with(":turn_output_parse")); }
            _ => { result.unwrap(); assert!(own.is_empty(), "Successful parsed response must clear preceding errors"); }
        }
    }
    server.join().unwrap();
}

use super::*;
use std::io::{Read, Write};

#[tokio::test]
async fn terminal_response_does_not_wait_for_http_body_eof() {
    for status in ["completed", "incomplete"] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (release, wait) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                connection.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            let header = String::from_utf8(header).unwrap().to_lowercase();
            let length: usize = header.lines().find_map(|line| line.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
            connection.read_exact(&mut vec![0; length]).unwrap();
            let event = serde_json::json!({"type":format!("response.{status}"),"response":{
                "id":"terminal", "status":status, "output":[], "output_text":"finished",
                "usage":{"input_tokens":12,"output_tokens":3,"total_tokens":15}
            }});
            // No Content-Length, chunk terminator, [DONE], or EOF: completion
            // must follow the semantic terminal event, not socket closure.
            write!(connection, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\nevent: response.{status}\ndata: {event}\n\n").unwrap();
            connection.flush().unwrap();
            let _ = wait.recv_timeout(std::time::Duration::from_secs(5));
        });
        let mut conf: crate::Conf = toml::from_str("").unwrap();
        conf.runtime_dir = Some(std::env::temp_dir().join(format!("vac_terminal_response_{}", rand::random::<u64>())));
        let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
        let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
        let client = ResponsesClient::new(ResponsesClientConfig {
            api_key: "local-test".into(), base_url: format!("http://{address}"), ..Default::default()
        }).unwrap();
        let request = CreateResponseRequest { model: "gpt-5.6-luna".into(), ..Default::default() };
        let result = tokio::time::timeout(std::time::Duration::from_secs(2),
            drive_responses_tool_loop(&client, request, true, false, &Default::default(), None, &state, "test-output", "test", false, None, None)
        ).await;
        let _ = release.send(());
        server.join().unwrap();
        let output = result.expect("terminal response must not wait for HTTP EOF").unwrap();
        assert_eq!(output.content, "finished");
        assert_eq!(output.usage.api_requests, 1);
        assert_eq!(output.usage.input_tokens, 12);
        assert_eq!(output.usage.output_tokens, 3);
    }
}

#[tokio::test]
async fn empty_response_retry_preserves_context_and_is_bounded() {
    for streaming in [false, true] {
        for recover in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let mut requests = Vec::new();
                for attempt in 0..2 {
                    let (mut connection, _) = listener.accept().unwrap();
                    connection.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
                    let mut bytes = Vec::new();
                    loop {
                        let mut one = [0];
                        connection.read_exact(&mut one).unwrap();
                        bytes.push(one[0]);
                        if bytes.ends_with(b"\r\n\r\n") { break; }
                    }
                    let header_end = bytes.len();
                    let header = String::from_utf8(bytes.clone()).unwrap().to_lowercase();
                    let length: usize = header.lines().find_map(|line| line.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
                    bytes.resize(header_end + length, 0);
                    connection.read_exact(&mut bytes[header_end..]).unwrap();
                    requests.push(serde_json::from_slice::<serde_json::Value>(&bytes[header_end..]).unwrap());
                    let text = if recover && attempt == 1 {
                        r#"{"action":"respond","speak":"62パーセントだ。","episode_memory":null,"avatar_intent":null,"avatar_pose":null,"visual_notes":[{"datum_id":42,"kind":"state","readouts":[{"label":"HUD","value":"62%"}],"evidence":""}]}"#
                    } else { "" };
                    let response = serde_json::json!({"id":"mock","status":"completed","output":[],"output_text":text});
                    let (mime, body) = if streaming {
                        let delta = if text.is_empty() { String::new() } else {
                            format!("event: response.output_text.delta\ndata: {}\n\n", serde_json::json!({"type":"response.output_text.delta","item_id":"msg","output_index":0,"content_index":0,"delta":text}))
                        };
                        ("text/event-stream", format!("{delta}event: response.completed\ndata: {}\n\n", serde_json::json!({"type":"response.completed","response":response})))
                    } else { ("application/json", response.to_string()) };
                    write!(connection, "HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                }
                requests
            });
            let mut conf: crate::Conf = toml::from_str("").unwrap();
            conf.runtime_dir = Some(std::env::temp_dir().join(format!("vac_empty_retry_{}", rand::random::<u64>())));
            let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
            let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
            let client = ResponsesClient::new(ResponsesClientConfig {
                api_key: "local-test".into(), base_url: format!("http://{address}"), ..Default::default()
            }).unwrap();
            let request = CreateResponseRequest {
                model: "gpt-5.6-luna".into(), max_output_tokens: Some(2000), text: Some(turn_manager_text_config_with_history(true)),
                input: vec![InputItem::message("developer", "Keep this persona"),
                    serde_json::from_value(serde_json::json!({"type":"message","role":"user","content":[{"type":"input_text","text":"この画像の数字は？"},{"type":"input_image","image_url":"data:image/png;base64,dGVzdA=="}]})).unwrap()],
                ..Default::default()
            };
            let output = drive_responses_tool_loop(&client, request, streaming, false, &Default::default(), None, &state, "test-output", "test", true, None, None).await.unwrap();
            assert_eq!(output.usage.api_requests, 2);
            assert_eq!(output.content.is_empty(), !recover);
            if recover {
                let turn = parse_turn_manager_output(&output.content).unwrap();
                assert_eq!(turn.speak.as_deref(), Some("62パーセントだ。"));
                assert_eq!(turn.visual_notes[0].datum_id, 42);
                assert_eq!(turn.visual_notes[0].kind, super::super::visual_history::VisualNoteKind::State);
                assert_eq!(turn.visual_notes[0].readouts[0].value, "62%");
            }
            let requests = server.join().unwrap();
            assert_eq!(requests[0], requests[1], "retry must retain the entire request");
            assert!(requests[1]["text"]["format"].is_object());
            assert_eq!(requests[1]["input"][0]["content"], "Keep this persona");
        }
    }
}

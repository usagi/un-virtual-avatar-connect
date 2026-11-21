use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn chat_send_diagnostic_records_local_service_outcomes_without_payload() {
    let temp = std::env::temp_dir().join(format!("vac_chat_send_diagnostic_{}", rand::random::<u64>()));
    let mut conf: crate::Conf = toml::from_str("").unwrap();
    conf.runtime_dir = Some(temp.join("runtime"));
    let audio = std::sync::Arc::new(tokio::sync::Mutex::new(crate::AudioSink::open_default().unwrap()));
    let state = crate::State::new(&conf, audio, crate::shutdown::ShutdownBroker::new()).await.unwrap();
    let mut events = state.read().await.control_event_tx.subscribe();
    let mut ctx = ExecCtx { node_id: "review::notice_chat".into(), state_handle: Some(std::sync::Arc::downgrade(&state)), ..Default::default() };
    let mut inputs = InputMap::new();
    for (key, value) in [("text", "PRIVATE_TEST_MESSAGE"), ("broadcaster_id", "1"), ("sender_user_id", "2"), ("access_token", "PRIVATE_TEST_TOKEN"), ("client_id", "test-client")] {
        inputs.insert(key.into(), SocketValue::String(value.into()));
    }
    let mut fire = ExecFireSet::default(); fire.insert("exec_in");
    for (status, body, expected) in [
        (200, r#"{"data":[{"is_sent":true}]}"#, "sent"),
        (200, r#"{"data":[{"is_sent":false,"drop_reason":{"message":"PRIVATE_REMOTE_ERROR"}}]}"#, "failed"),
        (200, "invalid-json-PRIVATE_REMOTE_ERROR", "failed"),
        (403, "PRIVATE_REMOTE_ERROR", "failed"),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                let mut byte = [0]; socket.read_exact(&mut byte).await.unwrap(); header.push(byte[0]);
                assert!(header.len() < 16384);
            }
            let header = String::from_utf8(header).unwrap().to_lowercase();
            let length: usize = header.lines().find_map(|line| line.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
            socket.read_exact(&mut vec![0; length]).await.unwrap();
            let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        inputs.insert("endpoint".into(), SocketValue::String(format!("http://{address}")));
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), ChatSendNode.execute(&mut ctx, &InputMap::new(), &inputs, &fire)).await.unwrap();
        server.await.unwrap();
        assert_eq!(chat_send_outcome(&result), expected);
        let event = tokio::time::timeout(std::time::Duration::from_secs(1), events.recv()).await.unwrap().unwrap();
        assert_eq!(serde_json::to_value(event).unwrap(), json!({"kind":"twitch_chat_send_result","node_id":"review::notice_chat","outcome":expected}));
    }
    // Early exits must also be observed, without another network request.
    inputs.insert("text".into(), SocketValue::String(" ".into()));
    ChatSendNode.execute(&mut ctx, &InputMap::new(), &inputs, &fire).await.unwrap();
    assert_eq!(serde_json::to_value(events.recv().await.unwrap()).unwrap()["outcome"], "skipped_empty");
    inputs.remove("text");
    assert!(ChatSendNode.execute(&mut ctx, &InputMap::new(), &inputs, &fire).await.is_err());
    assert_eq!(serde_json::to_value(events.recv().await.unwrap()).unwrap()["outcome"], "failed");
    ChatSendNode.execute(&mut ctx, &InputMap::new(), &inputs, &ExecFireSet::default()).await.unwrap();
    assert!(events.try_recv().is_err(), "Inactive node must not report a send attempt");

    let journal = std::fs::read_dir(temp.join("runtime")).unwrap().flatten()
        .find(|entry| entry.path().is_dir()).unwrap().path().join("control-events.diagnostic.jsonl");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let text = std::fs::read_to_string(&journal).unwrap_or_default();
            if text.matches("twitch_chat_send_result").count() == 6 {
                for secret in ["PRIVATE_TEST_MESSAGE", "PRIVATE_TEST_TOKEN", "PRIVATE_REMOTE_ERROR"] { assert!(!text.contains(secret)); }
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await.unwrap();
}

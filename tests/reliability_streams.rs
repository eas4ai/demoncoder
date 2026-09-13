use demoncoder::{
    adapters,
    config::Connection,
    events::{Event, EventSink},
    session::TurnEnd,
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};

const LIMIT: usize = 1024 * 1024;

async fn read_request(socket: &mut TcpStream) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut request = Vec::new();
        let mut buf = [0; 8192];
        loop {
            let n = socket.read(&mut buf).await.unwrap();
            assert_ne!(n, 0);
            request.extend_from_slice(&buf[..n]);
            assert!(request.len() <= 4 * LIMIT, "fixture request exceeded bound");
            if let Some(end) = request.windows(4).position(|x| x == b"\r\n\r\n") {
                let header = std::str::from_utf8(&request[..end]).unwrap();
                let length: usize = header
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                if request.len() >= end + 4 + length {
                    return serde_json::from_slice(&request[end + 4..end + 4 + length]).unwrap();
                }
            }
        }
    })
    .await
    .expect("fixture request did not finish")
}

async fn stream_case(bytes: usize, fragment: usize, inline: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    let (sent, received) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let (finished, mut finish) = oneshot::channel();
    let prefix = r#"{"path":"stream-write","content":""#;
    let suffix = "\"}";
    // Multibyte contents distinguish a byte limit from a character limit.
    let fill = bytes - prefix.len() - suffix.len();
    let content = "é".repeat(fill / 2) + &"x".repeat(fill % 2);
    let arguments = format!("{prefix}{content}{suffix}");
    assert_eq!(arguments.len(), bytes);
    let mut server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut requests = vec![read_request(&mut socket).await];
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        let input = if inline {
            serde_json::from_str(&arguments).unwrap()
        } else {
            json!({})
        };
        let mut values = vec![
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"earlier-call","name":"write","input":{"path":"earlier-write","content":"effect"}}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"stream-call","name":"write","input":input}}),
        ];
        let mut start = 0;
        while !inline && start < arguments.len() {
            let mut end = (start + fragment).min(arguments.len());
            while !arguments.is_char_boundary(end) {
                end -= 1;
            }
            values.push(json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":&arguments[start..end]}}));
            start = end;
        }
        for value in values {
            socket
                .write_all(format!("data: {value}\n\n").as_bytes())
                .await
                .unwrap();
        }
        sent.send(()).unwrap();
        // Deliberately omit block_stop and message_stop until the test releases us.
        let _ = released.await;
        for value in [
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
            json!({"type":"message_stop"}),
        ] {
            if socket
                .write_all(format!("data: {value}\n\n").as_bytes())
                .await
                .is_err()
            {
                break;
            }
        }
        loop {
            // A queued request wins over the turn-finished signal, so unexpected
            // ordinary or summary traffic is retained instead of hidden by cleanup.
            let mut next = tokio::select! { biased;
                connection = listener.accept() => connection.unwrap().0,
                _ = &mut finish => return requests,
            };
            assert!(requests.len() < 2, "unexpected third provider request");
            requests.push(read_request(&mut next).await);
            let body = format!("data: {}\n\n", json!({"type":"message_stop"}));
            next.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
    });
    let root = tempfile::tempdir().unwrap();
    let config: Connection = serde_json::from_value(json!({"adapter":"anthropic-api","model":"fixture","endpoint":endpoint,"api_key":"synthetic-stream-key","max_output_tokens":1000000})).unwrap();
    let mut session = adapters::builtins()
        .unwrap()
        .open(&config, root.path())
        .unwrap();
    let (_commands, mut commands) = mpsc::channel(8);
    let (tx, mut rx) = mpsc::channel(128);
    let sink = EventSink::new("stream-limit".into(), tx, None).unwrap();
    let mut turn =
        tokio::spawn(async move { session.turn("write".into(), &mut commands, &sink).await });
    tokio::time::timeout(Duration::from_secs(5), received)
        .await
        .unwrap()
        .unwrap();
    let outcome = if bytes > LIMIT {
        let outcome = tokio::time::timeout(Duration::from_secs(1), &mut turn).await;
        let _ = release.send(());
        outcome
    } else {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut turn)
                .await
                .is_err(),
            "valid response must await completion"
        );
        assert!(!root.path().join("stream-write").exists());
        assert!(!root.path().join("earlier-write").exists());
        while let Ok(envelope) = rx.try_recv() {
            assert!(!matches!(
                envelope.event,
                Event::ToolStarted { .. } | Event::ToolFinished { .. }
            ));
        }
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), &mut turn).await
    };
    if outcome.is_err() {
        turn.abort();
        let _ = turn.await;
    }
    let _ = finished.send(());
    let requests = match tokio::time::timeout(Duration::from_secs(5), &mut server).await {
        Ok(result) => result.unwrap(),
        Err(_) => {
            server.abort();
            let _ = server.await;
            panic!("stream fixture did not stop after turn completion");
        }
    };
    let outcome = outcome
        .expect("turn did not settle within the required stream phase")
        .unwrap();
    let mut started = Vec::new();
    let mut completed = Vec::new();
    while let Ok(envelope) = rx.try_recv() {
        match envelope.event {
            Event::ToolStarted { call } => started.push(call.id),
            Event::ToolFinished { result } => completed.push(result),
            _ => {}
        }
    }
    assert!(
        requests
            .iter()
            .all(|r| r["tools"].as_array().is_some_and(|t| !t.is_empty())),
        "unexpected tool-free summary request"
    );
    if bytes > LIMIT {
        let Err(error) = outcome else {
            panic!("oversized arguments accepted");
        };
        assert!(
            format!("{error:#}").contains(
                "Anthropic tool input exceeds 1 MiB; no tool calls from this response were executed"
            ),
            "{error:#}"
        );
        assert!(!root.path().join("stream-write").exists());
        assert!(!root.path().join("earlier-write").exists());
        assert!(started.is_empty() && completed.is_empty());
        assert_eq!(requests.len(), 1);
    } else {
        assert_eq!(
            std::fs::read_to_string(root.path().join("earlier-write")).unwrap(),
            "effect"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("stream-write")).unwrap(),
            content
        );
        assert_eq!(started, ["earlier-call", "stream-call"]);
        assert_eq!(
            completed
                .iter()
                .map(|r| r.call_id.as_str())
                .collect::<Vec<_>>(),
            started
        );
        assert!(completed.iter().all(|r| r.success));
        if bytes == LIMIT {
            // The complete response was admitted. Only the following model
            // boundary is held: its framed history exceeds the summary cap.
            let Err(error) = outcome else {
                panic!("exact-limit history falsely completed the turn");
            };
            assert_eq!(
                error.to_string(),
                "Context compaction held: source exceeds 1 MiB"
            );
            assert_eq!(
                requests.len(),
                1,
                "unexpected request after compaction preparation held"
            );
        } else {
            assert!(matches!(outcome.unwrap(), TurnEnd::Complete));
            assert_eq!(requests.len(), 2);
            let history = requests[1]["messages"].as_array().unwrap();
            let calls = history[1]["content"].as_array().unwrap();
            let results = history[2]["content"].as_array().unwrap();
            assert_eq!(
                calls
                    .iter()
                    .map(|c| c["id"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                started
            );
            assert_eq!(
                results
                    .iter()
                    .map(|r| r["tool_use_id"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                started
            );
            assert!(results.iter().all(|r| r["type"] == "tool_result"));
        }
    }
}

#[tokio::test]
async fn anthropic_rejects_excess_before_completion() {
    for fragment in [64 * 1024, LIMIT + 1] {
        stream_case(LIMIT + 1, fragment, false).await;
    }
}

#[tokio::test]
async fn anthropic_accepts_exact_byte_bound_after_completion() {
    stream_case(LIMIT, 64 * 1024, false).await;
}

#[tokio::test]
async fn anthropic_bounds_inline_input_before_admitting_any_response_calls() {
    stream_case(LIMIT + 1, LIMIT + 1, true).await;
    stream_case(LIMIT, LIMIT, true).await;
}

#[tokio::test]
async fn anthropic_valid_stream_below_compaction_threshold_completes_with_paired_results() {
    stream_case(LIMIT / 4, 64 * 1024, false).await;
}

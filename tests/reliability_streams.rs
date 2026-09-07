use demoncoder::{
    adapters,
    config::Connection,
    events::{Event, EventSink},
    session::TurnEnd,
};
use serde_json::json;
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
};

const LIMIT: usize = 1024 * 1024;

async fn stream_case(extra: usize, fragment: usize, inline: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    let (sent, received) = oneshot::channel();
    let (release, released) = oneshot::channel();
    let prefix = r#"{"path":"stream-write","content":""#;
    let suffix = "\"}";
    // Multibyte contents distinguish a byte limit from a character limit.
    let fill = LIMIT + extra - prefix.len() - suffix.len();
    let content = "é".repeat(fill / 2) + &"x".repeat(fill % 2);
    let arguments = format!("{prefix}{content}{suffix}");
    assert_eq!(arguments.len(), LIMIT + extra);
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 8192];
        loop {
            let n = socket.read(&mut buf).await.unwrap();
            assert_ne!(n, 0);
            request.extend_from_slice(&buf[..n]);
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
                    break;
                }
            }
        }
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
                return;
            }
        }
        if extra == 0 {
            let (mut next, _) = listener.accept().await.unwrap();
            let _ = next.read(&mut buf).await.unwrap();
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
    if extra > 0 {
        let outcome = tokio::time::timeout(Duration::from_secs(1), &mut turn).await;
        if outcome.is_err() {
            turn.abort();
        }
        let _ = release.send(());
        server.await.unwrap();
        let error = outcome
            .expect("oversized arguments must fail while stream is held open")
            .unwrap()
            .err()
            .expect("oversized arguments accepted");
        assert!(format!("{error:#}").contains("1 MiB"), "{error:#}");
        assert!(!root.path().join("stream-write").exists());
        assert!(!root.path().join("earlier-write").exists());
        while let Ok(envelope) = rx.try_recv() {
            assert!(!matches!(
                envelope.event,
                Event::ToolStarted { .. } | Event::ToolFinished { .. }
            ));
        }
    } else {
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut turn)
                .await
                .is_err(),
            "exact-bound response must await completion"
        );
        assert!(!root.path().join("stream-write").exists());
        assert!(!root.path().join("earlier-write").exists());
        release.send(()).unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(5), turn)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            TurnEnd::Complete
        ));
        server.await.unwrap();
        assert_eq!(
            std::fs::read_to_string(root.path().join("earlier-write")).unwrap(),
            "effect"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("stream-write"))
                .unwrap()
                .len(),
            fill
        );
    }
}

#[tokio::test]
async fn anthropic_rejects_excess_before_completion() {
    for fragment in [64 * 1024, LIMIT + 1] {
        stream_case(1, fragment, false).await;
    }
}

#[tokio::test]
async fn anthropic_accepts_exact_byte_bound_after_completion() {
    stream_case(0, 64 * 1024, false).await;
}

#[tokio::test]
async fn anthropic_bounds_inline_input_before_admitting_any_response_calls() {
    stream_case(1, LIMIT + 1, true).await;
    stream_case(0, LIMIT, true).await;
}

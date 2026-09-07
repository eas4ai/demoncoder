use demoncoder::{
    adapters,
    config::Connection,
    events::{Event, EventSink},
    session::{Session, TurnEnd},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tokio::sync::mpsc;

#[derive(Clone)]
struct Request {
    method: String,
    path: String,
    body: Value,
}
struct Reply {
    status: u16,
    body: String,
    kind: &'static str,
    location: Option<String>,
}
impl Reply {
    fn json(value: Value) -> Self {
        Self {
            status: 200,
            body: value.to_string(),
            kind: "application/json",
            location: None,
        }
    }
    fn events(values: Vec<Value>) -> Self {
        Self {
            status: 200,
            body: values
                .into_iter()
                .map(|v| format!("data: {v}\n\n"))
                .collect(),
            kind: "text/event-stream",
            location: None,
        }
    }
}
struct Server {
    endpoint: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new(handler: impl Fn(&Request) -> Reply + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("accept fixture request: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 4096];
                let header_end = loop {
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0, "closed before request headers");
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break end + 4;
                    }
                };
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < header_end + length {
                    let n = stream.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                }
                let mut first = headers.lines().next().unwrap().split_whitespace();
                let request = Request {
                    method: first.next().unwrap().into(),
                    path: first.next().unwrap().into(),
                    body: if length == 0 {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
                    },
                };
                recorded.lock().unwrap().push(request.clone());
                let reply = handler(&request);
                let response = format!(
                    "HTTP/1.1 {} fixture\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n{}",
                    reply.status,
                    reply.kind,
                    reply.body.len(),
                    reply
                        .location
                        .map(|url| format!("Location: {url}\r\n"))
                        .unwrap_or_default(),
                    reply.body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            worker: Some(worker),
        }
    }
    fn config(&self, limit: Option<u32>) -> Connection {
        let mut value = json!({"adapter":"anthropic-api", "model":"modern-model-alias", "endpoint":self.endpoint, "api_key":"synthetic-limit-key"});
        if let Some(limit) = limit {
            value["max_output_tokens"] = limit.into();
        }
        let parsed = serde_json::from_value(value);
        assert!(
            parsed.is_ok(),
            "explicit output settings must be accepted: {:?}",
            parsed.as_ref().err()
        );
        parsed.unwrap()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn anthropic(stop: &str, tool: bool) -> Reply {
    let block = if tool {
        json!({"type":"tool_use","id":"write-call","name":"write","input":{"path":"must-not-exist","content":"changed"}})
    } else {
        json!({"type":"text","text":"long output ".repeat(5000)})
    };
    Reply::events(vec![
        json!({"type":"message_start","message":{"usage":{"input_tokens":17}}}),
        json!({"type":"content_block_start","index":0,"content_block":block}),
        json!({"type":"message_delta","delta":{"stop_reason":stop},"usage":{"output_tokens":8192}}),
        json!({"type":"message_stop"}),
    ])
}
async fn turn(session: &mut dyn Session, prompt: &str) -> (anyhow::Result<TurnEnd>, Vec<Event>) {
    let (_commands, mut receiver) = mpsc::channel(8);
    let (tx, mut rx) = mpsc::channel(64);
    let events = EventSink::new("output-limits".into(), tx, None).unwrap();
    let outcome = session.turn(prompt.into(), &mut receiver, &events).await;
    let mut observed = Vec::new();
    while let Ok(envelope) = rx.try_recv() {
        observed.push(envelope.event);
    }
    (outcome, observed)
}

#[tokio::test]
async fn model_limit_is_discovered_once_and_used_in_real_requests() {
    let server = Server::new(|request| {
        if request.method == "GET" {
            Reply::json(json!({"id":"modern-model-version","max_tokens":128_000}))
        } else {
            anthropic("end_turn", false)
        }
    });
    let root = tempfile::tempdir().unwrap();
    let mut session = adapters::builtins()
        .unwrap()
        .open(&server.config(None), root.path())
        .unwrap();
    for prompt in ["first", "next"] {
        assert!(matches!(
            turn(&mut *session, prompt).await.0,
            Ok(TurnEnd::Complete)
        ));
    }
    let requests = server.requests.lock().unwrap();
    let posts: Vec<_> = requests.iter().filter(|r| r.method == "POST").collect();
    assert_eq!(posts.len(), 2);
    assert_eq!(
        posts[0].body["max_tokens"], 128_000,
        "model output must not be fixed at 4096"
    );
    assert_eq!(posts[1].body["max_tokens"], 128_000);
    let gets: Vec<_> = requests.iter().filter(|r| r.method == "GET").collect();
    assert_eq!(gets.len(), 1);
    assert_eq!(gets[0].path, "/v1/models/modern-model-alias");
    assert_eq!(posts[0].body["model"], "modern-model-alias");
}

#[tokio::test]
async fn explicit_limit_bypasses_model_discovery() {
    let server = Server::new(|_| anthropic("end_turn", false));
    let root = tempfile::tempdir().unwrap();
    let mut session = adapters::builtins()
        .unwrap()
        .open(&server.config(Some(96_000)), root.path())
        .unwrap();
    assert!(matches!(
        turn(&mut *session, "first").await.0,
        Ok(TurnEnd::Complete)
    ));
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].body["max_tokens"], 96_000);
}

#[tokio::test]
async fn truncated_text_or_tool_calls_fail_with_usage_and_a_usable_next_prompt() {
    for stop in ["max_tokens", "model_context_window_exceeded"] {
        for tool in [false, true] {
            let stopped = stop.to_owned();
            let server = Server::new(move |request| {
                if request.method == "GET" {
                    return Reply::json(json!({"max_tokens":128_000}));
                }
                if request.body["messages"].as_array().unwrap().len() > 1 {
                    anthropic("end_turn", false)
                } else {
                    anthropic(&stopped, tool)
                }
            });
            let root = tempfile::tempdir().unwrap();
            let mut session = adapters::builtins()
                .unwrap()
                .open(&server.config(None), root.path())
                .unwrap();
            let (outcome, events) = turn(&mut *session, "first").await;
            assert!(
                outcome.is_err(),
                "{stop} must not complete or execute tools"
            );
            assert!(!root.path().join("must-not-exist").exists());
            assert!(events.iter().any(|event| matches!(
                event,
                Event::Usage {
                    output: Some(8192),
                    ..
                }
            )));
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, Event::ToolFinished { .. }))
            );
            assert!(matches!(
                turn(&mut *session, "continue").await.0,
                Ok(TurnEnd::Complete)
            ));
            let requests = server.requests.lock().unwrap();
            let last = &requests.last().unwrap().body["messages"];
            assert!(
                last.as_array()
                    .unwrap()
                    .iter()
                    .all(|entry| entry["role"] == "user"),
                "truncated calls must not pollute history"
            );
        }
    }
}

#[tokio::test]
async fn missing_or_invalid_model_limits_never_send_a_message() {
    for metadata in [
        json!({}),
        json!({"max_tokens":null}),
        json!({"max_tokens":0}),
        json!({"max_tokens":-1}),
        json!({"max_tokens":"128000"}),
        json!({"max_tokens":4_294_967_296_u64}),
    ] {
        let server = Server::new(move |request| {
            if request.method == "GET" {
                Reply::json(metadata.clone())
            } else {
                anthropic("end_turn", false)
            }
        });
        let root = tempfile::tempdir().unwrap();
        let mut session = adapters::builtins()
            .unwrap()
            .open(&server.config(None), root.path())
            .unwrap();
        let outcome = turn(&mut *session, "first").await.0;
        assert!(
            outcome.is_err(),
            "missing model limits must not use a guessed constant"
        );
        assert!(format!("{:#}", outcome.err().unwrap()).contains("max_output_tokens"));
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|r| r.method == "GET")
        );
    }
}

#[test]
fn output_limits_are_positive_and_unsupported_backends_reject_them() {
    for (adapter, limit) in [
        ("anthropic-api", 0),
        ("openai-api", 0),
        ("codex", 16000),
        ("claude", 16000),
    ] {
        let value = json!({"adapter":adapter,"max_output_tokens":limit});
        let config: Connection = serde_json::from_value(value)
            .expect("setting must deserialize for semantic validation");
        assert!(config.validate().is_err());
    }
}

#[tokio::test]
async fn openai_keeps_provider_defaults_or_uses_an_explicit_limit() {
    for limit in [None, Some(96_000)] {
        let server = Server::new(|_| {
            Reply::events(vec![
                json!({"type":"response.completed","response":{"output":[],"usage":{"output_tokens":8192}}}),
            ])
        });
        let root = tempfile::tempdir().unwrap();
        let mut config = server.config(limit);
        config.adapter = "openai-api".into();
        let mut session = adapters::builtins()
            .unwrap()
            .open(&config, root.path())
            .unwrap();
        assert!(matches!(
            turn(&mut *session, "first").await.0,
            Ok(TurnEnd::Complete)
        ));
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].body.get("max_output_tokens"),
            limit.map(Value::from).as_ref()
        );
    }
}

#[tokio::test]
async fn openai_incomplete_output_retains_usage_without_admitting_calls() {
    let server = Server::new(|_| {
        Reply::events(vec![
            json!({"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"},"output":[{"type":"function_call","call_id":"cut","name":"write","arguments":"{\"path\":\"must-not-exist\",\"content\":\"changed\"}"}],"usage":{"input_tokens":17,"output_tokens":8192}}}),
        ])
    });
    let root = tempfile::tempdir().unwrap();
    let mut config = server.config(None);
    config.adapter = "openai-api".into();
    let mut session = adapters::builtins()
        .unwrap()
        .open(&config, root.path())
        .unwrap();
    let (outcome, events) = turn(&mut *session, "first").await;
    assert!(outcome.is_err());
    assert!(!root.path().join("must-not-exist").exists());
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Usage {
            output: Some(8192),
            ..
        }
    )));
}

#[tokio::test]
async fn cancellation_during_discovery_prevents_message_admission() {
    let server = Server::new(|request| {
        assert_eq!(request.method, "GET");
        thread::sleep(Duration::from_millis(650));
        Reply::json(json!({"max_tokens":128_000}))
    });
    let root = tempfile::tempdir().unwrap();
    let mut session = adapters::builtins()
        .unwrap()
        .open(&server.config(None), root.path())
        .unwrap();
    let (commands, mut receiver) = mpsc::channel(8);
    let (tx, _rx) = mpsc::channel(64);
    let events = EventSink::new("output-cancel".into(), tx, None).unwrap();
    let task =
        tokio::spawn(async move { session.turn("first".into(), &mut receiver, &events).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while server.requests.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    commands
        .send(demoncoder::session::Command::Cancel)
        .await
        .unwrap();
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(300), task)
            .await
            .unwrap()
            .unwrap(),
        Ok(TurnEnd::Cancelled)
    ));
    assert!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| r.method == "GET")
    );
}

#[tokio::test]
async fn discovery_errors_are_actionable_and_do_not_reflect_secret_bodies() {
    for status in [302, 401, 404, 503] {
        let server = Server::new(move |_| Reply {
            status,
            kind: "application/json",
            location: None,
            body: "secret-reflected-key".into(),
        });
        let root = tempfile::tempdir().unwrap();
        let mut session = adapters::builtins()
            .unwrap()
            .open(&server.config(None), root.path())
            .unwrap();
        let error = turn(&mut *session, "first")
            .await
            .0
            .err()
            .expect("metadata failure")
            .to_string();
        assert!(error.contains(&status.to_string()));
        assert!(error.contains("max_output_tokens"));
        assert!(!error.contains("secret-reflected-key"));
        assert!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .all(|r| r.method == "GET")
        );
    }
}

#[tokio::test]
async fn discovery_encodes_model_names_without_changing_the_endpoint_origin() {
    let server = Server::new(|request| {
        if request.method == "GET" {
            Reply::json(json!({"max_tokens":64_000}))
        } else {
            anthropic("end_turn", false)
        }
    });
    let root = tempfile::tempdir().unwrap();
    let mut config = server.config(None);
    config.model = Some("family/model?revision=1".into());
    let mut session = adapters::builtins()
        .unwrap()
        .open(&config, root.path())
        .unwrap();
    assert!(matches!(
        turn(&mut *session, "first").await.0,
        Ok(TurnEnd::Complete)
    ));
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests[0].path, "/v1/models/family%2Fmodel%3Frevision=1");
    assert_eq!(requests[1].body["model"], "family/model?revision=1");
}

#[tokio::test]
async fn malformed_or_oversized_metadata_fails_without_reflecting_its_body() {
    for body in ["secret-reflected-key".to_owned(), "x".repeat(65_537)] {
        let server = Server::new(move |_| Reply {
            status: 200,
            kind: "application/json",
            location: None,
            body: body.clone(),
        });
        let root = tempfile::tempdir().unwrap();
        let mut session = adapters::builtins()
            .unwrap()
            .open(&server.config(None), root.path())
            .unwrap();
        let error = turn(&mut *session, "first")
            .await
            .0
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("max_output_tokens"));
        assert!(!error.contains("secret-reflected-key"));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert_eq!(server.requests.lock().unwrap()[0].method, "GET");
    }
}

#[tokio::test]
async fn metadata_redirect_does_not_contact_another_origin() {
    let target = Server::new(|_| Reply::json(json!({"max_tokens":128_000})));
    let location = target.endpoint.clone();
    let server = Server::new(move |_| Reply {
        status: 302,
        kind: "application/json",
        location: Some(location.clone()),
        body: String::new(),
    });
    let root = tempfile::tempdir().unwrap();
    let mut session = adapters::builtins()
        .unwrap()
        .open(&server.config(None), root.path())
        .unwrap();
    assert!(turn(&mut *session, "first").await.0.is_err());
    assert!(target.requests.lock().unwrap().is_empty());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

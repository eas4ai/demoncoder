use demoncoder::{
    config::Connection,
    events::EventSink,
    session::{Session, SessionStart, TurnEnd},
    workflow::{WorkflowSession, runtime::SharedRuntime},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

#[path = "support/workspace_change_runners.rs"]
mod workspace_change_runners;

fn native_session_end_plan() -> (
    tempfile::TempDir,
    Arc<demoncoder::plugins::non_tool::NonToolPlan>,
) {
    use demoncoder::plugins::{
        self,
        dispatch::{Declaration, DeclarationIdentity, HandlerClass, Matcher, Scope},
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        runners::{CommandConfig, CommandProgram, CommandRunner},
    };

    let source = tempfile::tempdir().unwrap();
    std::fs::create_dir(source.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        source.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"workspace-session-end","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(
        source.path().join("hook.py"),
        r#"import json,os,sys
x=json.load(sys.stdin)
assert x['hook_event_name']=='SessionEnd'
with open('session-end-canary.txt','w') as f: f.write(os.getcwd())
print('{}')
"#,
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
    let declaration = Declaration {
        required_gate: false,
        source: None,
        once: None,
        identity: DeclarationIdentity {
            package: "workspace-session-end".into(),
            code: "replaced-by-captured-code".into(),
            policy: "workspace-session-end-policy".into(),
            configuration: "replaced-by-config".into(),
            generation: "workspace-session-end-generation".into(),
            scope: Scope::Project,
            role: "worker".into(),
            declaration: "workspace-session-end".into(),
            index: 0,
            dialect: HookDialect::Native,
            runner: HandlerKind::Command,
        },
        class: HandlerClass::Observer,
        priority: 0,
        matcher: Matcher::default(),
        reads: GateReadSet::default(),
        concurrent_group: None,
        read_only_endpoint: None,
        external_precondition: None,
    };
    let mut config = CommandConfig::new(CommandProgram::Argv(vec![
        "/usr/bin/python3".into(),
        "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
    ]));
    config.write_paths = vec!["session-end-canary.txt".into()];
    let registration = CommandRunner::registration_for_event(
        package,
        declaration,
        HookEvent::SessionEnd,
        config,
        None,
    )
    .unwrap();
    (
        source,
        Arc::new(
            demoncoder::plugins::non_tool::NonToolPlan::new(
                HookEvent::SessionEnd,
                vec![registration],
            )
            .unwrap(),
        ),
    )
}

async fn actual_session_end_after_workspace_change(
    replace_original: bool,
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    demoncoder::workflow::runtime::Record,
) {
    use demoncoder::{events::Event, session::Command};

    let roots = tempfile::tempdir().unwrap();
    let a = roots.path().join("a");
    let b = roots.path().join("b");
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    std::fs::write(a.join("session-end-canary.txt"), "").unwrap();
    let (_source, plan) = native_session_end_plan();
    let mut connection: Connection = serde_json::from_value(json!({
        "adapter":"openai-api", "endpoint":"http://127.0.0.1:9",
        "model":"workspace-session-end", "api_key":"synthetic-session-end-key"
    }))
    .unwrap();
    connection.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    connection.access.non_tools.push(plan);
    let (runtime, _) = SharedRuntime::open(&a, &connection, None).unwrap();
    let directory = runtime.directory().unwrap();
    let inner = demoncoder::adapters::builtins()
        .unwrap()
        .open(&connection, &a)
        .unwrap();
    let session = WorkflowSession::new(
        inner,
        connection,
        a.clone(),
        Default::default(),
        runtime.clone(),
        false,
    )
    .unwrap();
    let (command_tx, command_rx) = mpsc::channel(8);
    let (event_tx, mut event_rx) = mpsc::channel(256);
    let events = EventSink::new("workspace-session-end".into(), event_tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let controls = async {
        while !matches!(event_rx.recv().await.unwrap().event, Event::Ready { .. }) {}
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        command_tx
            .send(Command::Submit {
                text: format!("/workspace {}", b.display()),
                reply: reply_tx,
            })
            .await
            .unwrap();
        assert!(reply_rx.await.unwrap().is_ok());
        while !matches!(
            event_rx.recv().await.unwrap().event,
            Event::WorkspaceChanged { .. }
        ) {}
        if replace_original {
            std::fs::rename(&a, roots.path().join("renamed-original-a")).unwrap();
            std::fs::create_dir(&a).unwrap();
        }
        command_tx.send(Command::Shutdown).await.unwrap();
        while event_rx.recv().await.is_some() {}
    };
    let (result, ()) = tokio::join!(
        demoncoder::session::run(Box::new(session), command_rx, events),
        controls
    );
    result.unwrap();
    let record = runtime.record().unwrap();
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
    (roots, a, b, record)
}

async fn peer(
    adapter: &'static str,
    requests: Arc<Mutex<Vec<Value>>>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        for index in 0..4 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 8192];
            let body_start;
            let length;
            loop {
                let size = stream.read(&mut buffer).await.unwrap();
                assert!(size > 0 && request.len() < 2 * 1024 * 1024);
                request.extend_from_slice(&buffer[..size]);
                let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
                    continue;
                };
                let headers = std::str::from_utf8(&request[..end]).unwrap();
                let parsed = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() < end + 4 + parsed {
                    continue;
                }
                body_start = end + 4;
                length = parsed;
                break;
            }
            requests
                .lock()
                .unwrap()
                .push(serde_json::from_slice(&request[body_start..body_start + length]).unwrap());
            let frames = if index == 1 {
                let arguments =
                    json!({"path":"workspace-proof.txt","content":"written-in-selected-root"})
                        .to_string();
                if adapter == "openai-api" {
                    vec![
                        json!({"type":"response.completed","response":{"output":[{"type":"function_call","call_id":"workspace-write","name":"write","arguments":arguments}]}}),
                    ]
                } else {
                    vec![
                        json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
                        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"workspace-write","name":"write","input":{}}}),
                        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":arguments}}),
                        json!({"type":"content_block_stop","index":0}),
                        json!({"type":"message_stop"}),
                    ]
                }
            } else if adapter == "openai-api" {
                vec![json!({"type":"response.completed","response":{"output":[]}})]
            } else {
                vec![
                    json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
                    json!({"type":"message_stop"}),
                ]
            };
            let body = frames
                .into_iter()
                .map(|frame| format!("data: {frame}\n\n"))
                .collect::<String>();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    });
    (endpoint, task)
}

#[tokio::test]
async fn native_api_workspace_changes_preserve_checkpoint_and_execute_in_new_root() {
    for adapter in ["openai-api", "anthropic-api"] {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (endpoint, server) = peer(adapter, requests.clone()).await;
        let roots = tempfile::tempdir().unwrap();
        let a = roots.path().join("a");
        let b = roots.path().join("b with spaces;literal");
        let c = b.join("relative c");
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        std::fs::create_dir(&c).unwrap();
        let connection: Connection = serde_json::from_value(json!({"adapter":adapter,"endpoint":endpoint,"model":"workspace-model","max_output_tokens":64,"api_key":"synthetic-workspace-key"})).unwrap();
        let (runtime, _) = SharedRuntime::open(&a, &connection, None).unwrap();
        let directory = runtime.directory().unwrap();
        let inner = demoncoder::adapters::builtins()
            .unwrap()
            .open(&connection, &a)
            .unwrap();
        let mut session = WorkflowSession::new(
            inner,
            connection,
            a.clone(),
            Default::default(),
            runtime.clone(),
            false,
        )
        .unwrap();
        let (event_tx, mut _event_rx) = mpsc::channel(256);
        let events = EventSink::new(adapter.into(), event_tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let (_command_tx, mut commands) = mpsc::channel(8);
        session
            .open_lifetime(SessionStart::Startup, &events)
            .unwrap();
        assert!(matches!(
            session
                .turn("conversation from A".into(), &mut commands, &events)
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        assert!(matches!(
            session
                .turn(
                    format!("/workspace {}", b.display()),
                    &mut commands,
                    &events
                )
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        let after_first_change = runtime.record().unwrap().operations.len();
        let requests_before_noop = requests.lock().unwrap().len();
        assert!(matches!(
            session
                .turn(
                    format!("/workspace {}", b.display()),
                    &mut commands,
                    &events
                )
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        assert_eq!(
            runtime.record().unwrap().operations.len(),
            after_first_change,
            "{adapter} no-op created a durable owner"
        );
        assert_eq!(
            requests.lock().unwrap().len(),
            requests_before_noop,
            "{adapter} no-op reached the provider"
        );
        assert!(matches!(
            session
                .turn("write the proof in B".into(), &mut commands, &events)
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        assert!(
            b.join("workspace-proof.txt").exists(),
            "{adapter} did not write B; requests={:?}; operations={}",
            requests.lock().unwrap(),
            serde_json::to_string(&runtime.record().unwrap().operations).unwrap()
        );
        assert_eq!(
            std::fs::read_to_string(b.join("workspace-proof.txt")).unwrap(),
            "written-in-selected-root"
        );
        assert!(!a.join("workspace-proof.txt").exists());
        assert!(matches!(
            session
                .turn("/workspace relative c".into(), &mut commands, &events)
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        assert!(matches!(
            session
                .turn("conversation from C".into(), &mut commands, &events)
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        server.await.unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            4,
            "{adapter} request sequence changed: {requests:?}"
        );
        let b_input = serde_json::to_string(&requests[1]).unwrap();
        let c_input = serde_json::to_string(&requests[3]).unwrap();
        assert!(
            b_input.contains("conversation from A") && b_input.contains("write the proof in B"),
            "{adapter} B request did not receive the retained A checkpoint and new B prompt: {b_input}"
        );
        assert!(
            c_input.contains("conversation from A")
                && c_input.contains("write the proof in B")
                && c_input.contains("conversation from C"),
            "{adapter} C request did not receive the retained A/B checkpoint and new C prompt: {c_input}"
        );
        assert!(
            c_input.contains("workspace-write") && c_input.contains("workspace-proof.txt"),
            "{adapter} C checkpoint omitted the B tool call/result evidence: {c_input}"
        );
        assert!(
            !b_input.contains(&format!("/workspace {}", b.display()))
                && !c_input.contains("/workspace relative c"),
            "host control entered provider input: B={b_input}; C={c_input}"
        );
        drop(requests);
        let record = runtime.record().unwrap();
        assert_eq!(record.workspace, c.canonicalize().unwrap());
        assert_eq!(
            record
                .messages
                .iter()
                .filter(|message| message.role == "developer")
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "conversation from A",
                "write the proof in B",
                "conversation from C"
            ]
        );
        assert_eq!(
            record
                .operations
                .iter()
                .filter(|operation| matches!(
                    operation.host_invocation,
                    Some(demoncoder::workflow::runtime::HostInvocation::WorkspaceChange(_))
                ))
                .count(),
            2
        );
        drop(session);
        drop(events);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[tokio::test]
async fn original_native_session_end_command_uses_a_after_change_and_rejects_replaced_a() {
    use demoncoder::{plugins::receipts::NonToolOccurrence, workflow::runtime::HostInvocation};

    let (_roots, a, b, record) = actual_session_end_after_workspace_change(false).await;
    assert_eq!(
        std::fs::read_to_string(a.join("session-end-canary.txt")).unwrap_or_else(|error| {
            panic!(
                "SessionEnd canary missing from A: {error}; operations={}",
                serde_json::to_string(&record.operations).unwrap()
            )
        }),
        a.display().to_string(),
    );
    assert!(!b.join("session-end-canary.txt").exists());
    assert!(
        record.operations.iter().any(|operation| matches!(
            &operation.host_invocation,
            Some(HostInvocation::Lifecycle(receipt))
                if matches!(receipt.facts.subject.occurrence, NonToolOccurrence::SessionEnd { .. })
                    && receipt.hooks.iter().any(|hook| hook.outcome.is_some())
        )),
        "unchanged original A did not execute its retained SessionEnd command"
    );

    let (roots, replacement_a, b, record) = actual_session_end_after_workspace_change(true).await;
    assert!(!replacement_a.join("session-end-canary.txt").exists());
    assert_eq!(
        std::fs::read_to_string(
            roots
                .path()
                .join("renamed-original-a/session-end-canary.txt")
        )
        .unwrap(),
        ""
    );
    assert!(!b.join("session-end-canary.txt").exists());
    assert!(
        !record.operations.iter().any(|operation| matches!(
            &operation.host_invocation,
            Some(HostInvocation::Lifecycle(receipt))
                if matches!(receipt.facts.subject.occurrence, NonToolOccurrence::SessionEnd { .. })
        )),
        "physically replaced A received a SessionEnd effect"
    );
    let Some(HostInvocation::NativeSession(owner)) =
        record.operations.iter().find_map(|operation| {
            operation
                .host_invocation
                .as_ref()
                .filter(|owner| matches!(owner, HostInvocation::NativeSession(_)))
        })
    else {
        panic!("native session owner missing")
    };
    assert!(owner.end.is_some());
    assert!(
        record
            .operations
            .iter()
            .find(|operation| matches!(
                operation.host_invocation,
                Some(HostInvocation::NativeSession(_))
            ))
            .unwrap()
            .complete,
        "physical replacement did not finish bounded native-session cleanup"
    );
    assert!(
        owner
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic
                .contains("original SessionEnd workspace identity was replaced")),
        "physical replacement was not reported truthfully: {:?}",
        owner.diagnostics
    );
}

#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CLAUDE and DEMONCODER_TEST_CODEX pinned installed artifacts"]
async fn installed_external_workspace_change_delivers_truthful_handoff_to_next_prompt() {
    for adapter in ["claude", "codex"] {
        let mut temporary = Some(
            tempfile::Builder::new()
                .prefix(&format!("workspace-change-{adapter}-"))
                .tempdir_in("/home/shawn/demoncoder-check-tmp")
                .unwrap(),
        );
        let root = if std::env::var_os("DEMONCODER_RETAIN_WORKSPACE_CHANGE_TEST").is_some() {
            temporary.take().unwrap().keep()
        } else {
            temporary.as_ref().unwrap().path().to_path_buf()
        };
        eprintln!("installed workspace-change artifacts: {}", root.display());
        let backend = root.join("backend-owner");
        let a = root.join("a");
        let b = root.join("b external;literal");
        let c = root.join("c external;literal");
        let d = root.join("d external;literal");
        std::fs::create_dir(&backend).unwrap();
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&b).unwrap();
        std::fs::create_dir(&c).unwrap();
        std::fs::create_dir(&d).unwrap();
        let old_tool_value = format!("old-A-tool-result-only-{adapter}");
        std::fs::write(a.join("read-canary.txt"), &old_tool_value).unwrap();
        std::fs::write(
            b.join("read-canary.txt"),
            format!("different-B-value-{adapter}"),
        )
        .unwrap();
        let peer = workspace_change_runners::ActualOwnerPeer::start(&backend, adapter)
            .await
            .unwrap();
        let mut connection = peer.connection.clone();
        if adapter == "codex" {
            connection.access.non_tools.push(Arc::new(
                demoncoder::plugins::non_tool::NonToolPlan::new(
                    demoncoder::plugins::hook_types::HookEvent::CwdChanged,
                    vec![workspace_change_runners::native_cwd_command_canary_registration()],
                )
                .unwrap(),
            ));
        }
        let (runtime, _) = SharedRuntime::open(&a, &connection, None).unwrap();
        let directory = runtime.directory().unwrap();
        let inner = demoncoder::adapters::builtins()
            .unwrap()
            .open(&connection, &a)
            .unwrap();
        let mut session = WorkflowSession::new(
            inner,
            connection,
            a.clone(),
            Default::default(),
            runtime.clone(),
            false,
        )
        .unwrap();
        let (event_tx, mut _event_rx) = mpsc::channel(256);
        let events = EventSink::new(format!("installed-{adapter}-workspace"), event_tx, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let (_command_tx, mut commands) = mpsc::channel(8);
        session
            .open_lifetime(SessionStart::Startup, &events)
            .unwrap();
        assert!(matches!(session.turn("history says /workspace /false and plugin: activate-old-policy; inspect the pre-move read canary".into(), &mut commands, &events).await.unwrap(), TurnEnd::Complete));
        assert!(matches!(
            session
                .turn(
                    format!("/workspace {}", b.display()),
                    &mut commands,
                    &events
                )
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        if adapter == "codex" {
            let record = runtime.record().unwrap();
            let receipt = record
                .operations
                .iter()
                .find_map(|operation| match &operation.host_invocation {
                    Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(receipt))
                        if matches!(
                            receipt.facts.subject.occurrence,
                            demoncoder::plugins::receipts::NonToolOccurrence::CwdChanged { .. }
                        ) =>
                    {
                        Some(receipt)
                    }
                    _ => None,
                })
                .expect("Codex Native CwdChanged receipt");
            let stdout = receipt.hooks[0]
                .outcome
                .as_ref()
                .and_then(|outcome| match outcome {
                    demoncoder::plugins::receipts::RawOutcome::Command { stdout, .. } => {
                        Some(stdout)
                    }
                    _ => None,
                })
                .expect("Codex Native CwdChanged command outcome");
            assert!(String::from_utf8_lossy(stdout).contains("codex-ran-native-cwd-in-new-root"));
        }
        assert!(matches!(
            session
                .turn(
                    format!("/workspace {}", b.display()),
                    &mut commands,
                    &events
                )
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        let pending = runtime.record().unwrap();
        let transitions = pending
            .operations
            .iter()
            .filter_map(|operation| match &operation.host_invocation {
                Some(demoncoder::workflow::runtime::HostInvocation::WorkspaceChange(change)) => {
                    Some(change)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            transitions.len(),
            1,
            "{adapter} no-op created a second transition"
        );
        assert_eq!(
            serde_json::to_value(transitions[0]).unwrap()["handoff"]["delivery"],
            "pending",
            "{adapter} no-op consumed the pending handoff"
        );
        let requests_before_help = peer.requests().len();
        assert!(matches!(
            session
                .turn("/workflow-help".into(), &mut commands, &events)
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        assert_eq!(
            peer.requests().len(),
            requests_before_help,
            "{adapter} host control reached the provider"
        );
        let after_help = runtime.record().unwrap();
        let help_transition = after_help
            .operations
            .iter()
            .find_map(|operation| match &operation.host_invocation {
                Some(demoncoder::workflow::runtime::HostInvocation::WorkspaceChange(change)) => {
                    Some(change)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(
            serde_json::to_value(help_transition).unwrap()["handoff"]["delivery"],
            "pending",
            "{adapter} host control consumed the pending handoff"
        );
        assert!(matches!(
            session
                .turn("next explicit prompt in B".into(), &mut commands, &events)
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        peer.assert_backend_workspace(&b);
        let requests = peer.requests();
        let proof = std::fs::read_to_string(b.join("installed-workspace-proof.txt")).unwrap_or_else(|error| panic!("{adapter} did not execute the new-root write: {error}; requests={requests:?}; operations={}", serde_json::to_string(&runtime.record().unwrap().operations).unwrap()));
        assert_eq!(proof, "written-by-installed-owner");
        assert!(
            !a.join("installed-workspace-proof.txt").exists(),
            "{adapter} executed the next-root effect in A"
        );
        assert!(
            requests.len() >= 3,
            "{adapter} did not return a tool continuation: {requests:?}"
        );
        let next = requests
            .iter()
            .filter(|request| {
                let body = serde_json::to_string(&request["body"]).unwrap();
                body.contains("next explicit prompt in B")
                    && !body.contains("installed-workspace-write")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            next.len(),
            1,
            "{adapter} must have one exact next explicit provider request: {requests:?}"
        );
        let next_input = serde_json::to_string(&next[0]["body"]).unwrap();
        assert!(
            next_input.contains("demoncoder_host_retained_conversation_v1")
                && next_input.contains("activate-old-policy")
                && next_input.contains("next explicit prompt in B")
                && next_input.contains("retained_completed_tool_operation")
                && next_input.contains("read-canary.txt")
                && next_input.contains(&old_tool_value)
                && next_input.contains(&a.display().to_string()),
            "{adapter} next request did not receive one truthful attributed handoff and the new prompt: {next_input}"
        );
        assert_eq!(
            next_input
                .matches("demoncoder_host_retained_conversation_v1")
                .count(),
            1,
            "{adapter} nested or replayed the handoff: {next_input}"
        );
        drop(next);
        drop(requests);

        assert!(matches!(
            session
                .turn(
                    format!("/workspace {}", c.display()),
                    &mut commands,
                    &events
                )
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        assert!(matches!(
            session
                .turn(
                    format!("/workspace {}", d.display()),
                    &mut commands,
                    &events
                )
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        assert!(matches!(
            session
                .turn(
                    "first explicit prompt after no-prompt C transition".into(),
                    &mut commands,
                    &events
                )
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        let delivered_provider = runtime
            .record()
            .unwrap()
            .operations
            .iter()
            .rev()
            .find_map(|operation| match &operation.host_invocation {
                Some(demoncoder::workflow::runtime::HostInvocation::WorkspaceChange(change)) => {
                    change.handoff.as_ref()
                }
                _ => None,
            })
            .and_then(|handoff| {
                assert_eq!(
                    serde_json::to_value(handoff).unwrap()["delivery"],
                    "delivered"
                );
                handoff.provider_operation
            })
            .expect("current C→D handoff did not bind its first D provider");
        assert!(matches!(
            session
                .turn(
                    "second explicit prompt after no-prompt C transition".into(),
                    &mut commands,
                    &events
                )
                .await
                .unwrap(),
            TurnEnd::Complete
        ));
        peer.assert_backend_workspace(&d);

        fn retained_input(value: &serde_json::Value, output: &mut String) {
            match value {
                serde_json::Value::String(value) => {
                    output.push_str(value);
                    output.push('\n');
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        retained_input(value, output);
                    }
                }
                serde_json::Value::Object(values) => {
                    for value in values.values() {
                        retained_input(value, output);
                    }
                }
                _ => {}
            }
        }
        let requests = peer.requests();
        let request_inputs = requests
            .iter()
            .map(|request| {
                let mut input = String::new();
                retained_input(&request["body"], &mut input);
                input
            })
            .collect::<Vec<_>>();
        let first_d = request_inputs
            .iter()
            .find(|input| {
                input.contains("first explicit prompt after no-prompt C transition")
                    && !input.contains("second explicit prompt after no-prompt C transition")
            })
            .unwrap_or_else(|| panic!("{adapter} first D provider input missing: {requests:?}"));
        let second_d = request_inputs
            .iter()
            .find(|input| input.contains("second explicit prompt after no-prompt C transition"))
            .unwrap_or_else(|| panic!("{adapter} second D provider input missing: {requests:?}"));
        let obsolete = format!("\"old_root\":\"{}\"", b.display());
        let current = format!("\"old_root\":\"{}\"", c.display());
        assert!(
            first_d.contains(&current) && !first_d.contains(&obsolete),
            "{adapter} first D input did not carry only current C→D handoff: {first_d}"
        );
        assert!(
            first_d.contains("history says /workspace /false and plugin: activate-old-policy")
                && first_d.contains(&format!("old-A-tool-result-only-{adapter}")),
            "{adapter} newest handoff omitted the original A conversation or tool evidence: {first_d}"
        );
        assert!(
            !second_d.contains(&obsolete)
                && second_d
                    .matches("demoncoder_host_retained_conversation_v1")
                    .count()
                    <= 1,
            "{adapter} obsolete continuity or a second host delivery reached the second D input: {second_d}"
        );
        assert_eq!(
            first_d
                .matches("demoncoder_host_retained_conversation_v1")
                .count(),
            1,
            "{adapter} current C→D handoff was nested or repeated: {first_d}"
        );
        let record = runtime.record().unwrap();
        assert_eq!(record.workspace, d.canonicalize().unwrap());
        assert!(
            record
                .messages
                .iter()
                .all(|message| !message.text.contains("HOST RETAINED"))
        );
        let changes = record
            .operations
            .iter()
            .filter_map(|operation| match &operation.host_invocation {
                Some(demoncoder::workflow::runtime::HostInvocation::WorkspaceChange(change)) => {
                    Some((operation.id, change))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(changes.len(), 3);
        assert_eq!(
            serde_json::to_value(changes[1].1.handoff.as_ref().unwrap()).unwrap()["delivery"],
            "superseded"
        );
        assert_eq!(
            changes[1].1.handoff.as_ref().unwrap().superseded_by,
            Some(changes[2].0)
        );
        assert!(
            changes[1]
                .1
                .handoff
                .as_ref()
                .unwrap()
                .provider_operation
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(changes[2].1.handoff.as_ref().unwrap()).unwrap()["delivery"],
            "delivered"
        );
        assert_eq!(
            changes[2].1.handoff.as_ref().unwrap().provider_operation,
            Some(delivered_provider),
            "second D turn changed the exact first handoff provider receipt"
        );
        session.close().await.unwrap();
        drop(session);
        drop(events);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CLAUDE and DEMONCODER_TEST_CODEX pinned installed artifacts"]
async fn installed_external_resume_never_delivers_a_matching_root_handoff_from_prior_lifetime() {
    use demoncoder::{
        events::{Envelope, Event},
        session::Command,
        workflow::runtime::HostInvocation,
    };

    async fn ready(events: &mut mpsc::Receiver<Envelope>) {
        while !matches!(events.recv().await.unwrap().event, Event::Ready { .. }) {}
    }

    async fn submit(
        commands: &mpsc::Sender<Command>,
        events: &mut mpsc::Receiver<Envelope>,
        text: String,
    ) {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        commands
            .send(Command::Submit {
                text,
                reply: reply_tx,
            })
            .await
            .unwrap();
        reply_rx.await.unwrap().unwrap();
        loop {
            if let Event::TurnFinished { status } = events.recv().await.unwrap().event {
                assert_eq!(status, "complete");
                break;
            }
        }
    }

    for adapter in ["claude", "codex"] {
        let mut temporary = Some(
            tempfile::Builder::new()
                .prefix(&format!("workspace-lifetime-collision-{adapter}-"))
                .tempdir_in("/home/shawn/demoncoder-check-tmp")
                .unwrap(),
        );
        let root = if std::env::var_os("DEMONCODER_RETAIN_WORKSPACE_CHANGE_TEST").is_some() {
            temporary.take().unwrap().keep()
        } else {
            temporary.as_ref().unwrap().path().to_path_buf()
        };
        eprintln!(
            "installed workspace lifetime-collision artifacts: {}",
            root.display()
        );
        let backend = root.join("backend-owner");
        let a = root.join("a");
        let x = root.join("x collision;literal");
        let b = root.join("b collision;literal");
        std::fs::create_dir(&backend).unwrap();
        std::fs::create_dir(&a).unwrap();
        std::fs::create_dir(&x).unwrap();
        std::fs::create_dir(&b).unwrap();
        let peer = workspace_change_runners::ActualOwnerPeer::start(&backend, adapter)
            .await
            .unwrap();
        let connection = peer.connection.clone();
        let (runtime, _) = SharedRuntime::open(&a, &connection, None).unwrap();
        let directory = runtime.directory().unwrap();

        let first_inner = demoncoder::adapters::builtins()
            .unwrap()
            .open(&connection, &a)
            .unwrap();
        let first = WorkflowSession::new(
            first_inner,
            connection.clone(),
            a.clone(),
            Default::default(),
            runtime.clone(),
            false,
        )
        .unwrap();
        let (first_command_tx, first_command_rx) = mpsc::channel(8);
        let (first_event_tx, mut first_event_rx) = mpsc::channel(256);
        let first_events = EventSink::new(
            format!("installed-{adapter}-workspace-lifetime-one"),
            first_event_tx,
            None,
        )
        .unwrap()
        .with_runtime(runtime.clone());
        let first_worker = tokio::spawn(demoncoder::session::run(
            Box::new(first),
            first_command_rx,
            first_events,
        ));
        ready(&mut first_event_rx).await;
        submit(
            &first_command_tx,
            &mut first_event_rx,
            format!("/workspace {}", x.display()),
        )
        .await;
        submit(
            &first_command_tx,
            &mut first_event_rx,
            format!("/workspace {}", b.display()),
        )
        .await;
        assert!(
            !backend.join("model-requests.jsonl").exists(),
            "{adapter} L1 control reached provider"
        );
        first_command_tx.send(Command::Shutdown).await.unwrap();
        while first_event_rx.recv().await.is_some() {}
        first_worker.await.unwrap().unwrap();

        let second_inner = demoncoder::adapters::builtins()
            .unwrap()
            .open(&connection, &b)
            .unwrap();
        let second = WorkflowSession::new(
            second_inner,
            connection.clone(),
            b.clone(),
            Default::default(),
            runtime.clone(),
            true,
        )
        .unwrap();
        let (second_command_tx, second_command_rx) = mpsc::channel(8);
        let (second_event_tx, mut second_event_rx) = mpsc::channel(256);
        let second_events = EventSink::new(
            format!("installed-{adapter}-workspace-lifetime-two"),
            second_event_tx,
            None,
        )
        .unwrap()
        .with_runtime(runtime.clone());
        let second_worker = tokio::spawn(demoncoder::session::run(
            Box::new(second),
            second_command_rx,
            second_events,
        ));
        ready(&mut second_event_rx).await;
        submit(
            &second_command_tx,
            &mut second_event_rx,
            "/reconcile unchanged B inspected after resume".into(),
        )
        .await;
        submit(
            &second_command_tx,
            &mut second_event_rx,
            format!("/workspace {}", x.display()),
        )
        .await;
        submit(
            &second_command_tx,
            &mut second_event_rx,
            format!("/workspace {}", b.display()),
        )
        .await;
        assert!(
            !backend.join("model-requests.jsonl").exists(),
            "{adapter} L2 control reached provider"
        );
        submit(
            &second_command_tx,
            &mut second_event_rx,
            "first explicit L2 prompt in B".into(),
        )
        .await;

        let after_first = runtime.record().unwrap();
        let lifetimes = after_first
            .operations
            .iter()
            .filter_map(|operation| {
                matches!(
                    operation.host_invocation,
                    Some(HostInvocation::NativeSession(_))
                )
                .then_some(operation.id)
            })
            .collect::<Vec<_>>();
        assert_eq!(lifetimes.len(), 2);
        let matching = after_first
            .operations
            .iter()
            .filter_map(|operation| match &operation.host_invocation {
                Some(HostInvocation::WorkspaceChange(change))
                    if change.to.path == b.canonicalize().unwrap() && change.to.generation == 2 =>
                {
                    Some((operation.id, change))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 2);
        assert_eq!(matching[0].1.to, matching[1].1.to);
        assert_eq!(matching[0].1.native_session, lifetimes[0]);
        assert_eq!(matching[1].1.native_session, lifetimes[1]);
        assert_eq!(
            serde_json::to_value(matching[0].1.handoff.as_ref().unwrap()).unwrap()["delivery"],
            "pending"
        );
        assert!(
            matching[0]
                .1
                .handoff
                .as_ref()
                .unwrap()
                .provider_operation
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(matching[1].1.handoff.as_ref().unwrap()).unwrap()["delivery"],
            "delivered"
        );
        let current_provider = matching[1]
            .1
            .handoff
            .as_ref()
            .unwrap()
            .provider_operation
            .expect("L2 handoff provider");
        drop(matching);
        drop(after_first);

        submit(
            &second_command_tx,
            &mut second_event_rx,
            "second explicit L2 prompt in B".into(),
        )
        .await;
        peer.assert_backend_workspace(&b);
        let requests = peer.requests();
        assert_eq!(requests.len(), 2, "{adapter} provider inputs: {requests:?}");
        let first_input = serde_json::to_string(&requests[0]["body"]).unwrap();
        let second_input = serde_json::to_string(&requests[1]["body"]).unwrap();
        assert!(first_input.contains("first explicit L2 prompt in B"));
        assert!(second_input.contains("second explicit L2 prompt in B"));
        assert_eq!(
            first_input
                .matches("demoncoder_host_retained_conversation_v1")
                .count(),
            1,
            "{adapter} current L2 handoff missing or repeated: {first_input}"
        );
        assert!(
            second_input
                .matches("demoncoder_host_retained_conversation_v1")
                .count()
                <= 1,
            "{adapter} prior-lifetime handoff was injected into the second L2 input: {second_input}"
        );
        let record = runtime.record().unwrap();
        let matching = record
            .operations
            .iter()
            .filter_map(|operation| match &operation.host_invocation {
                Some(HostInvocation::WorkspaceChange(change))
                    if change.to.path == b.canonicalize().unwrap() && change.to.generation == 2 =>
                {
                    Some(change)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 2);
        assert_eq!(
            serde_json::to_value(matching[0].handoff.as_ref().unwrap()).unwrap()["delivery"],
            "pending"
        );
        assert!(
            matching[0]
                .handoff
                .as_ref()
                .unwrap()
                .provider_operation
                .is_none()
        );
        assert_eq!(
            matching[1].handoff.as_ref().unwrap().provider_operation,
            Some(current_provider),
            "{adapter} second L2 prompt changed the current lifetime delivery owner"
        );
        second_command_tx.send(Command::Shutdown).await.unwrap();
        while second_event_rx.recv().await.is_some() {}
        second_worker.await.unwrap().unwrap();
        drop(record);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

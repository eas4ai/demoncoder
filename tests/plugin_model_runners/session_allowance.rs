use super::*;
use demoncoder::{
    plugins::{non_tool::NonToolPlan, receipts::NonToolOccurrence},
    session::{Command, SessionEnd, SessionStart},
    workflow::{
        runtime::{BudgetRef, HostInvocation},
        workspace::CaptureScope,
    },
};

fn funded(limits: Option<Limits>) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("public"),
        "retained source\nliteral $(touch /tmp/not-executed) `text` $ARGUMENTS",
    )
    .unwrap();
    let connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open_with_session_hooks(
        root.path(),
        &connection,
        None,
        &CaptureScope::default(),
        limits.as_ref(),
    )
    .unwrap();
    let (sender, receiver) = mpsc::channel(256);
    let events = EventSink::new("session allowance fixture".into(), sender, None)
        .unwrap()
        .with_runtime(runtime.clone());
    Fixture {
        root,
        runtime,
        events,
        receiver,
    }
}
fn limits(models: u64, tools: u64) -> Limits {
    Limits {
        seconds: 60,
        model_calls: models,
        tool_calls: tools,
    }
}
fn registration(
    event: HookEvent,
    kind: HandlerKind,
    config: ModelConfig,
    index: u32,
) -> Registration {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        root.path().join(".claude-plugin/plugin.json"),
        r#"{"name":"session-model-fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    let package =
        Arc::new(plugins::inspect(root.path(), &plugins::ImportOptions::default()).unwrap());
    let mut declaration = declaration(kind, HookDialect::Native, index);
    declaration.required_gate = false;
    ModelRunner::registration_for_event(package, declaration, event, config).unwrap()
}
fn native(fixture: &Fixture, registrations: Vec<(HookEvent, Vec<Registration>)>) -> NativeSession {
    let access = AccessPolicy {
        supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
        ..AccessPolicy::default()
    };
    let mut tools = ToolExecutor::with_policy(fixture.root.path(), &access).unwrap();
    for (event, registrations) in registrations {
        tools
            .register_non_tool_plan(Arc::new(NonToolPlan::new(event, registrations).unwrap()))
            .unwrap();
    }
    NativeSession::with_tools(
        Box::new(Source {
            calls: VecDeque::new(),
            results: Arc::new(Mutex::new(vec![])),
        }),
        tools,
    )
}
async fn run(fixture: &mut Fixture, session: NativeSession) {
    let (sender, commands) = mpsc::channel(4);
    let events = fixture.events.clone();
    let controls = async {
        loop {
            if matches!(
                fixture.receiver.recv().await.unwrap().event,
                Event::Ready { .. }
            ) {
                break;
            }
        }
        sender.send(Command::Shutdown).await.unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(8), async {
        tokio::join!(
            demoncoder::session::run(Box::new(session), commands, events),
            controls
        )
    })
    .await
    .expect("whole native startup/end exceeded boundary");
    result.unwrap();
}
fn lifecycle_hooks(record: &Record) -> Vec<&demoncoder::plugins::receipts::HookReceipt> {
    record
        .operations
        .iter()
        .filter_map(|o| match &o.host_invocation {
            Some(HostInvocation::Lifecycle(r)) => Some(r),
            _ => None,
        })
        .flat_map(|r| r.hooks.iter())
        .collect()
}

#[tokio::test]
async fn session_allowance_no_prompt_native_start_end_prompt_and_agent_charge_original_grant() {
    let _lock = FIXTURES.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
            let server = Server::new(adapter, move |index, _| {
                if kind == HandlerKind::Agent && index % 2 == 0 {
                    json!({"tool":{"name":"snapshot_read","arguments":{"path":"public"}}})
                } else {
                    json!({"ok":true})
                }
            });
            let mut fixture = funded(Some(limits(4, 2)));
            let session = native(
                &fixture,
                [HookEvent::SessionStart, HookEvent::SessionEnd]
                    .into_iter()
                    .map(|event| {
                        (
                            event,
                            vec![registration(event, kind, config(&server, adapter), 0)],
                        )
                    })
                    .collect(),
            );
            let deadline = fixture
                .record()
                .session_hook_allowance
                .unwrap()
                .allocation
                .deadline_ms;
            run(&mut fixture, session).await;
            let record = fixture.record();
            let calls = if kind == HandlerKind::Agent { 4 } else { 2 };
            assert_eq!(
                server.count(),
                calls,
                "{adapter} {kind:?}: {:?}",
                record.operations
            );
            assert!(record.allocation.is_none(), "no task was submitted");
            let grant = record.session_hook_allowance.as_ref().unwrap();
            assert_eq!(grant.allocation.model_calls, calls as u64);
            assert_eq!(
                grant.allocation.tool_calls,
                if kind == HandlerKind::Agent { 2 } else { 0 }
            );
            assert_eq!(grant.allocation.usage.reported_input, 11 * calls as u64);
            assert_eq!(grant.allocation.usage.reported_output, 7 * calls as u64);
            assert_eq!(grant.allocation.deadline_ms, deadline);
            assert_eq!(record.backend_invocations, 0);
            assert_eq!(
                serde_json::to_value(grant).unwrap()["backend_invocations"],
                0
            );
            let hooks = lifecycle_hooks(&record);
            assert_eq!(hooks.len(), 2);
            assert!(
                hooks
                    .iter()
                    .all(|h| matches!(h.outcome, Some(RawOutcome::Model { .. }))),
                "{hooks:?}"
            );
            let lifetime = record
                .operations
                .iter()
                .find_map(|o| match &o.host_invocation {
                    Some(HostInvocation::NativeSession(lifetime)) => Some(lifetime),
                    _ => None,
                })
                .unwrap();
            assert_eq!(lifetime.source, SessionStart::Startup);
            assert_eq!(lifetime.end, Some(SessionEnd::Shutdown));
            for operation in &record.operations {
                assert!(
                    matches!(&operation.budget, Some(BudgetRef::SessionHooks {session}) if session == &lifetime.session),
                    "{operation:?}"
                );
            }
            let requests = server.requests.lock().unwrap();
            assert!(prompt(&requests[0]).contains("\"source\":\"startup\""));
            assert!(prompt(&requests[calls / 2]).contains("\"reason\":\"shutdown\""));
            if kind == HandlerKind::Prompt {
                assert!(requests.iter().all(|r| r["tools"] == json!([])));
            }
            let occurrences: Vec<_> = record
                .operations
                .iter()
                .filter_map(|o| match &o.host_invocation {
                    Some(HostInvocation::Lifecycle(r)) => Some(r),
                    _ => None,
                })
                .map(|r| &r.facts.subject.occurrence)
                .collect();
            assert_eq!(
                occurrences,
                vec![
                    &NonToolOccurrence::SessionStart {
                        source: SessionStart::Startup
                    },
                    &NonToolOccurrence::SessionEnd {
                        reason: SessionEnd::Shutdown
                    }
                ]
            );
        }
    }
}

#[tokio::test]
async fn session_allowance_expired_task_cannot_block_prompt_or_snapshot_tools() {
    let _lock = FIXTURES.lock().await;
    let server = Server::new("openai-api", |index, _| {
        if index == 0 {
            json!({"tool":{"name":"snapshot_read","arguments":{"path":"public"}}})
        } else {
            json!({"ok":true})
        }
    });
    let mut fixture = funded(Some(limits(2, 1)));
    fixture
        .runtime
        .allocate(
            Limits {
                seconds: 1,
                model_calls: 1,
                tool_calls: 1,
            },
            None,
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let session = native(
        &fixture,
        vec![(
            HookEvent::SessionStart,
            vec![registration(
                HookEvent::SessionStart,
                HandlerKind::Agent,
                config(&server, "openai-api"),
                0,
            )],
        )],
    );
    run(&mut fixture, session).await;
    assert_eq!(
        server.count(),
        2,
        "{:?}",
        lifecycle_hooks(&fixture.record())
    );
    let record = fixture.record();
    assert_eq!(record.allocation.as_ref().unwrap().model_calls, 0);
    assert_eq!(record.allocation.as_ref().unwrap().tool_calls, 0);
    let grant = record.session_hook_allowance.unwrap();
    assert_eq!(grant.allocation.model_calls, 2);
    assert_eq!(grant.allocation.tool_calls, 1);
    let tool = record
        .operations
        .iter()
        .find_map(|o| o.tool_receipt.as_ref())
        .unwrap();
    assert!(tool.effect_started);
}

#[tokio::test]
async fn session_allowance_external_backends_charge_session_and_reap_owned_processes() {
    use std::os::unix::fs::PermissionsExt;
    let _lock = FIXTURES.lock().await;
    for adapter in ["claude", "codex"] {
        for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
            let mut fixture = funded(Some(limits(2, 2)));
            let backend = tempfile::tempdir().unwrap();
            let binary = backend.path().join("backend");
            let records = backend.path().join("requests.jsonl");
            std::fs::write(&binary, include_str!("../plugin_model_backend.py")).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::write(binary.with_extension("json"),json!({"agent":kind==HandlerKind::Agent,"workspace":fixture.root.path(),"records":records,"behavior":"allow"}).to_string()).unwrap();
            let connection: Connection = serde_json::from_value(
                json!({"adapter":adapter,"model":"explicit-hook-model","binary":binary}),
            )
            .unwrap();
            let session = native(
                &fixture,
                [HookEvent::SessionStart, HookEvent::SessionEnd]
                    .into_iter()
                    .map(|event| {
                        (
                            event,
                            vec![registration(
                                event,
                                kind,
                                ModelConfig::new(
                                    connection.clone(),
                                    "Inspect original session evidence.".into(),
                                ),
                                0,
                            )],
                        )
                    })
                    .collect(),
            );
            run(&mut fixture, session).await;
            let messages = std::fs::read_to_string(&records)
                .unwrap_or_default()
                .lines()
                .map(|l| serde_json::from_str::<Value>(l).unwrap())
                .collect::<Vec<_>>();
            let calls = messages
                .iter()
                .filter(|v| {
                    v["message"]["type"] == "user" || v["message"]["method"] == "turn/start"
                })
                .count();
            assert_eq!(
                calls,
                2,
                "{adapter} {kind:?}: {:?}",
                lifecycle_hooks(&fixture.record())
            );
            let record = fixture.record();
            let grant = record.session_hook_allowance.as_ref().unwrap();
            assert_eq!(
                record.backend_invocations, 0,
                "session hooks must not debit task/delegation backend count"
            );
            assert_eq!(
                serde_json::to_value(grant).unwrap()["backend_invocations"],
                2
            );
            assert_eq!(grant.allocation.model_calls, 2);
            assert_eq!(
                grant.allocation.tool_calls,
                if kind == HandlerKind::Agent { 2 } else { 0 }
            );
            assert_eq!(grant.allocation.usage.reported_input, 26);
            assert_eq!(grant.allocation.usage.reported_output, 18);
            assert!(grant.allocation.usage.unknown_cost);
            assert!(
                lifecycle_hooks(&record)
                    .iter()
                    .all(|h| matches!(h.outcome, Some(RawOutcome::Model { .. })))
            );
            assert!(
                messages
                    .iter()
                    .filter_map(|v| v["cwd"].as_str())
                    .all(|cwd| !std::path::Path::new(cwd).exists())
            );
            for pid in messages.iter().filter_map(|v| v["pid"].as_u64()) {
                assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
            }
        }
    }
}

#[tokio::test]
async fn session_allowance_missing_empty_expired_or_held_sends_no_request_with_funded_task() {
    let _lock = FIXTURES.lock().await;
    for case in ["missing", "empty", "expired", "held"] {
        for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
            let server = Server::new("openai-api", |_, _| json!({"ok":true}));
            let mut grant = limits(if case == "empty" { 0 } else { 2 }, 2);
            if case == "expired" {
                grant.seconds = 1;
            }
            let mut fixture = funded((case != "missing").then_some(grant));
            fixture.runtime.allocate(limits(8, 8), None).unwrap();
            if case == "expired" {
                tokio::time::sleep(Duration::from_millis(1100)).await;
            }
            if case == "held" {
                fixture.runtime.hold().unwrap();
            }
            let session = native(
                &fixture,
                [HookEvent::SessionStart, HookEvent::SessionEnd]
                    .into_iter()
                    .map(|event| {
                        (
                            event,
                            vec![registration(event, kind, config(&server, "openai-api"), 0)],
                        )
                    })
                    .collect(),
            );
            run(&mut fixture, session).await;
            assert_eq!(server.count(), 0, "{case} {kind:?}");
            let record = fixture.record();
            assert_eq!(record.allocation.as_ref().unwrap().model_calls, 0);
            assert_eq!(record.allocation.as_ref().unwrap().tool_calls, 0);
            assert!(record.operations.iter().all(|o| o.tool_receipt.is_none()));
            if let Some(grant) = record.session_hook_allowance {
                assert_eq!(grant.allocation.model_calls, 0);
                assert_eq!(grant.allocation.tool_calls, 0);
            }
        }
    }
}

#[tokio::test]
async fn session_allowance_multiple_hooks_share_last_slot_and_end_cannot_replenish_it() {
    let _lock = FIXTURES.lock().await;
    let server = Server::new("openai-api", |_, _| json!({"ok":true}));
    let mut fixture = funded(Some(limits(1, 0)));
    let mut registrations = vec![];
    for index in 0..2 {
        let reg = registration(
            HookEvent::SessionStart,
            HandlerKind::Prompt,
            config(&server, "openai-api"),
            index,
        );
        registrations.push(reg);
    }
    let end = registration(
        HookEvent::SessionEnd,
        HandlerKind::Prompt,
        config(&server, "openai-api"),
        0,
    );
    let session = native(
        &fixture,
        vec![
            (HookEvent::SessionStart, registrations),
            (HookEvent::SessionEnd, vec![end]),
        ],
    );
    run(&mut fixture, session).await;
    let record = fixture.record();
    assert_eq!(server.count(), 1);
    assert_eq!(
        record
            .session_hook_allowance
            .as_ref()
            .unwrap()
            .allocation
            .model_calls,
        1
    );
    assert_eq!(
        lifecycle_hooks(&record)
            .iter()
            .filter(|h| matches!(h.outcome, Some(RawOutcome::Model { .. })))
            .count(),
        1
    );
    assert_eq!(lifecycle_hooks(&record).len(), 3);
}

#[tokio::test]
async fn session_allowance_zero_tools_blocks_agent_effect_and_prompt_cannot_use_tools() {
    let _lock = FIXTURES.lock().await;
    for kind in [HandlerKind::Agent, HandlerKind::Prompt] {
        let server = Server::new("openai-api", |index, _| {
            if index == 0 {
                json!({"tool":{"name":"snapshot_read","arguments":{"path":"public"}}})
            } else {
                json!({"ok":true})
            }
        });
        let mut fixture = funded(Some(limits(
            2,
            if kind == HandlerKind::Agent { 0 } else { 2 },
        )));
        let session = native(
            &fixture,
            vec![(
                HookEvent::SessionStart,
                vec![registration(
                    HookEvent::SessionStart,
                    kind,
                    config(&server, "openai-api"),
                    0,
                )],
            )],
        );
        run(&mut fixture, session).await;
        let record = fixture.record();
        assert!(server.count() > 0);
        assert_eq!(
            record
                .session_hook_allowance
                .as_ref()
                .unwrap()
                .allocation
                .tool_calls,
            0
        );
        assert!(
            record
                .operations
                .iter()
                .filter_map(|o| o.tool_receipt.as_ref())
                .all(|r| !r.effect_started)
        );
        if kind == HandlerKind::Prompt {
            assert!(
                lifecycle_hooks(&record)
                    .iter()
                    .all(|h| !matches!(h.outcome, Some(RawOutcome::Model { .. })))
            );
        }
    }
}

#[tokio::test]
async fn session_allowance_invalid_or_denied_terminal_verdict_does_not_veto_shutdown() {
    let _lock = FIXTURES.lock().await;
    for value in [
        json!({"ok":false,"reason":"cannot cancel host shutdown"}),
        json!({"raw":"invalid verdict"}),
    ] {
        let server = Server::new("openai-api", move |_, _| value.clone());
        let mut fixture = funded(Some(limits(2, 0)));
        let session = native(
            &fixture,
            [HookEvent::SessionStart, HookEvent::SessionEnd]
                .into_iter()
                .map(|event| {
                    (
                        event,
                        vec![registration(
                            event,
                            HandlerKind::Prompt,
                            config(&server, "openai-api"),
                            0,
                        )],
                    )
                })
                .collect(),
        );
        run(&mut fixture, session).await;
        assert_eq!(server.count(), 2);
        let record = fixture.record();
        assert!(record.task.is_none());
        for receipt in record
            .operations
            .iter()
            .filter_map(|o| match &o.host_invocation {
                Some(HostInvocation::Lifecycle(r)) => Some(r),
                _ => None,
            })
        {
            assert!(receipt.settled && receipt.hold.is_none());
            assert!(!receipt.diagnostics.is_empty());
        }
    }
}

#[tokio::test]
async fn session_allowance_native_timeout_settles_original_unknown_usage() {
    let _lock = FIXTURES.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        let server = Server::new(adapter, |_, _| {
            thread::sleep(Duration::from_millis(300));
            json!({"ok":true})
        });
        let mut fixture = funded(Some(limits(1, 0)));
        let mut config = config(&server, adapter);
        config.timeout_ms = 100;
        let session = native(
            &fixture,
            vec![(
                HookEvent::SessionEnd,
                vec![registration(
                    HookEvent::SessionEnd,
                    HandlerKind::Prompt,
                    config,
                    0,
                )],
            )],
        );
        let began = std::time::Instant::now();
        run(&mut fixture, session).await;
        assert!(began.elapsed() < Duration::from_secs(5));
        assert_eq!(server.count(), 1);
        let record = fixture.record();
        let grant = record.session_hook_allowance.as_ref().unwrap();
        assert!(grant.allocation.usage.unknown_input && grant.allocation.usage.unknown_output);
        assert!(
            record
                .operations
                .iter()
                .filter(|o| matches!(o.host_invocation, Some(HostInvocation::Model)))
                .all(|o| o.complete)
        );
        assert!(
            lifecycle_hooks(&record)
                .iter()
                .all(|h| matches!(h.outcome, Some(RawOutcome::Failure { .. })))
        );
    }
}

#[tokio::test]
async fn session_allowance_task_changes_during_native_request_keep_snapshot_and_funding() {
    use demoncoder::workflow::{state::Task, workspace};
    let _lock = FIXTURES.lock().await;
    for change in ["stopped", "accepted", "replaced", "archived"] {
        let mut fixture = funded(Some(limits(2, 1)));
        fixture.runtime.allocate(limits(8, 8), None).unwrap();
        let task = Task::new(
            1,
            "original task".into(),
            vec![],
            workspace::capture(fixture.root.path()).unwrap(),
            1,
        )
        .unwrap();
        fixture
            .runtime
            .save_task(&Some(task.clone()), 2, None)
            .unwrap();
        let runtime = fixture.runtime.clone();
        let root = fixture.root.path().to_owned();
        let deadline = fixture
            .record()
            .session_hook_allowance
            .unwrap()
            .allocation
            .deadline_ms;
        let server = Server::new("openai-api", move |index, request| {
            if index == 0 {
                let mut task = task.clone();
                match change {
                    "stopped" => task.stopped = true,
                    "accepted" => task.accepted = Some("accepted-evidence".into()),
                    "replaced" => {
                        task.id = 2;
                        runtime.allocate(limits(16, 16), None).unwrap();
                    }
                    "archived" => runtime.archive().unwrap(),
                    _ => unreachable!(),
                }
                if change != "archived" {
                    runtime.save_task(&Some(task), 3, None).unwrap();
                }
                std::fs::write(root.join("public"), "MUTATED-LIVE-CONTENT").unwrap();
                json!({"tool":{"name":"snapshot_read","arguments":{"path":"public"}}})
            } else {
                assert!(request.to_string().contains("retained source"));
                assert!(!request.to_string().contains("MUTATED-LIVE-CONTENT"));
                json!({"ok":true})
            }
        });
        let session = native(
            &fixture,
            vec![(
                HookEvent::SessionStart,
                vec![registration(
                    HookEvent::SessionStart,
                    HandlerKind::Agent,
                    config(&server, "openai-api"),
                    0,
                )],
            )],
        );
        run(&mut fixture, session).await;
        assert_eq!(
            server.count(),
            2,
            "{change}: {:?}",
            lifecycle_hooks(&fixture.record())
        );
        let record = fixture.record();
        let grant = record.session_hook_allowance.unwrap();
        assert_eq!(grant.allocation.model_calls, 2);
        assert_eq!(grant.allocation.tool_calls, 1);
        assert_eq!(grant.allocation.usage.reported_input, 22);
        assert_eq!(grant.allocation.deadline_ms, deadline);
        assert!(record.allocation.is_none_or(|a| a.model_calls == 0
            && a.tool_calls == 0
            && a.usage.reported_input == 0));
    }
}

#[tokio::test]
async fn session_allowance_response_delivery_rechecks_hold_after_request() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(Some(limits(1, 0)));
    let runtime = fixture.runtime.clone();
    let server = Server::new("openai-api", move |_, _| {
        runtime.hold().unwrap();
        json!({"ok":true})
    });
    let session = native(
        &fixture,
        vec![(
            HookEvent::SessionStart,
            vec![registration(
                HookEvent::SessionStart,
                HandlerKind::Prompt,
                config(&server, "openai-api"),
                0,
            )],
        )],
    );
    run(&mut fixture, session).await;
    assert_eq!(server.count(), 1);
    let record = fixture.record();
    assert!(record.recovery_pending);
    assert!(
        lifecycle_hooks(&record)
            .iter()
            .all(|h| !matches!(h.outcome, Some(RawOutcome::Model { .. }))),
        "held owner delivered model verdict: {:?}",
        lifecycle_hooks(&record)
    );
}

#[tokio::test]
async fn session_allowance_cancelled_external_hook_reaps_and_settles_while_retaining_lease() {
    use std::os::unix::fs::PermissionsExt;
    let _lock = FIXTURES.lock().await;
    for adapter in ["claude", "codex"] {
        let fixture = funded(Some(limits(1, 0)));
        let backend = tempfile::tempdir().unwrap();
        let binary = backend.path().join("backend");
        let records = backend.path().join("requests.jsonl");
        std::fs::write(&binary, include_str!("../plugin_model_backend.py")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(binary.with_extension("json"),json!({"agent":false,"workspace":fixture.root.path(),"records":records,"behavior":"timeout"}).to_string()).unwrap();
        let connection = serde_json::from_value(
            json!({"adapter":adapter,"model":"explicit-hook-model","binary":binary}),
        )
        .unwrap();
        let session = native(
            &fixture,
            vec![(
                HookEvent::SessionStart,
                vec![registration(
                    HookEvent::SessionStart,
                    HandlerKind::Prompt,
                    ModelConfig::new(connection, "Inspect original session evidence.".into()),
                    0,
                )],
            )],
        );
        let (_sender, commands) = mpsc::channel(4);
        let events = fixture.events.clone();
        let task = tokio::spawn(demoncoder::session::run(
            Box::new(session),
            commands,
            events,
        ));
        let messages = tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                let messages = std::fs::read_to_string(&records)
                    .unwrap_or_default()
                    .lines()
                    .map(|l| serde_json::from_str::<Value>(l).unwrap())
                    .collect::<Vec<_>>();
                if messages.iter().any(|v| {
                    v["message"]["type"] == "user" || v["message"]["method"] == "turn/start"
                }) {
                    break messages;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("external fixture never received admitted request");
        let began = std::time::Instant::now();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(4), async {
            loop {
                let settled = fixture
                    .record()
                    .operations
                    .iter()
                    .filter(|o| matches!(o.host_invocation, Some(HostInvocation::Backend)))
                    .all(|o| o.complete);
                let reaped = messages
                    .iter()
                    .filter_map(|v| v["pid"].as_u64())
                    .all(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists());
                let removed = messages
                    .iter()
                    .filter_map(|v| v["cwd"].as_str())
                    .all(|cwd| !std::path::Path::new(cwd).exists());
                if settled && reaped && removed {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("cancelled model runner leaked owned process, directory or unsettled usage");
        assert!(began.elapsed() < Duration::from_secs(4));
        let record = fixture.record();
        let grant = record.session_hook_allowance.unwrap();
        assert_eq!(grant.allocation.model_calls, 1);
        assert_eq!(grant.backend_invocations, 1);
        assert!(grant.allocation.usage.unknown_input && grant.allocation.usage.unknown_output);
        assert_eq!(record.backend_invocations, 0);
    }
}

#[tokio::test]
async fn session_allowance_external_missing_or_empty_grant_never_admits_backend_request() {
    use std::os::unix::fs::PermissionsExt;
    let _lock = FIXTURES.lock().await;
    for adapter in ["claude", "codex"] {
        for configured in [false, true] {
            let mut fixture = funded(configured.then(|| limits(0, 2)));
            fixture.runtime.allocate(limits(8, 8), None).unwrap();
            let backend = tempfile::tempdir().unwrap();
            let binary = backend.path().join("backend");
            let records = backend.path().join("requests.jsonl");
            std::fs::write(&binary, include_str!("../plugin_model_backend.py")).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
            std::fs::write(binary.with_extension("json"),json!({"agent":false,"workspace":fixture.root.path(),"records":records,"behavior":"allow"}).to_string()).unwrap();
            let connection: Connection = serde_json::from_value(
                json!({"adapter":adapter,"model":"explicit-hook-model","binary":binary}),
            )
            .unwrap();
            let session = native(
                &fixture,
                [HookEvent::SessionStart, HookEvent::SessionEnd]
                    .into_iter()
                    .map(|event| {
                        (
                            event,
                            vec![registration(
                                event,
                                HandlerKind::Prompt,
                                ModelConfig::new(
                                    connection.clone(),
                                    "Inspect original session evidence.".into(),
                                ),
                                0,
                            )],
                        )
                    })
                    .collect(),
            );
            run(&mut fixture, session).await;
            let messages = std::fs::read_to_string(&records)
                .unwrap_or_default()
                .lines()
                .map(|l| serde_json::from_str::<Value>(l).unwrap())
                .collect::<Vec<_>>();
            assert!(
                !messages
                    .iter()
                    .any(|v| v["message"]["type"] == "user"
                        || v["message"]["method"] == "turn/start"),
                "{adapter} grant={configured}: {messages:?}"
            );
            let record = fixture.record();
            assert_eq!(record.backend_invocations, 0);
            assert_eq!(record.allocation.as_ref().unwrap().model_calls, 0);
            assert!(!record.operations.iter().any(|o| matches!(
                o.host_invocation,
                Some(HostInvocation::Backend | HostInvocation::Model)
            )));
            if let Some(grant) = record.session_hook_allowance {
                assert_eq!(grant.backend_invocations, 0);
                assert_eq!(grant.allocation.model_calls, 0);
            }
            for pid in messages.iter().filter_map(|v| v["pid"].as_u64()) {
                assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
            }
            assert!(
                messages
                    .iter()
                    .filter_map(|v| v["cwd"].as_str())
                    .all(|cwd| !std::path::Path::new(cwd).exists())
            );
        }
    }
}

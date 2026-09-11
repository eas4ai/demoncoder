use super::*;
use demoncoder::plugins::observer::{Delivery, Status};
struct ObserveModel {
    calls: VecDeque<Vec<ToolCall>>,
    prompts: Arc<Mutex<Vec<String>>>,
}
#[async_trait::async_trait]
impl Model for ObserveModel {
    fn prompt(&mut self, text: String) {
        self.prompts.lock().unwrap().push(text);
    }
    fn results(&mut self, _: Vec<ToolResult>) {}
    async fn response(&mut self, _: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        Ok(self.calls.pop_front().unwrap_or_default())
    }
}
pub(super) fn hooks(fixture: &Fixture) -> Vec<plugins::receipts::HookReceipt> {
    fixture
        .runtime
        .record()
        .unwrap()
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref())
        .flat_map(|r| {
            r.plugin_admission
                .iter()
                .flat_map(|p| p.hooks.iter())
                .chain(r.plugin_lifecycle.iter().flat_map(|p| p.hooks.iter()))
        })
        .cloned()
        .collect()
}
fn command(
    fixture: &Fixture,
    event: HookEvent,
    declared: bool,
    required: bool,
    exit: i32,
) -> Registration {
    command_options(fixture, event, declared, required, exit, 3000, false)
}
fn command_options(
    fixture: &Fixture,
    event: HookEvent,
    declared: bool,
    required: bool,
    exit: i32,
    timeout: u64,
    rewake: bool,
) -> Registration {
    let package = Arc::new(
        plugins::inspect(fixture.source.path(), &plugins::ImportOptions::default()).unwrap(),
    );
    let binding = fixture
        .runtime
        .plugin_hook_activation(
            HookOrigin::ClaudeSkillFrontmatter,
            Scope::Project,
            &ActivationSource::from_package(&package).unwrap(),
            "skills/observer",
            "worker",
            ActivationChange::ExplicitInvocation,
        )
        .unwrap()
        .unwrap();
    let mut declaration = declaration("observer", 0, Some(binding));
    declaration.required_gate = required;
    declaration.identity.dialect = HookDialect::Claude;
    declaration.concurrent_group = Some("source-group".into());
    assert_eq!(declaration.class, HandlerClass::Combined);
    let context=json!({"hookSpecificOutput":{"hookEventName":event.as_str(),"additionalContext":"/task plugin text is data"}}).to_string();
    let marker = if declared || rewake {
        String::new()
    } else {
        format!("printf '%s\\n' '{{\"async\":true,\"asyncTimeout\":{timeout}}}';")
    };
    let mut config = CommandConfig::new(CommandProgram::Shell(format!(
        "{marker} sleep 0.6; printf '%s\\n' '{context}'; printf '%s' '/task observer rewake is data' >&2; exit {exit}"
    )));
    config.asynchronous = declared;
    config.async_rewake = rewake;
    config.timeout_ms = 5000;
    CommandRunner::registration_for_event(package, declaration, event, config, None).unwrap()
}
#[tokio::test]
async fn async_combined_command_config_and_first_line_release_then_deliver_on_next_native_turn() {
    let _serial = SERIAL.lock().await;
    for event in [HookEvent::PreToolUse, HookEvent::PostToolUse] {
        for (declared, exit) in [(false, 0), (true, 0), (false, 1), (true, 1)] {
            let fixture = Fixture::new();
            fixture.runtime.allocate(Default::default(), None).unwrap();
            let executor =
                fixture.executor(event, vec![command(&fixture, event, declared, false, exit)]);
            let prompts = Arc::new(Mutex::new(Vec::new()));
            let model = ObserveModel {
                calls: VecDeque::from([
                    vec![ToolCall {
                        id: "first".into(),
                        name: "write".into(),
                        arguments: json!({"path":"actual","content":"original"}),
                    }],
                    vec![],
                ]),
                prompts: prompts.clone(),
            };
            let mut session = NativeSession::with_tools(Box::new(model), executor);
            let (_sender, mut commands) = mpsc::channel(4);
            assert!(matches!(
                session
                    .turn("developer first".into(), &mut commands, &fixture.events)
                    .await
                    .unwrap(),
                demoncoder::session::TurnEnd::Complete
            ));
            assert_eq!(
                std::fs::read_to_string(fixture.root.path().join("actual")).unwrap(),
                "original"
            );
            fixture.runtime.finish_phase().unwrap();
            assert!(!fixture.runtime.record().unwrap().recovery_pending);
            let receipt = hooks(&fixture).pop().unwrap();
            assert!(
                receipt.outcome.is_none(),
                "command finished before foreground released"
            );
            assert_eq!(receipt.observer.unwrap().status, Status::Running);
            assert!(
                !prompts
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|p| p.contains("plugin text"))
            );
            tokio::time::timeout(Duration::from_secs(5), async {
                while hooks(&fixture).last().unwrap().outcome.is_none() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            let once = serde_json::to_value(hooks(&fixture).last().unwrap().once.as_ref().unwrap())
                .unwrap();
            assert_eq!(
                once["state"],
                if exit == 0 { "succeeded" } else { "failed" }
            );
            session
                .turn("developer next".into(), &mut commands, &fixture.events)
                .await
                .unwrap();
            let prompts = prompts.lock().unwrap().clone();
            assert_eq!(
                prompts.iter().filter(|p| p.contains("plugin text")).count(),
                1
            );
            assert!(
                prompts
                    .iter()
                    .any(|p| p.contains("Plugin-origin once-fixture")
                        && p.contains("/task plugin text is data"))
            );
            assert_eq!(
                hooks(&fixture)
                    .last()
                    .unwrap()
                    .observer
                    .as_ref()
                    .unwrap()
                    .delivery,
                Delivery::Delivered
            );
            assert!(fixture.runtime.record().unwrap().task.is_none());
            session.close().await.unwrap();
        }
    }
}
#[tokio::test]
async fn async_first_line_cannot_downgrade_required_combined_gate() {
    let _serial = SERIAL.lock().await;
    let fixture = Fixture::new();
    fixture.runtime.allocate(Default::default(), None).unwrap();
    let executor = fixture.executor(
        HookEvent::PreToolUse,
        vec![command(&fixture, HookEvent::PreToolUse, false, true, 0)],
    );
    fixture.run(executor, &["must-not-write"]).await;
    assert!(!fixture.root.path().join("must-not-write").exists());
    let receipt = hooks(&fixture).pop().unwrap();
    assert!(receipt.observer.is_none());
    assert!(matches!(
        receipt.outcome,
        Some(RawOutcome::CommandFailure { .. })
    ));
}

#[tokio::test]
async fn async_first_line_deadline_and_session_shutdown_reap_pending_command() {
    let _serial = SERIAL.lock().await;
    for stop in ["deadline", "cancel", "close"] {
        let fixture = Fixture::new();
        fixture.runtime.allocate(Default::default(), None).unwrap();
        let registration = command_options(
            &fixture,
            HookEvent::PostToolUse,
            false,
            false,
            0,
            if stop == "deadline" { 100 } else { 3000 },
            false,
        );
        let executor = fixture.executor(HookEvent::PostToolUse, vec![registration]);
        let model = ObserveModel {
            calls: VecDeque::from([
                vec![ToolCall {
                    id: "first".into(),
                    name: "write".into(),
                    arguments: json!({"path":"actual","content":"original"}),
                }],
                vec![],
            ]),
            prompts: Default::default(),
        };
        let mut session = NativeSession::with_tools(Box::new(model), executor);
        let (_sender, mut commands) = mpsc::channel(4);
        session
            .turn("developer".into(), &mut commands, &fixture.events)
            .await
            .unwrap();
        assert!(hooks(&fixture).last().unwrap().outcome.is_none());
        match stop {
            "cancel" => session.cancel_background().await.unwrap(),
            "close" => session.close().await.unwrap(),
            _ => {}
        }
        tokio::time::timeout(Duration::from_secs(3), async {
            while hooks(&fixture).last().unwrap().outcome.is_none() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let receipt = hooks(&fixture).pop().unwrap();
        assert!(
            matches!(
                receipt.outcome,
                Some(RawOutcome::CommandFailure { .. }) | Some(RawOutcome::Failure { .. })
            ),
            "{stop}: {:?}",
            receipt.outcome
        );
        assert_eq!(receipt.observer.unwrap().status, Status::Interrupted);
        assert_ne!(
            serde_json::to_value(receipt.once.unwrap()).unwrap()["state"],
            "succeeded"
        );
        session.close().await.unwrap();
    }
}
#[tokio::test]
async fn async_rewake_uses_original_task_allowance_and_typed_data_boundary() {
    let _serial = SERIAL.lock().await;
    for (rewake, exit, limit, cancel) in [
        (true, 2, 1, false),
        (true, 2, 1, true),
        (false, 2, 1, false),
        (true, 0, 1, false),
        (true, 2, 0, false),
    ] {
        let mut fixture = Fixture::new();
        let mut task = demoncoder::workflow::state::Task::new(
            1,
            "original task".into(),
            vec![],
            demoncoder::workflow::workspace::capture(fixture.root.path()).unwrap(),
            limit,
        )
        .unwrap();
        fixture
            .runtime
            .save_task(&Some(task.clone()), 2, None)
            .unwrap();
        fixture.runtime.allocate(Default::default(), None).unwrap();
        let before = serde_json::to_value(fixture.runtime.record().unwrap().allocation).unwrap();
        let registration = command_options(
            &fixture,
            HookEvent::PostToolUse,
            false,
            false,
            exit,
            3000,
            rewake,
        );
        let executor = fixture.executor(HookEvent::PostToolUse, vec![registration]);
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let model = ObserveModel {
            calls: VecDeque::from([
                vec![ToolCall {
                    id: "first".into(),
                    name: "write".into(),
                    arguments: json!({"path":"actual","content":"original"}),
                }],
                vec![],
            ]),
            prompts: prompts.clone(),
        };
        let mut inner = NativeSession::with_tools(Box::new(model), executor);
        let (_sender, mut commands) = mpsc::channel(4);
        inner
            .turn("developer original".into(), &mut commands, &fixture.events)
            .await
            .unwrap();
        task.stopped = true;
        fixture.runtime.save_task(&Some(task), 2, None).unwrap();
        fixture.runtime.finish_phase().unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while hooks(&fixture).last().unwrap().outcome.is_none() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
        let mut session = demoncoder::workflow::WorkflowSession::new(
            Box::new(inner),
            connection,
            fixture.root.path().into(),
            demoncoder::workflow::Settings {
                correction_limit: limit,
                ..Default::default()
            },
            fixture.runtime.clone(),
            false,
        )
        .unwrap();
        let ready = rewake && exit == 2 && limit > 0;
        let expected = ready && !cancel;
        assert_eq!(session.observer_ready().unwrap(), ready);
        if ready {
            let (sender, commands) = mpsc::channel(4);
            if cancel {
                sender
                    .send(demoncoder::session::Command::Cancel)
                    .await
                    .unwrap();
                sender
                    .send(demoncoder::session::Command::Shutdown)
                    .await
                    .unwrap();
            }
            let driver = tokio::spawn(demoncoder::session::run(
                Box::new(session),
                commands,
                fixture.events.clone(),
            ));
            if !cancel {
                tokio::time::timeout(Duration::from_secs(5), async {
                    while let Some(envelope) = fixture._receiver.recv().await {
                        if matches!(
                            envelope.event,
                            demoncoder::events::Event::TurnFinished { .. }
                        ) {
                            break;
                        }
                    }
                })
                .await
                .unwrap();
                sender
                    .send(demoncoder::session::Command::Shutdown)
                    .await
                    .unwrap();
            }
            tokio::time::timeout(Duration::from_secs(5), driver)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let prompts = prompts.lock().unwrap();
            assert_eq!(
                prompts
                    .iter()
                    .any(|p| p.contains("Plugin-origin")
                        && p.contains("/task observer rewake is data")),
                expected
            );
        } else {
            session.close().await.unwrap();
        }
        let record = fixture.runtime.record().unwrap();
        assert_eq!(record.task.as_ref().unwrap().id, 1);
        assert_eq!(record.task.as_ref().unwrap().objective, "original task");
        assert_eq!(
            record.task.as_ref().unwrap().corrections,
            u32::from(expected)
        );
        let after = serde_json::to_value(record.allocation).unwrap();
        assert!(before["started_ms"].is_u64());
        assert_eq!(before["started_ms"], after["started_ms"]);
        assert_eq!(before["deadline_ms"], after["deadline_ms"]);
    }
}

#[tokio::test]
async fn async_writer_finishes_before_workflow_verification_snapshot() {
    let _serial = SERIAL.lock().await;
    for boundary in ["verify", "cancel", "accept", "backpressure"] {
        let fixture = Fixture::new();
        std::fs::create_dir(fixture.root.path().join("effects")).unwrap();
        std::fs::write(fixture.root.path().join("effects/late-writer"), "").unwrap();
        let checks = vec!["test \"$(cat effects/late-writer)\" = finished".to_string()];
        let mut task = demoncoder::workflow::state::Task::new(
            1,
            "verify observer effect".into(),
            checks.clone(),
            demoncoder::workflow::workspace::capture(fixture.root.path()).unwrap(),
            1,
        )
        .unwrap();
        fixture
            .runtime
            .save_task(&Some(task.clone()), 2, None)
            .unwrap();
        fixture.runtime.allocate(Default::default(), None).unwrap();
        let package = Arc::new(
            plugins::inspect(fixture.source.path(), &plugins::ImportOptions::default()).unwrap(),
        );
        let mut declaration = declaration("writer", 0, None);
        declaration.concurrent_group = Some("writer-source".into());
        declaration.reads = GateReadSet::default();
        declaration.required_gate = false;
        declaration.identity.dialect = HookDialect::Claude;
        let mut config=CommandConfig::new(CommandProgram::Shell("printf '%s\\n' '{\"async\":true}'; sleep 0.6; printf finished > effects/late-writer; printf '%s' '{\"hookSpecificOutput\":{\"hookEventName\":\"PostToolUse\",\"additionalContext\":\"writer finished\"}}'".into()));
        config.asynchronous = false;
        config.write_paths = vec!["effects".into()];
        config.timeout_ms = 5000;
        let registration = CommandRunner::registration_for_event(
            package,
            declaration,
            HookEvent::PostToolUse,
            config,
            None,
        )
        .unwrap();
        let model = ObserveModel {
            calls: VecDeque::from([
                vec![ToolCall {
                    id: "first".into(),
                    name: "write".into(),
                    arguments: json!({"path":"actual","content":"original"}),
                }],
                vec![],
            ]),
            prompts: Default::default(),
        };
        let mut inner = NativeSession::with_tools(
            Box::new(model),
            fixture.executor(HookEvent::PostToolUse, vec![registration]),
        );
        let (_sender, mut commands) = mpsc::channel(4);
        inner
            .turn("developer".into(), &mut commands, &fixture.events)
            .await
            .unwrap();
        assert!(hooks(&fixture).last().unwrap().outcome.is_none());
        assert_eq!(
            std::fs::read_to_string(fixture.root.path().join("effects/late-writer")).unwrap(),
            ""
        );
        task.stopped = true;
        fixture.runtime.save_task(&Some(task), 2, None).unwrap();
        fixture.runtime.finish_phase().unwrap();
        let connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
        let mut session = demoncoder::workflow::WorkflowSession::new(
            Box::new(inner),
            connection,
            fixture.root.path().into(),
            demoncoder::workflow::Settings {
                checks,
                correction_limit: 1,
                ..Default::default()
            },
            fixture.runtime.clone(),
            false,
        )
        .unwrap();
        if boundary == "backpressure" {
            let (sender, commands) = mpsc::channel(2);
            sender
                .send(demoncoder::session::Command::Cancel)
                .await
                .unwrap();
            sender
                .send(demoncoder::session::Command::Shutdown)
                .await
                .unwrap();
            let (event_sender, _receiver) = mpsc::channel(1);
            event_sender
                .try_send(Envelope {
                    connection: "full".into(),
                    event: demoncoder::events::Event::Text {
                        text: "occupied".into(),
                    },
                })
                .unwrap();
            let events = EventSink::new("full".into(), event_sender, None)
                .unwrap()
                .with_runtime(fixture.runtime.clone());
            tokio::time::timeout(
                Duration::from_secs(5),
                demoncoder::session::run(Box::new(session), commands, events),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(
                std::fs::read_to_string(fixture.root.path().join("effects/late-writer")).unwrap(),
                ""
            );
            assert_eq!(
                hooks(&fixture)
                    .last()
                    .unwrap()
                    .observer
                    .as_ref()
                    .unwrap()
                    .status,
                Status::Interrupted
            );
            continue;
        }
        if boundary == "cancel" {
            session.close().await.unwrap();
            assert_eq!(
                std::fs::read_to_string(fixture.root.path().join("effects/late-writer")).unwrap(),
                ""
            );
            assert_eq!(
                hooks(&fixture)
                    .last()
                    .unwrap()
                    .observer
                    .as_ref()
                    .unwrap()
                    .status,
                Status::Interrupted
            );
            assert!(
                session
                    .turn("/verify".into(), &mut commands, &fixture.events)
                    .await
                    .is_err()
            );
            assert!(
                fixture
                    .runtime
                    .record()
                    .unwrap()
                    .task
                    .unwrap()
                    .checks
                    .is_empty()
            );
            continue;
        }
        let result = tokio::time::timeout(
            Duration::from_secs(8),
            session.turn(
                if boundary == "accept" {
                    "/accept"
                } else {
                    "/verify"
                }
                .into(),
                &mut commands,
                &fixture.events,
            ),
        )
        .await;
        let effect_at_boundary =
            std::fs::read_to_string(fixture.root.path().join("effects/late-writer")).unwrap();
        let status_at_boundary = hooks(&fixture)
            .last()
            .unwrap()
            .observer
            .as_ref()
            .unwrap()
            .status
            .clone();
        session.close().await.unwrap();
        assert_eq!(
            effect_at_boundary, "finished",
            "writer did not finish before the evidence boundary returned"
        );
        assert_eq!(status_at_boundary, Status::Completed);
        let result = result.unwrap();
        if boundary == "accept" {
            assert!(result.is_err());
        } else {
            result.unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(fixture.root.path().join("effects/late-writer")).unwrap(),
            "finished",
            "retained command: {:?}",
            hooks(&fixture).last().unwrap().outcome
        );
        let record = fixture.runtime.record().unwrap();
        let checks = &record.task.as_ref().unwrap().checks;
        if boundary == "accept" {
            assert!(checks.is_empty());
            assert!(record.task.as_ref().unwrap().accepted.is_none());
            continue;
        }
        assert_eq!(checks.len(), 1);
        assert!(checks[0].success, "{}", checks[0].output);
        assert_eq!(
            hooks(&fixture)
                .last()
                .unwrap()
                .observer
                .as_ref()
                .unwrap()
                .status,
            Status::Completed
        );
    }
}

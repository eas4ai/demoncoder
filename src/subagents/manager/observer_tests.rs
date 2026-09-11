struct ObserverWriter {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    path: PathBuf,
}
#[async_trait::async_trait]
impl crate::plugins::dispatch::HookRunner for ObserverWriter {
    fn observer_config(&self) -> Option<crate::plugins::observer::ObserverConfig> {
        Some(crate::plugins::observer::ObserverConfig {
            declared: true,
            rewake: false,
            timeout_ms: 5000,
        })
    }
    fn mutates_workspace(&self) -> bool {
        true
    }
    async fn run(
        &self,
        _: &crate::plugins::dispatch::HookInvocation,
    ) -> Result<crate::plugins::receipts::RawOutcome> {
        self.entered.notify_one();
        self.release.notified().await;
        std::fs::write(&self.path, "observer finished")?;
        Ok(crate::plugins::receipts::RawOutcome::Callback {
            value: json!({"additionalContext":"child writer finished"}),
        })
    }
}
struct ObserverWorkerModel {
    first: bool,
}
#[async_trait::async_trait]
impl crate::native::Model for ObserverWorkerModel {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<crate::tools::ToolResult>) {}
    async fn response(&mut self, _: &EventSink) -> Result<Vec<crate::tools::ToolCall>> {
        if !std::mem::take(&mut self.first) {
            return Ok(vec![]);
        }
        Ok(vec![crate::tools::ToolCall {
            id: "original".into(),
            name: "write".into(),
            arguments: json!({"path":"owned","content":"foreground"}),
        }])
    }
}
#[tokio::test]
async fn child_worker_waits_for_observer_effect_and_holds_cancelled_writer() {
    use crate::plugins::{
        dispatch::*, gate_snapshot::GateReadSet, hook_types::*, lifecycle::PostToolPlan,
        receipts::Scope,
    };
    for cancellation in ["none", "unknown", "explicit"] {
        let cancel = cancellation != "none";
        let fixture = integration_fixture_with_record(true, "sessions/1-1").await;
        fixture
            .runtime
            .update_agent(1, |agent| {
                agent.identity = Identity::from(&fixture.manager.settings.connections["worker"]);
                agent.status = AgentStatus::Running;
                agent.completed = false;
                agent.orchestration.as_mut().unwrap().stage = OrchestrationStage::Working;
                Ok(())
            })
            .unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let mut executor = crate::tools::ToolExecutor::new(&fixture.identity.root).unwrap();
        let registration = Registration {
            declaration: Declaration {
                identity: DeclarationIdentity {
                    package: "child-observer".into(),
                    code: "code".into(),
                    policy: "policy".into(),
                    configuration: "config".into(),
                    generation: "generation".into(),
                    scope: Scope::Project,
                    role: "agent:1:worker".into(),
                    declaration: "writer".into(),
                    index: 0,
                    dialect: HookDialect::Native,
                    runner: HandlerKind::Command,
                },
                required_gate: false,
                source: None,
                once: None,
                class: HandlerClass::Observer,
                priority: 0,
                matcher: Matcher::default(),
                reads: GateReadSet::default(),
                concurrent_group: None,
                read_only_endpoint: None,
                external_precondition: None,
            },
            runner: Arc::new(ObserverWriter {
                entered: entered.clone(),
                release: release.clone(),
                path: fixture.identity.root.join("owned"),
            }),
            revalidation: None,
        };
        executor
            .register_post_tool_plan(Arc::new(
                PostToolPlan::new(HookEvent::PostToolUse, vec![registration]).unwrap(),
            ))
            .unwrap();
        let mut session: Option<Box<dyn Session>> =
            Some(Box::new(crate::native::NativeSession::with_tools(
                Box::new(ObserverWorkerModel { first: true }),
                executor,
            )));
        let events = fixture.events.for_phase("agent:1:worker");
        {
            let work = fixture.manager.worker_turn(1, &events, &mut session, None);
            tokio::pin!(work);
            tokio::select! { result=&mut work=>panic!("worker ended before observer start: {result:?}"), entered=tokio::time::timeout(Duration::from_secs(3),entered.notified())=>{entered.unwrap();} }
            let mut restored = persisted_record(&fixture.record_root);
            crate::workflow::runtime::plugin_observer::interrupt_restored(&mut restored);
            assert_eq!(restored.agents[0].status, AgentStatus::Uncertain);
            assert!(!restored.recovery_pending);
            assert_eq!(
                std::fs::read_to_string(fixture.identity.root.join("owned")).unwrap(),
                "foreground"
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(25), &mut work)
                    .await
                    .is_err()
            );
            if cancellation == "explicit" {
                fixture.manager.cancel(1).await.unwrap();
            } else if cancel {
                fixture
                    .runtime
                    .stop_observers(Some("agent:1"), false)
                    .await
                    .unwrap();
            }
            release.notify_one();
            let result = tokio::time::timeout(Duration::from_secs(3), &mut work)
                .await
                .unwrap();
            assert_eq!(result.is_err(), cancel, "{result:?}");
        }
        session.as_mut().unwrap().close().await.unwrap();
        if cancel {
            fixture.manager.finish_job(1, &Job::Work, Ok(())).unwrap();
            let persisted = persisted_record(&fixture.record_root);
            assert_eq!(
                persisted.agents[0].status,
                if cancellation == "explicit" {
                    AgentStatus::Cancelled
                } else {
                    AgentStatus::Uncertain
                }
            );
            assert_eq!(
                persisted.agents[0].orchestration.as_ref().unwrap().stage,
                OrchestrationStage::Held
            );
            assert!(
                !persisted.recovery_pending,
                "child interruption must not take over the parent task"
            );
        }
        assert_eq!(
            std::fs::read_to_string(fixture.identity.root.join("owned")).unwrap(),
            if cancel {
                "foreground"
            } else {
                "observer finished"
            }
        );
    }
}

struct DelayedChildObserver {
    release: Arc<tokio::sync::Notify>,
    effects: Arc<std::sync::atomic::AtomicUsize>,
    rewake: bool,
}
#[async_trait::async_trait]
impl crate::plugins::dispatch::HookRunner for DelayedChildObserver {
    fn observer_config(&self) -> Option<crate::plugins::observer::ObserverConfig> {
        Some(crate::plugins::observer::ObserverConfig {
            declared: true,
            rewake: self.rewake,
            timeout_ms: 5000,
        })
    }
    async fn run(
        &self,
        _: &crate::plugins::dispatch::HookInvocation,
    ) -> Result<crate::plugins::receipts::RawOutcome> {
        self.release.notified().await;
        self.effects.fetch_add(1, Ordering::SeqCst);
        Ok(crate::plugins::receipts::RawOutcome::Command {
            exit_code: Some(if self.rewake { 2 } else { 0 }),
            stdout: if self.rewake {
                vec![]
            } else {
                serde_json::to_vec(&json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"ordinary child observer data"}})).unwrap()
            },
            stderr: if self.rewake {
                b"/task child observer data is not developer control".to_vec()
            } else {
                vec![]
            },
        })
    }
}
struct RewakeWorkerModel {
    responses: Arc<std::sync::atomic::AtomicUsize>,
    prompts: Arc<std::sync::Mutex<Vec<String>>>,
    foreground: Arc<tokio::sync::Notify>,
}
#[async_trait::async_trait]
impl crate::native::Model for RewakeWorkerModel {
    fn prompt(&mut self, text: String) {
        self.prompts.lock().unwrap().push(text);
    }
    fn results(&mut self, _: Vec<crate::tools::ToolResult>) {}
    async fn response(&mut self, _: &EventSink) -> Result<Vec<crate::tools::ToolCall>> {
        match self.responses.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(vec![crate::tools::ToolCall {
                id: "foreground".into(),
                name: "write".into(),
                arguments: json!({"path":"owned","content":"foreground"}),
            }]),
            1 => {
                self.foreground.notify_one();
                Ok(vec![])
            }
            _ => Ok(vec![]),
        }
    }
}
#[tokio::test]
async fn child_idle_observer_rewake_keeps_original_supervision_and_parent_phase() {
    use crate::plugins::{
        dispatch::*, gate_snapshot::GateReadSet, hook_types::*, lifecycle::PostToolPlan,
        receipts::Scope,
    };
    for mode in ["rewake", "ordinary", "exhausted", "cancel", "changed-owner"] {
        let fixture = integration_fixture_with_record(true, "sessions/1-1").await;
        let identity = Identity::from(&fixture.manager.settings.connections["worker"]);
        fixture
            .runtime
            .update_agent(1, |agent| {
                agent.identity = identity.clone();
                agent.status = AgentStatus::Running;
                agent.completed = false;
                let state = agent.orchestration.as_mut().unwrap();
                state.stage = OrchestrationStage::Working;
                state.correction_rounds = if mode == "exhausted" { 2 } else { 0 };
                Ok(())
            })
            .unwrap();
        fixture.runtime.begin_phase("worker", None).unwrap();
        let before = serde_json::to_value(fixture.runtime.record().unwrap().allocation).unwrap();
        let release = Arc::new(tokio::sync::Notify::new());
        let foreground = Arc::new(tokio::sync::Notify::new());
        let effects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let responses = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let prompts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut executor = crate::tools::ToolExecutor::new(&fixture.identity.root).unwrap();
        let registration = Registration {
            declaration: Declaration {
                identity: DeclarationIdentity {
                    package: "child-observer".into(),
                    code: "code".into(),
                    policy: "policy".into(),
                    configuration: "config".into(),
                    generation: "generation".into(),
                    scope: Scope::Project,
                    role: "agent:1:worker".into(),
                    declaration: "late".into(),
                    index: 0,
                    dialect: HookDialect::Claude,
                    runner: HandlerKind::Command,
                },
                required_gate: false,
                source: None,
                once: None,
                class: HandlerClass::Combined,
                priority: 0,
                matcher: Matcher::default(),
                reads: GateReadSet::default(),
                concurrent_group: Some("source".into()),
                read_only_endpoint: None,
                external_precondition: None,
            },
            runner: Arc::new(DelayedChildObserver {
                release: release.clone(),
                effects: effects.clone(),
                rewake: mode != "ordinary",
            }),
            revalidation: None,
        };
        executor
            .register_post_tool_plan(Arc::new(
                PostToolPlan::new(HookEvent::PostToolUse, vec![registration]).unwrap(),
            ))
            .unwrap();
        let mut session: Option<Box<dyn Session>> =
            Some(Box::new(crate::native::NativeSession::with_tools(
                Box::new(RewakeWorkerModel {
                    responses: responses.clone(),
                    prompts: prompts.clone(),
                    foreground: foreground.clone(),
                }),
                executor,
            )));
        let events = fixture.events.for_phase("agent:1:worker");
        {
            let work = fixture.manager.worker_turn(1, &events, &mut session, None);
            tokio::pin!(work);
            tokio::select! {
                result=&mut work=>panic!("child advanced while its admitted observer was paused after foreground: {result:?}; responses={}",responses.load(Ordering::SeqCst)),
                done=tokio::time::timeout(Duration::from_secs(3),foreground.notified())=>done.unwrap(),
            }
            assert_eq!(responses.load(Ordering::SeqCst), 2);
            assert!(
                tokio::time::timeout(Duration::from_millis(25), &mut work)
                    .await
                    .is_err()
            );
            assert_eq!(effects.load(Ordering::SeqCst), 0);
            if mode == "cancel" {
                fixture.manager.cancel(1).await.unwrap();
            }
            if mode == "changed-owner" {
                fixture
                    .runtime
                    .update_agent(1, |agent| {
                        agent.request.objective = "changed owner".into();
                        Ok(())
                    })
                    .unwrap();
                tokio::time::timeout(Duration::from_secs(2), async {
                    while fixture.runtime.record().unwrap().agents[0].status
                        != AgentStatus::Uncertain
                    {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
            }
            release.notify_one();
            let result = tokio::time::timeout(Duration::from_secs(3), &mut work)
                .await
                .unwrap();
            assert_eq!(
                result.is_err(),
                matches!(mode, "cancel" | "changed-owner"),
                "{mode}: {result:?}"
            );
        }
        let record = fixture.runtime.record().unwrap();
        assert_eq!(record.phase.as_deref(), Some("worker"));
        assert_eq!(record.task.as_ref().unwrap().id, 1);
        assert_eq!(record.task.as_ref().unwrap().corrections, 0);
        let after = serde_json::to_value(&record.allocation).unwrap();
        assert_eq!(before["started_ms"], after["started_ms"]);
        assert_eq!(before["deadline_ms"], after["deadline_ms"]);
        assert_eq!(
            record.agents[0]
                .orchestration
                .as_ref()
                .unwrap()
                .correction_rounds,
            if mode == "rewake" {
                1
            } else if mode == "exhausted" {
                2
            } else {
                0
            }
        );
        assert_eq!(
            responses.load(Ordering::SeqCst),
            if mode == "rewake" { 3 } else { 2 }
        );
        assert_eq!(
            effects.load(Ordering::SeqCst),
            usize::from(!matches!(mode, "cancel" | "changed-owner"))
        );
        assert_eq!(
            prompts
                .lock()
                .unwrap()
                .iter()
                .any(|p| p.contains("Plugin-origin") && p.contains("/task child observer data")),
            mode == "rewake"
        );
        if mode == "ordinary" {
            let delivery = fixture
                .runtime
                .reserve_observer_context("agent:1:worker", Some(&identity), false)
                .unwrap()
                .unwrap();
            assert!(delivery.text.contains("ordinary child observer data"));
        }
        session.as_mut().unwrap().close().await.unwrap();
    }
}

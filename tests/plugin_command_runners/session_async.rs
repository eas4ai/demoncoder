use super::*;
use demoncoder::{
    events::Event,
    plugins::{hook_types::HookEvent, observer::Status},
    session::{self, Command},
    workflow::{allocation::Limits, runtime::BudgetRef, workspace::CaptureScope},
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn funded(slots: u64) -> Fixture {
    funded_seconds(slots, 60)
}
fn funded_seconds(slots: u64, seconds: u64) -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open_with_session_hooks(
        root.path(),
        &connection,
        None,
        &CaptureScope::default(),
        Some(&Limits {
            seconds,
            model_calls: slots,
            tool_calls: slots,
        }),
    )
    .unwrap();
    let (sender, receiver) = mpsc::channel(256);
    let events = EventSink::new("async session".into(), sender, None)
        .unwrap()
        .with_runtime(runtime.clone());
    std::fs::write(root.path().join("lifetime.txt"), "").unwrap();
    Fixture {
        root,
        runtime,
        events,
        _receiver: receiver,
    }
}

struct Counting {
    prompts: Arc<Mutex<Vec<String>>>,
    requests: Arc<AtomicUsize>,
    hold: Option<Arc<tokio::sync::Notify>>,
}
#[async_trait::async_trait]
impl Model for Counting {
    fn prompt(&mut self, text: String) {
        self.prompts.lock().unwrap().push(text);
    }
    fn results(&mut self, _: Vec<ToolResult>) {
        panic!("unexpected tools");
    }
    async fn response(&mut self, _: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        if let Some(hold) = &self.hold {
            hold.notified().await;
        }
        Ok(vec![])
    }
}
struct Running {
    commands: mpsc::Sender<Command>,
    owner: tokio::task::JoinHandle<anyhow::Result<()>>,
    requests: Arc<AtomicUsize>,
    prompts: Arc<Mutex<Vec<String>>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.owner.abort();
    }
}
fn launch(fixture: &Fixture, event: HookEvent, source: &str) -> Running {
    let mut executor = fixture.executor(vec![], false);
    executor
        .register_non_tool_plan(lifetime_plan(fixture, event, source, true, false))
        .unwrap();
    launch_executor(fixture, executor, None)
}
fn launch_executor(
    fixture: &Fixture,
    executor: ToolExecutor,
    hold: Option<Arc<tokio::sync::Notify>>,
) -> Running {
    let requests = Arc::new(AtomicUsize::new(0));
    let prompts = Arc::new(Mutex::new(vec![]));
    let native = NativeSession::with_tools(
        Box::new(Counting {
            requests: requests.clone(),
            prompts: prompts.clone(),
            hold,
        }),
        executor,
    );
    let (commands, receiver) = mpsc::channel(8);
    let owner = tokio::spawn(session::run(
        Box::new(native),
        receiver,
        fixture.events.clone(),
    ));
    Running {
        commands,
        owner,
        requests,
        prompts,
    }
}
async fn ready(fixture: &mut Fixture) {
    tokio::time::timeout(Duration::from_secs(4), async {
        while !matches!(
            fixture._receiver.recv().await.unwrap().event,
            Event::Ready { .. }
        ) {}
    })
    .await
    .expect("async startup blocked readiness");
}
fn hook(fixture: &Fixture) -> Option<plugins::receipts::HookReceipt> {
    fixture
        .record()
        .operations
        .iter()
        .filter_map(|o| match &o.host_invocation {
            Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(receipt)) => {
                Some(receipt)
            }
            _ => None,
        })
        .flat_map(|o| &o.hooks)
        .find(|h| h.observer.is_some())
        .cloned()
}
async fn completed(fixture: &Fixture) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while hook(fixture).is_none_or(|h| h.outcome.is_none()) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("async command never completed");
}
async fn stop(mut running: Running) {
    running.commands.send(Command::Shutdown).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), &mut running.owner)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
fn unspent(fixture: &Fixture) {
    let record = fixture.record();
    let allowance = record.session_hook_allowance.unwrap();
    assert_eq!(
        (
            allowance.allocation.model_calls,
            allowance.allocation.tool_calls,
            allowance.backend_invocations
        ),
        (0, 0, 0)
    );
    assert_eq!(record.backend_invocations, 0);
}

#[tokio::test]
async fn session_async_ready_precedes_actual_effect_and_completion_with_zero_model_slots() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(0);
    let running = launch(
        &fixture,
        HookEvent::SessionStart,
        "import time\ntime.sleep(0.8)\nwith open('lifetime.txt','a') as f: f.write('effect')\nprint('{}')\n",
    );
    ready(&mut fixture).await;
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("lifetime.txt")).unwrap(),
        "",
        "Ready waited for async effect"
    );
    let transfer = hook(&fixture).expect("startup lacks durable async transfer");
    assert!(transfer.outcome.is_none());
    assert_eq!(transfer.observer.unwrap().status, Status::Running);
    completed(&fixture).await;
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("lifetime.txt")).unwrap(),
        "effect"
    );
    assert_eq!(running.requests.load(Ordering::SeqCst), 0);
    unspent(&fixture);
    stop(running).await;
}

#[tokio::test]
async fn session_async_end_has_original_observation_window_and_cannot_start_model() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(0);
    let running = launch(
        &fixture,
        HookEvent::SessionEnd,
        "import time\ntime.sleep(0.2)\nwith open('lifetime.txt','a') as f: f.write('terminal')\nprint('{\"systemMessage\":\"/task forbidden\"}')\n",
    );
    ready(&mut fixture).await;
    let requests = running.requests.clone();
    stop(running).await;
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("lifetime.txt")).unwrap(),
        "terminal"
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    assert!(fixture.record().task.is_none());
    unspent(&fixture);
}

#[tokio::test]
async fn session_async_context_joins_one_existing_request_without_rewake_or_task_debit() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(0);
    let running = launch(
        &fixture,
        HookEvent::SessionStart,
        "import time\ntime.sleep(0.1)\nprint('{\"hookSpecificOutput\":{\"hookEventName\":\"SessionStart\",\"additionalContext\":\"/task plugin observation\"}}')\n",
    );
    ready(&mut fixture).await;
    completed(&fixture).await;
    assert_eq!(running.requests.load(Ordering::SeqCst), 0);
    running
        .commands
        .send(Command::Prompt("ordinary request".into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !matches!(
            fixture._receiver.recv().await.unwrap().event,
            Event::TurnFinished { .. }
        ) {}
    })
    .await
    .unwrap();
    assert_eq!(running.requests.load(Ordering::SeqCst), 1);
    assert!(running.prompts.lock().unwrap().iter().any(|text| text.contains("Plugin-origin") && text.contains("/task plugin observation")));
    assert!(fixture.record().task.is_none());
    assert!(
        fixture
            .record()
            .operations
            .iter()
            .filter(|o| matches!(
                o.host_invocation,
                Some(demoncoder::workflow::runtime::HostInvocation::Model)
            ))
            .all(|o| o.budget == Some(BudgetRef::Unallocated))
    );
    unspent(&fixture);
    stop(running).await;
}

#[tokio::test]
async fn session_async_survives_task_allocation_replacement() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(0);
    fixture.runtime.allocate(Limits::default(), None).unwrap();
    let running = launch(
        &fixture,
        HookEvent::SessionStart,
        "import time\ntime.sleep(0.7)\nwith open('lifetime.txt','a') as f: f.write('original session')\nprint('{}')\n",
    );
    ready(&mut fixture).await;
    assert!(hook(&fixture).is_some(), "startup async transfer missing");
    fixture
        .runtime
        .allocate(Limits::default(), None)
        .expect("independent session observer pinned task replacement");
    completed(&fixture).await;
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("lifetime.txt")).unwrap(),
        "original session"
    );
    assert_eq!(fixture.record().allocation.unwrap().model_calls, 0);
    unspent(&fixture);
    stop(running).await;
}

#[tokio::test]
async fn session_async_live_grant_ignores_unrelated_expired_task_during_transfer_and_settlement() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(0);
    fixture
        .runtime
        .allocate(
            Limits {
                seconds: 1,
                ..Limits::default()
            },
            None,
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(
        fixture.record().allocation.unwrap().remaining_ms().unwrap(),
        0
    );
    let running = launch(
        &fixture,
        HookEvent::SessionStart,
        "import time\ntime.sleep(0.1)\nwith open('lifetime.txt','a') as f: f.write('session funded')\nprint('{}')\n",
    );
    ready(&mut fixture).await;
    completed(&fixture).await;
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("lifetime.txt")).unwrap(),
        "session funded"
    );
    assert_eq!(running.requests.load(Ordering::SeqCst), 0);
    assert_eq!(
        hook(&fixture).unwrap().observer.unwrap().status,
        Status::Completed
    );
    unspent(&fixture);
    stop(running).await;
}

#[tokio::test]
async fn session_async_context_response_can_finish_after_original_session_time_expires() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded_seconds(0, 2);
    let mut executor = fixture.executor(vec![], false);
    executor.register_non_tool_plan(lifetime_plan(&fixture,HookEvent::SessionStart,
        "print('{\"hookSpecificOutput\":{\"hookEventName\":\"SessionStart\",\"additionalContext\":\"late response context\"}}')\n",true,false)).unwrap();
    let release = Arc::new(tokio::sync::Notify::new());
    let running = launch_executor(&fixture, executor, Some(release.clone()));
    ready(&mut fixture).await;
    completed(&fixture).await;
    running
        .commands
        .send(Command::Prompt("ordinary request".into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while running.requests.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let reservation = hook(&fixture).unwrap().observer.unwrap();
    assert_eq!(reservation.delivery, plugins::observer::Delivery::Reserved);
    assert!(
        running
            .prompts
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.contains("late response context"))
    );
    tokio::time::sleep(Duration::from_millis(2100)).await;
    assert_eq!(
        fixture
            .record()
            .session_hook_allowance
            .unwrap()
            .allocation
            .remaining_ms()
            .unwrap(),
        0
    );
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        while !matches!(
            fixture._receiver.recv().await.unwrap().event,
            Event::TurnFinished { .. }
        ) {}
    })
    .await
    .unwrap();
    let settled = hook(&fixture).unwrap().observer.unwrap();
    assert_eq!(settled.delivery, plugins::observer::Delivery::Delivered);
    assert_eq!(settled.delivery_operation, reservation.delivery_operation);
    assert_eq!(running.requests.load(Ordering::SeqCst), 1);
    assert!(fixture.record().task.is_none());
    unspent(&fixture);
    stop(running).await;
}

async fn started(fixture: &Fixture) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while std::fs::read(fixture.root.path().join("lifetime.txt"))
            .unwrap()
            .is_empty()
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn session_async_end_deadline_and_aborted_host_reap_actual_process_tree() {
    let _lock = FIXTURES.lock().await;
    for event in [HookEvent::SessionStart, HookEvent::SessionEnd] {
        let mut fixture = funded(0);
        let mut running = launch(
            &fixture,
            event,
            "import os,time\nif os.fork()==0:\n while True:\n  with open('lifetime.txt','a') as f: f.write('effect\\n')\n  time.sleep(0.02)\ntime.sleep(100)\n",
        );
        ready(&mut fixture).await;
        let ended = tokio::time::Instant::now();
        if event == HookEvent::SessionEnd {
            running.commands.send(Command::Shutdown).await.unwrap();
        }
        started(&fixture).await;
        let (supervisor, namespace) = sandbox_owners(&format!(
            "lifetime-{}",
            fixture.root.path().file_name().unwrap().to_string_lossy()
        ));
        if event == HookEvent::SessionStart {
            running.owner.abort();
        }
        let deadline = ended + Duration::from_secs(5);
        let result = tokio::time::timeout_at(deadline, &mut running.owner).await;
        if result.is_err() {
            running.owner.abort();
            let _ = (&mut running.owner).await;
        }
        let stopped = tokio::time::timeout_at(deadline, async {
            while !namespace.stopped() || !supervisor.stopped() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        if !stopped {
            // Cleanup after a failing measurement grants no extra test budget.
            namespace.stop();
            supervisor.stop();
            tokio::time::timeout(Duration::from_secs(3), async {
                while !namespace.stopped() || !supervisor.stopped() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
        }
        assert!(
            result.is_ok() && stopped,
            "host and actual processes exceeded original five-second end deadline"
        );
        let result = result.unwrap();
        if event == HookEvent::SessionEnd {
            result.unwrap().unwrap();
            assert!(
                ended.elapsed() >= Duration::from_millis(1800),
                "end transfer did not join its original observation window"
            );
        } else {
            assert!(result.unwrap_err().is_cancelled());
        }
        let effect = std::fs::read(fixture.root.path().join("lifetime.txt")).unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            std::fs::read(fixture.root.path().join("lifetime.txt")).unwrap(),
            effect
        );
        assert_eq!(running.requests.load(Ordering::SeqCst), 0);
        let receipt = hook(&fixture).unwrap();
        assert_eq!(receipt.observer.unwrap().status, Status::Interrupted);
        assert!(receipt.uncertain_effects && fixture.record().recovery_pending);
        unspent(&fixture);
    }
}

#[tokio::test]
async fn session_async_completed_start_does_not_close_separately_admitted_end_jobs() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(0);
    let mut executor = fixture.executor(vec![], false);
    executor.register_non_tool_plan(lifetime_plan(&fixture,HookEvent::SessionStart,
        "print('{\"hookSpecificOutput\":{\"hookEventName\":\"SessionStart\",\"additionalContext\":\"withheld at actual end\"}}')\n",true,false)).unwrap();
    executor.register_non_tool_plan(lifetime_plan(&fixture,HookEvent::SessionEnd,
        "import time\ntime.sleep(0.1)\nwith open('lifetime.txt','a') as f: f.write('end owned')\nprint('{}')\n",true,false)).unwrap();
    let running = launch_executor(&fixture, executor, None);
    ready(&mut fixture).await;
    completed(&fixture).await;
    stop(running).await;
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("lifetime.txt")).unwrap(),
        "end owned"
    );
    assert_eq!(
        hook(&fixture).unwrap().observer.unwrap().delivery,
        plugins::observer::Delivery::Withheld
    );
    assert!(!fixture.record().recovery_pending);
    unspent(&fixture);
}

fn native_registration(
    fixture: &Fixture,
    name: &str,
    code: &str,
    asynchronous: bool,
) -> Registration {
    let (_source, captured) = package(HookDialect::Native, code);
    let mut declared = declaration(name, HookDialect::Native, HandlerClass::Observer);
    declared.matcher = Matcher::default();
    declared.priority = if asynchronous { 0 } else { 1 };
    let mut config = python_config();
    config.asynchronous = asynchronous;
    config.write_paths = vec!["lifetime.txt".into()];
    if let CommandProgram::Argv(args) = &mut config.program {
        args.push(format!(
            "lifetime-{}",
            fixture.root.path().file_name().unwrap().to_string_lossy()
        ));
    }
    CommandRunner::registration_for_event(captured, declared, HookEvent::SessionStart, config, None)
        .unwrap()
}
#[tokio::test]
async fn session_async_cancelled_startup_drains_earlier_transferred_writer_before_ready() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(0);
    let first = native_registration(
        &fixture,
        "first",
        "import os,time\nif os.fork()==0:\n while True:\n  with open('lifetime.txt','a') as f: f.write('early\\n')\n  time.sleep(0.02)\ntime.sleep(100)\n",
        true,
    );
    let second = native_registration(
        &fixture,
        "second",
        "with open('lifetime.txt','a') as f: f.write('forbidden second')\nprint('{}')\n",
        false,
    );
    let mut executor = fixture.executor(vec![], false);
    executor
        .register_non_tool_plan(Arc::new(
            plugins::non_tool::NonToolPlan::new(HookEvent::SessionStart, vec![first, second])
                .unwrap(),
        ))
        .unwrap();
    let running = launch_executor(&fixture, executor, None);
    started(&fixture).await;
    let (supervisor, namespace) = sandbox_owners(&format!(
        "lifetime-{}",
        fixture.root.path().file_name().unwrap().to_string_lossy()
    ));
    assert!(hook(&fixture).unwrap().outcome.is_none());
    running.commands.send(Command::Cancel).await.unwrap();
    ready(&mut fixture).await;
    assert!(
        namespace.stopped() && supervisor.stopped(),
        "interrupted readiness preceded transferred process cleanup"
    );
    let effect = std::fs::read_to_string(fixture.root.path().join("lifetime.txt")).unwrap();
    assert!(!effect.contains("forbidden"));
    assert_eq!(
        hook(&fixture).unwrap().observer.unwrap().status,
        Status::Interrupted
    );
    assert!(fixture.record().recovery_pending);
    assert_eq!(running.requests.load(Ordering::SeqCst), 0);
    unspent(&fixture);
    stop(running).await;
}

#[tokio::test]
async fn session_async_completion_during_response_waits_for_the_next_existing_request() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(0);
    let mut executor = fixture.executor(vec![], false);
    executor.register_non_tool_plan(lifetime_plan(&fixture,HookEvent::SessionStart,
        "import time\ntime.sleep(0.6)\nprint('{\"hookSpecificOutput\":{\"hookEventName\":\"SessionStart\",\"additionalContext\":\"context completed during response\"}}')\n",true,false)).unwrap();
    let release = Arc::new(tokio::sync::Notify::new());
    let running = launch_executor(&fixture, executor, Some(release.clone()));
    ready(&mut fixture).await;
    running
        .commands
        .send(Command::Prompt("first ordinary request".into()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while running.requests.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    completed(&fixture).await;
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        while !matches!(
            fixture._receiver.recv().await.unwrap().event,
            Event::TurnFinished { .. }
        ) {}
    })
    .await
    .unwrap();
    assert_eq!(
        running.requests.load(Ordering::SeqCst),
        1,
        "completion entered post-response correction path"
    );
    assert!(
        !running
            .prompts
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.contains("context completed during response"))
    );
    assert_eq!(
        hook(&fixture).unwrap().observer.unwrap().delivery,
        plugins::observer::Delivery::Pending
    );
    running
        .commands
        .send(Command::Prompt("second ordinary request".into()))
        .await
        .unwrap();
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        while !matches!(
            fixture._receiver.recv().await.unwrap().event,
            Event::TurnFinished { .. }
        ) {}
    })
    .await
    .unwrap();
    assert_eq!(running.requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        running
            .prompts
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.contains("context completed during response"))
            .count(),
        1
    );
    assert!(fixture.record().task.is_none());
    unspent(&fixture);
    stop(running).await;
}

#[tokio::test]
async fn session_async_native_first_line_marker_cannot_transfer_synchronous_command() {
    let _lock = FIXTURES.lock().await;
    let mut fixture = funded(0);
    let mut executor = fixture.executor(vec![], false);
    executor.register_non_tool_plan(lifetime_plan(&fixture,HookEvent::SessionStart,
        "import time\nprint('{\"async\":true}',flush=True)\ntime.sleep(0.4)\nwith open('lifetime.txt','a') as f: f.write('synchronous effect')\nprint('{}')\n",false,false)).unwrap();
    let running = launch_executor(&fixture, executor, None);
    ready(&mut fixture).await;
    assert_eq!(
        std::fs::read_to_string(fixture.root.path().join("lifetime.txt")).unwrap(),
        "synchronous effect"
    );
    assert!(
        hook(&fixture).is_none(),
        "native output minted a Claude-only transfer"
    );
    assert_eq!(running.requests.load(Ordering::SeqCst), 0);
    unspent(&fixture);
    stop(running).await;
}

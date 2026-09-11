use super::*;
use crate::{
    config::Connection,
    plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        lifecycle::PostToolPlan,
        receipts::PostDelivery,
    },
    tools::ToolHook,
    workflow::{allocation::Limits, runtime::SharedRuntime, state::Task, workspace},
};
use serde_json::json;
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::Poll,
    time::Duration,
};

struct Correction;
#[async_trait]
impl HookRunner for Correction {
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        Ok(RawOutcome::Model {
            value: json!({"ok":false,"reason":"correct this completed result"}),
            continue_on_block: false,
        })
    }
}

struct FinalBoundary {
    commands: mpsc::Sender<Command>,
    captures: Arc<tokio::sync::Semaphore>,
    reserved: Arc<Mutex<Option<tokio::sync::OwnedSemaphorePermit>>>,
    presented: Arc<AtomicBool>,
    block_capture: bool,
}
impl ToolHook for FinalBoundary {
    fn before(&self, _: &mut ToolCall) -> Result<()> {
        Ok(())
    }
    fn present(&self, _: &ToolResult) -> Result<String> {
        if self.block_capture {
            let permits = self.captures.available_permits() as u32;
            assert!(permits > 0);
            *self.reserved.lock().unwrap() =
                Some(self.captures.clone().try_acquire_many_owned(permits)?);
        } else {
            self.commands.try_send(Command::Cancel)?;
        }
        self.presented.store(true, Ordering::SeqCst);
        Ok("completed presentation".into())
    }
}

struct OneWrite(Arc<AtomicUsize>);
#[async_trait]
impl Model for OneWrite {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, _: Vec<ToolResult>) {}
    async fn response(&mut self, _: &EventSink) -> Result<Vec<ToolCall>> {
        Ok(if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            vec![ToolCall {
                id: "completed".into(),
                name: "write".into(),
                arguments: json!({"path":"created","content":"retained"}),
            }]
        } else {
            Vec::new()
        })
    }
}

static FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn cancel_final_release(block_capture: bool) {
    let _lock = FIXTURE.lock().await;
    let root = tempfile::tempdir().unwrap();
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    runtime.allocate(Limits::default(), None).unwrap();
    let task = Task::new(
        1,
        "cancel without spending correction".into(),
        vec![],
        workspace::capture(root.path()).unwrap(),
        1,
    )
    .unwrap();
    runtime.save_task(&Some(task), 2, None).unwrap();
    let plan = Arc::new(
        PostToolPlan::new(
            HookEvent::PostToolUse,
            vec![Registration {
                declaration: Declaration {
                    identity: DeclarationIdentity {
                        package: "cancellation-regression".into(),
                        code: "code".into(),
                        policy: "policy".into(),
                        configuration: "configuration".into(),
                        generation: "generation-1".into(),
                        scope: Scope::Project,
                        role: "worker".into(),
                        declaration: "correction".into(),
                        index: 0,
                        dialect: HookDialect::Native,
                        runner: HandlerKind::Prompt,
                    },
                    class: HandlerClass::DecisionGate,
                    priority: 0,
                    matcher: Matcher::default(),
                    reads: GateReadSet::default(),
                    concurrent_group: None,
                    read_only_endpoint: None,
                    external_precondition: None,
                },
                runner: Arc::new(Correction),
                revalidation: None,
            }],
        )
        .unwrap(),
    );
    let (commands, mut command_rx) = mpsc::channel(4);
    let reserved = Arc::new(Mutex::new(None));
    let presented = Arc::new(AtomicBool::new(false));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools.register_post_tool_plan(plan.clone()).unwrap();
    tools.add_hook(Box::new(FinalBoundary {
        commands: commands.clone(),
        captures: plan.plan.captures.clone(),
        reserved: reserved.clone(),
        presented: presented.clone(),
        block_capture,
    }));
    let requests = Arc::new(AtomicUsize::new(0));
    let mut session = NativeSession::with_tools(Box::new(OneWrite(requests.clone())), tools);
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("post-cancel".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let turn = session.turn("write once".into(), &mut command_rx, &events);
    tokio::pin!(turn);
    if block_capture {
        // The real final capture waits for its permit. No sleep or timing race
        // decides whether cancellation precedes the release checkpoint.
        tokio::time::timeout(
            Duration::from_secs(3),
            std::future::poll_fn(|cx| {
                assert!(turn.as_mut().poll(cx).is_pending());
                if presented.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            }),
        )
        .await
        .unwrap();
        commands.try_send(Command::Cancel).unwrap();
    }
    let result = tokio::time::timeout(Duration::from_millis(500), &mut turn).await;
    reserved.lock().unwrap().take();
    assert!(
        matches!(result, Ok(Ok(TurnEnd::Cancelled))),
        "cancellation must own the final release wait"
    );
    let record = runtime.record().unwrap();
    assert_eq!(record.task.as_ref().unwrap().corrections, 0);
    assert_eq!(record.allocation.as_ref().unwrap().model_calls, 1);
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    let operation = record.operations.iter().find(|o| o.call.is_some()).unwrap();
    assert!(operation.result.as_ref().unwrap().success);
    assert_eq!(
        operation.result.as_ref().unwrap().output,
        "Wrote 8 bytes to created"
    );
    let post = operation
        .tool_receipt
        .as_ref()
        .unwrap()
        .plugin_lifecycle
        .as_ref()
        .unwrap();
    assert_eq!(post.delivery, PostDelivery::LocalPending);
    assert!(!post.correction_admitted);
    assert_eq!(
        std::fs::read_to_string(root.path().join("created")).unwrap(),
        "retained"
    );
    std::fs::remove_dir_all(runtime.directory().unwrap()).unwrap();
}

#[tokio::test]
async fn cancellation_queued_by_presentation_precedes_post_release_charge() {
    cancel_final_release(false).await;
}

#[tokio::test]
async fn cancellation_during_final_post_capture_precedes_release_charge() {
    cancel_final_release(true).await;
}

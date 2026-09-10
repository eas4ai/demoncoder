use super::*;
use crate::{
    config::Connection,
    events::{Event, EventSink},
    tools::{ToolExecutor, ToolHook},
    workflow::{
        runtime::{Identity, Record},
        store::Store,
    },
};
use serde_json::json;
use tokio::sync::mpsc;

fn fixture(
    root: &std::path::Path,
    capacity: usize,
) -> (
    SharedRuntime,
    EventSink,
    mpsc::Receiver<crate::events::Envelope>,
) {
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let record: Record = serde_json::from_value(json!({
        "workspace":root, "identity":Identity::from(&connection),
        "archived":[], "next_task":1, "checkpoint_cursor":0, "operations":[],
        "messages":[], "recovery_pending":false, "decisions":[]
    }))
    .unwrap();
    let runtime = SharedRuntime::for_test(&root.join("record"), record).unwrap();
    let (sender, receiver) = mpsc::channel(capacity);
    let events = EventSink::new("fixture".into(), sender, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let id = events.begin_model().unwrap();
    (runtime, events.for_invocation(id), receiver)
}

fn edit() -> ToolCall {
    ToolCall {
        id: "same-source".into(),
        name: "edit".into(),
        arguments: json!({"path":"counter","old_text":"a","new_text":"ab"}),
    }
}

#[tokio::test]
async fn durable_result_precedes_blocked_ui_and_late_failure_cannot_replace_it() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 1);
    let tools = ToolExecutor::new(root.path()).unwrap();
    let timed_out = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        tools.execute(edit(), &events),
    )
    .await;
    assert!(timed_out.is_err());
    let saved = Store::read_snapshot(&runtime.directory().unwrap()).unwrap();
    let record: Record = serde_json::from_value(saved).unwrap();
    let original = record.operations.last().unwrap().result.as_ref().unwrap();
    assert!(original.success);
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "ab"
    );
    let late_failure = ToolResult {
        success: false,
        output: "late delivery failed".into(),
        ..original.clone()
    };
    assert!(
        runtime
            .observe(
                &Event::ToolFinished {
                    result: late_failure
                },
                "worker"
            )
            .is_err()
    );
    let after = runtime.record().unwrap();
    assert_eq!(after.operations.len(), 2);
    assert!(after.operations[1].result.as_ref().unwrap().success);
}

#[tokio::test]
async fn repeated_tool_started_notice_cannot_allocate_another_operation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    let tools = ToolExecutor::new(root.path()).unwrap();
    tools.execute(edit(), &events).await.unwrap();
    for _ in 0..2 {
        events
            .emit(Event::ToolStarted { call: edit() })
            .await
            .unwrap();
    }
    assert_eq!(runtime.record().unwrap().operations.len(), 2);
}

#[tokio::test]
async fn new_host_invocation_may_reuse_source_call_but_changed_request_holds() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    let tools = ToolExecutor::new(root.path()).unwrap();
    assert!(tools.execute(edit(), &events).await.unwrap().success);
    assert!(tools.execute(edit(), &events).await.unwrap().success);
    let next = events.for_invocation(events.begin_model().unwrap());
    assert!(tools.execute(edit(), &next).await.unwrap().success);
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "abb"
    );
    let mut changed = edit();
    changed.arguments["new_text"] = json!("different");
    assert!(
        tools
            .execute(changed, &next)
            .await
            .unwrap_err()
            .to_string()
            .contains("changed")
    );
    assert!(runtime.record().unwrap().recovery_pending);
    assert_eq!(runtime.record().unwrap().operations.len(), 4);
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "abb"
    );
}

struct FailPresentation;
impl ToolHook for FailPresentation {
    fn before(&self, _: &mut ToolCall) -> Result<()> {
        Ok(())
    }
    fn present(&self, _: &ToolResult) -> Result<String> {
        anyhow::bail!("observer failed")
    }
}

#[tokio::test]
async fn failed_observer_preserves_original_and_cannot_replay_the_write() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools.add_hook(Box::new(FailPresentation));
    assert!(
        tools
            .execute(edit(), &events)
            .await
            .unwrap_err()
            .to_string()
            .contains("observer failed")
    );
    assert!(tools.execute(edit(), &events).await.is_err());
    let record = runtime.record().unwrap();
    let operation = record.operations.last().unwrap();
    assert!(operation.complete);
    assert!(operation.result.as_ref().unwrap().success);
    assert_eq!(
        operation
            .tool_receipt
            .as_ref()
            .unwrap()
            .observer_error
            .as_deref(),
        Some("observer failed")
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "ab"
    );
}

#[tokio::test]
async fn captured_request_is_durable_before_before_hook_and_rejected_identity_is_not_executed() {
    struct CheckAndRename(std::path::PathBuf);
    impl ToolHook for CheckAndRename {
        fn before(&self, call: &mut ToolCall) -> Result<()> {
            let record: Record = serde_json::from_value(Store::read_snapshot(&self.0)?).unwrap();
            assert_eq!(
                record
                    .operations
                    .last()
                    .unwrap()
                    .tool_receipt
                    .as_ref()
                    .unwrap()
                    .original_call
                    .id,
                call.id
            );
            call.name = "write".into();
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools.add_hook(Box::new(CheckAndRename(runtime.directory().unwrap())));
    let result = tools.execute(edit(), &events).await.unwrap();
    assert!(!result.success);
    assert_eq!(result.tool, "edit");
    let record = runtime.record().unwrap();
    let receipt = record
        .operations
        .last()
        .unwrap()
        .tool_receipt
        .as_ref()
        .unwrap();
    assert!(!receipt.admitted);
    assert!(!receipt.effect_started);
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "a"
    );
}

#[tokio::test]
async fn pending_duplicate_is_held_without_second_effect_or_observer() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 1);
    events
        .emit(Event::Ready { owner: "fixture" })
        .await
        .unwrap();
    let tools = ToolExecutor::new(root.path()).unwrap();
    let first = tools.execute(edit(), &events);
    tokio::pin!(first);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), &mut first)
            .await
            .is_err()
    );
    assert!(tools.execute(edit(), &events).await.is_err());
    assert_eq!(runtime.record().unwrap().operations.len(), 2);
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "a"
    );
}

#[tokio::test]
async fn unfinished_observers_hold_continuation_without_changing_known_tool_outcome() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 1);
    runtime.finish_model(1).unwrap();
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools.add_hook(Box::new(FailPresentation));
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            tools.execute(edit(), &events)
        )
        .await
        .is_err()
    );
    runtime.finish_phase().unwrap();
    let record = runtime.record().unwrap();
    assert!(record.operations[1].complete);
    assert!(record.operations[1].result.as_ref().unwrap().success);
    assert!(
        record.recovery_pending,
        "unfinished observers must hold continuation independently of the known tool outcome"
    );
}

#[tokio::test]
async fn blocked_ui_does_not_make_an_observer_free_result_unsettled() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 1);
    let tools = ToolExecutor::new(root.path()).unwrap();
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(20),
            tools.execute(edit(), &events)
        )
        .await
        .is_err()
    );
    assert!(tools.execute(edit(), &events).await.unwrap().success);
    assert!(
        runtime.record().unwrap().operations[1]
            .tool_receipt
            .as_ref()
            .unwrap()
            .observers_complete
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "ab"
    );
}

#[tokio::test]
async fn settled_model_result_cannot_be_replaced_when_it_equals_original() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    let tools = ToolExecutor::new(root.path()).unwrap();
    let original = tools.execute(edit(), &events).await.unwrap();
    let mut changed = original.clone();
    changed.output.push_str("invented later feedback");
    assert!(runtime.model_tool_result(2, &changed).is_err());
    assert_eq!(
        runtime.record().unwrap().operations[1].model_result(),
        Some(&original)
    );
}

#[tokio::test]
async fn rejected_transaction_cannot_leak_into_later_success() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    let before = runtime.record().unwrap().operations.len();
    assert!(runtime.begin_tool("verification", 1, &edit()).is_err());
    let tools = ToolExecutor::new(root.path()).unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    assert!(tools.execute(edit(), &events).await.unwrap().success);
    assert_eq!(runtime.record().unwrap().operations.len(), before + 1);
}

#[test]
fn historical_source_id_does_not_authorize_new_tool_execution() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, _, _) = fixture(root.path(), 4);
    runtime
        .update(|record| {
            let mut historical = serde_json::to_value(&record.operations[0])?;
            historical
                .as_object_mut()
                .unwrap()
                .remove("host_invocation");
            record.operations[0] = serde_json::from_value(historical)?;
            Ok(())
        })
        .unwrap();
    assert!(
        runtime.begin_tool("worker", 1, &edit()).is_err(),
        "historical source ID cannot authorize replay"
    );
    assert_eq!(runtime.record().unwrap().operations.len(), 1);
}

#[tokio::test]
async fn publication_failure_keeps_recovery_memory_and_never_invokes_observers() {
    use crate::tools::{AccessPolicy, ToolExtension};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Effect(std::path::PathBuf);
    #[async_trait::async_trait]
    impl ToolExtension for Effect {
        fn definitions(&self) -> Vec<serde_json::Value> {
            vec![json!({"name":"fixture_effect","input_schema":{"type":"object"}})]
        }
        async fn execute(&self, _: &ToolCall, _: &EventSink) -> Result<String> {
            std::fs::write(self.0.join("effect"), "once")?;
            std::fs::write(self.0.join("record/state.json"), "damaged publication")?;
            Ok("effect returned".into())
        }
    }
    struct Observer(Arc<AtomicUsize>);
    impl ToolHook for Observer {
        fn before(&self, _: &mut ToolCall) -> Result<()> {
            Ok(())
        }
        fn present(&self, _: &ToolResult) -> Result<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok("observed".into())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    let seen = Arc::new(AtomicUsize::new(0));
    let access = AccessPolicy {
        extension: Some(Arc::new(Effect(root.path().into()))),
        ..Default::default()
    };
    let mut tools = ToolExecutor::with_policy(root.path(), &access).unwrap();
    tools.add_hook(Box::new(Observer(seen.clone())));
    let call = ToolCall {
        id: "effect".into(),
        name: "fixture_effect".into(),
        arguments: json!({}),
    };
    assert!(
        tools
            .execute(call.clone(), &events)
            .await
            .unwrap_err()
            .to_string()
            .contains("persist")
    );
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert!(tools.take_completed().unwrap().success);
    assert!(runtime.remaining().is_err());
    assert!(tools.execute(call, &events).await.is_err());
    assert_eq!(
        std::fs::read_to_string(root.path().join("effect")).unwrap(),
        "once"
    );
}

#[test]
fn distinct_model_feedback_and_historical_fallback_preserve_original_evidence() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    let (scoped, _) = events.begin_tool(&edit()).unwrap();
    scoped.admit_tool(&edit()).unwrap();
    scoped.tool_effect().unwrap();
    let original = ToolResult {
        call_id: edit().id,
        tool: "edit".into(),
        success: true,
        output: "Edited counter".into(),
        exit_code: None,
    };
    scoped.original_tool_result(&original).unwrap();
    let mut feedback = original.clone();
    feedback.output.push_str("\nLanguage diagnostics: checked");
    scoped.model_tool_result(&feedback).unwrap();
    scoped.settle_tool().unwrap();
    let record: Record =
        serde_json::from_value(Store::read_snapshot(&runtime.directory().unwrap()).unwrap())
            .unwrap();
    let operation = &record.operations[1];
    assert_eq!(operation.result.as_ref(), Some(&original));
    assert_eq!(operation.model_result(), Some(&feedback));
    let (_, replay) = events.begin_tool(&edit()).unwrap();
    assert_eq!(replay, Some(feedback));
    let mut historical = serde_json::to_value(operation).unwrap();
    historical.as_object_mut().unwrap().remove("tool_receipt");
    historical
        .as_object_mut()
        .unwrap()
        .remove("host_invocation");
    let historical: Operation = serde_json::from_value(historical).unwrap();
    assert_eq!(historical.model_result(), Some(&original));
}

#[tokio::test]
async fn interrupted_effect_reopened_from_store_is_never_replayed() {
    use crate::tools::{AccessPolicy, ToolExtension};
    use std::{io::Write, sync::Arc};
    struct Pending(std::path::PathBuf);
    #[async_trait::async_trait]
    impl ToolExtension for Pending {
        fn definitions(&self) -> Vec<serde_json::Value> {
            vec![json!({"name":"fixture_pending","input_schema":{"type":"object"}})]
        }
        async fn execute(&self, _: &ToolCall, _: &EventSink) -> Result<String> {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.0)?
                .write_all(b"x")?;
            std::future::pending().await
        }
    }
    let root = tempfile::tempdir().unwrap();
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    let directory = runtime.directory().unwrap();
    let (sender, _receiver) = mpsc::channel(64);
    let sink = EventSink::new("fixture".into(), sender, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let invocation = sink.begin_model().unwrap();
    sink.finish_model(invocation).unwrap();
    let events = sink.for_invocation(invocation);
    drop(sink);
    let access = AccessPolicy {
        extension: Some(Arc::new(Pending(root.path().join("effect")))),
        ..Default::default()
    };
    let tools = ToolExecutor::with_policy(root.path(), &access).unwrap();
    let call = ToolCall {
        id: "pending".into(),
        name: "fixture_pending".into(),
        arguments: json!({}),
    };
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(30),
            tools.execute(call.clone(), &events)
        )
        .await
        .is_err()
    );
    let operation = runtime.record().unwrap().operations.pop().unwrap();
    assert!(operation.tool_receipt.unwrap().effect_started);
    assert!(operation.result.is_none());
    drop(events);
    drop(runtime);
    let (runtime, resumed) =
        SharedRuntime::open(root.path(), &connection, Some(&directory)).unwrap();
    assert!(resumed);
    assert!(runtime.record().unwrap().recovery_pending);
    let (sender, _receiver) = mpsc::channel(64);
    let events = EventSink::new("fixture".into(), sender, None)
        .unwrap()
        .with_runtime(runtime.clone())
        .for_invocation(invocation);
    assert!(tools.execute(call, &events).await.is_err());
    assert_eq!(
        std::fs::read_to_string(root.path().join("effect")).unwrap(),
        "x"
    );
    assert_eq!(runtime.record().unwrap().operations.len(), 2);
    drop(events);
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn host_command_identity_does_not_spend_model_or_backend_allowances() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 4);
    runtime
        .update(|record| {
            record.allocation = Some(crate::workflow::allocation::Allocation::new(
                Default::default(),
            )?);
            Ok(())
        })
        .unwrap();
    let events = events.for_commands().unwrap();
    let record = runtime.record().unwrap();
    assert_eq!(record.backend_invocations, 0);
    assert_eq!(record.allocation.as_ref().unwrap().model_calls, 0);
    assert_eq!(record.allocation.as_ref().unwrap().tool_calls, 0);
    assert!(matches!(
        record.operations[1].host_invocation,
        Some(HostInvocation::Commands)
    ));
    assert!(events.begin_tool(&edit()).is_ok());
}

#[tokio::test]
async fn original_is_durable_while_real_language_observer_waits_and_feedback_is_replayed() {
    use std::{os::unix::fs::PermissionsExt, process::Stdio};
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("language-server");
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/lsp_fixture.py"),
        &binary,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    use tokio::io::AsyncBufReadExt;
    let mut controller = tokio::process::Command::new("/usr/bin/python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/lsp_control.py"))
        .arg(root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut address = String::new();
    let mut output = tokio::io::BufReader::new(controller.stdout.take().unwrap());
    let received = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        output.read_line(&mut address),
    )
    .await
    .expect("controller startup timed out")
    .expect("controller startup failed");
    assert!(
        received > 0,
        "controller exited before reporting its address"
    );
    std::fs::write(root.path().join(".fixture-control"), address).unwrap();
    std::fs::write(root.path().join(".fixture-mode"), "background-probe").unwrap();
    std::fs::write(
        root.path().join(".fixture-canaries"),
        r#"{"read_paths":{}}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("main.rs"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    let access = crate::tools::AccessPolicy {
        language_servers: crate::language_services::LanguageServers {
            rust: Some(binary),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut tools = ToolExecutor::with_policy(root.path(), &access).unwrap();
    let mut call = edit();
    call.arguments["path"] = json!("main.rs");
    let result = {
        let run = tools.execute(call.clone(), &events);
        tokio::pin!(run);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::select! {
                result = &mut run => panic!("diagnostic observer finished before release: {result:?}"),
                _ = async {
                    while !root.path().join("request-started").exists() {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                } => {}
            }
        }).await.unwrap();
        let record: Record =
            serde_json::from_value(Store::read_snapshot(&runtime.directory().unwrap()).unwrap())
                .unwrap();
        let operation = record.operations.last().unwrap();
        assert!(operation.result.as_ref().unwrap().success);
        assert!(
            !operation
                .result
                .as_ref()
                .unwrap()
                .output
                .contains("Language diagnostics")
        );
        assert!(
            operation
                .tool_receipt
                .as_ref()
                .unwrap()
                .model_result
                .is_none()
        );
        assert!(!operation.tool_receipt.as_ref().unwrap().observers_complete);
        assert_eq!(
            std::fs::read_to_string(root.path().join("main.rs")).unwrap(),
            "ab"
        );
        std::fs::write(root.path().join("release-probe"), "").unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), run)
            .await
            .unwrap()
            .unwrap()
    };
    assert!(result.output.contains("Language diagnostics:"));
    tools.stop_language_services().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), controller.kill())
        .await
        .expect("controller shutdown timed out")
        .unwrap();
    // Removing the configured peer proves the duplicate cannot start it again.
    std::fs::remove_file(root.path().join("language-server")).unwrap();
    assert_eq!(tools.execute(call, &events).await.unwrap(), result);
    let record = runtime.record().unwrap();
    let operation = record.operations.last().unwrap();
    assert!(
        !operation
            .result
            .as_ref()
            .unwrap()
            .output
            .contains("Language diagnostics")
    );
    assert_eq!(operation.model_result(), Some(&result));
    assert_eq!(
        std::fs::read_to_string(root.path().join("main.rs")).unwrap(),
        "ab"
    );
}

#[tokio::test]
async fn exhausted_allocation_refuses_effectful_before_hook() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Before(Arc<AtomicUsize>);
    impl ToolHook for Before {
        fn before(&self, _: &mut ToolCall) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    runtime
        .update(|record| {
            let mut allocation = crate::workflow::allocation::Allocation::new(Default::default())?;
            allocation.tool_calls = allocation.limits.tool_calls;
            record.allocation = Some(allocation);
            Ok(())
        })
        .unwrap();
    let seen = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools.add_hook(Box::new(Before(seen.clone())));
    let _ = tools.execute(edit(), &events).await;
    assert_eq!(
        seen.load(Ordering::SeqCst),
        0,
        "an exhausted allowance must not start effectful gates"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_distinct_calls_reserve_allowance_before_effectful_gates() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Before {
        seen: Arc<AtomicUsize>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl ToolHook for Before {
        fn before(&self, _: &mut ToolCall) -> Result<()> {
            if self.seen.fetch_add(1, Ordering::SeqCst) == 0 {
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .context("test coordinator did not release first gate")?;
            }
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    runtime
        .update(|record| {
            record.allocation = Some(crate::workflow::allocation::Allocation::new(
                crate::workflow::allocation::Limits {
                    tool_calls: 1,
                    ..Default::default()
                },
            )?);
            Ok(())
        })
        .unwrap();
    let seen = Arc::new(AtomicUsize::new(0));
    let (release, released) = std::sync::mpsc::channel();
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools.add_hook(Box::new(Before {
        seen: seen.clone(),
        release: Mutex::new(released),
    }));
    let tools = Arc::new(tools);
    // JoinSet aborts the peer on assertion failure; dropping release wakes its
    // bounded synchronous receive even if the coordinator exits early.
    let mut running = tokio::task::JoinSet::new();
    {
        let tools = tools.clone();
        let events = events.clone();
        running.spawn(async move { tools.execute(edit(), &events).await });
    }
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while seen.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("first gate did not arrive");
    let mut second = edit();
    second.id = "different-source".into();
    let second = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tools.execute(second, &events),
    )
    .await;
    release.send(()).expect("first gate exited before release");
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), running.join_next())
        .await
        .expect("first call did not finish")
        .unwrap()
        .unwrap();
    assert_eq!(
        seen.load(Ordering::SeqCst),
        1,
        "a gate must own a reserved allowance before it runs"
    );
    assert!(first.unwrap().success);
    assert!(!second.unwrap().unwrap().success);
    assert_eq!(runtime.record().unwrap().allocation.unwrap().tool_calls, 1);
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "ab"
    );
}

#[tokio::test]
async fn final_admission_rejection_never_runs_presentation_observers() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Expire {
        runtime: SharedRuntime,
        seen: Arc<AtomicUsize>,
    }
    impl ToolHook for Expire {
        fn before(&self, _: &mut ToolCall) -> Result<()> {
            self.runtime.update(|record| {
                record.allocation.as_mut().unwrap().deadline_ms = 0;
                Ok(())
            })
        }
        fn present(&self, _: &ToolResult) -> Result<String> {
            self.seen.fetch_add(1, Ordering::SeqCst);
            Ok("observer effect".into())
        }
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    runtime
        .update(|record| {
            record.allocation = Some(crate::workflow::allocation::Allocation::new(
                Default::default(),
            )?);
            Ok(())
        })
        .unwrap();
    let seen = Arc::new(AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools.add_hook(Box::new(Expire {
        runtime: runtime.clone(),
        seen: seen.clone(),
    }));
    let _ = tools.execute(edit(), &events).await;
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "a"
    );
    assert_eq!(runtime.record().unwrap().allocation.unwrap().tool_calls, 1);
}

struct CountPresent(std::sync::Arc<std::sync::atomic::AtomicUsize>);
impl ToolHook for CountPresent {
    fn before(&self, _: &mut ToolCall) -> Result<()> {
        Ok(())
    }
    fn present(&self, _: &ToolResult) -> Result<String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok("observer".into())
    }
}

#[tokio::test]
async fn forbidden_paths_never_run_presentation() {
    for path in [".git/config", "../outside"] {
        let root = tempfile::tempdir().unwrap();
        let (_, events, _receiver) = fixture(root.path(), 64);
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut tools = ToolExecutor::new(root.path()).unwrap();
        tools.add_hook(Box::new(CountPresent(count.clone())));
        let result = tools
            .execute(
                ToolCall {
                    id: "forbidden".into(),
                    name: "write".into(),
                    arguments: json!({"path":path,"content":"forbidden"}),
                },
                &events,
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn blocked_start_rechecks_deadline_and_hold_before_create_or_write() {
    for existing in [false, true] {
        for interruption in ["hold", "deadline", "child-cancel"] {
            let root = tempfile::tempdir().unwrap();
            if existing {
                std::fs::write(root.path().join("target"), "original").unwrap();
            }
            let (runtime, mut events, mut receiver) = fixture(root.path(), 1);
            if interruption == "child-cancel" {
                runtime
                    .update(|record| {
                        record.agents.push(crate::inspection::tests::agent(
                            1,
                            crate::subagents::state::AgentStatus::Running,
                            &record.identity,
                        ));
                        Ok(())
                    })
                    .unwrap();
                events = events.for_phase("agent:1:worker");
                let invocation = events.begin_model().unwrap();
                events = events.for_invocation(invocation);
            }
            runtime
                .update(|record| {
                    record.allocation = Some(crate::workflow::allocation::Allocation::new(
                        Default::default(),
                    )?);
                    Ok(())
                })
                .unwrap();
            events
                .emit(Event::ToolPresentation {
                    call_id: "unrelated".into(),
                    text: "occupy channel".into(),
                })
                .await
                .unwrap();
            let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut tools = ToolExecutor::new(root.path()).unwrap();
            tools.add_hook(Box::new(CountPresent(count.clone())));
            let call = ToolCall {
                id: "blocked".into(),
                name: "write".into(),
                arguments: json!({"path":"target","content":"changed"}),
            };
            let future = tools.execute(call, &events);
            tokio::pin!(future);
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(20), &mut future)
                    .await
                    .is_err()
            );
            runtime
                .update(|record| {
                    if interruption == "deadline" {
                        record.allocation.as_mut().unwrap().deadline_ms = 0;
                    } else if interruption == "child-cancel" {
                        record.agents[0].status = crate::subagents::state::AgentStatus::Cancelled;
                    } else {
                        record.recovery_pending = true;
                    }
                    Ok(())
                })
                .unwrap();
            let drain = tokio::spawn(async move { while receiver.recv().await.is_some() {} });
            let result = tokio::time::timeout(std::time::Duration::from_secs(2), &mut future)
                .await
                .unwrap()
                .unwrap();
            assert!(!result.success);
            assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
            if existing {
                assert_eq!(
                    std::fs::read_to_string(root.path().join("target")).unwrap(),
                    "original"
                );
            } else {
                assert!(!root.path().join("target").exists());
            }
            drain.abort();
        }
    }
}

#[tokio::test]
async fn oracle_denial_and_hold_during_review_prevent_effect_and_presentation() {
    for mode in ["deny", "hold"] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("new-target");
        std::fs::write(root.path().join("oracle-mode"), mode).unwrap();
        let (runtime, events, mut receiver) = fixture(root.path(), 64);
        let connection: Connection=serde_json::from_value(json!({"adapter":"claude","binary":concat!(env!("CARGO_MANIFEST_DIR"),"/tests/oracle_fixture.py")})).unwrap();
        let access = crate::tools::AccessPolicy {
            unrestricted: true,
            oracle: Some(Box::new(connection)),
            ..Default::default()
        };
        let mut tools = ToolExecutor::with_policy(root.path(), &access).unwrap();
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        tools.add_hook(Box::new(CountPresent(count.clone())));
        let drain = tokio::spawn(async move { while receiver.recv().await.is_some() {} });
        let call = ToolCall {
            id: "oracle-proposal".into(),
            name: "write".into(),
            arguments: json!({"path":target,"content":"forbidden"}),
        };
        let run = tools.execute(call, &events);
        tokio::pin!(run);
        if mode == "hold" {
            let marker = root.path().join("oracle-request.json");
            let wait = async {
                while !marker.exists() {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            };
            tokio::select! { result=&mut run=>panic!("review ended before hold: {result:?}"), result=tokio::time::timeout(std::time::Duration::from_secs(5),wait)=>result.unwrap() }
            runtime
                .update(|record| {
                    record.recovery_pending = true;
                    Ok(())
                })
                .unwrap();
            std::fs::write(root.path().join("oracle-release"), "").unwrap();
        }
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), &mut run)
            .await
            .unwrap()
            .unwrap();
        assert!(!result.success, "{result:?}");
        if mode == "deny" {
            assert!(
                result.output.contains("Oracle blocked"),
                "{}",
                result.output
            );
        }
        assert!(!target.exists());
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
        drain.abort();
    }
}

thread_local! {
    static INVALIDATE_OBSERVER_CHECKPOINT: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn invalidate_observer_checkpoint(record: &mut Record) {
    let armed = INVALIDATE_OBSERVER_CHECKPOINT.with(|target| {
        let mut target = target.borrow_mut();
        if target.as_ref() == Some(&record.workspace) {
            target.take();
            true
        } else {
            false
        }
    });
    if armed {
        // Same rollback simulation as the runtime admission regression: the
        // initial check succeeded, but the subsequent checkpoint must fail.
        record.allocation.as_mut().unwrap().observed_ms += 60_000;
    }
}

#[tokio::test]
async fn observer_checkpoint_clock_failure_withholds_presentation_and_retains_outcomes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("counter"), "a").unwrap();
    let (runtime, events, _receiver) = fixture(root.path(), 64);
    runtime
        .update(|record| {
            record.allocation = Some(crate::workflow::allocation::Allocation::new(
                Default::default(),
            )?);
            Ok(())
        })
        .unwrap();
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools.add_hook(Box::new(CountPresent(count.clone())));
    INVALIDATE_OBSERVER_CHECKPOINT.set(Some(root.path().to_path_buf()));
    let result = tools.execute(edit(), &events).await;
    INVALIDATE_OBSERVER_CHECKPOINT.set(None);
    assert!(
        result.is_err(),
        "invalid checkpoint must withhold observer admission"
    );
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
    let record = runtime.record().unwrap();
    assert!(record.allocation.unwrap().clock_invalid);
    let operation = record.operations.last().unwrap();
    assert!(operation.result.as_ref().unwrap().success);
    assert_eq!(
        std::fs::read_to_string(root.path().join("counter")).unwrap(),
        "ab"
    );
    assert_eq!(
        operation.tool_receipt.as_ref().unwrap().observer_pending,
        Some(0)
    );
    // Completion evidence remains writable after clock invalidation.
    runtime
        .tool_observer(
            operation.id,
            0,
            Some(Err("observer withheld by clock checkpoint")),
        )
        .unwrap();
    let saved: Record =
        serde_json::from_value(Store::read_snapshot(&runtime.directory().unwrap()).unwrap())
            .unwrap();
    assert!(saved.allocation.unwrap().clock_invalid);
    assert!(
        saved
            .operations
            .last()
            .unwrap()
            .tool_receipt
            .as_ref()
            .unwrap()
            .observer_error
            .is_some()
    );
}

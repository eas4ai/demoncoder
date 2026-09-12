use super::*;
#[test]
fn serialized_response_reserves_transport_newline() -> Result<()> {
    let value = json!({"context":"x".repeat(65521)});
    assert_eq!(response_bytes(&value)?.len(), 65535);
    assert!(response_bytes(&json!({"context":"x".repeat(65522)})).is_err());
    Ok(())
}

use crate::plugins::{
    dispatch::*,
    gate_snapshot::GateReadSet,
    hook_types::{HandlerKind, HookDialect},
};
use crate::{
    plugins::{hook_types::HookEvent, non_tool::NonToolPlan},
    workflow::runtime::SharedRuntime,
};
use std::sync::Arc;
struct Allow;

#[async_trait::async_trait]
impl HookRunner for Allow {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        Ok(RawOutcome::Callback { value: json!({}) })
    }
}
fn registration(event: HookEvent) -> Registration {
    Registration {
        declaration: Declaration {
            required_gate: true,
            source: None,
            once: None,
            identity: DeclarationIdentity {
                package: "ordinary-test".into(),
                code: "code".into(),
                policy: "policy".into(),
                configuration: "config".into(),
                generation: "1".into(),
                scope: Scope::Project,
                role: "worker".into(),
                declaration: event.as_str().into(),
                index: 0,
                dialect: HookDialect::Native,
                runner: HandlerKind::Command,
            },
            class: HandlerClass::Combined,
            priority: 0,
            matcher: Matcher::default(),
            reads: GateReadSet::default(),
            concurrent_group: None,
            read_only_endpoint: None,
            external_precondition: None,
        },
        runner: Arc::new(Allow),
        revalidation: None,
    }
}

struct Fixture {
    root: tempfile::TempDir,
    _state: tempfile::TempDir,
    _receiver: tokio::sync::mpsc::Receiver<crate::events::Envelope>,
    runtime: SharedRuntime,
    events: EventSink,
    tools: ToolExecutor,
    callbacks: Callbacks,
}
impl Fixture {
    fn new() -> Result<Self> {
        Self::with_stop(false)
    }
    fn with_stop(correct: bool) -> Result<Self> {
        let root = tempfile::tempdir()?;
        let state = tempfile::tempdir()?;
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        record.task = Some(crate::workflow::state::Task::new(
            1,
            "prompt".into(),
            vec![],
            crate::workflow::workspace::capture(root.path())?,
            1,
        )?);
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record)?;
        runtime.allocate(crate::workflow::allocation::Limits::default(), None)?;
        let (tx, receiver) = tokio::sync::mpsc::channel(256);
        let events = EventSink::new("ordinary".into(), tx, None)?.with_runtime(runtime.clone());
        let events = events.for_invocation(events.begin_backend()?);
        let mut tools = ToolExecutor::new(root.path())?;
        for event in [HookEvent::UserPromptSubmit, HookEvent::Stop] {
            let mut registration = registration(event);
            if correct && event == HookEvent::Stop {
                registration.runner = Arc::new(Correct);
            }
            tools.register_non_tool_plan(Arc::new(NonToolPlan::new(event, vec![registration])?))?;
        }
        let mut callbacks = Callbacks::new(
            root.path().into(),
            root.path().join("private.json"),
            "session".into(),
            "/source/transcript".into(),
            Some("observed-model".into()),
        )?;
        callbacks.begin(&json!({"id":42,"method":"turn/start","params":{"threadId":"session","input":[{"type":"text","text":"prompt"}]}}), SourceOrigin::HostSubmission, &events)?;
        callbacks.observe(
            &json!({"method":"turn/started","params":{"threadId":"session","turn":{"id":"turn"}}}),
            &events,
        )?;
        Ok(Self {
            root,
            _state: state,
            _receiver: receiver,
            runtime,
            events,
            tools,
            callbacks,
        })
    }
    fn frame(&self, event: &str, completed: bool) -> Value {
        json!({"method":if completed { "hook/completed" } else { "hook/started" }, "params":{"threadId":"session","turnId":"turn","run":{"id":format!("{}:{}:{}", if event == "stop" { "stop" } else { "user-prompt-submit" }, if event == "stop" { 1 } else { 0 }, self.callbacks.source.display()),"eventName":event,"handlerType":"command","executionMode":"sync","scope":"turn","sourcePath":self.callbacks.source,"source":"sessionFlags","displayOrder":if event == "stop" { 1 } else { 0 },"status":if completed { "completed" } else { "running" }}}})
    }
    fn input(&self, event: &str, delivery: u64) -> Value {
        let mut input = json!({"session_id":"session","turn_id":"turn","transcript_path":"/source/transcript","cwd":self.root.path(),"model":"observed-model","permission_mode":"default","hook_event_name":event,"demonCoderOrdinary":{"protocol":"demoncoder-ordinary-v1","delivery_id":format!("00000000-0000-4000-8000-{delivery:012}"),"hook_event_name":event,"session_id":"session","turn_id":"turn"}});
        if event == "UserPromptSubmit" {
            input["prompt"] = "prompt".into();
        } else {
            input["stop_hook_active"] = false.into();
            input["last_assistant_message"] = "done".into();
        }
        input
    }
    async fn submit(&mut self) -> Result<()> {
        self.callbacks
            .observe(&self.frame("userPromptSubmit", false), &self.events)?;
        self.callbacks
            .handle(self.input("UserPromptSubmit", 1), &self.events, &self.tools)
            .await?;
        self.callbacks.sent(&self.events)?;
        self.callbacks
            .observe(&self.frame("userPromptSubmit", true), &self.events)
    }
    fn assistant(&mut self) -> Result<()> {
        for method in ["item/started", "item/completed"] {
            self.callbacks.observe(&json!({"method":method,"params":{"threadId":"session","turnId":"turn","item":{"type":"agentMessage","id":"message","text":"done"}}}), &self.events)?;
        }
        Ok(())
    }
}
#[tokio::test]
async fn actual_native_run_is_required_before_private_callback() -> Result<()> {
    let mut f = Fixture::new()?;
    assert!(
        f.callbacks
            .handle(f.input("UserPromptSubmit", 1), &f.events, &f.tools)
            .await
            .is_err()
    );
    assert_eq!(f.runtime.record()?.operations.len(), 1);
    Ok(())
}
#[tokio::test]
async fn callback_preserves_source_owner_and_requires_exact_completion() -> Result<()> {
    let mut f = Fixture::new()?;
    f.callbacks
        .observe(&f.frame("userPromptSubmit", false), &f.events)?;
    let input = f.input("UserPromptSubmit", 1);
    let output = f
        .callbacks
        .handle(input.clone(), &f.events, &f.tools)
        .await?;
    assert_eq!(output["demonCoderOrdinary"], input["demonCoderOrdinary"]);
    assert!(
        f.callbacks
            .handle(input, &f.events, &f.tools)
            .await
            .is_err()
    );
    assert!(f.assistant().is_err());
    let mut wrong = f.frame("userPromptSubmit", true);
    wrong["params"]["run"]["id"] = "other".into();
    f.callbacks.sent(&f.events)?;
    assert!(f.callbacks.observe(&wrong, &f.events).is_err());
    f.callbacks
        .observe(&f.frame("userPromptSubmit", true), &f.events)?;
    let record = f.runtime.record()?;
    let receipt = record
        .operations
        .iter()
        .find_map(|o| o.non_tool_receipt())
        .unwrap();
    assert_eq!(receipt.source_delivery, Some(SourceDelivery::Acknowledged));
    let source = receipt.facts.callback.as_ref().unwrap();
    assert_eq!(
        source.request_id,
        format!("user-prompt-submit:0:{}", f.callbacks.source.display())
    );
    assert_eq!(source.command_request_id, Some(42));
    assert!(source.command_uuid.is_none());
    let ObservedLifecycle::Codex(input) = receipt.facts.source.as_ref().unwrap() else {
        panic!()
    };
    assert!(input.get("demonCoderOrdinary").is_none());
    Ok(())
}
#[tokio::test]
async fn private_callback_cannot_forge_source_fields_or_envelope() -> Result<()> {
    for path in [
        "session_id",
        "turn_id",
        "cwd",
        "transcript_path",
        "prompt",
        "hook_event_name",
        "demonCoderOrdinary",
    ] {
        let mut f = Fixture::new()?;
        f.callbacks
            .observe(&f.frame("userPromptSubmit", false), &f.events)?;
        let mut input = f.input("UserPromptSubmit", 1);
        input[path] = "wrong".into();
        assert!(
            f.callbacks
                .handle(input, &f.events, &f.tools)
                .await
                .is_err(),
            "{path}"
        );
        assert_eq!(f.runtime.record()?.operations.len(), 1);
    }
    Ok(())
}
#[tokio::test]
async fn stop_cannot_forge_assistant_text_or_correction_state() -> Result<()> {
    for field in ["last_assistant_message", "stop_hook_active"] {
        let mut f = Fixture::new()?;
        f.submit().await?;
        f.assistant()?;
        f.callbacks.observe(&f.frame("stop", false), &f.events)?;
        let mut input = f.input("Stop", 2);
        input[field] = if field == "stop_hook_active" {
            true.into()
        } else {
            "forged".into()
        };
        assert!(
            f.callbacks
                .handle(input, &f.events, &f.tools)
                .await
                .is_err()
        );
    }
    Ok(())
}

#[tokio::test]
async fn native_hook_start_must_match_the_exact_private_registration() -> Result<()> {
    for field in ["id", "displayOrder"] {
        let mut f = Fixture::new()?;
        let mut start = f.frame("userPromptSubmit", false);
        start["params"]["run"][field] = if field == "displayOrder" {
            99.into()
        } else {
            "unrelated-run".into()
        };
        assert!(f.callbacks.observe(&start, &f.events).is_err(), "{field}");
    }
    Ok(())
}

struct Correct;
#[async_trait::async_trait]
impl HookRunner for Correct {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        Ok(RawOutcome::Callback {
            value: json!({"decision":"block","reason":"correct the response"}),
        })
    }
}
#[tokio::test]
async fn repeated_native_stop_id_requires_new_delivery_and_spends_original_budget() -> Result<()> {
    let mut f = Fixture::with_stop(true)?;
    f.submit().await?;
    f.assistant()?;
    f.callbacks.observe(&f.frame("stop", false), &f.events)?;
    let first = f.input("Stop", 2);
    let response = f
        .callbacks
        .handle(first.clone(), &f.events, &f.tools)
        .await?;
    assert_eq!(response["continue"], true);
    assert_eq!(response["decision"], "block");
    assert_eq!(f.runtime.record()?.task.as_ref().unwrap().corrections, 1);
    f.callbacks.sent(&f.events)?;
    let mut completed = f.frame("stop", true);
    completed["params"]["run"]["status"] = "blocked".into();
    f.callbacks.observe(&completed, &f.events)?;
    f.assistant()?;
    f.callbacks.observe(&f.frame("stop", false), &f.events)?;
    assert!(
        f.callbacks
            .handle(first, &f.events, &f.tools)
            .await
            .is_err()
    );
    let mut next = f.input("Stop", 3);
    next["stop_hook_active"] = true.into();
    let response = f.callbacks.handle(next, &f.events, &f.tools).await?;
    assert_eq!(response["continue"], false);
    assert!(f.callbacks.sent(&f.events).is_err());
    let record = f.runtime.record()?;
    assert_eq!(record.task.as_ref().unwrap().corrections, 1);
    let receipts: Vec<_> = record
        .operations
        .iter()
        .filter_map(|o| o.non_tool_receipt())
        .collect();
    assert_eq!(receipts.len(), 3);
    let first = receipts[1].facts.callback.as_ref().unwrap();
    let next = receipts[2].facts.callback.as_ref().unwrap();
    assert_eq!(first.request_id, next.request_id);
    assert_ne!(first.envelope_id, next.envelope_id);
    assert_eq!(first.sequence + 1, next.sequence);
    assert_eq!(first.backend_operation, next.backend_operation);
    Ok(())
}

#[tokio::test]
async fn source_stop_without_assistant_text_is_an_observed_ordinary_occurrence() -> Result<()> {
    let mut f = Fixture::new()?;
    f.submit().await?;
    f.callbacks.observe(&f.frame("stop", false), &f.events)?;
    let mut input = f.input("Stop", 2);
    input["last_assistant_message"] = Value::Null;
    let response = f.callbacks.handle(input, &f.events, &f.tools).await?;
    assert_eq!(response["continue"], true);
    f.callbacks.sent(&f.events)?;
    f.callbacks.observe(&f.frame("stop", true), &f.events)?;
    let record = f.runtime.record()?;
    let stop = record
        .operations
        .iter()
        .filter_map(|o| o.non_tool_receipt())
        .next_back()
        .unwrap();
    assert!(matches!(
        stop.facts.subject.occurrence,
        NonToolOccurrence::Stop {
            last_assistant_message: None,
            ..
        }
    ));
    assert_eq!(stop.source_delivery, Some(SourceDelivery::Acknowledged));
    Ok(())
}
#[tokio::test]
async fn unregistered_source_callback_retains_an_empty_owned_observation() -> Result<()> {
    let mut f = Fixture::new()?;
    f.tools = ToolExecutor::new(f.root.path())?;
    f.callbacks
        .observe(&f.frame("userPromptSubmit", false), &f.events)?;
    f.callbacks
        .handle(f.input("UserPromptSubmit", 1), &f.events, &f.tools)
        .await?;
    let record = f.runtime.record()?;
    let receipt = record
        .operations
        .iter()
        .find_map(|o| o.non_tool_receipt())
        .expect("actual callback needs durable ownership even with no handlers");
    assert!(receipt.hooks.is_empty() && receipt.declarations.is_empty() && receipt.settled);
    assert_eq!(receipt.source_delivery, Some(SourceDelivery::Pending));
    f.callbacks.sent(&f.events)?;
    f.callbacks
        .observe(&f.frame("userPromptSubmit", true), &f.events)?;
    Ok(())
}
#[tokio::test]
async fn unregistered_source_callback_cannot_release_a_cancelled_owner() -> Result<()> {
    let mut f = Fixture::new()?;
    f.tools = ToolExecutor::new(f.root.path())?;
    f.callbacks
        .observe(&f.frame("userPromptSubmit", false), &f.events)?;
    f.runtime.finish_phase()?;
    assert!(
        f.callbacks
            .handle(f.input("UserPromptSubmit", 1), &f.events, &f.tools)
            .await
            .is_err(),
        "no allow response may be returned after owner cancellation"
    );
    Ok(())
}

#[tokio::test]
async fn unregistered_native_event_remains_a_no_op() -> Result<()> {
    let f = Fixture::new()?;
    let tools = ToolExecutor::new(f.root.path())?;
    assert!(
        tools
            .dispatch_non_tool(
                NonToolOccurrence::UserPromptSubmit {
                    prompt: "prompt".into(),
                    correction: false
                },
                &f.events
            )
            .await?
            .is_none()
    );
    assert_eq!(f.runtime.record()?.operations.len(), 1);
    Ok(())
}

#[tokio::test]
async fn pending_release_is_revalidated_before_transport_write() -> Result<()> {
    for no_handler in [false, true] {
        let mut f = Fixture::new()?;
        if no_handler {
            f.tools = ToolExecutor::new(f.root.path())?;
        }
        f.callbacks
            .observe(&f.frame("userPromptSubmit", false), &f.events)?;
        f.callbacks
            .handle(f.input("UserPromptSubmit", 1), &f.events, &f.tools)
            .await?;
        f.callbacks.prepare(&f.events)?;
        f.runtime.finish_phase()?;
        assert!(f.callbacks.prepare(&f.events).is_err());
        let record = f.runtime.record()?;
        let receipt = record
            .operations
            .iter()
            .find_map(|o| o.non_tool_receipt())
            .unwrap();
        assert_eq!(receipt.source_delivery, Some(SourceDelivery::Pending));
    }
    Ok(())
}

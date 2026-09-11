use super::*;
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
        Self::with_once(false)
    }
    fn with_once(once: bool) -> Result<Self> {
        Self::with_task(once, false)
    }
    fn with_task(once: bool, task: bool) -> Result<Self> {
        let root = tempfile::tempdir()?;
        let state = tempfile::tempdir()?;
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = Some("worker".into());
        if task {
            record.task = Some(crate::workflow::state::Task::new(
                1,
                "prompt".into(),
                vec![],
                crate::workflow::workspace::capture(root.path())?,
                1,
            )?);
        }
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record)?;
        if task {
            runtime.allocate(crate::workflow::allocation::Limits::default(), None)?;
        }
        let (tx, receiver) = tokio::sync::mpsc::channel(256);
        let events = EventSink::new("ordinary".into(), tx, None)?.with_runtime(runtime.clone());
        let events = events.for_invocation(events.begin_backend()?);
        let mut tools = ToolExecutor::new(root.path())?;
        for event in [HookEvent::UserPromptSubmit, HookEvent::Stop] {
            let mut registration = registration(event);
            if once {
                use crate::plugins::once::{ActivationChange, ActivationSource, HookOrigin};
                let binding = runtime
                    .plugin_hook_activation(
                        HookOrigin::Native,
                        Scope::Project,
                        &ActivationSource::host_namespace("ordinary-test")?,
                        event.as_str(),
                        "worker",
                        ActivationChange::ExplicitInvocation,
                    )?
                    .unwrap();
                registration.declaration.source = Some(binding.source());
                registration.declaration.once = Some(binding);
            }
            tools.register_non_tool_plan(Arc::new(NonToolPlan::new(event, vec![registration])?))?;
        }
        let mut callbacks = Callbacks::new(root.path().into(), Some("observed-model".into()))?;
        callbacks.begin(
            &json!({"uuid":"command","message":{"content":"prompt"}}),
            SourceOrigin::HostSubmission,
            &events,
        )?;
        for state in ["queued", "started"] {
            callbacks.observe(&json!({"type":"command_lifecycle","command_uuid":"command","state":state,"session_id":"source-session"}), &events)?;
        }
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
    fn message(&self, event: &str, id: &str) -> Value {
        let mut input = json!({"hook_event_name":event,"session_id":"source-session","cwd":self.root.path(),"transcript_path":self.root.path().join("source.jsonl"),"prompt_id":"source-prompt","permission_mode":"default"});
        if event == "UserPromptSubmit" {
            input["prompt"] = json!("prompt");
        } else {
            input["stop_hook_active"] = json!(false);
            input["last_assistant_message"] = json!("done");
        }
        json!({"type":"control_request","request_id":id,"request":{"subtype":"hook_callback","callback_id":self.callbacks.registration(Value::Null)[event][0]["hookCallbackIds"][0],"tool_use_id":format!("outer-{id}"),"input":input}})
    }
    async fn submit(&mut self) -> Result<()> {
        self.callbacks
            .handle(
                &self.message("UserPromptSubmit", "submit"),
                Some("source-session"),
                &self.events,
                &self.tools,
            )
            .await?;
        self.callbacks.sent(&self.events)
    }
    fn response(&mut self, command: &str) -> Result<()> {
        self.callbacks.observe(&json!({"type":"stream_event","user_message_uuid":command,"event":{"type":"message_start"}}), &self.events)?;
        for event in [
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_stop"}),
        ] {
            self.callbacks
                .observe(&json!({"type":"stream_event","event":event}), &self.events)?;
        }
        Ok(())
    }
}

#[tokio::test]
async fn source_stop_continuation_may_omit_command_identity_only_once() -> Result<()> {
    let mut f = Fixture::with_task(false, true)?;
    f.submit().await?;
    let missing = json!({"type":"stream_event","event":{"type":"message_start"}});
    assert!(f.callbacks.observe(&missing, &f.events).is_err());
    f.response("command")?;
    let mut tools = ToolExecutor::new(f.root.path())?;
    let mut gate = registration(HookEvent::Stop);
    gate.runner = Arc::new(Correct);
    tools.register_non_tool_plan(Arc::new(NonToolPlan::new(HookEvent::Stop, vec![gate])?))?;
    let response = f
        .callbacks
        .handle(
            &f.message("Stop", "correct"),
            Some("source-session"),
            &f.events,
            &tools,
        )
        .await?;
    assert_eq!(response["decision"], "block");
    assert!(f.callbacks.awaiting_correction);
    f.callbacks.sent(&f.events)?;
    let mut wrong = missing.clone();
    wrong["user_message_uuid"] = json!("another-command");
    assert!(f.callbacks.observe(&wrong, &f.events).is_err());
    wrong["user_message_uuid"] = Value::Null;
    assert!(f.callbacks.observe(&wrong, &f.events).is_err());
    let mut plural = missing.clone();
    plural["user_message_uuids"] = json!(["another-command"]);
    assert!(f.callbacks.observe(&plural, &f.events).is_err());
    f.callbacks.observe(&missing, &f.events)?;
    assert!(!f.callbacks.awaiting_correction);
    assert!(f.callbacks.pending.is_none());
    f.callbacks.observe(
        &json!({"type":"stream_event","event":{"type":"message_stop"}}),
        &f.events,
    )?;
    assert!(f.callbacks.observe(&missing, &f.events).is_err());
    let mut stop = f.message("Stop", "accepted");
    stop["request"]["input"]["stop_hook_active"] = json!(true);
    stop["request"]["input"]
        .as_object_mut()
        .unwrap()
        .remove("last_assistant_message");
    f.callbacks
        .handle(&stop, Some("source-session"), &f.events, &f.tools)
        .await?;
    f.callbacks.sent(&f.events)?;
    assert!(f.callbacks.accepted);
    assert!(
        f.callbacks
            .observe(&json!({"type":"result"}), &f.events)
            .is_err()
    );
    f.callbacks.observe(
        &json!({"type":"result","user_message_uuid":"command"}),
        &f.events,
    )?;
    Ok(())
}

#[tokio::test]
async fn malformed_forged_and_stale_callbacks_reserve_no_effects() -> Result<()> {
    for attack in [
        "nonce",
        "event",
        "session",
        "workspace",
        "prompt",
        "prompt-id",
        "envelope",
        "request",
        "backend",
        "stop-first",
        "oversize",
    ] {
        let mut f = Fixture::new()?;
        let mut message = f.message("UserPromptSubmit", "submit");
        let mut events = f.events.clone();
        match attack {
            "nonce" => message["request"]["callback_id"] = json!("forged"),
            "event" => message["request"]["input"]["hook_event_name"] = json!("PreCompact"),
            "session" => message["request"]["input"]["session_id"] = json!("sibling"),
            "workspace" => message["request"]["input"]["cwd"] = json!("/other"),
            "prompt" => message["request"]["input"]["prompt"] = json!("forged"),
            "prompt-id" => message["request"]["input"]["prompt_id"] = Value::Null,
            "envelope" => message["request"]["tool_use_id"] = Value::Null,
            "request" => message["request_id"] = Value::Null,
            "backend" => events = events.for_invocation(Some(999)),
            "stop-first" => message = f.message("Stop", "stop"),
            "oversize" => message["extra"] = json!("x".repeat(1024 * 1024)),
            _ => unreachable!(),
        }
        assert!(
            f.callbacks
                .handle(&message, Some("source-session"), &events, &f.tools)
                .await
                .is_err(),
            "{attack}"
        );
        assert!(
            f.runtime
                .record()?
                .operations
                .iter()
                .all(|o| o.non_tool_receipt().is_none()),
            "{attack}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn callback_delivery_stays_uncertain_until_actual_correlated_continuation() -> Result<()> {
    let mut f = Fixture::new()?;
    f.submit().await?;
    let receipt = f
        .runtime
        .record()?
        .operations
        .last()
        .unwrap()
        .non_tool_receipt()
        .unwrap()
        .clone();
    assert!(receipt.settled);
    assert_eq!(receipt.source_delivery, Some(SourceDelivery::Sent));
    assert!(f.callbacks.observe(&json!({"type":"stream_event","user_message_uuid":"sibling-command","event":{"type":"message_start"}}), &f.events).is_err(), "another command cannot acknowledge this callback");
    Ok(())
}

#[tokio::test]
async fn repeated_callbacks_and_contradictory_stop_evidence_are_held() -> Result<()> {
    let mut f = Fixture::new()?;
    f.submit().await?;
    f.response("command")?;
    let original = f.message("UserPromptSubmit", "submit");
    assert!(
        f.callbacks
            .handle(&original, Some("source-session"), &f.events, &f.tools)
            .await
            .is_err()
    );
    let mut changed = original;
    changed["request"]["input"]["prompt"] = json!("changed");
    assert!(
        f.callbacks
            .handle(&changed, Some("source-session"), &f.events, &f.tools)
            .await
            .is_err()
    );
    let mut stop = f.message("Stop", "stop");
    stop["request"]["input"]["last_assistant_message"] = json!("forged text");
    assert!(
        f.callbacks
            .handle(&stop, Some("source-session"), &f.events, &f.tools)
            .await
            .is_err(),
        "Stop text must match actual assistant evidence"
    );
    Ok(())
}

#[tokio::test]
async fn uncertain_source_delivery_blocks_another_backend() -> Result<()> {
    let mut f = Fixture::new()?;
    f.submit().await?;
    let before = f.runtime.record()?.operations.len();
    assert!(
        f.events.begin_backend().is_err(),
        "uncertain source continuation must fence new work"
    );
    assert_eq!(f.runtime.record()?.operations.len(), before);
    Ok(())
}

#[tokio::test]
async fn source_delivery_recovery_retains_settled_effects_without_replay() -> Result<()> {
    let mut f = Fixture::with_once(true)?;
    f.submit().await?;
    f.runtime.finish_phase()?;
    let retained = f.runtime.record()?;
    assert!(retained.recovery_pending);
    let receipt = retained
        .operations
        .last()
        .unwrap()
        .non_tool_receipt()
        .unwrap();
    assert!(receipt.settled && receipt.hooks.iter().all(|h| h.outcome.is_some()));
    assert_eq!(
        receipt.hooks[0].once.as_ref().unwrap().state,
        crate::plugins::once::OnceState::Succeeded
    );
    assert_eq!(receipt.source_delivery, Some(SourceDelivery::Sent));
    let reopened = SharedRuntime::for_test(
        &f._state.path().join("reopened"),
        serde_json::from_value(serde_json::to_value(&retained)?)?,
    )?;
    assert!(reopened.begin_phase("worker", None).is_err());
    assert!(
        f.callbacks
            .handle(
                &f.message("Stop", "stopped"),
                Some("source-session"),
                &f.events,
                &f.tools
            )
            .await
            .is_err()
    );
    assert_eq!(
        reopened.record()?.operations.len(),
        retained.operations.len()
    );
    Ok(())
}

#[tokio::test]
async fn command_and_stop_occurrence_correlation_cannot_be_rebound() -> Result<()> {
    for attack in [
        "command",
        "phase",
        "transcript",
        "prompt-id",
        "stop-state",
        "envelope-repeat",
    ] {
        let mut f = Fixture::new()?;
        if attack == "command" {
            assert!(f.callbacks.observe(&json!({"type":"command_lifecycle","command_uuid":"another","state":"started","session_id":"source-session"}), &f.events).is_err());
            continue;
        }
        if attack == "phase" {
            f.runtime.finish_phase()?;
            assert!(
                f.callbacks
                    .handle(
                        &f.message("UserPromptSubmit", "submit"),
                        Some("source-session"),
                        &f.events,
                        &f.tools
                    )
                    .await
                    .is_err()
            );
            continue;
        }
        f.submit().await?;
        f.response("command")?;
        let mut stop = f.message("Stop", "stop");
        match attack {
            "transcript" => stop["request"]["input"]["transcript_path"] = json!("/another.jsonl"),
            "prompt-id" => stop["request"]["input"]["prompt_id"] = json!("another"),
            "stop-state" => stop["request"]["input"]["stop_hook_active"] = json!(true),
            "envelope-repeat" => stop["request"]["tool_use_id"] = json!("outer-submit"),
            _ => unreachable!(),
        }
        assert!(
            f.callbacks
                .handle(&stop, Some("source-session"), &f.events, &f.tools)
                .await
                .is_err(),
            "{attack}"
        );
        assert_eq!(
            f.runtime
                .record()?
                .operations
                .iter()
                .filter(|o| o.non_tool_receipt().is_some())
                .count(),
            1
        );
    }
    Ok(())
}

#[test]
fn source_submission_strings_and_provider_blocks_have_distinct_projection() -> Result<()> {
    assert_eq!(
        source_prompt(&json!("  original prompt  "))?,
        "  original prompt  "
    );
    let content = json!([{"type":"text","text":"prefix"},{"type":"text","text":"  first\n"},{"type":"image","source":{"type":"base64","data":"retained"}},{"type":"text","text":"\nsecond  "}]);
    let retained = content.clone();
    assert_eq!(source_prompt(&content)?, "prefix\n  first\n\n\nsecond");
    assert_eq!(
        content, retained,
        "projection cannot rewrite provider content"
    );
    assert!(source_prompt(&json!([{"type":"text","text":3}])).is_err());
    Ok(())
}

#[test]
fn retired_terminal_notice_never_starts_or_acknowledges_current_command() -> Result<()> {
    let mut f = Fixture::new()?;
    f.callbacks.command_started = false;
    f.callbacks.retired_command = Some(("retired".into(), "completed"));
    assert!(
        f.callbacks
            .observe(
                &json!({"type":"command_lifecycle","command_uuid":"retired","state":"started"}),
                &f.events
            )
            .is_err()
    );
    f.callbacks.observe(
        &json!({"type":"command_lifecycle","command_uuid":"retired","state":"completed"}),
        &f.events,
    )?;
    assert!(!f.callbacks.command_started);
    assert!(
        f.callbacks
            .observe(
                &json!({"type":"command_lifecycle","command_uuid":"retired","state":"completed"}),
                &f.events
            )
            .is_err()
    );
    assert!(
        f.runtime
            .record()?
            .operations
            .iter()
            .all(|o| o.non_tool_receipt().is_none())
    );
    Ok(())
}

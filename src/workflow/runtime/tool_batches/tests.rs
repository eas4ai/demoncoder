use super::*;
use serde_json::json;
#[test]
fn batch_cannot_settle_while_original_member_observers_are_pending() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let source = runtime.begin_model("worker").unwrap();
    let outer = ToolCall {
        id: "wrapper".into(),
        name: "tool_batch".into(),
        arguments: json!({"calls":[{"tool":"write","arguments":{"path":"proof","content":"actual"}}]}),
    };
    let wrapper = match runtime.begin_tool("worker", source, &outer).unwrap() {
        super::super::ToolAdmission::Fresh(id) => id,
        _ => panic!("fresh wrapper"),
    };
    runtime.admit_tool(wrapper, &outer).unwrap();
    runtime.tool_effect(wrapper).unwrap();
    let (id, members) = runtime
        .begin_tool_batch(
            wrapper,
            vec![ToolCall {
                id: String::new(),
                name: "write".into(),
                arguments: json!({"path":"proof","content":"actual"}),
            }],
        )
        .unwrap();
    let member = &members[0];
    let operation = match runtime.begin_tool("worker", id, member).unwrap() {
        super::super::ToolAdmission::Fresh(id) => id,
        _ => panic!("fresh member"),
    };
    runtime.admit_tool(operation, member).unwrap();
    runtime.tool_effect(operation).unwrap();
    std::fs::write(root.path().join("proof"), "actual").unwrap();
    runtime
        .original_tool_result(
            operation,
            &ToolResult {
                call_id: member.id.clone(),
                tool: "write".into(),
                success: true,
                output: "actual effect completed".into(),
                exit_code: None,
            },
        )
        .unwrap();
    let record = runtime.record().unwrap();
    assert!(
        record
            .operations
            .iter()
            .find(|o| o.id == operation)
            .unwrap()
            .result
            .is_some()
    );
    assert!(
        runtime.settle_tool_batch(id).is_err(),
        "member result alone cannot cross unfinished observer boundary"
    );
    assert!(runtime.batch_occurrence(id).is_err());
    assert_eq!(
        std::fs::read_to_string(root.path().join("proof")).unwrap(),
        "actual"
    );
}

#[tokio::test]
async fn batch_framing_uses_actual_rewritten_member_and_retains_original_request() {
    use crate::{
        events::EventSink,
        native::{Model, NativeSession},
        plugins::{
            dispatch::*,
            gate_snapshot::GateReadSet,
            hook_types::{HandlerKind, HookDialect, HookEvent},
            non_tool::NonToolPlan,
        },
        session::Session,
        tools::ToolExecutor,
    };
    use std::sync::{Arc, Mutex};
    struct Rewrite;
    #[async_trait::async_trait]
    impl HookRunner for Rewrite {
        fn side_effect_free(&self) -> bool {
            true
        }
        async fn run(&self, _: &HookInvocation) -> anyhow::Result<RawOutcome> {
            Ok(RawOutcome::Callback {
                value: json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":{"path":"actual","content":"rewritten bytes"}}}),
            })
        }
    }
    struct Deny;
    #[async_trait::async_trait]
    impl HookRunner for Deny {
        fn side_effect_free(&self) -> bool {
            true
        }
        async fn run(&self, _: &HookInvocation) -> anyhow::Result<RawOutcome> {
            Ok(RawOutcome::Callback {
                value: json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"retained member policy"}}),
            })
        }
    }
    struct Observe(Arc<Mutex<Vec<serde_json::Value>>>);
    #[async_trait::async_trait]
    impl HookRunner for Observe {
        fn side_effect_free(&self) -> bool {
            true
        }
        async fn run(&self, invocation: &HookInvocation) -> anyhow::Result<RawOutcome> {
            let crate::plugins::receipts::NonToolOccurrence::PostToolBatch {
                batch: Some(batch),
                ..
            } = invocation.lifecycle.as_ref().unwrap().subject.occurrence
            else {
                panic!("batch facts")
            };
            *self.0.lock().unwrap() = invocation.events.batch_runtime()?.batch_input(batch)?;
            Ok(RawOutcome::Callback { value: json!({}) })
        }
    }
    fn registration(
        class: HandlerClass,
        runner: Arc<dyn HookRunner>,
        matcher: Matcher,
        index: u32,
    ) -> Registration {
        Registration {
            declaration: Declaration {
                required_gate: false,
                source: None,
                once: None,
                identity: DeclarationIdentity {
                    package: "rewritten-batch".into(),
                    code: "code".into(),
                    policy: "policy".into(),
                    configuration: "config".into(),
                    generation: "1".into(),
                    scope: Scope::Project,
                    role: "worker".into(),
                    declaration: format!("handler-{index}"),
                    index,
                    dialect: HookDialect::Native,
                    runner: HandlerKind::Command,
                },
                class,
                priority: 0,
                matcher,
                reads: GateReadSet::default(),
                concurrent_group: None,
                read_only_endpoint: None,
                external_precondition: None,
            },
            runner,
            revalidation: None,
        }
    }
    struct Response(Option<ToolCall>);
    #[async_trait::async_trait]
    impl Model for Response {
        fn prompt(&mut self, _: String) {}
        fn results(&mut self, _: Vec<ToolResult>) {}
        async fn response(&mut self, _: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
            Ok(self.0.take().into_iter().collect())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let seen = Arc::new(Mutex::new(vec![]));
    let mut tools = ToolExecutor::new(root.path()).unwrap();
    tools
        .register_pre_tool_plan(Arc::new(
            PreToolPlan::new(vec![
                registration(
                    HandlerClass::Transformer,
                    Arc::new(Rewrite),
                    Matcher {
                        tool: Some("write".into()),
                        ..Default::default()
                    },
                    0,
                ),
                registration(
                    HandlerClass::DecisionGate,
                    Arc::new(Deny),
                    Matcher {
                        tool: Some("edit".into()),
                        ..Default::default()
                    },
                    2,
                ),
            ])
            .unwrap(),
        ))
        .unwrap();
    tools
        .register_non_tool_plan(Arc::new(
            NonToolPlan::new(
                HookEvent::PostToolBatch,
                vec![registration(
                    HandlerClass::Observer,
                    Arc::new(Observe(seen.clone())),
                    Matcher::default(),
                    1,
                )],
            )
            .unwrap(),
        ))
        .unwrap();
    let mut native = NativeSession::with_tools(
        Box::new(Response(Some(ToolCall {
            id: "batch".into(),
            name: "tool_batch".into(),
            arguments: json!({"calls":[{"tool":"write","arguments":{"path":"requested","content":"original bytes"}}, {"tool":"edit","arguments":{"path":"actual","old_text":"rewritten bytes","new_text":"forbidden"}}, {"tool":"bash","arguments":{"command":"exit 7"}}]}),
        }))),
        tools,
    );
    let (tx, _rx) = tokio::sync::mpsc::channel(256);
    let events = EventSink::new("rewritten-batch".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let (_tx, mut commands) = tokio::sync::mpsc::channel(4);
    native
        .turn("execute declared batch".into(), &mut commands, &events)
        .await
        .unwrap();
    assert!(!root.path().join("requested").exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("actual")).unwrap(),
        "rewritten bytes"
    );
    let record = runtime.record().unwrap();
    let member = record
        .operations
        .iter()
        .find(|o| o.call.as_ref().is_some_and(|c| c.name == "write"))
        .unwrap();
    assert_eq!(
        member
            .tool_receipt
            .as_ref()
            .unwrap()
            .original_call
            .arguments,
        json!({"path":"requested","content":"original bytes"})
    );
    assert_eq!(
        member.call.as_ref().unwrap().arguments,
        json!({"path":"actual","content":"rewritten bytes"})
    );
    let facts = seen.lock().unwrap();
    assert_eq!(facts.len(), 3);
    let profile = crate::plugins::profile::CompatibilityProfile::embedded().unwrap();
    for fact in facts.iter() {
        profile
            .validate_claude_type("PostToolBatchToolCall", fact)
            .expect("host member input must fit the unchanged Claude item schema");
    }
    assert_eq!(
        facts[0]["tool_input"],
        member.call.as_ref().unwrap().arguments,
        "batch must describe actually admitted input, not original requested bytes"
    );
    assert_eq!(
        facts[0]["tool_response"],
        serde_json::to_value(member.result.as_ref().unwrap()).unwrap()
    );
    assert!(member.tool_receipt.as_ref().unwrap().admitted);
    assert!(member.tool_receipt.as_ref().unwrap().effect_started);
    assert_eq!(facts[1]["tool_name"], "edit");
    assert_eq!(facts[1]["tool_input"]["new_text"], "forbidden");
    let denied = record
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref())
        .find(|r| r.original_call.name == "edit")
        .unwrap();
    assert!(!denied.admitted && !denied.effect_started);
    assert_eq!(facts[1]["tool_response"]["success"], false);
    assert_eq!(facts[2]["tool_input"]["command"], "exit 7");
    let failed = record
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref())
        .find(|r| r.original_call.name == "bash")
        .unwrap();
    assert!(failed.admitted && failed.effect_started);
    assert_eq!(facts[2]["tool_response"]["success"], false);
    assert_eq!(facts[2]["tool_response"]["exit_code"], 7);
}

use super::*;
use crate::{
    config::Connection,
    plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect},
    },
    workflow::runtime::{Identity, Record, SharedRuntime},
};

struct Allow;
#[async_trait::async_trait]
impl HookRunner for Allow {
    async fn run(&self, _: &HookInvocation) -> Result<RawOutcome> {
        Ok(RawOutcome::Callback {
            value: json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}),
        })
    }
}

#[tokio::test]
async fn final_rescan_rechecks_file_and_parent_after_await_before_actual_write() {
    for host in [false, true] {
        for existing in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let workspace = root.path().join("workspace");
            std::fs::create_dir(&workspace).unwrap();
            std::fs::create_dir(workspace.join("generated")).unwrap();
            if existing {
                std::fs::write(workspace.join("generated/file"), "original").unwrap();
            }
            let connection: Connection =
                serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
            let record: Record = serde_json::from_value(json!({"workspace":workspace,"identity":Identity::from(&connection),"archived":[],"next_task":1,"checkpoint_cursor":0,"operations":[],"messages":[],"recovery_pending":false,"decisions":[]})).unwrap();
            let runtime = SharedRuntime::for_test(&root.path().join("record"), record).unwrap();
            let (sender, _receiver) = tokio::sync::mpsc::channel(16);
            let events = EventSink::new("fixture".into(), sender, None)
                .unwrap()
                .with_runtime(runtime.clone());
            let events = events.for_invocation(events.begin_model().unwrap());
            let executor = ToolExecutor::with_policy(
                &workspace,
                &AccessPolicy {
                    unrestricted: host,
                    ..Default::default()
                },
            )
            .unwrap();
            let plan = PreToolPlan::new(vec![Registration {
                declaration: Declaration {
                    source: None,
                    once: None,
                    identity: DeclarationIdentity {
                        package: "policy".into(),
                        code: "code".into(),
                        policy: "policy".into(),
                        configuration: "config".into(),
                        generation: "1".into(),
                        scope: Scope::Project,
                        role: "worker".into(),
                        declaration: "gate".into(),
                        index: 0,
                        dialect: HookDialect::Native,
                        runner: HandlerKind::Command,
                    },
                    class: HandlerClass::DecisionGate,
                    priority: 0,
                    matcher: Matcher {
                        tool: Some("write".into()),
                        path: Some("generated/file".into()),
                    },
                    reads: GateReadSet::new(vec![], vec![], vec![]).unwrap(),
                    concurrent_group: None,
                    read_only_endpoint: None,
                    external_precondition: None,
                },
                runner: Arc::new(Allow),
                revalidation: None,
            }])
            .unwrap();
            let mut call = ToolCall {
                id: "write".into(),
                name: "write".into(),
                arguments: json!({"path":"generated/file","content":"forbidden"}),
            };
            let (events, _) = events.begin_tool(&call).unwrap();
            let meta = executor.root.metadata().unwrap();
            let identity = (meta.dev(), meta.ino());
            let admitted = plan
                .admit(
                    &mut call,
                    &events,
                    executor.gate_workspace.clone(),
                    identity,
                    &executor,
                )
                .await
                .unwrap();
            events.admit_tool(&call).unwrap();
            let mut effect = ToolEffect {
                events: &events,
                started: false,
                admission_refused: false,
                admission: Some(admitted),
                boundary: events.mutation_boundary(identity).unwrap(),
                guard: None,
            };
            // Block only the final rescan, after the gate and initial rescan.
            let permit = plan.captures.acquire().await.unwrap();
            let attempt = async {
                let mut file = executor
                    .admitted_file(&call, "generated/file", OFlags::WRONLY, true, &mut effect)
                    .await?;
                effect.start().await?;
                write_text(&mut file, "forbidden")
            };
            tokio::pin!(attempt);
            tokio::select! {
                result = &mut attempt => panic!("write did not wait for final rescan: {result:?}"),
                _ = tokio::time::sleep(Duration::from_millis(1)) => {}
            }
            if existing {
                std::fs::rename(
                    workspace.join("generated/file"),
                    workspace.join("moved-file"),
                )
                .unwrap();
                std::fs::write(workspace.join("generated/file"), "replacement").unwrap();
            } else {
                std::fs::rename(workspace.join("generated"), workspace.join("moved-parent"))
                    .unwrap();
                std::fs::create_dir(workspace.join("generated")).unwrap();
            }
            drop(permit);
            assert!(attempt.await.is_err(), "host={host}, existing={existing}");
            assert!(
                !runtime
                    .record()
                    .unwrap()
                    .operations
                    .last()
                    .unwrap()
                    .tool_receipt
                    .as_ref()
                    .unwrap()
                    .effect_started
            );
            if existing {
                assert_eq!(
                    std::fs::read_to_string(workspace.join("moved-file")).unwrap(),
                    "original"
                );
                assert_eq!(
                    std::fs::read_to_string(workspace.join("generated/file")).unwrap(),
                    "replacement"
                );
            } else {
                assert!(!workspace.join("moved-parent/file").exists());
                assert!(!workspace.join("generated/file").exists());
            }
        }
    }
}

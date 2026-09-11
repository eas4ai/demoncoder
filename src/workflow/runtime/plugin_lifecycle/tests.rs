use super::*;
use crate::{
    plugins::lifecycle::PostEffects,
    subagents::state::{
        AgentStatus, DelegationIdentity, OrchestrationIdentity, OrchestrationStage,
        OrchestrationState, RoleReceipt,
    },
    tools::{ToolCall, ToolResult},
    workflow::{
        allocation::{Allocation, Limits},
        review::{Role, Verdict},
        runtime::{HostInvocation, ToolAdmission},
        state::Task,
        workspace,
    },
};
use serde_json::json;

fn correction() -> PostEffects {
    let mut effects = PostEffects::default();
    effects.continuation = PostContinuation::Correction;
    effects.consume_correction = true;
    effects
}

fn fixture(
    root: &std::path::Path,
    variant: &str,
    external: bool,
) -> (SharedRuntime, u64, u64, String) {
    let mut record = crate::inspection::tests::record(root);
    record.allocation = Some(Allocation::new(Limits::default()).unwrap());
    if variant != "no-parent-task" {
        let mut task = Task::new(
            1,
            "parent objective must stay separate".into(),
            vec![],
            workspace::capture(root).unwrap(),
            2,
        )
        .unwrap();
        task.corrections = 1;
        task.stopped = true; // Background child authority remains assignment-owned.
        record.task = Some(task);
    }
    record.delegation = Some(DelegationIdentity {
        connections: Default::default(),
        reviewer: None,
        max_active: 2,
        backend_limit: if variant == "backend-exhausted" { 1 } else { 8 },
        default_roles: vec![],
        orchestration: (variant != "unsupervised-identity").then(|| OrchestrationIdentity {
            judge: record.identity.clone(),
            correction_limit: 2,
            checks: vec!["check".into()],
        }),
    });
    let mut child = crate::inspection::tests::agent(7, AgentStatus::Running, &record.identity);
    child.parent_task = record.task.as_ref().map(|t| t.id);
    child.request.objective = "child assignment objective".into();
    let mut state = OrchestrationState::new(vec![]);
    state.stage = OrchestrationStage::Working;
    if variant == "exhausted" {
        state.correction_rounds = 2;
    }
    state.receipts.push(RoleReceipt {
        role: Role::Judge,
        correction_round: 0,
        connection: record.identity.clone(),
        snapshot: "retained-snapshot".into(),
        evidence: "retained-evidence".into(),
        verdict: Verdict::Findings,
        findings: vec!["unresolved supervised finding".into()],
        explanation: "needs independent verification".into(),
    });
    if variant == "supervisor-correction" {
        // Manager retains initial-worker completion while the supervisor admits
        // a fresh worker correction after judging its findings.
        child.completed = true;
        state.admit_correction(2).unwrap();
        state.stage = OrchestrationStage::Correcting;
        child.status = AgentStatus::Running;
    }
    child.orchestration = (variant != "unsupervised-child").then_some(state);
    record.agents.push(child);
    let runtime = SharedRuntime::for_test(&root.join("record"), record).unwrap();
    let phase = if variant == "advisor" {
        "agent:7:advisor"
    } else if variant == "judge" {
        "agent:7:judge"
    } else if variant == "verification" {
        "agent:7:verification"
    } else if variant == "reviewer" {
        "agent:7:reviewer"
    } else {
        "agent:7:worker"
    }
    .to_owned();
    let identity = runtime.record().unwrap().agents[0].identity.clone();
    let source = if external {
        runtime
            .begin_backend_owned(&phase, Some(&identity), None)
            .unwrap()
    } else {
        runtime
            .begin_model_owned(&phase, Some(&identity), None)
            .unwrap()
    };
    let call = ToolCall {
        id: "completed".into(),
        name: "write".into(),
        arguments: json!({"path":"created","content":"retained"}),
    };
    let id = match runtime.begin_tool(&phase, source, &call).unwrap() {
        ToolAdmission::Fresh(id) => id,
        _ => panic!("fresh child tool required"),
    };
    runtime.admit_tool(id, &call).unwrap();
    runtime.tool_effect(id).unwrap();
    let result = ToolResult {
        call_id: call.id,
        tool: call.name,
        success: true,
        output: "retained original child result".into(),
        exit_code: None,
    };
    runtime.original_tool_result(id, &result).unwrap();
    let representation = if external {
        ToolRepresentation::CodexDynamic {
            tool_use_id: "source-tool".into(),
            turn_id: "old-turn".into(),
            session_id: "child-thread".into(),
            model: None,
            permission_mode: "default".into(),
            transcript_path: None,
        }
    } else {
        ToolRepresentation::Native
    };
    runtime
        .begin_post_tool(
            id,
            HookEvent::PostToolUse,
            "explicit-child-capability".into(),
            vec![],
            representation,
        )
        .unwrap();
    runtime.model_tool_result(id, &result).unwrap();
    (runtime, id, source, phase)
}

#[test]
fn supervised_child_post_correction_spends_only_its_retained_assignment_ledger() {
    for external in [false, true] {
        for variant in ["available", "no-parent-task", "supervisor-correction"] {
            let root = tempfile::tempdir().unwrap();
            let (runtime, id, source, phase) = fixture(root.path(), variant, external);
            assert!(
                runtime
                    .post_model_context(id, HookEvent::PostToolUse)
                    .unwrap()
                    .correction_available
            );
            let continuation = runtime
                .settle_post_tool(id, HookEvent::PostToolUse, correction())
                .unwrap();
            assert_eq!(continuation, PostContinuation::Correction);
            runtime.settle_tool(id).unwrap();
            if external {
                runtime.start_post_supersession(id).unwrap();
                runtime.finish_model(source).unwrap();
                runtime.finish_post_supersession(id).unwrap();
                let identity = runtime.record().unwrap().agents[0].identity.clone();
                let next = runtime
                    .reserve_post_correction(id, &phase, Some(&identity))
                    .unwrap();
                runtime
                    .ack_post_correction(
                        id,
                        next,
                        CorrectionAcknowledgment::CodexTurn {
                            thread_id: "child-thread".into(),
                            turn_id: "new-turn".into(),
                            request_id: 42,
                        },
                    )
                    .unwrap();
                assert!(matches!(
                    runtime
                        .record()
                        .unwrap()
                        .operations
                        .last()
                        .unwrap()
                        .host_invocation,
                    Some(HostInvocation::Backend)
                ));
                assert_eq!(runtime.record().unwrap().backend_invocations, 2);
            } else {
                runtime.complete_local_post_release(id).unwrap();
            }
            let record = runtime.record().unwrap();
            if let Some(task) = &record.task {
                assert_eq!(task.corrections, 1);
                assert!(task.accepted.is_none());
            }
            let child = &record.agents[0];
            let state = child.orchestration.as_ref().unwrap();
            assert_eq!(
                state.correction_rounds,
                if variant == "supervisor-correction" {
                    2
                } else {
                    1
                }
            );
            assert_eq!(state.stage, OrchestrationStage::Correcting);
            assert_eq!(
                state.receipts[0].findings,
                vec!["unresolved supervised finding"]
            );
            assert_eq!(child.status, AgentStatus::Running);
            assert_eq!(child.completed, variant == "supervisor-correction");
            assert!(!child.orchestration_allows_integration());
            assert!(
                runtime
                    .post_tool_receipt(id)
                    .unwrap()
                    .unwrap()
                    .correction_admitted
            );
            assert_eq!(
                record
                    .operations
                    .iter()
                    .find(|o| o.id == id)
                    .unwrap()
                    .result
                    .as_ref()
                    .unwrap()
                    .output,
                "retained original child result"
            );
        }
    }
}

#[test]
fn unsupervised_or_nonworker_child_cannot_borrow_the_parent_correction_ledger() {
    for variant in [
        "unsupervised-child",
        "unsupervised-identity",
        "exhausted",
        "advisor",
        "judge",
        "reviewer",
        "verification",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (runtime, id, _, _) = fixture(root.path(), variant, false);
        assert!(
            !runtime
                .post_model_context(id, HookEvent::PostToolUse)
                .unwrap()
                .correction_available,
            "{variant}"
        );
        let continuation = runtime
            .settle_post_tool(id, HookEvent::PostToolUse, correction())
            .unwrap();
        assert!(
            matches!(continuation, PostContinuation::Held { .. }),
            "{variant}"
        );
        assert!(runtime.complete_local_post_release(id).is_err());
        let record = runtime.record().unwrap();
        assert_eq!(record.task.as_ref().unwrap().corrections, 1);
        assert_eq!(
            record.agents[0]
                .orchestration
                .as_ref()
                .map(|s| s.correction_rounds),
            if variant == "unsupervised-child" {
                None
            } else {
                Some(if variant == "exhausted" { 2 } else { 0 })
            }
        );
    }
}

#[test]
fn child_release_rechecks_status_identity_budget_and_owner_without_spending() {
    for variant in [
        "cancelled",
        "stopped",
        "checking",
        "completed-working",
        "completed-unadmitted",
        "identity",
        "parent",
        "rounds",
        "allocation",
        "backend-exhausted",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (runtime, id, source, phase) = fixture(root.path(), variant, true);
        assert_eq!(
            runtime
                .settle_post_tool(id, HookEvent::PostToolUse, correction())
                .unwrap(),
            PostContinuation::Correction
        );
        runtime.settle_tool(id).unwrap();
        runtime.start_post_supersession(id).unwrap();
        runtime.finish_model(source).unwrap();
        runtime.finish_post_supersession(id).unwrap();
        runtime
            .update(|record| {
                match variant {
                    "cancelled" => record.agents[0].status = AgentStatus::Cancelled,
                    "stopped" => record.agents[0].status = AgentStatus::Stopped,
                    "checking" => {
                        record.agents[0].orchestration.as_mut().unwrap().stage =
                            OrchestrationStage::Checking
                    }
                    "completed-working" => record.agents[0].completed = true,
                    "completed-unadmitted" => {
                        record.agents[0].completed = true;
                        record.agents[0].orchestration.as_mut().unwrap().stage =
                            OrchestrationStage::Correcting;
                    }
                    "identity" => record.agents[0].identity.model = Some("changed-model".into()),
                    "parent" => record.agents[0].parent_task = Some(99),
                    "rounds" => {
                        record.agents[0]
                            .orchestration
                            .as_mut()
                            .unwrap()
                            .correction_rounds = 2
                    }
                    "allocation" => {
                        let a = record.allocation.as_mut().unwrap();
                        a.tool_calls = a.limits.tool_calls;
                    }
                    _ => {}
                }
                Ok(())
            })
            .unwrap();
        let before = runtime.record().unwrap();
        assert!(
            runtime
                .reserve_post_correction(id, &phase, Some(&before.agents[0].identity))
                .is_err(),
            "{variant}"
        );
        let after = runtime.record().unwrap();
        assert_eq!(after.operations.len(), before.operations.len(), "{variant}");
        assert_eq!(after.task.as_ref().unwrap().corrections, 1);
        assert_eq!(
            after.agents[0]
                .orchestration
                .as_ref()
                .unwrap()
                .correction_rounds,
            if variant == "rounds" { 2 } else { 0 }
        );
        assert!(
            !runtime
                .post_tool_receipt(id)
                .unwrap()
                .unwrap()
                .correction_admitted
        );
    }
}

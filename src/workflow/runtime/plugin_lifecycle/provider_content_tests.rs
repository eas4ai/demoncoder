use super::*;
use crate::{
    plugins::lifecycle::PostEffects,
    tools::{ToolCall, ToolResult},
    workflow::{
        allocation::{Allocation, Limits},
        runtime::ToolAdmission,
    },
};
use serde_json::{Value, json};

fn search(enabled: Option<bool>) -> Value {
    let mut value = json!([{"type":"search_result","source":"source","title":"title","content":[{"type":"text","text":"retained result"}]}]);
    if let Some(enabled) = enabled {
        value[0]["citations"] = json!({"enabled":enabled});
    }
    value
}

fn fixture(
    root: &std::path::Path,
    sessions: [&str; 2],
    policies: [Option<bool>; 2],
) -> (SharedRuntime, [u64; 2]) {
    fixture_with_correction(root, sessions, policies, false)
}

fn fixture_with_correction(
    root: &std::path::Path,
    sessions: [&str; 2],
    policies: [Option<bool>; 2],
    correction: bool,
) -> (SharedRuntime, [u64; 2]) {
    let mut record = crate::inspection::tests::record(root);
    record.allocation = Some(Allocation::new(Limits::default()).unwrap());
    record.task = Some(
        crate::workflow::state::Task::new(
            1,
            "owning task".into(),
            vec![],
            crate::workflow::workspace::capture(root).unwrap(),
            2,
        )
        .unwrap(),
    );
    let runtime = SharedRuntime::for_test(&root.join("record"), record).unwrap();
    let mut ids = Vec::new();
    // Execute both host calls before staging either presentation. The source
    // conversation may be resumed under another role/turn; its policy persists.
    for (index, phase) in ["worker", "judge"].into_iter().enumerate() {
        let source = runtime.begin_backend_owned(phase, None, None).unwrap();
        let call = ToolCall {
            id: format!("call-{index}"),
            name: "write".into(),
            arguments: json!({"path":format!("file-{index}"),"content":"retained"}),
        };
        let ToolAdmission::Fresh(id) = runtime.begin_tool(phase, source, &call).unwrap() else {
            panic!("fresh tool required")
        };
        runtime.admit_tool(id, &call).unwrap();
        runtime.tool_effect(id).unwrap();
        let result = ToolResult {
            call_id: call.id,
            tool: call.name,
            success: true,
            output: format!("original-result-{index}"),
            exit_code: None,
        };
        runtime.original_tool_result(id, &result).unwrap();
        runtime.model_tool_result(id, &result).unwrap();
        ids.push(id);
    }
    for (index, id) in ids.iter().copied().enumerate() {
        runtime
            .begin_post_tool(
                id,
                HookEvent::PostToolUse,
                "source-profile".into(),
                vec![],
                ToolRepresentation::ClaudeMcp {
                    tool_name: "mcp__demoncoder__write".into(),
                    tool_use_id: format!("source-{index}"),
                    source_input: json!({"session_id":sessions[index]}),
                },
            )
            .unwrap();
        let mut effects = PostEffects::default();
        effects.model_content = Some(search(policies[index]));
        if correction && index == 0 {
            effects.continuation = PostContinuation::Correction;
            effects.consume_correction = true;
        }
        effects.proposals = vec![AppliedProposal {
            invocation: 0,
            proposal: PendingProposal {
                index: 0,
                kind: ProposalKind::ReplaceModelOutput,
            },
            disposition: ProposalDisposition::Applied,
        }];
        runtime
            .settle_post_tool(id, HookEvent::PostToolUse, effects)
            .unwrap();
        runtime.settle_tool(id).unwrap();
    }
    (runtime, ids.try_into().unwrap())
}

#[test]
fn citation_release_rechecks_all_deliveries_atomically_and_persists_rejection() {
    for first in [0, 1] {
        let root = tempfile::tempdir().unwrap();
        let (runtime, ids) = fixture(
            root.path(),
            ["same-source", "same-source"],
            [Some(true), None],
        );
        runtime.reserve_post_delivery(ids[first]).unwrap();
        let before =
            serde_json::to_value(runtime.post_tool_receipt(ids[first]).unwrap().unwrap()).unwrap();
        let later = ids[1 - first];
        let error = runtime.reserve_post_delivery(later).unwrap_err();
        assert!(error.to_string().contains("request compatibility"));
        assert_eq!(
            serde_json::to_value(runtime.post_tool_receipt(ids[first]).unwrap().unwrap()).unwrap(),
            before
        );
        let post = runtime.post_tool_receipt(later).unwrap().unwrap();
        assert_eq!(post.delivery, PostDelivery::Staged);
        assert!(!post.correction_admitted);
        assert!(
            matches!(post.continuation, PostContinuation::Held { ref reason } if reason.contains("request compatibility"))
        );
        assert!(post.model_content.is_none());
        assert!(matches!(
            post.proposals[0].disposition,
            ProposalDisposition::Held
        ));
        let record = runtime.record().unwrap();
        assert_eq!(
            record
                .operations
                .iter()
                .find(|o| o.id == later)
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .output,
            format!("original-result-{}", 1 - first)
        );
        assert!(runtime.ensure_post_continuation(&post.facts.role).is_err());
        let state_path = runtime.0.lock().unwrap().store.state_path().to_owned();
        let persisted: Value = serde_json::from_slice(&std::fs::read(state_path).unwrap()).unwrap();
        let retained = persisted["payload"]["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == later)
            .unwrap();
        assert_eq!(
            retained["tool_receipt"]["plugin_lifecycle"]["continuation"]["state"],
            "held"
        );
    }
}

#[test]
fn citation_release_accepts_default_false_and_separate_source_sessions() {
    for (sessions, policies) in [
        (["same", "same"], [None, Some(false)]),
        (["first", "second"], [Some(true), Some(false)]),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (runtime, ids) = fixture(root.path(), sessions, policies);
        runtime.reserve_post_delivery(ids[1]).unwrap();
        runtime.ack_post_delivery(ids[1]).unwrap();
        runtime.reserve_post_delivery(ids[0]).unwrap();
        assert_eq!(
            runtime
                .post_tool_receipt(ids[0])
                .unwrap()
                .unwrap()
                .model_content,
            Some(search(policies[0]))
        );
    }
}

#[test]
fn missing_source_session_cannot_disable_request_consistency() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, ids) = fixture(root.path(), ["", "valid"], [None, None]);
    assert!(
        runtime
            .reserve_post_delivery(ids[0])
            .unwrap_err()
            .to_string()
            .contains("nonempty source session")
    );
    let post = runtime.post_tool_receipt(ids[0]).unwrap().unwrap();
    assert!(matches!(post.continuation, PostContinuation::Held { .. }));
}

fn supersede(runtime: &SharedRuntime, id: u64) {
    runtime.start_post_supersession(id).unwrap();
    let source = runtime
        .post_tool_receipt(id)
        .unwrap()
        .unwrap()
        .facts
        .source_operation;
    runtime.finish_model(source).unwrap();
    runtime.finish_post_supersession(id).unwrap();
}

#[test]
fn typed_correction_and_ordinary_delivery_share_atomic_citation_policy() {
    for correction_first in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (runtime, ids) =
            fixture_with_correction(root.path(), ["same", "same"], [Some(true), None], true);
        supersede(&runtime, ids[0]);
        let before_count = runtime.record().unwrap().operations.len();
        if correction_first {
            runtime
                .reserve_post_correction(ids[0], "worker", None)
                .unwrap();
            assert!(runtime.reserve_post_delivery(ids[1]).is_err());
            let post = runtime.post_tool_receipt(ids[0]).unwrap().unwrap();
            assert_eq!(
                post.correction_presentation,
                Some(CorrectionPresentation::ClaudeProviderBlocksV1)
            );
            assert!(matches!(
                post.delivery,
                PostDelivery::CorrectionReserved { .. }
            ));
            assert_eq!(post.model_content, Some(search(Some(true))));
            assert_eq!(runtime.record().unwrap().task.unwrap().corrections, 1);
        } else {
            runtime.reserve_post_delivery(ids[1]).unwrap();
            assert!(
                runtime
                    .reserve_post_correction(ids[0], "worker", None)
                    .unwrap_err()
                    .to_string()
                    .contains("request compatibility")
            );
            let record = runtime.record().unwrap();
            assert_eq!(record.operations.len(), before_count);
            assert_eq!(record.task.unwrap().corrections, 0);
            let post = runtime.post_tool_receipt(ids[0]).unwrap().unwrap();
            assert!(!post.correction_admitted);
            assert!(post.correction_presentation.is_none());
            assert!(matches!(post.continuation, PostContinuation::Held { .. }));
            assert_eq!(post.delivery, PostDelivery::Superseded);
        }
    }
}

#[test]
fn legacy_stringified_correction_is_not_a_structured_search_delivery() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, ids) =
        fixture_with_correction(root.path(), ["same", "same"], [Some(true), None], true);
    supersede(&runtime, ids[0]);
    runtime
        .reserve_post_correction(ids[0], "worker", None)
        .unwrap();
    let mut serialized =
        serde_json::to_value(runtime.post_tool_receipt(ids[0]).unwrap().unwrap()).unwrap();
    serialized
        .as_object_mut()
        .unwrap()
        .remove("correction_presentation");
    let historical: LifecycleReceipt = serde_json::from_value(serialized).unwrap();
    assert!(historical.correction_presentation.is_none());
    runtime
        .update(|record| {
            record
                .operations
                .iter_mut()
                .find(|operation| operation.id == ids[0])
                .unwrap()
                .tool_receipt
                .as_mut()
                .unwrap()
                .plugin_lifecycle = Some(historical);
            Ok(())
        })
        .unwrap();
    runtime.reserve_post_delivery(ids[1]).unwrap();
}

#[test]
fn inconsistent_retained_search_history_cannot_silently_choose_a_policy() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, ids) = fixture(root.path(), ["same", "same"], [Some(true), Some(false)]);
    let mut record = runtime.record().unwrap();
    for operation in &mut record.operations {
        if let Some(post) = operation
            .tool_receipt
            .as_mut()
            .and_then(|r| r.plugin_lifecycle.as_mut())
        {
            post.delivery = PostDelivery::Acknowledged;
        }
    }
    let mut next = runtime.post_tool_receipt(ids[0]).unwrap().unwrap().facts;
    next.operation = 999;
    assert!(
        crate::plugins::lifecycle::claude_content::retained_search_policy(&record, &next)
            .unwrap_err()
            .to_string()
            .contains("retained search citation settings disagree")
    );
}

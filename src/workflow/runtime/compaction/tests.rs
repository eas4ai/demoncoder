use super::*;
use crate::{
    plugins::{hook_types::HookEvent, receipts::NonToolOccurrence},
    session::SessionStart,
    workflow::allocation::Limits,
};
use serde_json::json;
fn fixture(
    funded: bool,
) -> (
    tempfile::TempDir,
    tempfile::TempDir,
    SharedRuntime,
    u64,
    u64,
) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.phase = Some("worker".into());
    record.task = None;
    record.allocation = None;
    record.checkpoint = Some(
        json!({"model":[{"role":"user","content":"x".repeat(5000)}],"developer_prompt":"original"}),
    );
    if funded {
        record.session_hook_allowance = Some(
            crate::workflow::runtime::session_budget::SessionHookAllowance::new(Limits {
                seconds: 60,
                model_calls: 4,
                tool_calls: 4,
            })
            .unwrap(),
        );
    }
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let lifetime = runtime
        .begin_native_session(SessionStart::Startup, None, vec![])
        .unwrap();
    let source = runtime.record().unwrap().checkpoint.unwrap();
    let id = runtime
        .begin_compaction("worker", None, "manual", &source, Some(lifetime))
        .unwrap();
    (root, state, runtime, lifetime, id)
}
fn pre(runtime: &SharedRuntime, id: u64) -> Result<u64> {
    Ok(runtime
        .begin_non_tool_as(
            "worker",
            None,
            None,
            NonToolOccurrence::PreCompact {
                compaction: Some(id),
                trigger: "manual".into(),
                custom_instructions: None,
            },
            "plan".into(),
            vec![],
        )?
        .operation)
}
#[test]
fn compaction_hook_authority_rejects_ended_cancelled_replaced_recovered_and_expired_owners() {
    for change in [
        "ended",
        "cancelled",
        "replaced",
        "recovered",
        "expired",
        "task",
        "child",
        "missing",
    ] {
        let (_root, _state, runtime, lifetime, id) = fixture(change != "missing");
        let occurrence = pre(&runtime, id).unwrap();
        if change != "missing" {
            assert!(
                runtime
                    .plugin_funded_remaining(occurrence, HookEvent::PreCompact)
                    .is_ok()
            );
        }
        match change {
            "ended" => runtime.end_compaction(id, None).unwrap(),
            "cancelled" => runtime.finalize_native_session(lifetime).unwrap(),
            "replaced" => {
                runtime
                    .begin_native_session(SessionStart::Resume, None, vec![])
                    .unwrap();
            }
            "recovered" => runtime
                .update(|r| {
                    r.recovery_pending = true;
                    Ok(())
                })
                .unwrap(),
            "expired" => runtime
                .update(|r| {
                    r.session_hook_allowance
                        .as_mut()
                        .unwrap()
                        .allocation
                        .clock_invalid = true;
                    Ok(())
                })
                .unwrap(),
            "task" => {
                runtime.allocate(Limits::default(), None).unwrap();
            }
            "child" => runtime
                .update(|r| {
                    r.operations.iter_mut().find(|o| o.id == id).unwrap().phase =
                        "agent:1:worker".into();
                    Ok(())
                })
                .unwrap(),
            "missing" => {}
            _ => unreachable!(),
        }
        assert!(
            runtime
                .plugin_funded_remaining(occurrence, HookEvent::PreCompact)
                .is_err(),
            "{change}"
        );
        if change == "ended" {
            assert!(pre(&runtime, id).is_err());
            assert!(runtime.compaction_model(id, "worker", None).is_err());
        }
    }
}
#[test]
fn compaction_application_and_reopen_preserve_atomic_checkpoint_and_evidence() {
    let (_root, _state, runtime, _lifetime, id) = fixture(false);
    let old = runtime.record().unwrap();
    let model = runtime.compaction_model(id, "worker", None).unwrap();
    runtime.finish_model(model).unwrap();
    let new = json!({"model":[{"role":"user","content":"retained summary"}],"developer_prompt":"original"});
    runtime
        .apply_compaction(id, "worker", new.clone(), "retained summary".into())
        .unwrap();
    let installed = runtime.installed_compaction_checkpoint("worker").unwrap();
    assert_eq!(installed, new);
    let applied = runtime.record().unwrap();
    assert_eq!(applied.operations[0].budget, old.operations[0].budget);
    let reloaded: Record = serde_json::from_value(serde_json::to_value(applied).unwrap()).unwrap();
    assert_eq!(reloaded.checkpoint, Some(new));
    assert!(
        reloaded
            .operations
            .iter()
            .any(|o| o.id == id && !o.complete)
    );
    runtime.end_compaction(id, None).unwrap();
    assert!(
        runtime
            .apply_compaction(id, "worker", old.checkpoint.unwrap(), "replay".into())
            .is_err()
    );
}
#[test]
fn compaction_swap_failure_latches_runtime_and_does_not_restore_over_disk() {
    let (_root, _state, runtime, _lifetime, id) = fixture(false);
    let model = runtime.compaction_model(id, "worker", None).unwrap();
    runtime.finish_model(model).unwrap();
    let old = runtime.installed_compaction_checkpoint("worker").unwrap();
    use std::os::unix::fs::PermissionsExt;
    let dir = runtime.directory().unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        runtime
            .apply_compaction(id, "worker", json!({"model":[]}), "summary".into())
            .is_err()
    );
    assert!(runtime.end_compaction(id, Some("cleanup".into())).is_err());
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        runtime.installed_compaction_checkpoint("worker").unwrap(),
        old
    );
}

#[tokio::test]
async fn compaction_child_retains_original_task_allocation_and_changes_only_child_checkpoint() {
    use crate::subagents::{state::AgentStatus, worktree};
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-q"],
        vec![
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.test",
            "commit",
            "--allow-empty",
            "-qm",
            "baseline",
        ],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(root.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let child_path = state.path().join("child");
    let tree = worktree::prepare(root.path(), &child_path).await.unwrap();
    let mut record = crate::inspection::tests::record(root.path());
    record.allocation =
        Some(crate::workflow::allocation::Allocation::new(Limits::default()).unwrap());
    let mut task = crate::workflow::state::Task::new(
        1,
        "original parent objective".into(),
        vec![],
        crate::workflow::workspace::capture(root.path()).unwrap(),
        1,
    )
    .unwrap();
    task.stopped = true;
    record.task = Some(task);
    record.checkpoint = Some(json!({"model":["parent context is immutable"]}));
    record.session_hook_allowance = Some(
        crate::workflow::runtime::session_budget::SessionHookAllowance::new(Limits::default())
            .unwrap(),
    );
    let mut child = crate::inspection::tests::agent(7, AgentStatus::Running, &record.identity);
    child.parent_task = Some(1);
    child.planned_root = Some(child_path);
    child.worktree = Some(tree);
    child.checkpoint = Some(
        json!({"model":[{"role":"user","content":"child data ".repeat(1000)}],"developer_prompt":"exact child request"}),
    );
    record.agents.push(child);
    let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
    let before = runtime.record().unwrap();
    let source = before.agents[0].checkpoint.clone().unwrap();
    let identity = before.agents[0].identity.clone();
    let lifetime = runtime
        .begin_native_session(SessionStart::Startup, None, vec![])
        .unwrap();
    let id = runtime
        .begin_compaction(
            "agent:7:worker",
            Some(&identity),
            "manual",
            &source,
            Some(lifetime),
        )
        .unwrap();
    assert!(
        hook_lifetime(&runtime.record().unwrap(), id, "agent:7:worker")
            .unwrap()
            .is_none()
    );
    let model = runtime
        .compaction_model(id, "agent:7:worker", Some(&identity))
        .unwrap();
    runtime.finish_model(model).unwrap();
    let new = json!({"model":[{"role":"user","content":"child summary"}],"developer_prompt":"exact child request"});
    runtime
        .apply_compaction(id, "agent:7:worker", new.clone(), "child summary".into())
        .unwrap();
    runtime.end_compaction(id, None).unwrap();
    let after = runtime.record().unwrap();
    assert_eq!(after.checkpoint, before.checkpoint);
    assert_eq!(after.agents[0].checkpoint, Some(new));
    assert_eq!(after.allocation.as_ref().unwrap().model_calls, 1);
    assert_eq!(
        after.allocation.as_ref().unwrap().deadline_ms,
        before.allocation.unwrap().deadline_ms
    );
    assert_eq!(
        after.session_hook_allowance.unwrap().allocation.model_calls,
        0
    );
    assert_eq!(after.task_allocation_epoch, before.task_allocation_epoch);
    assert_eq!(
        serde_json::to_value(after.task).unwrap(),
        serde_json::to_value(before.task).unwrap()
    );
    assert!(matches!(
        after
            .operations
            .iter()
            .find(|o| o.id == model)
            .unwrap()
            .budget,
        Some(crate::workflow::runtime::BudgetRef::Task { .. })
    ));
}

#[test]
fn compaction_cannot_borrow_a_colliding_lifetime_id_from_another_session() {
    let (_root_a, _state_a, a, _, ca) = fixture(true);
    let (_root_b, _state_b, b, _, cb) = fixture(true);
    a.end_compaction(ca, None).unwrap();
    b.end_compaction(cb, None).unwrap();
    let (tx, _rx) = tokio::sync::mpsc::channel(4);
    let events = crate::events::EventSink::new("session-a".into(), tx, None)
        .unwrap()
        .with_runtime(a.clone());
    events
        .begin_host_lifetime(SessionStart::Startup, vec![])
        .unwrap();
    b.begin_native_session(SessionStart::Startup, None, vec![])
        .unwrap();
    let wrong = events.with_runtime(b.clone());
    let source = b.record().unwrap().checkpoint.unwrap();
    let result = wrong.begin_compaction("manual", &source);
    assert!(result.is_err());
    assert!(format!("{:#}", result.err().unwrap()).contains("another session"));
}

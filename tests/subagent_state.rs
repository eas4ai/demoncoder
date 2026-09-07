use demoncoder::subagents::state;
use demoncoder::workflow;
use state::AssignmentRequest;

#[test]
fn ownership_uses_path_components_and_never_grants_git_administration() {
    let request = AssignmentRequest {
        connection: "worker".into(),
        objective: "repair parser".into(),
        context: String::new(),
        owned_paths: vec!["src/parser".into()],
    };
    request.validate().unwrap();
    assert!(request.owns("src/parser/mod.rs"));
    for path in [
        "src/parser-old/file",
        "../outside",
        "/tmp/file",
        "src/parser/.git/config",
    ] {
        assert!(!request.owns(path), "incorrectly owns {path}");
    }
    let all = AssignmentRequest {
        owned_paths: vec![".".into()],
        ..request
    };
    all.validate().unwrap();
    assert!(all.owns("new-file"));
    assert!(!all.owns(".git"));
}

#[test]
fn malformed_or_unbounded_assignments_never_acquire_ownership() {
    for owned in [
        "../home",
        "/home",
        ".git",
        "src/.git/config",
        "",
        "bad\npath",
    ] {
        let request = AssignmentRequest {
            connection: "worker".into(),
            objective: "task".into(),
            context: String::new(),
            owned_paths: vec![owned.into()],
        };
        assert!(request.validate().is_err(), "admitted {owned:?}");
    }
    let request = AssignmentRequest {
        connection: "worker".into(),
        objective: "task".into(),
        context: "x".repeat(65537),
        owned_paths: vec!["src".into()],
    };
    assert!(request.validate().is_err());
}

#[test]
fn integration_requires_current_complete_checks_and_review() {
    use state::{AgentRecord, AgentStatus, WorktreeIdentity};
    use workflow::{
        runtime::Identity,
        state::{CheckReceipt, ReviewReceipt},
        workspace,
    };
    let directory = tempfile::tempdir().unwrap();
    let snapshot = workspace::capture(directory.path()).unwrap();
    let connection: demoncoder::config::Connection =
        serde_json::from_value(serde_json::json!({"adapter":"anthropic-api"})).unwrap();
    let identity = Identity::from(&connection);
    let digest = snapshot.digest.clone();
    let mut record = AgentRecord {
        id: 1,
        parent_task: None,
        request: AssignmentRequest {
            connection: "worker".into(),
            objective: "repair".into(),
            context: String::new(),
            owned_paths: vec![".".into()],
        },
        identity: identity.clone(),
        worktree: Some(WorktreeIdentity {
            root: directory.path().into(),
            git_dir: directory.path().join(".git"),
            common_dir: directory.path().join(".git"),
            repository_head: "base".into(),
            baseline_commit: "baseline".into(),
            parent_baseline: snapshot.clone(),
            child_baseline: snapshot,
        }),
        status: AgentStatus::Ready,
        outcome: String::new(),
        commands: vec!["test -f repaired".into()],
        reviewer: Some(identity),
        checks: vec![CheckReceipt {
            command: "test -f repaired".into(),
            snapshot: digest.clone(),
            success: true,
            output: String::new(),
            exit_code: Some(0),
        }],
        review: Some(ReviewReceipt {
            evidence: "actual patch and source".into(),
            snapshot: digest.clone(),
            verification_generation: 1,
            reviewer: "reviewer".into(),
            findings: vec![],
            clear: true,
            explanation: "examined".into(),
        }),
        validation_generation: 1,
        activity: vec![],
        checkpoint: None,
        checkpoint_cursor: 0,
        integration: None,
        decisions: vec![],
    };
    assert!(record.can_integrate(&digest));
    assert!(!record.can_integrate("changed workspace"));
    record.status = AgentStatus::Uncertain;
    assert!(!record.can_integrate(&digest));
    record.status = AgentStatus::Ready;
    record.checks[0].success = false;
    assert!(!record.can_integrate(&digest));
    record.checks[0].success = true;
    record.validation_generation += 1;
    assert!(!record.can_integrate(&digest));
    record.validation_generation -= 1;
    record
        .review
        .as_mut()
        .unwrap()
        .findings
        .push("unresolved".into());
    assert!(!record.can_integrate(&digest));
    record.commands.clear();
    assert!(!record.can_integrate(&digest));
}

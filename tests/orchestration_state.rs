use clap::Parser;
use demoncoder::{
    config::{Args, Connection},
    subagents::{
        schedule,
        state::{
            AgentRecord, AgentStatus, AssignmentOrigin, AssignmentRequest, DelegationIdentity,
            OrchestrationIdentity, OrchestrationStage, OrchestrationState, RoleReceipt,
        },
        supervision,
    },
    workflow::{
        review::{Decision, Role, Verdict},
        runtime::Identity,
    },
};

fn identity(adapter: &str) -> Identity {
    let connection: Connection =
        serde_json::from_value(serde_json::json!({"adapter": adapter})).unwrap();
    Identity::from(&connection)
}

fn request() -> AssignmentRequest {
    AssignmentRequest {
        connection: "worker".into(),
        objective: "repair greeting".into(),
        context: String::new(),
        owned_paths: vec!["greeting".into()],
    }
}

fn legacy_record() -> AgentRecord {
    serde_json::from_value(serde_json::json!({
        "id": 1,
        "parent_task": null,
        "request": request(),
        "identity": identity("openai-api"),
        "worktree": null,
        "status": "stopped",
        "outcome": "legacy",
        "commands": [],
        "reviewer": null,
        "checks": [],
        "review": null,
        "validation_generation": 0,
        "activity": [],
        "checkpoint": null,
        "checkpoint_cursor": 0,
        "integration": null,
        "decisions": []
    }))
    .unwrap()
}

#[test]
fn old_records_and_delegation_identity_remain_readable() {
    let record = legacy_record();
    assert_eq!(record.origin, AssignmentOrigin::ParentAgent);
    assert!(record.orchestration.is_none());

    let delegation: DelegationIdentity = serde_json::from_value(serde_json::json!({
        "connections": {},
        "reviewer": null,
        "max_active": 2,
        "backend_limit": 64
    }))
    .unwrap();
    assert!(delegation.orchestration.is_none());
}

#[test]
fn correction_admission_is_durable_and_never_exceeds_two() {
    let mut state = OrchestrationState::new(vec![]);
    assert_eq!(state.admit_correction(2).unwrap(), 1);
    assert_eq!(state.admit_correction(2).unwrap(), 2);
    assert!(state.admit_correction(2).is_err());

    let restored: OrchestrationState =
        serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    assert_eq!(restored.correction_rounds, 2);
    assert!(restored.clone().admit_correction(2).is_err());
}

#[test]
fn orchestration_integration_requires_a_current_resolved_ready_stage() {
    let mut record = legacy_record();
    record.status = AgentStatus::Ready;
    record.completed = true;
    record.orchestration = Some(OrchestrationState::new(vec![]));
    assert!(!record.orchestration_allows_integration());

    record.orchestration.as_mut().unwrap().stage = OrchestrationStage::Ready;
    assert!(record.orchestration_allows_integration());
    record.orchestration.as_mut().unwrap().stage = OrchestrationStage::Held;
    assert!(!record.orchestration_allows_integration());
}

#[test]
fn dependency_projection_preserves_terminal_and_active_meaning() {
    let mut queued = legacy_record();
    queued.status = AgentStatus::Queued;
    queued.orchestration = Some(OrchestrationState::new(vec![]));
    let mut ready = legacy_record();
    ready.id = 2;
    ready.status = AgentStatus::Ready;
    ready.orchestration = Some(OrchestrationState::new(vec![1]));
    let mut failed = legacy_record();
    failed.id = 3;
    failed.status = AgentStatus::Failed;
    failed.orchestration = Some(OrchestrationState::new(vec![]));

    let nodes = supervision::dependency_nodes(&[queued, ready, failed]).unwrap();
    assert_eq!(nodes[0].status, schedule::Status::Queued);
    assert_eq!(nodes[1].status, schedule::Status::AwaitingIntegration);
    assert_eq!(nodes[1].dependencies, vec![1]);
    assert_eq!(nodes[2].status, schedule::Status::Blocked);
}

#[test]
fn role_receipts_retain_effective_identity_and_exact_evidence() {
    let evidence = r#"{"assignment":{"objective":"repair"},"checks":[]}"#;
    let receipt = RoleReceipt::new(
        Role::Judge,
        2,
        identity("anthropic-api"),
        "snapshot-2".into(),
        evidence.into(),
        Decision {
            verdict: Verdict::Findings,
            findings: vec!["still broken".into()],
            explanation: "current source still violates the assignment".into(),
        },
    )
    .unwrap();
    assert_eq!(receipt.evidence, evidence);
    assert_eq!(receipt.correction_round, 2);
    assert_eq!(receipt.role, Role::Judge);
    assert_eq!(receipt.findings, vec!["still broken"]);

    let delegation = DelegationIdentity {
        connections: Default::default(),
        reviewer: None,
        max_active: 2,
        backend_limit: 64,
        orchestration: Some(OrchestrationIdentity {
            judge: identity("anthropic-api"),
            correction_limit: 2,
            checks: vec!["cargo test".into()],
        }),
    };
    let orchestration = delegation.orchestration.unwrap();
    assert_eq!(orchestration.correction_limit, 2);
    assert_eq!(orchestration.checks, vec!["cargo test"]);
}

#[test]
fn orchestration_cli_requires_complete_trusted_supervision_selection() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("settings.toml");
    std::fs::write(
        &config,
        r#"
[connections.worker]
adapter = "openai-api"
[connections.advisor]
adapter = "anthropic-api"
[connections.judge]
adapter = "openai-api"
"#,
    )
    .unwrap();
    let base = [
        "demoncoder",
        "--config",
        config.to_str().unwrap(),
        "--agent-connection",
        "worker",
        "--check",
        "cargo test",
        "--reviewer",
        "advisor",
        "--orchestrate",
    ];
    let missing_judge = Args::try_parse_from(base).unwrap();
    assert!(missing_judge.agent_settings().is_err());

    let valid = Args::try_parse_from(base.into_iter().chain(["--judge", "judge"])).unwrap();
    let settings = valid.agent_settings().unwrap().unwrap();
    let orchestration = settings.orchestration.unwrap();
    assert_eq!(orchestration.correction_limit, 2);
    assert_eq!(
        serde_json::to_value(Identity::from(&orchestration.judge)).unwrap()["tools_enabled"],
        false
    );

    let stray_judge = Args::try_parse_from([
        "demoncoder",
        "--config",
        config.to_str().unwrap(),
        "--agent-connection",
        "worker",
        "--judge",
        "judge",
    ])
    .unwrap();
    assert!(stray_judge.agent_settings().is_err());
}

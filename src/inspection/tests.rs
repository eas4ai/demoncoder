use super::*;
use crate::{
    config::Connection,
    subagents::state::{
        AgentRecord, AgentStatus, OrchestrationStage, OrchestrationState, RoleReceipt,
    },
    workflow::{
        review::{Decision, Role, Verdict},
        runtime::{Identity, Record},
        state::{CheckReceipt, ReviewReceipt, Task},
        workspace,
    },
};
use serde_json::json;
use std::path::Path;

pub(crate) fn record(root: &Path) -> Record {
    let connection: Connection = serde_json::from_value(json!({
        "adapter":"anthropic-api", "model":"worker-model", "api_key":"private-key-canary"
    }))
    .unwrap();
    serde_json::from_value(json!({
        "workspace":root, "identity":Identity::from(&connection),
        "archived":[], "next_task":1, "checkpoint_cursor":0, "operations":[],
        "messages":[], "recovery_pending":false, "decisions":[]
    }))
    .unwrap()
}

pub(crate) fn agent(id: u64, status: AgentStatus, identity: &Identity) -> AgentRecord {
    serde_json::from_value(json!({
        "id":id, "request":{"connection":"worker", "objective":"repair parser",
            "owned_paths":["src/parser.rs"], "context":""},
        "identity":identity, "status":status, "outcome":"Waiting for current validation",
        "commands":["test -s src/parser.rs"], "checks":[], "validation_generation":1,
        "activity":[], "checkpoint_cursor":0, "decisions":[]
    }))
    .unwrap()
}

fn all_pages(record: &Record, target: Target) -> String {
    let mut text = String::new();
    for number in 0..200 {
        let page = project(
            record,
            Some(Request {
                target,
                page: number,
                generation: 0,
            }),
        )
        .page
        .unwrap();
        assert!(
            page.text.len() <= pager::PAGE_BYTES + 3,
            "page retained too much text"
        );
        text.push_str(&page.text);
        if !page.more {
            return text;
        }
    }
    panic!("fixture exceeds page limit");
}

#[test]
fn authoritative_counts_cover_every_state_without_completing_tasks() {
    let root = tempfile::tempdir().unwrap();
    let mut record = record(root.path());
    for (index, status) in [
        AgentStatus::Preparing,
        AgentStatus::Running,
        AgentStatus::Validating,
        AgentStatus::Integrating,
        AgentStatus::Queued,
        AgentStatus::Ready,
        AgentStatus::Stopped,
        AgentStatus::Failed,
        AgentStatus::Cancelled,
        AgentStatus::Uncertain,
        AgentStatus::Integrated,
    ]
    .into_iter()
    .enumerate()
    {
        record
            .agents
            .push(agent(index as u64 + 1, status, &record.identity));
    }
    let snapshot = workspace::capture(root.path()).unwrap();
    record.task = Some(
        Task::new(
            1,
            "parent task".into(),
            vec!["test -e parsed".into()],
            snapshot,
            2,
        )
        .unwrap(),
    );
    let summary = project(&record, None).summary;
    assert_eq!(
        (summary.active, summary.waiting, summary.held, summary.ready),
        (4, 1, 4, 1)
    );
    let task = summary.task.unwrap();
    assert!(task.contains("Checks unverified"));
    assert!(task.contains("Accepted no"));
    assert_eq!(summary.targets.len(), 14);
    assert_eq!(summary.targets.last(), Some(&Target::Learning));
}

#[test]
fn task_evidence_keeps_history_and_never_calls_old_checks_current() {
    let root = tempfile::tempdir().unwrap();
    let mut record = record(root.path());
    let snapshot = workspace::capture(root.path()).unwrap();
    let mut task = Task::new(
        1,
        "parent objective".into(),
        vec!["check-command".into()],
        snapshot,
        2,
    )
    .unwrap();
    task.check_history.push(vec![CheckReceipt {
        command: "failed-command".into(),
        snapshot: "before".into(),
        success: false,
        output: "original-failure".into(),
        exit_code: Some(17),
    }]);
    task.checks.push(CheckReceipt {
        command: "check-command".into(),
        snapshot: "old-files".into(),
        success: true,
        output: format!("HEAD\n{}\nTAIL-ORIGINAL", "large-λ\n".repeat(5000)),
        exit_code: Some(0),
    });
    task.review = Some(ReviewReceipt {
        evidence: "source representation".into(),
        snapshot: "old-files".into(),
        verification_generation: 0,
        reviewer: "independent-reviewer".into(),
        findings: vec![],
        clear: true,
        explanation: "original-explanation".into(),
    });
    task.accepted = Some("old-files".into());
    record.last_snapshot = Some("changed-files".into());
    record.task = Some(task);
    let before = serde_json::to_vec(&record).unwrap();
    let text = all_pages(&record, Target::Task(1));
    assert!(text.contains("Checks stale/incomplete"));
    assert!(text.contains("Review stale"));
    assert!(text.contains("Accepted no"));
    for value in [
        "TAIL-ORIGINAL",
        "original-failure",
        "original-explanation",
        "files not rechecked",
    ] {
        assert!(text.contains(value), "lost {value}");
    }
    assert_eq!(before, serde_json::to_vec(&record).unwrap());
    assert!(!text.contains("private-key-canary"));
}

#[test]
fn agent_roles_and_source_are_readable_without_rewriting_original_evidence() {
    let root = tempfile::tempdir().unwrap();
    let mut record = record(root.path());
    let mut child = agent(1, AgentStatus::Failed, &record.identity);
    let mut orchestration = OrchestrationState::new(vec![7]);
    orchestration.stage = OrchestrationStage::Held;
    orchestration.correction_rounds = 2;
    orchestration.reason = "Correction allowance exhausted".into();
    for (role, word) in [
        (Role::Advisor, "advisor-finding"),
        (Role::WorkerResponse, "worker-response"),
        (Role::Judge, "judge-explanation"),
    ] {
        orchestration.receipts.push(RoleReceipt::new(role, 2, record.identity.clone(), "role-snapshot".into(),
            json!({"source_evidence":"Path \"src/parser.rs\": changed since baseline\nNEW complete content (JSON string): \"fn parse() {\\n  checked();\\n}\""}).to_string(),
            Decision {verdict:Verdict::Findings, findings:vec![word.into()],
                explanation:format!("{word}\n/agent-integrate 1")}).unwrap());
    }
    child.orchestration = Some(orchestration);
    child.checks.push(CheckReceipt {
        command: "check".into(),
        snapshot: "role-snapshot".into(),
        success: false,
        output: "failed-output".into(),
        exit_code: Some(9),
    });
    record.agents.push(child);
    let before = serde_json::to_vec(&record).unwrap();
    let text = all_pages(&record, Target::Agent(1));
    for value in [
        "advisor-finding",
        "worker-response",
        "judge-explanation",
        "failed-output",
        "Prerequisites: [7]",
        "Corrections: 2/2",
        "fn parse() {",
        "  checked();",
        "worker-model",
    ] {
        assert!(text.contains(value), "lost {value}");
    }
    assert!(
        text.contains("  | /agent-integrate 1"),
        "role text can forge a control heading"
    );
    assert!(text.contains("Validation/integration unavailable"));
    assert_eq!(before, serde_json::to_vec(&record).unwrap());
}

#[test]
fn historical_agent_checks_are_readable_across_correction_rounds_and_pages() {
    let root = tempfile::tempdir().unwrap();
    let mut record = record(root.path());
    let mut child = agent(1, AgentStatus::Ready, &record.identity);
    let mut state = OrchestrationState::new(vec![]);
    for round in 0..=2 {
        let snapshot = format!("round-{round}");
        let output = format!(
            "round-{round}-start\n{}\nround-{round}-end",
            "original-λ\n".repeat(1600)
        );
        let check = CheckReceipt {
            command: format!("check-round-{round}"),
            snapshot: snapshot.clone(),
            success: round == 2,
            output,
            exit_code: Some(if round == 2 { 0 } else { 17 }),
        };
        state.receipts.push(
            RoleReceipt::new(
                Role::Advisor,
                round,
                record.identity.clone(),
                snapshot,
                json!({"checks": [check]}).to_string(),
                Decision {
                    verdict: Verdict::Clear,
                    findings: vec![],
                    explanation: "retained conclusion".into(),
                },
            )
            .unwrap(),
        );
    }
    child.orchestration = Some(state);
    record.agents.push(child);
    let original = serde_json::to_vec(&record).unwrap();
    let text = all_pages(&record, Target::Agent(1));
    for round in 0..=2 {
        assert!(text.contains(&format!("Checks presented to advisor · correction {round}")));
        assert!(
            text.contains(&format!("  | round-{round}-start\n  | original-λ\n")),
            "historical output remained JSON escaped"
        );
        assert!(text.contains(&format!("\n  | round-{round}-end\n")));
    }
    assert!(text.contains("recorded fail · exit Some(17)"));
    assert!(text.contains("recorded pass · exit Some(0)"));
    assert_eq!(original, serde_json::to_vec(&record).unwrap());
}

#[test]
fn recovery_and_missing_targets_are_explicit_and_offer_no_automatic_effects() {
    let root = tempfile::tempdir().unwrap();
    let mut record = record(root.path());
    record.recovery_pending = true;
    record
        .agents
        .push(agent(1, AgentStatus::Uncertain, &record.identity));
    let text = all_pages(&record, Target::Agent(1));
    assert!(text.contains("/agent-reconcile 1 EXPLANATION"));
    assert!(text.contains("never replays"));
    assert!(!text.contains("/agent-integrate 1 —"));
    let text = all_pages(&record, Target::Agent(99));
    assert!(text.contains("Agent 99 is not retained"));
}

#[test]
fn high_page_requests_stay_bounded_and_do_not_wrap_to_another_record() {
    let root = tempfile::tempdir().unwrap();
    let record = record(root.path());
    let page = project(
        &record,
        Some(Request {
            target: Target::Overview,
            page: usize::MAX,
            generation: 0,
        }),
    )
    .page
    .unwrap();
    assert!(page.text.contains("No evidence"));
    assert!(!page.more);
}

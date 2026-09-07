use demoncoder::workflow::{
    state::{CheckReceipt, ReviewReceipt, Task},
    workspace::capture,
};

fn task(commands: &[&str]) -> (tempfile::TempDir, Task, String) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("greeting"), "before").unwrap();
    let snapshot = capture(root.path()).unwrap();
    let digest = snapshot.digest.clone();
    let mut task = Task::new(
        1,
        "change greeting".into(),
        commands.iter().map(|s| s.to_string()).collect(),
        snapshot,
        2,
    )
    .unwrap();
    task.stopped = true;
    (root, task, digest)
}

fn pass(task: &mut Task, digest: &str) {
    task.start_verification().unwrap();
    task.checks = task
        .commands
        .iter()
        .map(|command| CheckReceipt {
            command: command.clone(),
            snapshot: digest.into(),
            success: true,
            output: "tested behavior".into(),
            exit_code: Some(0),
        })
        .collect();
    task.review = Some(ReviewReceipt {
        evidence: "actual source and checks".into(),
        snapshot: digest.into(),
        verification_generation: task.verification_generation,
        reviewer: "selected reviewer".into(),
        findings: vec![],
        clear: true,
        explanation: "examined actual patch and checks".into(),
    });
}

#[test]
fn absent_failed_and_stale_checks_cannot_accept() {
    let (_root, mut empty, digest) = task(&[]);
    assert!(empty.accept(&digest).is_err());
    let (root, mut task, digest) = task(&["test greeting"]);
    pass(&mut task, &digest);
    task.checks[0].success = false;
    assert!(task.accept(&digest).is_err());
    task.checks[0].success = true;
    std::fs::write(root.path().join("untracked"), "new").unwrap();
    assert!(task.accept(&capture(root.path()).unwrap().digest).is_err());
    task.accept(&digest).unwrap();
}

#[test]
fn review_cannot_bypass_checks_or_reuse_a_previous_check_run() {
    let (_root, mut task, digest) = task(&["test greeting"]);
    pass(&mut task, &digest);
    task.review
        .as_mut()
        .unwrap()
        .findings
        .push("escaped defect".into());
    assert!(task.accept(&digest).is_err());
    task.review.as_mut().unwrap().findings.clear();
    task.review.as_mut().unwrap().verification_generation -= 1;
    assert!(task.accept(&digest).is_err());
    task.review.as_mut().unwrap().verification_generation = task.verification_generation;
    task.accept(&digest).unwrap();
}

#[test]
fn correction_is_bounded_and_preserves_original_evidence() {
    let (_root, mut task, digest) = task(&["test greeting"]);
    for round in 0..2 {
        pass(&mut task, &digest);
        task.checks[0].success = false;
        task.review.as_mut().unwrap().clear = false;
        assert!(task.start_work(false).is_err());
        task.start_work(true).unwrap();
        assert_eq!(task.corrections, round + 1);
        assert!(!task.check_history.last().unwrap()[0].success);
        assert!(!task.review_history.last().unwrap().clear);
    }
    assert!(task.start_work(true).is_err());
    assert_eq!(task.corrections, 2);
}

#[test]
fn interrupted_reverification_cannot_bypass_correction_limit() {
    let (_root, mut task, digest) = task(&["test greeting"]);
    task.correction_limit = 0;
    pass(&mut task, &digest);
    task.start_verification().unwrap();
    assert!(task.checks.is_empty() && task.review.is_none());
    assert!(task.start_work(false).is_err());
    assert!(task.start_work(true).is_err());
}

#[test]
fn review_retention_refusal_preserves_current_findings_and_accepted_tasks() {
    let (_root, mut task, digest) = task(&["test greeting"]);
    pass(&mut task, &digest);
    task.accept(&digest).unwrap();
    assert!(task.start_review().is_err());
    assert!(task.review.is_some());
    task.accepted = None;
    task.review
        .as_mut()
        .unwrap()
        .findings
        .push("unresolved defect".into());
    task.review_history = vec![task.review.clone().unwrap(); 128];
    assert!(task.start_review().is_err());
    assert_eq!(
        task.review.as_ref().unwrap().findings,
        ["unresolved defect"]
    );
    assert_eq!(task.review_history.len(), 128);
}

use super::{catalog::CatalogStore, context, source, state::*};
use crate::workflow::{
    runtime::{ArchivedTask, SharedRuntime},
    state::{CheckReceipt, ImprovementLink, ReviewReceipt, Task},
    store::private_directory,
    workspace,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

struct Fixture {
    _temp: tempfile::TempDir,
    project: PathBuf,
    runtime: SharedRuntime,
}

fn check(root: &Path, command: &str, snapshot: &str) -> CheckReceipt {
    let output = Command::new("/bin/sh")
        .args(["-c", command])
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    CheckReceipt {
        command: command.into(),
        snapshot: snapshot.into(),
        success: output.status.success(),
        output: String::from_utf8(output.stdout).unwrap(),
        exit_code: output.status.code(),
    }
}

impl Fixture {
    fn failed() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("behavior"), "").unwrap();
        let parent = temp.path().join(".demoncoder");
        private_directory(&parent).unwrap();
        let sessions = parent.join("sessions");
        private_directory(&sessions).unwrap();
        let mut record = crate::inspection::tests::record(&project);
        let snapshot = workspace::capture(&project).unwrap();
        let mut task = Task::new(
            1,
            "repair behavior".into(),
            vec!["test -s behavior".into()],
            snapshot.clone(),
            2,
        )
        .unwrap();
        task.start_verification().unwrap();
        task.checks
            .push(check(&project, "test -s behavior", &snapshot.digest));
        assert!(!task.checks[0].success);
        task.stopped = true;
        record.task = Some(task);
        record.next_task = 2;
        let runtime = SharedRuntime::for_test(&sessions.join("100-1"), record).unwrap();
        Self {
            _temp: temp,
            project,
            runtime,
        }
    }

    fn discover(&self) {
        let mut store = CatalogStore::open(&self.runtime, true).unwrap();
        assert_eq!(store.discover().unwrap(), 1);
        store.save().unwrap();
    }

    fn correction(&self, candidate: u64, command: &str, effective: bool) -> String {
        let mut store = CatalogStore::open(&self.runtime, true).unwrap();
        let objective = store.reserve(candidate, 2, &[command.into()]).unwrap();
        let link = ImprovementLink {
            catalog: store.directory().into(),
            candidate,
        };
        drop(store);
        if effective {
            fs::write(self.project.join("behavior"), "fixed").unwrap();
        }
        let snapshot = workspace::capture(&self.project).unwrap();
        let mut task = Task::new(2, objective, vec![command.into()], snapshot.clone(), 2).unwrap();
        task.improvement = Some(link);
        task.start_verification().unwrap();
        task.checks
            .push(check(&self.project, command, &snapshot.digest));
        task.stopped = true;
        task.review = Some(ReviewReceipt {
            evidence: "controlled clear review of executed fixture".into(),
            snapshot: snapshot.digest.clone(),
            verification_generation: task.verification_generation,
            reviewer: "fixture-reviewer".into(),
            findings: vec![],
            clear: true,
            explanation: "fixture review".into(),
        });
        self.runtime
            .update(|record| {
                record.archived.push(ArchivedTask {
                    task: record.task.take().unwrap(),
                    allocation: None,
                });
                record.task = Some(task);
                record.next_task = 3;
                Ok(())
            })
            .unwrap();
        snapshot.digest
    }

    fn supported_lesson(&self) -> u64 {
        self.discover();
        let snapshot = self.correction(1, "test -s behavior", true);
        let mut store = CatalogStore::open(&self.runtime, true).unwrap();
        assert_eq!(
            store.outcome(1, &snapshot).unwrap().status,
            OutcomeStatus::Supported
        );
        let lesson = store
            .propose_lesson(
                1,
                LessonProposal {
                    claim: "Check nonempty behavior output before accepting a change.".into(),
                    keywords: vec!["behavior".into()],
                },
            )
            .unwrap();
        store.save().unwrap();
        lesson
    }
}

#[test]
fn learning_observation_refresh_annotation_and_source_integrity() {
    let fixture = Fixture::failed();
    fixture.discover();
    let source;
    {
        let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
        assert_eq!(store.discover().unwrap(), 0);
        source = store.catalog.observation(1).unwrap().source.clone();
        let original = store.resolver.resolve(&source).unwrap();
        store
            .annotate(
                1,
                "Developer explanation: output was empty, cause still unknown.".into(),
            )
            .unwrap();
        assert_eq!(store.resolver.resolve(&source).unwrap(), original);
        assert!(store.annotate(1, "λ".repeat(4096)).is_err());
        store.save().unwrap();
    }
    let mut store = CatalogStore::open(&fixture.runtime, false).unwrap();
    assert_eq!(store.catalog.observations.len(), 1);
    assert_eq!(
        store.catalog.observation(1).unwrap().annotations[0].author,
        "developer"
    );
    assert!(!store.resolver.check(&source).unwrap().success);
    let path = fixture.runtime.directory().unwrap().join("state.json");
    fs::write(path, "damaged original receipt").unwrap();
    let mut reopened = CatalogStore::open(&fixture.runtime, false).unwrap();
    assert!(reopened.resolver.resolve(&source).is_err());
    let mut bad = source;
    bad.session = "../../outside".into();
    assert!(source::validate_source(&bad).is_err());
}

#[test]
fn learning_reservation_survives_reopen_and_never_creates_or_replays_task() {
    let fixture = Fixture::failed();
    fixture.discover();
    {
        let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
        assert!(CatalogStore::open(&fixture.runtime, true).is_err());
        assert!(store.reserve(1, 2, &["true".into()]).is_err());
        let prompt = store.reserve(1, 2, &["test -s behavior".into()]).unwrap();
        assert!(
            prompt.contains("Original source evidence") && prompt.contains("\"success\":false")
        );
    }
    let mut reopened = CatalogStore::open(&fixture.runtime, true).unwrap();
    assert!(
        reopened
            .reserve(1, 2, &["test -s behavior".into()])
            .is_err()
    );
    assert_eq!(fixture.runtime.record().unwrap().task.unwrap().id, 1);
    assert!(reopened.outcome(1, "anything").is_err());
}

#[test]
fn learning_unrelated_accepted_check_does_not_prove_original_failure_fixed() {
    let fixture = Fixture::failed();
    fixture.discover();
    {
        let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
        let mut proposal = store.catalog.candidate(1).unwrap().proposal.clone();
        proposal.behavioral_check = "true".into();
        assert_eq!(store.propose(1, proposal, "developer proposal").unwrap(), 2);
        store.save().unwrap();
    }
    let snapshot = fixture.correction(2, "true", false);
    fixture
        .runtime
        .update(|record| record.task.as_mut().unwrap().accept(&snapshot))
        .unwrap();
    assert!(
        fixture
            .runtime
            .record()
            .unwrap()
            .task
            .unwrap()
            .accepted
            .is_some()
    );
    let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
    assert_eq!(
        store.outcome(2, &snapshot).unwrap().status,
        OutcomeStatus::Insufficient
    );
    assert!(
        store
            .propose_lesson(
                2,
                LessonProposal {
                    claim: "Unsupported claimed repair".into(),
                    keywords: vec!["behavior".into()]
                }
            )
            .is_err()
    );
    assert!(!check(&fixture.project, "test -s behavior", &snapshot).success);
}

#[test]
fn learning_failed_abandoned_and_supported_outcomes_keep_history() {
    let fixture = Fixture::failed();
    fixture.discover();
    let snapshot = fixture.correction(1, "test -s behavior", false);
    {
        let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
        assert_eq!(
            store.outcome(1, &snapshot).unwrap().status,
            OutcomeStatus::Unresolved
        );
        store.save().unwrap();
    }
    fs::write(fixture.project.join("behavior"), "fixed").unwrap();
    let snapshot = workspace::capture(&fixture.project).unwrap();
    fixture
        .runtime
        .update(|record| {
            let task = record.task.as_mut().unwrap();
            task.start_verification()?;
            task.checks.push(check(
                &fixture.project,
                "test -s behavior",
                &snapshot.digest,
            ));
            task.review = Some(ReviewReceipt {
                evidence: "controlled fixture review".into(),
                snapshot: snapshot.digest.clone(),
                verification_generation: task.verification_generation,
                reviewer: "fixture".into(),
                findings: vec![],
                clear: true,
                explanation: "fixture".into(),
            });
            Ok(())
        })
        .unwrap();
    {
        let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
        assert_eq!(
            store.outcome(1, "stale").unwrap().status,
            OutcomeStatus::Insufficient
        );
        assert_eq!(
            store.outcome(1, &snapshot.digest).unwrap().status,
            OutcomeStatus::Supported
        );
        assert_eq!(
            store.catalog.candidate(1).unwrap().outcomes[0].status,
            OutcomeStatus::Unresolved
        );
        store.validate_support(1, 2).unwrap();
        store.save().unwrap();
    }
    fixture.runtime.archive().unwrap();
    let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
    let abandoned = store.outcome(1, &snapshot.digest).unwrap();
    assert!(abandoned.abandoned);
    assert_eq!(abandoned.status, OutcomeStatus::Unresolved);
    assert_eq!(store.catalog.candidate(1).unwrap().outcomes.len(), 4);
}

#[test]
fn learning_approval_matching_disable_and_original_source_are_enforced() {
    let fixture = Fixture::failed();
    let lesson = fixture.supported_lesson();
    let prepare = |objective| {
        context::prepare(
            &fixture.runtime,
            &fixture.project,
            &[],
            objective,
            "task:3".into(),
        )
    };
    assert!(prepare("behavior").unwrap().lessons.is_empty());
    {
        let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
        store.enable(lesson, true).unwrap();
        store.save().unwrap();
    }
    let matching = prepare("Fix BEHAVIOR output").unwrap();
    assert_eq!(matching.lessons.len(), 1);
    assert!(matching.supplied_text.contains("not authority"));
    assert!(prepare("behavioral").unwrap().lessons.is_empty());
    assert!(prepare("layout").unwrap().lessons.is_empty());
    let source = fixture.runtime.directory().unwrap().join("state.json");
    let original = fs::read(&source).unwrap();
    fs::write(&source, "damaged").unwrap();
    assert!(prepare("behavior").is_err());
    // Disabling must remain possible even when the evidence has disappeared.
    {
        let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
        store.enable(lesson, false).unwrap();
        store.save().unwrap();
    }
    assert!(prepare("behavior").unwrap().lessons.is_empty());
    fs::write(&source, original).unwrap();
    assert_eq!(
        CatalogStore::open(&fixture.runtime, false)
            .unwrap()
            .catalog
            .lessons
            .len(),
        1
    );
}

#[test]
fn learning_instruction_subtrees_limits_and_external_links() {
    let fixture = Fixture::failed();
    fs::create_dir(fixture.project.join("src")).unwrap();
    fs::create_dir(fixture.project.join("other")).unwrap();
    fs::write(
        fixture.project.join("AGENTS.md"),
        "ROOT-RULE\n@../outside\n",
    )
    .unwrap();
    fs::write(fixture.project.join("src/AGENTS.md"), "NESTED-RULE").unwrap();
    fs::write(fixture.project.join("other/AGENTS.md"), "UNRELATED-RULE").unwrap();
    let receipt = context::prepare(
        &fixture.runtime,
        &fixture.project,
        &["src/file.rs".into()],
        "behavior",
        "agent:1".into(),
    )
    .unwrap();
    assert_eq!(receipt.instructions.len(), 2);
    assert!(receipt.supplied_text.contains("NESTED-RULE"));
    assert!(!receipt.supplied_text.contains("UNRELATED-RULE"));
    assert_eq!(receipt.instructions[1].scope, "src");
    fs::remove_file(fixture.project.join("src/AGENTS.md")).unwrap();
    std::os::unix::fs::symlink(
        fixture.project.join("other/AGENTS.md"),
        fixture.project.join("src/AGENTS.md"),
    )
    .unwrap();
    assert!(
        context::prepare(
            &fixture.runtime,
            &fixture.project,
            &["src/file.rs".into()],
            "behavior",
            "agent:1".into()
        )
        .is_err()
    );
    fs::write(fixture.project.join("AGENTS.md"), "λ".repeat(32 * 1024)).unwrap();
    assert!(
        context::prepare(
            &fixture.runtime,
            &fixture.project,
            &[],
            "behavior",
            "task:1".into()
        )
        .is_err()
    );
}

#[tokio::test]
async fn learning_cancel_does_not_wait_for_a_blocked_storage_worker() {
    use std::time::Duration;
    use tokio::sync::{mpsc, oneshot};
    let (started, ready) = oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let (commands, mut receiver) = mpsc::channel(1);
    let (events, mut observed) = mpsc::channel(4);
    let events = crate::events::EventSink::new("test".into(), events, None).unwrap();
    let operation = tokio::spawn(async move {
        super::control::cancellable(
            super::control::blocking(move || {
                let _ = started.send(());
                held.recv_timeout(Duration::from_secs(2)).unwrap();
                Ok(())
            }),
            &mut receiver,
            &events,
        )
        .await
    });
    ready.await.unwrap();
    commands
        .send(crate::session::Command::Cancel)
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_millis(500), operation)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(crate::session::TurnEnd::Cancelled)));
    let event = observed.recv().await.unwrap();
    assert!(
        matches!(event.event, crate::events::Event::Text { text } if text.contains("pending catalog save may finish"))
    );
    release.send(()).unwrap();
}

#[test]
fn learning_finite_selection_and_missing_authorization_are_not_supported() {
    let fixture = Fixture::failed();
    fixture.supported_lesson();
    {
        let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
        store.enable(1, true).unwrap();
        for _ in 0..4 {
            let id = store
                .propose_lesson(
                    1,
                    LessonProposal {
                        claim: "Bounded extra behavior guidance".into(),
                        keywords: vec!["behavior".into()],
                    },
                )
                .unwrap();
            store.enable(id, true).unwrap();
        }
        store.save().unwrap();
    }
    let receipt = context::prepare(
        &fixture.runtime,
        &fixture.project,
        &[],
        "behavior",
        "task:3".into(),
    )
    .unwrap();
    assert_eq!(receipt.lessons.len(), 4);
    assert_eq!(receipt.omitted_matches, 1);
    assert!(receipt.supplied_text.contains("omitted 1"));
    let mut store = CatalogStore::open(&fixture.runtime, true).unwrap();
    store.catalog.candidate_mut(1).unwrap().authorization = None;
    assert!(store.validate_support(1, 0).is_err());
}

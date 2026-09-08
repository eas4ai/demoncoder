use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    time::{Duration, Instant},
};

use demoncoder::workflow::workspace::{CaptureScope, capture, capture_with_scope, review_evidence};
use tempfile::tempdir;

#[test]
fn declared_outputs_skip_large_artifacts_but_scope_and_source_changes_invalidate_identity() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("source.rs"), "source").unwrap();
    let scope = CaptureScope::new(vec!["build".into()]).unwrap();
    let before = capture_with_scope(root.path(), &scope).unwrap();
    fs::create_dir(root.path().join("build")).unwrap();
    let artifact = fs::File::create(root.path().join("build/artifact")).unwrap();
    artifact.set_len(9 * 1024 * 1024).unwrap();
    fs::write(root.path().join("build/note"), "GENERATED_CONTENT_CANARY").unwrap();
    let after = capture_with_scope(root.path(), &scope).unwrap();
    assert_eq!(before.digest, after.digest);
    assert!(
        !serde_json::to_string(&after)
            .unwrap()
            .contains("GENERATED_CONTENT_CANARY")
    );
    assert!(
        review_evidence(&before, &after)
            .unwrap()
            .contains("Generated-output scope")
    );
    assert!(
        capture(root.path()).is_err(),
        "undeclared large output must still refuse capture"
    );
    fs::write(root.path().join("build-other"), "in-scope input").unwrap();
    assert_ne!(
        after.digest,
        capture_with_scope(root.path(), &scope).unwrap().digest
    );
    fs::remove_file(root.path().join("build-other")).unwrap();
    fs::write(root.path().join("source.rs"), "changed input").unwrap();
    assert_ne!(
        after.digest,
        capture_with_scope(root.path(), &scope).unwrap().digest
    );
    let changed_scope = CaptureScope::new(vec!["build".into(), "other".into()]).unwrap();
    let changed = capture_with_scope(root.path(), &changed_scope).unwrap();
    assert_ne!(
        changed.digest,
        capture_with_scope(root.path(), &scope).unwrap().digest
    );
    assert!(
        review_evidence(&after, &changed)
            .unwrap_err()
            .to_string()
            .contains("scope changed")
    );
}

#[test]
fn generated_output_scope_is_explicit_bounded_and_cannot_name_the_root_or_private_paths() {
    for path in [
        "",
        ".",
        "..",
        "../outside",
        "/outside",
        ".git",
        "nested/.git",
        ".env",
        "a\nb",
    ] {
        assert!(
            CaptureScope::new(vec![path.into()]).is_err(),
            "accepted {path:?}"
        );
    }
    assert!(CaptureScope::new(vec!["build".into(); 129]).is_err());
    assert_eq!(
        CaptureScope::new(vec!["b".into(), "a".into(), "b".into()]).unwrap(),
        CaptureScope::new(vec!["a".into(), "b".into()]).unwrap()
    );
    let malformed: CaptureScope = serde_json::from_str(r#"{"generated_outputs":["."]}"#).unwrap();
    assert!(capture_with_scope(tempdir().unwrap().path(), &malformed).is_err());
}

#[test]
fn bounded_review_keeps_changed_source_selected_context_and_omitted_identities() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("change.rs"), "COMPLETE_OLD_SOURCE").unwrap();
    fs::write(root.path().join("support.rs"), "SELECTED_SUPPORT").unwrap();
    fs::write(
        root.path().join("large.rs"),
        format!("OMITTED_CONTENT{}", "x".repeat(1_200_000)),
    )
    .unwrap();
    let scope = CaptureScope::default()
        .with_review_context(Some(vec!["support.rs".into()]))
        .unwrap();
    let before = capture_with_scope(root.path(), &scope).unwrap();
    assert!(
        serde_json::to_string(&before)
            .unwrap()
            .contains("OMITTED_CONTENT"),
        "review scope must not omit verification inputs"
    );
    fs::write(root.path().join("change.rs"), "COMPLETE_NEW_SOURCE").unwrap();
    let after = capture_with_scope(root.path(), &scope).unwrap();
    let evidence = review_evidence(&before, &after).unwrap();
    for text in [
        "COMPLETE_OLD_SOURCE",
        "COMPLETE_NEW_SOURCE",
        "SELECTED_SUPPORT",
        "large.rs",
        "sha256",
        "not reviewed",
        "review_context",
    ] {
        assert!(evidence.contains(text), "missing {text}");
    }
    assert!(!evidence.contains("OMITTED_CONTENT"));
    fs::write(root.path().join("large.rs"), "y".repeat(1_200_000)).unwrap();
    let changed = capture_with_scope(root.path(), &scope).unwrap();
    assert_ne!(changed.digest, after.digest);
    assert!(
        review_evidence(&before, &changed)
            .unwrap_err()
            .to_string()
            .contains("1 MiB")
    );
}

#[test]
fn review_context_is_validated_and_changes_snapshot_identity() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("source"), "source").unwrap();
    let all = capture(root.path()).unwrap();
    let scope = CaptureScope::default()
        .with_review_context(Some(vec![]))
        .unwrap();
    let selected = capture_with_scope(root.path(), &scope).unwrap();
    assert_ne!(all.digest, selected.digest);
    assert!(review_evidence(&all, &selected).is_err());
    assert!(
        CaptureScope::new(vec!["build".into()])
            .unwrap()
            .with_review_context(Some(vec!["build/file".into()]))
            .is_err()
    );
    assert!(
        CaptureScope::default()
            .with_review_context(Some(vec!["../outside".into()]))
            .is_err()
    );
    let missing = CaptureScope::default()
        .with_review_context(Some(vec!["missing".into()]))
        .unwrap();
    let snapshot = capture_with_scope(root.path(), &missing).unwrap();
    assert!(
        review_evidence(&snapshot, &snapshot)
            .unwrap_err()
            .to_string()
            .contains("absent")
    );
    fs::write(root.path().join("binary"), [0, 1]).unwrap();
    let before = capture_with_scope(root.path(), &scope).unwrap();
    fs::write(root.path().join("binary"), [0, 2]).unwrap();
    assert!(
        review_evidence(&before, &capture_with_scope(root.path(), &scope).unwrap())
            .unwrap_err()
            .to_string()
            .contains("binary")
    );
}

#[test]
fn private_source_never_enters_capture_or_review_even_from_older_snapshots() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join(".demoncoder")).unwrap();
    fs::write(
        root.path().join(".demoncoder/token"),
        "PRIVATE_RUNTIME_CANARY",
    )
    .unwrap();
    fs::write(root.path().join(".env"), "PRIVATE_ENV_CANARY").unwrap();
    fs::write(root.path().join("source.rs"), "public source").unwrap();
    let snapshot = capture(root.path()).unwrap();
    let retained = serde_json::to_string(&snapshot).unwrap();
    assert!(!retained.contains("PRIVATE_RUNTIME_CANARY"));
    assert!(!retained.contains("PRIVATE_ENV_CANARY"));
    let mut older = serde_json::to_value(&snapshot).unwrap();
    older.as_object_mut().unwrap().remove("export_policy");
    let mut private = older["entries"]["./source.rs"].clone();
    private["text"] = "OLDER_PRIVATE_CANARY".into();
    older["entries"]["./.demoncoder/token"] = private;
    let older = serde_json::from_value(older).unwrap();
    let evidence = review_evidence(&older, &snapshot).unwrap();
    assert!(!evidence.contains("OLDER_PRIVATE_CANARY"));
    assert!(evidence.contains("public source"));
    assert!(evidence.contains(".demoncoder") && evidence.contains("excluded"));
}

#[test]
fn unchanged_capture_and_serialization_are_deterministic() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("existing.rs"), "let old = 1;\n").unwrap();
    fs::create_dir(root.path().join("nested")).unwrap();
    let first = capture(root.path()).unwrap();
    assert_eq!(first.digest, capture(root.path()).unwrap().digest);
    let restored = serde_json::from_str(&serde_json::to_string(&first).unwrap()).unwrap();
    assert_eq!(first, restored);
}

#[test]
fn edits_additions_removals_modes_and_symlink_targets_change_identity() {
    let root = tempdir().unwrap();
    let file = root.path().join("preexisting.txt");
    fs::write(&file, "old").unwrap();
    let mut previous = capture(root.path()).unwrap().digest;
    let mut changed = || {
        let next = capture(root.path()).unwrap().digest;
        assert_ne!(previous, next);
        previous = next;
    };
    fs::write(&file, "new").unwrap();
    changed();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
    changed();
    fs::write(root.path().join("untracked.txt"), "untracked").unwrap();
    changed();
    fs::remove_file(file).unwrap();
    changed();
    symlink("outside-a", root.path().join("link")).unwrap();
    changed();
    fs::remove_file(root.path().join("link")).unwrap();
    symlink("outside-b", root.path().join("link")).unwrap();
    changed();
}

#[test]
fn evidence_contains_actual_old_new_preexisting_and_untracked_text() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("preexisting.rs"), "old developer edit\n").unwrap();
    fs::write(root.path().join("retained.rs"), "relevant source\n").unwrap();
    fs::write(root.path().join("deleted.rs"), "deleted content\n").unwrap();
    fs::write(root.path().join(".gitignore"), "ignored.rs\n").unwrap();
    let before = capture(root.path()).unwrap();
    fs::write(root.path().join("preexisting.rs"), "new worker edit\n").unwrap();
    fs::write(
        root.path().join("ignored.rs"),
        "new untracked ignored content\n",
    )
    .unwrap();
    fs::remove_file(root.path().join("deleted.rs")).unwrap();
    let evidence = review_evidence(&before, &capture(root.path()).unwrap()).unwrap();
    for expected in [
        "old developer edit",
        "new worker edit",
        "relevant source",
        "deleted content",
        "new untracked ignored content",
        "pre-existing",
        "untracked/ignored",
        "NEW: absent",
    ] {
        assert!(
            evidence.contains(expected),
            "missing {expected}: {evidence}"
        );
    }
}

#[test]
fn symlink_targets_are_never_read_and_only_root_git_is_excluded() {
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret"), "OUTSIDE_SECRET_CANARY").unwrap();
    symlink(outside.path(), root.path().join("link")).unwrap();
    symlink(outside.path(), root.path().join(".git")).unwrap();
    fs::create_dir_all(root.path().join("nested/.git")).unwrap();
    fs::write(root.path().join("nested/.git/config"), "nested content").unwrap();
    let before = capture(root.path()).unwrap();
    let evidence = review_evidence(&before, &before).unwrap();
    assert!(!evidence.contains("OUTSIDE_SECRET_CANARY"));
    assert!(evidence.contains("nested content"));
    fs::write(outside.path().join("secret"), "CHANGED_OUTSIDE").unwrap();
    assert_eq!(before.digest, capture(root.path()).unwrap().digest);
}

#[test]
fn special_files_are_rejected_without_opening_or_waiting() {
    let root = tempdir().unwrap();
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        root.path().join("pipe"),
        rustix::fs::Mode::RUSR,
    )
    .unwrap();
    let started = Instant::now();
    assert!(
        capture(root.path())
            .unwrap_err()
            .to_string()
            .contains("special file")
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn changed_binary_blocks_review_but_unchanged_binary_is_identified() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("image.bin"), [0, 1, 2]).unwrap();
    let before = capture(root.path()).unwrap();
    assert!(
        review_evidence(&before, &before)
            .unwrap()
            .contains("Unchanged binary")
    );
    fs::write(root.path().join("image.bin"), [0, 1, 3]).unwrap();
    assert!(
        review_evidence(&before, &capture(root.path()).unwrap())
            .unwrap_err()
            .to_string()
            .contains("binary")
    );
}

#[test]
fn oversized_file_and_evidence_are_explicit_refusals() {
    let root = tempdir().unwrap();
    let file = fs::File::create(root.path().join("large")).unwrap();
    file.set_len(8 * 1024 * 1024 + 1).unwrap();
    assert!(
        capture(root.path())
            .unwrap_err()
            .to_string()
            .contains("8 MiB")
    );
    fs::write(root.path().join("large"), "x".repeat(1024 * 1024)).unwrap();
    let snapshot = capture(root.path()).unwrap();
    assert!(
        review_evidence(&snapshot, &snapshot)
            .unwrap_err()
            .to_string()
            .contains("1 MiB")
    );
}

#[test]
fn hardlinks_and_symlink_roots_are_rejected() {
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("secret"), "secret").unwrap();
    fs::hard_link(outside.path().join("secret"), root.path().join("alias")).unwrap();
    assert!(
        capture(root.path())
            .unwrap_err()
            .to_string()
            .contains("multiply linked")
    );
    symlink(root.path(), outside.path().join("root-alias")).unwrap();
    assert!(capture(&outside.path().join("root-alias")).is_err());
}

#[test]
fn depth_is_bounded_and_replaced_roots_cannot_share_review() {
    let root = tempdir().unwrap();
    let before = capture(root.path()).unwrap();
    let other = tempdir().unwrap();
    assert!(review_evidence(&before, &capture(other.path()).unwrap()).is_err());
    let mut path = root.path().to_path_buf();
    for _ in 0..65 {
        path.push("d");
        fs::create_dir(&path).unwrap();
    }
    assert!(
        capture(root.path())
            .unwrap_err()
            .to_string()
            .contains("depth limit")
    );
}

#[test]
fn concurrent_symlink_replacement_cannot_read_outside_content() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let secret = outside.path().join("secret");
    fs::write(&secret, "RACE_OUTSIDE_SECRET_CANARY").unwrap();
    let target = root.path().join("target");
    fs::write(&target, "inside").unwrap();
    let running = Arc::new(AtomicBool::new(true));
    let thread_running = Arc::clone(&running);
    let scratch = root.path().join("swap");
    let attacker = std::thread::spawn(move || {
        while thread_running.load(Ordering::Relaxed) {
            symlink(&secret, &scratch).unwrap();
            fs::rename(&scratch, &target).unwrap();
            fs::write(&scratch, "inside").unwrap();
            fs::rename(&scratch, &target).unwrap();
        }
    });
    let mut leaked = false;
    for _ in 0..50 {
        if let Ok(snapshot) = capture(root.path()) {
            leaked |= serde_json::to_string(&snapshot)
                .unwrap()
                .contains("RACE_OUTSIDE_SECRET_CANARY");
        }
    }
    running.store(false, Ordering::Relaxed);
    attacker.join().unwrap();
    assert!(!leaked);
}

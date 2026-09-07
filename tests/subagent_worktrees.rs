use demoncoder::subagents::{
    state::AssignmentRequest,
    worktree::{build_delta, inspect, integrate, prepare},
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::Command,
};

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn repository() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("parent");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "test@localhost"]);
    git(&root, &["config", "user.name", "Test"]);
    fs::write(root.join("owned"), "base\n").unwrap();
    fs::write(root.join("unrelated"), "base\n").unwrap();
    fs::write(root.join("deleted"), "deleted\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "baseline"]);
    let child = temp.path().join("child");
    (temp, root, child)
}
fn request(owned: &[&str]) -> AssignmentRequest {
    AssignmentRequest {
        connection: "test".into(),
        objective: "edit owned files".into(),
        context: String::new(),
        owned_paths: owned.iter().map(|s| s.to_string()).collect(),
    }
}
#[tokio::test]
async fn captures_dirty_binary_ignored_deleted_modes_and_symlinks_without_touching_parent_index() {
    let (_temp, parent, child) = repository();
    fs::write(parent.join("owned"), "staged\n").unwrap();
    git(&parent, &["add", "owned"]);
    fs::write(parent.join("owned"), "unstaged\n").unwrap();
    fs::remove_file(parent.join("deleted")).unwrap();
    fs::write(parent.join(".gitignore"), "ignored\n").unwrap();
    fs::write(parent.join("ignored"), "ignored data").unwrap();
    fs::write(parent.join("binary"), [0, 255, 1]).unwrap();
    fs::write(parent.join("executable"), "#!/bin/sh\n").unwrap();
    fs::set_permissions(parent.join("executable"), fs::Permissions::from_mode(0o751)).unwrap();
    fs::create_dir(parent.join("readonly")).unwrap();
    fs::write(parent.join("readonly/file"), "readable baseline").unwrap();
    fs::set_permissions(parent.join("readonly"), fs::Permissions::from_mode(0o555)).unwrap();
    fs::create_dir(parent.join("empty")).unwrap();
    symlink("missing-target", parent.join("link")).unwrap();
    let index = fs::read(parent.join(".git/index")).unwrap();
    let identity = prepare(&parent, &child).await.unwrap();
    for name in ["owned", "ignored", "binary", "executable", ".gitignore"] {
        assert_eq!(
            fs::read(parent.join(name)).unwrap(),
            fs::read(child.join(name)).unwrap()
        );
    }
    assert_eq!(
        fs::read_to_string(child.join("readonly/file")).unwrap(),
        "readable baseline"
    );
    assert_eq!(
        fs::metadata(child.join("readonly"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o555
    );
    assert!(!child.join("deleted").exists());
    assert!(child.join("empty").is_dir());
    assert_eq!(
        fs::read_link(child.join("link")).unwrap(),
        PathBuf::from("missing-target")
    );
    assert_eq!(
        fs::metadata(child.join("executable"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o751
    );
    assert_eq!(fs::read(parent.join(".git/index")).unwrap(), index);
    assert!(child.join(".git").is_file());
    assert_ne!(identity.git_dir, identity.common_dir);
    assert_eq!(inspect(&identity).await.unwrap(), identity.child_baseline);
}
#[tokio::test]
async fn integrates_owned_delta_and_preserves_unrelated_dirty_parent_and_index() {
    let (_temp, parent, child) = repository();
    fs::write(parent.join("owned"), "developer baseline\n").unwrap();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::write(child.join("owned"), "child result\n").unwrap();
    fs::write(parent.join("unrelated"), "later developer change\n").unwrap();
    let index = fs::read(parent.join(".git/index")).unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["owned"]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert_eq!(
        fs::read_to_string(parent.join("owned")).unwrap(),
        "child result\n"
    );
    assert_eq!(
        fs::read_to_string(parent.join("unrelated")).unwrap(),
        "later developer change\n"
    );
    assert_eq!(fs::read(parent.join(".git/index")).unwrap(), index);
    git(&parent, &["cat-file", "-e", &plan.result_commit]);
}
#[tokio::test]
async fn conflicts_stale_child_and_unowned_changes_refuse_before_effects() {
    let (_temp, parent, child) = repository();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::write(child.join("owned"), "child\n").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["owned"]), &snapshot.digest)
        .await
        .unwrap();
    fs::write(parent.join("owned"), "developer\n").unwrap();
    assert!(
        integrate(&parent, &identity, &plan)
            .await
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );
    assert_eq!(
        fs::read_to_string(parent.join("owned")).unwrap(),
        "developer\n"
    );
    fs::write(child.join("owned"), "stale\n").unwrap();
    assert!(
        integrate(&parent, &identity, &plan)
            .await
            .unwrap_err()
            .to_string()
            .contains("after validation")
    );
    fs::write(child.join("unrelated"), "unowned\n").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    assert!(
        build_delta(&identity, &request(&["owned"]), &snapshot.digest)
            .await
            .unwrap_err()
            .to_string()
            .contains("unowned")
    );
}
#[tokio::test]
async fn hooks_filters_and_signers_cannot_execute() {
    let (temp, parent, child) = repository();
    let marker = temp.path().join("executed");
    let payload = format!("touch {}", marker.display());
    fs::write(
        parent.join(".gitattributes"),
        "* filter=evil diff=evil text eol=crlf ident working-tree-encoding=UTF-16LE\n",
    )
    .unwrap();
    fs::create_dir_all(parent.join(".git/info")).unwrap();
    fs::write(
        parent.join(".git/info/attributes"),
        "* filter=evil text eol=crlf\n",
    )
    .unwrap();
    for key in [
        "filter.evil.clean",
        "filter.evil.smudge",
        "filter.evil.process",
        "diff.evil.command",
        "gpg.program",
        "core.fsmonitor",
    ] {
        git(&parent, &["config", key, &payload]);
    }
    git(&parent, &["config", "filter.evil.required", "true"]);
    git(&parent, &["config", "commit.gpgSign", "true"]);
    let hook = parent.join(".git/hooks/post-checkout");
    fs::write(&hook, format!("#!/bin/sh\n{payload}\n")).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::write(child.join("owned"), "result\n").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["owned"]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert!(!marker.exists());
}
#[tokio::test]
async fn metadata_retargeting_and_unsafe_baselines_are_rejected() {
    let (_temp, parent, child) = repository();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::write(
        child.join(".git"),
        format!("gitdir: {}\n", parent.join(".git").display()),
    )
    .unwrap();
    assert!(inspect(&identity).await.is_err());
    fs::hard_link(parent.join("owned"), parent.join("hardlink")).unwrap();
    assert!(
        prepare(&parent, &child.with_file_name("other"))
            .await
            .is_err()
    );
}
#[tokio::test]
async fn additions_deletions_binary_and_executable_delta_are_exact() {
    let (_temp, parent, child) = repository();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::create_dir(child.join("newdir")).unwrap();
    fs::write(child.join("newdir/binary"), [0, 255, 1, 2]).unwrap();
    fs::write(child.join("owned"), "#!/bin/sh\n").unwrap();
    fs::set_permissions(child.join("owned"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::remove_file(child.join("deleted")).unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["."]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert_eq!(
        fs::read(parent.join("newdir/binary")).unwrap(),
        [0, 255, 1, 2]
    );
    assert!(!parent.join("deleted").exists());
    assert_eq!(
        fs::metadata(parent.join("owned"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[tokio::test]
async fn same_path_admin_replacement_and_symlink_pointer_are_rejected() {
    let (_temp, parent, child) = repository();
    let identity = prepare(&parent, &child).await.unwrap();
    let pointer = fs::read(child.join(".git")).unwrap();
    fs::write(child.with_file_name("pointer"), &pointer).unwrap();
    fs::remove_file(child.join(".git")).unwrap();
    symlink(child.with_file_name("pointer"), child.join(".git")).unwrap();
    assert!(
        inspect(&identity)
            .await
            .unwrap_err()
            .to_string()
            .contains("pointer")
    );
    fs::remove_file(child.join(".git")).unwrap();
    fs::write(child.join(".git"), pointer).unwrap();
    fs::rename(&identity.git_dir, identity.git_dir.with_extension("saved")).unwrap();
    fs::create_dir(&identity.git_dir).unwrap();
    assert!(
        inspect(&identity)
            .await
            .unwrap_err()
            .to_string()
            .contains("replaced")
    );
}

#[tokio::test]
async fn mode_only_delta_and_quoted_filenames_preserve_exact_bytes() {
    let (_temp, parent, child) = repository();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::set_permissions(child.join("owned"), fs::Permissions::from_mode(0o600)).unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["owned"]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert_eq!(
        fs::metadata(parent.join("owned"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let another = child.with_file_name("another");
    let identity = prepare(&parent, &another).await.unwrap();
    let name = "-quoted\t\"file\\name";
    fs::write(another.join(name), b"$Id$\r\nbytes\0\xff").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&[name]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert_eq!(fs::read(parent.join(name)).unwrap(), b"$Id$\r\nbytes\0\xff");
}

#[tokio::test]
async fn forged_result_and_parent_replacement_refuse_before_effects() {
    let (temp, parent, child) = repository();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::write(child.join("owned"), "result\n").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["owned"]), &snapshot.digest)
        .await
        .unwrap();
    let mut altered = serde_json::to_value(&plan).unwrap();
    altered["result_commit"] = identity.baseline_commit.clone().into();
    let altered = serde_json::from_value(altered).unwrap();
    assert!(
        integrate(&parent, &identity, &altered)
            .await
            .unwrap_err()
            .to_string()
            .contains("does not match")
    );
    assert_eq!(fs::read_to_string(parent.join("owned")).unwrap(), "base\n");
    let saved = temp.path().join("saved-parent");
    fs::rename(&parent, &saved).unwrap();
    fs::create_dir(&parent).unwrap();
    fs::write(parent.join("owned"), "base\n").unwrap();
    assert!(integrate(&parent, &identity, &plan).await.is_err());
    assert_eq!(fs::read_to_string(parent.join("owned")).unwrap(), "base\n");
}

#[tokio::test]
async fn owned_empty_directory_delta_integrates_and_nested_git_is_refused() {
    let (_temp, parent, child) = repository();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::create_dir(child.join("empty")).unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["empty"]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert!(parent.join("empty").is_dir());
    fs::create_dir_all(parent.join("nested/.git")).unwrap();
    let destination = child.with_file_name("unsafe");
    assert!(
        prepare(&parent, &destination)
            .await
            .unwrap_err()
            .to_string()
            .contains("unsafe baseline")
    );
    assert!(!destination.exists());
}

#[tokio::test]
async fn deleting_last_file_preserves_the_validated_empty_directory() {
    let (_temp, parent, child) = repository();
    fs::create_dir(parent.join("nested")).unwrap();
    fs::set_permissions(parent.join("nested"), fs::Permissions::from_mode(0o750)).unwrap();
    fs::write(parent.join("nested/last"), "remove me").unwrap();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::remove_file(child.join("nested/last")).unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["nested"]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert!(parent.join("nested").is_dir());
    assert_eq!(
        fs::metadata(parent.join("nested"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o750
    );
    assert!(!parent.join("nested/last").exists());
}

#[tokio::test]
async fn directory_modes_removal_and_symlink_delta_respect_ownership_and_parent_additions() {
    let (_temp, parent, child) = repository();
    fs::create_dir(parent.join("directory")).unwrap();
    fs::write(parent.join("directory/file"), "old").unwrap();
    symlink("outside-old", parent.join("link")).unwrap();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::set_permissions(child.join("directory"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(child.join("directory/file"), "new").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    assert!(
        build_delta(&identity, &request(&["directory/file"]), &snapshot.digest)
            .await
            .unwrap_err()
            .to_string()
            .contains("unowned")
    );
    fs::remove_file(child.join("link")).unwrap();
    symlink("outside-new", child.join("link")).unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(
        &identity,
        &request(&["directory", "link"]),
        &snapshot.digest,
    )
    .await
    .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert_eq!(
        fs::read_link(parent.join("link")).unwrap(),
        PathBuf::from("outside-new")
    );
    assert_eq!(
        fs::metadata(parent.join("directory"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    let another = child.with_file_name("another");
    let identity = prepare(&parent, &another).await.unwrap();
    fs::remove_file(another.join("directory/file")).unwrap();
    fs::remove_dir(another.join("directory")).unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["directory"]), &snapshot.digest)
        .await
        .unwrap();
    fs::write(parent.join("directory/later"), "developer addition").unwrap();
    assert!(
        integrate(&parent, &identity, &plan)
            .await
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );
    assert_eq!(
        fs::read_to_string(parent.join("directory/file")).unwrap(),
        "new"
    );
    fs::remove_file(parent.join("directory/later")).unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert!(!parent.join("directory").exists());
}

#[tokio::test]
async fn deep_owned_file_creates_only_necessary_ancestor_directories() {
    let (_temp, parent, child) = repository();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::create_dir_all(child.join("a/b")).unwrap();
    fs::write(child.join("a/b/file"), "owned").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["a/b/file"]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert_eq!(
        fs::read_to_string(parent.join("a/b/file")).unwrap(),
        "owned"
    );
    fs::create_dir(child.join("a/unowned-empty")).unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    assert!(
        build_delta(&identity, &request(&["a/b/file"]), &snapshot.digest)
            .await
            .unwrap_err()
            .to_string()
            .contains("unowned")
    );
}

#[tokio::test]
async fn validated_permission_increase_precedes_writes_inside_readonly_directory() {
    let (_temp, parent, child) = repository();
    fs::create_dir(parent.join("readonly")).unwrap();
    fs::write(parent.join("readonly/file"), "old").unwrap();
    fs::set_permissions(parent.join("readonly"), fs::Permissions::from_mode(0o555)).unwrap();
    let identity = prepare(&parent, &child).await.unwrap();
    fs::set_permissions(child.join("readonly"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(child.join("readonly/file"), "new").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let plan = build_delta(&identity, &request(&["readonly"]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &plan).await.unwrap();
    assert_eq!(
        fs::read_to_string(parent.join("readonly/file")).unwrap(),
        "new"
    );
    assert_eq!(
        fs::metadata(parent.join("readonly"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
}

#[tokio::test]
async fn repository_diff_prefixes_cannot_redirect_integration_into_unowned_files() {
    for no_prefix in ["false", "true"] {
        let (_temp, parent, child) = repository();
        fs::create_dir(parent.join("unowned")).unwrap();
        fs::write(parent.join("unowned/owned"), "base\n").unwrap();
        git(&parent, &["config", "diff.srcPrefix", "a/unowned/"]);
        git(&parent, &["config", "diff.dstPrefix", "b/unowned/"]);
        git(&parent, &["config", "diff.noprefix", no_prefix]);
        git(&parent, &["config", "diff.relative", no_prefix]);
        git(&parent, &["config", "diff.mnemonicPrefix", no_prefix]);
        git(
            &parent,
            &[
                "config",
                "color.ui",
                if no_prefix == "true" {
                    "always"
                } else {
                    "never"
                },
            ],
        );
        let identity = prepare(&parent, &child).await.unwrap();
        fs::write(child.join("owned"), "child\n").unwrap();
        let snapshot = inspect(&identity).await.unwrap();
        let plan = build_delta(&identity, &request(&["owned"]), &snapshot.digest)
            .await
            .unwrap();
        let outcome = integrate(&parent, &identity, &plan).await;
        assert_eq!(
            fs::read_to_string(parent.join("unowned/owned")).unwrap(),
            "base\n",
            "Git configuration redirected a child delta into unowned content"
        );
        outcome.unwrap();
        assert_eq!(fs::read_to_string(parent.join("owned")).unwrap(), "child\n");
    }
}

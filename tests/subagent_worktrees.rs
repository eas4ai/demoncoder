use demoncoder::subagents::{
    state::AssignmentRequest,
    worktree::{build_delta, inspect, integrate, prepare, prepare_with_scope},
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
async fn generated_outputs_stay_out_of_child_files_git_objects_and_integration() {
    use demoncoder::workflow::workspace::CaptureScope;

    let (_temp, parent, child) = repository();
    fs::create_dir(parent.join("build")).unwrap();
    fs::write(parent.join("build/note"), "PARENT_GENERATED_CANARY").unwrap();
    fs::File::create(parent.join("build/artifact"))
        .unwrap()
        .set_len(9 * 1024 * 1024)
        .unwrap();
    let scope = CaptureScope::new(vec!["build".into()]).unwrap();
    let identity = prepare_with_scope(&parent, &child, &scope).await.unwrap();
    assert!(!child.join("build").exists());
    assert_eq!(identity.child_baseline.scope, scope);
    fs::create_dir(child.join("build")).unwrap();
    fs::write(child.join("build/note"), "CHILD_GENERATED_CANARY").unwrap();
    fs::File::create(child.join("build/artifact"))
        .unwrap()
        .set_len(9 * 1024 * 1024)
        .unwrap();
    assert_eq!(
        inspect(&identity).await.unwrap().digest,
        identity.child_baseline.digest
    );
    fs::write(child.join("owned"), "corrected source").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let delta = build_delta(&identity, &request(&["owned"]), &snapshot.digest)
        .await
        .unwrap();
    assert!(
        delta
            .changed_paths
            .iter()
            .all(|path| !path.contains("build"))
    );
    let restored = serde_json::from_str(&serde_json::to_string(&identity).unwrap()).unwrap();
    integrate(&parent, &restored, &delta).await.unwrap();
    assert_eq!(
        fs::read_to_string(parent.join("owned")).unwrap(),
        "corrected source"
    );
    assert_eq!(
        fs::read_to_string(parent.join("build/note")).unwrap(),
        "PARENT_GENERATED_CANARY"
    );
    for root in [&parent, &child] {
        // Without -w, this computes the ID but cannot create the forbidden object.
        let oid = String::from_utf8(git(root, &["hash-object", "build/note"])).unwrap();
        assert!(
            !Command::new("git")
                .current_dir(&parent)
                .args(["cat-file", "-e", oid.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }
    let mut mismatched = identity;
    mismatched.parent_baseline.scope = CaptureScope::default();
    assert!(
        inspect(&mismatched)
            .await
            .unwrap_err()
            .to_string()
            .contains("scope")
    );
}

#[tokio::test]
async fn private_source_is_not_copied_or_committed_by_delegation() {
    let (_temp, parent, child) = repository();
    fs::create_dir(parent.join(".demoncoder")).unwrap();
    fs::write(parent.join(".demoncoder/token"), "PRIVATE_GIT_CANARY").unwrap();
    fs::write(parent.join(".env"), "PRIVATE_ENV_CANARY").unwrap();
    fs::create_dir_all(parent.join(".config/gh")).unwrap();
    fs::write(parent.join(".config/gh/hosts.yml"), "PRIVATE_GITHUB_CANARY").unwrap();
    fs::create_dir(parent.join(".cargo")).unwrap();
    fs::write(
        parent.join(".cargo/credentials.toml"),
        "PRIVATE_CARGO_CANARY",
    )
    .unwrap();
    fs::write(parent.join(".gitignore"), ".demoncoder/\n.env\n").unwrap();
    let identity = prepare(&parent, &child).await.unwrap();
    assert!(!child.join(".demoncoder").exists());
    assert!(!child.join(".env").exists());
    assert!(!child.join(".config/gh").exists());
    assert!(!child.join(".cargo/credentials.toml").exists());
    let tree = String::from_utf8(git(
        &parent,
        &["ls-tree", "-r", "--name-only", &identity.baseline_commit],
    ))
    .unwrap();
    assert!(!tree.contains(".demoncoder"));
    assert!(!tree.lines().any(|path| path == ".env"));
    let mut private_objects = Vec::new();
    for value in [
        "PRIVATE_GIT_CANARY",
        "PRIVATE_ENV_CANARY",
        "PRIVATE_GITHUB_CANARY",
        "PRIVATE_CARGO_CANARY",
    ] {
        // Compute the canary's object ID without writing it into this repository.
        use std::io::Write;
        let mut hash = Command::new("git")
            .current_dir(&parent)
            .args(["hash-object", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        hash.stdin
            .take()
            .unwrap()
            .write_all(value.as_bytes())
            .unwrap();
        let output = hash.wait_with_output().unwrap();
        assert!(output.status.success());
        let canary = String::from_utf8(output.stdout).unwrap();
        private_objects.push(canary.trim().to_owned());
        assert!(
            !Command::new("git")
                .current_dir(&parent)
                .args(["cat-file", "-e", canary.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(child.join("owned"), "corrected public source").unwrap();
    let snapshot = inspect(&identity).await.unwrap();
    let delta = build_delta(&identity, &request(&["owned"]), &snapshot.digest)
        .await
        .unwrap();
    integrate(&parent, &identity, &delta).await.unwrap();
    for object in private_objects {
        assert!(
            !Command::new("git")
                .current_dir(&parent)
                .args(["cat-file", "-e", &object])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success(),
            "integration exported private source into Git"
        );
    }
    assert_eq!(
        fs::read_to_string(parent.join(".demoncoder/token")).unwrap(),
        "PRIVATE_GIT_CANARY"
    );
    assert_eq!(
        fs::read_to_string(parent.join("owned")).unwrap(),
        "corrected public source"
    );
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

#[test]
fn declared_private_roots_block_capture_and_git_export() {
    const PROBE: &str = "DEMONCODER_PRIVATE_ROOT_PROBE";
    if let Some(parent) = std::env::var_os(PROBE) {
        let parent = PathBuf::from(parent);
        let child = parent.parent().unwrap().join("refused-child");
        assert!(
            demoncoder::workflow::workspace::capture(&parent).is_err(),
            "private root entered a retained workspace snapshot"
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(
            runtime.block_on(prepare(&parent, &child)).is_err(),
            "private root entered delegated source"
        );
        let oid = git(
            &parent,
            &["hash-object", "--no-filters", "runtime-secrets/auth.json"],
        );
        assert!(
            !Command::new("git")
                .current_dir(&parent)
                .args(["cat-file", "-e", std::str::from_utf8(&oid).unwrap().trim()])
                .output()
                .unwrap()
                .status
                .success(),
            "private blob entered Git objects"
        );
        assert!(!child.join("runtime-secrets/auth.json").exists());
        return;
    }
    for case in [
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
        "AWS_SHARED_CREDENTIALS_FILE",
        "alias",
        "ancestor",
        "home",
    ] {
        let (temp, mut parent, _) = repository();
        if case == "home" {
            let destination = temp.path().join(".codex/project");
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::rename(&parent, &destination).unwrap();
            parent = destination;
        }
        let secret_dir = parent.join("runtime-secrets");
        fs::create_dir(&secret_dir).unwrap();
        fs::write(
            secret_dir.join("auth.json"),
            "DYNAMIC_PRIVATE_EXPORT_CANARY",
        )
        .unwrap();
        let (variable, value) = match case {
            "AWS_SHARED_CREDENTIALS_FILE" => (case, secret_dir.join("auth.json")),
            "alias" => {
                let alias = temp.path().join("outside-alias");
                symlink(&secret_dir, &alias).unwrap();
                ("CODEX_HOME", alias)
            }
            "ancestor" => ("CODEX_HOME", temp.path().to_owned()),
            "home" => ("HOME", temp.path().to_owned()),
            _ => (case, secret_dir),
        };
        // Set process-local environment in a fresh test process; never mutate the
        // shared environment of Rust's parallel test threads.
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "declared_private_roots_block_capture_and_git_export",
                "--nocapture",
            ])
            .env(PROBE, &parent)
            .env(variable, value)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{case}: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

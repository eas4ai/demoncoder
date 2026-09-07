use demoncoder::{
    events::EventSink,
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolResult},
};
use serde_json::{Value, json};
use std::os::unix::fs::symlink;
async fn tool(e: &ToolExecutor, name: &str, arguments: Value) -> ToolResult {
    let (tx, mut rx) = tokio::sync::mpsc::channel(256);
    let sink = EventSink::new("strict".into(), tx, None).unwrap();
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let result = e
        .execute(
            ToolCall {
                id: "strict".into(),
                name: name.into(),
                arguments,
            },
            &sink,
        )
        .await
        .unwrap();
    drop(sink);
    drain.await.unwrap();
    result
}
#[tokio::test]
async fn native_files_are_strictly_rooted() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("work");
    std::fs::create_dir(&root).unwrap();
    let outside = parent.path().join("secret");
    std::fs::write(&outside, "CANARY").unwrap();
    symlink(&outside, root.join("symlink")).unwrap();
    std::fs::hard_link(&outside, root.join("hardlink")).unwrap();
    std::fs::write(root.join(".git"), "gitdir: outside").unwrap();
    let e = ToolExecutor::with_policy(&root, &AccessPolicy::worktree_only(vec![])).unwrap();
    for path in [
        outside.to_str().unwrap(),
        "../secret",
        "symlink",
        "hardlink",
        ".git",
    ] {
        for (name, args) in [
            ("read", json!({"path":path})),
            ("write", json!({"path":path,"content":"BAD"})),
            (
                "edit",
                json!({"path":path,"old_text":"CANARY","new_text":"BAD"}),
            ),
        ] {
            let r = tool(&e, name, args).await;
            assert!(!r.success, "{name} {path}: {}", r.output);
        }
    }
    assert!(
        tool(&e, "write", json!({"path":"ok","content":"before"}))
            .await
            .success
    );
    assert!(
        tool(
            &e,
            "edit",
            json!({"path":"ok","old_text":"before","new_text":"after"})
        )
        .await
        .success
    );
    assert_eq!(tool(&e, "read", json!({"path":"ok"})).await.output, "after");
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "CANARY");
}
#[test]
fn conflicting_host_policy_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let mut policy = AccessPolicy::worktree_only(vec![]);
    policy.unrestricted = true;
    assert!(ToolExecutor::with_policy(root.path(), &policy).is_err());
}
#[tokio::test]
async fn shell_cannot_reach_home_or_git_administration() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("work");
    let home = parent.path().join("home");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&home).unwrap();
    std::fs::write(home.join("secret"), "HOME-CANARY").unwrap();
    std::fs::write(root.join(".git"), "GIT-CANARY").unwrap();
    let e = ToolExecutor::with_policy(&root, &AccessPolicy::worktree_only(vec![])).unwrap();
    let r = tool(&e, "bash", json!({"command":"printf working > ok; cat ok"})).await;
    assert!(r.success, "{}", r.output);
    assert_eq!(r.output, "working");
    for command in [
        format!("cat '{}/secret'", home.display()),
        format!("mv '{}' /tmp/stolen", home.display()),
        format!("printf BAD > '{}/secret'", home.display()),
        "cat .git".into(),
        "printf BAD > .git".into(),
        "rm .git".into(),
        "mv .git moved".into(),
    ] {
        let r = tool(&e, "bash", json!({"command":command})).await;
        assert!(!r.success, "{command}: {}", r.output);
        assert!(!r.output.contains("HOME-CANARY"));
    }
    let _ = tool(
        &e,
        "bash",
        json!({"command":format!("rm -rf '{}'",home.display())}),
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(home.join("secret")).unwrap(),
        "HOME-CANARY"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".git")).unwrap(),
        "GIT-CANARY"
    );
}

#[tokio::test]
async fn shell_aliases_and_inherited_descriptors_cannot_escape() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("work");
    std::fs::create_dir(&root).unwrap();
    let outside = parent.path().join("outside");
    std::fs::write(&outside, "OUTSIDE-CANARY").unwrap();
    symlink(&outside, root.join("alias")).unwrap();
    let e = ToolExecutor::with_policy(&root, &AccessPolicy::worktree_only(vec![])).unwrap();
    for command in [
        "cat ../outside",
        "cat alias",
        "printf BAD > alias",
        "cat /proc/self/fd/0/../outside",
        "printf BAD > /proc/self/fd/0/../outside",
    ] {
        let r = tool(&e, "bash", json!({"command":command})).await;
        assert!(!r.success, "{command}: {}", r.output);
        assert!(!r.output.contains("OUTSIDE-CANARY"));
    }
    std::fs::hard_link(&outside, root.join("hardlink")).unwrap();
    let r = tool(
        &e,
        "bash",
        json!({"command":"cat hardlink; printf BAD > hardlink"}),
    )
    .await;
    assert!(!r.success, "{}", r.output);
    assert!(!r.output.contains("OUTSIDE-CANARY"));
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "OUTSIDE-CANARY");
}
#[tokio::test]
async fn credentials_and_git_directories_are_masked() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    std::fs::write(root.path().join(".git/config"), "ADMIN-CANARY").unwrap();
    let credential = root.path().join("custom-key");
    std::fs::write(&credential, "CREDENTIAL-CANARY").unwrap();
    let e = ToolExecutor::with_policy(
        root.path(),
        &AccessPolicy::worktree_only(vec![credential.clone()]),
    )
    .unwrap();
    for path in [".git/config", "custom-key"] {
        assert!(!tool(&e, "read", json!({"path":path})).await.success);
        for command in [format!("cat {path}"), format!("printf BAD > {path}")] {
            let r = tool(&e, "bash", json!({"command":command})).await;
            assert!(!r.success, "{}", r.output);
        }
    }
    for command in ["rm -rf .git", "mv .git moved", "mv custom-key moved"] {
        let r = tool(&e, "bash", json!({"command":command})).await;
        assert!(!r.success, "{command}: {}", r.output);
    }
    assert_eq!(
        std::fs::read_to_string(credential).unwrap(),
        "CREDENTIAL-CANARY"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join(".git/config")).unwrap(),
        "ADMIN-CANARY"
    );
}
#[tokio::test]
async fn failed_sandbox_preparation_never_runs_host_command() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("executed");
    std::fs::write(root.path().join("entry"), "x").unwrap();
    std::fs::hard_link(root.path().join("entry"), root.path().join("alias")).unwrap();
    let e = ToolExecutor::with_policy(root.path(), &AccessPolicy::worktree_only(vec![])).unwrap();
    let r = tool(&e, "bash", json!({"command":"touch executed"})).await;
    assert!(!r.success);
    assert!(!marker.exists());
}

#[tokio::test]
async fn shell_has_no_writable_scaffolding_or_host_socket_access() {
    let root = tempfile::tempdir().unwrap();
    let e = ToolExecutor::with_policy(root.path(), &AccessPolicy::worktree_only(vec![])).unwrap();
    for command in [
        "touch /tmp/escape",
        "touch /dev/shm/escape",
        "mkdir /outside",
        "python3 -c 'import socket; socket.socket(socket.AF_UNIX)'",
    ] {
        let result = tool(&e, "bash", json!({"command":command})).await;
        assert!(!result.success, "{command}: {}", result.output);
    }
    let result = tool(
        &e,
        "bash",
        json!({"command":"python3 -c 'import socket; socket.socket(socket.AF_INET); print(42)'"}),
    )
    .await;
    assert!(result.success, "{}", result.output);
    assert_eq!(result.output.trim(), "42");
}
#[tokio::test]
async fn cancelling_strict_shell_stops_its_descendants() {
    let root = tempfile::tempdir().unwrap();
    let e = std::sync::Arc::new(
        ToolExecutor::with_policy(root.path(), &AccessPolicy::worktree_only(vec![])).unwrap(),
    );
    let handle = tokio::spawn(async move {
        tool(
            &e,
            "bash",
            json!({"command":"touch started; (sleep 1; touch escaped) & wait"}),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !root.path().join("started").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    handle.abort();
    assert!(handle.await.unwrap_err().is_cancelled());
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    assert!(!root.path().join("escaped").exists());
}
#[test]
fn credential_store_cannot_overlap_system_runtime() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        ToolExecutor::with_policy(
            root.path(),
            &AccessPolicy::worktree_only(vec!["/usr/lib/fixture-secret".into()])
        )
        .is_err()
    );
}

#[tokio::test]
async fn configured_oracle_cannot_expand_child_access() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("work");
    std::fs::create_dir(&root).unwrap();
    let outside = parent.path().join("canary");
    std::fs::write(&outside, "ORACLE-CANARY").unwrap();
    let mut policy = AccessPolicy::worktree_only(vec![]);
    policy.oracle = Some(Box::new(
        serde_json::from_value(json!({
            "adapter":"claude", "binary":"/nonexistent/oracle-must-not-run"
        }))
        .unwrap(),
    ));
    let e = ToolExecutor::with_policy(&root, &policy).unwrap();
    let result = tool(&e, "read", json!({"path":outside})).await;
    assert!(!result.success);
    assert!(result.output.contains("relative"), "{}", result.output);
    let result = tool(&e, "bash", json!({"command":"printf permitted"})).await;
    assert!(result.success, "{}", result.output);
    assert_eq!(result.output, "permitted");
}

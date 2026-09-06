use demoncoder::{
    events::EventSink,
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolResult},
};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::symlink,
    time::{Duration, Instant},
};
use tokio::sync::mpsc;

async fn tool(executor: &ToolExecutor, name: &str, arguments: serde_json::Value) -> ToolResult {
    let (tx, mut rx) = mpsc::channel(256);
    let events = EventSink::new("usability".into(), tx, None).unwrap();
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let result = executor
        .execute(
            ToolCall {
                id: "usability".into(),
                name: name.into(),
                arguments,
            },
            &events,
        )
        .await
        .unwrap();
    drop(events);
    drain.await.unwrap();
    result
}

async fn bash(executor: &ToolExecutor, command: String) -> ToolResult {
    tool(executor, "bash", json!({"command": command})).await
}

#[tokio::test]
async fn normal_build_links_and_reference_symlinks_do_not_disable_bash() {
    let parent = tempfile::tempdir().unwrap();
    let workspace = parent.path().join("project");
    std::fs::create_dir_all(workspace.join("target/debug/deps")).unwrap();
    std::fs::create_dir_all(workspace.join("reference")).unwrap();
    std::fs::write(workspace.join("target/debug/deps/build-output"), "old").unwrap();
    std::fs::hard_link(
        workspace.join("target/debug/deps/build-output"),
        workspace.join("target/debug/build-output"),
    )
    .unwrap();
    let docs = parent.path().join("documentation.md");
    std::fs::write(&docs, "EXTERNAL-DOCUMENTATION").unwrap();
    symlink(&docs, workspace.join("reference/guide.md")).unwrap();
    let executor = ToolExecutor::new(&workspace).unwrap();
    let result = bash(&executor, r#"set -e; pwd; cat reference/guide.md; printf rebuilt > target/debug/build-output; test "$(cat target/debug/deps/build-output)" = rebuilt"#.into()).await;
    assert!(result.success, "{}", result.output);
    assert!(result.output.contains("EXTERNAL-DOCUMENTATION"));
    assert_eq!(
        std::fs::read_to_string(docs).unwrap(),
        "EXTERNAL-DOCUMENTATION"
    );
}

#[tokio::test]
async fn outside_hard_link_and_symlink_writes_are_blocked_without_disabling_other_commands() {
    let parent = tempfile::tempdir().unwrap();
    let workspace = parent.path().join("project");
    std::fs::create_dir(&workspace).unwrap();
    let outside = parent.path().join("outside.txt");
    std::fs::write(&outside, "OUTSIDE-CANARY").unwrap();
    std::fs::hard_link(&outside, workspace.join("hard-link")).unwrap();
    symlink(&outside, workspace.join("symbolic-link")).unwrap();
    let executor = ToolExecutor::new(&workspace).unwrap();
    let result = bash(&executor, "pwd; printf READY".into()).await;
    assert!(result.success, "{}", result.output);
    assert!(result.output.contains("READY"));
    for path in [
        "hard-link".to_owned(),
        "symbolic-link".to_owned(),
        outside.to_string_lossy().into_owned(),
    ] {
        let result = bash(&executor, format!("printf changed > '{path}'")).await;
        assert!(!result.success, "outside write succeeded through {path}");
        assert!(
            result.output.contains(&path),
            "failure must identify the affected path: {}",
            result.output
        );
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "OUTSIDE-CANARY");
    }
}

#[tokio::test]
async fn native_read_accepts_outside_documentation_and_git_metadata_but_mutation_stays_rooted() {
    let parent = tempfile::tempdir().unwrap();
    let workspace = parent.path().join("project");
    std::fs::create_dir_all(workspace.join(".git")).unwrap();
    let docs = parent.path().join("BEST_PRACTICES.md");
    std::fs::write(&docs, "MACHINE-STANDARD").unwrap();
    std::fs::write(workspace.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let executor = ToolExecutor::new(&workspace).unwrap();
    for path in [
        docs.to_string_lossy().into_owned(),
        "../BEST_PRACTICES.md".into(),
        ".git/HEAD".into(),
    ] {
        let result = tool(&executor, "read", json!({"path":path})).await;
        assert!(result.success, "{path}: {}", result.output);
        assert!(!result.output.is_empty());
    }
    let result = tool(&executor, "write", json!({"path":docs,"content":"changed"})).await;
    assert!(!result.success);
    assert_eq!(std::fs::read_to_string(docs).unwrap(), "MACHINE-STANDARD");
}

#[tokio::test]
async fn git_can_inspect_and_commit_the_selected_repository() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = ToolExecutor::new(workspace.path()).unwrap();
    let result = bash(&executor, "set -e; git init -q -b main; printf source > source.txt; git add source.txt; git -c user.name=Fixture -c user.email=fixture@example.invalid -c commit.gpgsign=false commit -qm initial; git branch --show-current; git status --porcelain; git rev-parse --verify HEAD".into()).await;
    assert!(result.success, "{}", result.output);
    assert!(result.output.contains("main"));
    let update = bash(&executor, "set -e; printf updated > source.txt; git add source.txt; git -c user.name=Fixture -c user.email=fixture@example.invalid -c commit.gpgsign=false commit -qm update; git log -1 --format=%s".into()).await;
    assert!(update.success, "{}", update.output);
    assert_eq!(update.output.trim(), "update");
    let metadata = tool(&executor, "read", json!({"path":".git/HEAD"})).await;
    assert!(metadata.success, "{}", metadata.output);
    assert_eq!(metadata.output.trim(), "ref: refs/heads/main");
}

#[tokio::test]
async fn bash_can_reach_a_network_documentation_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = [0; 2048];
                    let _ = stream.read(&mut request);
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\nConnection: close\r\n\r\nNETWORK-DOCS").unwrap();
                    return true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("{error}"),
            }
        }
        false
    });
    let workspace = tempfile::tempdir().unwrap();
    let executor = ToolExecutor::new(workspace.path()).unwrap();
    let result = bash(
        &executor,
        format!(
            "curl --noproxy '*' --fail --silent --show-error --max-time 2 http://{address}/docs"
        ),
    )
    .await;
    let connected = server.join().unwrap();
    assert!(result.success && connected, "{}", result.output);
    assert_eq!(result.output, "NETWORK-DOCS");
}

#[tokio::test]
async fn installed_developer_tools_are_available_without_exposing_provider_environment() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = ToolExecutor::new(workspace.path()).unwrap();
    let result = bash(&executor, r#"set -e; git --version; cargo --version; rustc --version; python3 -B -c 'import os; assert not any(k in os.environ for k in ["OPENAI_API_KEY","ANTHROPIC_API_KEY","CLAUDE_CODE_OAUTH_TOKEN","CODEX_HOME"]); print("CLEAN-ENVIRONMENT")'"#.into()).await;
    assert!(result.success, "{}", result.output);
    assert!(result.output.contains("cargo "));
    assert!(result.output.contains("rustc "));
    assert!(result.output.contains("CLEAN-ENVIRONMENT"));
}

#[tokio::test]
async fn private_workspace_settings_and_their_aliases_are_unreadable() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir(workspace.path().join(".demoncoder")).unwrap();
    let secret = workspace.path().join(".demoncoder/settings.toml");
    std::fs::write(&secret, "SYNTHETIC-CREDENTIAL").unwrap();
    std::fs::hard_link(&secret, workspace.path().join("credential-alias")).unwrap();
    symlink(&secret, workspace.path().join("credential-link")).unwrap();
    let executor = ToolExecutor::new(workspace.path()).unwrap();
    for path in [
        ".demoncoder/settings.toml",
        "credential-alias",
        "credential-link",
    ] {
        let result = tool(&executor, "read", json!({"path":path})).await;
        assert!(!result.success, "native read exposed {path}");
        assert!(!result.output.contains("SYNTHETIC-CREDENTIAL"));
        let result = bash(&executor, format!("cat '{path}'")).await;
        assert!(!result.success, "Bash read exposed {path}");
        assert!(!result.output.contains("SYNTHETIC-CREDENTIAL"));
    }
}

#[tokio::test]
async fn selected_private_config_cannot_be_changed_by_native_tools() {
    let workspace = tempfile::tempdir().unwrap();
    let config = workspace.path().join("private-connection.toml");
    std::fs::write(&config, "SYNTHETIC-SAVED-KEY").unwrap();
    let policy = AccessPolicy {
        credential_paths: vec![config.clone()],
        ..AccessPolicy::default()
    };
    let executor = ToolExecutor::with_policy(workspace.path(), &policy).unwrap();
    for (name, arguments) in [
        (
            "write",
            json!({"path":"private-connection.toml","content":"changed"}),
        ),
        (
            "edit",
            json!({"path":"private-connection.toml","old_text":"SYNTHETIC-SAVED-KEY","new_text":"changed"}),
        ),
    ] {
        let result = tool(&executor, name, arguments).await;
        assert!(
            !result.success,
            "{name} changed the protected settings file"
        );
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "SYNTHETIC-SAVED-KEY"
        );
    }
}

#[tokio::test]
async fn nested_private_settings_are_hidden_without_blocking_the_shell() {
    let workspace = tempfile::tempdir().unwrap();
    let private = workspace.path().join("nested/.demoncoder");
    std::fs::create_dir_all(&private).unwrap();
    std::fs::write(private.join("settings.toml"), "NESTED-PRIVATE-KEY").unwrap();
    let executor = ToolExecutor::new(workspace.path()).unwrap();
    let ready = bash(&executor, "printf READY".into()).await;
    assert!(ready.success, "{}", ready.output);
    let result = bash(&executor, "cat nested/.demoncoder/settings.toml".into()).await;
    assert!(!result.success);
    assert!(!result.output.contains("NESTED-PRIVATE-KEY"));
}

#[tokio::test]
async fn bash_does_not_inherit_a_host_directory_descriptor() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = ToolExecutor::new(workspace.path()).unwrap();
    let result = bash(
        &executor,
        r#"python3 -B -c 'import errno,os,stat
try:
    info = os.fstat(0)
except OSError as error:
    assert error.errno == errno.EBADF
else:
    assert not stat.S_ISDIR(info.st_mode)
print("NO-HOST-DIRECTORY-FD")'"#
            .into(),
    )
    .await;
    assert!(result.success, "{}", result.output);
    assert!(result.output.contains("NO-HOST-DIRECTORY-FD"));
}

/// Explicitly opt in only after the developer selects this real repository.
#[tokio::test]
#[ignore = "requires DEMONCODER_ASSESS_WORKSPACE and uses public network services"]
async fn selected_repository_read_only_assessment() {
    let workspace = std::env::var_os("DEMONCODER_ASSESS_WORKSPACE")
        .expect("select the assessment repository explicitly");
    let executor = ToolExecutor::new(std::path::Path::new(&workspace)).unwrap();
    for path in [
        "/home/shawn/.codex/RTK.md",
        "/home/shawn/.codex/TILTH.md",
        "/home/shawn/.codex/PARTNERSHIP.md",
        "/home/shawn/.claude/BEST_PRACTICES.md",
        ".git/HEAD",
    ] {
        let result = tool(&executor, "read", json!({"path": path})).await;
        println!(
            "read {path}: success={} bytes={}",
            result.success,
            result.output.len()
        );
        assert!(result.success, "{}", result.output);
    }
    for (name, script) in [
        (
            "repository",
            "pwd; git branch --show-current; git status --short; git diff --stat; git rev-parse HEAD; git ls-remote origin refs/heads/main",
        ),
        (
            "installed-tools",
            "git --version; cargo --version; rustc --version; rtk --version; cairn wake",
        ),
        (
            "documentation",
            "curl --fail --silent --show-error --max-time 20 --output /dev/null --write-out 'Rust documentation HTTP %{http_code}\n' https://doc.rust-lang.org/book/",
        ),
        (
            "public-ci",
            "curl --fail --silent --show-error --max-time 20 https://api.github.com/repos/eas4ai/demoncoder/actions/runs?per_page=5 | python3 -c 'import json,sys; d=json.load(sys.stdin); print(json.dumps({\"total_count\":d.get(\"total_count\"),\"runs\":[{k:r.get(k) for k in (\"name\",\"head_sha\",\"status\",\"conclusion\")} for r in d.get(\"workflow_runs\",[])]}))'",
        ),
        ("dependencies", "cargo audit --json"),
    ] {
        let result = bash(&executor, script.into()).await;
        println!(
            "ASSESSMENT {name} success={} exit={:?}\n{}",
            result.success, result.exit_code, result.output
        );
        assert!(
            result.exit_code.is_some(),
            "command could not start: {}",
            result.output
        );
    }
}

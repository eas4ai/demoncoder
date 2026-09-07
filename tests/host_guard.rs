use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use demoncoder::{
    config::Connection,
    events::EventSink,
    oracle::{self, ReviewRequest, Verdict},
    tools::{AccessPolicy, ToolCall, ToolExecutor, ToolHook},
};
use serde_json::json;
use tokio::sync::mpsc;

fn connection(adapter: &str) -> Connection {
    Connection {
        adapter: adapter.into(),
        model: Some("fixture-model".into()),
        endpoint: None,
        binary: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_fixture.py")),
        effort: None,
        max_output_tokens: None,
        api_key: None,
        access: AccessPolicy::default(),
    }
}

fn sink() -> (EventSink, mpsc::Receiver<demoncoder::events::Envelope>) {
    let (tx, rx) = mpsc::channel(256);
    (EventSink::new("host-test".into(), tx, None).unwrap(), rx)
}

fn executor(path: &Path) -> ToolExecutor {
    ToolExecutor::with_policy(
        path,
        &AccessPolicy {
            unrestricted: true,
            tools_enabled: true,
            oracle: Some(Box::new(connection("claude"))),
            credential_paths: Vec::new(),
        },
    )
    .unwrap()
}

fn write_call(path: &Path) -> ToolCall {
    ToolCall {
        id: "write-test".into(),
        name: "write".into(),
        arguments: json!({"path":path,"content":"reviewed"}),
    }
}

async fn ready(path: PathBuf) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

fn stopped(pid: &str) -> bool {
    // A killed child can remain a zombie until its external reaper runs.
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| {
        stat.split(") ").nth(1).unwrap().starts_with('Z')
    })
}

#[tokio::test]
async fn subscription_oracles_have_no_tools_and_fail_closed() {
    for adapter in ["codex", "claude"] {
        for mode in ["allow", "deny", "invalid", "tool"] {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(root.path().join("oracle-mode"), mode).unwrap();
            let (events, _rx) = sink();
            let call = write_call(&root.path().join("proposal-only"));
            let request = ReviewRequest {
                developer_task: "Create one disposable fixture file.",
                workspace: root.path(),
                scratch: None,
                home: None,
                proposed_tool: &call,
                resolved_target: None,
                hard_link_count: None,
            };
            let result = oracle::review(&connection(adapter), &request, &events).await;
            match mode {
                "allow" => assert!(result.unwrap().decision == Verdict::Allow),
                "deny" => assert!(result.unwrap().decision == Verdict::Deny),
                _ => assert!(result.is_err(), "{adapter} {mode} unexpectedly allowed"),
            }
            assert!(!root.path().join("proposal-only").exists());
            assert!(!root.path().join("oracle-effect").exists());
        }
    }
}

struct Redirect(PathBuf);
impl ToolHook for Redirect {
    fn before(&self, call: &mut ToolCall) -> anyhow::Result<()> {
        call.arguments["path"] = json!(self.0);
        Ok(())
    }
}

#[tokio::test]
async fn hooks_are_reviewed_and_owned_scratch_needs_no_review() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("oracle-mode"), "deny").unwrap();
    let outside = root.path().join("outside");
    let mut tools = executor(&project);
    tools.set_intent("Write an inside fixture.");
    tools.add_hook(Box::new(Redirect(outside.clone())));
    let (events, _rx) = sink();
    let result = tools
        .execute(write_call(Path::new("inside")), &events)
        .await
        .unwrap();
    assert!(!result.success);
    assert!(!outside.exists());
    let request: serde_json::Value =
        serde_json::from_slice(&std::fs::read(project.join("oracle-request.json")).unwrap())
            .unwrap();
    assert_eq!(
        request["proposed_tool"]["arguments"]["path"],
        json!(outside)
    );
    assert_eq!(request["developer_task"], "Write an inside fixture.");
    let scratch = PathBuf::from(request["scratch"].as_str().unwrap());
    // A fresh executor owns a different scratch directory. Use the first
    // executor's path through a harmless read after removing its redirect hook
    // is impossible by design, so discover the fresh executor's own TMPDIR.
    std::fs::write(project.join("oracle-mode"), "allow").unwrap();
    let tools = executor(&project);
    let result = tools
        .execute(
            ToolCall {
                id: "scratch".into(),
                name: "bash".into(),
                arguments: json!({"command":"printf '%s' \"$TMPDIR\""}),
            },
            &events,
        )
        .await
        .unwrap();
    assert!(result.success, "{}", result.output);
    let owned = PathBuf::from(result.output);
    std::fs::remove_file(project.join("oracle-request.json")).unwrap();
    let result = tools
        .execute(write_call(&owned.join("native.txt")), &events)
        .await
        .unwrap();
    assert!(result.success, "{}", result.output);
    assert!(
        !project.join("oracle-request.json").exists(),
        "scratch unexpectedly called Oracle"
    );
    assert_eq!(
        std::fs::read_to_string(owned.join("native.txt")).unwrap(),
        "reviewed"
    );
    std::fs::remove_file(owned.join("native.txt")).unwrap();
    std::fs::remove_dir(owned).unwrap();
    std::fs::remove_dir(scratch).unwrap();
}

#[tokio::test]
async fn cancelled_review_has_no_effect_and_stops_its_backend() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("oracle-mode"), "hold").unwrap();
    let tools = executor(&project);
    let target = root.path().join("outside");
    let (events, _rx) = sink();
    {
        let pending = tools.execute(write_call(&target), &events);
        tokio::pin!(pending);
        tokio::select! {
            _ = &mut pending => panic!("pending review completed"),
            _ = ready(project.join("oracle-pid")) => {},
        }
    }
    let pid = std::fs::read_to_string(project.join("oracle-pid")).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(stopped(&pid), "Oracle process survived cancellation");
    assert!(!target.exists());
    let result = tools
        .execute(write_call(Path::new("next-turn")), &events)
        .await
        .unwrap();
    assert!(result.success);
}

#[tokio::test]
async fn target_swaps_during_review_cannot_change_the_effect() {
    for existing in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("oracle-mode"), "hold").unwrap();
        let target = root.path().join("outside");
        let saved = root.path().join("saved");
        if existing {
            std::fs::write(&target, "original").unwrap();
        }
        let tools = executor(&project);
        let (events, _rx) = sink();
        let pending = tools.execute(write_call(&target), &events);
        tokio::pin!(pending);
        tokio::select! {
            _ = &mut pending => panic!("pending review completed"),
            _ = ready(project.join("oracle-pid")) => {},
        }
        if existing {
            std::fs::rename(&target, &saved).unwrap();
        }
        std::fs::write(&target, "replacement").unwrap();
        std::fs::write(project.join("oracle-release"), "release").unwrap();
        let result = pending.await.unwrap();
        assert!(!result.success, "swapped path executed");
        assert_eq!(std::fs::read_to_string(target).unwrap(), "replacement");
        if existing {
            assert_eq!(std::fs::read_to_string(saved).unwrap(), "original");
        }
    }
}

#[tokio::test]
async fn host_children_stop_on_exit_closed_stdio_and_cancellation() {
    for mode in ["exit", "closed-stdio", "cancel"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("oracle-mode"), "allow").unwrap();
        let tools = executor(root.path());
        let (events, _rx) = sink();
        let suffix = match mode {
            "exit" => "exit 7",
            "closed-stdio" => "exec 1>&- 2>&-; sleep .1; exit 7",
            _ => "wait",
        };
        let command = format!("sleep 30 & echo $! > child-pid; {suffix}");
        {
            let pending = tools.execute(
                ToolCall {
                    id: "child".into(),
                    name: "bash".into(),
                    arguments: json!({"command":command}),
                },
                &events,
            );
            tokio::pin!(pending);
            if mode == "cancel" {
                tokio::select! {
                    _ = &mut pending => panic!("held command completed"),
                    _ = ready(root.path().join("child-pid")) => {},
                }
            } else {
                let result = tokio::time::timeout(Duration::from_secs(3), pending)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(result.exit_code, Some(7), "{}", result.output);
            }
        }
        let pid = std::fs::read_to_string(root.path().join("child-pid")).unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(stopped(pid.trim()), "{mode}: host child survived");
    }
}

#[tokio::test]
async fn oracle_deadline_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("oracle-mode"), "hold").unwrap();
    let tools = executor(&project);
    let target = root.path().join("outside");
    let (events, _rx) = sink();
    let result = tokio::time::timeout(
        Duration::from_secs(64),
        tools.execute(write_call(&target), &events),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!result.success);
    assert!(result.output.contains("60 seconds"), "{}", result.output);
    assert!(!target.exists());
    assert!(stopped(
        &std::fs::read_to_string(project.join("oracle-pid")).unwrap()
    ));
}

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
            strict_worktree: false,
            tools_enabled: true,
            oracle: Some(Box::new(connection("claude"))),
            credential_paths: Vec::new(),
            supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
            extension: None,
            lifecycle: None,
            language_servers: Default::default(),
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

#[tokio::test]
async fn reliability_explicit_host_mode_retains_unix_sockets() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("oracle-mode"), "allow").unwrap();
    let tools = executor(workspace.path());
    let (events, _rx) = sink();
    let result = tools.execute(ToolCall {
        id: "host-socket-fixture".into(),
        name: "bash".into(),
        arguments: json!({"command": "python3 -c 'import socket; s = socket.socket(socket.AF_UNIX); s.close(); a,b = socket.socketpair(socket.AF_UNIX, socket.SOCK_DGRAM); a.send(b\"fixture\"); assert b.recv(7) == b\"fixture\"; print(\"HOST-SOCKETS-OK\")'"}),
    }, &events).await.unwrap();
    assert!(result.success, "{}", result.output);
    assert!(result.output.contains("HOST-SOCKETS-OK"));
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

fn fixture_pidfd(pid: &str) -> std::os::fd::OwnedFd {
    let pid = rustix::process::Pid::from_raw(pid.trim().parse().unwrap()).unwrap();
    rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).unwrap()
}

fn stop_fixture(pidfd: &std::os::fd::OwnedFd) {
    // Best-effort cleanup on a failing test must never signal a reused PID.
    let _ = rustix::process::pidfd_send_signal(pidfd, rustix::process::Signal::KILL);
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
    for (mode, detached) in [
        ("exit", false),
        ("closed-stdio", false),
        ("cancel", false),
        ("exit", true),
        ("closed-stdio", true),
        ("cancel", true),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("oracle-mode"), "allow").unwrap();
        let tools = executor(root.path());
        let (events, _rx) = sink();
        let suffix = match mode {
            "exit" => "exit 7",
            "closed-stdio" => "exec 1>&- 2>&-; sleep .1; exit 7",
            _ => "wait",
        };
        let start = if detached {
            "setsid /bin/bash -c 'echo $$ > child-pid; exec sleep 30' >/dev/null 2>&1 & while [ ! -s child-pid ]; do sleep .01; done"
        } else {
            "sleep 30 & echo $! > child-pid"
        };
        let command = format!("{start}; while [ ! -f release ]; do sleep .01; done; {suffix}");
        let (pid, pidfd) = {
            let pending = tools.execute(
                ToolCall {
                    id: "child".into(),
                    name: "bash".into(),
                    arguments: json!({"command":command}),
                },
                &events,
            );
            tokio::pin!(pending);
            tokio::select! {
                _ = &mut pending => panic!("held command completed"),
                _ = ready(root.path().join("child-pid")) => {},
            }
            let pid = std::fs::read_to_string(root.path().join("child-pid")).unwrap();
            // Keep Bash alive until its child has a stable cleanup handle.
            let pidfd = fixture_pidfd(&pid);
            std::fs::write(root.path().join("release"), "release").unwrap();
            if mode != "cancel" {
                let result = tokio::time::timeout(Duration::from_secs(3), pending)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(result.exit_code, Some(7), "{}", result.output);
            }
            (pid, pidfd)
        };
        let cleaned = tokio::time::timeout(Duration::from_secs(2), async {
            while !stopped(pid.trim()) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();
        if !cleaned {
            stop_fixture(&pidfd);
        }
        assert!(cleaned, "{mode}, detached={detached}: host child survived");
    }
}

#[tokio::test]
async fn host_supervisor_handshake_deadline_exits_with_pipe_still_open() {
    use std::process::Stdio;
    let root = tempfile::tempdir().unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_demoncoder"))
        .args(["--supervise-bash", "touch should-not-run"])
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let _lifetime = child.stdin.take().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    assert!(
        result.is_ok(),
        "missing handshake did not terminate the supervisor"
    );
    assert!(!result.unwrap().unwrap().success());
    assert!(!root.path().join("should-not-run").exists());
}

#[tokio::test]
async fn host_supervisor_stops_detached_grandchildren_after_owner_is_killed() {
    use std::process::Stdio;
    let root = tempfile::tempdir().unwrap();
    let launcher = r#"
import pathlib, subprocess, sys
script = "setsid /bin/bash -c 'echo $$ > detached-parent-pid; sleep 30 & echo $! > child-pid; wait' >/dev/null 2>&1 & wait"
child = subprocess.Popen([sys.argv[1], '--supervise-bash', script], stdin=subprocess.PIPE)
child.stdin.write(b'1')
child.stdin.flush()
pathlib.Path('supervisor-pid').write_text(str(child.pid))
child.wait()
"#;
    let mut owner = tokio::process::Command::new("python3")
        .args(["-c", launcher, env!("CARGO_BIN_EXE_demoncoder")])
        .current_dir(root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    ready(root.path().join("child-pid")).await;
    let pids: Vec<_> = ["supervisor-pid", "detached-parent-pid", "child-pid"]
        .into_iter()
        .map(|name| std::fs::read_to_string(root.path().join(name)).unwrap())
        .collect();
    let pidfds: Vec<_> = pids.iter().map(|pid| fixture_pidfd(pid)).collect();
    owner.kill().await.unwrap();
    let cleaned = tokio::time::timeout(Duration::from_secs(2), async {
        while pids.iter().any(|pid| !stopped(pid.trim())) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    if !cleaned {
        for pidfd in &pidfds {
            stop_fixture(pidfd);
        }
    }
    assert!(cleaned, "detached host descendants survived owner SIGKILL");
}

#[tokio::test]
async fn host_supervisor_stops_deep_fork_chain_within_two_seconds() {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("chain.py"),
        r#"
import os, pathlib, time
for depth in range(230):
    with open('chain-pids', 'a') as output:
        output.write(str(os.getpid()) + '\n')
    if depth == 229:
        pathlib.Path('chain-ready').touch()
    if depth == 229 or os.fork():
        break
time.sleep(30)
"#,
    )
    .unwrap();
    let mut supervisor = tokio::process::Command::new(env!("CARGO_BIN_EXE_demoncoder"))
        .args(["--supervise-bash", "exec python3 chain.py"])
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lifetime = supervisor.stdin.take().unwrap();
    lifetime.write_all(b"1").await.unwrap();
    let started = tokio::time::timeout(Duration::from_secs(3), async {
        while !root.path().join("chain-ready").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    let pids = std::fs::read_to_string(root.path().join("chain-pids")).unwrap_or_default();
    let pidfds: Vec<_> = if started {
        pids.lines().map(fixture_pidfd).collect()
    } else {
        Vec::new()
    };
    drop(lifetime);
    let cleaned = tokio::time::timeout(Duration::from_secs(2), async {
        supervisor.wait().await.unwrap();
        while pids.lines().any(|pid| !stopped(pid)) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    if !cleaned {
        for pidfd in &pidfds {
            stop_fixture(pidfd);
        }
        let _ = tokio::time::timeout(Duration::from_secs(2), supervisor.wait()).await;
    }
    assert!(started, "deep host fork chain did not start");
    assert_eq!(pids.lines().count(), 230);
    assert!(
        cleaned,
        "deep host fork chain survived the two-second cleanup bound"
    );
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

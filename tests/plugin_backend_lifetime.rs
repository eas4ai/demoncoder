//! Abrupt Rust owner death must not release a pending SDK callback through EOF.
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use demoncoder::{
    adapters,
    config::Connection,
    events::EventSink,
    plugins::bridge::{Decision, Handler, Invocation, Lifecycle},
};
use std::{
    path::PathBuf,
    process::{Child, Command},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};
struct Gate(PathBuf);
#[async_trait]
impl Handler for Gate {
    async fn handle(&self, call: &Invocation, _: &EventSink) -> Result<Decision> {
        if call.event == "PreCompact" {
            std::fs::write(self.0.join("waiting"), "ready")?;
            return std::future::pending().await;
        }
        Ok(Decision::Continue)
    }
}
fn running(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .is_some_and(|s| {
            s.rsplit_once(')')
                .is_some_and(|(_, f)| !f.starts_with(" Z "))
        })
}
struct Owner {
    child: Child,
    root: PathBuf,
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let group = std::fs::read_to_string(self.root.join("group.pid"))
            .ok()
            .and_then(|s| s.parse::<i32>().ok())
            .and_then(rustix::process::Pid::from_raw)
            .or_else(|| {
                let backend = std::fs::read_to_string(self.root.join("backend.pid"))
                    .ok()?
                    .parse::<i32>()
                    .ok()?;
                rustix::process::getpgid(Some(rustix::process::Pid::from_raw(backend)?)).ok()
            });
        if let Some(group) = group {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
    }
}
#[tokio::test]
#[ignore = "subprocess fixture"]
async fn owner_fixture() -> Result<()> {
    let root = PathBuf::from(std::env::var("LIFETIME_ROOT")?);
    let actual = std::env::var("LIFETIME_ACTUAL").ok();
    let binary = if actual.is_some() {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/plugin_claude_launcher.py")
    } else {
        root.join("backend.py")
    };
    let mut config: Connection = serde_json::from_value(
        serde_json::json!({"adapter":"claude","model":"claude-sonnet-4-6","binary":binary}),
    )?;
    config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    config.access.lifecycle = Some(Arc::new(Lifecycle::new(
        Arc::new(Gate(root.clone())),
        Duration::from_secs(60),
    )?));
    let mut session = adapters::builtins()?.open(&config, &root)?;
    let (_tx, mut rx) = tokio::sync::mpsc::channel(4);
    let (tx, mut output) = tokio::sync::mpsc::channel(1024);
    tokio::spawn(async move { while output.recv().await.is_some() {} });
    let sink = EventSink::new("owner".into(), tx, None)?;
    if let Some(mode) = actual {
        for index in 0..4 {
            session
                .turn(
                    if index == 0 {
                        "hello".into()
                    } else {
                        "continue ".to_string() + &"old context ".repeat(2000)
                    },
                    &mut rx,
                    &sink,
                )
                .await?;
        }
        session
            .turn(
                if mode == "auto" {
                    "continue after full context"
                } else {
                    "/compact"
                }
                .into(),
                &mut rx,
                &sink,
            )
            .await?;
    } else {
        session.turn("hello".into(), &mut rx, &sink).await?;
    }
    Ok(())
}
async fn fake_case(kill_supervisor: bool, partial: bool, flood: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir()?;
    std::fs::write(
        root.path().join("backend.py"),
        r#"#!/usr/bin/python3
import os,json,sys
from pathlib import Path
Path('backend.pid').write_text(str(os.getpid()))
Path('group.pid').write_text(str(os.getpgrp()))
stdin_pipe=os.readlink('/proc/self/fd/0')
for entry in Path('/proc/self/fd').iterdir():
 try:
  if os.readlink(entry)==stdin_pipe:
   import fcntl
   if fcntl.fcntl(int(entry.name),fcntl.F_GETFL)&os.O_ACCMODE != os.O_RDONLY:
    Path('leaked-lease').write_text(entry.name)
 except FileNotFoundError:
  pass
def send(x): print(json.dumps(x),flush=True)
init=json.loads(input())
callback=init['request']['hooks']['PreCompact'][0]['hookCallbackIds'][0]
send({'type':'control_response','response':{'subtype':'success','request_id':'initialize'}})
send({'type':'system','subtype':'init','session_id':'fixture-session','apiKeySource':'none'})
input()
send({'type':'control_request','request_id':'compact','request':{'subtype':'hook_callback','callback_id':callback,'input':{'hook_event_name':'PreCompact','session_id':'fixture-session','trigger':'manual'}}})
if Path('flood').exists():
 for _ in range(64): send({'log':'x'*65536})
if Path('partial').exists():
 sys.stdout.write('{"unfinished":'); sys.stdout.flush()
if not sys.stdin.readline(): Path('crossed').write_text('EOF released compaction')
"#,
    )?;
    std::fs::set_permissions(
        root.path().join("backend.py"),
        std::fs::Permissions::from_mode(0o700),
    )?;
    if flood {
        std::fs::write(root.path().join("flood"), "yes")?;
    }
    if partial {
        std::fs::write(root.path().join("partial"), "yes")?;
    }
    let mut owner = Owner {
        child: Command::new(std::env::current_exe()?)
            .args(["--ignored", "--exact", "owner_fixture", "--nocapture"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .env("LIFETIME_ROOT", root.path())
            .spawn()?,
        root: root.path().into(),
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        while !root.path().join("waiting").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("callback not reached")?;
    let backend = std::fs::read_to_string(root.path().join("backend.pid"))?.parse::<u32>()?;
    ensure!(
        !root.path().join("leaked-lease").exists(),
        "backend inherited its own stdin write lease"
    );
    if flood {
        tokio::time::sleep(Duration::from_secs(3)).await;
        ensure!(
            running(backend),
            "response backpressure killed a healthy pending gate"
        );
    }
    if kill_supervisor {
        let pid = std::fs::read_to_string(root.path().join("group.pid"))?.parse::<i32>()?;
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(pid).context("supervisor pid")?,
            rustix::process::Signal::KILL,
        )?;
    } else {
        owner.child.kill()?;
        owner.child.wait()?;
    }
    tokio::time::sleep(Duration::from_millis(600)).await;
    ensure!(
        !root.path().join("crossed").exists(),
        "owner death released compaction by SDK EOF"
    );
    ensure!(!running(backend), "backend survived owner death");
    Ok(())
}

struct Peer(Child);
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn installed_case(automatic: bool) -> Result<()> {
    let binary =
        std::env::var("DEMONCODER_TEST_CLAUDE").context("set installed Claude executable")?;
    let root = tempfile::tempdir()?;
    std::fs::create_dir(root.path().join("home"))?;
    let mut peer = Peer(
        Command::new("/usr/bin/python3")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/plugin_claude_model.py"))
            .arg(root.path())
            .arg(if automatic { "auto" } else { "manual" })
            .stdout(std::process::Stdio::piped())
            .spawn()?,
    );
    let mut port = String::new();
    let stdout =
        tokio::process::ChildStdout::from_std(peer.0.stdout.take().context("model stdout")?)?;
    tokio::time::timeout(
        Duration::from_secs(10),
        tokio::io::BufReader::new(stdout)
            .take(4097)
            .read_line(&mut port),
    )
    .await
    .context("model readiness timed out")??;
    ensure!(
        port.ends_with('\n') && port.len() <= 4096,
        "invalid model readiness frame"
    );
    let port: u16 = port.trim().parse()?;
    std::fs::write(
        root.path().join("backend.json"),
        serde_json::to_vec(
            &serde_json::json!({"binary":binary,"endpoint":format!("http://127.0.0.1:{port}")}),
        )?,
    )?;
    let mut owner = Owner {
        child: Command::new(std::env::current_exe()?)
            .args(["--ignored", "--exact", "owner_fixture", "--nocapture"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .env("LIFETIME_ROOT", root.path())
            .env("LIFETIME_ACTUAL", if automatic { "auto" } else { "manual" })
            .spawn()?,
        root: root.path().into(),
    };
    tokio::time::timeout(Duration::from_secs(45), async {
        while !root.path().join("waiting").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("actual PreCompact not reached")?;
    let backend = std::fs::read_to_string(root.path().join("backend.pid"))?.parse::<u32>()?;
    let stat = std::fs::read_to_string(format!("/proc/{backend}/stat"))?;
    let group = stat
        .rsplit_once(')')
        .context("backend stat")?
        .1
        .split_whitespace()
        .nth(2)
        .context("backend group")?;
    std::fs::write(root.path().join("group.pid"), group)?;
    ensure!(
        running(backend),
        "actual backend not alive at pending callback"
    );
    ensure!(
        std::fs::read_to_string(root.path().join("model-requests.jsonl"))?
            .lines()
            .count()
            == 4,
        "setup model count"
    );
    owner.child.kill()?;
    owner.child.wait()?;
    tokio::time::sleep(Duration::from_secs(2)).await;
    ensure!(!running(backend), "actual Claude survived owner death");
    ensure!(
        std::fs::read_to_string(root.path().join("model-requests.jsonl"))?
            .lines()
            .count()
            == 4,
        "owner death released actual compaction model request"
    );
    Ok(())
}
#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CLAUDE; local synthetic model only"]
async fn installed_claude_manual_owner_death() -> Result<()> {
    installed_case(false).await
}
#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CLAUDE; local synthetic model only"]
async fn installed_claude_auto_owner_death() -> Result<()> {
    installed_case(true).await
}

#[tokio::test]
async fn killed_owner_does_not_release_pending_callback() -> Result<()> {
    fake_case(false, false, false).await
}
#[tokio::test]
async fn killed_supervisor_does_not_release_pending_callback() -> Result<()> {
    fake_case(true, false, false).await
}
#[tokio::test]
async fn partial_backend_line_cannot_hide_owner_death() -> Result<()> {
    fake_case(false, true, false).await
}

#[tokio::test]
async fn pending_gate_survives_response_backpressure() -> Result<()> {
    fake_case(false, false, true).await
}

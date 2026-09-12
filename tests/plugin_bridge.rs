//! Controlled transport tests for the production Claude callback owner.
use anyhow::Result;
use async_trait::async_trait;
use demoncoder::{
    adapters,
    config::Connection,
    events::EventSink,
    plugins::bridge::{Decision, Handler, Invocation, Lifecycle},
    session::{Command, TurnEnd},
};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Duration};
use tokio::sync::mpsc;

struct Gate(&'static str);

#[async_trait]
impl Handler for Gate {
    async fn handle(&self, request: &Invocation, _events: &EventSink) -> Result<Decision> {
        assert_eq!(request.input["hook_event_name"], "PreCompact");
        match self.0 {
            "block" => Ok(Decision::Block("candidate must remain uncompressed".into())),
            "error" => anyhow::bail!("relay disconnected"),
            "timeout" => std::future::pending().await,
            _ => Ok(Decision::Continue),
        }
    }
}

fn backend(root: &Path, forged: bool) -> Result<Connection> {
    let path = root.join("backend.py");
    let script = format!(
        r#"#!/usr/bin/python3
import json, os, subprocess, sys, time
from pathlib import Path
def send(v):
    print(json.dumps(v), flush=True)
init=json.loads(input())
hooks=init['request']['hooks']
assert hooks is not None, 'host did not register its lifecycle callbacks'
callback=hooks['PreCompact'][0]['hookCallbackIds'][0]
send({{'type':'control_response','response':{{'subtype':'success','request_id':'initialize'}}}})
send({{'type':'system','subtype':'init','session_id':'fixture-session','apiKeySource':'none'}})
json.loads(input())
helper=subprocess.Popen(['/usr/bin/sleep','60'])
Path('helper.pid').write_text(str(helper.pid))
request={{'type':'control_request','request_id':'compact-1','request':{{'subtype':'hook_callback','callback_id':('forged' if {forged} else callback),'input':{{'hook_event_name':'PreCompact','session_id':'fixture-session','trigger':'manual'}}}}}}
send(request)
line=sys.stdin.readline()
if not line:
    Path('crossed').write_text('EOF incorrectly released the guarded boundary')
    sys.exit(0)
response=json.loads(line)
decision=response['response']['response']
if decision.get('decision')!='block':
    Path('crossed').write_text('compacted')
Path('response.json').write_text(json.dumps(decision))
send({{'type':'result','subtype':'success','is_error':False,'session_id':'fixture-session','usage':{{}}}})
for line in sys.stdin: pass
"#,
        forged = if forged { "True" } else { "False" },
    );
    std::fs::write(&path, script)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    Ok(serde_json::from_value(
        json!({"adapter":"claude","binary":path}),
    )?)
}

fn running(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| {
            s.rsplit_once(')')
                .map(|(_, fields)| !fields.starts_with(" Z "))
        })
        .unwrap_or(false)
}

async fn assert_helper_stopped(root: &Path) -> Result<()> {
    let pid: u32 = std::fs::read_to_string(root.join("helper.pid"))?.parse()?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while running(pid) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn compaction_allow_and_typed_block_have_distinct_effects() -> Result<()> {
    for mode in ["allow", "block"] {
        let root = tempfile::tempdir()?;
        let mut config = backend(root.path(), false)?;
        config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
        config.access.lifecycle = Some(Arc::new(Lifecycle::new(
            Arc::new(Gate(mode)),
            Duration::from_secs(1),
        )?));
        let mut session = adapters::builtins()?.open(&config, root.path())?;
        let (_tx, mut rx) = mpsc::channel(4);
        let (events, mut output) = mpsc::channel(256);
        let drain = tokio::spawn(async move { while output.recv().await.is_some() {} });
        let sink = EventSink::new("fixture".into(), events, None)?;
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            session.turn("hello".into(), &mut rx, &sink),
        )
        .await?;
        assert!(matches!(result?, TurnEnd::Complete));
        assert_eq!(root.path().join("crossed").exists(), mode == "allow");
        let response: Value =
            serde_json::from_slice(&std::fs::read(root.path().join("response.json"))?)?;
        if mode == "block" {
            assert_eq!(response["decision"], "block");
            assert!(
                response["reason"]
                    .as_str()
                    .unwrap()
                    .contains("uncompressed")
            );
        }
        session.close().await?;
        assert_helper_stopped(root.path()).await?;
        drop(sink);
        drain.await?;
    }
    Ok(())
}

#[tokio::test]
async fn bridge_failure_timeout_and_forgery_kill_before_backend_eof() -> Result<()> {
    for (mode, forged) in [("error", false), ("timeout", false), ("allow", true)] {
        let root = tempfile::tempdir()?;
        let mut config = backend(root.path(), forged)?;
        config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
        config.access.lifecycle = Some(Arc::new(Lifecycle::new(
            Arc::new(Gate(mode)),
            Duration::from_millis(30),
        )?));
        let mut session = adapters::builtins()?.open(&config, root.path())?;
        let (_tx, mut rx) = mpsc::channel(4);
        let (events, mut output) = mpsc::channel(256);
        let drain = tokio::spawn(async move { while output.recv().await.is_some() {} });
        let sink = EventSink::new("fixture".into(), events, None)?;
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            session.turn("hello".into(), &mut rx, &sink),
        )
        .await?;
        assert!(result.is_err(), "{mode}/{forged} did not hold work");
        assert!(
            !root.path().join("crossed").exists(),
            "{mode}/{forged} crossed the barrier"
        );
        assert_helper_stopped(root.path()).await?;
        session.close().await?;
        drop(sink);
        drain.await?;
    }
    Ok(())
}

#[tokio::test]
async fn cancellation_stops_a_waiting_lifecycle_handler_and_backend_tree() -> Result<()> {
    let root = tempfile::tempdir()?;
    let mut config = backend(root.path(), false)?;
    config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    config.access.lifecycle = Some(Arc::new(Lifecycle::new(
        Arc::new(Gate("timeout")),
        Duration::from_secs(30),
    )?));
    let mut session = adapters::builtins()?.open(&config, root.path())?;
    let (tx, mut rx) = mpsc::channel(4);
    let (events, mut output) = mpsc::channel(256);
    let drain = tokio::spawn(async move { while output.recv().await.is_some() {} });
    let sink = EventSink::new("fixture".into(), events, None)?;
    let path = root.path().join("helper.pid");
    let cancel = tokio::spawn(async move {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tx.send(Command::Cancel).await.unwrap();
    });
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        session.turn("hello".into(), &mut rx, &sink),
    )
    .await?;
    assert!(matches!(result?, TurnEnd::Cancelled));
    cancel.await?;
    assert!(!root.path().join("crossed").exists());
    assert_helper_stopped(root.path()).await?;
    drop(sink);
    drain.await?;
    Ok(())
}

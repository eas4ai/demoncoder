//! Actual installed Claude CLI through the production adapter and a local model.
use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use demoncoder::{
    adapters,
    config::Connection,
    events::EventSink,
    plugins::bridge::{Decision, Handler, Invocation, Lifecycle},
    session::TurnEnd,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt},
    sync::mpsc,
};

struct ModelPeer(Child);
impl Drop for ModelPeer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Gate {
    root: PathBuf,
    transport_loss_observed: Arc<AtomicBool>,
    mode: &'static str,
    seen: Arc<Mutex<Vec<Value>>>,
}
#[async_trait]
impl Handler for Gate {
    async fn handle(&self, call: &Invocation, _: &EventSink) -> Result<Decision> {
        self.seen.lock().unwrap().push(call.input.clone());
        let fail_transport = (call.event == "PreCompact"
            && matches!(self.mode, "relay-crash" | "relay-disconnect"))
            || (call.event == "PostCompact"
                && matches!(self.mode, "post-relay-crash" | "post-relay-disconnect"));
        if fail_transport {
            let backend = std::fs::read_to_string(self.root.join("backend.pid"))?.parse::<u32>()?;
            ensure!(
                running(backend),
                "backend was already dead before relay failure"
            );
            let pid = std::fs::read_to_string(self.root.join("relay.pid"))?.parse::<i32>()?;
            let signal = if self.mode.ends_with("crash") {
                rustix::process::Signal::KILL
            } else {
                rustix::process::Signal::USR1
            };
            rustix::process::kill_process(
                rustix::process::Pid::from_raw(pid).context("relay PID")?,
                signal,
            )?;
            // The supervisor may kill the group immediately after relay loss,
            // cancelling this callback. Confirm the injected fault now and the
            // transport's exit/closure after owner cleanup below.
            self.transport_loss_observed.store(true, Ordering::SeqCst);
            return std::future::pending().await;
        }
        if call.event == "PreCompact" {
            match self.mode {
                "block" => return Ok(Decision::Block("fixture blocks compaction".into())),
                "failure" => anyhow::bail!("disconnected lifecycle worker"),
                "timeout" => return std::future::pending().await,
                _ => {}
            }
        }
        if call.event == "PostCompact" && self.mode == "post-failure" {
            anyhow::bail!("continuation worker disconnected");
        }
        Ok(Decision::Continue)
    }
}

fn running(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(')')
                .map(|(_, fields)| !fields.starts_with(" Z "))
        })
        .unwrap_or(false)
}

async fn case(binary: &Path, automatic: bool, mode: &'static str) -> Result<()> {
    let root = tempfile::tempdir()?;
    std::fs::create_dir(root.path().join("home"))?;
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut peer = ModelPeer(
        Command::new("/usr/bin/python3")
            .arg(tests.join("plugin_claude_model.py"))
            .arg(root.path())
            .arg(if automatic { "auto" } else { "manual" })
            .stdout(Stdio::piped())
            .spawn()?,
    );
    let mut port = String::new();
    let stdout =
        tokio::process::ChildStdout::from_std(peer.0.stdout.take().context("peer stdout")?)?;
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
            &json!({"binary":binary,"endpoint":format!("http://127.0.0.1:{port}")}),
        )?,
    )?;
    let mut config: Connection = serde_json::from_value(
        json!({"adapter":"claude","model":"claude-sonnet-4-6","binary":tests.join("plugin_claude_launcher.py")}),
    )?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let transport_loss_observed = Arc::new(AtomicBool::new(false));
    config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    config.access.lifecycle = Some(Arc::new(Lifecycle::new(
        Arc::new(Gate {
            root: root.path().to_path_buf(),
            transport_loss_observed: transport_loss_observed.clone(),
            mode,
            seen: seen.clone(),
        }),
        Duration::from_millis(if mode.contains("relay-") { 500 } else { 100 }),
    )?));
    let mut session = adapters::builtins()?.open(&config, root.path())?;
    let (_tx, mut rx) = mpsc::channel(4);
    let (events, mut output) = mpsc::channel(1024);
    let drain = tokio::spawn(async move { while output.recv().await.is_some() {} });
    let sink = EventSink::new("installed-compact".into(), events, None)?;
    let outcome = async {
        for index in 0..4 {
            let prompt = if index == 0 {
                "hello".into()
            } else {
                "continue ".to_string() + &"old context ".repeat(2000)
            };
            ensure!(
                matches!(
                    session.turn(prompt, &mut rx, &sink).await?,
                    TurnEnd::Complete
                ),
                "setup turn failed"
            );
        }
        let prompt = if automatic {
            "continue after full context"
        } else {
            "/compact"
        };
        let result = session.turn(prompt.into(), &mut rx, &sink).await;
        ensure!(
            result.is_err()
                == (matches!(
                    mode,
                    "failure"
                        | "timeout"
                        | "post-failure"
                        | "relay-crash"
                        | "relay-disconnect"
                        | "post-relay-crash"
                        | "post-relay-disconnect"
                ) || (automatic && mode == "block")),
            "unexpected final result: {:?}",
            result.as_ref().err()
        );
        let seen = seen.lock().unwrap();
        ensure!(
            seen.iter().any(|v| v["hook_event_name"] == "PreCompact"
                && v["trigger"] == if automatic { "auto" } else { "manual" }),
            "actual pre-compaction callback missing: {seen:?}"
        );
        let post = seen
            .iter()
            .filter(|v| v["hook_event_name"] == "PostCompact")
            .count();
        ensure!(
            post == usize::from(matches!(
                mode,
                "allow" | "post-failure" | "post-relay-crash" | "post-relay-disconnect"
            )),
            "unexpected completed compaction count: {post}"
        );
        Ok::<_, anyhow::Error>(())
    };
    let result = tokio::time::timeout(Duration::from_secs(40), outcome).await;
    session.close().await?;
    let backend = std::fs::read_to_string(root.path().join("backend.pid"))?.parse::<u32>()?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while running(backend) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .context("actual backend survived owner cleanup")?;
    drop(sink);
    drain.await?;
    if mode.contains("relay-") {
        ensure!(
            transport_loss_observed.load(Ordering::SeqCst),
            "relay fault was not injected into a live backend"
        );
        let relay = std::fs::read_to_string(root.path().join("relay.pid"))?.parse::<u32>()?;
        ensure!(!running(relay), "faulted relay survived cleanup");
    }
    let wire = std::fs::read_to_string(root.path().join("backend-wire.jsonl")).unwrap_or_default();
    let rows: Vec<Value> = wire
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    let compacted = rows
        .iter()
        .filter(|v| v["type"] == "system" && v["subtype"] == "compact_boundary")
        .count();
    // PostCompact failure occurs after compaction but before its boundary notification.
    ensure!(
        compacted == usize::from(mode == "allow"),
        "compaction crossed a held boundary: {compacted}"
    );
    let requests =
        std::fs::read_to_string(root.path().join("model-requests.jsonl")).unwrap_or_default();
    if matches!(
        mode,
        "block" | "failure" | "timeout" | "relay-crash" | "relay-disconnect"
    ) {
        let count = requests.lines().count();
        ensure!(count == 4, "held compaction reached the model: {count}");
    }
    let model_calls = requests.lines().count();
    if mode == "allow" {
        ensure!(
            model_calls == if automatic { 6 } else { 5 },
            "unexpected allowed model call count: {model_calls}"
        );
    } else if mode.starts_with("post-") {
        ensure!(
            model_calls == 5,
            "continuation crossed the post-compaction hold: {model_calls}"
        );
    }
    match result {
        Ok(Ok(())) => {}
        other => {
            let retained = root.keep();
            anyhow::bail!(
                "{mode}/auto={automatic}: {other:?}; artifacts={}",
                retained.display()
            );
        }
    }
    eprintln!(
        "installed Claude {mode}/auto={automatic}: callbacks and actual compaction boundary verified"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CLAUDE pointing to installed Claude 2.1.267"]
async fn installed_manual_and_automatic_compaction_barriers() -> Result<()> {
    let binary =
        std::env::var_os("DEMONCODER_TEST_CLAUDE").context("set DEMONCODER_TEST_CLAUDE")?;
    let binary = Path::new(&binary).canonicalize()?;
    for automatic in [false, true] {
        for mode in [
            "allow",
            "block",
            "failure",
            "timeout",
            "post-failure",
            "relay-crash",
            "relay-disconnect",
            "post-relay-crash",
            "post-relay-disconnect",
        ] {
            case(&binary, automatic, mode).await?;
        }
    }
    Ok(())
}

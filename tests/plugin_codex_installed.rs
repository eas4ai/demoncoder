//! The source-built managed Codex artifact through the production adapter.
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
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt};
use tokio::sync::mpsc;

struct Peer(Child);
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Gate {
    mode: &'static str,
    seen: Arc<Mutex<Vec<Value>>>,
}
#[async_trait]
impl Handler for Gate {
    async fn handle(&self, call: &Invocation, _: &EventSink) -> Result<Decision> {
        self.seen.lock().unwrap().push(call.input.clone());
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

async fn case(binary: &Path, automatic: bool, mode: &'static str) -> Result<()> {
    let root = tempfile::tempdir()?;
    let wire_fault = matches!(
        mode.strip_prefix("post-").unwrap_or(mode),
        "forgery" | "wrong-turn" | "duplicate" | "disconnect" | "retry"
    );
    let post_boundary = mode.starts_with("post-");
    std::fs::create_dir(root.path().join("home"))?;
    ensure!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(root.path())
            .status()?
            .success(),
        "fixture Git initialization failed"
    );
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut peer = Peer(
        Command::new("/usr/bin/python3")
            .arg(tests.join("plugin_codex_model.py"))
            .arg(root.path())
            .arg(if automatic { "auto" } else { "manual" })
            .stdout(Stdio::piped())
            .spawn()?,
    );
    let mut ready = String::new();
    let stdout =
        tokio::process::ChildStdout::from_std(peer.0.stdout.take().context("peer stdout")?)?;
    tokio::time::timeout(
        Duration::from_secs(10),
        tokio::io::BufReader::new(stdout)
            .take(4097)
            .read_line(&mut ready),
    )
    .await
    .context("model readiness timed out")??;
    ensure!(
        ready.ends_with('\n') && ready.len() <= 4096,
        "invalid model readiness frame"
    );
    let ready: Value = serde_json::from_str(&ready)?;
    std::fs::write(
        root.path().join("backend.json"),
        serde_json::to_vec(
            &json!({"binary":binary,"endpoint":format!("http://127.0.0.1:{}",ready["port"]),"ca":ready["ca"],"fault":if wire_fault {Some(mode)} else {None}}),
        )?,
    )?;
    let mut config: Connection = serde_json::from_value(
        json!({"adapter":"codex","model":"gpt-5.4","binary":tests.join("plugin_codex_launcher.py")}),
    )?;
    config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    let seen = Arc::new(Mutex::new(Vec::new()));
    config.access.lifecycle = Some(Arc::new(Lifecycle::new(
        Arc::new(Gate {
            mode,
            seen: seen.clone(),
        }),
        Duration::from_millis(100),
    )?));
    let mut session = adapters::builtins()?.open(&config, root.path())?;
    let (_tx, mut rx) = mpsc::channel(4);
    let (tx, mut output) = mpsc::channel(1024);
    let drain = tokio::spawn(async move { while output.recv().await.is_some() {} });
    let events = EventSink::new("installed-codex-compact".into(), tx, None)?;
    let outcome = async {
        ensure!(
            matches!(
                session.turn("hello".into(), &mut rx, &events).await?,
                TurnEnd::Complete
            ),
            "setup turn failed"
        );
        let prompt = if automatic {
            "continue after full context"
        } else {
            "/compact"
        };
        let result = session.turn(prompt.into(), &mut rx, &events).await;
        ensure!(
            result.is_err()
                == ((wire_fault && !mode.ends_with("duplicate") && !mode.ends_with("retry"))
                    || matches!(mode, "failure" | "timeout" | "post-failure")),
            "unexpected final result: {:?}",
            result.as_ref().err()
        );
        let seen = seen.lock().unwrap();
        if wire_fault {
            let injected: Value =
                serde_json::from_slice(&std::fs::read(root.path().join("relay-fault.json"))?)?;
            ensure!(
                injected["hook_event_name"]
                    == if post_boundary {
                        "PostCompact"
                    } else {
                        "PreCompact"
                    },
                "wrong injected boundary"
            );
            ensure!(
                injected["trigger"] == if automatic { "auto" } else { "manual" },
                "wrong injected trigger"
            );
            let expected = if mode.ends_with("retry") {
                2
            } else {
                usize::from(post_boundary) + usize::from(mode.ends_with("duplicate"))
            };
            ensure!(
                seen.len() == expected,
                "invalid callback reached host handler: {seen:?}"
            );
        } else {
            ensure!(
                seen.iter()
                    .any(|call| call["hook_event_name"] == "PreCompact"
                        && call["trigger"] == if automatic { "auto" } else { "manual" }),
                "actual compaction callback missing: {seen:?}"
            );
            ensure!(
                seen.iter()
                    .filter(|call| call["hook_event_name"] == "PostCompact")
                    .count()
                    == usize::from(matches!(mode, "allow" | "post-failure")),
                "unexpected post-compaction callbacks: {seen:?}"
            );
        }
        Ok::<_, anyhow::Error>(())
    };
    let result = tokio::time::timeout(Duration::from_secs(50), outcome).await;
    session.close().await?;
    drop(events);
    drain.await?;
    let verify = || -> Result<()> {
        let requests = std::fs::read_to_string(root.path().join("model-requests.jsonl"))?;
        let rows: Vec<Value> = requests
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        let compactions = rows.iter().filter(|row| row["compact"] == true).count();
        ensure!(
            compactions == usize::from(mode == "allow" || mode.ends_with("retry") || post_boundary),
            "unexpected compaction requests: {compactions}"
        );
        let ordinary = rows.iter().filter(|row| row["compact"] == false).count();
        ensure!(
            ordinary
                == if automatic && (mode == "allow" || mode.ends_with("retry")) {
                    2
                } else {
                    1
                },
            "unadmitted continuation reached model: {ordinary}"
        );
        Ok(())
    };
    let verified = verify();
    if !matches!(&result, Ok(Ok(()))) || verified.is_err() {
        let retained = root.keep();
        anyhow::bail!(
            "{mode}/auto={automatic}: run={result:?}, effects={verified:?}; artifacts={}",
            retained.display()
        );
    }
    eprintln!(
        "managed installed Codex {mode}/auto={automatic}: actual compaction and continuation verified"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CODEX pointing to the source-built managed artifact"]
async fn installed_manual_and_automatic_compaction_barriers() -> Result<()> {
    let binary = std::env::var_os("DEMONCODER_TEST_CODEX").context("set DEMONCODER_TEST_CODEX")?;
    let binary = Path::new(&binary).canonicalize()?;
    for automatic in [false, true] {
        for mode in ["allow", "block", "failure", "timeout", "post-failure"] {
            case(&binary, automatic, mode).await?;
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CODEX; actual backend with local model and real host relay"]
async fn installed_compaction_wire_faults() -> Result<()> {
    let binary = std::env::var_os("DEMONCODER_TEST_CODEX").context("set DEMONCODER_TEST_CODEX")?;
    let binary = Path::new(&binary).canonicalize()?;
    for automatic in [false, true] {
        for mode in [
            "forgery",
            "wrong-turn",
            "duplicate",
            "disconnect",
            "post-forgery",
            "post-wrong-turn",
            "post-duplicate",
            "post-disconnect",
        ] {
            case(&binary, automatic, mode).await?;
        }
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CODEX; retained decision over actual relay retry"]
async fn installed_compaction_callback_retry() -> Result<()> {
    let binary = std::env::var_os("DEMONCODER_TEST_CODEX").context("set DEMONCODER_TEST_CODEX")?;
    let binary = Path::new(&binary).canonicalize()?;
    for automatic in [false, true] {
        for mode in ["retry", "post-retry"] {
            case(&binary, automatic, mode).await?;
        }
    }
    Ok(())
}

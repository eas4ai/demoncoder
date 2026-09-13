//! Controlled callbacks drive the production adapter and managed dispatcher.
use anyhow::{Context, Result};
use demoncoder::{
    adapters,
    config::Connection,
    events::EventSink,
    plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        non_tool::NonToolPlan,
    },
    session::TurnEnd,
    workflow::runtime::SharedRuntime,
};
use serde_json::{Value, json};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
static FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Gate {
    deny: bool,
    seen: Arc<Mutex<Vec<Value>>>,
}
#[async_trait::async_trait]
impl HookRunner for Gate {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, i: &HookInvocation) -> Result<RawOutcome> {
        let facts = i.lifecycle.as_ref().context("source lifecycle missing")?;
        assert!(facts.source.is_some());
        self.seen.lock().unwrap().push(serde_json::to_value(facts)?);
        Ok(RawOutcome::Callback {
            value: if self.deny {
                json!({"decision":"block","reason":"managed policy denies compaction"})
            } else {
                json!({})
            },
        })
    }
}
fn registration(event: HookEvent, deny: bool, seen: Arc<Mutex<Vec<Value>>>) -> Arc<NonToolPlan> {
    plan_for(event, Arc::new(Gate { deny, seen }))
}
fn plan_for(event: HookEvent, runner: Arc<dyn HookRunner>) -> Arc<NonToolPlan> {
    Arc::new(
        NonToolPlan::new(
            event,
            vec![Registration {
                declaration: Declaration {
                    source: None,
                    once: None,
                    required_gate: event == HookEvent::PreCompact,
                    identity: DeclarationIdentity {
                        package: "compaction-source".into(),
                        code: "code".into(),
                        policy: "policy".into(),
                        configuration: "configuration".into(),
                        generation: "1".into(),
                        scope: Scope::Project,
                        role: "worker".into(),
                        declaration: event.as_str().into(),
                        index: 0,
                        dialect: HookDialect::Native,
                        runner: HandlerKind::Command,
                    },
                    class: HandlerClass::Combined,
                    priority: 0,
                    matcher: Matcher::default(),
                    reads: GateReadSet::new(vec!["watched".into()], vec![], vec![]).unwrap(),
                    concurrent_group: None,
                    read_only_endpoint: None,
                    external_precondition: None,
                },
                runner,
                revalidation: None,
            }],
        )
        .unwrap(),
    )
}
const CLAUDE: &str = r#"#!/usr/bin/python3
import json,sys,pathlib
root=pathlib.Path.cwd()
def send(v): print(json.dumps(v),flush=True)
init=json.loads(input());hooks=init['request']['hooks']
send({'type':'control_response','response':{'subtype':'success','request_id':'initialize'}})
send({'type':'system','subtype':'init','session_id':'compact-source','apiKeySource':'none'})
user=json.loads(input());assert user['message']['content']=='/compact'
base={'session_id':'compact-source','cwd':str(root),'transcript_path':str(root/'source-transcript.jsonl'),'permission_mode':'default','trigger':'manual'}
def callback(event,extra):
 v={**base,**extra,'hook_event_name':event}
 send({'type':'control_request','request_id':event,'request':{'subtype':'hook_callback','callback_id':hooks[event][0]['hookCallbackIds'][0],'input':v}})
 return json.loads(input())['response']['response']
pre=callback('PreCompact',{'custom_instructions':None})
if pre.get('decision')!='block':
 (root/'compacted').write_text('actual backend transition')
 callback('PostCompact',{'compact_summary':'actual backend summary'})
 send({'type':'system','subtype':'compact_boundary','session_id':'compact-source'})
send({'type':'result','subtype':'success','is_error':False,'session_id':'compact-source','usage':{}})
for line in sys.stdin: pass
"#;
#[tokio::test]
async fn managed_claude_manual_compaction_gates_actual_transition_and_pairs_source_receipts()
-> Result<()> {
    let _lock = FIXTURE.lock().await;
    for deny in [false, true] {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("watched"),
            "managed compaction policy input",
        )?;
        let backend = root.path().join("source.py");
        std::fs::write(&backend, CLAUDE)?;
        std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o700))?;
        let mut config: Connection =
            serde_json::from_value(json!({"adapter":"claude","binary":backend}))?;
        config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
        let seen = Arc::new(Mutex::new(vec![]));
        config.access.non_tools = vec![
            registration(HookEvent::PreCompact, deny, seen.clone()),
            registration(HookEvent::PostCompact, false, seen.clone()),
        ];
        let (runtime, _) = SharedRuntime::open(root.path(), &config, None)?;
        let directory = runtime.directory()?;
        let mut session = adapters::builtins()?.open(&config, root.path())?;
        let (tx, mut rx) = mpsc::channel(256);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let events =
            EventSink::new("external-compaction".into(), tx, None)?.with_runtime(runtime.clone());
        let (_tx, mut commands) = mpsc::channel(4);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            session.compact(&mut commands, &events),
        )
        .await?;
        let record = runtime.record()?;
        session.close().await?;
        drop(session);
        drop(events);
        drop(runtime);
        drain.await?;
        std::fs::remove_dir_all(directory)?;
        assert_eq!(result.is_err(), deny, "{:?}", result.as_ref().err());
        if !deny {
            assert!(matches!(result, Ok(TurnEnd::Complete)));
        }
        assert_eq!(root.path().join("compacted").exists(), !deny);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), if deny { 1 } else { 2 });
        let id = seen[0]["subject"]["occurrence"]["compaction"]
            .as_u64()
            .unwrap();
        if !deny {
            assert_eq!(seen[1]["subject"]["occurrence"]["compaction"], id);
            let operation = record.operations.iter().find(|o| o.id == id).unwrap();
            assert!(operation.complete);
        }
    }
    Ok(())
}

struct Peer(std::process::Child);
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CLAUDE pinned installed backend; controlled local model"]
async fn installed_claude_manual_compaction_uses_managed_pre_and_post_handlers() -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let _lock = FIXTURE.lock().await;
    let binary = std::path::PathBuf::from(
        std::env::var_os("DEMONCODER_TEST_CLAUDE").context("installed Claude missing")?,
    )
    .canonicalize()?;
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for deny in [false, true] {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("watched"),
            "managed compaction policy input",
        )?;
        std::fs::create_dir(root.path().join("home"))?;
        let mut peer = Peer(
            std::process::Command::new("/usr/bin/python3")
                .arg(tests.join("plugin_claude_model.py"))
                .arg(root.path())
                .arg("manual")
                .stdout(std::process::Stdio::piped())
                .spawn()?,
        );
        let stdout =
            tokio::process::ChildStdout::from_std(peer.0.stdout.take().context("peer stdout")?)?;
        let mut port = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::io::BufReader::new(stdout)
                .take(4097)
                .read_line(&mut port),
        )
        .await??;
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
        config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
        let seen = Arc::new(Mutex::new(vec![]));
        config.access.non_tools = vec![
            registration(HookEvent::PreCompact, deny, seen.clone()),
            registration(HookEvent::PostCompact, false, seen.clone()),
        ];
        let (runtime, _) = SharedRuntime::open(root.path(), &config, None)?;
        let directory = runtime.directory()?;
        let mut session = adapters::builtins()?.open(&config, root.path())?;
        let (tx, mut rx) = mpsc::channel(1024);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let events = EventSink::new("installed-managed-compaction".into(), tx, None)?
            .with_runtime(runtime.clone());
        let (_tx, mut commands) = mpsc::channel(4);
        let result = tokio::time::timeout(std::time::Duration::from_secs(40), async {
            for i in 0..4 {
                let outcome = session
                    .turn(
                        format!("seed {i}: {}", "old context ".repeat(2000)),
                        &mut commands,
                        &events,
                    )
                    .await?;
                anyhow::ensure!(outcome == TurnEnd::Complete, "seed stopped");
            }
            session.compact(&mut commands, &events).await
        })
        .await;
        session.close().await?;
        let record = runtime.record()?;
        drop(session);
        drop(events);
        drop(runtime);
        drain.await?;
        std::fs::remove_dir_all(directory)?;
        let error = result.as_ref().err().map(|e| format!("{e:#}")).or_else(|| {
            result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref().err().map(|e| format!("{e:#}")))
        });
        if let Some(error) =
            error.filter(|e| !deny || !e.contains("managed policy denies compaction"))
        {
            let path = root.keep();
            anyhow::bail!(
                "installed deny={deny}: {error}; artifacts={}",
                path.display()
            );
        }
        let facts = seen.lock().unwrap();
        assert_eq!(facts.len(), if deny { 1 } else { 2 });
        let wire = std::fs::read_to_string(root.path().join("backend-wire.jsonl"))?;
        let rows: Vec<Value> = wire
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(
            rows.iter()
                .filter(|v| v["type"] == "system" && v["subtype"] == "compact_boundary")
                .count(),
            usize::from(!deny)
        );
        let requests = std::fs::read_to_string(root.path().join("model-requests.jsonl"))?;
        assert_eq!(requests.lines().count(), if deny { 4 } else { 5 });
        assert!(record.operations.iter().any(|o| matches!(
            o.host_invocation,
            Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(_))
        )));
        eprintln!(
            "installed Claude managed compaction deny={deny}: {} callbacks",
            facts.len()
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CODEX pinned installed backend; controlled local model"]
async fn installed_codex_manual_compaction_uses_managed_pre_and_post_handlers() -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let _lock = FIXTURE.lock().await;
    let binary = std::path::PathBuf::from(
        std::env::var_os("DEMONCODER_TEST_CODEX").context("installed Codex missing")?,
    )
    .canonicalize()?;
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for deny in [false, true] {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("watched"),
            "managed compaction policy input",
        )?;
        std::fs::create_dir(root.path().join("home"))?;
        anyhow::ensure!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .arg(root.path())
                .status()?
                .success(),
            "fixture Git init"
        );
        let mut peer = Peer(
            std::process::Command::new("/usr/bin/python3")
                .arg(tests.join("plugin_codex_model.py"))
                .arg(root.path())
                .arg("manual")
                .stdout(std::process::Stdio::piped())
                .spawn()?,
        );
        let stdout =
            tokio::process::ChildStdout::from_std(peer.0.stdout.take().context("peer stdout")?)?;
        let mut port = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::io::BufReader::new(stdout)
                .take(4097)
                .read_line(&mut port),
        )
        .await??;
        let ready: Value = serde_json::from_str(&port)?;
        let port = ready["port"].as_u64().context("peer port")?;
        std::fs::write(
            root.path().join("backend.json"),
            serde_json::to_vec(
                &json!({"binary":binary,"endpoint":format!("http://127.0.0.1:{port}"),"ca":ready["ca"]}),
            )?,
        )?;
        let mut config: Connection = serde_json::from_value(
            json!({"adapter":"codex","model":"gpt-5.4","binary":tests.join("plugin_codex_launcher.py")}),
        )?;
        config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
        let seen = Arc::new(Mutex::new(vec![]));
        config.access.non_tools = vec![
            registration(HookEvent::PreCompact, deny, seen.clone()),
            registration(HookEvent::PostCompact, false, seen.clone()),
        ];
        let (runtime, _) = SharedRuntime::open(root.path(), &config, None)?;
        let directory = runtime.directory()?;
        let mut session = adapters::builtins()?.open(&config, root.path())?;
        let (tx, mut rx) = mpsc::channel(1024);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let events = EventSink::new("installed-managed-compaction".into(), tx, None)?
            .with_runtime(runtime.clone());
        let (_tx, mut commands) = mpsc::channel(4);
        let result = tokio::time::timeout(std::time::Duration::from_secs(40), async {
            for i in 0..4 {
                let outcome = session
                    .turn(
                        format!("seed {i}: {}", "old context ".repeat(2000)),
                        &mut commands,
                        &events,
                    )
                    .await?;
                anyhow::ensure!(outcome == TurnEnd::Complete, "seed stopped");
            }
            session.compact(&mut commands, &events).await
        })
        .await;
        session.close().await?;
        let record = runtime.record()?;
        drop(session);
        drop(events);
        drop(runtime);
        drain.await?;
        std::fs::remove_dir_all(directory)?;
        let error = result.as_ref().err().map(|e| format!("{e:#}")).or_else(|| {
            result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref().err().map(|e| format!("{e:#}")))
        });
        if let Some(error) =
            error.filter(|e| !deny || !e.contains("managed policy denies compaction"))
        {
            let path = root.keep();
            anyhow::bail!(
                "installed deny={deny}: {error}; artifacts={}",
                path.display()
            );
        }
        let facts = seen.lock().unwrap();
        assert_eq!(facts.len(), if deny { 1 } else { 2 });
        let wire = std::fs::read_to_string(root.path().join("backend-wire.jsonl"))?;
        let rows: Vec<Value> = wire
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        assert!(rows.iter().any(|v| v["method"] == "turn/completed"));
        let requests = std::fs::read_to_string(root.path().join("model-requests.jsonl"))?;
        let model_rows: Vec<Value> = requests
            .lines()
            .map(serde_json::from_str)
            .collect::<std::result::Result<_, _>>()?;
        assert_eq!(
            model_rows.iter().filter(|v| v["compact"] == true).count(),
            usize::from(!deny)
        );
        assert!(record.operations.iter().any(|o| matches!(
            o.host_invocation,
            Some(demoncoder::workflow::runtime::HostInvocation::Lifecycle(_))
        )));
        eprintln!(
            "installed Codex managed compaction deny={deny}: {} callbacks",
            facts.len()
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires DEMONCODER_TEST_CLAUDE pinned installed backend; controlled local model"]
async fn installed_claude_batch_callback_follows_actual_settled_members() -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let _lock = FIXTURE.lock().await;
    let binary = std::path::PathBuf::from(
        std::env::var_os("DEMONCODER_TEST_CLAUDE").context("installed Claude missing")?,
    )
    .canonicalize()?;
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    for deny in [false] {
        let root = tempfile::tempdir()?;
        std::fs::write(
            root.path().join("watched"),
            "managed compaction policy input",
        )?;
        std::fs::create_dir(root.path().join("home"))?;
        let mut peer = Peer(
            std::process::Command::new("/usr/bin/python3")
                .arg(tests.join("plugin_batch_model.py"))
                .arg(root.path())
                .arg("manual")
                .stdout(std::process::Stdio::piped())
                .spawn()?,
        );
        let stdout =
            tokio::process::ChildStdout::from_std(peer.0.stdout.take().context("peer stdout")?)?;
        let mut port = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            tokio::io::BufReader::new(stdout)
                .take(4097)
                .read_line(&mut port),
        )
        .await??;
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
        config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
        let seen = Arc::new(Mutex::new(vec![]));
        config.access.non_tools = vec![registration(HookEvent::PostToolBatch, false, seen.clone())];
        let (runtime, _) = SharedRuntime::open(root.path(), &config, None)?;
        let directory = runtime.directory()?;
        runtime.begin_phase("worker", None)?;
        let mut session = adapters::builtins()?.open(&config, root.path())?;
        let (tx, mut rx) = mpsc::channel(1024);
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let events = EventSink::new("installed-managed-compaction".into(), tx, None)?
            .with_runtime(runtime.clone());
        let (_tx, mut commands) = mpsc::channel(4);
        let result = tokio::time::timeout(std::time::Duration::from_secs(40), async {
            session
                .turn(
                    "Execute the two source tools then finish.".into(),
                    &mut commands,
                    &events,
                )
                .await
        })
        .await;
        session.close().await?;
        let record = runtime.record()?;
        drop(session);
        drop(events);
        drop(runtime);
        drain.await?;
        std::fs::remove_dir_all(directory)?;
        let error = result.as_ref().err().map(|e| format!("{e:#}")).or_else(|| {
            result
                .as_ref()
                .ok()
                .and_then(|r| r.as_ref().err().map(|e| format!("{e:#}")))
        });
        if let Some(error) =
            error.filter(|e| !deny || !e.contains("managed policy denies compaction"))
        {
            let path = root.keep();
            anyhow::bail!(
                "installed deny={deny}: {error}; artifacts={}",
                path.display()
            );
        }
        let facts = seen.lock().unwrap();
        assert_eq!(facts.len(), 1, "actual SDK batch callback missing");
        let members = facts[0]["subject"]["occurrence"]["tool_calls"]
            .as_array()
            .context("batch members")?;
        assert_eq!(members.len(), 2);
        assert!(facts[0]["source"].is_object());
        assert_eq!(
            std::fs::read_to_string(root.path().join("proof.txt"))?,
            "external mixed effect\n"
        );
        assert_eq!(
            record
                .operations
                .iter()
                .filter_map(|o| o.tool_receipt.as_ref())
                .filter(|r| r.original_call.name == "write")
                .count(),
            2
        );
        eprintln!("installed Claude actual SDK batch: two settled equal members, one callback");
    }
    Ok(())
}

struct HostBatchObserver(Arc<Mutex<Vec<Value>>>);
#[async_trait::async_trait]
impl HookRunner for HostBatchObserver {
    fn side_effect_free(&self) -> bool {
        true
    }
    async fn run(&self, i: &HookInvocation) -> Result<RawOutcome> {
        self.0.lock().unwrap().push(serde_json::to_value(
            i.lifecycle.as_ref().context("batch facts")?,
        )?);
        Ok(RawOutcome::Callback { value: json!({}) })
    }
}
async fn shared_wrapper_case(adapter: &str, binary: Option<std::path::PathBuf>) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    let root = tempfile::tempdir()?;
    std::fs::write(root.path().join("watched"), "fixed public policy")?;
    std::fs::create_dir(root.path().join("home"))?;
    anyhow::ensure!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(root.path())
            .status()?
            .success(),
        "fixture Git init"
    );
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut peer = Peer(
        std::process::Command::new("/usr/bin/python3")
            .arg(tests.join("plugin_batch_model.py"))
            .arg(root.path())
            .arg(adapter)
            .arg("wrapper")
            .stdout(std::process::Stdio::piped())
            .spawn()?,
    );
    let stdout =
        tokio::process::ChildStdout::from_std(peer.0.stdout.take().context("peer stdout")?)?;
    let mut ready = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::io::BufReader::new(stdout)
            .take(4097)
            .read_line(&mut ready),
    )
    .await??;
    let ready: Value = serde_json::from_str(&ready)?;
    let endpoint = format!("http://127.0.0.1:{}", ready["port"]);
    let mut config: Connection = if let Some(binary) = binary {
        std::fs::write(
            root.path().join("backend.json"),
            serde_json::to_vec(&json!({"binary":binary,"endpoint":endpoint,"ca":ready["ca"]}))?,
        )?;
        serde_json::from_value(
            json!({"adapter":adapter,"model":if adapter=="claude"{"claude-sonnet-4-6"}else{"gpt-5.4"},"binary":tests.join(format!("plugin_{adapter}_launcher.py"))}),
        )?
    } else {
        serde_json::from_value(
            json!({"adapter":adapter,"endpoint":format!("{endpoint}/{}",if adapter=="openai-api"{"responses"}else{"messages"}),"api_key":"synthetic-only","model":"same-selected-model","max_output_tokens":1024}),
        )?
    };
    let seen = Arc::new(Mutex::new(vec![]));
    let p = plan_for(
        HookEvent::PostToolBatch,
        Arc::new(HostBatchObserver(seen.clone())),
    );
    config.access.non_tools = vec![p];
    config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    let (runtime, _) = SharedRuntime::open(root.path(), &config, None)?;
    runtime.begin_phase("worker", None)?;
    let directory = runtime.directory()?;
    let mut session = adapters::builtins()?.open(&config, root.path())?;
    let (tx, mut rx) = mpsc::channel(512);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let events = EventSink::new("shared-wrapper".into(), tx, None)?.with_runtime(runtime.clone());
    let (_tx, mut commands) = mpsc::channel(4);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(40),
        session.turn(
            "Execute the explicit host tool batch.".into(),
            &mut commands,
            &events,
        ),
    )
    .await;
    session.close().await?;
    let record = runtime.record()?;
    drop(session);
    drop(events);
    drop(runtime);
    drain.await?;
    std::fs::remove_dir_all(directory)?;
    if let Err(error) = result.map_err(anyhow::Error::from).and_then(|v| v) {
        let path = root.keep();
        anyhow::bail!("{adapter}: {error:#}; artifacts={}", path.display());
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("proof.txt"))?,
        "settled host batch"
    );
    let host = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|v| v["subject"]["occurrence"]["batch"].is_u64())
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(host.len(), 1, "{adapter}: one real host batch");
    assert_eq!(
        host[0]["subject"]["occurrence"]["tool_calls"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(host[0]["provenance"], "explicit_host_operation_v1");
    let tool_ops = record
        .operations
        .iter()
        .filter(|o| o.tool_receipt.is_some())
        .collect::<Vec<_>>();
    assert_eq!(tool_ops.len(), 4);
    assert!(
        tool_ops
            .iter()
            .all(|o| o.complete && o.result.as_ref().is_some_and(|r| r.success))
    );
    eprintln!("{adapter}: actual shared wrapper and three settled members");
    Ok(())
}
#[tokio::test]
async fn public_batch_on_both_native_api_adapters() -> Result<()> {
    let _lock = FIXTURE.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        shared_wrapper_case(adapter, None).await?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "requires pinned installed Claude/Codex, controlled local model peers"]
async fn public_batch_on_both_installed_external_adapters() -> Result<()> {
    let _lock = FIXTURE.lock().await;
    for (adapter, key) in [
        ("claude", "DEMONCODER_TEST_CLAUDE"),
        ("codex", "DEMONCODER_TEST_CODEX"),
    ] {
        shared_wrapper_case(
            adapter,
            Some(
                std::path::PathBuf::from(
                    std::env::var_os(key).context("installed backend missing")?,
                )
                .canonicalize()?,
            ),
        )
        .await?;
    }
    Ok(())
}

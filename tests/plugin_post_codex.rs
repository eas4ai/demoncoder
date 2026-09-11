use demoncoder::{
    config::Connection,
    events::EventSink,
    plugins::{
        dispatch::*,
        gate_snapshot::GateReadSet,
        hook_types::{HandlerKind, HookDialect, HookEvent},
        lifecycle::PostToolPlan,
        receipts::ToolRepresentation,
    },
    workflow::runtime::SharedRuntime,
};
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;
struct Capture {
    facts: Arc<Mutex<Vec<ToolRepresentation>>>,
    hold: bool,
    cancel: Option<Arc<tokio::sync::Notify>>,
    replace: bool,
}
#[async_trait::async_trait]
impl HookRunner for Capture {
    async fn run(&self, input: &HookInvocation) -> anyhow::Result<RawOutcome> {
        self.facts.lock().unwrap().push(
            input
                .completed
                .as_ref()
                .unwrap()
                .facts
                .representation
                .clone(),
        );
        if let Some(ready) = &self.cancel {
            ready.notify_one();
            std::future::pending::<()>().await;
        }
        Ok(RawOutcome::Callback {
            value: if self.hold {
                json!({"continue":false,"stopReason":"inspect write"})
            } else if self.replace {
                json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedToolOutput":"model replacement"}})
            } else {
                json!({})
            },
        })
    }
}
static FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[derive(Clone, Copy)]
enum Case {
    Success,
    Held,
    Failed,
    Cancelled,
    CommandFrame,
    Replacement,
}
async fn run(case: Case) {
    let hold = matches!(case, Case::Held);
    let failed = matches!(case, Case::Failed);
    let cancelled = matches!(case, Case::Cancelled);
    let ready = Arc::new(tokio::sync::Notify::new());
    let _fixture = FIXTURE.lock().await;
    let root = tempfile::tempdir().unwrap();
    let binary = root.path().join("backend.py");
    let script = r#"#!/usr/bin/python3
import json,sys,pathlib
root=pathlib.Path.cwd()
def send(v): print(json.dumps(v),flush=True)
for line in sys.stdin:
 r=json.loads(line); m=r.get('method')
 if m=='initialize': v={}
 elif m=='initialized': continue
 elif m=='config/read': v={'config':{}}
 elif m=='account/read': v={'account':{'type':'chatgpt'},'requiresOpenaiAuth':True}
 elif m=='thread/start': v={'thread':{'id':'actual-thread','path':'/actual/transcript.jsonl'},'model':'actual-model'}
 elif m=='turn/start':
  send({'id':r['id'],'result':{'turn':{'id':'actual-turn'}}})
  send({'method':'turn/started','params':{'threadId':'actual-thread','turn':{'id':'actual-turn'}}})
  send({'id':'rpc-call','method':'item/tool/call','params':{'threadId':'actual-thread','turnId':'actual-turn','callId':'actual-call','tool':'write','arguments':{'path':'created','content':'written'}}})
  reply=json.loads(sys.stdin.readline())
  result=json.loads(reply['result']['contentItems'][0]['text'])
  assert result['success'] and result['output']=='Wrote 7 bytes to created',result
  (root/'delivered').write_text('yes')
  send({'method':'turn/completed','params':{'threadId':'actual-thread','turn':{'id':'actual-turn','status':'completed'}}})
  continue
 else: raise AssertionError(r)
 send({'id':r['id'],'result':v})
"#;
    let script = if matches!(case, Case::Replacement) {
        script.replace(
            "assert result['success'] and result['output']=='Wrote 7 bytes to created',result",
            "(root/'model-response.json').write_text(json.dumps(result))",
        )
    } else if failed {
        script
            .replace(
                "'tool':'write','arguments':{'path':'created','content':'written'}",
                "'tool':'read','arguments':{'path':'missing'}",
            )
            .replace(
                "assert result['success'] and result['output']=='Wrote 7 bytes to created',result",
                "assert not result['success'],result",
            )
    } else {
        script.to_owned()
    };
    std::fs::write(&binary, script).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let facts = Arc::new(Mutex::new(Vec::new()));
    let mut registration = Registration {
        declaration: Declaration {
            identity: DeclarationIdentity {
                package: "post".into(),
                code: "code".into(),
                policy: "policy".into(),
                configuration: "config".into(),
                generation: "one".into(),
                scope: Scope::Project,
                role: "worker".into(),
                declaration: "post".into(),
                index: 0,
                dialect: HookDialect::Codex,
                runner: HandlerKind::Command,
            },
            class: HandlerClass::Combined,
            priority: 0,
            matcher: Matcher::default(),
            reads: GateReadSet::default(),
            concurrent_group: None,
            read_only_endpoint: None,
            external_precondition: None,
        },
        runner: Arc::new(Capture {
            facts: facts.clone(),
            hold,
            cancel: cancelled.then(|| ready.clone()),
            replace: matches!(case, Case::Replacement),
        }),
        revalidation: None,
    };
    if matches!(case, Case::Replacement) {
        registration.declaration.identity.dialect = HookDialect::Native;
    }
    if matches!(case, Case::CommandFrame) {
        use demoncoder::plugins::{
            self,
            runners::{CommandConfig, CommandProgram, CommandRunner},
        };
        let package_root = tempfile::tempdir().unwrap();
        std::fs::create_dir(package_root.path().join(".codex-plugin")).unwrap();
        std::fs::write(
            package_root.path().join(".codex-plugin/plugin.json"),
            r#"{"name":"source-frame","version":"1.0.0"}"#,
        )
        .unwrap();
        std::fs::write(package_root.path().join("hook.py"),r#"import json,sys
x=json.load(sys.stdin)
assert x['hook_event_name']=='PostToolUse',x
assert x['tool_use_id']=='actual-call' and x['turn_id']=='actual-turn' and x['session_id']=='actual-thread',x
assert x['model']=='actual-model' and x['permission_mode']=='bypassPermissions',x
assert x['transcript_path']=='/actual/transcript.jsonl',x
assert x['tool_name']=='write' and x['tool_input']=={'path':'created','content':'written'},x
assert isinstance(x['tool_response'],str),x
r=json.loads(x['tool_response'])
assert r['success'] and r['output']=='Wrote 7 bytes to created' and r['call_id']=='actual-call',r
print('{}')
"#).unwrap();
        let package = Arc::new(
            plugins::inspect(package_root.path(), &plugins::ImportOptions::default()).unwrap(),
        );
        let mut config = CommandConfig::new(CommandProgram::Argv(vec![
            "/usr/bin/python3".into(),
            "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
        ]));
        config.model = Some("wrong-configured-model".into());
        config.transcript_path = Some("/wrong/configured-path".into());
        registration = CommandRunner::registration_for_event(
            package,
            registration.declaration,
            HookEvent::PostToolUse,
            config,
            None,
        )
        .unwrap();
    }
    let mut connection: Connection =
        serde_json::from_value(json!({"adapter":"codex","binary":binary})).unwrap();
    connection.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    connection.access.post_tools = vec![Arc::new(
        PostToolPlan::new(HookEvent::PostToolUse, vec![registration]).unwrap(),
    )];
    let (runtime, _) = SharedRuntime::open(root.path(), &connection, None).unwrap();
    let directory = runtime.directory().unwrap();
    let (tx, _rx) = mpsc::channel(256);
    let events = EventSink::new("codex".into(), tx, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let mut session = demoncoder::adapters::builtins()
        .unwrap()
        .open(&connection, root.path())
        .unwrap();
    let (command_tx, mut commands) = mpsc::channel(4);
    let cancel_tx = command_tx.clone();
    let cancellation = cancelled.then(|| {
        tokio::spawn(async move {
            ready.notified().await;
            cancel_tx
                .send(demoncoder::session::Command::Cancel)
                .await
                .unwrap();
        })
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        session.turn("write".into(), &mut commands, &events),
    )
    .await
    .unwrap();
    session.close().await.unwrap();
    if let Some(task) = cancellation {
        task.await.unwrap();
    }
    if cancelled {
        assert!(matches!(
            result,
            Ok(demoncoder::session::TurnEnd::Cancelled)
        ));
    } else {
        assert_eq!(result.is_err(), hold, "{:?}", result.err());
    }
    assert_eq!(root.path().join("delivered").exists(), !hold && !cancelled);
    if !failed {
        assert_eq!(
            std::fs::read_to_string(root.path().join("created")).unwrap(),
            "written"
        );
    }
    let observed = facts.lock().unwrap();
    assert_eq!(
        observed.len(),
        usize::from(!failed && !matches!(case, Case::CommandFrame))
    );
    let record = runtime.record().unwrap();
    let operation = record.operations.iter().find(|o| o.call.is_some()).unwrap();
    if !failed {
        let lifecycle = operation
            .tool_receipt
            .as_ref()
            .unwrap()
            .plugin_lifecycle
            .as_ref()
            .unwrap();
        if matches!(case, Case::CommandFrame) {
            assert!(matches!(
                lifecycle.hooks[0].outcome,
                Some(RawOutcome::Command {
                    exit_code: Some(0),
                    ..
                })
            ));
        }
        match &lifecycle.facts.representation {
            ToolRepresentation::CodexDynamic {
                tool_use_id,
                turn_id,
                session_id,
                model,
                permission_mode,
                transcript_path,
            } => {
                assert_eq!(tool_use_id, "actual-call");
                assert_eq!(turn_id, "actual-turn");
                assert_eq!(session_id, "actual-thread");
                assert_eq!(model.as_deref(), Some("actual-model"));
                assert_eq!(permission_mode, "bypassPermissions");
                assert_eq!(transcript_path.as_deref(), Some("/actual/transcript.jsonl"));
            }
            other => panic!("missing actual Codex identity: {other:?}"),
        }
    }
    assert_eq!(operation.result.as_ref().unwrap().success, !failed);
    if matches!(case, Case::Replacement) {
        assert_eq!(
            operation.result.as_ref().unwrap().output,
            "Wrote 7 bytes to created"
        );
        let delivered: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.path().join("model-response.json")).unwrap(),
        )
        .unwrap();
        let output = delivered["output"].as_str().unwrap();
        assert!(output.starts_with("model replacement\n"), "{output}");
        assert!(
            output.contains("[Plugin-origin post \"replace_model_output\"]"),
            "{output}"
        );
    }
    if cancelled {
        let lifecycle = operation
            .tool_receipt
            .as_ref()
            .unwrap()
            .plugin_lifecycle
            .as_ref()
            .unwrap();
        assert!(!lifecycle.settled);
        assert_eq!(lifecycle.hooks.len(), 1);
        assert!(lifecycle.hooks[0].outcome.is_none());
        assert!(lifecycle.hooks[0].uncertain_effects);
    }
    if !hold && !failed && !cancelled {
        assert!(matches!(
            operation
                .tool_receipt
                .as_ref()
                .unwrap()
                .plugin_lifecycle
                .as_ref()
                .unwrap()
                .delivery,
            demoncoder::plugins::receipts::PostDelivery::Acknowledged
        ));
    }
    drop(session);
    drop(events);
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
}
#[tokio::test]
async fn codex_completion_retains_actual_source_identity() {
    run(Case::Success).await;
}
#[tokio::test]
async fn codex_post_hold_keeps_write_but_prevents_delivery() {
    run(Case::Held).await;
}

#[tokio::test]
async fn codex_failure_does_not_fabricate_success_event() {
    run(Case::Failed).await;
}

#[tokio::test]
async fn codex_cancellation_keeps_original_and_unsettled_hook_without_delivery() {
    run(Case::Cancelled).await;
}

#[tokio::test]
async fn codex_actual_command_runner_receives_source_frame() {
    run(Case::CommandFrame).await;
}

#[tokio::test]
async fn codex_replacement_only_is_attributed_in_model_facing_response() {
    run(Case::Replacement).await;
}

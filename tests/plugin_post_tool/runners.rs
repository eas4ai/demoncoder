use super::*;
use demoncoder::plugins::{
    self,
    runners::{CommandConfig, CommandProgram, CommandRunner},
};
use demoncoder::tools::AccessPolicy;

pub(super) fn package(dialect: HookDialect, code: &str) -> Arc<plugins::Package> {
    let root = tempfile::tempdir().unwrap();
    let directory = if dialect == HookDialect::Codex {
        ".codex-plugin"
    } else {
        ".claude-plugin"
    };
    std::fs::create_dir(root.path().join(directory)).unwrap();
    std::fs::write(
        root.path().join(directory).join("plugin.json"),
        r#"{"name":"post-fixture","version":"1.0.0"}"#,
    )
    .unwrap();
    std::fs::write(root.path().join("hook.py"), code).unwrap();
    Arc::new(plugins::inspect(root.path(), &plugins::ImportOptions::default()).unwrap())
}
fn declaration(dialect: HookDialect, kind: HandlerKind) -> Declaration {
    let mut d = registration(
        "post",
        HandlerClass::Combined,
        runner(|_| output(json!({}))),
    )
    .declaration;
    d.identity.dialect = dialect;
    d.identity.runner = kind;
    d.concurrent_group = (dialect == HookDialect::Claude).then(|| "source-group".into());
    d
}
fn request(event: HookEvent) -> ToolCall {
    if event == HookEvent::PostToolUse {
        write("source", "created")
    } else {
        ToolCall {
            id: "source".into(),
            name: "read".into(),
            arguments: json!({"path":"missing"}),
        }
    }
}
fn executor(f: &Fixture, event: HookEvent, r: Registration) -> ToolExecutor {
    let mut tools = ToolExecutor::with_policy(
        f.root.path(),
        &AccessPolicy {
            supervisor: Some(env!("CARGO_BIN_EXE_demoncoder").into()),
            ..AccessPolicy::default()
        },
    )
    .unwrap();
    tools
        .register_post_tool_plan(Arc::new(PostToolPlan::new(event, vec![r]).unwrap()))
        .unwrap();
    tools
}
#[tokio::test]
async fn actual_command_runner_emits_all_applicable_post_source_frames() {
    let _lock = FIXTURE.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude, HookDialect::Codex] {
        for event in [HookEvent::PostToolUse, HookEvent::PostToolUseFailure] {
            if dialect == HookDialect::Codex && event == HookEvent::PostToolUseFailure {
                continue;
            }
            let f = Fixture::new();
            let code = format!(
                r#"import json,sys
x=json.load(sys.stdin)
assert x['hook_event_name']=={event:?}
assert x['tool_use_id']=='source'
assert x['tool_input']['path']=={path:?}
assert ('tool_response' in x)=={success}
assert ('error' in x)=={failure}
print(json.dumps({{'hookSpecificOutput':{{'hookEventName':{event:?},'additionalContext':'post command observed'}}}}))
"#,
                event = event.as_str(),
                path = if event == HookEvent::PostToolUse {
                    "created"
                } else {
                    "missing"
                },
                success = if event == HookEvent::PostToolUse {
                    "True"
                } else {
                    "False"
                },
                failure = if event == HookEvent::PostToolUseFailure {
                    "True"
                } else {
                    "False"
                }
            );
            let mut config = CommandConfig::new(CommandProgram::Argv(vec![
                "/usr/bin/python3".into(),
                "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
            ]));
            config.model = Some("fixture-model".into());
            let r = CommandRunner::registration_for_event(
                package(dialect, &code),
                declaration(dialect, HandlerKind::Command),
                event,
                config,
                None,
            )
            .unwrap();
            let (end, results, requests) =
                f.run(executor(&f, event, r), vec![request(event)]).await;
            assert!(
                end.is_ok(),
                "{dialect:?}/{event:?}: {}",
                end.err().map(|e| e.to_string()).unwrap_or_default()
            );
            assert_eq!(requests, 2);
            assert_eq!(results[0].success, event == HookEvent::PostToolUse);
            assert!(results[0].output.contains("post command observed"));
            let record = f.record();
            let lifecycle = record
                .operations
                .iter()
                .find_map(|o| o.tool_receipt.as_ref()?.plugin_lifecycle.as_ref())
                .unwrap();
            assert!(matches!(
                lifecycle.hooks[0].outcome,
                Some(RawOutcome::Command {
                    exit_code: Some(0),
                    ..
                })
            ));
        }
    }
}
#[test]
fn command_runner_event_identity_cannot_be_reused_as_a_different_event() {
    let dialect = HookDialect::Native;
    let r = CommandRunner::registration(
        package(dialect, "print('{}')"),
        declaration(dialect, HandlerKind::Command),
        CommandConfig::new(CommandProgram::Argv(vec!["/bin/true".into()])),
        None,
    )
    .unwrap();
    assert!(
        PostToolPlan::new(HookEvent::PostToolUse, vec![r]).is_err(),
        "PreToolUse runner was reinterpreted as PostToolUse"
    );
}

use demoncoder::plugins::runners::{
    HttpConfig, HttpRunner, McpBinding, McpConfig, McpRunner, ModelConfig, ModelRunner,
};
use demoncoder::plugins::services::{
    AdmittedTool, ManagedServices, ServiceConfig, ServiceIdentity, ServiceTransport,
};
use demoncoder::workflow::allocation::Limits;

struct Peer {
    endpoint: String,
    requests: Arc<Mutex<Vec<Value>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Peer {
    async fn new(
        handler: impl Fn(usize, &Value) -> (String, String) + Send + Sync + 'static,
    ) -> Self {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let exchange = async {
                    let mut bytes = Vec::new();
                    let mut buffer = [0u8; 4096];
                    let header = loop {
                        let n = stream.read(&mut buffer).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                        assert!(bytes.len() < 1024 * 1024);
                        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                    };
                    let headers = String::from_utf8_lossy(&bytes[..header]);
                    let get = headers.starts_with("GET ");
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (k, v) = line.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    while bytes.len() < header + length {
                        let n = stream.read(&mut buffer).await.unwrap();
                        if n == 0 {
                            return;
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    let value = if length == 0 {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes[header..header + length]).unwrap()
                    };
                    let index = {
                        let mut requests = captured.lock().unwrap();
                        let n = requests.len();
                        requests.push(value.clone());
                        n
                    };
                    let (status, content_type, body) = if get {
                        (405, "text/plain".into(), String::new())
                    } else if value["method"] == "notifications/initialized" {
                        (202, "text/plain".into(), String::new())
                    } else {
                        let (kind, body) = handler(index, &value);
                        (200, kind, body)
                    };
                    let response = format!(
                        "HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                };
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), exchange).await;
            }
        });
        Self {
            endpoint,
            requests,
            task,
        }
    }
    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
    fn connection(&self) -> Connection {
        serde_json::from_value(json!({"adapter":"openai-api","model":"post-hook-model","endpoint":self.endpoint,"api_key":"synthetic-hook-key","max_output_tokens":512})).unwrap()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn allowance(f: &Fixture) {
    f.runtime
        .allocate(
            Limits {
                seconds: 30,
                model_calls: 32,
                tool_calls: 64,
            },
            None,
        )
        .unwrap();
}
fn model_response(value: Value) -> (String, String) {
    let text = value
        .get("raw")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    let tool = value.get("tool");
    let mut events = Vec::new();
    if tool.is_none() {
        events.push(json!({"type":"response.output_text.delta","delta":text}));
    }
    let output=tool.map(|t|vec![json!({"type":"function_call","call_id":"inspect","name":t["name"],"arguments":t["arguments"].to_string()})]).unwrap_or_default();
    events.push(json!({"type":"response.completed","response":{"output":output,"usage":{"input_tokens":1,"output_tokens":1,"input_tokens_details":{"cached_tokens":0}}}}));
    (
        "text/event-stream".into(),
        events.iter().map(|v| format!("data: {v}\n\n")).collect(),
    )
}
#[tokio::test]
async fn actual_http_runner_dispatches_native_and_claude_success_and_failure() {
    let _lock = FIXTURE.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude] {
        for event in [HookEvent::PostToolUse, HookEvent::PostToolUseFailure] {
            let f = Fixture::new();
            allowance(&f);
            let peer=Peer::new(move |_,input| {
                assert_eq!(input["hook_event_name"],event.as_str());
                ("application/json".into(),json!({"hookSpecificOutput":{"hookEventName":event.as_str(),"additionalContext":"http post context"}}).to_string())
            }).await;
            let r = HttpRunner::registration_for_event(
                package(dialect, ""),
                declaration(dialect, HandlerKind::Http),
                event,
                HttpConfig::new(peer.endpoint.clone()),
                None,
            )
            .unwrap();
            let (end, results, _) = f.run(executor(&f, event, r), vec![request(event)]).await;
            assert!(
                end.is_ok(),
                "{dialect:?}/{event:?}: {} RECORD {:?}",
                end.err().map(|e| e.to_string()).unwrap_or_default(),
                serde_json::to_value(f.record()).unwrap()
            );
            assert_eq!(peer.count(), 1);
            assert!(results[0].output.contains("http post context"));
            assert_eq!(results[0].success, event == HookEvent::PostToolUse);
        }
    }
}
#[tokio::test]
async fn actual_prompt_and_agent_follow_source_post_continuation_rules() {
    let _lock = FIXTURE.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude] {
        for event in [HookEvent::PostToolUse, HookEvent::PostToolUseFailure] {
            for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
                for allow in [true, false] {
                    for continue_on_block in [false, true] {
                        if continue_on_block
                            && !(dialect == HookDialect::Claude && kind == HandlerKind::Prompt)
                        {
                            continue;
                        }
                        let f = Fixture::new();
                        allowance(&f);
                        if dialect == HookDialect::Claude {
                            let task = demoncoder::workflow::state::Task::new(
                                1,
                                "fixture task".into(),
                                vec![],
                                demoncoder::workflow::workspace::capture(f.root.path()).unwrap(),
                                2,
                            )
                            .unwrap();
                            f.runtime.save_task(&Some(task), 2, None).unwrap();
                        }
                        let peer = Peer::new(move |_, input| {
                            let framed = input["input"][0]["content"].as_str().unwrap();
                            assert!(framed.contains(event.as_str()));
                            model_response(json!({"ok":allow,"reason":"model post feedback"}))
                        })
                        .await;
                        let mut d = declaration(dialect, kind);
                        if dialect == HookDialect::Native {
                            d.class = HandlerClass::DecisionGate;
                        }
                        let mut config = ModelConfig::new(
                            peer.connection(),
                            "Evaluate completed result: $ARGUMENTS".into(),
                        );
                        config.continue_on_block = continue_on_block;
                        let r = ModelRunner::registration_for_event(
                            package(dialect, ""),
                            d,
                            event,
                            config,
                        )
                        .unwrap();
                        let (end, results, requests) =
                            f.run(executor(&f, event, r), vec![request(event)]).await;
                        let held = !allow
                            && (dialect == HookDialect::Native
                                || (event == HookEvent::PostToolUse
                                    && kind == HandlerKind::Prompt
                                    && !continue_on_block));
                        assert_eq!(
                            end.is_err(),
                            held,
                            "{dialect:?}/{event:?}/{kind:?} allow={allow} continue={continue_on_block}: {}",
                            end.err().map(|e| e.to_string()).unwrap_or_default()
                        );
                        assert_eq!(peer.count(), 1);
                        assert_eq!(requests, if held { 1 } else { 2 });
                        assert_eq!(results[0].success, event == HookEvent::PostToolUse);
                        if dialect == HookDialect::Native {
                            assert!(
                                f.record().task.is_none(),
                                "session model allowance cannot mint task correction authority"
                            );
                        } else {
                            assert_eq!(
                                f.record().task.as_ref().unwrap().corrections,
                                if !allow && !held { 1 } else { 0 },
                                "source continuation must consume its existing task correction"
                            );
                        }
                    }
                }
            }
        }
    }
}
#[tokio::test]
async fn actual_mcp_runner_dispatches_every_applicable_post_source_pair() {
    use std::os::unix::fs::MetadataExt;
    let _lock = FIXTURE.lock().await;
    for dialect in [HookDialect::Native, HookDialect::Claude, HookDialect::Codex] {
        for event in [HookEvent::PostToolUse, HookEvent::PostToolUseFailure] {
            if dialect == HookDialect::Codex && event == HookEvent::PostToolUseFailure {
                continue;
            }
            let f = Fixture::new();
            allowance(&f);
            let metadata =
                json!({"name":"post","inputSchema":{"type":"object","additionalProperties":true}});
            let metadata_peer = metadata.clone();
            let peer=Peer::new(move |_,input| {
                let result=match input["method"].as_str() {
                    Some("initialize")=>json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"post","version":"1"}}),
                    Some("tools/list")=>json!({"tools":[metadata_peer]}),
                    Some("tools/call")=>{
                        assert_eq!(input["params"]["arguments"]["event"],event.as_str());
                        json!({"content":[],"structuredContent":{"hookSpecificOutput":{"hookEventName":event.as_str(),"additionalContext":"mcp post context"}}})
                    },
                    _=>json!({}),
                };
                ("application/json".into(),json!({"jsonrpc":"2.0","id":input["id"],"result":result}).to_string())
            }).await;
            let package = package(dialect, "");
            let root = std::fs::metadata(f.root.path()).unwrap();
            let service = ManagedServices::default()
                .admit(
                    package.clone(),
                    ServiceConfig {
                        identity: ServiceIdentity {
                            workspace: (root.dev(), root.ino()),
                            role: "worker".into(),
                            generation: "generation-1".into(),
                            state: "state".into(),
                            credential_revision: "credentials".into(),
                        },
                        transport: ServiceTransport::Http(HttpConfig::new(peer.endpoint.clone())),
                        tools: vec![AdmittedTool {
                            metadata,
                            read_only: true,
                        }],
                        timeout_ms: 2000,
                        max_calls: 4,
                    },
                )
                .unwrap();
            let r = McpRunner::registration_for_event(
                package,
                declaration(dialect, HandlerKind::McpTool),
                event,
                McpBinding {
                    service: service.clone(),
                    tool: "post".into(),
                    input: json!({"event":"${hook_event_name}","input":"${tool_input}"}),
                },
                None,
                McpConfig {
                    model: Some("fixture-main-model".into()),
                    ..Default::default()
                },
            )
            .unwrap();
            let (end, results, _) = f.run(executor(&f, event, r), vec![request(event)]).await;
            assert!(
                end.is_ok(),
                "{dialect:?}/{event:?}: {} RECORD {:?}",
                end.err().map(|e| e.to_string()).unwrap_or_default(),
                serde_json::to_value(f.record()).unwrap()
            );
            assert!(results[0].output.contains("mcp post context"));
            assert_eq!(results[0].success, event == HookEvent::PostToolUse);
            assert_eq!(
                peer.requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| r["method"] == "tools/call")
                    .count(),
                1
            );
            service.stop().await.unwrap();
        }
    }
}

#[tokio::test]
async fn mutating_command_observer_does_not_turn_its_own_effect_into_a_gate() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    std::fs::create_dir(f.root.path().join("effects")).unwrap();
    let code = "import json,sys\njson.load(sys.stdin)\nopen('effects/observed','w').write('post effect')\nprint('{}')\n";
    let mut config = CommandConfig::new(CommandProgram::Argv(vec![
        "/usr/bin/python3".into(),
        "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
    ]));
    config.write_paths = vec!["effects".into()];
    let mut d = declaration(HookDialect::Native, HandlerKind::Command);
    d.class = HandlerClass::Observer;
    d.required_gate = false;
    let registration = CommandRunner::registration_for_event(
        package(HookDialect::Native, code),
        d,
        HookEvent::PostToolUse,
        config,
        None,
    )
    .unwrap();
    let (end, results, requests) = f
        .run(
            executor(&f, HookEvent::PostToolUse, registration),
            vec![write("one", "created")],
        )
        .await;
    assert!(end.is_ok(), "{:?}", end.err());
    assert!(results[0].success);
    assert_eq!(requests, 2);
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("effects/observed")).unwrap(),
        "post effect"
    );
}

#[tokio::test]
async fn post_agent_reads_completed_dirty_snapshot_and_changed_decision_holds() {
    let _lock = FIXTURE.lock().await;
    for changed in [false, true] {
        let f = Fixture::new();
        allowance(&f);
        let root = f.root.path().to_path_buf();
        std::fs::write(root.join("dirty"), "retained dirty evidence").unwrap();
        let peer = Peer::new(move |index, input| {
            if index == 0 {
                let tools = input["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v["name"].as_str().unwrap())
                    .collect::<Vec<_>>();
                assert_eq!(tools, ["snapshot_read", "snapshot_list", "snapshot_search"]);
                assert!(
                    root.join("created").exists(),
                    "agent inspection ran before actual tool completion"
                );
                if changed {
                    std::fs::write(root.join("dirty"), "changed live evidence").unwrap();
                }
                model_response(
                    json!({"tool":{"name":"snapshot_read","arguments":{"path":"dirty"}}}),
                )
            } else {
                assert!(input.to_string().contains("retained dirty evidence"));
                assert!(!input.to_string().contains("changed live evidence"));
                model_response(json!({"ok":true}))
            }
        })
        .await;
        let mut d = declaration(HookDialect::Native, HandlerKind::Agent);
        d.class = HandlerClass::DecisionGate;
        d.reads = GateReadSet::new(vec!["dirty".into()], vec![], vec![]).unwrap();
        let r = ModelRunner::registration_for_event(
            package(HookDialect::Native, ""),
            d,
            HookEvent::PostToolUse,
            ModelConfig::new(peer.connection(), "Inspect completed result".into()),
        )
        .unwrap();
        let (end, results, requests) = f
            .run(
                executor(&f, HookEvent::PostToolUse, r),
                vec![write("one", "created")],
            )
            .await;
        assert_eq!(end.is_err(), changed, "{:?}", end.err());
        assert!(results[0].success);
        assert_eq!(requests, if changed { 1 } else { 2 });
        assert_eq!(peer.count(), 2);
    }
}

#[tokio::test]
async fn post_command_timeout_holds_original_and_reaps_owned_descendant() {
    let _lock = FIXTURE.lock().await;
    let f = Fixture::new();
    std::fs::create_dir(f.root.path().join("effects")).unwrap();
    let code = r#"import json,sys,subprocess,time
json.load(sys.stdin)
subprocess.Popen(['/usr/bin/python3','-c',"import time\nwhile True:\n with open('effects/heartbeat','a') as f:f.write('x')\n time.sleep(.01)"],start_new_session=True)
time.sleep(30)
"#;
    let mut config = CommandConfig::new(CommandProgram::Argv(vec![
        "/usr/bin/python3".into(),
        "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
    ]));
    config.write_paths = vec!["effects".into()];
    config.timeout_ms = 1000;
    let r = CommandRunner::registration_for_event(
        package(HookDialect::Native, code),
        declaration(HookDialect::Native, HandlerKind::Command),
        HookEvent::PostToolUse,
        config,
        None,
    )
    .unwrap();
    let (end, _, requests) = f
        .run(
            executor(&f, HookEvent::PostToolUse, r),
            vec![write("one", "created")],
        )
        .await;
    assert!(end.is_err());
    assert_eq!(requests, 1);
    let record = f.record();
    let operation = record.operations.iter().find(|o| o.call.is_some()).unwrap();
    assert!(operation.result.as_ref().unwrap().success);
    assert!(
        operation
            .tool_receipt
            .as_ref()
            .unwrap()
            .plugin_lifecycle
            .as_ref()
            .unwrap()
            .hooks[0]
            .uncertain_effects
    );
    let heartbeat = f.root.path().join("effects/heartbeat");
    assert!(
        std::fs::metadata(&heartbeat).unwrap().len() > 0,
        "owned descendant never started"
    );
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let before = std::fs::metadata(&heartbeat).unwrap().len();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if std::fs::metadata(&heartbeat).unwrap().len() == before {
                break;
            }
        }
    })
    .await
    .expect("post hook descendant continued writing after owned timeout cleanup");
}

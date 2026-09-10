use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use demoncoder::{
    config::Connection,
    events::EventSink,
    native::{Model, NativeSession},
    session::Session,
    tools::{ToolCall, ToolExecutor, ToolResult},
    workflow::runtime::SharedRuntime,
};
use serde_json::json;
use tokio::sync::mpsc;

// Public session creation names directories with wall time and process ID.
// Keep independent session fixtures from colliding within one millisecond.
static SESSION_FIXTURE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Responses {
    calls: VecDeque<Vec<ToolCall>>,
    results: Arc<Mutex<Vec<ToolResult>>>,
}

#[async_trait::async_trait]
impl Model for Responses {
    fn prompt(&mut self, _: String) {}
    fn results(&mut self, results: Vec<ToolResult>) {
        self.results.lock().unwrap().extend(results);
    }
    async fn response(&mut self, _: &EventSink) -> anyhow::Result<Vec<ToolCall>> {
        Ok(self.calls.pop_front().unwrap_or_default())
    }
}

#[tokio::test]
async fn native_duplicate_source_call_reuses_original_result_without_repeating_effect() {
    let _fixture = SESSION_FIXTURE.lock().await;
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("counter"), "a").unwrap();
    let connection: Connection = serde_json::from_value(json!({"adapter":"openai-api"})).unwrap();
    let (runtime, _) = SharedRuntime::open(workspace.path(), &connection, None).unwrap();
    let directory = runtime.directory().unwrap();
    let (sender, _receiver) = mpsc::channel(64);
    let events = EventSink::new("fixture".into(), sender, None)
        .unwrap()
        .with_runtime(runtime.clone());
    let call = ToolCall {
        id: "source-1".into(),
        name: "edit".into(),
        arguments: json!({"path":"counter","old_text":"a","new_text":"ab"}),
    };
    let results = Arc::new(Mutex::new(Vec::new()));
    let model = Responses {
        calls: VecDeque::from([vec![call.clone(), call]]),
        results: results.clone(),
    };
    let mut native = NativeSession::with_tools(
        Box::new(model),
        ToolExecutor::new(workspace.path()).unwrap(),
    );
    let (_sender, mut commands) = mpsc::channel(4);
    native
        .turn("edit once".into(), &mut commands, &events)
        .await
        .unwrap();
    let contents = std::fs::read_to_string(workspace.path().join("counter")).unwrap();
    let operations = runtime.record().unwrap().operations;
    drop(native);
    drop(events);
    drop(runtime);
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(
        contents, "ab",
        "same invocation must not repeat a tool effect"
    );
    assert_eq!(
        operations
            .iter()
            .filter(|operation| operation.call.is_some())
            .count(),
        1
    );
    assert_eq!(results.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn external_duplicates_reuse_receipt_while_new_turns_may_reuse_source_ids() {
    let _fixture = SESSION_FIXTURE.lock().await;
    use std::os::unix::fs::PermissionsExt;
    for adapter in ["claude", "codex"] {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("counter"), "a").unwrap();
        let binary = workspace.path().join("backend.py");
        // The same controlled JSONL transports used by backend_fixture.py.
        std::fs::write(&binary, r#"#!/usr/bin/python3
import json, sys
codex = 'app-server' in sys.argv
turn = 0
def send(value): print(json.dumps(value), flush=True)
def tool():
    args={'path':'counter','old_text':'a','new_text':'ab'}
    if codex:
        send({'id':'request-1','method':'item/tool/call','params':{'threadId':'thread','turnId':str(turn),'callId':'source-1','tool':'edit','arguments':args}})
    else:
        send({'type':'control_request','request_id':'source-1','request':{'subtype':'mcp_message','server_name':'demoncoder','message':{'jsonrpc':'2.0','id':'source-1','method':'tools/call','params':{'name':'edit','arguments':args}}}})
    reply=json.loads(sys.stdin.readline())
    result=json.loads(reply['result']['contentItems'][0]['text'] if codex else reply['response']['response']['mcp_response']['result']['content'][0]['text'])
    assert result['success'], result
    return result
for line in sys.stdin:
    row=json.loads(line)
    method=row.get('method')
    if codex:
        if method=='initialize': value={}
        elif method=='initialized': continue
        elif method=='config/read': value={'config':{}}
        elif method=='account/read': value={'account':{'type':'chatgpt'},'requiresOpenaiAuth':True}
        elif method=='thread/start': value={'thread':{'id':'thread'}}
        elif method=='turn/start':
            turn+=1
            send({'id':row['id'],'result':{'turn':{'id':str(turn)}}})
            send({'method':'turn/started','params':{'threadId':'thread','turn':{'id':str(turn)}}})
            assert tool()==tool()
            send({'method':'turn/completed','params':{'threadId':'thread','turn':{'id':str(turn),'status':'completed'}}})
            continue
        else: raise AssertionError(row)
        send({'id':row['id'],'result':value})
    elif row.get('type')=='control_request':
        send({'type':'control_response','response':{'subtype':'success','request_id':row['request_id'],'response':{}}})
    elif row.get('type')=='user':
        turn+=1
        send({'type':'system','subtype':'init','session_id':'session','apiKeySource':'none'})
        assert tool()==tool()
        send({'type':'result','subtype':'success','is_error':False,'session_id':'session','usage':{'input_tokens':1,'output_tokens':1}})
"#).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let connection: Connection =
            serde_json::from_value(json!({"adapter":adapter,"binary":binary})).unwrap();
        let (runtime, _) = SharedRuntime::open(workspace.path(), &connection, None).unwrap();
        let directory = runtime.directory().unwrap();
        let (sender, _receiver) = mpsc::channel(128);
        let events = EventSink::new(adapter.into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let mut session = demoncoder::adapters::builtins()
            .unwrap()
            .open(&connection, workspace.path())
            .unwrap();
        let (_sender, mut commands) = mpsc::channel(4);
        for _ in 0..2 {
            let end = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                session.turn("edit".into(), &mut commands, &events),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(matches!(end, demoncoder::session::TurnEnd::Complete));
        }
        session.close().await.unwrap();
        let record = runtime.record().unwrap();
        let tools: Vec<_> = record
            .operations
            .iter()
            .filter(|operation| operation.call.is_some())
            .collect();
        assert_eq!(
            tools.len(),
            2,
            "{adapter}: duplicate tool notices allocated another operation"
        );
        let first = tools[0].tool_receipt.as_ref().unwrap();
        let second = tools[1].tool_receipt.as_ref().unwrap();
        assert_eq!(first.original_call.id, second.original_call.id);
        assert_ne!(first.invocation, second.invocation);
        assert!(first.observers_complete && second.observers_complete);
        assert_eq!(
            record.backend_invocations, 0,
            "ordinary backend turns do not debit delegation"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("counter")).unwrap(),
            "abb",
            "{adapter}"
        );
        drop(session);
        drop(events);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[tokio::test]
async fn native_api_responses_scope_real_source_ids_to_each_host_invocation() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _fixture = SESSION_FIXTURE.lock().await;
    for adapter in ["openai-api", "anthropic-api"] {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("counter"), "a").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/fixture", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for count in [2, 1, 0] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let size = stream.read(&mut buffer).await.unwrap();
                    assert!(size > 0 && request.len() < 1024 * 1024);
                    request.extend_from_slice(&buffer[..size]);
                    if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&request[..end]).unwrap();
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|value| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        if request.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let arguments =
                    json!({"path":"counter","old_text":"a","new_text":"ab"}).to_string();
                // Provider framing follows the existing tool_cycle_fixture.py.
                let frames = if adapter == "openai-api" {
                    vec![
                        json!({"type":"response.completed","response":{"output":(0..count).map(|_| json!({"type":"function_call","call_id":"source-1","name":"edit","arguments":arguments})).collect::<Vec<_>>()}}),
                    ]
                } else {
                    let mut frames = vec![
                        json!({"type":"message_start","message":{"usage":{"input_tokens":1}}}),
                    ];
                    for index in 0..count {
                        frames.push(json!({"type":"content_block_start","index":index,"content_block":{"type":"tool_use","id":"source-1","name":"edit","input":{}}}));
                        frames.push(json!({"type":"content_block_delta","index":index,"delta":{"type":"input_json_delta","partial_json":arguments}}));
                        frames.push(json!({"type":"content_block_stop","index":index}));
                    }
                    frames.push(json!({"type":"message_stop"}));
                    frames
                };
                let body = frames
                    .into_iter()
                    .map(|frame| format!("data: {frame}\n\n"))
                    .collect::<String>();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        let connection: Connection = serde_json::from_value(json!({"adapter":adapter,"endpoint":endpoint,"model":"fixture-model","max_output_tokens":32,"api_key":"synthetic-fixture-key"})).unwrap();
        let (runtime, _) = SharedRuntime::open(workspace.path(), &connection, None).unwrap();
        let directory = runtime.directory().unwrap();
        let (sender, _receiver) = mpsc::channel(128);
        let events = EventSink::new(adapter.into(), sender, None)
            .unwrap()
            .with_runtime(runtime.clone());
        let mut session = demoncoder::adapters::builtins()
            .unwrap()
            .open(&connection, workspace.path())
            .unwrap();
        let (_sender, mut commands) = mpsc::channel(4);
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            session.turn("edit twice".into(), &mut commands, &events),
        )
        .await
        .unwrap()
        .unwrap();
        server.await.unwrap();
        session.close().await.unwrap();
        let record = runtime.record().unwrap();
        let tools: Vec<_> = record
            .operations
            .iter()
            .filter_map(|operation| operation.tool_receipt.as_ref())
            .collect();
        assert_eq!(tools.len(), 2, "{adapter}");
        assert_eq!(tools[0].original_call.id, tools[1].original_call.id);
        assert_ne!(tools[0].invocation, tools[1].invocation);
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("counter")).unwrap(),
            "abb",
            "{adapter}"
        );
        drop(session);
        drop(events);
        drop(runtime);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

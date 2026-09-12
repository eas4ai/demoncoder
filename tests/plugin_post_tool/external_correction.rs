use super::*;
use demoncoder::{
    session::Command,
    workflow::{allocation::Limits, state::Task, workspace},
};
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};

struct PendingLifecycle {
    lock: Arc<tokio::sync::Mutex<()>>,
    entered: Arc<std::sync::atomic::AtomicBool>,
}
#[async_trait::async_trait]
impl demoncoder::plugins::bridge::Handler for PendingLifecycle {
    async fn handle(
        &self,
        _: &demoncoder::plugins::bridge::Invocation,
        _: &demoncoder::events::EventSink,
    ) -> anyhow::Result<demoncoder::plugins::bridge::Decision> {
        let _guard = self.lock.lock().await;
        self.entered.store(true, Ordering::SeqCst);
        std::future::pending().await
    }
}

const BACKEND: &str = r#"#!/usr/bin/python3
import json,sys,pathlib,uuid,os,time
root=pathlib.Path.cwd()
(root/'backend-pid').write_text(str(os.getpid()))
(root/'backend-start-stat').write_text(pathlib.Path('/proc/self/stat').read_text())
(root/'supervisor-start-stat').write_text(pathlib.Path(f'/proc/{os.getppid()}/stat').read_text())
case=json.loads((root/'case.json').read_text())
adapter,mode=case['adapter'],case['mode']
if '--demoncoder-compaction-capability' in sys.argv:
    print(json.dumps({'protocol':'demoncoder-compaction-v1','source_version':'0.153.4','patch_version':1}),flush=True);sys.exit(0)
def send(v): print(json.dumps(v,separators=(',',':'),ensure_ascii=False),flush=True)
def receive():
    line=sys.stdin.readline()
    if not line: sys.exit(0)
    value=json.loads(line)
    with (root/'wire.jsonl').open('a') as f:f.write(json.dumps(value)+'\n')
    return value
def flood():
    frame='{"id":-1,"result":{}}\n' if mode=='noisy-ack' else '{"type":"noise","method":"noise"}\n'
    while True:sys.stdout.write(frame*4096);sys.stdout.flush()
def wait_cancel():
    while True: receive()
def request(identity,body):
    send({'type':'control_request','request_id':identity,'request':body})
    reply=receive()
    assert reply['response']['subtype']=='success',reply
    return reply['response']['response']
if adapter=='codex':
    while True:
        r=receive();m=r.get('method')
        if m=='initialize':v={}
        elif m=='initialized':continue
        elif m=='account/read':v={'account':{'type':'chatgpt'},'requiresOpenaiAuth':True}
        elif m=='config/read':v={'config':{}}
        elif m in ('thread/start','thread/resume'):v={'thread':{'id':'actual-thread','path':'/actual/transcript.jsonl'},'model':'actual-model'}
        elif m=='turn/start':break
        else:raise AssertionError(r)
        send({'id':r['id'],'result':v})
    send({'id':r['id'],'result':{'turn':{'id':'original-turn'}}})
    send({'method':'turn/started','params':{'threadId':'actual-thread','turn':{'id':'original-turn'}}})
    send({'id':'pending-tool-rpc','method':'item/tool/call','params':{'threadId':'actual-thread','turnId':'original-turn','callId':'original-tool','tool':'write','arguments':{'path':'created','content':'written'}}})
    interrupt=receive()
    assert interrupt.get('method')=='turn/interrupt',interrupt
    assert interrupt['params']=={'threadId':'actual-thread','turnId':'original-turn'}
    (root/'interrupt-seen').write_text('yes')
    if mode=='blocked-context':
        while not (root/'queue-filled').exists():time.sleep(.001)
    if mode.startswith('delayed-response'):
        time.sleep(29)
        identity='i'*524288
        if '-relay-' in mode:
            import socket
            requirement=json.loads(os.environ['CODEX_DEMONCODER_COMPACTION_RELAY'])
            address=json.loads(pathlib.Path(requirement['source_path']).with_name('address.json').read_text())
            relay=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);relay.connect(address['socket'])
            if mode.endswith('-worker'):
                value={'token':address['token'],'input':{'session_id':'actual-thread','turn_id':'original-turn','hook_event_name':'PreCompact','trigger':'auto','demonCoderCompaction':{'protocol':'demoncoder-compaction-v1','challenge':'fresh'}}}
                relay.sendall(json.dumps(value).encode());relay.shutdown(socket.SHUT_WR)
            (root/'blocking-response-started').write_text('yes')
            while True:time.sleep(1)
        if adapter=='codex':
            frame={'id':identity,'method':'item/tool/call','params':{'threadId':'actual-thread','turnId':'original-turn','callId':'refused-tool','tool':'read','arguments':{'path':'created'}}} if mode.endswith('-tool') else {'id':identity,'method':'unknown-probe'}
        else:
            method='tools/call' if mode.endswith('-tool') else 'ping'
            frame={'type':'control_request','request_id':identity,'request':{'subtype':'mcp_message','server_name':'demoncoder','message':{'jsonrpc':'2.0','id':99,'method':method,'params':{'name':'read','arguments':{'path':'created'}}}}}
        send(frame)
        (root/'blocking-response-started').write_text('yes')
        while True:time.sleep(1)
    if case.get('steering'):
        while not (root/'steering-queued').exists():time.sleep(.001)
    if mode=='noisy-interrupt':flood()
    if mode=='cancel-before-release':wait_cancel()
    if mode=='stale':(root/'watched').write_text('changed')
    ack={'id':interrupt['id'],'result':{}}
    if mode=='missing-interrupt-result':del ack['result']
    terminal={'method':'turn/completed','params':{'threadId':'actual-thread','turn':{'id':'wrong-turn' if mode=='forged-terminal' else 'original-turn','status':'interrupted'}}}
    if mode=='duplicate-interrupt-ack':send(ack);send(ack)
    elif mode=='duplicate-terminal':send(terminal);send(terminal)
    elif mode=='terminal-first':send(terminal);send(ack)
    else:send(ack);send(terminal)
    if mode=='blocked-write':
        while True:time.sleep(1)
    next_turn=receive()
    assert next_turn.get('method')=='turn/start',next_turn
    (root/'correction-request.json').write_text(json.dumps(next_turn))
    def user_notifications():
        if case.get('steering'):
            notifications=[('item/started','startedAtMs'),('item/completed','completedAtMs')]
            for method,clock in notifications*(33 if case.get('user_fault')=='user-flood' else 1):
                event={'method':method,'params':{'item':{'type':'userMessage','id':str(uuid.uuid4()),'clientId':None,'content':[dict(next_turn['params']['input'][0],text_elements=[])]},'threadId':'actual-thread','turnId':'correction-turn',clock:1789078667247},'emittedAtMs':1789078667247}
                fault=case.get('user_fault')
                if fault=='wrong-thread':event['params']['threadId']='wrong-thread'
                if fault=='wrong-turn':event['params']['turnId']='wrong-turn'
                if fault=='missing-thread':del event['params']['threadId']
                if fault=='missing-turn':del event['params']['turnId']
                if fault=='malformed-thread':event['params']['threadId']=42
                if fault=='malformed-turn':event['params']['turnId']={}
                if fault=='request-id':event['id']='source-request'
                if fault=='unknown-method':event['method']='future/notification'
                if fault=='other-item':event['params']['item']['type']='futureItem'
                if fault=='oversized-turn':event['params']['turnId']='x'*(1100*1024)
                send(event)
    if case.get('user_before_ack'):user_notifications()
    (root/'next-turn-seen').write_text('yes')
    if mode in ('cancel-after-reserve','silent-ack'):wait_cancel()
    if mode=='noisy-ack':flood()
    next_ack={'id':next_turn['id']+1 if mode=='forged-ack' else next_turn['id'],'result':{'turn':{'id':'original-turn' if mode=='stale-ack' else 'correction-turn'}}}
    started={'method':'turn/started','params':{'threadId':'actual-thread','turn':{'id':'correction-turn'}}}
    if mode.startswith('started-first'):
        send(started)
        if mode=='started-first-duplicate':send(started)
        if mode=='started-first-mismatch':next_ack['result']['turn']['id']='different-turn'
        if mode=='started-first-missing':sys.exit(0)
    if mode.startswith('early-tool'):
        send(started)
        tool={'id':'early-read','method':'item/tool/call','params':{'threadId':'actual-thread','turnId':'wrong-turn' if mode=='early-tool-mismatch' else 'correction-turn','callId':'early-read','tool':'read','arguments':{'path':'created'}}}
        for _ in range(65 if mode=='early-tool-overflow' else 1):send(tool)
        if mode=='early-tool':
            import select
            assert not select.select([sys.stdin],[],[],0.1)[0], 'tool executed before acknowledgment'
    if mode=='early-completed':
        send(started)
        send({'method':'turn/completed','params':{'threadId':'actual-thread','turn':{'id':'correction-turn','status':'completed'}}})
    send(next_ack)
    if mode=='early-completed':wait_cancel()
    if not case.get('user_before_ack'):user_notifications()
    if mode=='ack-clears-deadline':time.sleep(31)
    if mode=='early-tool':
        reply=receive()
        assert reply['id']=='early-read' and reply['result']['success'],reply
    if mode=='duplicate-new-ack':send(next_ack)
    if mode=='forged-ack':
        send({'method':'turn/started','params':{'threadId':'actual-thread','turn':{'id':'unrelated-turn'}}})
        wait_cancel()
    if not mode.startswith(('started-first','early-tool')):send(started)
    if mode=='oracle-intent':
        send({'id':'reviewed-bash','method':'item/tool/call','params':{'threadId':'actual-thread','turnId':'correction-turn','callId':'reviewed-bash','tool':'bash','arguments':{'command':'true'}}})
        reviewed=receive()
        assert not json.loads(reviewed['result']['contentItems'][0]['text'])['success']
    send({'method':'turn/completed','params':{'threadId':'actual-thread','turn':{'id':'correction-turn','status':'completed'}}})
else:
    init=receive();hooks=init['request']['hooks']
    send({'type':'control_response','response':{'request_id':'initialize','subtype':'success'}})
    receive()
    send({'type':'system','subtype':'init','session_id':'actual-session','apiKeySource':'none'})
    args={'path':'created','content':'written'}
    base={'hook_event_name':'PreToolUse','session_id':'actual-session','transcript_path':str(root/'actual-transcript.jsonl'),'cwd':str(root),'permission_mode':'default','tool_name':'mcp__demoncoder__write','tool_input':args,'tool_use_id':'original-tool'}
    def callback(identity,event,value):
        return request(identity,{'subtype':'hook_callback','callback_id':hooks[event][0]['hookCallbackIds'][0],'tool_use_id':value['tool_use_id'],'input':value})
    callback('pre','PreToolUse',base)
    request('permission',{'subtype':'can_use_tool','tool_name':base['tool_name'],'tool_use_id':'original-tool','input':args})
    rpc={'jsonrpc':'2.0','id':2,'method':'tools/call','params':{'name':'write','arguments':args,'_meta':{'claudecode/toolUseId':'original-tool'}}}
    result=request('tool',{'subtype':'mcp_message','server_name':'demoncoder','message':rpc})['mcp_response']['result']
    original=json.loads(result['content'][0]['text'])
    assert original['success'] and original['output']=='Wrote 7 bytes to created'
    post=dict(base,hook_event_name='PostToolUse',tool_response=result['content'])
    send({'type':'control_request','request_id':'pending-post','request':{'subtype':'hook_callback','callback_id':hooks['PostToolUse'][0]['hookCallbackIds'][0],'tool_use_id':'original-tool','input':post}})
    interrupt=receive()
    assert interrupt.get('type')=='control_request' and interrupt['request']['subtype']=='interrupt',interrupt
    (root/'interrupt-seen').write_text('yes')
    if mode=='blocked-context':
        while not (root/'queue-filled').exists():time.sleep(.001)
    if mode.startswith('delayed-response'):
        time.sleep(29)
        identity='i'*524288
        if '-relay-' in mode:
            import socket
            requirement=json.loads(os.environ['CODEX_DEMONCODER_COMPACTION_RELAY'])
            address=json.loads(pathlib.Path(requirement['source_path']).with_name('address.json').read_text())
            relay=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);relay.connect(address['socket'])
            if mode.endswith('-worker'):
                value={'token':address['token'],'input':{'session_id':'actual-thread','turn_id':'original-turn','hook_event_name':'PreCompact','trigger':'auto','demonCoderCompaction':{'protocol':'demoncoder-compaction-v1','challenge':'fresh'}}}
                relay.sendall(json.dumps(value).encode());relay.shutdown(socket.SHUT_WR)
            (root/'blocking-response-started').write_text('yes')
            while True:time.sleep(1)
        if adapter=='codex':
            frame={'id':identity,'method':'item/tool/call','params':{'threadId':'actual-thread','turnId':'original-turn','callId':'refused-tool','tool':'read','arguments':{'path':'created'}}} if mode.endswith('-tool') else {'id':identity,'method':'unknown-probe'}
        else:
            method='tools/call' if mode.endswith('-tool') else 'ping'
            frame={'type':'control_request','request_id':identity,'request':{'subtype':'mcp_message','server_name':'demoncoder','message':{'jsonrpc':'2.0','id':99,'method':method,'params':{'name':'read','arguments':{'path':'created'}}}}}
        send(frame)
        (root/'blocking-response-started').write_text('yes')
        while True:time.sleep(1)
    if case.get('steering'):
        while not (root/'steering-queued').exists():time.sleep(.001)
    if mode=='noisy-interrupt':flood()
    if mode=='cancel-before-release':wait_cancel()
    request('cancelled-notice',{'subtype':'mcp_message','server_name':'demoncoder','message':{'jsonrpc':'2.0','method':'notifications/cancelled','params':{'requestId':2,'reason':'interrupted'}}})
    send({'type':'control_cancel_request','request_id':'wrong-post' if mode=='forged-cancel' else 'pending-post'})
    if mode=='stale':(root/'watched').write_text('changed')
    ack={'type':'control_response','response':{'request_id':interrupt['request_id'],'subtype':'success'}}
    terminal={'type':'result','session_id':'wrong-session' if mode=='forged-terminal' else 'actual-session','is_error':True,'subtype':'error_during_execution','terminal_reason':'aborted_tools','usage':{}}
    if mode=='duplicate-interrupt-ack':send(ack);send(ack)
    elif mode=='duplicate-terminal':send(terminal);send(terminal)
    elif mode=='terminal-first':send(terminal);send(ack)
    else:send(ack);send(terminal)
    if mode=='blocked-write':
        while True:time.sleep(1)
    next_turn=receive()
    assert next_turn.get('type')=='user',next_turn
    if case.get('replacement') is not None:
        assert next_turn['message']['content'][1:]==case['replacement'],next_turn
        assert next_turn['message']['content'][0]['type']=='text'
    assert '--replay-user-messages' in sys.argv
    (root/'correction-request.json').write_text(json.dumps(next_turn))
    (root/'next-turn-seen').write_text('yes')
    if mode in ('cancel-after-reserve','silent-ack'):wait_cancel()
    if mode=='noisy-ack':flood()
    assert str(uuid.UUID(next_turn['uuid']))==next_turn['uuid']
    send({'type':'system','subtype':'init','session_id':'actual-session','apiKeySource':'none'})
    echoed=dict(next_turn,isReplay=True,timestamp='2026-09-10T23:52:20.334Z')
    if mode in ('forged-ack','stale-ack'):echoed['uuid']=str(uuid.uuid4())
    if mode=='forged-content':
        if case.get('replacement') is not None:
            echoed=json.loads(json.dumps(echoed));echoed['message']['content'][1]['tampered']=True
        else:echoed['message']={'role':'user','content':'different prompt'}
    send(echoed)
    if mode=='ack-clears-deadline':time.sleep(31)
    if mode=='duplicate-new-ack':send(echoed)
    if mode=='oracle-intent':
        args={'command':'true'}
        base=dict(base,hook_event_name='PreToolUse',tool_name='mcp__demoncoder__bash',tool_input=args,tool_use_id='reviewed-bash')
        callback('pre-bash','PreToolUse',base)
        request('permission-bash',{'subtype':'can_use_tool','tool_name':base['tool_name'],'tool_use_id':'reviewed-bash','input':args})
        rpc={'jsonrpc':'2.0','id':3,'method':'tools/call','params':{'name':'bash','arguments':args,'_meta':{'claudecode/toolUseId':'reviewed-bash'}}}
        reviewed=request('tool-bash',{'subtype':'mcp_message','server_name':'demoncoder','message':rpc})['mcp_response']['result']
        assert not json.loads(reviewed['content'][0]['text'])['success']
        callback('post-bash','PostToolUseFailure',dict(base,hook_event_name='PostToolUseFailure',error=reviewed['content'][0]['text'],is_interrupt=False))
    send({'type':'result','session_id':'actual-session','is_error':False,'subtype':'success','usage':{}})
while True:receive()
"#;

async fn case(adapter: &str, mode: &str) {
    case_with_replacement(adapter, mode, None).await;
}
async fn case_with_replacement(adapter: &str, mode: &str, replacement: Option<Value>) {
    case_with_steering(adapter, mode, replacement, None).await;
}
async fn case_with_steering(
    adapter: &str,
    mode: &str,
    replacement: Option<Value>,
    steering: Option<String>,
) -> Option<Value> {
    case_with_notification_order(adapter, mode, replacement, steering, false, None).await
}
async fn case_with_notification_order(
    adapter: &str,
    mode: &str,
    replacement: Option<Value>,
    steering: Option<String>,
    user_before_ack: bool,
    user_fault: Option<&str>,
) -> Option<Value> {
    let delayed_response = mode.starts_with("delayed-response");
    let mut f = Fixture::new();
    std::fs::write(f.root.path().join("watched"), "original").unwrap();
    let backend = f.root.path().join("backend.py");
    std::fs::write(&backend, BACKEND).unwrap();
    std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        f.root.path().join("case.json"),
        serde_json::to_vec(&json!({"adapter":adapter,"mode":mode,"replacement":replacement,"steering":steering.is_some(),"user_before_ack":user_before_ack,"user_fault":user_fault}))
            .unwrap(),
    )
    .unwrap();
    if mode != "absent" {
        let mut limits = Limits::default();
        // Exhaust the tool allowance after the first completed write instead
        // of refusing its initial execution.
        if mode == "allocation" {
            limits.model_calls = 1;
            limits.tool_calls = 1;
        }
        f.runtime.allocate(limits, None).unwrap();
        let mut task = Task::new(
            1,
            if mode == "blocked-write" {
                "o".repeat(64 * 1024)
            } else {
                "original owning task".into()
            },
            vec![],
            workspace::capture(f.root.path()).unwrap(),
            2,
        )
        .unwrap();
        if mode == "exhausted" {
            task.corrections = 2;
        }
        f.runtime.save_task(&Some(task), 2, None).unwrap();
    }
    let reason = if mode == "blocked-write" {
        "r".repeat(65536)
    } else {
        "plugin correction advice".into()
    };
    let hook = runner(move |_| RawOutcome::Model {
        value: json!({"ok":false,"reason":reason}),
        continue_on_block: false,
    });
    let mut registration = registration(
        "correction-plugin",
        HandlerClass::DecisionGate,
        hook.clone(),
    );
    registration.declaration.identity.runner = HandlerKind::Prompt;
    registration.declaration.matcher.tool = Some("write".into());
    registration.declaration.reads =
        GateReadSet::new(vec!["watched".into()], vec![], vec![]).unwrap();
    let mut connection: Connection =
        serde_json::from_value(json!({"adapter":adapter,"binary":backend,"model":"fixture"}))
            .unwrap();
    connection.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    let relay_lock = Arc::new(tokio::sync::Mutex::new(()));
    let relay_entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if mode.contains("-relay-") {
        connection.access.lifecycle = Some(Arc::new(
            demoncoder::plugins::bridge::Lifecycle::new(
                Arc::new(PendingLifecycle {
                    lock: relay_lock.clone(),
                    entered: relay_entered.clone(),
                }),
                Duration::from_secs(60),
            )
            .unwrap(),
        ));
    }
    if mode == "oracle-intent" {
        std::fs::write(f.root.path().join("oracle-mode"), "deny").unwrap();
        connection.access.unrestricted = true;
        connection.access.oracle = Some(Box::new(serde_json::from_value(json!({"adapter":"claude","binary":std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle_fixture.py")})).unwrap()));
    }
    let mut registrations = vec![registration];
    if let Some(value) = replacement.clone() {
        let mut replace = super::registration(
            "structured-replacement",
            HandlerClass::Combined,
            runner(move |_| {
                output(
                    json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":value}}),
                )
            }),
        );
        replace.declaration.identity.dialect = demoncoder::plugins::hook_types::HookDialect::Claude;
        replace.declaration.concurrent_group = Some("source-replacement".into());
        replace.declaration.reads =
            GateReadSet::new(vec!["watched".into()], vec![], vec![]).unwrap();
        registrations.push(replace);
    }
    connection.access.post_tools.push(Arc::new(
        PostToolPlan::new(HookEvent::PostToolUse, registrations).unwrap(),
    ));
    let mut session = demoncoder::adapters::builtins()
        .unwrap()
        .open(&connection, f.root.path())
        .unwrap();
    let (tx, mut commands) = mpsc::channel(4);
    let cancel = match mode {
        "cancel-before-release" => Some("interrupt-seen"),
        "cancel-after-reserve" => Some("next-turn-seen"),
        _ => None,
    };
    let steering_task = steering.map(|text| {
        let root = f.root.path().to_owned();
        let tx = tx.clone();
        tokio::spawn(async move {
            while !root.join("interrupt-seen").exists() {
                tokio::task::yield_now().await;
            }
            let (reply, received) = tokio::sync::oneshot::channel();
            tx.send(Command::Submit { text, reply }).await.unwrap();
            received.await.unwrap().unwrap();
            std::fs::write(root.join("steering-queued"), "yes").unwrap();
        })
    });
    let has_steering = steering_task.is_some();
    let _command_keepalive = tx.clone();
    let cancellation_started = Arc::new(Mutex::new(None));
    let cancellation = cancel.map(|marker| {
        let cancellation_started = cancellation_started.clone();
        let path = f.root.path().join(marker);
        tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !path.exists() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("backend did not reach cancellation boundary");
            *cancellation_started.lock().unwrap() = Some(Instant::now());
            tx.send(Command::Cancel).await.unwrap();
        })
    });
    // Keep the command sender alive for non-cancellation cases.
    let (_keepalive, mut fallback_commands) = mpsc::channel(4);
    let commands = if cancel.is_some() || has_steering {
        &mut commands
    } else {
        &mut fallback_commands
    };
    let enclosing_deadline = Instant::now()
        + Duration::from_secs(
            if delayed_response
                || matches!(
                    mode,
                    "silent-ack"
                        | "noisy-interrupt"
                        | "noisy-ack"
                        | "blocked-write"
                        | "blocked-context"
                        | "ack-clears-deadline"
                )
            {
                36
            } else {
                8
            },
        );
    let outcome = tokio::time::timeout_at(
        tokio::time::Instant::from_std(enclosing_deadline),
        async {
            let fill_publication = async {
                while !f.root.path().join("interrupt-seen").exists() {
                    tokio::task::yield_now().await;
                }
                // Claude publishes one terminal Usage event before reservation;
                // Codex publishes none. Fill the rest without consuming events.
                let leave = usize::from(adapter == "claude");
                for _ in leave..f._receiver.capacity() {
                    f.events.emit(demoncoder::events::Event::Text { text: "queued fixture event".into() }).await.unwrap();
                }
                std::fs::write(f.root.path().join("queue-filled"), "yes").unwrap();
                std::future::pending::<()>().await;
            };
            tokio::select! {
                biased;
                _ = fill_publication, if mode == "blocked-context" => unreachable!(),
                result = session.turn("perform original task".into(), commands, &f.events) => result,
            }
        },
    )
    .await;
    let cancelled_at = *cancellation_started.lock().unwrap();
    if ((delayed_response
        || matches!(
            mode,
            "silent-ack" | "noisy-interrupt" | "noisy-ack" | "blocked-write" | "blocked-context"
        ))
        && outcome.as_ref().is_ok_and(|r| r.is_err()))
        || cancelled_at.is_some()
    {
        let backend = super::owned_process::Identity::parse(
            &std::fs::read_to_string(f.root.path().join("backend-start-stat")).unwrap(),
        )
        .unwrap();
        // Claude post-hook sessions use the configured supervisor. Codex's
        // non-snapshot session spawns the backend directly; its supervisor
        // setting only selects the relay executable (see each adapter spawn).
        if adapter == "claude" {
            let supervisor = super::owned_process::Identity::parse(
                &std::fs::read_to_string(f.root.path().join("supervisor-start-stat")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                backend.parent, supervisor.pid,
                "source backend parent changed"
            );
            assert_eq!(
                backend.group, supervisor.pid,
                "source backend escaped supervised group"
            );
            supervisor.reaped().unwrap_or_else(|e| {
                panic!("direct supervisor not reaped on turn return: {adapter}/{mode}: {e:#}")
            });
        } else {
            assert_eq!(
                backend.group, backend.pid,
                "direct backend escaped owned group"
            );
            backend.reaped().unwrap_or_else(|e| {
                panic!("direct backend not reaped on turn return: {adapter}/{mode}: {e:#}")
            });
        }
        // No fresh grace after return. Hidden correction starts retain the
        // original whole-turn bound; explicit Cancel has an observed send.
        let deadline = cancelled_at.map_or(enclosing_deadline, |at| {
            (at + Duration::from_secs(2)).min(enclosing_deadline)
        });
        backend.stopped_by(deadline).await.unwrap_or_else(|e| {
            panic!("owned work outlived enclosing/cancellation deadline: {adapter}/{mode}: {e:#}")
        });
    }
    session.close().await.unwrap();
    if mode.contains("-relay-") {
        assert_eq!(
            relay_entered.load(Ordering::SeqCst),
            mode.ends_with("-worker")
        );
        assert!(
            relay_lock.try_lock().is_ok(),
            "timed-out relay handler must release its lock"
        );
    }
    if let Some(task) = steering_task {
        task.await.unwrap();
    }
    if let Some(task) = cancellation {
        task.await.unwrap();
    }
    let outcome = outcome.expect("external correction protocol timeout");
    if delayed_response
        || matches!(
            mode,
            "silent-ack" | "noisy-interrupt" | "noisy-ack" | "blocked-write" | "blocked-context"
        )
    {
        let error = outcome
            .as_ref()
            .err()
            .expect("transport must time out")
            .to_string();
        let stage = if delayed_response || mode == "noisy-interrupt" {
            "supersession"
        } else {
            "acknowledgment"
        };
        assert!(error.contains(stage), "{adapter}/{mode}: {error}");
    }
    if let Some(fault) = user_fault {
        let error = outcome
            .as_ref()
            .err()
            .expect("invalid pre-acknowledgment metadata must fail")
            .to_string();
        let expected = if matches!(
            fault,
            "request-id" | "unknown-method" | "other-item" | "user-flood" | "oversized-turn"
        ) {
            "pre-acknowledgment frames exceed bound"
        } else if fault.contains("thread") {
            "thread"
        } else {
            "turn"
        };
        assert!(error.contains(expected), "{fault}: {error}");
    }
    if cancel.is_some() {
        assert!(matches!(outcome, Ok(TurnEnd::Cancelled)));
    } else if matches!(
        mode,
        "success"
            | "terminal-first"
            | "oracle-intent"
            | "started-first"
            | "early-tool"
            | "early-completed"
            | "ack-clears-deadline"
    ) {
        assert!(outcome.is_ok(), "{adapter}/{mode}: {:?}", outcome.err());
    } else {
        assert!(outcome.is_err(), "{adapter}/{mode} must remain unmet");
    }
    assert_eq!(
        std::fs::read_to_string(f.root.path().join("created")).unwrap(),
        "written"
    );
    assert_eq!(hook.count.load(Ordering::SeqCst), 1);
    let record = f.record();
    let operation = record.operations.iter().find(|o| o.call.is_some()).unwrap();
    assert!(operation.result.as_ref().unwrap().success);
    assert_eq!(
        operation.result.as_ref().unwrap().output,
        "Wrote 7 bytes to created"
    );
    let post = operation
        .tool_receipt
        .as_ref()
        .unwrap()
        .plugin_lifecycle
        .as_ref()
        .unwrap();
    let charged = matches!(
        mode,
        "success"
            | "terminal-first"
            | "oracle-intent"
            | "blocked-write"
            | "blocked-context"
            | "silent-ack"
            | "noisy-ack"
            | "cancel-after-reserve"
            | "forged-ack"
            | "stale-ack"
            | "forged-content"
            | "duplicate-new-ack"
            | "started-first"
            | "early-tool"
            | "early-completed"
            | "ack-clears-deadline"
            | "started-first-duplicate"
            | "started-first-mismatch"
            | "started-first-missing"
            | "early-tool-mismatch"
            | "early-tool-overflow"
    );
    assert_eq!(post.correction_admitted, charged, "{adapter}/{mode}");
    if mode == "oversized-frame" {
        assert!(
            outcome
                .as_ref()
                .err()
                .unwrap()
                .to_string()
                .contains("bound")
        );
        assert!(post.correction_presentation.is_none());
        assert_eq!(
            record
                .operations
                .iter()
                .filter(|o| matches!(
                    o.host_invocation,
                    Some(demoncoder::workflow::runtime::HostInvocation::Backend)
                ))
                .count(),
            1
        );
    }
    if replacement.is_some() && charged {
        assert_eq!(
            post.correction_presentation,
            Some(demoncoder::plugins::receipts::CorrectionPresentation::ClaudeProviderBlocksV1)
        );
        assert_eq!(post.model_content.as_ref(), replacement.as_ref());
    }
    if delayed_response
        || matches!(
            mode,
            "noisy-interrupt" | "silent-ack" | "noisy-ack" | "blocked-write" | "blocked-context"
        )
    {
        use demoncoder::{plugins::receipts::PostDelivery, workflow::runtime::HostInvocation};
        assert_eq!(
            record
                .operations
                .iter()
                .filter(|o| matches!(o.host_invocation, Some(HostInvocation::Backend)))
                .count(),
            if charged { 2 } else { 1 }
        );
        if charged {
            assert!(matches!(
                post.delivery,
                PostDelivery::CorrectionReserved { .. }
            ));
        } else {
            assert!(matches!(post.delivery, PostDelivery::Superseding));
        }
    }
    if let Some(task) = &record.task {
        assert_eq!(
            task.corrections,
            if mode == "exhausted" {
                2
            } else {
                u32::from(charged)
            }
        );
        assert!(task.accepted.is_none());
    }
    assert_eq!(
        f.root.path().join("correction-request.json").exists(),
        charged && !matches!(mode, "blocked-write" | "blocked-context"),
        "{adapter}/{mode}/user_before_ack={user_before_ack}: {:?}",
        outcome.as_ref().err().map(|error| format!("{error:#}"))
    );
    if charged && !matches!(mode, "blocked-write" | "blocked-context") {
        let request: Value = serde_json::from_slice(
            &std::fs::read(f.root.path().join("correction-request.json")).unwrap(),
        )
        .unwrap();
        let prompt = if adapter == "claude" {
            if let Some(replacement) = &replacement {
                let blocks = request["message"]["content"]
                    .as_array()
                    .expect("typed corrective user content");
                assert_eq!(&blocks[1..], replacement.as_array().unwrap());
                blocks[0]["text"].as_str().unwrap()
            } else {
                request["message"]["content"].as_str().unwrap()
            }
        } else {
            request["params"]["input"][0]["text"].as_str().unwrap()
        };
        assert!(prompt.contains("Wrote 7 bytes to created"), "{prompt}");
        assert!(
            prompt.contains("[Plugin-origin correction-plugin"),
            "{prompt}"
        );
        assert!(prompt.contains("interruption bookkeeping"), "{prompt}");
        assert!(prompt.contains("not a developer denial"), "{prompt}");
        let state = serde_json::to_value(&post.delivery).unwrap();
        assert!(
            state
                .get(
                    if matches!(
                        mode,
                        "success"
                            | "terminal-first"
                            | "oracle-intent"
                            | "started-first"
                            | "early-tool"
                            | "early-completed"
                            | "ack-clears-deadline"
                            | "duplicate-new-ack"
                    ) {
                        "correction_acknowledged"
                    } else {
                        "correction_reserved"
                    }
                )
                .is_some(),
            "{state}"
        );
    }
    if mode == "oracle-intent" {
        let request: Value = serde_json::from_slice(
            &std::fs::read(f.root.path().join("oracle-request.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            request["developer_task"], "perform original task",
            "{adapter}: plugin context must not become Oracle authority"
        );
    }
    if mode == "blocked-context" {
        // Restore the test UI consumer only after checking reserved uncertainty;
        // retry then proves the delivery guard, not the deliberately full sink.
        while f._receiver.try_recv().is_ok() {}
    }
    if delayed_response
        || matches!(
            mode,
            "oversized-frame"
                | "blocked-write"
                | "blocked-context"
                | "silent-ack"
                | "noisy-ack"
                | "cancel-after-reserve"
                | "forged-ack"
                | "stale-ack"
                | "forged-content"
        )
    {
        let before = f.record();
        let (_resume_tx, mut resume_rx) = mpsc::channel(4);
        let replay = tokio::time::timeout(
            Duration::from_secs(3),
            session.turn("retry uncertain delivery".into(), &mut resume_rx, &f.events),
        )
        .await
        .unwrap();
        assert!(
            replay.is_err(),
            "uncertain correction must never be automatically resent"
        );
        session.close().await.unwrap();
        let after = f.record();
        assert_eq!(after.operations.len(), before.operations.len());
        assert_eq!(
            after.task.as_ref().unwrap().corrections,
            before.task.as_ref().unwrap().corrections
        );
    }
    std::fs::read(f.root.path().join("correction-request.json"))
        .ok()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn external_correction_supersedes_both_backends_before_one_bounded_handoff() {
    let _lock = FIXTURE.lock().await;
    for adapter in ["claude", "codex"] {
        for mode in ["success", "terminal-first"] {
            case(adapter, mode).await;
        }
    }
}

#[tokio::test]
async fn external_correction_preserves_hold_and_no_charge_before_release() {
    let _lock = FIXTURE.lock().await;
    for adapter in ["claude", "codex"] {
        for mode in [
            "absent",
            "exhausted",
            "allocation",
            "stale",
            "cancel-before-release",
            "forged-terminal",
            "duplicate-interrupt-ack",
            "duplicate-terminal",
        ] {
            case(adapter, mode).await;
        }
    }
    case("claude", "forged-cancel").await;
    case("codex", "missing-interrupt-result").await;
}

#[tokio::test]
async fn external_correction_reserved_handoff_is_never_replayed_after_cancel_or_forgery() {
    let _lock = FIXTURE.lock().await;
    for adapter in ["claude", "codex"] {
        for mode in [
            "cancel-after-reserve",
            "forged-ack",
            "stale-ack",
            "duplicate-new-ack",
        ] {
            case(adapter, mode).await;
        }
    }
    case("claude", "forged-content").await;
}

#[tokio::test]
async fn external_plugin_correction_does_not_replace_oracle_developer_intent() {
    let _lock = FIXTURE.lock().await;
    for adapter in ["claude", "codex"] {
        case(adapter, "oracle-intent").await;
    }
}

#[tokio::test]
async fn codex_correction_start_notification_does_not_replace_rpc_acknowledgment() {
    let _lock = FIXTURE.lock().await;
    for mode in [
        "started-first",
        "started-first-duplicate",
        "started-first-mismatch",
        "started-first-missing",
        "early-tool",
        "early-tool-mismatch",
        "early-tool-overflow",
    ] {
        case("codex", mode).await;
    }
}

#[tokio::test]
async fn external_correction_transport_deadlines_bound_silent_and_noisy_peers() {
    let _lock = FIXTURE.lock().await;
    tokio::join!(
        case("claude", "silent-ack"),
        case("codex", "silent-ack"),
        case("claude", "noisy-ack"),
        case("codex", "noisy-ack")
    );
}

#[tokio::test]
async fn external_correction_noisy_peer_cannot_starve_supersession_deadline() {
    let _lock = FIXTURE.lock().await;
    tokio::join!(
        case("claude", "noisy-interrupt"),
        case("codex", "noisy-interrupt")
    );
}

#[tokio::test]
async fn external_correction_exact_acknowledgment_clears_transport_deadline() {
    let _lock = FIXTURE.lock().await;
    tokio::join!(
        case("claude", "ack-clears-deadline"),
        case("codex", "ack-clears-deadline")
    );
}

#[tokio::test]
async fn external_correction_transport_deadline_bounds_blocked_correction_write() {
    let _lock = FIXTURE.lock().await;
    tokio::join!(
        case("claude", "blocked-write"),
        case("codex", "blocked-write")
    );
}

#[tokio::test]
async fn claude_structured_correction_preserves_provider_blocks() {
    let _lock = FIXTURE.lock().await;
    for content in super::claude::provider_contents() {
        case_with_replacement("claude", "success", Some(content)).await;
    }
    let image = super::claude::provider_contents().remove(0);
    for mode in ["forged-content", "cancel-after-reserve", "oracle-intent"] {
        case_with_replacement("claude", mode, Some(image.clone())).await;
    }
}

#[tokio::test]
async fn claude_structured_correction_timeout_retains_marker_without_replay() {
    let _lock = FIXTURE.lock().await;
    case_with_replacement(
        "claude",
        "silent-ack",
        Some(super::claude::provider_contents().remove(0)),
    )
    .await;
}

#[tokio::test]
async fn claude_correction_full_frame_bounds_reject_escaped_steering_before_charge() {
    let _lock = FIXTURE.lock().await;
    for replacement in [
        None,
        Some(
            json!([{"type":"image","source":{"type":"url","url":"https://example.test/image.png"}}]),
        ),
    ] {
        case_with_steering(
            "claude",
            "oversized-frame",
            replacement,
            Some("\\".repeat(2 * 1024 * 1024)),
        )
        .await;
    }
}

#[tokio::test]
async fn claude_correction_full_frame_bounds_accept_near_limit_and_reject_envelope_overflow() {
    let _lock = FIXTURE.lock().await;
    for replacement in [
        Some(
            json!([{"type":"image","source":{"type":"url","url":"https://example.test/image.png"}}]),
        ),
        None,
    ] {
        let mut baseline =
            case_with_steering("claude", "success", replacement.clone(), Some("x".into()))
                .await
                .unwrap();
        baseline["isReplay"] = json!(true);
        baseline["timestamp"] = json!("2026-09-10T23:52:20.334Z");
        let overhead = serde_json::to_vec(&baseline).unwrap().len(); // includes one steering byte; newline replaces it
        let available = 4 * 1024 * 1024 - overhead;
        let padding = available - 32;
        let text = format!("{}{}", "é".repeat(padding / 2), "x".repeat(padding % 2));
        case_with_steering("claude", "success", replacement.clone(), Some(text)).await;
        case_with_steering(
            "claude",
            "oversized-frame",
            replacement,
            Some("x".repeat(available + 32)),
        )
        .await;
    }
}

#[tokio::test]
async fn codex_correction_full_frame_bounds_reject_escaped_steering_before_charge() {
    let _lock = FIXTURE.lock().await;
    case_with_steering(
        "codex",
        "oversized-frame",
        None,
        Some("\\".repeat(2 * 1024 * 1024)),
    )
    .await;
}

#[tokio::test]
async fn codex_correction_full_frame_bounds_accept_near_limit_and_reject_envelope_overflow() {
    let _lock = FIXTURE.lock().await;
    let request = case_with_steering("codex", "success", None, Some("x".into()))
        .await
        .unwrap();
    let uuid = "00000000-0000-4000-8000-000000000000";
    let notification = json!({"method":"item/completed","params":{
        "item":{"type":"userMessage","id":uuid,"clientId":null,
            "content":[{"type":"text","text":request["params"]["input"][0]["text"],"text_elements":[]}]},
        "threadId":request["params"]["threadId"],"turnId":uuid,"completedAtMs":i64::MAX,
    },"emittedAtMs":i64::MAX});
    let available = 4 * 1024 * 1024 - serde_json::to_vec(&notification).unwrap().len();
    let padding = available - 32;
    case_with_steering(
        "codex",
        "success",
        None,
        Some(format!(
            "{}{}",
            "é".repeat(padding / 2),
            "x".repeat(padding % 2)
        )),
    )
    .await;
    case_with_steering(
        "codex",
        "oversized-frame",
        None,
        Some("x".repeat(available + 64)),
    )
    .await;
}

#[tokio::test]
async fn codex_large_user_notifications_preserve_pre_acknowledgment_order() {
    let _lock = FIXTURE.lock().await;
    for before in [false, true] {
        for mode in ["success", "early-tool"] {
            case_with_notification_order(
                "codex",
                mode,
                None,
                Some("é".repeat(2 * 1024 * 1024 - 1024)),
                before,
                None,
            )
            .await;
        }
    }
}

#[tokio::test]
async fn codex_large_user_notifications_retain_correlation_and_unknown_queue_bounds() {
    let _lock = FIXTURE.lock().await;
    for fault in [
        "wrong-thread",
        "wrong-turn",
        "missing-thread",
        "missing-turn",
        "malformed-thread",
        "malformed-turn",
        "request-id",
        "unknown-method",
        "other-item",
        "user-flood",
        "oversized-turn",
    ] {
        case_with_notification_order(
            "codex",
            "forged-content",
            None,
            Some(if matches!(fault, "user-flood" | "oversized-turn") {
                "small".into()
            } else {
                "s".repeat(1100 * 1024)
            }),
            true,
            Some(fault),
        )
        .await;
    }
    case_with_notification_order(
        "codex",
        "early-completed",
        None,
        Some("s".repeat(1100 * 1024)),
        true,
        None,
    )
    .await;
    case_with_notification_order(
        "codex",
        "cancel-after-reserve",
        None,
        Some("s".repeat(1100 * 1024)),
        true,
        None,
    )
    .await;
    case_with_notification_order(
        "codex",
        "early-tool-mismatch",
        None,
        Some("s".repeat(1100 * 1024)),
        true,
        None,
    )
    .await;
    case_with_notification_order(
        "codex",
        "early-tool-overflow",
        None,
        Some("s".repeat(1100 * 1024)),
        true,
        None,
    )
    .await;
}

#[tokio::test]
async fn codex_large_user_notifications_still_require_rpc_acknowledgment() {
    let _lock = FIXTURE.lock().await;
    // Run large handoffs separately from other saturated fake transports: the
    // missing-ack clock is the boundary under test, not write backpressure.
    for mode in ["silent-ack", "noisy-ack"] {
        case_with_notification_order(
            "codex",
            mode,
            None,
            Some("s".repeat(1100 * 1024)),
            true,
            None,
        )
        .await;
    }
}

#[tokio::test]
async fn correction_deadline_bounds_delayed_response_dispatch() {
    let _lock = FIXTURE.lock().await;
    tokio::join!(
        case("claude", "delayed-response"),
        case("codex", "delayed-response"),
        case("claude", "delayed-response-tool"),
        case("codex", "delayed-response-tool"),
        case("codex", "delayed-response-relay-read"),
        case("codex", "delayed-response-relay-worker")
    );
}

#[tokio::test]
async fn correction_deadline_bounds_reserved_context_publication() {
    let _lock = FIXTURE.lock().await;
    tokio::join!(
        case("claude", "blocked-context"),
        case("codex", "blocked-context")
    );
}

use super::*;
use std::os::unix::fs::PermissionsExt;
const BACKEND: &str = r#"#!/usr/bin/python3
import json,sys,pathlib
root=pathlib.Path.cwd();adapter=(root/'adapter').read_text()
if '--demoncoder-compaction-capability' in sys.argv:
 print(json.dumps({'protocol':'demoncoder-compaction-v1','source_version':'0.153.4','patch_version':1}),flush=True);sys.exit(0)
def send(v): print(json.dumps(v),flush=True)
def receive():
 line=sys.stdin.readline()
 if not line: sys.exit(0)
 v=json.loads(line)
 with (root/'wire.jsonl').open('a') as f:f.write(json.dumps(v)+'\n')
 return v
def request(i,body):
 send({'type':'control_request','request_id':i,'request':body});r=receive()
 assert r['response']['subtype']=='success',r
 return r['response']['response']
if adapter=='claude':
 init=receive();hooks=init['request']['hooks'];send({'type':'control_response','response':{'request_id':'initialize','subtype':'success'}})
 for turn in range(2):
  user=receive();assert user['type']=='user',user
  send({'type':'system','subtype':'init','session_id':'observer-session','apiKeySource':'none'})
  if turn==0:
   args={'path':'original','content':'written'}
   base={'hook_event_name':'PreToolUse','session_id':'observer-session','transcript_path':str(root/'transcript'),'cwd':str(root),'permission_mode':'default','tool_name':'mcp__demoncoder__write','tool_input':args,'tool_use_id':'observer-tool'}
   def callback(i,event,value):return request(i,{'subtype':'hook_callback','callback_id':hooks[event][0]['hookCallbackIds'][0],'tool_use_id':'observer-tool','input':value})
   callback('pre','PreToolUse',base)
   request('permission',{'subtype':'can_use_tool','tool_name':base['tool_name'],'tool_use_id':'observer-tool','input':args})
   rpc={'jsonrpc':'2.0','id':2,'method':'tools/call','params':{'name':'write','arguments':args,'_meta':{'claudecode/toolUseId':'observer-tool'}}}
   result=request('tool',{'subtype':'mcp_message','server_name':'demoncoder','message':rpc})['mcp_response']['result']
   assert json.loads(result['content'][0]['text'])['success'],result
   post=dict(base,hook_event_name='PostToolUse',tool_response=result['content']);callback('post','PostToolUse',post)
  send({'type':'result','subtype':'success','is_error':False,'session_id':'observer-session','usage':{}})
else:
 while True:
  r=receive();m=r.get('method')
  if m=='initialize':v={}
  elif m=='initialized':continue
  elif m=='account/read':v={'account':{'type':'chatgpt'},'requiresOpenaiAuth':True}
  elif m=='config/read':v={'config':{}}
  elif m in ('thread/start','thread/resume'):v={'thread':{'id':'observer-thread','path':'/transcript'},'model':'observer-model'}
  elif m=='turn/start':break
  else:raise AssertionError(r)
  send({'id':r['id'],'result':v})
 for turn in range(2):
  if turn:r=receive()
  assert r['method']=='turn/start',r
  tid='observer-turn-'+str(turn)
  send({'id':r['id'],'result':{'turn':{'id':tid}}})
  send({'method':'turn/started','params':{'threadId':'observer-thread','turn':{'id':tid}}})
  if turn==0:
   send({'id':'tool','method':'item/tool/call','params':{'threadId':'observer-thread','turnId':tid,'callId':'observer-tool','tool':'write','arguments':{'path':'original','content':'written'}}})
   reply=receive();assert reply['id']=='tool',reply
  send({'method':'turn/completed','params':{'threadId':'observer-thread','turn':{'id':tid,'status':'completed'}}})
while True:receive()
"#;

#[tokio::test]
async fn async_external_context_waits_for_next_correlated_claude_and_codex_request() {
    let _serial = SERIAL.lock().await;
    for adapter in ["claude", "codex"] {
        let fixture = Fixture::new();
        fixture.runtime.allocate(Default::default(), None).unwrap();
        std::fs::write(fixture.root.path().join("adapter"), adapter).unwrap();
        let backend = fixture.root.path().join("peer.py");
        std::fs::write(&backend, BACKEND).unwrap();
        std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o700)).unwrap();
        let source = tempfile::tempdir().unwrap();
        let dialect = if adapter == "claude" {
            HookDialect::Claude
        } else {
            HookDialect::Codex
        };
        let metadata = source.path().join(if adapter == "claude" {
            ".claude-plugin"
        } else {
            ".codex-plugin"
        });
        std::fs::create_dir(&metadata).unwrap();
        std::fs::write(
            metadata.join("plugin.json"),
            r#"{"name":"observer-source","version":"1.0.0"}"#,
        )
        .unwrap();
        let package =
            Arc::new(plugins::inspect(source.path(), &plugins::ImportOptions::default()).unwrap());
        let mut declaration = declaration("external-observer", 0, None);
        declaration.required_gate = false;
        declaration.identity.dialect = dialect;
        declaration.concurrent_group = (adapter == "claude").then(|| "source".into());
        let mut command=CommandConfig::new(CommandProgram::Shell("sleep 0.6; printf '%s\\n' '{\"hookSpecificOutput\":{\"hookEventName\":\"PostToolUse\",\"additionalContext\":\"late external observer data\"}}'".into()));
        command.asynchronous = true;
        command.timeout_ms = 5000;
        command.model = Some("observer-model".into());
        let registration = CommandRunner::registration_for_event(
            package,
            declaration,
            HookEvent::PostToolUse,
            command,
            None,
        )
        .unwrap();
        let mut connection: Connection = serde_json::from_value(
            json!({"adapter":adapter,"binary":backend,"model":"observer-model"}),
        )
        .unwrap();
        connection.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
        connection.access.post_tools.push(Arc::new(
            PostToolPlan::new(HookEvent::PostToolUse, vec![registration]).unwrap(),
        ));
        let mut session = demoncoder::adapters::builtins()
            .unwrap()
            .open(&connection, fixture.root.path())
            .unwrap();
        let (_sender, mut commands) = mpsc::channel(4);
        let first = tokio::time::timeout(
            Duration::from_secs(8),
            session.turn("first developer".into(), &mut commands, &fixture.events),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(matches!(first, demoncoder::session::TurnEnd::Complete));
        assert_eq!(
            std::fs::read_to_string(fixture.root.path().join("original")).unwrap(),
            "written"
        );
        assert!(
            super::async_commands::hooks(&fixture)
                .last()
                .unwrap()
                .outcome
                .is_none()
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while super::async_commands::hooks(&fixture)
                .last()
                .unwrap()
                .outcome
                .is_none()
            {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::timeout(
            Duration::from_secs(8),
            session.turn("next developer".into(), &mut commands, &fixture.events),
        )
        .await
        .unwrap()
        .unwrap();
        let wire = std::fs::read_to_string(fixture.root.path().join("wire.jsonl")).unwrap();
        let requests = wire
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|v| v["type"] == "user" || v["method"] == "turn/start")
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 2);
        assert!(
            !requests[0]
                .to_string()
                .contains("late external observer data")
        );
        assert!(
            requests[1]
                .to_string()
                .contains("late external observer data")
        );
        assert!(
            requests[1]
                .to_string()
                .contains("Plugin-origin observer-source")
        );
        if adapter == "codex" {
            assert_eq!(requests[1]["params"]["threadId"], "observer-thread");
        } else {
            assert_eq!(requests[1]["session_id"], "observer-session");
        }
        session.close().await.unwrap();
    }
}

//! Controlled actual command/socket transport through the production Codex owner.
use anyhow::Result;
use async_trait::async_trait;
use demoncoder::{
    adapters,
    config::Connection,
    events::EventSink,
    plugins::bridge::{Decision, Handler, Invocation, Lifecycle},
    session::TurnEnd,
};
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
use tokio::sync::mpsc;

struct Gate(&'static str);
#[async_trait]
impl Handler for Gate {
    async fn handle(&self, request: &Invocation, _: &EventSink) -> Result<Decision> {
        assert_eq!(request.session, "thread-fixture");
        match self.0 {
            "block" => Ok(Decision::Block("host denial".into())),
            "failure" => anyhow::bail!("worker disconnected"),
            "timeout" => std::future::pending().await,
            _ => Ok(Decision::Continue),
        }
    }
}

#[tokio::test]
async fn managed_command_requires_host_acknowledgment_before_effect() -> Result<()> {
    for mode in [
        "allow",
        "credential-access",
        "manual-reordered",
        "block",
        "failure",
        "timeout",
        "forgery",
        "wrong-turn",
        "unqualified",
    ] {
        let root = tempfile::tempdir()?;
        let earlier_session_tools = demoncoder::tools::ToolExecutor::new(root.path())?;
        let backend = root.path().join("backend.py");
        std::fs::write(
            &backend,
            format!(
                r#"#!/usr/bin/python3
import json, os, subprocess, sys
from pathlib import Path
mode={mode:?}
if '--demoncoder-compaction-capability' in sys.argv:
    print(json.dumps({{'protocol':'demoncoder-compaction-v1','source_version':'0.153.4','patch_version':0 if mode=='unqualified' else 1}}))
    sys.exit(0)
def send(value): print(json.dumps(value),flush=True)
requirement=json.loads(os.environ['CODEX_DEMONCODER_COMPACTION_RELAY'])
source=json.loads(Path(requirement['source_path']).read_text())
command=source['hooks']['PreCompact'][0]['hooks'][0]['command']
for line in sys.stdin:
    row=json.loads(line)
    method=row.get('method')
    if method=='initialize': result={{}}
    elif method=='initialized': continue
    elif method=='account/read': result={{'account':{{'type':'chatgpt'}},'requiresOpenaiAuth':True}}
    elif method=='config/read': result={{'config':{{}}}}
    elif method=='thread/start': result={{'thread':{{'id':'thread-fixture'}}}}
    elif method in ['turn/start','thread/compact/start']:
        if mode=='manual-reordered':
            assert method=='thread/compact/start'
            send({{'method':'turn/started','params':{{'threadId':'thread-fixture','turn':{{'id':'turn-fixture'}}}}}})
            send({{'id':row['id'],'result':{{}}}})
        else:
            send({{'id':row['id'],'result':{{'turn':{{'id':'turn-fixture'}}}}}})
            send({{'method':'turn/started','params':{{'threadId':'thread-fixture','turn':{{'id':'turn-fixture'}}}}}})
        if mode=='credential-access':
            address=Path(requirement['source_path']).with_name('address.json')
            token=json.loads(address.read_text())['token']
            Path('private-address-path').write_text(str(address))
            import shlex
            for index, (tool, args) in enumerate([('read', {{'path':str(address)}}), ('bash', {{'command':'cat '+shlex.quote(str(address))}})]):
                send({{'id':900+index,'method':'item/tool/call','params':{{'threadId':'thread-fixture','turnId':'turn-fixture','callId':'private-'+str(index),'tool':tool,'arguments':args}}}})
                answer=json.loads(sys.stdin.readline())
                assert answer['id']==900+index
                if token in json.dumps(answer): Path('credential-leaked').write_text(tool)
                assert token not in json.dumps(answer), 'relay credential escaped through '+tool
                assert answer['result']['success'] is False, 'private address was admitted by '+tool
        if mode=='forgery':
            address=Path(requirement['source_path']).with_name('address.json')
            value=json.loads(address.read_text()); value['token']='forged'; address.write_text(json.dumps(value))
        call={{'session_id':'thread-fixture','turn_id':'other' if mode=='wrong-turn' else 'turn-fixture','hook_event_name':'PreCompact','trigger':'auto','demonCoderCompaction':{{'protocol':'demoncoder-compaction-v1','challenge':'fresh-challenge'}}}}
        child=subprocess.Popen(command,shell=True,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
        Path('relay.pid').write_text(str(child.pid))
        out,_=child.communicate(json.dumps(call).encode())
        response=json.loads(out) if child.returncode==0 else {{'continue':False}}
        if response.get('continue') is True:
            assert response['demonCoderCompaction']['challenge']=='fresh-challenge'
            Path('crossed').write_text('actual guarded effect')
        send({{'method':'turn/completed','params':{{'threadId':'thread-fixture','turn':{{'id':'turn-fixture','status':'completed'}}}}}})
        continue
    else: raise AssertionError(method)
    send({{'id':row['id'],'result':result}})
"#
            ),
        )?;
        std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o700))?;
        let mut config: Connection =
            serde_json::from_value(json!({"adapter":"codex","binary":backend}))?;
        config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
        config.access.lifecycle = Some(Arc::new(Lifecycle::new(
            Arc::new(Gate(mode)),
            Duration::from_millis(100),
        )?));
        let mut session = adapters::builtins()?.open(&config, root.path())?;
        let (_tx, mut rx) = mpsc::channel(4);
        let (tx, mut output) = mpsc::channel(256);
        let drain = tokio::spawn(async move { while output.recv().await.is_some() {} });
        let events = EventSink::new("codex-bridge".into(), tx, None)?;
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            session.turn(
                if mode == "manual-reordered" {
                    "/compact"
                } else {
                    "hello"
                }
                .into(),
                &mut rx,
                &events,
            ),
        )
        .await?;
        assert!(
            !root.path().join("credential-leaked").exists(),
            "relay credential escaped through a model tool"
        );
        if matches!(
            mode,
            "allow" | "block" | "manual-reordered" | "credential-access"
        ) {
            assert!(matches!(result?, TurnEnd::Complete));
        } else {
            assert!(result.is_err(), "{mode} failed to reject invalid relay");
        }
        assert_eq!(
            root.path().join("crossed").exists(),
            matches!(mode, "allow" | "manual-reordered" | "credential-access"),
            "{mode}"
        );
        if mode == "credential-access" {
            let address = std::fs::read_to_string(root.path().join("private-address-path"))?;
            for (name, arguments) in [
                ("read", json!({"path":address})),
                (
                    "bash",
                    json!({"command":format!("cat '{}'",address.replace('\'', "'\"'\"'"))}),
                ),
            ] {
                let result = earlier_session_tools
                    .execute(
                        demoncoder::tools::ToolCall {
                            id: "earlier-session".into(),
                            name: name.into(),
                            arguments,
                        },
                        &events,
                    )
                    .await?;
                assert!(
                    !result.success,
                    "earlier session admitted relay credentials through {name}"
                );
            }
        }
        session.close().await?;
        drop(events);
        drain.await?;
    }
    Ok(())
}

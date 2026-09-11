use super::*;
use demoncoder::plugins::hook_types::HookDialect;
use std::os::unix::fs::PermissionsExt;
const BACKEND: &str = r#"#!/usr/bin/python3
import json,sys,pathlib
root=pathlib.Path.cwd()
config=json.loads((root/'case.json').read_text())
def receive():
    line=sys.stdin.readline()
    if not line: sys.exit(3)
    value=json.loads(line)
    with (root/'wire.jsonl').open('a') as f:f.write(json.dumps(value)+'\n')
    return value
def send(value): print(json.dumps(value),flush=True)
def request(identity,body):
    send({'type':'control_request','request_id':identity,'request':body})
    reply=receive()
    assert reply['response']['subtype']=='success',reply
    return reply['response']['response']
init=receive();hooks=init['request']['hooks']
assert all(e in hooks for e in ['PreToolUse','PostToolUse','PostToolUseFailure'])
send({'type':'control_response','response':{'request_id':'initialize','subtype':'success'}})
while True:
    receive()
    send({'type':'system','subtype':'init','session_id':'actual-session','apiKeySource':'none'})
    name='read' if config['failure'] else 'write'
    args={'path':'absent'} if config['failure'] else {'path':'created','content':'written'}
    base={'hook_event_name':'PreToolUse','session_id':'actual-session','transcript_path':str(root/'actual-sdk-transcript.jsonl'),'cwd':str(root),'permission_mode':'default','prompt_id':'actual-prompt','effort':{'level':'high'},'tool_name':'mcp__demoncoder__'+name,'tool_input':args,'tool_use_id':'actual-tool-id'}
    def callback(identity,event,value):
        return request(identity,{'subtype':'hook_callback','callback_id':hooks[event][0]['hookCallbackIds'][0],'tool_use_id':value['tool_use_id'],'input':value})
    callback('pre','PreToolUse',base)
    request('permission',{'subtype':'can_use_tool','tool_name':base['tool_name'],'tool_use_id':base['tool_use_id'],'input':args})
    rpc={'jsonrpc':'2.0','id':2,'method':'tools/call','params':{'name':name,'arguments':args,'_meta':{'claudecode/toolUseId':'actual-tool-id'}}}
    result=request('tool',{'subtype':'mcp_message','server_name':'demoncoder','message':rpc})['mcp_response']['result']
    original=json.loads(result['content'][0]['text'])
    assert original['success'] == (not config['failure'] and config['mode']!='predeny')
    if original['success']: assert original['output']=='Wrote 7 bytes to '+('rewritten' if config['mode']=='rewrite' else 'created')
    if config['mode']=='stale': (root/'watched').write_text('changed')
    post=dict(base)
    event='PostToolUseFailure' if not original['success'] else 'PostToolUse'
    post['hook_event_name']=event
    if not original['success']:post.update(error=result['content'][0]['text'],is_interrupt=False)
    else:post['tool_response']=result['content']
    if config['mode']=='forged':post['tool_use_id']='forged-id'
    reply=callback('post',event,post)
    (root/'presentation.json').write_text(json.dumps(reply))
    if config['mode']=='duplicate':callback('post-again',event,post)
    (root/'next-model').write_text('requested')
    send({'type':'result','subtype':'success','is_error':False,'session_id':'actual-session','usage':{}})

"#;
async fn case(mode: &str, failure: bool) -> (Fixture, anyhow::Result<TurnEnd>, Arc<AtomicUsize>) {
    case_with_content(mode, failure, None).await
}
async fn case_with_content(
    mode: &str,
    failure: bool,
    content: Option<Value>,
) -> (Fixture, anyhow::Result<TurnEnd>, Arc<AtomicUsize>) {
    let f = Fixture::new();
    std::fs::write(f.root.path().join("watched"), "original").unwrap();
    let backend = f.root.path().join("backend.py");
    std::fs::write(&backend, BACKEND).unwrap();
    std::fs::set_permissions(&backend, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(
        f.root.path().join("case.json"),
        serde_json::to_vec(&json!({"mode":mode,"failure":failure})).unwrap(),
    )
    .unwrap();
    let hold = mode == "hold" || mode == "structured-hold";
    let rewritten = mode == "rewrite";
    let replacement_only = mode == "replacement-only";
    let switch_search = mode == "search-policy-change";
    let structured_calls = AtomicUsize::new(0);
    let invalid = match mode {
        "invalid-object" => Some(json!({"text":"unsupported object"})),
        "invalid-image-missing-mime" => Some(json!([{"type":"image","data":"unsupported"}])),
        "invalid-text" => Some(json!([{"type":"text","text":42}])),
        _ => None,
    };
    let hook = runner(move |invocation| {
        let completed = invocation.completed.as_ref().unwrap();
        if rewritten {
            assert_eq!(invocation.candidate.arguments["path"], "rewritten");
        }
        if let demoncoder::plugins::receipts::ToolRepresentation::ClaudeMcp {
            tool_use_id,
            source_input,
            ..
        } = &completed.facts.representation
        {
            assert_eq!(tool_use_id, "actual-tool-id");
            assert_eq!(source_input["prompt_id"], "actual-prompt");
            assert_eq!(source_input["effort"], json!({"level":"high"}));
            assert!(
                source_input["transcript_path"]
                    .as_str()
                    .unwrap()
                    .ends_with("actual-sdk-transcript.jsonl")
            );
        } else {
            panic!("missing actual source representation");
        }
        if let Some(value) = &content {
            let mut value = value.clone();
            if switch_search && structured_calls.fetch_add(1, Ordering::SeqCst) > 0 {
                value[0]["citations"] = json!({"enabled":false});
            }
            output(
                json!({"continue":!hold,"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":value,"additionalContext":"structured plugin advice"}}),
            )
        } else if let Some(value) = &invalid {
            output(
                json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":value}}),
            )
        } else if hold {
            output(json!({"continue":false}))
        } else if failure {
            output(
                json!({"hookSpecificOutput":{"hookEventName":"PostToolUseFailure","additionalContext":"failure advice"}}),
            )
        } else if replacement_only {
            output(
                json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":"model replacement"}}),
            )
        } else {
            output(
                json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":[{"type":"text","text":"model replacement"}],"additionalContext":"post advice"}}),
            )
        }
    });
    let mut registration = registration("claude-post", HandlerClass::Combined, hook.clone());
    registration.declaration.identity.dialect = HookDialect::Claude;
    registration.declaration.concurrent_group = Some("source-group".into());
    registration.declaration.reads =
        GateReadSet::new(vec!["watched".into()], vec![], vec![]).unwrap();
    if mode == "command" {
        use demoncoder::plugins::runners::{CommandConfig, CommandProgram, CommandRunner};
        let code = r#"import json,sys
x=json.load(sys.stdin)
assert x['hook_event_name']=='PostToolUse'
assert x['session_id']=='actual-session'
assert x['tool_use_id']=='actual-tool-id'
assert x['tool_name']=='mcp__demoncoder__write'
assert x['prompt_id']=='actual-prompt' and x['effort']=={'level':'high'}
assert x['transcript_path'].endswith('actual-sdk-transcript.jsonl')
assert x['permission_mode']=='default'
assert isinstance(x['tool_response'],list)
assert json.loads(x['tool_response'][0]['text'])['output']=='Wrote 7 bytes to created'
print(json.dumps({'hookSpecificOutput':{'hookEventName':'PostToolUse','updatedMCPToolOutput':[{'type':'text','text':'model replacement'}]}}))
"#;
        let mut config = CommandConfig::new(CommandProgram::Argv(vec![
            "/usr/bin/python3".into(),
            "${CLAUDE_PLUGIN_ROOT}/hook.py".into(),
        ]));
        config.transcript_path = Some("wrong-config-transcript".into());
        config.permission_mode = "bypassPermissions".into();
        registration = CommandRunner::registration_for_event(
            super::runners::package(HookDialect::Claude, code),
            registration.declaration,
            HookEvent::PostToolUse,
            config,
            None,
        )
        .unwrap();
    }
    let mut registrations = vec![registration];
    if mode == "conflict" {
        let mut other = super::registration(
            "other",
            HandlerClass::Combined,
            runner(|_| {
                output(
                    json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":"conflicting replacement"}}),
                )
            }),
        );
        other.declaration = registrations[0].declaration.clone();
        other.declaration.identity.index = 1;
        other.declaration.identity.declaration = "other".into();
        registrations.push(other);
    }
    let event = if failure {
        HookEvent::PostToolUseFailure
    } else {
        HookEvent::PostToolUse
    };
    let mut config: Connection =
        serde_json::from_value(json!({"adapter":"claude","binary":backend,"model":"fixture"}))
            .unwrap();
    config.access.supervisor = Some(env!("CARGO_BIN_EXE_demoncoder").into());
    if rewritten {
        let pre = runner(|_| {
            output(
                json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":{"path":"rewritten","content":"written"}}}),
            )
        });
        config.access.pre_tool = Some(Arc::new(
            PreToolPlan::new(vec![super::registration(
                "pre-rewrite",
                HandlerClass::Transformer,
                pre,
            )])
            .unwrap(),
        ));
    }
    if mode == "predeny" {
        config.access.pre_tool=Some(Arc::new(PreToolPlan::new(vec![super::registration("pre-deny",HandlerClass::DecisionGate,runner(|_|output(json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny"}}))))]).unwrap()));
    }
    config
        .access
        .post_tools
        .push(Arc::new(PostToolPlan::new(event, registrations).unwrap()));
    let mut session = demoncoder::adapters::builtins()
        .unwrap()
        .open(&config, f.root.path())
        .unwrap();
    let (_sender, mut commands) = mpsc::channel(4);
    let end = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        session.turn("test".into(), &mut commands, &f.events),
    )
    .await
    .expect("controlled backend timeout");
    let end = if matches!(mode, "reuse" | "search-policy-change") && end.is_ok() {
        tokio::time::timeout(
            std::time::Duration::from_secs(15),
            session.turn("second turn".into(), &mut commands, &f.events),
        )
        .await
        .unwrap()
    } else {
        end
    };
    session.close().await.unwrap();
    (f, end, hook.count.clone())
}
#[tokio::test]
async fn claude_source_callback_releases_typed_presentation_once_after_original_result() {
    let _lock = FIXTURE.lock().await;
    for (mode, failure) in [
        ("allow", false),
        ("allow", true),
        ("rewrite", false),
        ("command", false),
        ("reuse", false),
    ] {
        let (f, end, count) = case(mode, failure).await;
        assert!(
            end.is_ok(),
            "{mode}: {:?} {:?}",
            end.as_ref().err(),
            f.record()
                .operations
                .iter()
                .filter_map(|o| o.tool_receipt.as_ref()?.plugin_lifecycle.as_ref())
                .flat_map(|p| p.hooks.iter().map(|h| &h.outcome))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            if mode == "command" {
                0
            } else if mode == "reuse" {
                2
            } else {
                1
            }
        );
        assert!(f.root.path().join("next-model").exists());
        if mode == "rewrite" {
            assert!(f.root.path().join("rewritten").exists());
            assert!(!f.root.path().join("created").exists());
        }
        let presentation: Value = serde_json::from_slice(
            &std::fs::read(f.root.path().join("presentation.json")).unwrap(),
        )
        .unwrap();
        if failure {
            assert!(
                presentation["hookSpecificOutput"]["additionalContext"]
                    .as_str()
                    .unwrap()
                    .contains("failure advice")
            );
        } else {
            assert_eq!(
                presentation["hookSpecificOutput"]["updatedMCPToolOutput"],
                json!([{"type":"text","text":"model replacement"}])
            );
        }
        let lifecycle = f
            .record()
            .operations
            .into_iter()
            .find_map(|o| o.tool_receipt.and_then(|r| r.plugin_lifecycle))
            .unwrap();
        assert_eq!(
            lifecycle.delivery,
            demoncoder::plugins::receipts::PostDelivery::Acknowledged
        );
    }
}

#[tokio::test]
async fn claude_replacement_only_retains_exact_payload_and_separate_plugin_attribution() {
    let _lock = FIXTURE.lock().await;
    let (f, end, _) = case("replacement-only", false).await;
    assert!(end.is_ok(), "{:?}", end.err());
    let presentation: Value =
        serde_json::from_slice(&std::fs::read(f.root.path().join("presentation.json")).unwrap())
            .unwrap();
    assert_eq!(
        presentation["hookSpecificOutput"]["updatedMCPToolOutput"],
        "model replacement"
    );
    let context = presentation["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("replacement-only callback must include separate provenance context");
    assert!(context.contains("[Plugin-origin claude-post]"));
    assert!(context.contains("replacement"));
    let record = f.record();
    let operation = record.operations.iter().find(|o| o.call.is_some()).unwrap();
    assert_eq!(
        operation.result.as_ref().unwrap().output,
        "Wrote 7 bytes to created"
    );
}
#[tokio::test]
async fn claude_held_forged_stale_and_repeated_delivery_never_reaches_next_model() {
    let _lock = FIXTURE.lock().await;
    for mode in ["hold", "forged", "stale", "duplicate", "conflict"] {
        let (f, end, count) = case(mode, false).await;
        assert!(end.is_err(), "{mode}");
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(!f.root.path().join("next-model").exists(), "{mode}");
        assert_eq!(
            std::fs::read_to_string(f.root.path().join("created")).unwrap(),
            "written"
        );
        let record = f.record();
        let operation = record.operations.iter().find(|o| o.call.is_some()).unwrap();
        assert!(operation.result.as_ref().unwrap().success);
    }
}

#[tokio::test]
async fn claude_sdk_failure_after_host_pre_denial_does_not_dispatch_executed_failure() {
    let _lock = FIXTURE.lock().await;
    let (f, end, count) = case("predeny", false).await;
    assert!(end.is_ok(), "{:?}", end.err());
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(!f.root.path().join("created").exists());
    assert!(
        f.record()
            .operations
            .iter()
            .filter_map(|o| o.tool_receipt.as_ref())
            .all(|r| r.plugin_lifecycle.is_none())
    );
    assert!(f.root.path().join("next-model").exists());
}

#[tokio::test]
async fn claude_replacement_disposition_matches_delivered_shape() {
    let _lock = FIXTURE.lock().await;
    for mode in [
        "invalid-object",
        "invalid-image-missing-mime",
        "invalid-text",
        "replacement-only",
        "allow",
    ] {
        let (f, end, _) = case(mode, false).await;
        let invalid = mode.starts_with("invalid");
        assert_eq!(end.is_err(), invalid, "{mode}");
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
        let expected = if invalid { "held" } else { "applied" };
        let proposals = serde_json::to_value(&post.proposals).unwrap();
        let replacement = proposals
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["proposal"]["kind"] == "replace_model_output")
            .unwrap();
        assert_eq!(replacement["disposition"], expected, "{mode}");
        assert_eq!(post.model_content.is_none(), invalid);
        let RawOutcome::Callback { value } = post.hooks[0]
            .outcome
            .as_ref()
            .expect("raw outcome retained")
        else {
            panic!("callback outcome required")
        };
        let raw = &value["hookSpecificOutput"]["updatedMCPToolOutput"];
        let expected_raw = match mode {
            "invalid-object" => json!({"text":"unsupported object"}),
            "invalid-image-missing-mime" => json!([{"type":"image","data":"unsupported"}]),
            "invalid-text" => json!([{"type":"text","text":42}]),
            "replacement-only" => json!("model replacement"),
            _ => json!([{"type":"text","text":"model replacement"}]),
        };
        assert_eq!(
            raw, &expected_raw,
            "rejected source payload must remain retained"
        );
        assert_eq!(f.root.path().join("presentation.json").exists(), !invalid);
        if !invalid {
            let delivered: Value = serde_json::from_slice(
                &std::fs::read(f.root.path().join("presentation.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                delivered["hookSpecificOutput"]["updatedMCPToolOutput"],
                post.model_content.as_ref().unwrap().clone()
            );
        }
    }
}

#[tokio::test]
async fn claude_provider_content_preserves_value_and_original_evidence() {
    let _lock = FIXTURE.lock().await;
    for content in provider_contents() {
        let (f, end, _) = case_with_content("structured", false, Some(content.clone())).await;
        assert!(end.is_ok(), "{:?}", end.err());
        let delivered: Value = serde_json::from_slice(
            &std::fs::read(f.root.path().join("presentation.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            delivered["hookSpecificOutput"]["updatedMCPToolOutput"],
            content
        );
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
        assert_eq!(post.model_content.as_ref(), Some(&content));
        assert!(
            delivered["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .unwrap()
                .contains("[Plugin-origin claude-post]")
        );
    }
}

pub(super) fn provider_contents() -> Vec<Value> {
    vec![
        serde_json::from_str(r##"[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC"}}]"##).unwrap(),
        serde_json::from_str(r##"[{"type":"image","source":{"type":"url","url":"https://example.invalid/image.png"}}]"##).unwrap(),
        serde_json::from_str(r##"[{"type":"document","source":{"type":"text","media_type":"text/plain","data":"fixture document"},"title":"Fixture"}]"##).unwrap(),
        serde_json::from_str(r##"[{"type":"search_result","source":"https://example.invalid/fixture","title":"Fixture","content":[{"type":"text","text":"fixture search evidence"}],"citations":{"enabled":true}}]"##).unwrap(),
        serde_json::from_str(r##"[{"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA3MiA3Ml0gL0NvbnRlbnRzIDQgMCBSID4+CmVuZG9iago0IDAgb2JqCjw8IC9MZW5ndGggMCA+PgpzdHJlYW0KCmVuZHN0cmVhbQplbmRvYmoKeHJlZgowIDUKMDAwMDAwMDAwMCA2NTUzNSBmIAowMDAwMDAwMDA5IDAwMDAwIG4gCjAwMDAwMDAwNTggMDAwMDAgbiAKMDAwMDAwMDExNSAwMDAwMCBuIAowMDAwMDAwMjAwIDAwMDAwIG4gCnRyYWlsZXIKPDwgL1NpemUgNSAvUm9vdCAxIDAgUiA+PgpzdGFydHhyZWYKMjQ5CiUlRU9GCg=="}}]"##).unwrap(),
        serde_json::from_str(r##"[{"type":"document","source":{"type":"url","url":"https://example.invalid/fixture.pdf"}}]"##).unwrap(),
        serde_json::from_str(r##"[{"type":"document","source":{"type":"content","content":[{"type":"text","text":"fixture document"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4//8/AAX+Av4N70a4AAAAAElFTkSuQmCC"}}]}}]"##).unwrap(),
        json!([]),
    ]
}

#[tokio::test]
async fn claude_structured_replacement_hold_preserves_original_and_withholds_delivery() {
    let _lock = FIXTURE.lock().await;
    let content = provider_contents().remove(0);
    let (f, end, _) = case_with_content("structured-hold", false, Some(content.clone())).await;
    assert!(end.is_err());
    assert!(!f.root.path().join("presentation.json").exists());
    assert!(!f.root.path().join("next-model").exists());
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
    assert!(matches!(
        post.continuation,
        demoncoder::plugins::receipts::PostContinuation::Held { .. }
    ));
    assert_eq!(post.model_content, Some(content));
}

#[tokio::test]
async fn claude_unqualified_capabilities_hold_without_mislabeling_valid_shapes() {
    let _lock = FIXTURE.lock().await;
    for content in [
        json!([{"type":"tool_reference","tool_name":"mcp__demoncoder__write"}]),
        json!([{"type":"image","source":{"type":"file","file_id":"provider-file"}}]),
        json!([{"type":"document","source":{"type":"file","file_id":"provider-file"}}]),
    ] {
        let (f, end, _) = case_with_content("structured", false, Some(content.clone())).await;
        assert!(end.is_err());
        assert!(!f.root.path().join("presentation.json").exists());
        let record = f.record();
        let operation = record.operations.iter().find(|o| o.call.is_some()).unwrap();
        assert!(operation.result.as_ref().unwrap().success);
        let post = operation
            .tool_receipt
            .as_ref()
            .unwrap()
            .plugin_lifecycle
            .as_ref()
            .unwrap();
        assert!(
            matches!(&post.continuation, demoncoder::plugins::receipts::PostContinuation::Held { reason } if reason.contains("source capability unavailable"))
        );
        assert!(post.model_content.is_none());
        assert!(
            post.proposals
                .iter()
                .filter(|p| matches!(
                    p.proposal.kind,
                    demoncoder::plugins::receipts::ProposalKind::ReplaceModelOutput
                ))
                .all(|p| matches!(
                    p.disposition,
                    demoncoder::plugins::receipts::ProposalDisposition::Held
                ))
        );
        let RawOutcome::Callback { value } = post.hooks[0].outcome.as_ref().unwrap() else {
            panic!("retained callback")
        };
        assert_eq!(value["hookSpecificOutput"]["updatedMCPToolOutput"], content);
    }
}

#[tokio::test]
async fn claude_resumed_source_conversation_retains_search_citation_policy() {
    let _lock = FIXTURE.lock().await;
    let mut content = provider_contents().remove(3);
    content[0]["citations"] = json!({"enabled":true});
    let (f, end, count) =
        case_with_content("search-policy-change", false, Some(content.clone())).await;
    assert!(end.is_err());
    assert_eq!(count.load(Ordering::SeqCst), 2);
    let record = f.record();
    let operations: Vec<_> = record
        .operations
        .iter()
        .filter(|o| o.call.is_some())
        .collect();
    assert_eq!(operations.len(), 2);
    assert!(
        operations
            .iter()
            .all(|operation| operation.result.as_ref().unwrap().success)
    );
    let first = operations[0]
        .tool_receipt
        .as_ref()
        .unwrap()
        .plugin_lifecycle
        .as_ref()
        .unwrap();
    let second = operations[1]
        .tool_receipt
        .as_ref()
        .unwrap()
        .plugin_lifecycle
        .as_ref()
        .unwrap();
    assert_eq!(first.model_content, Some(content));
    assert_eq!(
        first.delivery,
        demoncoder::plugins::receipts::PostDelivery::Acknowledged
    );
    assert!(
        matches!(&second.continuation, demoncoder::plugins::receipts::PostContinuation::Held { reason } if reason.contains("request compatibility"))
    );
    assert!(second.model_content.is_none());
}

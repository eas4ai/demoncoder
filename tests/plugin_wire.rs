use demoncoder::plugins::{
    hook_types::{HandlerKind, HookDialect, HookEvent},
    profile::CompatibilityProfile,
};
use serde_json::json;

#[test]
fn strict_model_verdict_and_exact_applicability() {
    let p = CompatibilityProfile::embedded().unwrap();
    assert!(
        p.validate_model(
            HookDialect::Claude,
            HandlerKind::Prompt,
            &json!({"ok":false})
        )
        .is_err()
    );
    assert!(
        p.validate_model(
            HookDialect::Claude,
            HandlerKind::Prompt,
            &json!({"ok":false,"reason":"blocked"})
        )
        .is_ok()
    );
    assert!(
        p.require_runner(HookDialect::Codex, HookEvent::Stop, HandlerKind::Prompt)
            .is_err()
    );
    assert!(
        p.require_runner(
            HookDialect::Native,
            HookEvent::Interrupt,
            HandlerKind::Agent
        )
        .is_ok()
    );
}

#[test]
fn inherited_nested_fields_and_event_identity_are_enforced() {
    let p = CompatibilityProfile::embedded().unwrap();
    let mut input = json!({"session_id":"s","transcript_path":"t","cwd":"/w","hook_event_name":"Stop","stop_hook_active":false,"effort":{"level":"high"}});
    p.validate_claude_input(HookEvent::Stop, &input).unwrap();
    input["effort"] = json!({});
    assert!(p.validate_claude_input(HookEvent::Stop, &input).is_err());
    input["effort"] = json!({"level":"high"});
    assert!(
        p.validate_claude_input(HookEvent::PreToolUse, &input)
            .is_err()
    );
    input["secret-canary"] = json!("secret-canary");
    let error = p
        .validate_claude_input(HookEvent::Stop, &input)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("secret-canary"));
}

use demoncoder::plugins::{
    hook_types::{Applicability, ModelCallContext, ModelOutcome, TaskBoundary},
    profile::SchemaKey,
    wire,
};

#[test]
fn all_510_source_cells_have_independent_expected_status() {
    use Applicability::*;
    use HandlerKind::*;
    use HookDialect::*;
    use HookEvent::*;
    let p = CompatibilityProfile::embedded().unwrap();
    let claude_model = [
        PreToolUse,
        PostToolUse,
        PostToolUseFailure,
        PostToolBatch,
        UserPromptSubmit,
        UserPromptExpansion,
        Stop,
        SubagentStop,
        PermissionRequest,
        PermissionDenied,
        TeammateIdle,
        TaskCreated,
        TaskCompleted,
    ];
    let codex_events = [
        PreToolUse,
        PostToolUse,
        UserPromptSubmit,
        SessionStart,
        SessionEnd,
        Stop,
        SubagentStart,
        SubagentStop,
        PreCompact,
        PostCompact,
        PermissionRequest,
        Interrupt,
    ];
    let mut count = 0;
    for &dialect in HookDialect::ALL {
        for &event in HookEvent::ALL {
            for &handler in HandlerKind::ALL {
                let expected = match dialect {
                    Native => Run,
                    Claude if event == Interrupt => NoSourceEvent,
                    Claude if claude_model.contains(&event) => Run,
                    Claude if matches!(handler, Prompt | Agent) => NoSourceHandler,
                    Claude if matches!(event, SessionStart | Setup) && handler == Http => {
                        NoSourceHandler
                    }
                    Claude => Run,
                    Codex if !codex_events.contains(&event) => NoSourceEvent,
                    Codex if matches!(handler, Prompt | Agent) => SourceNonexecuting,
                    Codex if handler == Http => NoSourceHandler,
                    Codex => Run,
                };
                assert_eq!(
                    p.applicability(dialect, event, handler),
                    expected,
                    "{dialect:?}/{event:?}/{handler:?}"
                );
                assert_eq!(
                    p.require_runner(dialect, event, handler).is_ok(),
                    expected == Run
                );
                count += 1;
            }
        }
    }
    assert_eq!(count, 510);
}

#[test]
fn full_schemas_keep_nested_refs_one_of_all_of_and_distinct_definitions() {
    let p = CompatibilityProfile::embedded().unwrap();
    assert_eq!(p.schema_keys().count(), 29);
    let core = |name: &str| SchemaKey::Codex {
        path: "codex-rs/core/config.schema.json".into(),
        definition: Some(name.into()),
    };
    p.validate_schema(
        &core("HookHandlerConfig"),
        &json!({"type":"command","command":"true"}),
    )
    .unwrap();
    assert!(
        p.validate_schema(&core("HookHandlerConfig"), &json!({"type":"command"}))
            .is_err()
    );
    for kind in ["prompt", "agent"] {
        p.validate_schema(&core("HookHandlerConfig"), &json!({"type":kind}))
            .unwrap();
    }
    p.validate_schema(
        &core("MatcherGroup"),
        &json!({"hooks":[{"type":"mcp_tool","server":"s","tool":"t"}]}),
    )
    .unwrap();
    assert!(
        p.validate_schema(
            &core("MatcherGroup"),
            &json!({"hooks":[{"type":"mcp_tool","server":"s"}]})
        )
        .is_err()
    );
    let mut config = json!({"mcp_servers":{"s":{"tools":{"t":{"approval_mode":"writes","output_token_limit":1}}}}});
    p.validate_schema(&core("PluginConfig"), &config).unwrap();
    config["mcp_servers"]["s"]["tools"]["t"]["approval_mode"] = json!("invented");
    assert!(p.validate_schema(&core("PluginConfig"), &config).is_err());
    assert!(
        p.validate_schema(
            &core("PluginMcpServerConfig"),
            &json!({"tools":{"t":{"output_token_limit":0}}})
        )
        .is_err()
    );
    assert!(
        p.validate_schema(
            &core("PluginMcpServerConfig"),
            &json!({"command":"secret-canary"})
        )
        .is_err()
    );
    assert!(
        p.validate_schema(
            &SchemaKey::Codex {
                path: "codex-rs/core/config.schema.json".into(),
                definition: None
            },
            &json!({})
        )
        .is_err()
    );
    let key = SchemaKey::Portable("mcp".into());
    for server in [
        json!({"type":"stdio","command":"test","cwd":"./work","env":{"SAFE":"value"}}),
        json!({"type":"streamable-http","url":"https://example.test"}),
        json!({"type":"sse","url":"https://example.test"}),
    ] {
        p.validate_schema(&key,&json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"test":server}})).unwrap();
    }
    for server in [
        json!({"type":"stdio","command":""}),
        json!({"type":"stdio","command":"test","env":{"PLUGIN_ROOT":"override"}}),
        json!({"type":"stdio","command":"test","url":"https://example.test"}),
        json!({"type":"sse"}),
    ] {
        assert!(p.validate_schema(&key,&json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/mcp.schema.json","mcpServers":{"test":server}})).is_err());
    }
    let key = SchemaKey::Portable("plugin".into());
    for name in ["good-name", "good.name"] {
        p.validate_schema(&key,&json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":name})).unwrap();
    }
    for name in ["bad--name", "bad..name", "Bad", ""] {
        assert!(p.validate_schema(&key,&json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":name})).is_err());
    }
}

#[test]
fn bounded_json_rejects_duplicate_keys_depth_and_secret_echo() {
    assert!(wire::parse_json(br#"{"ok":true,"ok":false}"#).is_err());
    assert!(wire::parse_json(b"{} {}").is_err());
    assert!(
        !wire::parse_json(br#"{"outer":{"secret-canary":1,"secret-canary":2}}"#)
            .unwrap_err()
            .to_string()
            .contains("secret-canary")
    );
    assert!(wire::parse_json(&vec![b' '; wire::MAX_WIRE_BYTES + 1]).is_err());
    let nested = format!("{}0{}", "[".repeat(50), "]".repeat(50));
    assert!(wire::parse_json(nested.as_bytes()).is_err());
    let p = CompatibilityProfile::embedded().unwrap();
    let work_heavy = json!({"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"fixture","extensions":{"test":{"values":vec![serde_json::Value::Null;3000]}}});
    let error = p
        .validate_schema(&SchemaKey::Portable("plugin".into()), &work_heavy)
        .unwrap_err();
    assert_eq!(error.problem, "schema validation work limit exceeded");
    let err = p
        .validate_model_bytes(
            HookDialect::Claude,
            HandlerKind::Prompt,
            br#"{"ok":"secret-canary"}"#,
        )
        .unwrap_err();
    assert!(!err.to_string().contains("secret-canary"));
    assert!(p.validate_claude_type("HookCallback", &json!({})).is_err());
    assert!(
        p.validate_claude_type("HookCallbackMatcher", &json!({"hooks":[{}]}))
            .is_err()
    );
}

#[test]
fn strict_model_schemas_for_every_dialect_and_runner() {
    let p = CompatibilityProfile::embedded().unwrap();
    for dialect in [HookDialect::Claude, HookDialect::Native] {
        for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
            for good in [json!({"ok":true}), json!({"ok":false,"reason":"blocked"})] {
                p.validate_model(dialect, kind, &good).unwrap();
            }
            for bad in [
                json!({}),
                json!({"ok":false}),
                json!({"ok":"true"}),
                json!({"ok":false,"reason":3}),
                json!({"ok":true,"continueOnBlock":true}),
                json!({"ok":true,"teammate-stop":true}),
            ] {
                assert!(p.validate_model(dialect, kind, &bad).is_err());
            }
            assert_eq!(
                p.validate_model(
                    dialect,
                    kind,
                    &json!({"ok":false,"reason":"blocked","impossible":true})
                )
                .is_ok(),
                kind == HandlerKind::Prompt
            );
        }
    }
    assert!(
        p.validate_model(HookDialect::Codex, HandlerKind::Prompt, &json!({"ok":true}))
            .is_err()
    );
}

#[test]
fn model_table_obeys_event_context_and_never_approves_false() {
    use HookEvent::*;
    use ModelOutcome::*;
    let p = CompatibilityProfile::embedded().unwrap();
    for &dialect in [HookDialect::Claude, HookDialect::Native].iter() {
        for &event in HookEvent::ALL {
            for kind in [HandlerKind::Prompt, HandlerKind::Agent] {
                if p.require_runner(dialect, event, kind).is_err() {
                    continue;
                }
                for ok in [false, true] {
                    for impossible in [false, true] {
                        for continuation in [false, true] {
                            for boundary in
                                [TaskBoundary::ToolTransition, TaskBoundary::TeammateStop]
                            {
                                if kind == HandlerKind::Agent && impossible {
                                    continue;
                                }
                                if continuation
                                    && (kind == HandlerKind::Agent
                                        || dialect == HookDialect::Native)
                                {
                                    continue;
                                }
                                let mut response = json!({"ok":ok,"reason":"fixture"});
                                if kind == HandlerKind::Prompt {
                                    response["impossible"] = json!(impossible);
                                }
                                let verdict = p.validate_model(dialect, kind, &response).unwrap();
                                let context = ModelCallContext {
                                    continue_on_block: continuation,
                                    task_boundary: boundary,
                                    ..Default::default()
                                };
                                let expected = if dialect == HookDialect::Claude
                                    && matches!(event, PermissionRequest | PermissionDenied)
                                {
                                    NoSourceDecision
                                } else if ok {
                                    NoModelObjection
                                } else if dialect == HookDialect::Native {
                                    match event {
                                        Stop | SubagentStop if impossible => StopUnmet,
                                        PostToolUse | PostToolUseFailure | Stop | SubagentStop
                                        | PostCompact | TeammateIdle => BoundedCorrection,
                                        PreToolUse | UserPromptSubmit | UserPromptExpansion
                                        | PreCompact | PreModelSwitch | PermissionRequest
                                        | TaskCreated | TaskCompleted | Elicitation
                                        | ElicitationResult | ConfigChange | WorktreeCreate => {
                                            HoldPendingAction
                                        }
                                        _ => AttributedObservationOnly,
                                    }
                                } else {
                                    match event {
                                        PreToolUse
                                            if kind == HandlerKind::Agent || continuation =>
                                        {
                                            DenyToolAndContinue
                                        }
                                        PreToolUse => DenyToolAndEndTurn,
                                        PostToolUse
                                            if kind == HandlerKind::Agent || continuation =>
                                        {
                                            ContinueAfterResult
                                        }
                                        PostToolUse | PostToolBatch | UserPromptSubmit
                                        | UserPromptExpansion => EndTurnUnmet,
                                        PostToolUseFailure => ContinueWithFailure,
                                        Stop | SubagentStop if impossible => StopUnmet,
                                        Stop | SubagentStop => BoundedCorrection,
                                        TaskCreated => RejectAndContinue,
                                        TaskCompleted
                                            if boundary == TaskBoundary::ToolTransition =>
                                        {
                                            RejectAndContinue
                                        }
                                        TeammateIdle | TaskCompleted
                                            if kind == HandlerKind::Agent || continuation =>
                                        {
                                            KeepWorking
                                        }
                                        TeammateIdle | TaskCompleted => StopUnmet,
                                        _ => panic!(
                                            "missing independent expected model event {event:?}"
                                        ),
                                    }
                                };
                                assert_eq!(
                                    p.model_outcome(event, &verdict, &context).unwrap(),
                                    expected,
                                    "{dialect:?}/{event:?}/{kind:?}/{ok}/{impossible}/{continuation}/{boundary:?}"
                                );
                                if !ok {
                                    assert_ne!(expected, NoModelObjection);
                                }
                                if matches!(
                                    expected,
                                    ContinueAfterResult
                                        | ContinueWithFailure
                                        | DenyToolAndContinue
                                        | BoundedCorrection
                                        | RejectAndContinue
                                        | KeepWorking
                                ) {
                                    for stopped in [
                                        ModelCallContext {
                                            cancelled: true,
                                            ..context
                                        },
                                        ModelCallContext {
                                            allocation_available: false,
                                            ..context
                                        },
                                        ModelCallContext {
                                            correction_available: false,
                                            ..context
                                        },
                                    ] {
                                        assert_eq!(
                                            p.model_outcome(event, &verdict, &stopped).unwrap(),
                                            StopUnmet
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        !p.model_can_gate(HookDialect::Claude, PermissionRequest, HandlerKind::Prompt)
            .unwrap()
    );
    assert!(
        !p.model_can_gate(HookDialect::Native, Notification, HandlerKind::Agent)
            .unwrap()
    );
    assert!(
        p.model_can_gate(HookDialect::Native, WorktreeCreate, HandlerKind::Prompt)
            .unwrap()
    );
}

#[test]
fn every_serializable_graph_type_field_and_union_fixture_uses_production_validation() {
    let p = CompatibilityProfile::embedded().unwrap();
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/plugin-wire/claude-graph.json")).unwrap();
    for (i, fixture) in fixtures.iter().enumerate() {
        let name = fixture["type"].as_str().unwrap();
        p.validate_claude_type(name, &fixture["value"])
            .unwrap_or_else(|e| panic!("fixture {i} {name}: {e}"));
        assert!(
            p.validate_claude_type(name, &json!(false)).is_err(),
            "root type {name} accepts invalid Boolean"
        );
    }
}

struct EchoCallback;
impl wire::SdkHookCallback for EchoCallback {
    fn call<'a>(
        &'a self,
        input: serde_json::Value,
        tool_use_id: Option<String>,
        signal: wire::CallbackSignal,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = wire::WireResult<serde_json::Value>> + Send + 'a>,
    > {
        Box::pin(async move {
            assert_eq!(input["hook_event_name"], "Stop");
            assert!(!signal.is_cancelled());
            assert!(tool_use_id.is_none() || tool_use_id.as_deref() == Some("tool-id"));
            Ok(json!({"continue":true}))
        })
    }
}
#[tokio::test]
async fn sdk_callback_function_promise_undefined_and_abort_signal_remain_typed() {
    let p = CompatibilityProfile::embedded().unwrap();
    let input = json!({"session_id":"s","transcript_path":"t","cwd":"/w","hook_event_name":"Stop","stop_hook_active":false});
    let matcher = wire::CallbackMatcher {
        matcher: Some("tool".into()),
        hooks: vec![std::sync::Arc::new(EchoCallback)],
        timeout: Some(10.0),
    };
    p.validate_callback_matcher(&matcher).unwrap();
    for id in [None, Some("tool-id".into())] {
        let signal = wire::CallbackSignal::default();
        p.validate_callback_input(HookEvent::Stop, &input, id.as_deref(), &signal)
            .unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            matcher.hooks[0].call(input.clone(), id, signal),
        )
        .await
        .unwrap()
        .unwrap();
        p.validate_claude_output(HookEvent::Stop, &result).unwrap();
        assert_eq!(result["continue"], true);
    }
    let signal = wire::CallbackSignal::default();
    signal.cancel();
    assert!(
        p.validate_callback_input(HookEvent::Stop, &input, None, &signal)
            .is_err()
    );
    let invalid = wire::CallbackMatcher {
        timeout: Some(f64::NAN),
        ..matcher
    };
    assert!(p.validate_callback_matcher(&invalid).is_err());
    assert!(
        p.validate_claude_output(
            HookEvent::Stop,
            &json!({"hookSpecificOutput":{"hookEventName":"PreToolUse"}})
        )
        .is_err()
    );
}

#[test]
fn all_full_schema_fixtures_validate_with_their_exact_key() {
    let p = CompatibilityProfile::embedded().unwrap();
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/plugin-wire/full-schemas.json")).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for (i, f) in fixtures.iter().enumerate() {
        let key = if let Some(name) = f["key"]["portable"].as_str() {
            SchemaKey::Portable(name.into())
        } else {
            SchemaKey::Codex {
                path: f["key"]["codex"].as_str().unwrap().into(),
                definition: f["key"]["definition"].as_str().map(str::to_owned),
            }
        };
        p.validate_schema(&key, &f["value"])
            .unwrap_or_else(|e| panic!("schema fixture {i} {key:?}: {e}"));
        assert!(p.validate_schema(&key, &json!(false)).is_err());
        seen.insert(key);
    }
    assert_eq!(seen.len(), p.schema_keys().count());
}

#[test]
fn callback_ids_and_matchers_use_borrowed_string_limits() {
    let profile = CompatibilityProfile::embedded().unwrap();
    let input = json!({"session_id":"s","transcript_path":"t","cwd":"/w","hook_event_name":"Stop","stop_hook_active":false});
    let signal = wire::CallbackSignal::default();
    let boundary = (wire::MAX_WIRE_BYTES - 8) / 6;
    let mut text = "s".repeat(boundary + 1);
    profile
        .validate_callback_input(HookEvent::Stop, &input, Some(&text[..boundary]), &signal)
        .unwrap();
    let id_error = profile
        .validate_callback_input(HookEvent::Stop, &input, Some(&text), &signal)
        .unwrap_err();
    let mut matcher = wire::CallbackMatcher {
        matcher: Some(std::mem::take(&mut text)),
        hooks: vec![],
        timeout: None,
    };
    let matcher_error = profile.validate_callback_matcher(&matcher).unwrap_err();
    assert_eq!(id_error, matcher_error);
    assert_eq!(id_error.problem, "wire byte limit exceeded");
    assert_eq!(id_error.path, "/");
    matcher.matcher.as_mut().unwrap().truncate(boundary);
    profile.validate_callback_matcher(&matcher).unwrap();
}

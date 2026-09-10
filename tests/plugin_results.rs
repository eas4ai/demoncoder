use demoncoder::plugins::{
    hook_types::{HandlerKind, HookDialect, HookEvent, ModelCallContext, TaskBoundary},
    profile::CompatibilityProfile,
    results::*,
};
use serde_json::{Value, json};

fn context() -> ResultContext {
    ResultContext::default()
}
fn decode(
    dialect: HookDialect,
    event: HookEvent,
    ctx: &ResultContext,
    output: &Value,
) -> DecodedResult {
    decode_response(
        &CompatibilityProfile::embedded().unwrap(),
        dialect,
        event,
        HandlerKind::Command,
        ctx,
        HookResponse::Callback(output),
    )
}
fn command(
    dialect: HookDialect,
    event: HookEvent,
    ctx: &ResultContext,
    exit: i32,
    stdout: &str,
    stderr: &str,
) -> DecodedResult {
    decode_response(
        &CompatibilityProfile::embedded().unwrap(),
        dialect,
        event,
        HandlerKind::Command,
        ctx,
        HookResponse::Command {
            exit_code: Some(exit),
            stdout: stdout.as_bytes(),
            stderr: stderr.as_bytes(),
        },
    )
}
fn has(result: &DecodedResult, expected: ProposedEffect) {
    assert!(
        result.effects.contains(&expected),
        "missing {expected:?}: {result:?}"
    );
}

#[test]
fn required_failure_holds_but_observer_retains_failure_without_reversing_receipt() {
    for role in [ResultRole::RequiredGate, ResultRole::Observer] {
        let ctx = ResultContext { role, ..context() };
        let result = decode_response(
            &CompatibilityProfile::embedded().unwrap(),
            HookDialect::Claude,
            HookEvent::PreToolUse,
            HandlerKind::Command,
            &ctx,
            HookResponse::Failure(TransportFailure::Timeout),
        );
        assert!(result.failed());
        assert_eq!(
            result.gate,
            if role == ResultRole::RequiredGate {
                GateDisposition::Held
            } else {
                GateDisposition::NotAGate
            }
        );
        assert!(result.effects.is_empty());
    }
}

#[test]
fn rewrites_are_data_and_exit_two_cannot_become_permission() {
    let value = json!({"hookSpecificOutput":{"hookEventName":"PreToolUse", "permissionDecision":"allow", "updatedInput":{"path":"/outside"}}});
    let result = command(
        HookDialect::Claude,
        HookEvent::PreToolUse,
        &context(),
        2,
        &value.to_string(),
        "denied",
    );
    assert!(result.failed());
    assert_eq!(result.gate, GateDisposition::Held);
    has(
        &result,
        ProposedEffect::RewriteInput(Untrusted::new(json!({"path":"/outside"}))),
    );
    has(
        &result,
        ProposedEffect::Decision {
            choice: DecisionChoice::Deny,
            reason: Some(Untrusted::new("denied".into())),
        },
    );
    assert!(!result.effects.iter().any(|e| matches!(
        e,
        ProposedEffect::Decision {
            choice: DecisionChoice::NoObjection,
            ..
        }
    )));
}

#[test]
fn nonzero_claude_json_effects_survive_without_releasing_required_gate() {
    let result = command(
        HookDialect::Claude,
        HookEvent::PreToolUse,
        &context(),
        1,
        r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"hint"}}"#,
        "",
    );
    assert!(result.failed());
    assert_eq!(result.gate, GateDisposition::Held);
    has(
        &result,
        ProposedEffect::AdditionalContext(Untrusted::new("hint".into())),
    );
    let codex = command(
        HookDialect::Codex,
        HookEvent::PreToolUse,
        &context(),
        1,
        r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","additionalContext":"hint"}}"#,
        "",
    );
    assert!(codex.effects.is_empty());
}

#[test]
fn codex_rejects_schema_valid_reserved_semantics_and_uses_specific_precedence() {
    for value in [
        json!({"continue":false}),
        json!({"stopReason":"stop"}),
        json!({"suppressOutput":true}),
        json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}),
        json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask"}}),
        json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":{}}}),
        json!({"decision":"block", "reason":" "}),
    ] {
        let result = decode(
            HookDialect::Codex,
            HookEvent::PreToolUse,
            &context(),
            &value,
        );
        assert!(result.failed(), "{value}");
        assert_eq!(result.gate, GateDisposition::Held);
    }
    let result = decode(
        HookDialect::Codex,
        HookEvent::PreToolUse,
        &context(),
        &json!({
            "decision":"block", "reason":"legacy", "hookSpecificOutput":{
                "hookEventName":"PreToolUse", "permissionDecision":"allow", "updatedInput":{}
            }
        }),
    );
    assert!(!result.failed(), "{result:?}");
    assert_eq!(result.gate, GateDisposition::NoObjection);
    has(
        &result,
        ProposedEffect::RewriteInput(Untrusted::new(json!({}))),
    );
}

#[test]
fn nested_watch_replacement_is_atomic_including_explicit_empty_clear() {
    let ctx = ResultContext {
        role: ResultRole::Observer,
        ..context()
    };
    for event in [
        HookEvent::SessionStart,
        HookEvent::FileChanged,
        HookEvent::CwdChanged,
    ] {
        for paths in [json!([]), json!(["/workspace/src", "/workspace/tests"])] {
            let result = decode(
                HookDialect::Claude,
                event,
                &ctx,
                &json!({"hookSpecificOutput":{
                    "hookEventName": event.as_str(), "watchPaths": paths
                }}),
            );
            assert!(!result.failed(), "{result:?}");
            let expected = paths
                .as_array()
                .unwrap()
                .iter()
                .map(|p| std::path::PathBuf::from(p.as_str().unwrap()))
                .collect();
            has(&result, ProposedEffect::ReplaceDynamicWatches(expected));
        }
        let invalid = decode(
            HookDialect::Claude,
            event,
            &ctx,
            &json!({"hookSpecificOutput":{
                "hookEventName": event.as_str(), "watchPaths":["/workspace/ok", 12]
            }}),
        );
        assert!(invalid.failed());
        assert!(invalid.effects.is_empty());
    }
}

#[test]
fn worktree_paths_require_actual_transport_specific_value_and_host_validation() {
    let result = command(
        HookDialect::Native,
        HookEvent::WorktreeCreate,
        &context(),
        0,
        "starting worktree\n/workspace/child\n\n",
        "",
    );
    has(
        &result,
        ProposedEffect::WorktreePath(std::path::PathBuf::from("/workspace/child")),
    );
    for (exit, stdout) in [(0, ""), (0, "relative"), (1, "/workspace/child")] {
        assert!(
            command(
                HookDialect::Native,
                HookEvent::WorktreeCreate,
                &context(),
                exit,
                stdout,
                ""
            )
            .failed()
        );
    }
    let result = decode(
        HookDialect::Native,
        HookEvent::WorktreeCreate,
        &context(),
        &json!({
            "hookSpecificOutput":{"hookEventName":"WorktreeCreate","worktreePath":"/workspace/child"}
        }),
    );
    assert!(!result.failed(), "{result:?}");
    has(
        &result,
        ProposedEffect::WorktreePath(std::path::PathBuf::from("/workspace/child")),
    );
    assert!(
        decode(
            HookDialect::Native,
            HookEvent::WorktreeCreate,
            &context(),
            &json!({"ok":true})
        )
        .failed()
    );
}

#[test]
fn elicitation_proposals_are_plugin_origin_and_exit_two_ignores_accept_content() {
    let value = json!({"hookSpecificOutput":{"hookEventName":"Elicitation","action":"accept", "content":{"answer":"yes"}}});
    let result = decode(
        HookDialect::Claude,
        HookEvent::Elicitation,
        &context(),
        &value,
    );
    has(
        &result,
        ProposedEffect::Elicitation {
            action: Some(ElicitationAction::Accept),
            content: Some(Untrusted::new(json!({"answer":"yes"}))),
        },
    );
    let blocked = command(
        HookDialect::Claude,
        HookEvent::Elicitation,
        &context(),
        2,
        &value.to_string(),
        "no",
    );
    has(
        &blocked,
        ProposedEffect::Elicitation {
            action: Some(ElicitationAction::Decline),
            content: None,
        },
    );
    assert!(!blocked.effects.iter().any(|e| matches!(
        e,
        ProposedEffect::Elicitation {
            action: Some(ElicitationAction::Accept),
            ..
        }
    )));
}

#[test]
fn stop_followup_uses_owner_allocation_and_cannot_veto_cancel() {
    let value = json!({"decision":"block", "reason":"fix tests"});
    let active = decode(HookDialect::Claude, HookEvent::Stop, &context(), &value);
    has(
        &active,
        ProposedEffect::Control(ControlRequest::Followup(FollowupTarget::Task)),
    );
    for state in [
        ModelCallContext {
            cancelled: true,
            ..Default::default()
        },
        ModelCallContext {
            allocation_available: false,
            ..Default::default()
        },
        ModelCallContext {
            correction_available: false,
            ..Default::default()
        },
    ] {
        let ctx = ResultContext {
            work: state,
            ..context()
        };
        let result = decode(HookDialect::Claude, HookEvent::Stop, &ctx, &value);
        has(&result, ProposedEffect::Control(ControlRequest::StopUnmet));
        assert!(
            !result
                .effects
                .iter()
                .any(|e| matches!(e, ProposedEffect::Control(ControlRequest::Followup(_))))
        );
    }
}

#[test]
fn task_completion_boundary_is_host_owned_and_continue_is_ignored_for_tool_transition() {
    for boundary in [TaskBoundary::ToolTransition, TaskBoundary::TeammateStop] {
        let ctx = ResultContext {
            work: ModelCallContext {
                task_boundary: boundary,
                ..Default::default()
            },
            ..context()
        };
        let result = decode(
            HookDialect::Claude,
            HookEvent::TaskCompleted,
            &ctx,
            &json!({"continue":false,"stopReason":"done"}),
        );
        assert!(!result.failed());
        if boundary == TaskBoundary::ToolTransition {
            assert!(result.effects.is_empty());
            assert!(!result.diagnostics.is_empty());
        } else {
            has(&result, ProposedEffect::Control(ControlRequest::EndTurn));
            assert_eq!(result.gate, GateDisposition::Held);
        }
    }
}

#[test]
fn mcp_prefers_structured_and_does_not_fallback_after_invalid_structure() {
    let profile = CompatibilityProfile::embedded().unwrap();
    let value =
        json!({"hookSpecificOutput":{"hookEventName":"PreToolUse", "permissionDecision":"deny"}});
    let text =
        [r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}"#];
    let result = decode_response(
        &profile,
        HookDialect::Claude,
        HookEvent::PreToolUse,
        HandlerKind::McpTool,
        &context(),
        HookResponse::Mcp {
            structured: Some(&value),
            text: &text,
            is_error: false,
        },
    );
    has(
        &result,
        ProposedEffect::Decision {
            choice: DecisionChoice::Deny,
            reason: None,
        },
    );
    for value in [json!([]), json!("not an object"), json!({"unknown":true})] {
        let result = decode_response(
            &profile,
            HookDialect::Claude,
            HookEvent::PreToolUse,
            HandlerKind::McpTool,
            &context(),
            HookResponse::Mcp {
                structured: Some(&value),
                text: &text,
                is_error: false,
            },
        );
        assert!(result.failed());
        assert!(result.effects.is_empty());
    }
}

#[test]
fn unsafe_terminal_sequences_are_never_forwarded_and_safe_vocabulary_is_typed() {
    let ctx = ResultContext {
        role: ResultRole::Observer,
        interactive_display: true,
        ..context()
    };
    for value in [
        "\u{7}",
        "\u{1b}]777;notify;Title;body\u{7}",
        "\u{1b}]2;title\u{1b}\\",
    ] {
        let result = decode(
            HookDialect::Claude,
            HookEvent::Notification,
            &ctx,
            &json!({"terminalSequence":value}),
        );
        assert!(
            result
                .effects
                .iter()
                .any(|e| matches!(e, ProposedEffect::TerminalNotification(_))),
            "{result:?}"
        );
    }
    for value in [
        "\u{1b}[2J",
        "\u{1b}]52;c;secret\u{7}",
        "plain",
        "\u{1b}]9;hello\nworld\u{7}",
    ] {
        let result = decode(
            HookDialect::Claude,
            HookEvent::Notification,
            &ctx,
            &json!({"terminalSequence":value}),
        );
        assert!(
            !result
                .effects
                .iter()
                .any(|e| matches!(e, ProposedEffect::TerminalNotification(_)))
        );
        assert!(!result.diagnostics.is_empty());
    }
}

#[test]
fn source_nonexecuting_pairs_do_not_silently_use_native() {
    let result = decode_response(
        &CompatibilityProfile::embedded().unwrap(),
        HookDialect::Codex,
        HookEvent::PreToolUse,
        HandlerKind::Http,
        &context(),
        HookResponse::Http {
            status: 200,
            body: b"{}",
        },
    );
    assert!(result.failed());
    assert_eq!(result.gate, GateDisposition::Held);
    let native = decode_response(
        &CompatibilityProfile::embedded().unwrap(),
        HookDialect::Native,
        HookEvent::PreToolUse,
        HandlerKind::Http,
        &context(),
        HookResponse::Http {
            status: 200,
            body: b"{}",
        },
    );
    assert!(!native.failed(), "{native:?}");
}

// Independent source inventory, intentionally not read from require_runner or its
// applicability map. Each cell invokes the production decoder with real bytes.
fn supported(dialect: HookDialect, event: HookEvent, handler: HandlerKind) -> bool {
    match dialect {
        HookDialect::Native => true,
        HookDialect::Claude => {
            event != HookEvent::Interrupt
                && !(handler == HandlerKind::Http
                    && matches!(event, HookEvent::SessionStart | HookEvent::Setup))
        }
        HookDialect::Codex => {
            matches!(handler, HandlerKind::Command | HandlerKind::McpTool)
                && matches!(
                    event,
                    HookEvent::PreToolUse
                        | HookEvent::PostToolUse
                        | HookEvent::PermissionRequest
                        | HookEvent::UserPromptSubmit
                        | HookEvent::SessionStart
                        | HookEvent::SessionEnd
                        | HookEvent::Stop
                        | HookEvent::SubagentStart
                        | HookEvent::SubagentStop
                        | HookEvent::PreCompact
                        | HookEvent::PostCompact
                        | HookEvent::Interrupt
                )
        }
    }
}
fn observer_event(event: HookEvent) -> bool {
    matches!(
        event,
        HookEvent::SessionStart
            | HookEvent::InstructionsLoaded
            | HookEvent::PermissionDenied
            | HookEvent::PostToolBatch
            | HookEvent::StopFailure
            | HookEvent::Interrupt
            | HookEvent::SessionEnd
            | HookEvent::SubagentStart
            | HookEvent::WorktreeRemove
            | HookEvent::PostCompact
            | HookEvent::PostModelSwitch
            | HookEvent::Setup
            | HookEvent::Notification
            | HookEvent::FileChanged
            | HookEvent::CwdChanged
            | HookEvent::DirectoryAdded
            | HookEvent::MessageDisplay
    )
}
fn event_example(dialect: HookDialect, event: HookEvent) -> (Value, Vec<ProposedEffect>) {
    use HookEvent::*;
    let text = Untrusted::new("context".to_owned());
    let (specific, expected) = match event {
        SessionStart | UserPromptSubmit | UserPromptExpansion | PostToolUse
        | PostToolUseFailure | PostToolBatch | SubagentStart | PostModelSwitch | Setup
        | Notification => (
            json!({"additionalContext":"context"}),
            vec![ProposedEffect::AdditionalContext(text)],
        ),
        PreToolUse => (
            json!({"permissionDecision":"deny","permissionDecisionReason":"context"}),
            vec![ProposedEffect::Decision {
                choice: DecisionChoice::Deny,
                reason: Some(text),
            }],
        ),
        PreModelSwitch => (
            json!({"permissionDecision":"deny","permissionDecisionReason":"context"}),
            vec![ProposedEffect::Decision {
                choice: DecisionChoice::Deny,
                reason: Some(text),
            }],
        ),
        PermissionRequest => (
            json!({"decision":{"behavior":"deny","message":"context"}}),
            vec![ProposedEffect::Decision {
                choice: DecisionChoice::Deny,
                reason: Some(text),
            }],
        ),
        PermissionDenied => (
            json!({"retry":true}),
            vec![ProposedEffect::RetryDeniedOperation],
        ),
        FileChanged | CwdChanged => (
            json!({"watchPaths":[]}),
            vec![ProposedEffect::ReplaceDynamicWatches(vec![])],
        ),
        WorktreeCreate => (
            json!({"worktreePath":"/workspace/tree"}),
            vec![ProposedEffect::WorktreePath("/workspace/tree".into())],
        ),
        Elicitation | ElicitationResult => (
            json!({"action":"accept","content":{"answer":"yes"}}),
            vec![ProposedEffect::Elicitation {
                action: Some(ElicitationAction::Accept),
                content: Some(Untrusted::new(json!({"answer":"yes"}))),
            }],
        ),
        MessageDisplay => (
            json!({"displayContent":"context"}),
            vec![ProposedEffect::DisplayContent(text)],
        ),
        Stop | SubagentStop => {
            return (
                json!({"decision":"block","reason":"context"}),
                vec![
                    ProposedEffect::Feedback(text),
                    ProposedEffect::Control(ControlRequest::Followup(if event == Stop {
                        FollowupTarget::Task
                    } else {
                        FollowupTarget::Subagent
                    })),
                ],
            );
        }
        TaskCreated | ConfigChange => {
            return (
                json!({"decision":"block","reason":"context"}),
                vec![ProposedEffect::Decision {
                    choice: DecisionChoice::Deny,
                    reason: Some(text),
                }],
            );
        }
        TaskCompleted | TeammateIdle => {
            return (
                json!({"continue":false}),
                vec![ProposedEffect::Control(ControlRequest::EndTurn)],
            );
        }
        PreCompact => {
            return if dialect == HookDialect::Codex {
                (
                    json!({"continue":false}),
                    vec![ProposedEffect::Control(ControlRequest::HoldAction)],
                )
            } else {
                (
                    json!({"decision":"block","reason":"context"}),
                    vec![ProposedEffect::Decision {
                        choice: DecisionChoice::Deny,
                        reason: Some(text),
                    }],
                )
            };
        }
        PostCompact if dialect == HookDialect::Codex => {
            return (
                json!({"continue":false}),
                vec![ProposedEffect::Control(ControlRequest::HoldContinuation)],
            );
        }
        Interrupt => {
            return (
                json!({"systemMessage":"context"}),
                vec![ProposedEffect::Warning(text)],
            );
        }
        InstructionsLoaded | StopFailure | SessionEnd | WorktreeRemove | PostCompact
        | DirectoryAdded => return (json!({"continue":false}), vec![]),
    };
    let mut specific = specific;
    specific["hookEventName"] = json!(event.as_str());
    (json!({"hookSpecificOutput":specific}), expected)
}

#[test]
fn every_event_and_supported_nonmodel_transport_has_an_independent_effect_fixture() {
    let profile = CompatibilityProfile::embedded().unwrap();
    for &dialect in HookDialect::ALL {
        for &event in HookEvent::ALL {
            let ctx = ResultContext {
                role: if observer_event(event) {
                    ResultRole::Observer
                } else {
                    ResultRole::RequiredGate
                },
                work: ModelCallContext {
                    task_boundary: TaskBoundary::TeammateStop,
                    ..Default::default()
                },
                retry_eligible: true,
                ..context()
            };
            let (value, expected) = event_example(dialect, event);
            let bytes = value.to_string();
            for handler in [
                HandlerKind::Command,
                HandlerKind::Http,
                HandlerKind::McpTool,
            ] {
                let response = match handler {
                    HandlerKind::Command => HookResponse::Command {
                        exit_code: Some(0),
                        stdout: if event == HookEvent::WorktreeCreate {
                            b"/workspace/tree"
                        } else {
                            bytes.as_bytes()
                        },
                        stderr: b"",
                    },
                    HandlerKind::Http => HookResponse::Http {
                        status: 200,
                        body: bytes.as_bytes(),
                    },
                    HandlerKind::McpTool => HookResponse::Mcp {
                        structured: Some(&value),
                        text: &[],
                        is_error: false,
                    },
                    _ => unreachable!(),
                };
                let result = decode_response(&profile, dialect, event, handler, &ctx, response);
                if supported(dialect, event, handler) {
                    assert!(
                        !result.failed(),
                        "{dialect:?}/{event:?}/{handler:?}: {result:?}"
                    );
                    assert_eq!(
                        result.effects, expected,
                        "{dialect:?}/{event:?}/{handler:?}"
                    );
                    let callback = decode_response(
                        &profile,
                        dialect,
                        event,
                        handler,
                        &ctx,
                        HookResponse::Callback(&value),
                    );
                    assert!(
                        !callback.failed(),
                        "callback {dialect:?}/{event:?}/{handler:?}: {callback:?}"
                    );
                    assert_eq!(
                        callback.effects, expected,
                        "callback {dialect:?}/{event:?}/{handler:?}"
                    );
                } else {
                    assert!(
                        result.failed(),
                        "unsupported {dialect:?}/{event:?}/{handler:?}"
                    );
                    assert!(result.effects.is_empty());
                }
                assert_eq!(result.dialect, dialect);
                assert_eq!(result.event, event);
                assert_eq!(result.handler, handler);
            }
        }
    }
}

#[test]
fn every_native_observation_event_rejects_required_gate_registration() {
    let profile = CompatibilityProfile::embedded().unwrap();
    for &event in HookEvent::ALL {
        if observer_event(event) {
            for handler in [
                HandlerKind::Command,
                HandlerKind::Http,
                HandlerKind::McpTool,
            ] {
                let (value, _) = event_example(HookDialect::Native, event);
                let result = decode_response(
                    &profile,
                    HookDialect::Native,
                    event,
                    handler,
                    &context(),
                    HookResponse::Callback(&value),
                );
                assert!(result.failed(), "{event:?}/{handler:?}");
                assert_eq!(
                    result.gate,
                    if matches!(event, HookEvent::Interrupt | HookEvent::SessionEnd) {
                        GateDisposition::NotAGate
                    } else {
                        GateDisposition::Held
                    }
                );
            }
        }
    }
}

#[test]
fn malformed_exit_two_json_retains_blocking_source_effect_and_bounded_stderr() {
    for dialect in [HookDialect::Claude, HookDialect::Codex, HookDialect::Native] {
        let result = command(
            dialect,
            HookEvent::PreToolUse,
            &context(),
            2,
            r#"{"decision": }"#,
            "still denied",
        );
        assert!(result.failed());
        has(
            &result,
            ProposedEffect::Decision {
                choice: DecisionChoice::Deny,
                reason: Some(Untrusted::new("still denied".into())),
            },
        );
        assert_eq!(
            result.stderr.as_ref().map(Untrusted::get),
            Some(&"still denied".to_owned())
        );
    }
}

#[test]
fn continue_false_and_exit_two_do_not_request_contradictory_followup() {
    let result = command(
        HookDialect::Claude,
        HookEvent::Stop,
        &context(),
        2,
        r#"{"continue":false,"stopReason":"end","decision":"block","reason":"retry"}"#,
        "retry",
    );
    has(&result, ProposedEffect::Control(ControlRequest::EndTurn));
    assert!(
        !result
            .effects
            .iter()
            .any(|e| matches!(e, ProposedEffect::Control(ControlRequest::Followup(_))))
    );
}

#[test]
fn asynchronous_proposals_are_bounded_observers_and_cannot_satisfy_a_gate() {
    let value = json!({"async":true,"asyncTimeout":90_000});
    let ctx = ResultContext {
        role: ResultRole::Observer,
        observer_timeout_ms: 1000,
        ..context()
    };
    let result = decode(HookDialect::Claude, HookEvent::PreToolUse, &ctx, &value);
    assert!(!result.failed());
    has(
        &result,
        ProposedEffect::ScheduleObserver { timeout_ms: 1000 },
    );
    assert_eq!(result.gate, GateDisposition::NotAGate);
    assert_eq!(result.source_decision, SourceDecision::NoSourceDecision);
    let gate = decode(
        HookDialect::Claude,
        HookEvent::PreToolUse,
        &context(),
        &value,
    );
    assert!(gate.failed());
    assert_eq!(gate.gate, GateDisposition::Held);
    for output in [
        json!({"async":true,"asyncTimeout":-1}),
        json!({"async":true,"asyncTimeout":0}),
        json!({"asyncTimeout":1}),
        json!({"async":true,"continue":false}),
    ] {
        assert!(
            decode(HookDialect::Claude, HookEvent::PreToolUse, &ctx, &output).failed(),
            "{output}"
        );
    }
    let cancelled = ResultContext {
        work: ModelCallContext {
            cancelled: true,
            ..Default::default()
        },
        ..ctx.clone()
    };
    let result = decode(
        HookDialect::Claude,
        HookEvent::PreToolUse,
        &cancelled,
        &value,
    );
    assert!(
        !result
            .effects
            .iter()
            .any(|e| matches!(e, ProposedEffect::ScheduleObserver { .. }))
    );
    let late = ResultContext {
        asynchronous: true,
        ..ctx
    };
    let result = decode(
        HookDialect::Claude,
        HookEvent::PreToolUse,
        &late,
        &json!({"hookSpecificOutput":{
            "hookEventName":"PreToolUse", "permissionDecision":"deny", "updatedInput":{},"additionalContext":"late context"
        }}),
    );
    assert_eq!(
        result.effects,
        vec![ProposedEffect::AdditionalContext(Untrusted::new(
            "late context".into()
        ))]
    );
    assert_eq!(result.source_decision, SourceDecision::NoSourceDecision);
}

#[test]
fn session_initial_message_title_and_skill_rescan_follow_source_host_facts() {
    let value = json!({"continue":false,"suppressOutput":true,"hookSpecificOutput":{
        "hookEventName":"SessionStart","initialUserMessage":"machine task","sessionTitle":"title", "reloadSkills":true
    }});
    for source in [
        SessionStartSource::Startup,
        SessionStartSource::Resume,
        SessionStartSource::Fork,
        SessionStartSource::Clear,
        SessionStartSource::Compact,
    ] {
        for interactive in [false, true] {
            let ctx = ResultContext {
                role: ResultRole::Observer,
                session_source: source,
                interactive_session: interactive,
                ..context()
            };
            let result = decode(HookDialect::Claude, HookEvent::SessionStart, &ctx, &value);
            assert!(!result.failed());
            has(&result, ProposedEffect::StageSkillRescan);
            assert_eq!(
                result
                    .effects
                    .iter()
                    .any(|e| matches!(e, ProposedEffect::InitialMessage(_))),
                !interactive
            );
            assert_eq!(
                result
                    .effects
                    .iter()
                    .any(|e| matches!(e, ProposedEffect::SessionTitle(_))),
                !matches!(
                    source,
                    SessionStartSource::Clear | SessionStartSource::Compact
                )
            );
            assert!(
                !result
                    .effects
                    .iter()
                    .any(|e| matches!(e, ProposedEffect::Control(_)))
            );
        }
    }
}

#[test]
fn permission_update_data_and_rejected_codex_fields_do_not_grant_authority() {
    let value = json!({"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{
        "behavior":"allow","updatedInput":{"command":"data only"},"updatedPermissions":[{
            "type":"setMode","mode":"default","destination":"session"
        }]
    }}});
    for dialect in [HookDialect::Native, HookDialect::Claude] {
        let result = decode(dialect, HookEvent::PermissionRequest, &context(), &value);
        assert!(!result.failed(), "{result:?}");
        has(
            &result,
            ProposedEffect::Decision {
                choice: DecisionChoice::NoObjection,
                reason: None,
            },
        );
        has(
            &result,
            ProposedEffect::RewriteInput(Untrusted::new(json!({"command":"data only"}))),
        );
        has(
            &result,
            ProposedEffect::PermissionChanges(Untrusted::new(
                json!([{"type":"setMode","mode":"default","destination":"session"}]),
            )),
        );
    }
    for decision in [
        json!({"behavior":"allow","updatedInput":{}}),
        json!({"behavior":"allow","updatedPermissions":{}}),
        json!({"behavior":"deny","interrupt":true}),
    ] {
        assert!(
            decode(
                HookDialect::Codex,
                HookEvent::PermissionRequest,
                &context(),
                &json!({
                    "hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":decision}
                })
            )
            .failed()
        );
    }
    for dialect in [HookDialect::Claude, HookDialect::Codex] {
        let result = command(
            dialect,
            HookEvent::PermissionRequest,
            &context(),
            2,
            "",
            "declined",
        );
        assert_eq!(
            result.source_decision,
            if dialect == HookDialect::Claude {
                SourceDecision::NoSourceDecision
            } else {
                SourceDecision::Objection
            }
        );
        assert_eq!(result.gate, GateDisposition::Held);
    }
}

#[test]
fn post_tool_changes_are_explicit_untrusted_proposals() {
    let value = json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"hint",
        "classifierContext":"plugin assertion", "updatedToolOutput":{"stdout":"replacement", "failed":false,"interrupted":false}}});
    let result = decode(
        HookDialect::Claude,
        HookEvent::PostToolUse,
        &context(),
        &value,
    );
    has(
        &result,
        ProposedEffect::AdditionalContext(Untrusted::new("hint".into())),
    );
    has(
        &result,
        ProposedEffect::ClassifierContext(Untrusted::new("plugin assertion".into())),
    );
    has(
        &result,
        ProposedEffect::ReplaceModelOutput {
            kind: ModelOutputKind::Tool,
            value: Untrusted::new(
                json!({"stdout":"replacement","failed":false,"interrupted":false}),
            ),
        },
    );
    assert!(
        decode(
            HookDialect::Codex,
            HookEvent::PostToolUse,
            &context(),
            &json!({"hookSpecificOutput":{
                "hookEventName":"PostToolUse", "updatedMCPToolOutput":{}
            }})
        )
        .failed()
    );
    assert!(
        decode(
            HookDialect::Codex,
            HookEvent::PostToolUse,
            &context(),
            &json!({"suppressOutput":true})
        )
        .failed()
    );
    let failure = decode(
        HookDialect::Claude,
        HookEvent::PostToolUseFailure,
        &context(),
        &json!({"hookSpecificOutput":{
            "hookEventName":"PostToolUseFailure", "updatedToolOutput":{"success":true}
        }}),
    );
    assert!(failure.failed());
    assert!(failure.effects.is_empty());
}

#[test]
fn mcp_only_replacement_is_ignored_on_native_tool_and_retained_on_mcp_tool() {
    let value = json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":"replacement"}});
    for tool_is_mcp in [false, true] {
        let ctx = ResultContext {
            tool_is_mcp,
            ..context()
        };
        let result = decode(HookDialect::Claude, HookEvent::PostToolUse, &ctx, &value);
        assert_eq!(
            result
                .effects
                .iter()
                .any(|e| matches!(e, ProposedEffect::ReplaceModelOutput { .. })),
            tool_is_mcp
        );
    }
}

#[test]
fn prompt_display_suppression_requires_a_block_and_keeps_source_content_separate() {
    for event in [HookEvent::UserPromptSubmit, HookEvent::UserPromptExpansion] {
        for blocked in [false, true] {
            let mut value = json!({"hookSpecificOutput":{"hookEventName":event.as_str(),"suppressOriginalPrompt":true}});
            if blocked {
                value["decision"] = json!("block");
                value["reason"] = json!("declined");
            }
            let result = decode(HookDialect::Claude, event, &context(), &value);
            assert!(!result.failed());
            assert_eq!(
                result
                    .effects
                    .contains(&ProposedEffect::SuppressOriginalPrompt),
                blocked
            );
        }
    }
    let ctx = ResultContext {
        role: ResultRole::Observer,
        ..context()
    };
    let value =
        json!({"hookSpecificOutput":{"hookEventName":"MessageDisplay","displayContent":""}});
    let result = decode(HookDialect::Claude, HookEvent::MessageDisplay, &ctx, &value);
    has(
        &result,
        ProposedEffect::DisplayContent(Untrusted::new(String::new())),
    );
    let failed = command(
        HookDialect::Claude,
        HookEvent::MessageDisplay,
        &ctx,
        2,
        &value.to_string(),
        "",
    );
    assert!(failed.effects.is_empty());
}

#[test]
fn http_and_mcp_failures_hold_gates_and_do_not_decode_error_bodies() {
    let profile = CompatibilityProfile::embedded().unwrap();
    for body in [
        b"plain".as_slice(),
        b"{",
        b"[]",
        b"null",
        b"{\"continue\":0}",
    ] {
        let result = decode_response(
            &profile,
            HookDialect::Claude,
            HookEvent::PreToolUse,
            HandlerKind::Http,
            &context(),
            HookResponse::Http { status: 200, body },
        );
        assert!(result.failed());
        assert_eq!(result.gate, GateDisposition::Held);
    }
    for status in [199, 300, 400, 500] {
        let result = decode_response(&profile, HookDialect::Claude, HookEvent::PreToolUse, HandlerKind::Http,
            &context(), HookResponse::Http { status, body:br#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}"# });
        assert!(result.failed());
        assert!(result.effects.is_empty());
    }
    let result = decode_response(
        &profile,
        HookDialect::Claude,
        HookEvent::PreToolUse,
        HandlerKind::McpTool,
        &context(),
        HookResponse::Mcp {
            structured: Some(&json!({})),
            text: &[],
            is_error: true,
        },
    );
    assert!(result.failed());
    assert!(result.effects.is_empty());
    for response in [
        HookResponse::Http {
            status: 204,
            body: b"",
        },
        HookResponse::Http {
            status: 200,
            body: b"{}",
        },
    ] {
        let result = decode_response(
            &profile,
            HookDialect::Claude,
            HookEvent::PreToolUse,
            HandlerKind::Http,
            &context(),
            response,
        );
        assert!(!result.failed());
        assert_eq!(result.source_decision, SourceDecision::NoSourceDecision);
    }
}

#[test]
fn wire_limits_duplicates_and_transport_mismatch_are_secret_safe() {
    let profile = CompatibilityProfile::embedded().unwrap();
    let oversized = "secret-canary".repeat(100_000);
    for stdout in [
        oversized.as_bytes(),
        b"\xff",
        br#"{"continue":true,"continue":false}"#,
    ] {
        let result = decode_response(
            &profile,
            HookDialect::Claude,
            HookEvent::PreToolUse,
            HandlerKind::Command,
            &context(),
            HookResponse::Command {
                exit_code: Some(0),
                stdout,
                stderr: b"",
            },
        );
        assert!(result.failed());
        assert!(!format!("{result:?}").contains("secret-canary"));
    }
    let result = decode_response(
        &profile,
        HookDialect::Native,
        HookEvent::PreToolUse,
        HandlerKind::Http,
        &context(),
        HookResponse::Command {
            exit_code: Some(0),
            stdout: b"{}",
            stderr: b"",
        },
    );
    assert!(result.failed());
    let secret = decode(
        HookDialect::Claude,
        HookEvent::PreToolUse,
        &context(),
        &json!({"hookSpecificOutput":{
            "hookEventName":"PreToolUse","additionalContext":"secret-canary","updatedInput":{"secret":"secret-canary"}
        }}),
    );
    assert!(!format!("{secret:?}").contains("secret-canary"));
}

#[test]
fn plain_stdout_rules_preserve_source_differences() {
    for (dialect, event, text, context_expected, failure_expected) in [
        (
            HookDialect::Claude,
            HookEvent::SessionStart,
            "plain",
            true,
            false,
        ),
        (
            HookDialect::Claude,
            HookEvent::PreToolUse,
            "plain",
            false,
            false,
        ),
        (
            HookDialect::Claude,
            HookEvent::UserPromptSubmit,
            "[1,2]",
            true,
            false,
        ),
        (
            HookDialect::Claude,
            HookEvent::UserPromptSubmit,
            "{unfinished",
            true,
            false,
        ),
        (
            HookDialect::Claude,
            HookEvent::UserPromptSubmit,
            "{\"log\":1}\n{\"log\":2}",
            true,
            false,
        ),
        (
            HookDialect::Claude,
            HookEvent::UserPromptSubmit,
            "{\"continue\":true}\n{\"log\":2}",
            false,
            true,
        ),
        (
            HookDialect::Codex,
            HookEvent::UserPromptSubmit,
            "[1,2]",
            false,
            true,
        ),
        (
            HookDialect::Codex,
            HookEvent::PreCompact,
            "plain",
            false,
            false,
        ),
        (HookDialect::Codex, HookEvent::Stop, "plain", false, true),
        (
            HookDialect::Native,
            HookEvent::PreToolUse,
            "plain",
            false,
            true,
        ),
    ] {
        let ctx = ResultContext {
            role: if observer_event(event) {
                ResultRole::Observer
            } else {
                ResultRole::RequiredGate
            },
            ..context()
        };
        let result = command(dialect, event, &ctx, 0, text, "");
        assert_eq!(
            result.failed(),
            failure_expected,
            "{dialect:?}/{event:?}/{text}: {result:?}"
        );
        assert_eq!(
            result
                .effects
                .iter()
                .any(|e| matches!(e, ProposedEffect::AdditionalContext(_))),
            context_expected
        );
    }
}

#[test]
fn every_source_ignored_continue_remains_distinct_from_rejected_fields() {
    for event in [
        HookEvent::SessionStart,
        HookEvent::InstructionsLoaded,
        HookEvent::PermissionDenied,
        HookEvent::StopFailure,
        HookEvent::SessionEnd,
        HookEvent::TaskCreated,
        HookEvent::SubagentStart,
        HookEvent::WorktreeRemove,
        HookEvent::PreCompact,
        HookEvent::PostCompact,
        HookEvent::PostModelSwitch,
        HookEvent::ConfigChange,
        HookEvent::Setup,
        HookEvent::Notification,
        HookEvent::FileChanged,
        HookEvent::CwdChanged,
        HookEvent::DirectoryAdded,
        HookEvent::MessageDisplay,
        HookEvent::Elicitation,
        HookEvent::ElicitationResult,
    ] {
        let ctx = ResultContext {
            role: ResultRole::Observer,
            ..context()
        };
        let result = decode(HookDialect::Claude, event, &ctx, &json!({"continue":false}));
        assert!(!result.failed(), "{event:?}: {result:?}");
        assert!(result.effects.is_empty(), "{event:?}: {result:?}");
        assert_eq!(result.source_decision, SourceDecision::NoSourceDecision);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.kind == DiagnosticKind::Ignored && d.detail.path == "/continue")
        );
    }
}

#[test]
fn elicitation_content_only_override_uses_host_action_and_never_creates_credentials() {
    for action in [
        ElicitationAction::Accept,
        ElicitationAction::Decline,
        ElicitationAction::Cancel,
    ] {
        let ctx = ResultContext {
            elicitation_action: Some(action),
            ..context()
        };
        let result = decode(
            HookDialect::Claude,
            HookEvent::ElicitationResult,
            &ctx,
            &json!({"hookSpecificOutput":{
                "hookEventName":"ElicitationResult","content":{"answer":"plugin data"}
            }}),
        );
        assert!(!result.failed());
        assert_eq!(
            result.effects.iter().any(|e| matches!(
                e,
                ProposedEffect::Elicitation {
                    content: Some(_),
                    ..
                }
            )),
            action == ElicitationAction::Accept
        );
    }
    for event in [
        HookEvent::Elicitation,
        HookEvent::ElicitationResult,
        HookEvent::MessageDisplay,
    ] {
        let ctx = ResultContext {
            role: if event == HookEvent::MessageDisplay {
                ResultRole::Observer
            } else {
                ResultRole::RequiredGate
            },
            ..context()
        };
        assert!(decode(HookDialect::Native, event, &ctx, &json!({})).failed());
        assert!(decode(HookDialect::Native, event, &ctx, &json!({"ok":true})).failed());
    }
}

#[test]
fn source_specific_system_messages_watch_notices_and_retry_use_host_context() {
    for source in [DirectoryAddedSource::Command, DirectoryAddedSource::Sdk] {
        let ctx = ResultContext {
            role: ResultRole::Observer,
            directory_source: source,
            ..context()
        };
        let result = decode(
            HookDialect::Claude,
            HookEvent::DirectoryAdded,
            &ctx,
            &json!({"systemMessage":"directory context"}),
        );
        assert_eq!(
            result
                .effects
                .iter()
                .any(|e| matches!(e, ProposedEffect::AdditionalContext(_))),
            source == DirectoryAddedSource::Command
        );
    }
    for eligible in [false, true] {
        let ctx = ResultContext {
            role: ResultRole::Observer,
            retry_eligible: eligible,
            ..context()
        };
        let result = decode(
            HookDialect::Claude,
            HookEvent::PermissionDenied,
            &ctx,
            &json!({"hookSpecificOutput":{"hookEventName":"PermissionDenied","retry":true}}),
        );
        assert_eq!(
            result
                .effects
                .contains(&ProposedEffect::RetryDeniedOperation),
            eligible
        );
    }
    let ctx = ResultContext {
        role: ResultRole::Observer,
        interactive_display: true,
        ..context()
    };
    for event in [HookEvent::CwdChanged, HookEvent::FileChanged] {
        has(
            &decode(
                HookDialect::Claude,
                event,
                &ctx,
                &json!({"systemMessage":"notice"}),
            ),
            ProposedEffect::TransientNotice(Untrusted::new("notice".into())),
        );
    }
}

#[test]
fn compaction_recovery_failure_remains_separate_from_rejected_compaction() {
    let ctx = ResultContext {
        compaction_recovery: true,
        ..context()
    };
    let result = decode(
        HookDialect::Claude,
        HookEvent::PreCompact,
        &ctx,
        &json!({"decision":"block","reason":"hold"}),
    );
    has(&result, ProposedEffect::RetainContextLimitFailure);
    has(
        &result,
        ProposedEffect::Decision {
            choice: DecisionChoice::Deny,
            reason: Some(Untrusted::new("hold".into())),
        },
    );
    let source_policy = ResultContext {
        config_is_managed_policy: true,
        ..context()
    };
    let result = decode(
        HookDialect::Claude,
        HookEvent::ConfigChange,
        &source_policy,
        &json!({"decision":"block","reason":"hold"}),
    );
    assert_eq!(result.source_decision, SourceDecision::NoSourceDecision);
    assert!(result.effects.is_empty());
}

#[test]
fn source_absolute_watch_paths_preserve_dot_segments_for_host_admission() {
    let ctx = ResultContext {
        role: ResultRole::Observer,
        ..context()
    };
    for event in [
        HookEvent::SessionStart,
        HookEvent::FileChanged,
        HookEvent::CwdChanged,
    ] {
        let result = decode(
            HookDialect::Claude,
            event,
            &ctx,
            &json!({"hookSpecificOutput":{
                "hookEventName":event.as_str(),"watchPaths":["/workspace/a/../b","/workspace/./c"]
            }}),
        );
        assert!(!result.failed(), "{result:?}");
        has(
            &result,
            ProposedEffect::ReplaceDynamicWatches(vec![
                "/workspace/a/../b".into(),
                "/workspace/./c".into(),
            ]),
        );
    }
}

#[test]
fn cancellation_and_shutdown_never_become_a_plugin_gate_even_on_failure() {
    for dialect in HookDialect::ALL {
        for event in [HookEvent::Interrupt, HookEvent::SessionEnd] {
            let result = decode_response(
                &CompatibilityProfile::embedded().unwrap(),
                *dialect,
                event,
                HandlerKind::Command,
                &context(),
                HookResponse::Failure(TransportFailure::Timeout),
            );
            assert!(result.failed());
            assert_eq!(result.gate, GateDisposition::NotAGate);
            assert!(result.effects.is_empty());
        }
    }
}

#[test]
fn supported_cells_cover_invalid_nested_output_and_owner_cancellation() {
    let profile = CompatibilityProfile::embedded().unwrap();
    for &dialect in HookDialect::ALL {
        for &event in HookEvent::ALL {
            for handler in [
                HandlerKind::Command,
                HandlerKind::Http,
                HandlerKind::McpTool,
            ] {
                if !supported(dialect, event, handler) {
                    continue;
                }
                let ctx = ResultContext {
                    role: if observer_event(event) {
                        ResultRole::Observer
                    } else {
                        ResultRole::RequiredGate
                    },
                    ..context()
                };
                let invalid = json!({"hookSpecificOutput":{"hookEventName":"forged-event","additionalContext":12}});
                let result = decode_response(
                    &profile,
                    dialect,
                    event,
                    handler,
                    &ctx,
                    HookResponse::Callback(&invalid),
                );
                if dialect == HookDialect::Codex && event == HookEvent::SessionEnd {
                    assert!(!result.failed());
                    assert!(!result.diagnostics.is_empty());
                } else {
                    assert!(result.failed(), "{dialect:?}/{event:?}/{handler:?}");
                }
                assert!(result.effects.is_empty());
                let cancelled = decode_response(
                    &profile,
                    dialect,
                    event,
                    handler,
                    &ctx,
                    HookResponse::Failure(TransportFailure::Cancelled),
                );
                assert!(cancelled.failed());
                assert!(cancelled.effects.is_empty());
                assert_eq!(
                    cancelled.gate,
                    if ctx.role == ResultRole::Observer {
                        GateDisposition::NotAGate
                    } else {
                        GateDisposition::Held
                    }
                );
            }
        }
    }
}

#[test]
fn failed_command_plain_stdout_is_never_added_as_success_context() {
    for event in [
        HookEvent::SessionStart,
        HookEvent::SubagentStart,
        HookEvent::UserPromptSubmit,
        HookEvent::UserPromptExpansion,
        HookEvent::PostModelSwitch,
    ] {
        let ctx = ResultContext {
            role: if observer_event(event) {
                ResultRole::Observer
            } else {
                ResultRole::RequiredGate
            },
            ..context()
        };
        for exit in [1, 2, 127] {
            let result = command(
                HookDialect::Claude,
                event,
                &ctx,
                exit,
                "plain output",
                "error",
            );
            assert!(result.failed());
            assert!(
                !result
                    .effects
                    .iter()
                    .any(|e| matches!(e, ProposedEffect::AdditionalContext(_))),
                "{event:?}/{exit}: {result:?}"
            );
        }
    }
}

#[test]
fn every_command_event_has_explicit_exit_two_semantics() {
    let profile = CompatibilityProfile::embedded().unwrap();
    for &dialect in HookDialect::ALL {
        for &event in HookEvent::ALL {
            if !supported(dialect, event, HandlerKind::Command) {
                continue;
            }
            let ctx = ResultContext {
                role: if observer_event(event) {
                    ResultRole::Observer
                } else {
                    ResultRole::RequiredGate
                },
                ..context()
            };
            let reason = Untrusted::new("exit reason".to_owned());
            use HookEvent::*;
            let expected = if dialect == HookDialect::Codex
                && !matches!(
                    event,
                    PreToolUse
                        | PostToolUse
                        | PermissionRequest
                        | UserPromptSubmit
                        | Stop
                        | SubagentStop
                ) {
                vec![]
            } else {
                match event {
                    PreToolUse | UserPromptSubmit | UserPromptExpansion | TaskCreated
                    | TaskCompleted | PreModelSwitch | ConfigChange | PreCompact => {
                        vec![ProposedEffect::Decision {
                            choice: DecisionChoice::Deny,
                            reason: Some(reason),
                        }]
                    }
                    PermissionRequest if dialect != HookDialect::Claude => {
                        vec![ProposedEffect::Decision {
                            choice: DecisionChoice::Deny,
                            reason: Some(reason),
                        }]
                    }
                    Stop | SubagentStop | TeammateIdle => vec![
                        ProposedEffect::Feedback(reason),
                        ProposedEffect::Control(ControlRequest::Followup(match event {
                            Stop => FollowupTarget::Task,
                            SubagentStop => FollowupTarget::Subagent,
                            _ => FollowupTarget::Teammate,
                        })),
                    ],
                    PostToolUse | PostToolUseFailure => vec![ProposedEffect::Feedback(reason)],
                    PostToolBatch if dialect == HookDialect::Claude => vec![
                        ProposedEffect::Control(ControlRequest::HoldContinuation),
                        ProposedEffect::Feedback(reason),
                    ],
                    Elicitation | ElicitationResult => vec![ProposedEffect::Elicitation {
                        action: Some(ElicitationAction::Decline),
                        content: None,
                    }],
                    SessionStart | SubagentStart | PostModelSwitch | SessionEnd | CwdChanged
                    | FileChanged | PostCompact => vec![ProposedEffect::Warning(reason)],
                    PermissionRequest | PostToolBatch | InstructionsLoaded | StopFailure
                    | Interrupt | WorktreeCreate | WorktreeRemove | Setup | Notification
                    | DirectoryAdded | MessageDisplay | PermissionDenied => vec![],
                }
            };
            let result = decode_response(
                &profile,
                dialect,
                event,
                HandlerKind::Command,
                &ctx,
                HookResponse::Command {
                    exit_code: Some(2),
                    stdout: b"",
                    stderr: b"exit reason",
                },
            );
            assert!(result.failed(), "{dialect:?}/{event:?}");
            assert_eq!(result.effects, expected, "{dialect:?}/{event:?}");
        }
    }
}

#[test]
fn paths_are_untrusted_and_do_not_leak_through_debug_output() {
    let ctx = ResultContext {
        role: ResultRole::Observer,
        ..context()
    };
    let watches = decode(
        HookDialect::Claude,
        HookEvent::FileChanged,
        &ctx,
        &json!({"hookSpecificOutput":{
            "hookEventName":"FileChanged","watchPaths":["/workspace/secret-canary"]
        }}),
    );
    let worktree = command(
        HookDialect::Claude,
        HookEvent::WorktreeCreate,
        &context(),
        0,
        "/workspace/secret-canary",
        "",
    );
    for result in [watches, worktree] {
        assert!(!result.failed());
        assert!(!format!("{result:?}").contains("secret-canary"));
    }
}

#[test]
fn model_verdicts_cannot_enter_the_nonmodel_decoder() {
    let profile = CompatibilityProfile::embedded().unwrap();
    for dialect in [HookDialect::Native, HookDialect::Claude, HookDialect::Codex] {
        for handler in [HandlerKind::Prompt, HandlerKind::Agent] {
            let result = decode_response(
                &profile,
                dialect,
                HookEvent::PreToolUse,
                handler,
                &context(),
                HookResponse::Callback(&json!({"ok":true})),
            );
            assert!(result.failed());
            assert!(result.effects.is_empty());
        }
    }
}

#[test]
fn codex_optional_value_null_is_absent_not_a_reserved_effect() {
    let ctx = ResultContext {
        tool_is_mcp: true,
        ..context()
    };
    let result = decode(
        HookDialect::Codex,
        HookEvent::PostToolUse,
        &ctx,
        &json!({"hookSpecificOutput":{
            "hookEventName":"PostToolUse","updatedMCPToolOutput":null
        }}),
    );
    assert!(!result.failed(), "{result:?}");
    assert!(result.effects.is_empty());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.kind == DiagnosticKind::Ignored)
    );
    let result = decode(
        HookDialect::Codex,
        HookEvent::PermissionRequest,
        &ctx,
        &json!({"hookSpecificOutput":{
            "hookEventName":"PermissionRequest","decision":{"behavior":"allow","updatedInput":null,"updatedPermissions":null}
        }}),
    );
    assert!(!result.failed(), "{result:?}");
    assert_eq!(
        result.effects,
        vec![ProposedEffect::Decision {
            choice: DecisionChoice::NoObjection,
            reason: None
        }]
    );
    let result = decode(
        HookDialect::Codex,
        HookEvent::PreToolUse,
        &ctx,
        &json!({"decision":"block","reason":"legacy","hookSpecificOutput":{
            "hookEventName":"PreToolUse","updatedInput":null
        }}),
    );
    assert!(!result.failed(), "{result:?}");
    assert_eq!(
        result.effects,
        vec![ProposedEffect::Decision {
            choice: DecisionChoice::Deny,
            reason: Some(Untrusted::new("legacy".into()))
        }]
    );
    assert!(
        decode(
            HookDialect::Codex,
            HookEvent::PreToolUse,
            &ctx,
            &json!({"hookSpecificOutput":{
                "hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":null
            }})
        )
        .failed()
    );
}

#[test]
fn claude_defer_depends_on_interactive_host_and_never_becomes_allow() {
    let value =
        json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"defer"}});
    for interactive in [false, true] {
        let ctx = ResultContext {
            interactive_session: interactive,
            ..context()
        };
        let result = decode(HookDialect::Claude, HookEvent::PreToolUse, &ctx, &value);
        assert!(!result.failed());
        if interactive {
            assert!(result.effects.is_empty());
            assert_eq!(result.source_decision, SourceDecision::NoSourceDecision);
        } else {
            has(
                &result,
                ProposedEffect::Decision {
                    choice: DecisionChoice::Defer,
                    reason: None,
                },
            );
            assert_eq!(result.gate, GateDisposition::Held);
        }
    }
}

#[test]
fn native_deny_is_not_a_legacy_field_that_a_specific_allow_can_erase() {
    let result = decode(
        HookDialect::Native,
        HookEvent::PreToolUse,
        &context(),
        &json!({"decision":"block","reason":"deny","hookSpecificOutput":{
            "hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{}
        }}),
    );
    assert!(!result.failed());
    assert_eq!(result.gate, GateDisposition::Held);
    has(
        &result,
        ProposedEffect::Decision {
            choice: DecisionChoice::Deny,
            reason: Some(Untrusted::new("deny".into())),
        },
    );
}

#[test]
fn claude_worktree_paths_use_actual_cwd_and_strip_ansi_before_last_nonempty_line() {
    let ctx = ResultContext {
        working_directory: Some("/workspace/root".into()),
        ..context()
    };
    let relative = command(
        HookDialect::Claude,
        HookEvent::WorktreeCreate,
        &ctx,
        0,
        "../child",
        "",
    );
    assert!(!relative.failed());
    has(
        &relative,
        ProposedEffect::WorktreePath("/workspace/child".into()),
    );
    let colored = command(
        HookDialect::Claude,
        HookEvent::WorktreeCreate,
        &ctx,
        0,
        "banner\n\u{1b}[32m/workspace/child\u{1b}[0m\n\u{1b}[0m\n",
        "",
    );
    assert!(!colored.failed(), "{colored:?}");
    has(
        &colored,
        ProposedEffect::WorktreePath("/workspace/child".into()),
    );
    for path in [
        "/workspace/../child",
        "/workspace/./child",
        "/workspace/child\u{0}",
    ] {
        assert!(
            command(
                HookDialect::Claude,
                HookEvent::WorktreeCreate,
                &ctx,
                0,
                path,
                ""
            )
            .failed()
        );
    }
    assert!(
        command(
            HookDialect::Native,
            HookEvent::WorktreeCreate,
            &ctx,
            0,
            "\u{1b}[32m/workspace/child\u{1b}[0m",
            ""
        )
        .failed()
    );
    for path in [
        "/workspace/child\u{1b}[32",
        "/workspace/child\u{1b}]9;unfinished",
    ] {
        assert!(
            command(
                HookDialect::Claude,
                HookEvent::WorktreeCreate,
                &ctx,
                0,
                path,
                ""
            )
            .failed()
        );
    }
}

#[test]
fn codex_async_reserved_controls_preserve_source_context_and_warnings() {
    // Source event parsers apply invalid_reason only when can_apply_control_effects().
    // PreToolUse/PostToolUse explicitly test preservation for async handlers.
    let cases = [
        (
            HookEvent::PreToolUse,
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","additionalContext":"preserved"}}),
            true,
        ),
        (
            HookEvent::PreToolUse,
            json!({"continue":false,"stopReason":"ignored","suppressOutput":true,"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"ask","additionalContext":"preserved"}}),
            true,
        ),
        (
            HookEvent::PostToolUse,
            json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":{"ok":true},"additionalContext":"preserved"}}),
            true,
        ),
        (
            HookEvent::PostToolUse,
            json!({"decision":"block","hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"preserved"}}),
            true,
        ),
        (
            HookEvent::UserPromptSubmit,
            json!({"decision":"block","hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"preserved"}}),
            true,
        ),
        (
            HookEvent::PermissionRequest,
            json!({"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow","updatedInput":{"x":1},"updatedPermissions":[]}}}),
            false,
        ),
        (HookEvent::Stop, json!({"decision":"block"}), false),
        (HookEvent::SubagentStop, json!({"decision":"block"}), false),
    ];
    for (event, mut output, retains_context) in cases {
        output["systemMessage"] = json!("warning preserved");
        let synchronous = command(
            HookDialect::Codex,
            event,
            &ResultContext {
                role: ResultRole::RequiredGate,
                ..context()
            },
            0,
            &output.to_string(),
            "",
        );
        assert!(synchronous.failed(), "{event:?}: {synchronous:?}");
        assert_eq!(synchronous.gate, GateDisposition::Held);
        let asynchronous = command(
            HookDialect::Codex,
            event,
            &ResultContext {
                role: ResultRole::Observer,
                asynchronous: true,
                ..context()
            },
            0,
            &output.to_string(),
            "",
        );
        assert!(!asynchronous.failed(), "{event:?}: {asynchronous:?}");
        let mut expected = vec![ProposedEffect::Warning(Untrusted::new(
            "warning preserved".into(),
        ))];
        if retains_context {
            expected.push(ProposedEffect::AdditionalContext(Untrusted::new(
                "preserved".into(),
            )));
        }
        assert_eq!(asynchronous.effects, expected, "{event:?}");
        assert_eq!(
            asynchronous.source_decision,
            SourceDecision::NoSourceDecision
        );
        assert!(
            asynchronous
                .diagnostics
                .iter()
                .any(|d| d.kind == DiagnosticKind::Ignored)
        );
    }
}

#[test]
fn codex_async_observers_still_reject_malformed_schema_and_cannot_be_gates() {
    for role in [ResultRole::Observer, ResultRole::RequiredGate] {
        let result = command(
            HookDialect::Codex,
            HookEvent::PreToolUse,
            &ResultContext {
                role,
                asynchronous: true,
                ..context()
            },
            0,
            r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":42,"additionalContext":"must not survive"}}"#,
            "",
        );
        assert!(result.failed(), "{result:?}");
        assert!(result.effects.is_empty());
    }
}

#[test]
fn codex_async_valid_feedback_and_rewrites_are_ignored_without_losing_context() {
    for (event, output) in [
        (
            HookEvent::PreToolUse,
            json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"x":1},"additionalContext":"preserved"}}),
        ),
        (
            HookEvent::PostToolUse,
            json!({"decision":"block","reason":"feedback ignored","hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"preserved"}}),
        ),
        (
            HookEvent::UserPromptSubmit,
            json!({"continue":false,"stopReason":"ignored","hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"preserved"}}),
        ),
    ] {
        let result = command(
            HookDialect::Codex,
            event,
            &ResultContext {
                role: ResultRole::Observer,
                asynchronous: true,
                ..context()
            },
            0,
            &output.to_string(),
            "",
        );
        assert!(!result.failed(), "{result:?}");
        assert_eq!(
            result.effects,
            vec![ProposedEffect::AdditionalContext(Untrusted::new(
                "preserved".into()
            ))],
            "{event:?}"
        );
        assert_eq!(result.source_decision, SourceDecision::NoSourceDecision);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.kind == DiagnosticKind::Ignored)
        );
    }
}

#[test]
fn codex_async_shared_start_and_compact_parsers_retain_only_observations() {
    for (event, retains_context) in [
        (HookEvent::SessionStart, true),
        (HookEvent::SubagentStart, true),
        (HookEvent::PreCompact, false),
        (HookEvent::PostCompact, false),
    ] {
        let mut output = json!({"continue":false,"stopReason":"ignored","systemMessage":"warning"});
        if retains_context {
            output["hookSpecificOutput"] =
                json!({"hookEventName":format!("{event:?}"),"additionalContext":"preserved"});
        }
        let result = command(
            HookDialect::Codex,
            event,
            &ResultContext {
                role: ResultRole::Observer,
                asynchronous: true,
                ..context()
            },
            0,
            &output.to_string(),
            "",
        );
        assert!(!result.failed(), "{event:?}: {result:?}");
        let mut expected = vec![ProposedEffect::Warning(Untrusted::new("warning".into()))];
        if retains_context {
            expected.push(ProposedEffect::AdditionalContext(Untrusted::new(
                "preserved".into(),
            )));
        }
        assert_eq!(result.effects, expected, "{event:?}");
        assert_eq!(result.source_decision, SourceDecision::NoSourceDecision);
    }
}

#[test]
fn codex_async_exit_two_is_failure_without_feedback_or_control() {
    for event in [
        HookEvent::PreToolUse,
        HookEvent::PermissionRequest,
        HookEvent::PostToolUse,
        HookEvent::UserPromptSubmit,
        HookEvent::Stop,
        HookEvent::SubagentStop,
    ] {
        let result = command(
            HookDialect::Codex,
            event,
            &ResultContext {
                role: ResultRole::Observer,
                asynchronous: true,
                ..context()
            },
            2,
            "",
            "ignored control reason",
        );
        assert!(result.failed(), "{event:?}: {result:?}");
        assert!(result.effects.is_empty(), "{event:?}: {result:?}");
        assert_eq!(result.source_decision, SourceDecision::NoSourceDecision);
        assert_eq!(result.gate, GateDisposition::NotAGate);
    }
}

#[test]
fn codex_async_stop_plain_stdout_is_ignored_but_sync_and_json_errors_fail() {
    for event in [HookEvent::Stop, HookEvent::SubagentStop] {
        for asynchronous in [false, true] {
            let ctx = ResultContext {
                role: ResultRole::Observer,
                asynchronous,
                ..context()
            };
            for (stdout, malformed_json) in [
                ("observer done", false),
                ("{broken", true),
                ("[broken", true),
            ] {
                let result = command(HookDialect::Codex, event, &ctx, 0, stdout, "");
                assert_eq!(
                    result.failed(),
                    !asynchronous || malformed_json,
                    "{event:?} async={asynchronous} stdout={stdout}: {result:?}"
                );
                assert!(result.effects.is_empty());
            }
        }
    }
}

#[test]
fn codex_stopping_precedes_invalid_block_reason_without_admitting_invalid_context() {
    for event in [
        HookEvent::Stop,
        HookEvent::SubagentStop,
        HookEvent::UserPromptSubmit,
    ] {
        let mut output = json!({"continue":false,"decision":"block","systemMessage":"warning"});
        if event == HookEvent::UserPromptSubmit {
            output["hookSpecificOutput"] = json!({"hookEventName":"UserPromptSubmit","additionalContext":"must be suppressed"});
        }
        let result = command(
            HookDialect::Codex,
            event,
            &ResultContext {
                role: ResultRole::RequiredGate,
                ..context()
            },
            0,
            &output.to_string(),
            "",
        );
        assert!(!result.failed(), "{event:?}: {result:?}");
        assert_eq!(
            result.effects,
            vec![
                ProposedEffect::Warning(Untrusted::new("warning".into())),
                ProposedEffect::Control(ControlRequest::EndTurn)
            ],
            "{event:?}"
        );
        assert_eq!(result.gate, GateDisposition::Held);
    }
}

#[test]
fn codex_posttool_stop_feedback_and_context_follow_separate_source_conditions() {
    for (fields, context_allowed, feedback) in [
        (
            json!({"decision":"block"}),
            false,
            "PostToolUse hook stopped execution",
        ),
        (
            json!({"decision":"block","reason":"  "}),
            false,
            "PostToolUse hook stopped execution",
        ),
        (
            json!({"decision":"block","reason":"  useful feedback  ","suppressOutput":true}),
            false,
            "useful feedback",
        ),
        (
            json!({"stopReason":"stop text","suppressOutput":true}),
            false,
            "stop text",
        ),
        (
            json!({"hookSpecificOutput":{"hookEventName":"PostToolUse","updatedMCPToolOutput":{"x":1},"additionalContext":"context"}}),
            false,
            "PostToolUse hook stopped execution",
        ),
        (
            json!({"reason":"  reason without decision  "}),
            true,
            "reason without decision",
        ),
        (json!({}), true, "PostToolUse hook stopped execution"),
    ] {
        let mut output = json!({"continue":false,"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"context"}});
        output
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let result = command(
            HookDialect::Codex,
            HookEvent::PostToolUse,
            &ResultContext {
                role: ResultRole::RequiredGate,
                tool_is_mcp: true,
                ..context()
            },
            0,
            &output.to_string(),
            "",
        );
        assert!(!result.failed(), "{output}: {result:?}");
        has(
            &result,
            ProposedEffect::Control(ControlRequest::HoldContinuation),
        );
        has(
            &result,
            ProposedEffect::Feedback(Untrusted::new(feedback.into())),
        );
        assert_eq!(
            result
                .effects
                .contains(&ProposedEffect::AdditionalContext(Untrusted::new(
                    "context".into()
                ))),
            context_allowed,
            "{output}: {result:?}"
        );
        assert!(
            !result
                .effects
                .iter()
                .any(|effect| matches!(effect, ProposedEffect::ReplaceModelOutput { .. }))
        );
    }
}

#[test]
fn codex_stopping_retains_valid_prompt_context_but_never_bypasses_schema() {
    let ctx = ResultContext {
        role: ResultRole::RequiredGate,
        ..context()
    };
    let valid = json!({"continue":false,"decision":"block","reason":"valid reason","hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":"preserved"}});
    let result = command(
        HookDialect::Codex,
        HookEvent::UserPromptSubmit,
        &ctx,
        0,
        &valid.to_string(),
        "",
    );
    assert!(!result.failed(), "{result:?}");
    has(
        &result,
        ProposedEffect::AdditionalContext(Untrusted::new("preserved".into())),
    );
    has(&result, ProposedEffect::Control(ControlRequest::EndTurn));
    for event in [
        HookEvent::Stop,
        HookEvent::SubagentStop,
        HookEvent::UserPromptSubmit,
        HookEvent::PostToolUse,
    ] {
        let invalid = json!({"continue":false,"decision":{"invalid":"shape"}});
        let result = command(HookDialect::Codex, event, &ctx, 0, &invalid.to_string(), "");
        assert!(result.failed(), "{event:?}: {result:?}");
        assert!(result.effects.is_empty());
    }
    for event in [HookEvent::PreToolUse, HookEvent::PermissionRequest] {
        let result = command(
            HookDialect::Codex,
            event,
            &ctx,
            0,
            r#"{"continue":false}"#,
            "",
        );
        assert!(result.failed(), "{event:?}: {result:?}");
        assert!(result.effects.is_empty());
    }
}

//! Event control requests are not access grants or workflow completion.
use super::*;
use HookEvent::*;

pub(super) fn continues(
    result: &mut DecodedResult,
    context: &ResultContext,
    value: &Value,
) -> bool {
    let applies = match result.dialect {
        HookDialect::Native => !native::observation_only(result.event),
        HookDialect::Codex => matches!(
            result.event,
            SessionStart
                | UserPromptSubmit
                | PostToolUse
                | Stop
                | SubagentStop
                | PreCompact
                | PostCompact
        ),
        HookDialect::Claude => match result.event {
            PreToolUse | PermissionRequest | UserPromptSubmit | UserPromptExpansion
            | PostToolUse | PostToolUseFailure | PostToolBatch | Stop | SubagentStop
            | TeammateIdle | PreModelSwitch => true,
            TaskCompleted => context.work.task_boundary == TaskBoundary::TeammateStop,
            SessionStart | InstructionsLoaded | PermissionDenied | StopFailure | Interrupt
            | SessionEnd | TaskCreated | SubagentStart | WorktreeCreate | WorktreeRemove
            | PreCompact | PostCompact | PostModelSwitch | ConfigChange | Setup | Notification
            | FileChanged | CwdChanged | DirectoryAdded | MessageDisplay | Elicitation
            | ElicitationResult => false,
        },
    };
    if value.get("continue").is_some() && !applies {
        result.ignored(
            "/continue",
            "source ignores continuation control for this event or boundary",
        );
    }
    let halted = applies && value["continue"] == false;
    if value.get("stopReason").is_some() {
        if halted {
            push_text(result, &value["stopReason"], ProposedEffect::Warning);
        } else {
            result.ignored(
                "/stopReason",
                "stopReason has no effect without supported continue:false",
            );
        }
    }
    if halted {
        let control = match result.event {
            PreCompact => ControlRequest::HoldAction,
            SessionStart | PostCompact | PostToolUse | PostToolUseFailure | PostToolBatch => {
                ControlRequest::HoldContinuation
            }
            _ => ControlRequest::EndTurn,
        };
        result.push(ProposedEffect::Control(control));
        if result.event == PreCompact && context.compaction_recovery {
            result.push(ProposedEffect::RetainContextLimitFailure);
        }
    }
    halted
}

pub(super) fn top_level(
    result: &mut DecodedResult,
    context: &ResultContext,
    value: &Value,
    halted: bool,
) {
    if value.get("decision").is_none() {
        if value.get("reason").is_some() {
            result.ignored("/reason", "reason has no decision effect on its own");
        }
        return;
    }
    if halted {
        result.ignored(
            "/decision",
            "continue:false takes precedence over the event decision",
        );
        return;
    }
    if result.event == PreToolUse
        && match result.dialect {
            HookDialect::Codex => source::specific_decision(&value["hookSpecificOutput"]),
            HookDialect::Claude => value["hookSpecificOutput"]
                .get("permissionDecision")
                .is_some(),
            HookDialect::Native => false,
        }
    {
        result.ignored(
            "/decision",
            "hook-specific decision takes precedence over legacy decision",
        );
        return;
    }
    if value["decision"] == "approve" {
        if result.event == PreToolUse
            || (result.dialect == HookDialect::Native && !native::observation_only(result.event))
        {
            result.push(ProposedEffect::Decision {
                choice: DecisionChoice::NoObjection,
                reason: None,
            });
        } else {
            result.ignored("/decision", "source ignores approve for this event");
        }
        return;
    }
    let reason = value["reason"].as_str().map(|s| Untrusted::new(s.into()));
    match result.event {
        PreToolUse | UserPromptSubmit | UserPromptExpansion | TaskCreated | PreModelSwitch => {
            deny(result, reason)
        }
        PermissionRequest => result.ignored(
            "/decision",
            "permission response uses the nested decision object",
        ),
        ConfigChange
            if result.dialect == HookDialect::Claude && context.config_is_managed_policy =>
        {
            result.ignored("/decision", "source cannot block managed policy settings");
        }
        ConfigChange | PreCompact => {
            deny(result, reason);
            if result.event == PreCompact && context.compaction_recovery {
                result.push(ProposedEffect::RetainContextLimitFailure);
            }
        }
        PostToolUse | PostToolUseFailure => {
            if let Some(reason) = reason {
                result.push(ProposedEffect::Feedback(reason));
            } else {
                result.push(ProposedEffect::Feedback(Untrusted::new(String::new())));
            }
        }
        PostToolBatch => {
            if result.dialect == HookDialect::Native {
                result.ignored(
                    "/decision",
                    "native batch observation cannot control completed work",
                );
            } else {
                result.push(ProposedEffect::Control(ControlRequest::HoldContinuation));
                if let Some(reason) = reason {
                    result.push(ProposedEffect::Feedback(reason));
                }
            }
        }
        Stop => followup(result, context, FollowupTarget::Task, reason),
        SubagentStop => followup(result, context, FollowupTarget::Subagent, reason),
        TaskCompleted | TeammateIdle if result.dialect == HookDialect::Native => {
            if result.event == TeammateIdle
                || context.work.task_boundary == TaskBoundary::TeammateStop
            {
                followup(result, context, FollowupTarget::Teammate, reason);
            } else {
                deny(result, reason);
            }
        }
        SessionStart | InstructionsLoaded | PermissionDenied | StopFailure | Interrupt
        | SessionEnd | TaskCompleted | SubagentStart | TeammateIdle | WorktreeCreate
        | WorktreeRemove | PostCompact | PostModelSwitch | Setup | Notification | FileChanged
        | CwdChanged | DirectoryAdded | MessageDisplay | Elicitation | ElicitationResult => result
            .ignored(
                "/decision",
                "source ignores top-level decision for this event",
            ),
    }
}

pub(super) fn pretool(
    result: &mut DecodedResult,
    context: &ResultContext,
    specific: &Value,
    halted: bool,
) {
    if result.dialect == HookDialect::Claude
        && context.interactive_session
        && specific["permissionDecision"] == "defer"
    {
        result.ignored(
            "/hookSpecificOutput",
            "Claude ignores deferred hook results in interactive sessions",
        );
        return;
    }
    if let Some(input) = specific.get("updatedInput") {
        if result.dialect == HookDialect::Codex && input.is_null() {
            result.ignored(
                "/hookSpecificOutput/updatedInput",
                "Codex treats optional null input as absent",
            );
        } else {
            result.push(ProposedEffect::RewriteInput(Untrusted::new(input.clone())));
        }
    }
    push_text(
        result,
        &specific["additionalContext"],
        ProposedEffect::AdditionalContext,
    );
    if halted {
        if specific.get("permissionDecision").is_some() {
            result.ignored(
                "/hookSpecificOutput/permissionDecision",
                "continue:false takes precedence",
            );
        }
        return;
    }
    if let Some(choice) = choice(specific["permissionDecision"].as_str()) {
        let mut reason = specific["permissionDecisionReason"]
            .as_str()
            .map(|s| Untrusted::new(s.into()));
        let choice = if result.event == PreModelSwitch && result.dialect == HookDialect::Claude {
            if choice == DecisionChoice::NoObjection {
                if reason.is_some() {
                    result.ignored(
                        "/hookSpecificOutput/permissionDecisionReason",
                        "source ignores allow reason on model switch",
                    );
                }
                reason = None;
            }
            if choice == DecisionChoice::Ask && !context.model_switch_can_ask {
                result.ignored(
                    "/hookSpecificOutput/permissionDecision",
                    "source ask becomes refusal on this host surface",
                );
                DecisionChoice::Deny
            } else {
                choice
            }
        } else {
            choice
        };
        result.push(ProposedEffect::Decision { choice, reason });
    } else if specific.get("permissionDecisionReason").is_some() {
        result.ignored(
            "/hookSpecificOutput/permissionDecisionReason",
            "reason has no decision effect by itself",
        );
    }
}

pub(super) fn permission(result: &mut DecodedResult, specific: &Value, halted: bool) {
    let decision = &specific["decision"];
    if decision.is_null() {
        return;
    }
    if halted {
        result.ignored(
            "/hookSpecificOutput/decision",
            "continue:false takes precedence",
        );
        return;
    }
    if let Some(input) = decision.get("updatedInput") {
        if result.dialect == HookDialect::Codex && input.is_null() {
            result.ignored(
                "/hookSpecificOutput/decision/updatedInput",
                "Codex treats optional null input as absent",
            );
        } else {
            result.push(ProposedEffect::RewriteInput(Untrusted::new(input.clone())));
        }
    }
    if let Some(changes) = decision.get("updatedPermissions") {
        if result.dialect == HookDialect::Codex && changes.is_null() {
            result.ignored(
                "/hookSpecificOutput/decision/updatedPermissions",
                "Codex treats optional null permissions as absent",
            );
        } else {
            result.push(ProposedEffect::PermissionChanges(Untrusted::new(
                changes.clone(),
            )));
        }
    }
    if let Some(choice) = choice(decision["behavior"].as_str()) {
        result.push(ProposedEffect::Decision {
            choice,
            reason: decision["message"]
                .as_str()
                .map(|s| Untrusted::new(s.into())),
        });
    }
    if decision["interrupt"] == true {
        result.push(ProposedEffect::Control(ControlRequest::EndTurn));
    }
}

pub(super) fn followup(
    result: &mut DecodedResult,
    context: &ResultContext,
    target: FollowupTarget,
    reason: Option<Untrusted<String>>,
) {
    if let Some(reason) = reason {
        result.push(ProposedEffect::Feedback(reason));
    }
    let work = &context.work;
    let request = if work.cancelled || !work.allocation_available || !work.correction_available {
        ControlRequest::StopUnmet
    } else {
        ControlRequest::Followup(target)
    };
    // Context plus block on one response must not request two corrections.
    if !result.effects.contains(&ProposedEffect::Control(request)) {
        result.push(ProposedEffect::Control(request));
    }
}
pub(super) fn deny(result: &mut DecodedResult, reason: Option<Untrusted<String>>) {
    result.push(ProposedEffect::Decision {
        choice: DecisionChoice::Deny,
        reason,
    });
}
fn choice(value: Option<&str>) -> Option<DecisionChoice> {
    match value {
        Some("allow") => Some(DecisionChoice::NoObjection),
        Some("deny") => Some(DecisionChoice::Deny),
        Some("ask") => Some(DecisionChoice::Ask),
        Some("defer") => Some(DecisionChoice::Defer),
        _ => None,
    }
}
pub(super) fn push_text(
    result: &mut DecodedResult,
    value: &Value,
    make: fn(Untrusted<String>) -> ProposedEffect,
) {
    if let Some(text) = value.as_str() {
        result.push(make(Untrusted::new(text.into())));
    }
}

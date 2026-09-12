//! Interpret validated fields as atomic proposals, preserving event/source identity.
use super::*;
use crate::plugins::wire::WireResult;
use HookEvent::*;
use decisions::push_text;

pub(super) fn interpret(
    result: &mut DecodedResult,
    context: &ResultContext,
    value: &Value,
    exit_two: bool,
) -> WireResult {
    if result.dialect == HookDialect::Codex && result.event == SessionEnd {
        result.ignored("/", "Codex SessionEnd ignores response output");
        return Ok(());
    }
    if value["async"] == true {
        return schedule(result, context, value);
    }
    terminal(result, context, value);
    system_message(result, context, value);
    if value.get("suppressOutput").is_some() {
        result.ignored(
            "/suppressOutput",
            match result.dialect {
                HookDialect::Claude => "Claude recognizes suppressOutput but never acts on it",
                HookDialect::Codex => "this Codex event does not consume suppressOutput",
                HookDialect::Native => {
                    "Native v1 defines suppressOutput as an ignored compatibility field"
                }
            },
        );
    }
    if result.dialect == HookDialect::Codex && context.asynchronous {
        codex_observer(result, value);
        return Ok(());
    }
    if result.dialect == HookDialect::Codex && source::codex_stopping(result.event, value) {
        codex_stop(result, context, value);
        return Ok(());
    }
    let halted = decisions::continues(result, context, value);
    decisions::top_level(result, context, value, halted);
    specific(
        result,
        context,
        &value["hookSpecificOutput"],
        halted,
        exit_two,
    )?;
    Ok(())
}

/// Stopping wins over semantic control errors, but those errors still suppress
/// context. PostToolUse also sends model feedback even when its context is invalid.
fn codex_stop(result: &mut DecodedResult, context: &ResultContext, value: &Value) {
    let context_valid = source::validate_codex_effects(result.event, value).is_ok();
    if context_valid && matches!(result.event, UserPromptSubmit | PostToolUse) {
        push_text(
            result,
            &value["hookSpecificOutput"]["additionalContext"],
            ProposedEffect::AdditionalContext,
        );
    }
    decisions::continues(result, context, value);
    if !context_valid || value.get("decision").is_some() {
        result.ignored(
            "/",
            "Codex continuation stop takes precedence over control errors and decisions",
        );
    }
    if result.event == PostToolUse {
        let feedback = source::nonempty(&value["reason"])
            .or_else(|| value["stopReason"].as_str())
            .unwrap_or("PostToolUse hook stopped execution");
        result.push(ProposedEffect::Feedback(Untrusted::new(feedback.into())));
    }
}

/// Codex async event readers retain warnings/context but never evaluate controls.
/// Called after source schema validation and universal warning extraction.
fn codex_observer(result: &mut DecodedResult, value: &Value) {
    let specific = &value["hookSpecificOutput"];
    if matches!(
        result.event,
        PreToolUse | PostToolUse | UserPromptSubmit | SessionStart | SubagentStart
    ) {
        push_text(
            result,
            &specific["additionalContext"],
            ProposedEffect::AdditionalContext,
        );
    }
    let has_controls = ["continue", "stopReason", "decision", "reason"]
        .iter()
        .any(|key| value.get(key).is_some())
        || [
            "permissionDecision",
            "permissionDecisionReason",
            "updatedInput",
            "decision",
            "updatedMCPToolOutput",
        ]
        .iter()
        .any(|key| specific.get(key).is_some());
    if has_controls {
        result.ignored("/", "Codex asynchronous observer ignores control fields");
    }
}

fn specific(
    result: &mut DecodedResult,
    context: &ResultContext,
    value: &Value,
    halted: bool,
    exit_two: bool,
) -> WireResult {
    match result.event {
        PreToolUse | PreModelSwitch => decisions::pretool(result, context, value, halted),
        PermissionRequest => decisions::permission(result, value, halted),
        SessionStart => session_start(result, context, value)?,
        UserPromptSubmit | UserPromptExpansion => {
            push_text(
                result,
                &value["additionalContext"],
                ProposedEffect::AdditionalContext,
            );
            push_text(result, &value["sessionTitle"], ProposedEffect::SessionTitle);
            if value["suppressOriginalPrompt"] == true {
                if exit_two
                    || result.effects.iter().any(|effect| {
                        matches!(
                            effect,
                            ProposedEffect::Decision {
                                choice: DecisionChoice::Deny,
                                ..
                            }
                        )
                    })
                {
                    result.push(ProposedEffect::SuppressOriginalPrompt);
                } else {
                    result.ignored(
                        "/hookSpecificOutput/suppressOriginalPrompt",
                        "display suppression requires a blocked prompt",
                    );
                }
            }
        }
        PostToolUse => post_tool(result, context, value),
        PostToolUseFailure | PostToolBatch | SubagentStart | PostModelSwitch | Setup
        | Notification => {
            push_text(
                result,
                &value["additionalContext"],
                ProposedEffect::AdditionalContext,
            );
        }
        Stop | SubagentStop => {
            push_text(
                result,
                &value["additionalContext"],
                ProposedEffect::AdditionalContext,
            );
            if !halted
                && value["additionalContext"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
            {
                decisions::followup(
                    result,
                    context,
                    if result.event == Stop {
                        FollowupTarget::Task
                    } else {
                        FollowupTarget::Subagent
                    },
                    None,
                );
            }
        }
        PermissionDenied => {
            if value["retry"] == true {
                if context.retry_eligible
                    && !context.work.cancelled
                    && context.work.allocation_available
                {
                    result.push(ProposedEffect::RetryDeniedOperation);
                } else {
                    result.ignored(
                        "/hookSpecificOutput/retry",
                        "owner has no eligible retry within the current allowance",
                    );
                }
            }
        }
        FileChanged | CwdChanged => watches(result, value)?,
        WorktreeCreate => {
            if let Some(path) = value["worktreePath"].as_str() {
                worktree(result, context, path)?;
            }
        }
        Elicitation | ElicitationResult => {
            if result.dialect == HookDialect::Claude && exit_two {
                if value.is_object() {
                    result.ignored(
                        "/hookSpecificOutput",
                        "elicitation exit 2 ignores structured action and content",
                    );
                }
            } else {
                elicitation(result, context, value);
            }
        }
        MessageDisplay => {
            if !result.failed() {
                push_text(
                    result,
                    &value["displayContent"],
                    ProposedEffect::DisplayContent,
                );
            } else if value.get("displayContent").is_some() {
                result.ignored(
                    "/hookSpecificOutput/displayContent",
                    "failed display handler retains the original display",
                );
            }
        }
        InstructionsLoaded | StopFailure | Interrupt | SessionEnd | TaskCreated | TaskCompleted
        | TeammateIdle | WorktreeRemove | PreCompact | PostCompact | ConfigChange
        | DirectoryAdded => {
            if value.is_object() {
                result.ignored(
                    "/hookSpecificOutput",
                    "this event has no specific result effects",
                );
            }
        }
    }
    Ok(())
}
fn session_start(result: &mut DecodedResult, context: &ResultContext, value: &Value) -> WireResult {
    push_text(
        result,
        &value["additionalContext"],
        ProposedEffect::AdditionalContext,
    );
    if value.get("initialUserMessage").is_some() {
        if result.dialect == HookDialect::Claude && context.interactive_session {
            result.ignored(
                "/hookSpecificOutput/initialUserMessage",
                "source initial message is non-interactive only",
            );
        } else {
            push_text(
                result,
                &value["initialUserMessage"],
                ProposedEffect::InitialMessage,
            );
        }
    }
    if value.get("sessionTitle").is_some() {
        if result.dialect == HookDialect::Claude
            && matches!(
                context.session_source,
                SessionStartSource::Clear | SessionStartSource::Compact
            )
        {
            result.ignored(
                "/hookSpecificOutput/sessionTitle",
                "source ignores session title on clear and compact",
            );
        } else {
            push_text(result, &value["sessionTitle"], ProposedEffect::SessionTitle);
        }
    }
    watches(result, value)?;
    if value["reloadSkills"] == true {
        result.push(ProposedEffect::StageSkillRescan);
    }
    Ok(())
}
fn post_tool(result: &mut DecodedResult, context: &ResultContext, value: &Value) {
    push_text(
        result,
        &value["additionalContext"],
        ProposedEffect::AdditionalContext,
    );
    push_text(
        result,
        &value["classifierContext"],
        ProposedEffect::ClassifierContext,
    );
    if let Some(replacement) = value.get("updatedToolOutput") {
        result.push(ProposedEffect::ReplaceModelOutput {
            kind: ModelOutputKind::Tool,
            value: Untrusted::new(replacement.clone()),
        });
        if value.get("updatedMCPToolOutput").is_some() {
            result.ignored(
                "/hookSpecificOutput/updatedMCPToolOutput",
                "general tool replacement takes precedence",
            );
        }
    } else if let Some(replacement) = value.get("updatedMCPToolOutput") {
        if result.dialect == HookDialect::Claude
            && match replacement {
                Value::Null => true,
                Value::Bool(value) => !value,
                Value::Number(value) => value.as_f64() == Some(0.0),
                Value::String(value) => value.is_empty(),
                Value::Array(_) | Value::Object(_) => false,
            }
        {
            result.ignored(
                "/hookSpecificOutput/updatedMCPToolOutput",
                "Claude ignores falsy MCP output replacements",
            );
        } else if result.dialect == HookDialect::Codex && replacement.is_null() {
            result.ignored(
                "/hookSpecificOutput/updatedMCPToolOutput",
                "Codex treats optional null replacement as absent",
            );
        } else if context.tool_is_mcp {
            result.push(ProposedEffect::ReplaceModelOutput {
                kind: ModelOutputKind::McpTool,
                value: Untrusted::new(replacement.clone()),
            });
        } else {
            result.ignored(
                "/hookSpecificOutput/updatedMCPToolOutput",
                "MCP-only output replacement does not apply to this tool",
            );
        }
    }
}
fn watches(result: &mut DecodedResult, value: &Value) -> WireResult {
    if let Some(paths) = value.get("watchPaths") {
        let paths = paths
            .as_array()
            .ok_or_else(|| {
                WireError::new(
                    "/hookSpecificOutput/watchPaths",
                    "watch paths must be an array",
                )
            })?
            .iter()
            .map(|path| {
                watch_path(path.as_str().ok_or_else(|| {
                    WireError::new("/hookSpecificOutput/watchPaths", "watch path must be text")
                })?)
            })
            .collect::<WireResult<Vec<_>>>()?;
        // Build the whole list before publishing a single replacement proposal.
        result.push(ProposedEffect::ReplaceDynamicWatches(paths));
    }
    Ok(())
}
pub(super) fn worktree(
    result: &mut DecodedResult,
    context: &ResultContext,
    text: &str,
) -> WireResult {
    let path = if result.dialect == HookDialect::Claude && !std::path::Path::new(text).is_absolute()
    {
        let cwd = context.working_directory.as_ref().ok_or_else(|| {
            WireError::new(
                "/worktreePath",
                "relative source path requires the actual hook working directory",
            )
        })?;
        if !cwd.is_absolute() {
            return Err(WireError::new(
                "/context/workingDirectory",
                "hook working directory must be absolute",
            ));
        }
        if text.trim().is_empty() || text.chars().any(char::is_control) {
            return Err(WireError::new("/worktreePath", "invalid worktree path"));
        }
        let mut resolved = cwd.clone();
        for part in std::path::Path::new(text).components() {
            match part {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    resolved.pop();
                }
                std::path::Component::Normal(part) => resolved.push(part),
                _ => {
                    return Err(WireError::new(
                        "/worktreePath",
                        "unrepresentable source path",
                    ));
                }
            }
        }
        resolved
    } else {
        absolute_path(text)?
    };
    result.push(ProposedEffect::WorktreePath(path));
    Ok(())
}
fn watch_path(text: &str) -> WireResult<PathBuf> {
    let path = std::path::Path::new(text);
    if !path.is_absolute() || text.chars().any(char::is_control) {
        return Err(WireError::new(
            "/hookSpecificOutput/watchPaths",
            "watch path must be absolute and contain no control bytes",
        ));
    }
    // The watcher source contract requires absolute paths, but does not impose
    // WorktreeCreate's dot-component restriction. Admission resolves these paths.
    Ok(path.to_path_buf())
}
fn absolute_path(text: &str) -> WireResult<PathBuf> {
    let path = std::path::Path::new(text);
    if !path.is_absolute()
        || text.chars().any(char::is_control)
        || text.split('/').any(|part| matches!(part, "." | ".."))
    {
        return Err(WireError::new(
            "/path",
            "path must be an absolute normalized path without control bytes",
        ));
    }
    Ok(path.to_path_buf())
}
fn elicitation(result: &mut DecodedResult, context: &ResultContext, value: &Value) {
    let action = match value["action"].as_str() {
        Some("accept") => Some(ElicitationAction::Accept),
        Some("decline") => Some(ElicitationAction::Decline),
        Some("cancel") => Some(ElicitationAction::Cancel),
        _ => None,
    };
    let effective = action.or(if result.event == ElicitationResult {
        context.elicitation_action
    } else {
        None
    });
    let content = if effective == Some(ElicitationAction::Accept) {
        value.get("content").map(|v| Untrusted::new(v.clone()))
    } else {
        if value.get("content").is_some() {
            result.ignored(
                "/hookSpecificOutput/content",
                "content only applies to an accepted elicitation",
            );
        }
        None
    };
    if action.is_some() || content.is_some() {
        result.push(ProposedEffect::Elicitation { action, content });
    }
}

fn terminal(result: &mut DecodedResult, context: &ResultContext, value: &Value) {
    if let Some(text) = value["terminalSequence"].as_str() {
        match super::terminal::parse(text) {
            None => result.ignored(
                "/terminalSequence",
                "terminal value contains disallowed control bytes",
            ),
            Some(_) if !context.interactive_display => result.ignored(
                "/terminalSequence",
                "terminal notices require an active interactive display",
            ),
            Some(notices) => result.push(ProposedEffect::TerminalNotification(notices)),
        }
    }
}
fn system_message(result: &mut DecodedResult, context: &ResultContext, value: &Value) {
    let Some(text) = value["systemMessage"].as_str() else {
        return;
    };
    let text = Untrusted::new(text.into());
    if result.dialect != HookDialect::Claude {
        result.push(ProposedEffect::Warning(text));
        return;
    }
    match result.event {
        Notification | StopFailure | InstructionsLoaded | WorktreeCreate | WorktreeRemove
        | PreCompact | PostCompact | SessionEnd | Elicitation | ElicitationResult => result
            .ignored(
                "/systemMessage",
                "source discards systemMessage for this event",
            ),
        CwdChanged | FileChanged => {
            if context.interactive_display {
                result.push(ProposedEffect::TransientNotice(text));
            } else {
                result.ignored(
                    "/systemMessage",
                    "source only shows this notice on the interactive display",
                );
            }
        }
        DirectoryAdded => {
            if context.directory_source == DirectoryAddedSource::Command {
                result.push(ProposedEffect::AdditionalContext(text));
            } else {
                result.ignored(
                    "/systemMessage",
                    "SDK directory-add output is diagnostic only",
                );
            }
        }
        _ => result.push(ProposedEffect::Warning(text)),
    }
}
fn schedule(result: &mut DecodedResult, context: &ResultContext, value: &Value) -> WireResult {
    if context.work.cancelled || !context.work.allocation_available {
        return Err(WireError::new(
            "/async",
            "owner is cancelled or has no observer allowance",
        ));
    }
    if context.observer_timeout_ms == 0 {
        return Err(WireError::new(
            "/context/observerTimeout",
            "an observer needs a finite positive owner timeout",
        ));
    }
    let requested = value
        .get("asyncTimeout")
        .map(|v| {
            v.as_f64()
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or_else(|| {
                    WireError::new("/asyncTimeout", "async timeout must be finite and positive")
                })
        })
        .transpose()?;
    let timeout_ms = requested
        .map(|ms| ms.min(context.observer_timeout_ms as f64).ceil() as u64)
        .unwrap_or(context.observer_timeout_ms)
        .min(context.observer_timeout_ms);
    result.push(ProposedEffect::ScheduleObserver { timeout_ms });
    if context.role == ResultRole::RequiredGate {
        result.fail(WireError::new(
            "/async",
            "observer scheduling cannot satisfy a required gate",
        ));
    }
    Ok(())
}

pub(super) fn plain(result: &mut DecodedResult, context: &ResultContext, text: &str) -> WireResult {
    let context_event = match result.dialect {
        HookDialect::Claude => matches!(
            result.event,
            SessionStart
                | UserPromptSubmit
                | UserPromptExpansion
                | SubagentStart
                | Setup
                | PostModelSwitch
        ),
        HookDialect::Codex => matches!(
            result.event,
            SessionStart | SubagentStart | UserPromptSubmit
        ),
        HookDialect::Native => false,
    };
    if context_event {
        result.push(ProposedEffect::AdditionalContext(Untrusted::new(
            text.into(),
        )));
    } else if result.dialect == HookDialect::Native
        || (result.dialect == HookDialect::Codex
            && (result.event == Interrupt
                || (!context.asynchronous && matches!(result.event, Stop | SubagentStop))))
    {
        return Err(WireError::new(
            "/stdout",
            "this event requires structured output",
        ));
    } else {
        result.ignored(
            "/stdout",
            "source does not interpret plain stdout for this event",
        );
    }
    Ok(())
}

pub(super) fn exit_two(result: &mut DecodedResult, context: &ResultContext) {
    // Successful decoding never overrides the process failure. The source effect
    // still differs by event, independently of our stronger required-gate hold.
    if result.dialect == HookDialect::Codex && context.asynchronous {
        result.ignored(
            "/exitCode",
            "Codex asynchronous observer ignores exit 2 controls",
        );
        return;
    }
    let reason = result.stderr.as_ref().and_then(|text| {
        let text = text.get().trim();
        (!text.is_empty()).then(|| Untrusted::new(text.into()))
    });
    if result.dialect == HookDialect::Codex
        && !matches!(
            result.event,
            PreToolUse | PermissionRequest | PostToolUse | UserPromptSubmit | Stop | SubagentStop
        )
    {
        return;
    }
    if result.dialect == HookDialect::Codex && reason.is_none() {
        result.fail(WireError::new(
            "/stderr",
            "Codex exit 2 requires a nonempty reason",
        ));
        return;
    }
    if matches!(
        result.event,
        Stop | SubagentStop | TeammateIdle | TaskCompleted
    ) && result
        .effects
        .contains(&ProposedEffect::Control(ControlRequest::EndTurn))
    {
        result.ignored(
            "/command/exit",
            "supported continue:false stops without requesting follow-up",
        );
        return;
    }
    match result.event {
        PreToolUse | UserPromptSubmit | UserPromptExpansion | TaskCreated | PreModelSwitch => {
            force_deny(result, reason)
        }
        PermissionRequest if result.dialect != HookDialect::Claude => force_deny(result, reason),
        PermissionRequest => result.ignored(
            "/command/exit",
            "Claude PermissionRequest exit 2 has no decision effect",
        ),
        Stop => decisions::followup(result, context, FollowupTarget::Task, reason),
        SubagentStop => decisions::followup(result, context, FollowupTarget::Subagent, reason),
        TeammateIdle => decisions::followup(result, context, FollowupTarget::Teammate, reason),
        TaskCompleted if context.work.task_boundary == TaskBoundary::TeammateStop => {
            decisions::followup(result, context, FollowupTarget::Teammate, reason)
        }
        TaskCompleted => force_deny(result, reason),
        ConfigChange
            if result.dialect == HookDialect::Claude && context.config_is_managed_policy =>
        {
            result.ignored(
                "/command/exit",
                "source cannot block managed policy settings",
            )
        }
        ConfigChange | PreCompact => {
            force_deny(result, reason);
            if result.event == PreCompact && context.compaction_recovery {
                result.push(ProposedEffect::RetainContextLimitFailure);
            }
        }
        PostToolUse | PostToolUseFailure => {
            if let Some(reason) = reason {
                result.push(ProposedEffect::Feedback(reason));
            }
        }
        PostToolBatch if result.dialect != HookDialect::Native => {
            result.push(ProposedEffect::Control(ControlRequest::HoldContinuation));
            if let Some(reason) = reason {
                result.push(ProposedEffect::Feedback(reason));
            }
        }
        Elicitation | ElicitationResult => {
            result
                .effects
                .retain(|effect| !matches!(effect, ProposedEffect::Elicitation { .. }));
            result.push(ProposedEffect::Elicitation {
                action: Some(ElicitationAction::Decline),
                content: None,
            });
        }
        SessionStart | SubagentStart | PostModelSwitch | SessionEnd | CwdChanged | FileChanged
        | PostCompact => {
            if let Some(reason) = reason {
                result.push(ProposedEffect::Warning(reason));
            }
        }
        PostToolBatch | InstructionsLoaded | StopFailure | Interrupt | WorktreeCreate
        | WorktreeRemove | Setup | Notification | DirectoryAdded | MessageDisplay
        | PermissionDenied => {
            result.ignored(
                "/command/exit",
                "source has no exit-2 control effect for this event",
            );
        }
    }
}
fn force_deny(result: &mut DecodedResult, reason: Option<Untrusted<String>>) {
    let existing = result.effects.iter().any(|effect| {
        matches!(
            effect,
            ProposedEffect::Decision {
                choice: DecisionChoice::Deny,
                ..
            }
        )
    });
    result.effects.retain(|effect| {
        !matches!(
            effect,
            ProposedEffect::Decision {
                choice: DecisionChoice::NoObjection | DecisionChoice::Ask | DecisionChoice::Defer,
                ..
            }
        )
    });
    if !existing {
        decisions::deny(result, reason);
    }
}
pub(super) fn require_native_special(result: &mut DecodedResult) {
    let missing = match result.event {
        Elicitation | ElicitationResult => !result
            .effects
            .iter()
            .any(|e| matches!(e, ProposedEffect::Elicitation { .. })),
        MessageDisplay => !result
            .effects
            .iter()
            .any(|e| matches!(e, ProposedEffect::DisplayContent(_))),
        _ => false,
    };
    if missing {
        result.fail(WireError::new(
            "/hookSpecificOutput",
            "native special event requires an actual special result",
        ));
    }
}

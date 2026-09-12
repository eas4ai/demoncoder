//! Labeled evidence, with generated controls kept distinct from quoted content.
use std::fmt::{self, Write};

use super::{
    Page, Request, Summary, Target, agent_stage,
    pager::{MAX_PAGE, Pager},
    task_line,
};
use crate::{
    subagents::state::{AgentRecord, AgentStatus, RoleReceipt},
    workflow::{
        runtime::Record,
        state::{CheckReceipt, ReviewReceipt, Task},
    },
};

pub(super) fn page(record: &Record, request: Request) -> Page {
    let mut out = Pager::new(request.page);
    let result = match request.target {
        Target::Learning => learning_context(&mut out, record),
        Target::Overview => overview(&mut out, record),
        Target::Task(id) => {
            if let Some(task) = record.task.as_ref().filter(|task| task.id == id) {
                task_report(&mut out, record, task, false)
            } else if let Some(archived) = record.archived.iter().find(|entry| entry.task.id == id)
            {
                task_report(&mut out, record, &archived.task, true)
            } else {
                writeln!(
                    out,
                    "Task {id} is no longer retained. Select another target with Tab."
                )
            }
        }
        Target::Agent(id) => match record.agents.iter().find(|agent| agent.id == id) {
            Some(agent) => agent_report(&mut out, record, agent),
            None => writeln!(
                out,
                "Agent {id} is not retained. Select another target with Tab."
            ),
        },
    };
    if result.is_err() && !out.more {
        out.text
            .push_str("\nEvidence formatting failed; this page is incomplete.\n");
    }
    if out.text.is_empty() {
        out.text
            .push_str("No evidence on this page. Use Left for the preceding page.\n");
    }
    if request.page >= MAX_PAGE && out.more {
        out.text.push_str("\nInspection page limit reached; preserve the private session record for remaining evidence.\n");
        out.more = false;
    }
    Page {
        request,
        text: out.text,
        more: out.more,
    }
}

fn learning_context(out: &mut Pager, record: &Record) -> fmt::Result {
    writeln!(
        out,
        "Saved coding context. Use /improvements to refresh the workspace catalog; inspection alone runs no checks."
    )?;
    for receipt in &record.learning_context {
        writeln!(
            out,
            "Coding target: {} · Workspace: {} · Selected lessons: {} · Omitted matches: {}",
            receipt.target,
            receipt.coding_workspace.display(),
            receipt.lessons.len(),
            receipt.omitted_matches
        )?;
        quote(
            out,
            "Prepared coding context (provider receipts show whether delivery completed):",
            &receipt.supplied_text,
        )?;
    }
    Ok(())
}

fn quote(out: &mut Pager, heading: &str, value: &str) -> fmt::Result {
    writeln!(out, "{heading}")?;
    for line in value.split('\n') {
        writeln!(out, "  | {line}")?;
    }
    Ok(())
}

fn overview(out: &mut Pager, record: &Record) -> fmt::Result {
    let summary = Summary::from_record(record);
    writeln!(out, "{}", summary.counts())?;
    writeln!(
        out,
        "{}",
        summary
            .task
            .as_deref()
            .unwrap_or("No explicit task · ordinary conversation is not verified or accepted.")
    )?;
    if record.recovery_pending {
        writeln!(
            out,
            "Inspection required: interrupted work is uncertain. Inspect actual effects before /reconcile EXPLANATION; nothing replays automatically."
        )?;
    }
    if let Some(allocation) = &record.allocation {
        match allocation.remaining_ms() {
            Ok(ms) => writeln!(
                out,
                "Allocation: {}s remaining · native calls {}/{} · tools {}/{}",
                ms / 1000,
                allocation.model_calls,
                allocation.limits.model_calls,
                allocation.tool_calls,
                allocation.limits.tool_calls
            )?,
            Err(_) => writeln!(
                out,
                "Allocation unavailable: clock is uncertain; execution is held."
            )?,
        }
    }
    if let Some(grant) = &record.session_hook_allowance {
        let allocation = &grant.allocation;
        writeln!(
            out,
            "Session hook grant: model/backend slots {}/{} · host backend invocations {} · snapshot tools {}/{}",
            allocation.model_calls,
            allocation.limits.model_calls,
            grant.backend_invocations,
            allocation.tool_calls,
            allocation.limits.tool_calls
        )?;
        match allocation.remaining_ms() {
            Ok(ms) => writeln!(out, "Session hook time remaining: {}s", ms / 1000)?,
            Err(_) => writeln!(
                out,
                "Session hook time unavailable: clock is uncertain; execution is held."
            )?,
        }
    }
    if let Some(delegation) = &record.delegation {
        writeln!(
            out,
            "Active limit: {} · backend invocations {}/{}",
            delegation.max_active, record.backend_invocations, delegation.backend_limit
        )?;
    }
    writeln!(
        out,
        "\nTab selects a task or agent. Left/Right change evidence pages.\nF5 refreshes saved state; it does not run checks or scan files.\nControls are requests; the runtime rechecks authority and current files."
    )?;
    for agent in &record.agents {
        writeln!(
            out,
            "\nAgent {} · {} · {}",
            agent.id,
            agent.request.connection,
            agent_stage(
                agent.status,
                agent.orchestration.as_ref().map(|state| state.stage)
            )
        )?;
        quote(out, "Objective:", &agent.request.objective)?;
        quote(out, "Reason:", &agent.outcome)?;
    }
    if !record.archived.is_empty() {
        writeln!(
            out,
            "\nArchived tasks: {} · Tab visits retained history.",
            record.archived.len()
        )?;
    }
    if let Some(receipt) = &record.unattributed_usage {
        writeln!(
            out,
            "Unresolved usage attribution: {} reports without an exact model/backend recipient; totals retained, no grant charged. {:?}",
            receipt.reports, receipt.usage
        )?;
    }
    for operation in &record.operations {
        if let Some(receipt) = &operation.usage_receipt
            && let Some(reason) = &receipt.unresolved
        {
            writeln!(
                out,
                "Unresolved usage attribution for operation {}: {:?}; {} reports; missing report {}; totals retained, no grant inferred. {:?}",
                operation.id, reason, receipt.reports, receipt.missing_report, receipt.usage
            )?;
        }
    }
    lifecycle_report(out, record, None)
}

fn task_report(out: &mut Pager, record: &Record, task: &Task, archived: bool) -> fmt::Result {
    writeln!(
        out,
        "{}{}",
        if archived { "Archived · " } else { "" },
        task_line(
            task,
            if archived {
                task.accepted.as_deref()
            } else {
                record.last_snapshot.as_deref()
            }
        )
    )?;
    quote(out, "Objective:", &task.objective)?;
    lifecycle_report(out, record, Some(task.id))?;
    writeln!(
        out,
        "Connection: {} · model {}",
        task.creator_identity
            .as_ref()
            .unwrap_or(&record.identity)
            .display_adapter(),
        task.creator_identity
            .as_ref()
            .unwrap_or(&record.identity)
            .display_model()
    )?;
    writeln!(
        out,
        "Corrections: {}/{}\nBaseline snapshot: {}",
        task.corrections, task.correction_limit, task.baseline.digest
    )?;
    writeln!(
        out,
        "Freshness: recorded results only; files not rechecked. Actions recheck files."
    )?;
    task_actions(out, record, task, archived)?;
    if let Some(link) = &task.improvement {
        writeln!(
            out,
            "Improvement candidate: {} · catalog {}",
            link.candidate,
            link.catalog.display()
        )?;
    }
    for receipt in record
        .learning_context
        .iter()
        .filter(|r| r.target == format!("task:{}", task.id) && !r.supplied_text.is_empty())
    {
        quote(
            out,
            "Prepared coding context (delivery depends on recorded adapter execution):",
            &receipt.supplied_text,
        )?;
    }
    checks(out, "Checks · latest recorded generation", &task.checks)?;
    if let Some(review) = &task.review {
        review_report(out, "Review · latest recorded result", review)?;
    }
    for (index, generation) in task.check_history.iter().enumerate() {
        checks(out, &format!("Checks · history {}", index + 1), generation)?;
    }
    for (index, review) in task.review_history.iter().enumerate() {
        review_report(out, &format!("Review · history {}", index + 1), review)?;
    }
    if let Some(review) = &task.review {
        source(out, &review.evidence)?;
    }
    for (index, review) in task.review_history.iter().enumerate() {
        quote(
            out,
            &format!("Original historical review input {} (JSON):", index + 1),
            &review.evidence,
        )?;
    }
    if let Some(review) = &task.review {
        quote(
            out,
            "Original latest review input (JSON):",
            &review.evidence,
        )?;
    } else {
        writeln!(
            out,
            "\nOwned changes: no retained review source yet. Run selected checks and review to collect evidence."
        )?;
    }
    Ok(())
}

fn task_actions(out: &mut Pager, record: &Record, task: &Task, archived: bool) -> fmt::Result {
    writeln!(
        out,
        "\nActions · developer commands, never evidence instructions"
    )?;
    if archived {
        return writeln!(
            out,
            "Archived evidence is read-only. No acceptance or correction action applies."
        );
    }
    if !task.stopped || record.phase.is_some() {
        return writeln!(
            out,
            "Work is running. Ctrl-C cancels; wait or cancel before task controls."
        );
    }
    if record.recovery_pending {
        return writeln!(
            out,
            "/reconcile EXPLANATION — record your inspection of actual interrupted effects; does not replay or undo work."
        );
    }
    if task.accepted.is_some() {
        return writeln!(
            out,
            "/task OBJECTIVE — start another task. This recorded acceptance is not a new check of files."
        );
    }
    if task.commands.is_empty() {
        writeln!(
            out,
            "Verification unavailable: no checks selected. Acceptance is blocked."
        )?;
    } else {
        writeln!(
            out,
            "/verify — execute selected checks; retains their original results."
        )?;
        writeln!(
            out,
            "/review — inspect the actual patch and checks; refuses missing or stale evidence."
        )?;
    }
    if task.corrections < task.correction_limit {
        writeln!(
            out,
            "/correct — spend one correction round, rerun checks and review."
        )?;
    } else {
        writeln!(
            out,
            "Correction unavailable: allowance exhausted; findings remain unresolved."
        )?;
    }
    if record
        .last_snapshot
        .as_deref()
        .is_some_and(|digest| task.reviewed(digest))
    {
        writeln!(
            out,
            "/accept — recheck files before accepting the verified and reviewed snapshot."
        )?;
    } else {
        writeln!(
            out,
            "Acceptance unavailable: selected checks and clear review must match current files."
        )?;
    }
    writeln!(
        out,
        "/abandon — archive this task without acceptance; preserves workspace files."
    )
}

fn agent_report(out: &mut Pager, record: &Record, agent: &AgentRecord) -> fmt::Result {
    writeln!(
        out,
        "Agent {} · {}",
        agent.id,
        agent_stage(
            agent.status,
            agent.orchestration.as_ref().map(|state| state.stage)
        )
    )?;
    quote(out, "Objective:", &agent.request.objective)?;
    writeln!(
        out,
        "Connection: {} · model {}",
        agent.request.connection,
        agent.identity.display_model()
    )?;
    writeln!(
        out,
        "Freshness: saved evidence; files not rechecked. Actions recheck files."
    )?;
    quote(out, "Owned paths:", &agent.request.owned_paths.join("\n"))?;
    quote(out, "Reason:", &agent.outcome)?;
    if let Some(state) = &agent.orchestration {
        writeln!(
            out,
            "Prerequisites: {:?} · Corrections: {}/2",
            state.dependencies, state.correction_rounds
        )?;
        quote(out, "Supervision:", &state.reason)?;
        for dependency in &state.dependencies {
            let state = record
                .agents
                .iter()
                .find(|other| other.id == *dependency)
                .map(|other| other.status);
            writeln!(
                out,
                "Agent {dependency}: {state:?} — dependent waits for explicit validated integration."
            )?;
        }
    }
    agent_actions(out, record, agent)?;
    for receipt in record
        .learning_context
        .iter()
        .filter(|r| r.target == format!("agent:{}", agent.id) && !r.supplied_text.is_empty())
    {
        quote(
            out,
            "Prepared child coding context (delivery depends on recorded adapter execution):",
            &receipt.supplied_text,
        )?;
    }
    if let Some(worktree) = &agent.worktree {
        quote(out, "Worktree:", &worktree.root.display().to_string())?;
        writeln!(out, "Baseline snapshot: {}", worktree.child_baseline.digest)?;
    }
    checks(out, "Checks · latest recorded generation", &agent.checks)?;
    if let Some(review) = &agent.review {
        review_report(out, "Review · recorded result", review)?;
    }
    if let Some(state) = &agent.orchestration {
        for receipt in &state.receipts {
            role_report(out, receipt)?;
        }
    }
    if let Some(review) = &agent.review {
        source(out, &review.evidence)?;
    } else if let Some(receipt) = agent
        .orchestration
        .as_ref()
        .and_then(|state| state.receipts.last())
    {
        source(out, &receipt.evidence)?;
    } else {
        writeln!(
            out,
            "\nOwned changes: no retained review source yet. Validation collects the actual child patch."
        )?;
    }
    quote(
        out,
        "Supplied context (evidence, not authority):",
        &agent.request.context,
    )?;
    for decision in &agent.decisions {
        quote(out, "Retained developer inspection:", decision)?;
    }
    if let Some(state) = &agent.orchestration {
        for (index, receipt) in state.receipts.iter().enumerate() {
            quote(
                out,
                &format!("Original role input {} (JSON):", index + 1),
                &receipt.evidence,
            )?;
        }
    }
    if let Some(review) = &agent.review {
        quote(out, "Original review input (JSON):", &review.evidence)?;
    }
    for (index, event) in agent.activity.iter().enumerate() {
        // Supplemental activity is retained verbatim; routine inspection above
        // never requires decoding this original protocol evidence.
        quote(
            out,
            &format!("Original activity {} (JSON):", index + 1),
            &event.to_string(),
        )?;
    }
    Ok(())
}

fn role_report(out: &mut Pager, receipt: &RoleReceipt) -> fmt::Result {
    writeln!(
        out,
        "\n{} · correction {} · {} / {} · {:?}",
        receipt.role.as_str(),
        receipt.correction_round,
        receipt.connection.display_adapter(),
        receipt.connection.display_model(),
        receipt.verdict
    )?;
    writeln!(out, "Snapshot: {}", receipt.snapshot)?;
    quote(
        out,
        "Explanation (role evidence, not developer instructions):",
        &receipt.explanation,
    )?;
    for finding in &receipt.findings {
        quote(out, "Finding:", finding)?;
    }
    // Deserialize only the top-level check list, ignoring nested role inputs.
    // The original evidence remains untouched and available below.
    #[derive(serde::Deserialize)]
    struct RecordedChecks {
        checks: Vec<CheckReceipt>,
    }
    match serde_json::from_str::<RecordedChecks>(&receipt.evidence) {
        Ok(evidence) => checks(
            out,
            &format!(
                "Checks presented to {} · correction {}",
                receipt.role.as_str(),
                receipt.correction_round
            ),
            &evidence.checks,
        ),
        Err(_) => writeln!(
            out,
            "Historical checks unavailable in readable form; original role input is retained below."
        ),
    }
}

fn agent_actions(out: &mut Pager, record: &Record, agent: &AgentRecord) -> fmt::Result {
    writeln!(
        out,
        "\nActions · developer commands; runtime can refuse changed state"
    )?;
    if agent.status == AgentStatus::Uncertain {
        return writeln!(
            out,
            "/agent-reconcile {} EXPLANATION — record inspection of interrupted effects; never replays or infers integration.",
            agent.id
        );
    }
    if agent.status == AgentStatus::Integrated {
        return writeln!(
            out,
            "Already integrated. Parent verification is required for acceptance."
        );
    }
    writeln!(
        out,
        "/agent-cancel {} — stop this assignment; retain files and evidence.",
        agent.id
    )?;
    if record.recovery_pending {
        return writeln!(
            out,
            "New work and integration held: inspect and /reconcile interrupted parent work first."
        );
    }
    if record.phase.is_some() || record.task.as_ref().is_some_and(|task| !task.stopped) {
        return writeln!(
            out,
            "Validation/integration unavailable while parent work runs; wait or cancel the parent."
        );
    }
    if record
        .allocation
        .as_ref()
        .is_some_and(|allocation| allocation.remaining_ms().map_or(true, |ms| ms == 0))
    {
        return writeln!(
            out,
            "Validation/integration unavailable: shared deadline exhausted or clock uncertain."
        );
    }
    match agent.status {
        AgentStatus::Queued => writeln!(
            out,
            "Waiting: prerequisites need explicit validated integration and free active capacity. After recovery, /agents-resume resumes eligible queued work."
        ),
        AgentStatus::Ready => writeln!(
            out,
            "/agent-integrate {} — recheck evidence and conflicts, then apply validated changes to the parent workspace; requires free active capacity.",
            agent.id
        ),
        _ if agent.completed
            && matches!(agent.status, AgentStatus::Stopped | AgentStatus::Failed)
            && !agent.commands.is_empty()
            && agent.reviewer.is_some() =>
        {
            writeln!(
                out,
                "/agent-validate {} — run selected checks and review; shared limits and correction allowance still apply.",
                agent.id
            )
        }
        _ => writeln!(
            out,
            "Validation/integration unavailable: work must be completed and not cancelled; selected checks and reviewer are required. Integration also requires passing checks and clear current review."
        ),
    }
}

fn checks(out: &mut Pager, heading: &str, checks: &[CheckReceipt]) -> fmt::Result {
    writeln!(out, "\n{heading}")?;
    if checks.is_empty() {
        return writeln!(out, "No results. Missing checks mean unverified.");
    }
    for (index, check) in checks.iter().enumerate() {
        writeln!(
            out,
            "Check {} · {} · exit {:?} · snapshot {}",
            index + 1,
            if check.success {
                "recorded pass"
            } else {
                "recorded fail"
            },
            check.exit_code,
            check.snapshot
        )?;
        quote(out, "Command:", &check.command)?;
        quote(out, "Original output:", &check.output)?;
    }
    Ok(())
}

fn review_report(out: &mut Pager, heading: &str, review: &ReviewReceipt) -> fmt::Result {
    writeln!(
        out,
        "\n{heading} · {} · {}",
        review.reviewer,
        if review.clear {
            "recorded clear"
        } else {
            "recorded blocked"
        }
    )?;
    writeln!(
        out,
        "Snapshot: {} · check generation {}",
        review.snapshot, review.verification_generation
    )?;
    quote(out, "Explanation:", &review.explanation)?;
    for finding in &review.findings {
        quote(out, "Finding:", finding)?;
    }
    Ok(())
}

fn source(out: &mut Pager, evidence: &str) -> fmt::Result {
    let parsed = serde_json::from_str::<serde_json::Value>(evidence);
    let Some(source) = parsed
        .as_ref()
        .ok()
        .and_then(|value| value.get("source_evidence"))
        .and_then(|value| value.as_str())
    else {
        return quote(out, "Source evidence (original representation):", evidence);
    };
    writeln!(
        out,
        "\nOwned changes and source · captured for review, not a fresh filesystem scan"
    )?;
    for line in source.lines() {
        if let Some((label, json)) = line.split_once("complete content (JSON string): ")
            && let Ok(text) = serde_json::from_str::<String>(json)
        {
            quote(out, label.trim(), &text)?;
        } else {
            writeln!(out, "  | {line}")?;
        }
    }
    Ok(())
}

fn lifecycle_report(out: &mut Pager, record: &Record, task: Option<u64>) -> fmt::Result {
    for operation in &record.operations {
        if let Some(crate::workflow::runtime::HostInvocation::NativeSession(lifetime)) =
            &operation.host_invocation
        {
            writeln!(
                out,
                "\nNative session lifetime {} · {:?} · {:?}",
                operation.id, lifetime.source, lifetime.end
            )?;
            for diagnostic in &lifetime.diagnostics {
                quote(out, "Session diagnostic:", diagnostic)?;
            }
        }
        if let Some(crate::workflow::runtime::HostInvocation::NativeTurn(turn)) =
            &operation.host_invocation
            && turn.task == task
        {
            writeln!(
                out,
                "\nNative turn {} · {} · {:?} · owner {}",
                operation.id,
                match turn.origin {
                    crate::plugins::receipts::NativeTurnOrigin::Developer => "developer",
                    crate::plugins::receipts::NativeTurnOrigin::PluginContext => "plugin context",
                },
                turn.end,
                turn.owner_phase.as_deref().unwrap_or("no workflow phase")
            )?;
            for diagnostic in &turn.diagnostics {
                quote(out, "Turn diagnostic:", diagnostic)?;
            }
        }
    }
    for receipt in record
        .operations
        .iter()
        .filter_map(|o| o.non_tool_receipt())
        .filter(|r| r.facts.task == task)
    {
        writeln!(
            out,
            "\nLifecycle {} · operation {} · {}",
            receipt.facts.subject.occurrence.event().as_str(),
            receipt.facts.operation,
            if receipt.hold.is_some() {
                "unmet"
            } else if receipt.settled {
                "settled"
            } else {
                "pending; never replay automatically"
            }
        )?;
        if let Some(turn) = receipt.facts.native_turn {
            writeln!(
                out,
                "Native turn {turn} · host translation (not an external backend callback)"
            )?;
        } else {
            writeln!(
                out,
                "No recorded native turn linkage (legacy or external occurrence)"
            )?;
        }
        if let crate::plugins::receipts::NonToolOccurrence::UserPromptSubmit { prompt, .. } =
            &receipt.facts.subject.occurrence
        {
            quote(
                out,
                "Original submitted prompt (retained even when blocked):",
                prompt,
            )?;
        }
        if let Some(reason) = &receipt.hold {
            quote(out, "Unmet gate reason:", reason)?;
        }
        for message in &receipt.messages {
            quote(
                out,
                &format!("Plugin-origin {}:", message.package),
                &message.text,
            )?;
        }
        for hook in &receipt.hooks {
            if let Some(crate::plugins::receipts::RawOutcome::Failure { reason }) = &hook.outcome {
                quote(
                    out,
                    &format!("Plugin-origin {} failure:", hook.declaration.package),
                    reason,
                )?;
            }
        }
        for diagnostic in &receipt.diagnostics {
            quote(out, "Hook diagnostic:", diagnostic)?;
        }
    }
    Ok(())
}

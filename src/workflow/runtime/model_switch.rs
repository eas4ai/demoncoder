//! One durable owner for an admitted Creator identity transition.
use super::{
    BudgetRef, ContextBinding, HostInvocation, Identity, Operation, Record, SharedRuntime,
};
use crate::{config::Connection, plugins::admission::digest};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Prepared,
    Teardown,
    Applied,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSwitch {
    pub version: u32,
    pub from: Identity,
    pub requested: Identity,
    pub from_model: Option<String>,
    pub actual_from_model: Option<String>,
    pub requested_model: Option<String>,
    pub candidate_model: Option<String>,
    pub source_candidate: bool,
    pub actual_model: Option<String>,
    pub source: String,
    pub native_session: Option<u64>,
    pub task: Option<u64>,
    pub pre_plan: Option<String>,
    pub post_plan: Option<String>,
    pub pins: String,
    pub stage: Stage,
    pub hold: Option<String>,
}

fn operation(record: &Record, id: u64) -> Result<(&Operation, &ModelSwitch)> {
    let operation = record
        .operations
        .iter()
        .find(|operation| operation.id == id)
        .context("model switch owner missing")?;
    let Some(HostInvocation::ModelSwitch(switch)) = &operation.host_invocation else {
        anyhow::bail!("operation is not a model switch")
    };
    ensure!(
        switch.version == 1
            && operation.phase == "model-switch"
            && operation.identity.as_ref() == Some(&switch.from)
            && !operation.complete
            && !operation.reconciled
            && switch.hold.is_none(),
        "model switch owner changed, ended, or is held"
    );
    ensure!(
        switch.task == record.task.as_ref().map(|task| task.id)
            && switch.pins
                == digest(&(
                    &switch.from,
                    &switch.requested,
                    &switch.source,
                    &switch.pre_plan,
                    &switch.post_plan,
                    &record.plugin_activations,
                    switch.task,
                ))?,
        "model switch policy, selection, or task changed"
    );
    match switch.stage {
        Stage::Prepared | Stage::Teardown => ensure!(
            record.identity == switch.from && switch.actual_model.is_none(),
            "prepared model switch no longer owns the old identity"
        ),
        Stage::Applied => ensure!(
            record.identity == switch.requested,
            "applied model switch does not own the installed identity"
        ),
    }
    Ok((operation, switch))
}

fn original_lifetime_to<'a>(
    record: &'a Record,
    lifetime_id: u64,
    target: &Identity,
) -> Result<(
    &'a Operation,
    &'a super::plugin_session::NativeSessionLifetime,
)> {
    original_lifetime_before(record, lifetime_id, u64::MAX, target, true)
}

pub(super) fn validate_original_lifetime_current(
    record: &Record,
    lifetime_id: u64,
) -> Result<(u64, u64)> {
    let (_, lifetime) = original_lifetime_before_with_root(
        record,
        lifetime_id,
        u64::MAX,
        &record.identity,
        false,
        false,
    )?;
    Ok(lifetime.workspace)
}

fn original_lifetime_before<'a>(
    record: &'a Record,
    lifetime_id: u64,
    before: u64,
    target: &Identity,
    require_open: bool,
) -> Result<(
    &'a Operation,
    &'a super::plugin_session::NativeSessionLifetime,
)> {
    original_lifetime_before_with_root(record, lifetime_id, before, target, require_open, true)
}

fn original_lifetime_before_with_root<'a>(
    record: &'a Record,
    lifetime_id: u64,
    before: u64,
    target: &Identity,
    require_open: bool,
    require_physical_root: bool,
) -> Result<(
    &'a Operation,
    &'a super::plugin_session::NativeSessionLifetime,
)> {
    let (operation, lifetime) = if require_physical_root {
        super::plugin_session::validate_host_lifetime(record, lifetime_id)?
    } else {
        let owner = super::plugin_session::validate_host_authority(record, lifetime_id)?;
        super::workspace_change::validate_lifetime_lineage(record, lifetime_id)?;
        owner
    };
    let mut identity = operation
        .identity
        .clone()
        .context("original host lifetime identity missing")?;
    for candidate in record
        .operations
        .iter()
        .filter(|candidate| candidate.id > lifetime_id && candidate.id < before)
    {
        let Some(HostInvocation::ModelSwitch(switch)) = &candidate.host_invocation else {
            continue;
        };
        if candidate.complete
            && !candidate.reconciled
            && switch.hold.is_none()
            && switch.stage == Stage::Applied
            && switch.native_session == Some(lifetime_id)
            && switch.from == identity
        {
            identity = switch.requested.clone();
        }
    }
    ensure!(
        &identity == target && (!require_open || lifetime.end.is_none()),
        "model switch is not chained from its exact live original host lifetime"
    );
    Ok((operation, lifetime))
}

pub(super) fn owner<'a>(
    record: &'a Record,
    id: u64,
    event: crate::plugins::hook_types::HookEvent,
) -> Result<super::plugin_non_tool::owner::Owner<'a>> {
    let (_, switch) = operation(record, id)?;
    let identity = match event {
        crate::plugins::hook_types::HookEvent::PreModelSwitch => {
            ensure!(
                matches!(switch.stage, Stage::Prepared | Stage::Teardown),
                "PreModelSwitch is stale"
            );
            &switch.from
        }
        crate::plugins::hook_types::HookEvent::PostModelSwitch => {
            ensure!(
                switch.stage == Stage::Applied,
                "PostModelSwitch precedes application"
            );
            &switch.requested
        }
        _ => anyhow::bail!("event is not owned by a model switch"),
    };
    Ok(super::plugin_non_tool::owner::Owner {
        identity,
        root: &record.workspace,
        child: None,
    })
}

pub(super) fn hook_lifetime(
    record: &Record,
    id: u64,
    event: crate::plugins::hook_types::HookEvent,
) -> Result<Option<u64>> {
    let _ = owner(record, id, event)?;
    let (switch_operation, switch) = operation(record, id)?;
    let Some(lifetime_id) = switch.native_session else {
        return Ok(None);
    };
    let (lifetime_operation, lifetime) = original_lifetime_to(record, lifetime_id, &switch.from)?;
    ensure!(
        lifetime.end.is_none() && lifetime_operation.budget == switch_operation.budget,
        "model switch original host lifetime changed or ended"
    );
    Ok(Some(lifetime_id))
}

pub(super) fn validate_occurrence(
    record: &Record,
    id: u64,
    occurrence: &crate::plugins::receipts::NonToolOccurrence,
    plan: &str,
) -> Result<()> {
    use crate::plugins::receipts::NonToolOccurrence;
    let (_, switch) = operation(record, id)?;
    match occurrence {
        NonToolOccurrence::PreModelSwitch {
            model_switch,
            requested_model,
            resolved_model,
            source,
        } => ensure!(
            *model_switch == id
                && matches!(switch.stage, Stage::Prepared | Stage::Teardown)
                && requested_model == &switch.requested_model
                && resolved_model == &switch.candidate_model
                && source == &switch.source
                && switch.pre_plan.as_deref() == Some(plan),
            "PreModelSwitch differs from its admitted candidate"
        ),
        NonToolOccurrence::PostModelSwitch {
            model_switch,
            model,
            source,
        } => ensure!(
            *model_switch == id
                && switch.stage == Stage::Applied
                && model == &switch.actual_model
                && source == &switch.source
                && switch.post_plan.as_deref() == Some(plan),
            "PostModelSwitch lacks the actual applied candidate"
        ),
        _ => anyhow::bail!("occurrence is not a model switch"),
    }
    Ok(())
}

pub(super) fn service_anchor(record: &Record, id: u64, lifetime_id: u64) -> Result<String> {
    let switch_operation = record
        .operations
        .iter()
        .find(|operation| operation.id == id)
        .context("model switch service owner missing")?;
    let Some(HostInvocation::ModelSwitch(switch)) = &switch_operation.host_invocation else {
        anyhow::bail!("model switch service owner has different authority")
    };
    ensure!(
        switch.version == 1
            && switch_operation.phase == "model-switch"
            && switch_operation.identity.as_ref() == Some(&switch.from)
            && !switch_operation.reconciled
            && switch.hold.is_none()
            && (!switch_operation.complete || switch.stage == Stage::Applied)
            && switch.task == record.task.as_ref().map(|task| task.id)
            && switch.pins
                == digest(&(
                    &switch.from,
                    &switch.requested,
                    &switch.source,
                    &switch.pre_plan,
                    &switch.post_plan,
                    &record.plugin_activations,
                    switch.task,
                ))?,
        "model switch service owner changed, was reconciled, or ended before application"
    );
    let (lifetime_operation, lifetime) =
        original_lifetime_before(record, lifetime_id, id, &switch.from, true)?;
    ensure!(
        switch.native_session == Some(lifetime_id)
            && lifetime.end.is_none()
            && lifetime_operation.budget == switch_operation.budget,
        "model switch service lost its original host lifetime"
    );
    digest(&(
        &lifetime_operation.identity,
        lifetime_id,
        &lifetime.workspace,
        &lifetime.session,
        &lifetime.plans,
    ))
}

pub(super) fn validate_source(
    record: &Record,
    id: u64,
    callback: &crate::plugins::receipts::SourceCallback,
    input: &crate::plugins::receipts::ObservedLifecycle,
) -> Result<()> {
    let (_, switch) = operation(record, id)?;
    let crate::plugins::receipts::ObservedLifecycle::Claude(input) = input else {
        anyhow::bail!("model switch source is not a genuine Claude callback")
    };
    let event = input["hook_event_name"].as_str();
    ensure!(
        switch.from.adapter == "claude"
            && callback.backend_operation == id
            && callback.origin.is_none()
            && input["source"] == "sdk"
            && input["from_model"].as_str() == switch.actual_from_model.as_deref()
            && input["to_model"].as_str() == switch.candidate_model.as_deref()
            && input.get("requested_model")
                == Some(
                    &switch
                        .requested_model
                        .as_ref()
                        .map_or(Value::Null, |model| Value::String(model.clone())),
                )
            && matches!(
                (event, switch.stage),
                (Some("PreModelSwitch"), Stage::Prepared | Stage::Teardown)
                    | (Some("PostModelSwitch"), Stage::Applied)
            ),
        "Claude model switch callback differs from its exact resolved candidate"
    );
    Ok(())
}

impl SharedRuntime {
    pub(crate) fn begin_model_switch(
        &self,
        old: &Connection,
        requested: &Connection,
        source: &str,
        native_session: Option<u64>,
        pre_plan: Option<String>,
        post_plan: Option<String>,
    ) -> Result<u64> {
        let session = self.plugin_session()?;
        self.update(|record| {
            let from = Identity::from(old);
            let requested_identity = Identity::from(requested);
            ensure!(
                !record.recovery_pending
                    && record.phase.is_none()
                    && record.identity == from
                    && requested_identity != from
                    && record
                        .task
                        .as_ref()
                        .is_none_or(|task| task.accepted.is_some()),
                "model switch is not at its admitted taskless boundary"
            );
            ensure!(
                source == "settings" && record.operations.len() < 4096,
                "invalid or unbounded model switch source"
            );
            ensure!(
                !record.operations.iter().any(|operation| {
                    matches!(
                        operation.host_invocation,
                        Some(HostInvocation::ModelSwitch(_))
                    ) && !operation.complete
                }),
                "model switch already pending; never replay"
            );
            let budget = match native_session {
                Some(id) => {
                    original_lifetime_to(record, id, &from)?;
                    super::budget_accounting::inherited(record, id)?
                }
                None => BudgetRef::Unallocated,
            };
            super::budget_accounting::active(record, &session, &budget)?;
            let task = record.task.as_ref().map(|task| task.id);
            let pins = digest(&(
                &from,
                &requested_identity,
                source,
                &pre_plan,
                &post_plan,
                &record.plugin_activations,
                task,
            ))?;
            let id = record.operations.len() as u64 + 1;
            record.operations.push(Operation {
                id,
                budget: Some(budget),
                usage_receipt: None,
                phase: "model-switch".into(),
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::ModelSwitch(Box::new(ModelSwitch {
                    version: 1,
                    from_model: old.model.clone(),
                    actual_from_model: old.model.clone(),
                    requested_model: requested.model.clone(),
                    candidate_model: requested.model.clone(),
                    source_candidate: false,
                    actual_model: None,
                    from,
                    requested: requested_identity,
                    source: source.into(),
                    native_session,
                    task,
                    pre_plan,
                    post_plan,
                    pins,
                    stage: Stage::Prepared,
                    hold: None,
                }))),
                complete: false,
                reconciled: false,
                usage_reported: true,
                identity: Some(record.identity.clone()),
            });
            Ok(id)
        })
    }

    pub(crate) fn begin_model_switch_teardown(&self, id: u64) -> Result<()> {
        let session = self.plugin_session()?;
        self.update(|record| {
            let (operation, switch) = operation(record, id)?;
            let budget = operation
                .budget
                .clone()
                .context("model switch funding missing")?;
            let native_session = switch.native_session;
            let from = switch.from.clone();
            ensure!(
                !record.recovery_pending
                    && record.phase.is_none()
                    && record
                        .task
                        .as_ref()
                        .is_none_or(|task| task.accepted.is_some()),
                "model switch is no longer at its admitted release boundary"
            );
            let original_allowance_live =
                match super::budget_accounting::active(record, &session, &budget)? {
                    Some(allocation) => allocation.remaining_ms()? > 0,
                    None => true,
                };
            if let Some(lifetime_id) = native_session {
                let (lifetime_operation, _) =
                    original_lifetime_before(record, lifetime_id, id, &from, true)?;
                ensure!(
                    lifetime_operation.budget == Some(budget) && original_allowance_live,
                    "model switch original host allowance ended before release"
                );
            }
            let Some(HostInvocation::ModelSwitch(switch)) = &mut record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated")
                .host_invocation
            else {
                unreachable!()
            };
            ensure!(
                switch.stage == Stage::Prepared,
                "model switch teardown repeated"
            );
            switch.stage = Stage::Teardown;
            Ok(())
        })
    }

    pub(crate) fn resolve_model_switch_candidate(
        &self,
        id: u64,
        actual_from: &str,
        candidate: &str,
    ) -> Result<()> {
        self.update(|record| {
            let (_, switch) = operation(record, id)?;
            ensure!(
                switch.stage == Stage::Prepared
                    && switch.from.adapter == "claude"
                    && !actual_from.is_empty()
                    && !candidate.is_empty()
                    && !switch.source_candidate
                    && !record.operations.iter().any(|operation| {
                        operation.non_tool_receipt().is_some_and(|receipt| {
                            receipt.facts.subject.occurrence.host_operation() == Some(id)
                        })
                    }),
                "Claude model switch candidate is stale or already observed"
            );
            let Some(HostInvocation::ModelSwitch(switch)) = &mut record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated")
                .host_invocation
            else {
                unreachable!()
            };
            switch.actual_from_model = Some(actual_from.into());
            switch.candidate_model = Some(candidate.into());
            switch.source_candidate = true;
            Ok(())
        })
    }

    pub(crate) fn apply_model_switch(
        &self,
        id: u64,
        connection: &Connection,
        checkpoint: Option<Value>,
        actual_model: Option<String>,
    ) -> Result<()> {
        self.update_without_observers(false, |record| {
            let (_, switch) = operation(record, id)?;
            ensure!(
                switch.stage == Stage::Teardown
                    && switch.requested == Identity::from(connection)
                    && actual_model.as_ref().is_none_or(|model| !model.is_empty()),
                "model switch application differs from its admitted candidate"
            );
            ensure!(
                switch.candidate_model == actual_model,
                "applied model differs from the observed candidate"
            );
            ensure!(
                record.prior_contexts.len() < 64,
                "model change history is full; start a new session"
            );
            let old_identity = record.identity.clone();
            record.prior_contexts.push(ContextBinding {
                identity: old_identity.clone(),
                through_operation: record.operations.len() as u64,
                checkpoint: record.checkpoint.clone(),
            });
            if let Some(task) = &mut record.task
                && task.creator_identity.is_none()
            {
                task.creator_identity = Some(old_identity);
            }
            record.identity = Identity::from(connection);
            record.checkpoint = checkpoint;
            record.checkpoint_cursor = record.operations.len() as u64;
            let Some(HostInvocation::ModelSwitch(switch)) = &mut record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated")
                .host_invocation
            else {
                unreachable!()
            };
            switch.actual_model = actual_model;
            switch.stage = Stage::Applied;
            Ok(())
        })
    }

    pub(crate) fn end_model_switch(&self, id: u64, hold: Option<String>) -> Result<()> {
        self.update(|record| {
            let _ = operation(record, id)?;
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated");
            let Some(HostInvocation::ModelSwitch(switch)) = &mut operation.host_invocation else {
                unreachable!()
            };
            switch.hold = hold;
            operation.complete = true;
            Ok(())
        })
    }

    /// Cancellation or an unexpected owner error never replays a transition.
    /// A fully local cancellation before any lifecycle child ran is a known
    /// refusal; teardown, application, or unfinished callback work is uncertain.
    pub(crate) fn interrupt_model_switch(&self, id: u64) -> Result<()> {
        self.update(|record| {
            let (_, switch) = operation(record, id)?;
            let uncertain = switch.stage != Stage::Prepared
                || record.operations.iter().any(|candidate| {
                    candidate.id != id
                        && candidate.phase == "model-switch"
                        && candidate.needs_reconciliation()
                });
            let operation = record
                .operations
                .iter_mut()
                .find(|operation| operation.id == id)
                .expect("validated");
            let Some(HostInvocation::ModelSwitch(switch)) = &mut operation.host_invocation else {
                unreachable!()
            };
            switch.hold = Some(
                if uncertain {
                    "model switch interrupted with uncertain lifecycle or provider effects"
                } else {
                    "model switch cancelled before provider teardown"
                }
                .into(),
            );
            operation.complete = true;
            record.recovery_pending |= uncertain;
            Ok(())
        })
    }
}

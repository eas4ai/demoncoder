//! Cross-operation eligibility is checked inside the same lock as hook reservation.
use super::{Record, SharedRuntime};
use crate::plugins::{
    once::{
        Activation, ActivationChange, ActivationSource, Consumption, FailedAttestation, HookOrigin,
        OnceAttempt, OnceBinding, OnceSkip, OnceState, UnresolvedOnce,
    },
    receipts::{HookReceipt, Scope},
};
use anyhow::{Context, Result, ensure};

const MAX_ACTIVATIONS: usize = 256;
impl SharedRuntime {
    /// Trusted loader boundary. Rebuilding a plan must use Reuse; only an explicit
    /// host-observed invocation authorizes a fresh epoch. Settings/agent once is ignored.
    /// This is not the public skill loader, which has not yet been integrated.
    pub fn plugin_hook_activation(
        &self,
        origin: HookOrigin,
        scope: Scope,
        source: &ActivationSource,
        component: &str,
        role: &str,
        change: ActivationChange,
    ) -> Result<Option<OnceBinding>> {
        if matches!(
            origin,
            HookOrigin::ClaudeSettings | HookOrigin::ClaudeAgent | HookOrigin::Codex
        ) {
            return Ok(None);
        }
        let package = source.0.package.as_str();
        ensure!(
            origin == HookOrigin::Native || source.0.packaged,
            "source skill activation requires a captured package"
        );
        for value in [package, component, role] {
            ensure!(
                !value.is_empty() && value.len() <= 256,
                "activation identity exceeds bounds"
            );
        }
        let mut activation = Activation {
            session: self.plugin_session()?,
            source: source.0.clone(),
            origin,
            scope,
            package: package.into(),
            component: component.into(),
            role: role.into(),
            epoch: 1,
        };
        self.update(|record| {
            super::delegation::ensure_agent_active(record, role)?;
            ensure!(!record.plugin_activations.iter().any(|a| a.source.identity == source.0.identity
                && a.source.packaged == source.0.packaged && a.package != source.0.package),
                "captured source changed its package name; restore its registered identity before activation");
            if change == ActivationChange::ExplicitInvocation {
                ensure!(
                    !hooks(record).any(|h| {
                        h.once.as_ref().is_some_and(|a| {
                            a.activation.same_component(&activation) && unresolved(a)
                        })
                    }),
                    "prior one-shot effects are unresolved; activation cannot bypass them"
                );
            }
            let position = record
                .plugin_activations
                .iter()
                .position(|a| a.same_owner(&activation));
            if let Some(position) = position {
                activation.epoch = record.plugin_activations[position].epoch;
                if change == ActivationChange::ExplicitInvocation {
                    activation.epoch = activation
                        .epoch
                        .checked_add(1)
                        .context("activation epoch exhausted")?;
                    record.plugin_activations[position] = activation.clone();
                }
            } else {
                ensure!(
                    change == ActivationChange::ExplicitInvocation,
                    "explicit activation is missing"
                );
                ensure!(
                    record.plugin_activations.len() < MAX_ACTIVATIONS,
                    "activation history limit reached"
                );
                record.plugin_activations.push(activation.clone());
            }
            Ok(Some(OnceBinding(activation.clone())))
        })
    }
}
fn hooks(record: &Record) -> impl Iterator<Item = &HookReceipt> {
    record
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref())
        .flat_map(|r| {
            r.plugin_admission
                .iter()
                .flat_map(|a| a.hooks.iter())
                .chain(r.plugin_lifecycle.iter().flat_map(|l| l.hooks.iter()))
        })
}
fn unresolved(attempt: &OnceAttempt) -> bool {
    matches!(attempt.state, OnceState::Reserved | OnceState::Unknown)
        && attempt.reconciliation.is_none()
}
fn canonical(a: &HookReceipt, b: &HookReceipt) -> bool {
    a.declaration.scope == b.declaration.scope
        && a.declaration.declaration == b.declaration.declaration
        && a.inspected.role == b.inspected.role
        && a.inspected.event == b.inspected.event
        && match (&a.source, &b.source) {
            (Some(a), Some(b)) => a.identity == b.identity && a.packaged == b.packaged,
            // Historical/unpackaged receipts cannot prove distinct source roots.
            _ => a.declaration.package == b.declaration.package,
        }
}
fn reference(hook: &HookReceipt) -> Consumption {
    Consumption {
        session: hook.inspected.session.clone(),
        operation: hook.inspected.operation,
        event: hook.inspected.event.clone(),
        invocation: hook.invocation,
    }
}
/// Pure validation: callers publish the returned reservation/skip only after all
/// fallible checks. Even failed update closures must leave the live record intact.
pub(super) fn reserve(
    record: &Record,
    hook: &mut HookReceipt,
    binding: Option<&OnceBinding>,
) -> Result<Option<OnceSkip>> {
    ensure!(
        hook.once.is_none() && hook.outcome.is_none(),
        "reservation supplied fabricated one-shot facts"
    );
    if let Some(binding) = binding {
        binding.validate(&hook.declaration)?;
        ensure!(
            hook.source.as_ref() == Some(&binding.0.source),
            "one-shot reservation has a mismatched canonical source"
        );
        ensure!(
            binding.0.session == hook.inspected.session
                && record.plugin_activations.contains(&binding.0),
            "one-shot activation is stale or belongs to another session"
        );
    }
    let duplicate_skip = record
        .operations
        .iter()
        .filter_map(|o| o.tool_receipt.as_ref())
        .flat_map(|r| {
            r.plugin_admission
                .iter()
                .flat_map(|a| &a.once_skips)
                .chain(r.plugin_lifecycle.iter().flat_map(|a| &a.once_skips))
        })
        .any(|skip| {
            skip.inspected.operation == hook.inspected.operation
                && skip.inspected.event == hook.inspected.event
                && skip.declaration == hook.declaration
                && binding.is_some_and(|b| b.0 == skip.activation)
        });
    ensure!(
        !duplicate_skip,
        "one-shot exemption already recorded for this event"
    );
    let mut consumed = None;
    for prior in hooks(record).filter(|h| canonical(h, hook)) {
        let Some(attempt) = &prior.once else {
            continue;
        };
        // Omitting once or changing code/generation cannot remove an unknown fence.
        // Distinct explicitly bound components retain independent identities.
        if binding.is_some_and(|b| !attempt.activation.same_component(&b.0)) {
            continue;
        }
        ensure!(
            !unresolved(attempt),
            "one-shot effects are unresolved; automatic replay is forbidden"
        );
        if let Some(binding) = binding
            && hook.endpoint.is_none()
        {
            ensure!(
                prior.inspected.operation != hook.inspected.operation,
                "one-shot retry requires the next matching event"
            );
            if attempt.activation == binding.0 && attempt.state == OnceState::Succeeded {
                consumed = Some(reference(prior));
            }
        }
    }
    if let Some(binding) = binding.filter(|_| hook.endpoint.is_none()) {
        if let Some(consumed) = consumed {
            return Ok(Some(OnceSkip {
                declaration: hook.declaration.clone(),
                inspected: hook.inspected.clone(),
                consumed,
                activation: binding.0.clone(),
            }));
        }
        hook.once = Some(OnceAttempt {
            activation: binding.0.clone(),
            state: OnceState::Reserved,
            reconciliation: None,
        });
    }
    Ok(None)
}
pub(super) fn settle_pre(hook: &mut HookReceipt) -> Result<()> {
    if hook.once.is_none() {
        return Ok(());
    }
    let profile = crate::plugins::profile::CompatibilityProfile::embedded()?;
    let decoded = hook
        .outcome
        .as_ref()
        .context("one-shot outcome missing")?
        .decode(&profile, &hook.declaration);
    let state = if hook.uncertain_effects {
        OnceState::Unknown
    } else if crate::plugins::once::succeeded(hook, &decoded) {
        OnceState::Succeeded
    } else {
        OnceState::Failed
    };
    hook.once.as_mut().expect("checked").state = state;
    Ok(())
}
pub(super) fn settle_post_raw(hook: &mut HookReceipt) {
    if let Some(attempt) = &mut hook.once {
        // Wait for proposal-owner settlement before recording consumption.
        attempt.state = OnceState::Unknown;
    }
}
pub(super) fn validate_skips(record: &Record, skips: &[OnceSkip]) -> Result<()> {
    for skip in skips {
        let prior = hooks(record)
            .find(|h| reference(h) == skip.consumed)
            .context("consumed evidence missing")?;
        ensure!(
            prior
                .once
                .as_ref()
                .is_some_and(|a| a.activation == skip.activation && a.state == OnceState::Succeeded)
                && prior.inspected.session == skip.inspected.session
                && prior.inspected.operation != skip.inspected.operation
                && prior.inspected.event == skip.inspected.event
                && prior.declaration.package == skip.declaration.package
                && prior.declaration.scope == skip.declaration.scope
                && prior.declaration.declaration == skip.declaration.declaration
                && prior.declaration.role == skip.declaration.role,
            "one-shot exemption lacks exact prior successful evidence"
        );
    }
    Ok(())
}

pub(super) fn settle_post(
    lifecycle: &mut crate::plugins::receipts::LifecycleReceipt,
    successful: &[u32],
) {
    for hook in &mut lifecycle.hooks {
        if hook.once.is_none() {
            continue;
        }
        let applied = lifecycle
            .proposals
            .iter()
            .filter(|p| p.invocation == hook.invocation)
            .all(|p| {
                matches!(
                    p.disposition,
                    crate::plugins::receipts::ProposalDisposition::Applied
                )
            });
        let state = if hook.uncertain_effects {
            OnceState::Unknown
        } else if successful.contains(&hook.invocation) && applied {
            OnceState::Succeeded
        } else {
            OnceState::Failed
        };
        hook.once.as_mut().expect("checked").state = state;
    }
}

impl SharedRuntime {
    /// Return up to 256 oldest unresolved targets. Repeating inspection after
    /// resolving a page exposes the next page; no retained identity is evicted.
    /// These read-only targets carry no execution or success authority.
    pub fn unresolved_plugin_once(&self) -> Result<Vec<UnresolvedOnce>> {
        let record = self.record()?;
        let mut targets = Vec::new();
        for hook in hooks(&record) {
            if let Some(attempt) = &hook.once
                && unresolved(attempt)
            {
                if targets.len() == 256 {
                    break;
                }
                targets.push(UnresolvedOnce {
                    reference: reference(hook),
                    declaration: hook.declaration.clone(),
                    activation: attempt.activation.clone(),
                    evidence_digest: crate::plugins::admission::digest(hook)?,
                });
            }
        }
        Ok(targets)
    }
    /// Trusted developer-control boundary, never a hook/model tool. The actor
    /// attests that this exact runner has stopped and its attempt was unsuccessful.
    /// Original observed evidence remains unchanged; only a later event may retry.
    pub fn reconcile_failed_plugin_once(
        &self,
        target: &UnresolvedOnce,
        actor: &str,
        reason: &str,
    ) -> Result<()> {
        for value in [actor, reason] {
            ensure!(
                !value.trim().is_empty() && value.len() <= 4096,
                "one-shot reconciliation requires a bounded actor and reason"
            );
        }
        let session = self.plugin_session()?;
        let tracker = self.once_live()?;
        self.update(|record| {
            let owners = tracker
                .lock()
                .map_err(|_| anyhow::anyhow!("one-shot owner lock failed"))?;
            ensure!(
                owners
                    .runners
                    .get(&target.reference)
                    .is_none_or(|owner| owner.strong_count() == 0)
                    && owners
                        .post_lifecycles
                        .get(&(target.reference.operation, target.reference.event.clone()))
                        .is_none_or(|owner| owner.strong_count() == 0),
                "one-shot runner, cleanup, or lifecycle settlement is still live; stop and join it before reconciliation"
            );
            drop(owners);
            ensure!(
                target.reference.session == session && target.activation.session == session,
                "one-shot reconciliation belongs to another session"
            );
            let previous = hooks(record)
                .find(|h| reference(h) == target.reference)
                .context("one-shot attempt missing")?;
            ensure!(
                previous.declaration == target.declaration
                    && previous
                        .once
                        .as_ref()
                        .is_some_and(|a| a.activation == target.activation && unresolved(a))
                    && crate::plugins::admission::digest(previous)? == target.evidence_digest,
                "one-shot reconciliation is stale, mismatched, or already settled"
            );
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == target.reference.operation)
                .expect("validated");
            let tool = operation.tool_receipt.as_mut().expect("validated");
            let hooks = if target.reference.event == "PreToolUse" {
                &mut tool.plugin_admission.as_mut().expect("validated").hooks
            } else {
                &mut tool.plugin_lifecycle.as_mut().expect("validated").hooks
            };
            hooks[target.reference.invocation as usize]
                .once
                .as_mut()
                .expect("validated")
                .reconciliation = Some(FailedAttestation {
                actor: actor.into(),
                reason: reason.into(),
                evidence_digest: target.evidence_digest.clone(),
            });
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;

/// A weak reference to the existing execution lease remains live through command
/// teardown, including detached blocking cleanup after async cancellation.
pub(super) type LiveHooks = std::sync::Arc<std::sync::Mutex<LiveOwners>>;
#[derive(Default)]
pub(super) struct LiveOwners {
    runners:
        std::collections::BTreeMap<Consumption, std::sync::Weak<tokio::sync::OwnedSemaphorePermit>>,
    post_lifecycles: std::collections::BTreeMap<(u64, String), std::sync::Weak<()>>,
}
impl SharedRuntime {
    /// Keep failure attestation out of an active proposal owner's transaction.
    /// This token spans groups and settlement without retaining runner capacity.
    pub(crate) fn own_post_lifecycle(
        &self,
        operation: u64,
        event: crate::plugins::hook_types::HookEvent,
    ) -> Result<std::sync::Arc<()>> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        super::plugin_lifecycle::active(&runtime.record, operation, event)?;
        let mut owners = runtime
            .once_live
            .lock()
            .map_err(|_| anyhow::anyhow!("one-shot owner lock failed"))?;
        owners
            .post_lifecycles
            .retain(|_, owner| owner.strong_count() > 0);
        ensure!(
            owners.post_lifecycles.len() < 4096,
            "live post lifecycle owner limit reached"
        );
        let key = (operation, event.as_str().to_owned());
        ensure!(
            !owners.post_lifecycles.contains_key(&key),
            "post lifecycle already has an active owner"
        );
        let owner = std::sync::Arc::new(());
        owners
            .post_lifecycles
            .insert(key, std::sync::Arc::downgrade(&owner));
        Ok(owner)
    }

    pub(super) fn once_live(&self) -> Result<LiveHooks> {
        Ok(self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime lock failed"))?
            .once_live
            .clone())
    }
}
pub(super) fn track_live(
    tracker: &LiveHooks,
    hook: &HookReceipt,
    lease: Option<&std::sync::Arc<tokio::sync::OwnedSemaphorePermit>>,
) -> Result<()> {
    if hook.once.is_none() {
        return Ok(());
    }
    ensure!(
        lease.is_some() || cfg!(test),
        "one-shot execution lease missing"
    );
    let Some(lease) = lease else {
        return Ok(());
    };
    let mut live = tracker
        .lock()
        .map_err(|_| anyhow::anyhow!("one-shot owner lock failed"))?;
    live.runners.retain(|_, lease| lease.strong_count() > 0);
    ensure!(
        live.runners.len() < 4096,
        "live one-shot owner limit reached"
    );
    ensure!(
        !live.runners.contains_key(&reference(hook)),
        "one-shot invocation already owns a lease"
    );
    live.runners
        .insert(reference(hook), std::sync::Arc::downgrade(lease));
    Ok(())
}

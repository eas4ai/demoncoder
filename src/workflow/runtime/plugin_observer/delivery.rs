//! Durable delivery reservations refer to original outcomes; uncertain sends never retry.
use super::*;
pub(crate) struct ContextDelivery {
    references: Vec<Key>,
    pub(crate) text: String,
}
fn eligible(record: &Record, hook: &HookReceipt, phase: &str, rewake: bool) -> bool {
    hook.inspected.role == phase
        && hook.observer.as_ref().is_some_and(|o| {
            o.status == Status::Completed
                && o.delivery == Delivery::Pending
                && o.rewake == rewake
                && fingerprint(record, phase).is_ok_and(|f| f == o.owner)
        })
}
fn text(hook: &HookReceipt) -> Result<String> {
    let outcome = hook
        .outcome
        .as_ref()
        .context("observer completion missing")?;
    let observer = hook
        .observer
        .as_ref()
        .context("observer transfer missing")?;
    let mut text = format!(
        "\n[Plugin-origin {} {} operation {} role {}]\n",
        hook.declaration.package,
        hook.inspected.event,
        hook.inspected.operation,
        hook.inspected.role
    );
    if observer.rewake {
        let RawOutcome::Command {
            exit_code: Some(2),
            stdout,
            stderr,
        } = outcome
        else {
            anyhow::bail!("rewake lacks source exit 2");
        };
        text.push_str(std::str::from_utf8(if stderr.is_empty() {
            stdout
        } else {
            stderr
        })?);
    } else {
        let event = HookEvent::try_from(hook.inspected.event.as_str())?;
        let decoded = outcome.decode_for(
            &crate::plugins::profile::CompatibilityProfile::embedded()?,
            &hook.declaration,
            event,
            &crate::plugins::results::ResultContext {
                role: crate::plugins::results::ResultRole::Observer,
                asynchronous: true,
                ..Default::default()
            },
        );
        ensure!(context_valid(&decoded), "invalid observer context");
        for proposal in &hook.pending_proposals {
            match decoded.effects.get(proposal.index) {
                Some(
                    crate::plugins::results::ProposedEffect::AdditionalContext(value)
                    | crate::plugins::results::ProposedEffect::Warning(value)
                    | crate::plugins::results::ProposedEffect::TransientNotice(value),
                ) => {
                    text.push_str(value.get());
                    text.push('\n');
                }
                _ => anyhow::bail!("observer context reference mismatch"),
            }
        }
    }
    ensure!(
        text.len() <= 65536,
        "observer context exceeds delivery bound"
    );
    Ok(text)
}
impl SharedRuntime {
    pub(crate) fn has_observer_rewake(&self, phase: &str) -> Result<bool> {
        let record = self.record()?;
        let available = (delegation::agent_id(phase).is_some()
            || record.phase.as_deref().is_none_or(|active| active == phase))
            && record.allocation.as_ref().is_some_and(|allocation| {
                allocation.model_calls < allocation.limits.model_calls
                    && allocation.tool_calls < allocation.limits.tool_calls
            });
        Ok(available
            && hooks(&record).any(|h| {
                eligible(&record, h, phase, true)
                    && text(h).is_ok()
                    && super::super::plugin_lifecycle::owner::observer_available(&record, h)
            }))
    }
    pub(crate) fn reserve_observer_context(
        &self,
        phase: &str,
        identity: Option<&super::super::Identity>,
        rewake: bool,
    ) -> Result<Option<ContextDelivery>> {
        self.update(|target| {
            let mut record = target.clone();
            // A different backend or worker may not borrow this delivery.
            let expected_identity = match delegation::agent_id(phase) {
                Some(id) => record
                    .agents
                    .iter()
                    .find(|agent| agent.id == id)
                    .map(|agent| &agent.identity),
                None => Some(&record.identity),
            };
            if identity.is_some_and(|identity| expected_identity != Some(identity)) {
                return Ok(None);
            }
            let mut selected = Vec::new();
            let mut combined = String::new();
            let mut invalid = Vec::new();
            for hook in hooks(&record).filter(|h| eligible(&record, h, phase, rewake)) {
                if !rewake && hook.pending_proposals.is_empty() {
                    continue;
                }
                if rewake
                    && !super::super::plugin_lifecycle::owner::observer_available(&record, hook)
                {
                    continue;
                }
                let content = match text(hook) {
                    Ok(content) => content,
                    Err(_) => {
                        invalid.push(key(hook));
                        continue;
                    }
                };
                if combined.len().saturating_add(content.len()) > 65536 {
                    break;
                }
                combined.push_str(&content);
                selected.push(hook.clone());
                if rewake {
                    break;
                }
            }
            for hook in hooks_mut(&mut record).filter(|hook| invalid.contains(&key(hook))) {
                hook.observer
                    .as_mut()
                    .expect("selected invalid delivery")
                    .delivery = Delivery::Withheld;
            }
            if selected.is_empty() {
                *target = record;
                return Ok(None);
            }
            if rewake {
                let allocation = record
                    .allocation
                    .as_ref()
                    .context("rewake allowance missing")?;
                ensure!(
                    allocation.model_calls < allocation.limits.model_calls
                        && allocation.tool_calls < allocation.limits.tool_calls,
                    "rewake allowance exhausted"
                );
                super::super::plugin_lifecycle::owner::charge_observer(&mut record, &selected[0])?;
                if delegation::agent_id(phase).is_none() {
                    ensure!(
                        record.phase.as_deref().is_none_or(|p| p == phase),
                        "rewake cannot enter another owner phase"
                    );
                    record.phase = Some(phase.into());
                }
            }
            let references = selected.iter().map(key).collect::<Vec<_>>();
            for hook in hooks_mut(&mut record).filter(|h| references.contains(&key(h))) {
                hook.observer.as_mut().expect("selected").delivery = Delivery::Reserved;
            }
            *target = record;
            Ok(Some(ContextDelivery {
                references,
                text: combined,
            }))
        })
    }
    pub(crate) fn complete_observer_context(&self, delivery: &ContextDelivery) -> Result<()> {
        self.update(|record| {
            for reference in &delivery.references {
                ensure!(
                    exact(record, reference)?
                        .observer
                        .as_ref()
                        .is_some_and(|o| o.delivery == Delivery::Reserved),
                    "observer delivery not reserved"
                );
            }
            for hook in hooks_mut(record).filter(|h| delivery.references.contains(&key(h))) {
                hook.observer.as_mut().expect("validated").delivery = Delivery::Delivered;
            }
            Ok(())
        })
    }
}

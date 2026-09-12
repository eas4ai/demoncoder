//! An exact reserved transport call, distinct from its reusable service lifetime.
use super::*;
use crate::plugins::{
    dispatch::HookInvocation,
    gate_snapshot::{GateSnapshot, GateWorkspace},
};
use std::{sync::atomic::AtomicBool, time::Duration};

/// Retained service view authority survives the invocation that initialized it.
#[derive(Clone)]
pub(crate) struct TransportView {
    workspace: Arc<GateWorkspace>,
    snapshot: Arc<GateSnapshot>,
}
impl TransportView {
    pub(crate) fn validate(&self, cancelled: &AtomicBool) -> Result<()> {
        ensure!(
            self.workspace.is_current(&self.snapshot, cancelled)?,
            "transport retained view or credential authority changed"
        );
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct TransportInvocation {
    runtime: super::super::RuntimeReference,
    key: AdmissionKey,
    declaration: DeclarationIdentity,
    invocation: u32,
    event: HookEvent,
    budget: super::super::BudgetRef,
    endpoint: Option<String>,
    class: HandlerClass,
    required_gate: bool,
    view: TransportView,
}
impl TransportInvocation {
    pub(crate) fn capture(invocation: &HookInvocation) -> Result<Self> {
        let (runtime, operation) = invocation.events.plugin_context()?;
        ensure!(
            operation == invocation.key.operation,
            "transport operation differs from its reserved owner"
        );
        let owner = Self {
            runtime: runtime.downgrade(),
            key: invocation.key.clone(),
            declaration: invocation.declaration.clone(),
            invocation: invocation.invocation,
            event: invocation.events.plugin_event(),
            budget: runtime.operation_budget(operation)?,
            endpoint: invocation.endpoint.clone(),
            class: invocation.class,
            required_gate: invocation.required_gate,
            view: TransportView {
                workspace: Arc::new(GateWorkspace::open_with_credentials(
                    &invocation.host.workspace,
                    &invocation.host.credentials,
                )?),
                snapshot: invocation.snapshot.clone(),
            },
        };
        owner.validate_owner()?;
        Ok(owner)
    }
    pub(crate) fn validate_owner(&self) -> Result<Duration> {
        let runtime = self.runtime.upgrade()?;
        let remaining = runtime.plugin_funded_remaining(self.key.operation, self.event)?;
        let record = runtime.record()?;
        let hooks = active_for_event(&record, self.key.operation, self.event)?;
        let reserved = hooks.invocation(self.invocation)?;
        ensure!(
            self.key.session == runtime.plugin_session()?
                && reserved.inspected == self.key
                && reserved.declaration == self.declaration
                && reserved.endpoint == self.endpoint
                && reserved.class == self.class
                && reserved.required_gate == self.required_gate
                && reserved.outcome.is_none()
                && super::super::budget_accounting::inherited(&record, self.key.operation)?
                    == self.budget,
            "transport invocation is settled or differs from its original reservation"
        );
        ensure!(!remaining.is_zero(), "transport invocation owner expired");
        Ok(remaining)
    }
    pub(crate) fn validate_view(&self, cancelled: &AtomicBool) -> Result<Duration> {
        self.validate_owner()?;
        self.view.validate(cancelled)?;
        self.validate_owner()
    }
    pub(crate) fn retained_view(&self) -> TransportView {
        self.view.clone()
    }
    pub(crate) fn validate_service(&self, owner: &ServiceOwner) -> Result<Duration> {
        self.validate_owner()?;
        self.runtime.upgrade()?.validate_plugin_service_invocation(
            owner,
            self.key.operation,
            self.event,
        )
    }
}

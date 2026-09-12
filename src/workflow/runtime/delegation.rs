//! Child facts share the parent record and admission lock.
use anyhow::{Context, Result, ensure};
use serde_json::Value;

use super::{Operation, Record, SharedRuntime};
use crate::{
    subagents::state::{AgentRecord, DelegationIdentity},
    workflow::allocation::{Allocation, Limits},
};

pub(super) fn agent_id(phase: &str) -> Option<u64> {
    phase
        .strip_prefix("agent:")?
        .split(':')
        .next()?
        .parse()
        .ok()
}

pub(super) fn ensure_agent_active(record: &Record, phase: &str) -> Result<()> {
    if let Some(id) = agent_id(phase) {
        let agent = record
            .agents
            .iter()
            .find(|agent| agent.id == id)
            .context("agent admission has no assignment")?;
        ensure!(
            matches!(
                agent.status,
                crate::subagents::state::AgentStatus::Running
                    | crate::subagents::state::AgentStatus::Validating
            ),
            "agent is not admitted for execution"
        );
        ensure!(
            agent.parent_task == record.task.as_ref().map(|task| task.id),
            "agent belongs to a different parent task allocation"
        );
    }
    Ok(())
}

impl SharedRuntime {
    pub(crate) fn ensure_backend_available(&self) -> Result<()> {
        let record = self.record()?;
        let limit = record
            .delegation
            .as_ref()
            .context("backend admission requires a delegation allocation")?
            .backend_limit;
        ensure!(
            record.backend_invocations < limit,
            "cumulative backend invocation allowance exhausted"
        );
        Ok(())
    }

    pub(crate) fn configure_delegation(
        &self,
        identity: DelegationIdentity,
        limits: Limits,
    ) -> Result<()> {
        ensure!(
            (1..=8).contains(&identity.max_active),
            "active agent limit must be between 1 and 8"
        );
        ensure!(
            (1..=4096).contains(&identity.backend_limit),
            "backend invocation limit must be between 1 and 4096"
        );
        if let Some(orchestration) = &identity.orchestration {
            ensure!(
                orchestration.correction_limit == 2,
                "orchestration correction limit must be exactly two"
            );
            ensure!(
                !orchestration.checks.is_empty() && orchestration.checks.len() <= 16,
                "orchestration requires 1 to 16 selected checks"
            );
            ensure!(
                orchestration
                    .checks
                    .iter()
                    .all(|check| !check.trim().is_empty() && check.len() <= 8192),
                "each orchestration check must contain 1 to 8192 bytes"
            );
        }
        let session = self.plugin_session()?;
        self.update(|target| {
            let mut staged = target.clone();
            let record = &mut staged;
            if let Some(saved) = &record.delegation {
                ensure!(
                    record.allocation.is_some(),
                    "delegation allocation is missing; cannot restore spent allowances"
                );
                let mut comparison = identity.clone();
                // Enabling a role remains a launch decision. Only its model default
                // may change; each retained child restores its own captured identity.
                if saved.default_roles == comparison.default_roles {
                    for role in &saved.default_roles {
                        match role.as_str() {
                            "worker" => if let Some(original) = saved.connections.get("default") {
                                comparison.connections.insert("default".into(), original.clone());
                            },
                            "reviewer" => comparison.reviewer = saved.reviewer.clone(),
                            "judge" => if let (Some(old), Some(new)) = (&saved.orchestration, &mut comparison.orchestration) { new.judge = old.judge.clone(); },
                            _ => anyhow::bail!("unknown Settings default in recovery state"),
                        }
                    }
                }
                ensure!(
                    saved == &comparison,
                    "resume requires the original agent connections, reviewer, judge, orchestration settings and limits"
                );
            } else {
                record.delegation = Some(identity);
            }
            if record.allocation.is_none() {
                super::budget_accounting::replace(record, &session, Allocation::new(limits)?)?;
            }
            *target = staged;
            Ok(())
        })
    }

    #[cfg(test)]
    pub(crate) fn begin_backend(&self, phase: &str) -> Result<u64> {
        self.begin_backend_as(phase, None)
    }

    #[cfg(test)]
    pub(crate) fn begin_backend_as(
        &self,
        phase: &str,
        identity: Option<&super::Identity>,
    ) -> Result<u64> {
        self.begin_backend_owned(phase, identity, None)
    }

    #[cfg(test)]
    pub(crate) fn begin_backend_owned(
        &self,
        phase: &str,
        identity: Option<&super::Identity>,
        hook: Option<&super::plugin_admission::ModelAdmission>,
    ) -> Result<u64> {
        self.begin_backend_from(phase, identity, hook, None)
    }

    pub(crate) fn begin_backend_from(
        &self,
        phase: &str,
        identity: Option<&super::Identity>,
        hook: Option<&super::plugin_admission::ModelAdmission>,
        oracle: Option<&super::OracleSource>,
    ) -> Result<u64> {
        let session = self.plugin_session()?;
        let id = self.update(|record| {
            if let Some(source) = oracle {
                let budget = source.validate(record, &session, phase)?;
                ensure!(
                    hook.is_none_or(|h| h.budget == budget),
                    "Oracle hook budget changed"
                );
            }
            begin_backend_record(
                record,
                &session,
                phase,
                identity,
                hook,
                oracle.map(|s| s.operation()),
            )
        })?;
        self.validate_operation_deadline(id)?;
        if let Some(hook) = hook {
            self.validate_hook_model_owner(hook)?;
        }
        Ok(id)
    }

    pub(crate) fn update_agent<T>(
        &self,
        id: u64,
        update: impl FnOnce(&mut AgentRecord) -> Result<T>,
    ) -> Result<T> {
        self.update(|record| {
            let agent = record
                .agents
                .iter_mut()
                .find(|agent| agent.id == id)
                .context("agent assignment does not exist")?;
            update(agent)
        })
    }

    pub(crate) fn admit_agent_validation(&self, id: u64, max_active: u32) -> Result<()> {
        self.update(|record| {
            ensure!(
                !record.recovery_pending,
                "reconcile interrupted parent work before validation"
            );
            ensure!(
                record
                    .agents
                    .iter()
                    .filter(|agent| agent.status.active())
                    .count()
                    < max_active as usize,
                "active agent limit reached"
            );
            let agent = record
                .agents
                .iter_mut()
                .find(|agent| agent.id == id)
                .context("agent assignment does not exist")?;
            ensure!(
                agent.completed
                    && matches!(
                        agent.status,
                        crate::subagents::state::AgentStatus::Stopped
                            | crate::subagents::state::AgentStatus::Ready
                            | crate::subagents::state::AgentStatus::Failed
                    ),
                "validation requires completed child work"
            );
            ensure!(
                !agent.commands.is_empty() && agent.reviewer.is_some(),
                "select --check and --reviewer before assigning work"
            );
            agent.retain_validation()?;
            agent.status = crate::subagents::state::AgentStatus::Validating;
            if agent.orchestration.is_none() {
                agent.validation_generation += 1;
            }
            agent.validation_snapshot = None;
            Ok(())
        })
    }

    pub(crate) fn agent_checkpoint(&self, phase: &str, checkpoint: Value) -> Result<()> {
        let Some(id) = agent_id(phase) else {
            return Ok(());
        };
        // Review/Oracle calls must never replace the worker conversation.
        if phase != format!("agent:{id}:worker") {
            return Ok(());
        }
        self.update(|record| {
            let cursor = record.operations.len() as u64;
            let agent = record
                .agents
                .iter_mut()
                .find(|agent| agent.id == id)
                .context("agent checkpoint has no assignment")?;
            agent.checkpoint = Some(checkpoint);
            agent.checkpoint_cursor = cursor;
            Ok(())
        })
    }
}

/// The caller owns the admission transaction. Corrections reuse the same
/// accounting while atomically reserving their one exact superseded receipt.
pub(super) fn begin_backend_record(
    record: &mut Record,
    session: &str,
    phase: &str,
    identity: Option<&super::Identity>,
    hook: Option<&super::plugin_admission::ModelAdmission>,
    source: Option<u64>,
) -> Result<u64> {
    ensure!(
        !record.recovery_pending,
        "uncertain work needs reconciliation before backend admission"
    );
    ensure_agent_active(record, phase)?;
    ensure!(
        record.operations.len() < 4096,
        "session operation history is full"
    );
    let budget = match (hook, source) {
        (Some(hook), _) => hook.budget.clone(),
        (_, Some(source)) => super::budget_accounting::inherited(record, source)?,
        _ => super::budget_accounting::capture(record, session),
    };
    super::budget_accounting::active(record, session, &budget)?;
    let session_hook = matches!(budget, super::BudgetRef::SessionHooks { .. });
    let next_backend = if session_hook {
        ensure!(
            hook.is_some(),
            "session backend requires live model hook authority"
        );
        record
            .session_hook_allowance
            .as_ref()
            .context("session hook allowance missing")?
            .backend_invocations
            .checked_add(1)
            .context("session backend invocation count overflow")?
    } else {
        if let Some(delegation) = &record.delegation {
            ensure!(
                record.backend_invocations < delegation.backend_limit,
                "cumulative backend invocation allowance exhausted"
            );
        }
        record
            .backend_invocations
            .checked_add(u64::from(record.delegation.is_some() || hook.is_some()))
            .context("backend invocation count overflow")?
    };
    if let Some(hook) = hook {
        ensure!(
            hook.key.session == session,
            "model hook belongs to another session"
        );
        super::plugin_admission::validate_model_admission(record, phase, hook)?;
        super::budget_accounting::admit(record, session, &budget, true)?;
    }
    if session_hook {
        record
            .session_hook_allowance
            .as_mut()
            .expect("validated session allowance")
            .backend_invocations = next_backend;
    } else if record.delegation.is_some() || hook.is_some() {
        record.backend_invocations = next_backend;
    }
    let id = record.operations.len() as u64 + 1;
    record.operations.push(Operation {
        id,
        phase: phase.into(),
        verification: None,
        call: None,
        result: None,
        tool_receipt: None,
        budget: Some(budget),
        usage_receipt: None,
        host_invocation: Some(super::HostInvocation::Backend),
        complete: false,
        reconciled: false,
        usage_reported: false,
        identity: identity.cloned(),
    });
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Connection,
        subagents::state::{AgentRecord, AgentStatus, AssignmentOrigin, AssignmentRequest},
        workflow::{
            runtime::{Identity, Runtime},
            store::Store,
        },
    };
    use std::sync::{Arc, Mutex};

    fn agent(id: u64, identity: &Identity, status: AgentStatus) -> AgentRecord {
        AgentRecord {
            id,
            parent_task: Some(1),
            origin: AssignmentOrigin::Developer,
            completed: status == AgentStatus::Stopped,
            request: AssignmentRequest {
                connection: "worker".into(),
                objective: "test assignment".into(),
                context: String::new(),
                owned_paths: vec!["greeting".into()],
            },
            identity: identity.clone(),
            worktree: None,
            planned_root: None,
            status,
            outcome: String::new(),
            commands: vec!["true".into()],
            reviewer: Some(identity.clone()),
            checks: Vec::new(),
            review: None,
            validation_generation: 0,
            validation_snapshot: None,
            activity: Vec::new(),
            checkpoint: None,
            checkpoint_cursor: 0,
            integration: None,
            decisions: Vec::new(),
            orchestration: None,
        }
    }

    #[test]
    fn backend_invocations_have_a_durable_separate_limit_and_do_not_reset() {
        let root = tempfile::tempdir().unwrap();
        let connection: Connection =
            serde_json::from_value(serde_json::json!({"adapter":"openai-api"})).unwrap();
        let record: Record = serde_json::from_value(serde_json::json!({
            "workspace":root.path(), "identity":Identity::from(&connection), "archived":[],
            "next_task":1, "checkpoint_cursor":0, "operations":[], "messages":[],
            "recovery_pending":false, "decisions":[]
        }))
        .unwrap();
        let runtime = SharedRuntime(Arc::new(Mutex::new(Runtime {
            store: Store::create(&root.path().join("record")).unwrap(),
            record,
            failed: false,
            learning_view: None,
            mutation_boundaries: Default::default(),
            service_slots: Arc::new(tokio::sync::Semaphore::new(8)),
            once_live: Default::default(),
            observers: Default::default(),
        })));
        let mut judge = connection.clone();
        judge.model = Some("judge-a".into());
        judge.access = crate::tools::AccessPolicy::review_only();
        let identity = DelegationIdentity {
            default_roles: Vec::new(),
            connections: Default::default(),
            reviewer: None,
            max_active: 2,
            backend_limit: 1,
            orchestration: Some(crate::subagents::state::OrchestrationIdentity {
                judge: Identity::from(&judge),
                correction_limit: 2,
                checks: vec!["test -s greeting".into()],
            }),
        };
        runtime
            .configure_delegation(identity.clone(), Limits::default())
            .unwrap();
        let admission = runtime.begin_backend("reviewer").unwrap();
        runtime.finish_model(admission).unwrap();
        runtime
            .configure_delegation(identity.clone(), Limits::default())
            .unwrap();
        assert!(runtime.begin_backend("reviewer").is_err());
        judge.model = Some("judge-b".into());
        let mut changed = identity.clone();
        changed.orchestration.as_mut().unwrap().judge = Identity::from(&judge);
        let error = runtime
            .configure_delegation(changed, Limits::default())
            .unwrap_err();
        assert!(error.to_string().contains("judge"));
        let record = runtime.record().unwrap();
        assert_eq!(record.backend_invocations, 1);
        assert_eq!(record.allocation.unwrap().model_calls, 0);
        assert_eq!(record.operations.len(), 1);
        drop(runtime);
        assert_eq!(
            Store::open(&root.path().join("record"))
                .unwrap()
                .read()
                .unwrap()["backend_invocations"],
            1
        );
    }

    #[test]
    fn orchestration_identity_rejects_a_nonfixed_correction_limit() {
        let root = tempfile::tempdir().unwrap();
        let connection: Connection =
            serde_json::from_value(serde_json::json!({"adapter":"openai-api"})).unwrap();
        let record: Record = serde_json::from_value(serde_json::json!({
            "workspace":root.path(), "identity":Identity::from(&connection), "archived":[],
            "next_task":1, "checkpoint_cursor":0, "operations":[], "messages":[],
            "recovery_pending":false, "decisions":[]
        }))
        .unwrap();
        let runtime = SharedRuntime(Arc::new(Mutex::new(Runtime {
            store: Store::create(&root.path().join("record")).unwrap(),
            record,
            failed: false,
            learning_view: None,
            mutation_boundaries: Default::default(),
            service_slots: Arc::new(tokio::sync::Semaphore::new(8)),
            once_live: Default::default(),
            observers: Default::default(),
        })));
        let identity = DelegationIdentity {
            default_roles: Vec::new(),
            connections: Default::default(),
            reviewer: None,
            max_active: 2,
            backend_limit: 1,
            orchestration: Some(crate::subagents::state::OrchestrationIdentity {
                judge: Identity::from(&connection),
                correction_limit: 3,
                checks: vec!["cargo test".into()],
            }),
        };
        let error = runtime
            .configure_delegation(identity, Limits::default())
            .unwrap_err();
        assert!(error.to_string().contains("exactly two"));
    }

    #[test]
    fn validation_admission_rechecks_capacity_in_the_durable_transition() {
        let root = tempfile::tempdir().unwrap();
        let connection: Connection =
            serde_json::from_value(serde_json::json!({"adapter":"openai-api"})).unwrap();
        let identity = Identity::from(&connection);
        let mut record: Record = serde_json::from_value(serde_json::json!({
            "workspace":root.path(), "identity":identity, "archived":[],
            "next_task":1, "checkpoint_cursor":0, "operations":[], "messages":[],
            "recovery_pending":false, "decisions":[]
        }))
        .unwrap();
        record.agents = vec![
            agent(1, &identity, AgentStatus::Stopped),
            agent(2, &identity, AgentStatus::Running),
        ];
        let runtime = SharedRuntime(Arc::new(Mutex::new(Runtime {
            store: Store::create(&root.path().join("record")).unwrap(),
            record,
            failed: false,
            learning_view: None,
            mutation_boundaries: Default::default(),
            service_slots: Arc::new(tokio::sync::Semaphore::new(8)),
            once_live: Default::default(),
            observers: Default::default(),
        })));

        let error = runtime.admit_agent_validation(1, 1).unwrap_err();

        assert!(error.to_string().contains("active agent limit"));
        let retained = runtime.record().unwrap();
        assert_eq!(retained.agents[0].status, AgentStatus::Stopped);
        assert_eq!(retained.agents[0].validation_generation, 0);
    }
}

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
        self.update(|record| {
            if let Some(saved) = &record.delegation {
                ensure!(
                    record.allocation.is_some(),
                    "delegation allocation is missing; cannot restore spent allowances"
                );
                ensure!(
                    saved == &identity,
                    "resume requires the original agent connections, reviewer and limits"
                );
            } else {
                record.delegation = Some(identity);
            }
            if record.allocation.is_none() {
                record.allocation = Some(Allocation::new(limits)?);
            }
            Ok(())
        })
    }

    pub(crate) fn begin_backend(&self, phase: &str) -> Result<u64> {
        self.admission(|record| {
            ensure!(
                !record.recovery_pending,
                "uncertain work needs reconciliation before backend admission"
            );
            ensure_agent_active(record, phase)?;
            let limit = record
                .delegation
                .as_ref()
                .context("backend admission requires a delegation allocation")?
                .backend_limit;
            ensure!(
                record.backend_invocations < limit,
                "cumulative backend invocation allowance exhausted"
            );
            ensure!(
                record.operations.len() < 4096,
                "session operation history is full"
            );
            record.backend_invocations += 1;
            let id = record.operations.len() as u64 + 1;
            record.operations.push(Operation {
                id,
                phase: phase.into(),
                verification: None,
                call: None,
                result: None,
                complete: false,
                reconciled: false,
                usage_reported: false,
            });
            Ok(id)
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Connection,
        workflow::{
            runtime::{Identity, Runtime},
            store::Store,
        },
    };
    use std::sync::{Arc, Mutex};

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
        })));
        let identity = DelegationIdentity {
            connections: Default::default(),
            reviewer: None,
            max_active: 2,
            backend_limit: 1,
        };
        runtime
            .configure_delegation(identity.clone(), Limits::default())
            .unwrap();
        let admission = runtime.begin_backend("reviewer").unwrap();
        runtime.finish_model(admission).unwrap();
        runtime
            .configure_delegation(identity, Limits::default())
            .unwrap();
        assert!(runtime.begin_backend("reviewer").is_err());
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
}

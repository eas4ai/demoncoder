//! Host lifetime facts and narrowly bounded native command observation authority.
use super::{HostInvocation, Identity, Operation, Record, SharedRuntime};
use crate::{
    plugins::receipts::*,
    session::{NATIVE_END_BUDGET, SessionEnd, SessionStart},
};
use anyhow::{Context, Result, ensure};
use std::{
    os::unix::fs::MetadataExt,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionLifetime {
    pub version: u32,
    pub session: String,
    pub workspace: (u64, u64),
    pub source: SessionStart,
    pub plans: Vec<(crate::plugins::hook_types::HookEvent, String)>,
    pub end: Option<SessionEnd>,
    pub diagnostics: Vec<String>,
    /// An on-disk record alone can never restore executable lifetime authority.
    #[serde(skip)]
    pub(crate) deadline: Option<Instant>,
}

pub(super) fn lifetime(record: &Record, id: u64) -> Result<(&Operation, &NativeSessionLifetime)> {
    let operation = record
        .operations
        .iter()
        .find(|o| o.id == id)
        .context("native session lifetime missing")?;
    let Some(HostInvocation::NativeSession(lifetime)) = &operation.host_invocation else {
        anyhow::bail!("not a native session lifetime")
    };
    Ok((operation, lifetime))
}

pub(super) fn validate<'a>(
    record: &'a Record,
    id: u64,
    occurrence: &NonToolOccurrence,
) -> Result<(&'a Identity, (u64, u64))> {
    let (operation, owner) = lifetime(record, id)?;
    ensure!(
        !record.recovery_pending && !operation.reconciled,
        "native session observation requires reconciliation"
    );
    ensure!(
        owner.version == 1
            && operation.phase == "native-session"
            && operation.identity.as_ref() == Some(&record.identity),
        "native session execution identity changed"
    );
    ensure!(
        record
            .operations
            .iter()
            .rev()
            .find(|o| matches!(o.host_invocation, Some(HostInvocation::NativeSession(_))))
            .map(|o| o.id)
            == Some(id),
        "native session lifetime replaced"
    );
    ensure!(
        match occurrence {
            NonToolOccurrence::SessionStart { source } =>
                owner.end.is_none() && owner.source == *source,
            NonToolOccurrence::SessionEnd { reason } => owner.end == Some(*reason),
            _ => false,
        },
        "native session capability cannot authorize another occurrence"
    );
    ensure!(
        owner
            .deadline
            .is_some_and(|deadline| deadline > Instant::now()),
        "native session observation deadline expired or not live"
    );
    let metadata = std::fs::metadata(&record.workspace)?;
    ensure!(
        (metadata.dev(), metadata.ino()) == owner.workspace,
        "native session workspace identity changed"
    );
    Ok((&record.identity, owner.workspace))
}

impl SharedRuntime {
    pub(crate) fn validate_native_end_policy(
        &self,
        id: u64,
        reason: SessionEnd,
        plans: &[(crate::plugins::hook_types::HookEvent, String)],
    ) -> Result<()> {
        let runtime = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("native session lock failed"))?;
        ensure!(!runtime.failed, "session persistence failed");
        validate(
            &runtime.record,
            id,
            &NonToolOccurrence::SessionEnd { reason },
        )?;
        let (_, owner) = lifetime(&runtime.record, id)?;
        ensure!(
            owner.plans == plans,
            "native session hook policy differs from startup generation"
        );
        Ok(())
    }
    pub(crate) fn begin_native_session(
        &self,
        source: SessionStart,
        identity: Option<&Identity>,
        plans: Vec<(crate::plugins::hook_types::HookEvent, String)>,
    ) -> Result<u64> {
        let session = self.plugin_session()?;
        self.update(|record| {
            ensure!(
                identity.is_none_or(|identity| identity == &record.identity),
                "native startup execution identity differs"
            );
            ensure!(
                record.operations.len() < 4096,
                "session operation history is full"
            );
            let metadata = std::fs::metadata(&record.workspace)?;
            let id = record.operations.len() as u64 + 1;
            record.operations.push(Operation {
                id,
                budget: Some(if record.session_hook_allowance.is_some() {
                    super::BudgetRef::SessionHooks {
                        session: session.clone(),
                    }
                } else {
                    super::BudgetRef::Unallocated
                }),
                usage_receipt: None,
                phase: "native-session".into(),
                verification: None,
                call: None,
                result: None,
                tool_receipt: None,
                host_invocation: Some(HostInvocation::NativeSession(NativeSessionLifetime {
                    version: 1,
                    session: session.clone(),
                    workspace: (metadata.dev(), metadata.ino()),
                    source,
                    plans: plans.clone(),
                    end: None,
                    diagnostics: vec![],
                    deadline: Some(Instant::now() + Duration::from_secs(30)),
                })),
                // An open host lifetime is not itself an unresolved effect.
                complete: true,
                reconciled: false,
                usage_reported: true,
                identity: Some(record.identity.clone()),
            });
            Ok(id)
        })
    }

    pub(crate) fn end_native_session(&self, id: u64, reason: SessionEnd) -> Result<()> {
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("native session lifetime missing")?;
            let Some(HostInvocation::NativeSession(owner)) = &mut operation.host_invocation else {
                anyhow::bail!("not a native session lifetime")
            };
            ensure!(
                owner.end.is_none(),
                "native session termination already recorded"
            );
            owner.end = Some(reason);
            owner.deadline = Some(Instant::now() + NATIVE_END_BUDGET);
            Ok(())
        })
    }

    pub(crate) fn native_session_diagnostic(&self, id: u64, message: String) -> Result<()> {
        self.update(|record| {
            let operation = record
                .operations
                .iter_mut()
                .find(|o| o.id == id)
                .context("native session lifetime missing")?;
            let Some(HostInvocation::NativeSession(owner)) = &mut operation.host_invocation else {
                anyhow::bail!("not a native session lifetime")
            };
            if owner.diagnostics.len() < 32 {
                let message = if message.len() > 4096 {
                    let mut end = 4096 - " [truncated]".len();
                    while !message.is_char_boundary(end) {
                        end -= 1;
                    }
                    format!("{} [truncated]", &message[..end])
                } else {
                    message
                };
                owner.diagnostics.push(message);
            }
            owner.deadline = None;
            Ok(())
        })
    }

    pub(crate) fn plugin_remaining(
        &self,
        id: u64,
        event: crate::plugins::hook_types::HookEvent,
    ) -> Result<Duration> {
        use crate::plugins::hook_types::HookEvent;
        if !matches!(event, HookEvent::SessionStart | HookEvent::SessionEnd) {
            return self.remaining();
        }
        self.plugin_runner_owner(id, event)?;
        let record = self.record()?;
        let facts = &record
            .operations
            .iter()
            .find(|o| o.id == id)
            .and_then(Operation::non_tool_receipt)
            .context("native session occurrence missing")?
            .facts;
        let (_, owner) = lifetime(
            &record,
            facts
                .native_session
                .context("native session owner missing")?,
        )?;
        Ok(owner
            .deadline
            .context("native session is not live")?
            .saturating_duration_since(Instant::now()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn occurrence(source: SessionStart) -> NonToolOccurrence {
        NonToolOccurrence::SessionStart { source }
    }
    fn fixture() -> (tempfile::TempDir, tempfile::TempDir, SharedRuntime) {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut record = crate::inspection::tests::record(root.path());
        record.phase = None;
        let runtime = SharedRuntime::for_test(&state.path().join("record"), record).unwrap();
        (root, state, runtime)
    }
    fn begin(
        runtime: &SharedRuntime,
        id: u64,
        phase: &str,
        event: NonToolOccurrence,
    ) -> Result<NonToolFacts> {
        runtime.begin_non_tool_owned(
            phase,
            None,
            LifecycleOrigin {
                native_session: Some(id),
                ..Default::default()
            },
            event,
            "policy".into(),
            vec![],
        )
    }
    fn plans() -> Vec<(crate::plugins::hook_types::HookEvent, String)> {
        use crate::plugins::hook_types::HookEvent;
        vec![
            (HookEvent::SessionStart, "policy".into()),
            (HookEvent::SessionEnd, "policy".into()),
        ]
    }
    #[test]
    fn lifetime_rejects_stale_identity_workspace_and_reloaded_execution() {
        let (root, state, runtime) = fixture();
        let id = runtime
            .begin_native_session(SessionStart::Startup, None, plans())
            .unwrap();
        let saved: Record =
            serde_json::from_slice(&serde_json::to_vec(&runtime.record().unwrap()).unwrap())
                .unwrap();
        assert!(
            validate(&saved, id, &occurrence(SessionStart::Startup)).is_err(),
            "deserialization restored execution authority"
        );
        let moved = state.path().join("old-workspace");
        std::fs::rename(root.path(), &moved).unwrap();
        std::fs::create_dir(root.path()).unwrap();
        assert!(
            begin(
                &runtime,
                id,
                "native-session",
                occurrence(SessionStart::Startup)
            )
            .is_err()
        );
        std::fs::remove_dir(root.path()).unwrap();
        std::fs::rename(moved, root.path()).unwrap();
        runtime
            .update(|r| {
                r.identity.model = Some("replacement".into());
                Ok(())
            })
            .unwrap();
        assert!(
            begin(
                &runtime,
                id,
                "native-session",
                occurrence(SessionStart::Startup)
            )
            .is_err()
        );
        let next = runtime
            .begin_native_session(SessionStart::Resume, None, plans())
            .unwrap();
        assert!(next > id);
        assert!(
            begin(
                &runtime,
                id,
                "native-session",
                occurrence(SessionStart::Startup)
            )
            .is_err()
        );
        assert!(
            begin(
                &runtime,
                next,
                "native-session",
                occurrence(SessionStart::Resume)
            )
            .is_ok()
        );
    }
    #[test]
    fn lifetime_terminal_owner_records_held_fact_without_granting_effects() {
        let (_root, _state, runtime) = fixture();
        let id = runtime
            .begin_native_session(SessionStart::Startup, None, plans())
            .unwrap();
        runtime
            .update(|r| {
                r.recovery_pending = true;
                Ok(())
            })
            .unwrap();
        runtime
            .end_native_session(id, SessionEnd::Shutdown)
            .unwrap();
        assert!(
            begin(
                &runtime,
                id,
                "native-session",
                NonToolOccurrence::SessionEnd {
                    reason: SessionEnd::Shutdown
                }
            )
            .is_err()
        );
        let record = runtime.record().unwrap();
        assert_eq!(
            lifetime(&record, id).unwrap().1.end,
            Some(SessionEnd::Shutdown)
        );
        assert!(record.recovery_pending);
    }
    #[test]
    fn lifetime_does_not_reopen_stopped_or_accepted_task_or_reset_counters() {
        for accepted in [false, true] {
            let (root, _state, runtime) = fixture();
            let mut task = crate::workflow::state::Task::new(
                1,
                "original".into(),
                vec![],
                crate::workflow::workspace::capture(root.path()).unwrap(),
                1,
            )
            .unwrap();
            task.stopped = true;
            if accepted {
                task.accepted = Some("accepted-original".into());
            }
            runtime
                .allocate(crate::workflow::allocation::Limits::default(), None)
                .unwrap();
            runtime.save_task(&Some(task.clone()), 2, None).unwrap();
            runtime
                .update(|r| {
                    r.allocation.as_mut().unwrap().model_calls = 3;
                    r.allocation.as_mut().unwrap().tool_calls = 4;
                    r.allocation.as_mut().unwrap().deadline_ms = 1;
                    Ok(())
                })
                .unwrap();
            let before = runtime.record().unwrap().allocation.unwrap();
            let id = runtime
                .begin_native_session(SessionStart::Startup, None, plans())
                .unwrap();
            runtime
                .end_native_session(id, SessionEnd::Shutdown)
                .unwrap();
            let facts = begin(
                &runtime,
                id,
                "native-session",
                NonToolOccurrence::SessionEnd {
                    reason: SessionEnd::Shutdown,
                },
            )
            .unwrap();
            let context = runtime
                .non_tool_model_context(
                    facts.operation,
                    crate::plugins::hook_types::HookEvent::SessionEnd,
                )
                .unwrap();
            assert!(!context.allocation_available && !context.correction_available);
            assert!(runtime.admit_non_tool_correction(facts.operation).is_err());
            let record = runtime.record().unwrap();
            let after = record.allocation.unwrap();
            assert_eq!(
                (
                    after.started_ms,
                    after.deadline_ms,
                    after.model_calls,
                    after.tool_calls
                ),
                (before.started_ms, before.deadline_ms, 3, 4)
            );
            assert_eq!(
                serde_json::to_value(record.task.unwrap()).unwrap(),
                serde_json::to_value(task).unwrap()
            );
            assert!(record.phase.is_none());
        }
    }
    #[test]
    fn lifetime_observation_is_exact_and_cannot_replay_after_settlement() {
        let (_root, _state, runtime) = fixture();
        let id = runtime
            .begin_native_session(
                SessionStart::Startup,
                None,
                vec![(
                    crate::plugins::hook_types::HookEvent::SessionStart,
                    "policy".into(),
                )],
            )
            .unwrap();
        assert!(begin(&runtime, id, "worker", occurrence(SessionStart::Startup)).is_err());
        assert!(
            begin(
                &runtime,
                id,
                "agent:1:worker",
                occurrence(SessionStart::Startup)
            )
            .is_err()
        );
        assert!(
            begin(
                &runtime,
                id,
                "native-session",
                occurrence(SessionStart::Resume)
            )
            .is_err()
        );
        assert!(
            begin(
                &runtime,
                id,
                "native-session",
                NonToolOccurrence::Stop {
                    stop_hook_active: false,
                    last_assistant_message: None
                }
            )
            .is_err()
        );
        let facts = begin(
            &runtime,
            id,
            "native-session",
            occurrence(SessionStart::Startup),
        )
        .unwrap();
        runtime
            .settle_non_tool(
                facts.operation,
                crate::plugins::hook_types::HookEvent::SessionStart,
                Default::default(),
            )
            .unwrap();
        assert!(
            begin(
                &runtime,
                id,
                "native-session",
                occurrence(SessionStart::Startup)
            )
            .is_err(),
            "settled startup replayed under the same lifetime"
        );
    }
}

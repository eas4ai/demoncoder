//! Transferred observer facts remain attached to their original hook receipt.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Running,
    Completed,
    Interrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Pending,
    Reserved,
    Delivered,
    Withheld,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObserverReceipt {
    pub owner: String,
    pub task: Option<u64>,
    pub allocation_started_ms: u64,
    pub deadline_ms: u64,
    pub status: Status,
    pub delivery: Delivery,
    pub rewake: bool,
    /// Launch protocol is separate from the eventual command outcome.
    pub launch_marker: Option<serde_json::Value>,
}

/// Trusted runner configuration; source output cannot mint this eligibility.
#[derive(Clone, Copy)]
pub struct ObserverConfig {
    pub declared: bool,
    pub rewake: bool,
    pub timeout_ms: u64,
}

/// A transferred result is settled by its existing runtime job, not by the caller.
pub(crate) async fn dispatch(
    mut invocation: super::dispatch::HookInvocation,
    runner: std::sync::Arc<dyn super::dispatch::HookRunner>,
) -> anyhow::Result<Option<super::receipts::RawOutcome>> {
    let Some(config) = runner.observer_config() else {
        return Ok(Some(
            super::dispatch::run_owned(&invocation, runner.as_ref()).await,
        ));
    };
    let (runtime, _) = invocation.events.plugin_context()?;
    let lease = runtime.admit_observer(&invocation, config, runner.mutates_workspace())?;
    invocation.events = invocation.events.for_observer();
    invocation.observer = Some(lease.clone());
    if config.declared || config.rewake {
        lease.transfer(None)?;
    }
    let (sender, mut result) = tokio::sync::oneshot::channel();
    struct PendingTransfer(
        std::sync::Arc<crate::workflow::runtime::plugin_observer::ObserverLease>,
    );
    impl Drop for PendingTransfer {
        fn drop(&mut self) {
            if !self
                .0
                .transferred
                .load(std::sync::atomic::Ordering::Acquire)
            {
                self.0.revoke();
            }
        }
    }
    let _pending = PendingTransfer(lease.clone());
    let owner = lease.clone();
    let handle = tokio::spawn(async move {
        struct Completion {
            owner: std::sync::Arc<crate::workflow::runtime::plugin_observer::ObserverLease>,
            armed: bool,
        }
        impl Drop for Completion {
            fn drop(&mut self) {
                if self.armed
                    && self
                        .owner
                        .transferred
                        .load(std::sync::atomic::Ordering::Acquire)
                {
                    self.owner.abandoned();
                }
            }
        }
        let mut completion = Completion {
            owner: owner.clone(),
            armed: true,
        };
        let work = runner.run(&invocation);
        tokio::pin!(work);
        let outcome = loop {
            tokio::select! {
                result = &mut work => break result.unwrap_or_else(|_| super::receipts::RawOutcome::Failure {reason:"observer runner failed; effects may be unknown".into()}),
                _ = tokio::time::sleep(std::time::Duration::from_millis(5)) => {
                    if owner.validate().is_err() { break super::receipts::RawOutcome::Failure {reason:"observer owner revoked or expired; effects may be unknown".into()}; }
                }
            }
        };
        if owner.transferred.load(std::sync::atomic::Ordering::Acquire) {
            // A persistence failure latches the runtime; it cannot deliver or replay.
            if owner.settle(outcome).is_err() {
                owner.abandoned();
            }
            completion.armed = false;
            let _ = sender.send(None);
        } else {
            let _ = sender.send(Some(outcome));
        }
    });
    runtime.retain_observer_job(&lease, handle)?;
    if lease.transferred.load(std::sync::atomic::Ordering::Acquire) {
        return Ok(None);
    }
    tokio::select! {
        _ = lease.changed.notified() => Ok(None),
        result = &mut result => result.map_err(|_|anyhow::anyhow!("observer dispatch owner interrupted")),
    }
}

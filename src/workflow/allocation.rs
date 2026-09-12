//! Cumulative clocks and counters, including time spent between turns.
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Limits {
    pub seconds: u64,
    pub model_calls: u64,
    pub tool_calls: u64,
}

impl Limits {
    pub(crate) fn validate_session_hooks(&self) -> Result<()> {
        ensure!(
            (1..=86400).contains(&self.seconds),
            "session-hook deadline must be 1 to 86400 seconds"
        );
        ensure!(
            self.model_calls <= 4096,
            "session-hook model-call limit must be 0 to 4096"
        );
        ensure!(
            self.tool_calls <= 4096,
            "session-hook tool-call limit must be 0 to 4096"
        );
        Ok(())
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            seconds: 900,
            model_calls: 64,
            tool_calls: 128,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Usage {
    pub reported_input: u64,
    pub reported_output: u64,
    pub reported_cached: u64,
    pub reported_cost_usd: f64,
    pub unknown_input: bool,
    pub unknown_output: bool,
    pub unknown_cached: bool,
    pub unknown_cost: bool,
}

impl Usage {
    pub fn add(
        &mut self,
        input: Option<u64>,
        output: Option<u64>,
        cached: Option<u64>,
        cost: Option<f64>,
    ) -> Result<()> {
        for (value, total, unknown) in [
            (input, &mut self.reported_input, &mut self.unknown_input),
            (output, &mut self.reported_output, &mut self.unknown_output),
            (cached, &mut self.reported_cached, &mut self.unknown_cached),
        ] {
            match value {
                Some(n) => {
                    *total = total
                        .checked_add(n)
                        .context("task usage counter overflow")?
                }
                None => *unknown = true,
            }
        }
        match cost {
            Some(cost)
                if cost.is_finite()
                    && cost >= 0.0
                    && (self.reported_cost_usd + cost).is_finite() =>
            {
                self.reported_cost_usd += cost
            }
            _ => self.unknown_cost = true,
        }
        Ok(())
    }

    pub fn uncertain(&mut self) {
        self.unknown_input = true;
        self.unknown_output = true;
        self.unknown_cached = true;
        self.unknown_cost = true;
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Allocation {
    pub limits: Limits,
    pub started_ms: u64,
    pub deadline_ms: u64,
    pub observed_ms: u64,
    pub clock_invalid: bool,
    #[serde(skip, default = "Instant::now")]
    anchor: Instant,
    pub model_calls: u64,
    pub tool_calls: u64,
    pub usage: Usage,
}

pub fn now_ms() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock precedes Unix epoch")?
        .as_millis()
        .try_into()
        .context("system time overflow")
}

impl Allocation {
    pub fn new(limits: Limits) -> Result<Self> {
        ensure!(
            limits.seconds > 0 && limits.seconds <= 86400,
            "task deadline must be 1 to 86400 seconds"
        );
        ensure!(
            limits.model_calls > 0 && limits.model_calls <= 4096,
            "task model-call limit must be 1 to 4096"
        );
        ensure!(
            limits.tool_calls > 0 && limits.tool_calls <= 4096,
            "task tool-call limit must be 1 to 4096"
        );
        Self::from_validated(limits)
    }

    /// Call only after the owning allowance has validated its limits.
    pub(super) fn from_validated(limits: Limits) -> Result<Self> {
        let started_ms = now_ms()?;
        let deadline_ms = started_ms
            .checked_add(limits.seconds * 1000)
            .context("task deadline overflow")?;
        Ok(Self {
            limits,
            started_ms,
            deadline_ms,
            observed_ms: started_ms,
            clock_invalid: false,
            anchor: Instant::now(),
            model_calls: 0,
            tool_calls: 0,
            usage: Usage::default(),
        })
    }

    pub fn remaining_ms(&self) -> Result<u64> {
        ensure!(
            !self.clock_invalid,
            "system clock moved backwards; task execution is held"
        );
        let now = self.effective_now(now_ms()?)?;
        Ok(self.deadline_ms.saturating_sub(now))
    }

    fn effective_now(&self, wall: u64) -> Result<u64> {
        ensure!(
            wall >= self.observed_ms,
            "system clock moved backwards; task execution is held"
        );
        Ok(wall.max(
            self.observed_ms
                .saturating_add(self.anchor.elapsed().as_millis().min(u64::MAX as u128) as u64),
        ))
    }

    pub fn observe_time(&mut self) -> Result<()> {
        ensure!(
            !self.clock_invalid,
            "system clock moved backwards; task execution is held"
        );
        self.observed_ms = self.effective_now(now_ms()?)?;
        self.anchor = Instant::now();
        Ok(())
    }

    /// Preserve the latest elapsed time without discarding a completed result if
    /// the wall clock becomes invalid. Further admissions remain held after resume.
    pub fn checkpoint_time(&mut self) {
        if self.observe_time().is_err() {
            self.clock_invalid = true;
        }
    }

    pub fn admit(&mut self, model: bool) -> Result<()> {
        self.observe_time()?;
        ensure!(
            self.remaining_ms()? > 0,
            "cumulative task deadline exhausted"
        );
        if model {
            ensure!(
                self.model_calls < self.limits.model_calls,
                "cumulative task model-call allowance exhausted"
            );
            self.model_calls += 1;
        } else {
            ensure!(
                self.tool_calls < self.limits.tool_calls,
                "cumulative task tool-call allowance exhausted"
            );
            self.tool_calls += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rollback_and_monotonic_elapsed_cannot_extend_allocation() {
        let mut allocation = Allocation::new(Limits::default()).unwrap();
        allocation.observed_ms += 1000;
        assert!(
            allocation
                .effective_now(allocation.observed_ms - 1)
                .is_err()
        );
        allocation.anchor = Instant::now() - std::time::Duration::from_secs(2);
        assert!(
            allocation.effective_now(allocation.observed_ms).unwrap()
                >= allocation.observed_ms + 2000
        );
        let recovered: Allocation =
            serde_json::from_value(serde_json::to_value(&allocation).unwrap()).unwrap();
        assert!(recovered.effective_now(allocation.observed_ms - 1).is_err());
        allocation.checkpoint_time();
        assert!(allocation.clock_invalid);
        let recovered: Allocation =
            serde_json::from_value(serde_json::to_value(&allocation).unwrap()).unwrap();
        assert!(recovered.remaining_ms().is_err());
    }
    #[test]
    fn serialized_allocation_does_not_reset_consumption() {
        let mut allocation = Allocation::new(Limits {
            seconds: 30,
            model_calls: 1,
            tool_calls: 1,
        })
        .unwrap();
        allocation.admit(true).unwrap();
        allocation.admit(false).unwrap();
        let mut recovered: Allocation =
            serde_json::from_str(&serde_json::to_string(&allocation).unwrap()).unwrap();
        assert!(recovered.admit(true).is_err());
        assert!(recovered.admit(false).is_err());
        assert_eq!(recovered.deadline_ms, allocation.deadline_ms);
    }

    #[test]
    fn incomplete_usage_remains_unknown_after_known_reports() {
        let mut usage = Usage::default();
        usage.add(None, Some(8), None, None).unwrap();
        usage.add(Some(11), Some(9), Some(0), Some(0.01)).unwrap();
        assert_eq!(usage.reported_output, 17);
        assert!(usage.unknown_input && usage.unknown_cached && usage.unknown_cost);
        assert!(!usage.unknown_output);
    }
}

//! An explicit session grant persists independently from task and delegation funding.
use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::workflow::allocation::{Allocation, Limits};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionHookAllowance {
    pub allocation: Allocation,
    /// Host-admitted external hook invocations, not backend-internal model calls.
    #[serde(default)]
    pub backend_invocations: u64,
}

impl SessionHookAllowance {
    pub fn new(limits: Limits) -> Result<Self> {
        limits.validate_session_hooks()?;
        Ok(Self {
            allocation: Allocation::from_validated(limits)?,
            backend_invocations: 0,
        })
    }
}

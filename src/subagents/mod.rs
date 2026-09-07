//! Parent-owned delegated assignments and their isolated worktrees.
pub mod manager;
pub mod schedule;
pub mod session;
pub mod state;
pub mod worktree;

use crate::{config::Connection, workflow::allocation::Limits};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct Settings {
    pub connections: BTreeMap<String, Connection>,
    pub reviewer: Option<Connection>,
    pub checks: Vec<String>,
    pub limits: Limits,
    pub max_active: u32,
    pub backend_limit: u64,
}

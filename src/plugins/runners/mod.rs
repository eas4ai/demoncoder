//! Explicit host-selected runners; package inspection is not activation.
mod command;
mod launch;
mod package;
mod process;
pub(crate) mod snapshot;
mod staging;
pub use command::{CommandConfig, CommandProgram, CommandRunner, NetworkGrant};

/// Only ToolExecutor constructs this capability from its admitted host policy.
#[derive(Clone)]
pub(crate) struct HookHost {
    root: std::sync::Arc<std::fs::File>,
    workspace: std::path::PathBuf,
    credentials: Vec<std::path::PathBuf>,
    supervisor: Option<std::path::PathBuf>,
}
impl HookHost {
    pub(crate) fn new(
        root: std::sync::Arc<std::fs::File>,
        workspace: std::path::PathBuf,
        credentials: Vec<std::path::PathBuf>,
        supervisor: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            root,
            workspace,
            credentials,
            supervisor,
        }
    }
}

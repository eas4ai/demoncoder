//! Explicit host-selected runners; package inspection is not activation.
pub(crate) mod command;
mod event;
pub(crate) mod http;
mod inspection;
pub(crate) mod launch;
mod mcp;
mod mcp_input;
mod model;
pub(crate) mod package;
pub(crate) mod process;
pub(crate) mod snapshot;
mod staging;
pub use command::{CommandConfig, CommandProgram, CommandRunner, NetworkGrant};
pub use http::{HttpConfig, HttpCredential, HttpRunner};
pub use inspection::SnapshotInspection;
pub use mcp::{McpBinding, McpConfig, McpRunner};
pub use model::{ModelConfig, ModelRunner};

/// Only ToolExecutor constructs this capability from its admitted host policy.
#[derive(Clone)]
pub(crate) struct HookHost {
    pub(crate) root: std::sync::Arc<std::fs::File>,
    pub(crate) workspace: std::path::PathBuf,
    pub(crate) credentials: Vec<std::path::PathBuf>,
    pub(crate) supervisor: Option<std::path::PathBuf>,
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

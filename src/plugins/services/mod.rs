//! Host-owned managed MCP services.
mod http;
mod mcp;
mod protocol;
mod stdio;
pub use mcp::{
    AdmittedTool, ManagedService, ManagedServices, ServiceConfig, ServiceIdentity, ServiceState,
    ServiceTransport, StdioConfig,
};

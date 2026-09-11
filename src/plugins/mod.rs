//! Immutable package inspection. Import never executes or activates package code.
pub mod bridge;
pub mod codex_relay;
mod manifest;
mod snapshot;
mod types;
pub use manifest::MAX_COMPONENT_PATHS;
pub use snapshot::{MAX_ENTRIES, MAX_FILE_BYTES, MAX_PATH_DEPTH, MAX_TOTAL_BYTES};
pub use types::*;
pub fn inspect(source: &std::path::Path, options: &ImportOptions) -> anyhow::Result<Package> {
    manifest::inspect(snapshot::capture(source)?, options)
}

pub mod hook_types;
pub mod profile;
pub mod wire;

pub mod results;

pub mod gate_snapshot;

pub mod admission;
pub mod dispatch;
pub mod lifecycle;
pub mod receipts;
pub mod runners;
pub mod services;

pub mod once;

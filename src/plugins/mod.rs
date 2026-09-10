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

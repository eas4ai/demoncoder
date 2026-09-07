mod anthropic;
mod claude;
mod codex;
mod http;
mod openai;
mod process;

use crate::session::{ADAPTER_INTERFACE_VERSION, Registry, SessionCapabilities};
use anyhow::Result;

// Coding sessions have tools; the no-tools Oracle keeps its separate role.
const CREATOR_INSTRUCTIONS: &str = include_str!("creator.md");

pub fn builtins() -> Result<Registry> {
    let mut registry = Registry::default();
    for (name, factory) in [
        ("openai-api", openai::open as crate::session::Factory),
        ("anthropic-api", anthropic::open),
        ("codex", codex::open),
        ("claude", claude::open),
    ] {
        registry.register_with_capabilities(
            name,
            ADAPTER_INTERFACE_VERSION,
            SessionCapabilities::CODING_SESSION,
            factory,
        )?;
    }
    Ok(registry)
}

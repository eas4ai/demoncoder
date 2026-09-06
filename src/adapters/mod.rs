mod anthropic;
mod claude;
mod codex;
mod http;
mod openai;
mod process;

use crate::session::{ADAPTER_INTERFACE_VERSION, Registry};
use anyhow::Result;

pub fn builtins() -> Result<Registry> {
    let mut registry = Registry::default();
    for (name, factory) in [
        ("openai-api", openai::open as crate::session::Factory),
        ("anthropic-api", anthropic::open),
        ("codex", codex::open),
        ("claude", claude::open),
    ] {
        registry.register(name, ADAPTER_INTERFACE_VERSION, factory)?;
    }
    Ok(registry)
}

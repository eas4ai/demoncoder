# Use a small Rust terminal host with explicit session owners

Level: Judged
Decided by: Codex
Rests on: Agreed CODE-001 through CODE-008 and CONN-001 through CONN-006
Would be wrong if: The host duplicates an external backend loop, the terminal blocks on provider work, or adapters cannot enforce the agreed tool boundary

## Decision

Build one Rust crate with a terminal editor, a versioned session adapter interface, and native API adapters. Use Tokio for cancellable I/O, Crossterm and Ratatui for the terminal, and Reqwest for HTTPS streams. Codex app-server and Claude stream-json own their external loops. Each adapter emits the same application events and receives prompt, steering, and cancellation commands. Start with the real terminal-to-transport path for CODE-001, then add tool execution and the remaining behaviors in Cairn order. Protocol fixtures exercise the production adapter path; live authentication remains separately required. Do not import an entire reference crate or add a JavaScript extension runtime.

## Realized by

- bc8892fcf71fb3cc0c48becccd40f0d468489751 Connect terminal prompts to four session transports

This commit implements terminal prompt submission and the session registry.
Tools and the remaining first-session behavior still await their checks.

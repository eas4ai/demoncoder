# Plugin result decoder conformance

`tests/plugin_results.rs` constructs response cases directly so the expected typed
effects remain visible beside each assertion. It exercises all 34 frozen events
across the Native, Claude, and Codex dialects, transport applicability, callbacks,
source decision precedence, transport failures, and event-specific effects.

The contract is `docs/spec/plugin-compatibility.md`,
`docs/spec/plugin-runtime-contract.md`, and `docs/spec/lifecycle-hooks.md`.
Compatibility validation uses the repository's frozen profile. Claude behavior was
checked against https://code.claude.com/docs/en/hooks; Codex behavior was checked
against `reference/codex-rust-v0.153.4/codex-rs/hooks/src/engine/output_parser.rs`,
`events/`, and `schema.rs`. Native validation has its own explicit event table.

`deletion-evidence.txt` records a controlled mutation: deleting the production
`ReplaceDynamicWatches` emission causes the unchanged watch replacement test to
fail, including its explicit empty-list clear case. Restoring the original bytes
makes the same test pass. The mutation was temporary and is not part of the code.

This suite proves decoding into untrusted proposals. It does not prove host
admission, effect execution, worktree ownership, filesystem permission, persistence,
UI rendering, or invocation-bound receipt retention. Those belong to the host
integration. In particular, the host must validate a proposed replacement output
before applying its accompanying classifier context.

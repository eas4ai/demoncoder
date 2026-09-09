# Managed Language Services Implementation Plan

> Execute in the existing Cairn loop. The shared executor integration is coupled;
> delegate only isolated protocol work and independent reviews.

**Goal:** Deliver confirmed LSP-001 through LSP-006 on all four connections.

**Architecture:** ToolExecutor owns optional language service configuration and
bounded stdio servers. Its ordinary admission, confinement and completion receipts
remain authoritative. Diagnostics describe a source revision, never verification.

**Tech Stack:** Rust, Tokio, serde_json, existing bubblewrap policy; installed
rust-analyzer and typescript-language-server. No imported reference runtime.

## Tasks

- [x] Build and verify protocol, explicit configuration and shared
  tool integration, including the approved filtered filesystem, debounced
  synchronization, revision-aware diagnostics and process cleanup.
- [x] Add and run adversarial production-path tests, all-adapter fixtures and
  installed-server smoke checks; demonstrate each falsifier can fail.
- [x] Complete documentation, record the decision implementation, commit, run
  Cairn evidence, and review against the six requirements and production rules.

### Files and contracts

`src/language_services/protocol.rs` owns byte-length framing only. Export
`read_message<R: AsyncBufRead + Unpin>(&mut R) -> Result<Value>` and
`write_message<W: AsyncWrite + Unpin>(&mut W, &Value) -> Result<()>` as crate-visible
async functions. Reject duplicate/missing/invalid lengths, headers over 8 KiB,
bodies over 1 MiB, incomplete messages and non-object JSON. Unit tests use Tokio
duplex streams to split Unicode at every boundary and exercise malformed frames.

`src/language_services/mod.rs` owns configuration, bounded server lifecycle,
document revisions and result interpretation. Add `lsp` with operations status,
definition, references, hover and diagnostics. Tool positions are zero-based
UTF-16 with validation against current text. Use file URIs and source digests;
reject unsupported capabilities explicitly. Bound retained documents and frames.
Reject unsolicited edits/commands. Only matching versioned push diagnostics or
a current pull response can be current; unversioned push is freshness unknown.

`src/developer_access.rs` exposes the existing private-path list to language
admission. Preserve the existing Bash launch and socket filter.
`src/tools.rs` adds optional configuration and one manager, registers the tool,
validates source access and preserves completed mutation receipts before waiting
for diagnostics. Disabled/read-only/child policy behavior stays explicit.
`src/config.rs` adds opt-in executable flags; `src/lib.rs` registers the module.

The approved confinement correction supersedes reuse of the live developer
filesystem for language servers. A dedicated filtered view uses pinned file
descriptors, independent admitted copies and explicitly selected external read
roots. Use reflinks when supported with a bounded copy fallback. Keep the shared
socket endpoint restrictions and process-lifetime rules. The LSP-specific filter
also permits anonymous sequenced socket pairs for Cargo. Do not expose the live project or host
home. Add bounded deny decisions, real Git ignore matching, timer-driven
debounced reconciliation, and immediate synchronization before explicit queries.
Verify startup and late private files, dependency-root exclusions, alias races,
final-batch flushing and policy invalidation through production execution.

`tests/language_services.rs` and `tests/lsp_fixture.py` exercise the production
executor and controlled protocol failures. Test the concrete mutation invariant:

```rust,ignore
let result = tools.execute(write_call, &events).await.unwrap();
assert!(result.success);
assert_eq!(std::fs::read_to_string(source).unwrap(), corrected_text);
assert!(result.output.contains("diagnostics"));
// A server failure cannot erase the successful mutation or claim clean checks.
```

Adapter fixtures must capture actual requests/results on OpenAI, Anthropic,
Codex and Claude routes, with no model-success surrogate for tool execution.
Installed tests use harmless temporary Rust/TypeScript projects and selected
executables, and assert actual definition/hover and diagnostic behavior.

### Verification and delivery

Run targeted Rust tests while editing, then `cargo fmt --check`,
`cargo clippy --locked --all-targets -- -D warnings`, the dedicated
`scripts/check-managed-language-services.sh`, and applicable completed-product
regressions. The dedicated gate names real tests and must fail when they are
absent; a zero-test Cargo filter is not evidence. Record negative/corrected cases
in the commitment review. Commit implementation before `cairn check LSP-001`,
then commit its receipts and follow each subsequent wake verdict.

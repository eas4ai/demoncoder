# Own bounded language servers inside the shared tool executor

Superseded by: admit-filtered-filesystem-copies-before-language-server-access

Level: Judged
Decided by: Codex
Rests on: Agreed LSP-001 through LSP-006; shared executor and confined process decisions
Would be wrong if: A server bypasses confinement, stale diagnostics appear current, cancellation loses a completed edit, or an initial connection cannot use the same tools

## Decision

Add explicitly selected installed Rust and TypeScript stdio servers to the existing shared tool executor. Keep language servers confined even for host-mode sessions; child and review-only sessions expose no language server capability in this slice. Reuse the developer process policy with a separate pinned workspace descriptor so stdin can carry framed protocol messages. Bound frames, requests and retained documents; own each server process and stop it when the active request is cancelled or the executor is dropped. Synchronize document revisions, accept current diagnostics only with matching version or a current pull response, and label unversioned push results freshness unknown. Preserve mutation receipts before waiting for diagnostic feedback. Reject server-initiated edits and commands. Configure executable paths only through explicit invocation flags; do not discover executable project configuration.


Installed-server testing found that the machine's configured sccache compiler
wrapper cannot run under the existing Unix-socket policy. A confined direct
compiler run reports the real E0308 type error and its correction. For the Rust
language-server child only, set RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER to empty
values so Cargo uses rustc directly. Keep the existing socket policy and ordinary
Bash environment. Document this language-service compilation environment; its
diagnostics do not replace project verification.

## Realized by

(none yet: recorded, not built)

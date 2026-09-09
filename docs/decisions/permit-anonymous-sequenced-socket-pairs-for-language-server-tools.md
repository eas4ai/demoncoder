# Permit anonymous sequenced socket pairs for language-server tools

Level: Judged
Decided by: Codex
Rests on: LSP-001 LSP-004 LSP-005
Would be wrong if: Language servers can create host Unix endpoints, or the ordinary Bash socket policy changes
History: The earlier reversal removed live host filesystem exposure. This change keeps the approved copied filesystem intact and affects anonymous IPC only; Judged remains appropriate because host endpoint creation stays denied.

## Decision

Installed Rust compiler checking fails when Cargo creates an AF_UNIX SOCK_SEQPACKET anonymous pair. Add an explicit language-server filter variant permitting stream and sequenced-packet socketpair calls with known flags. Continue denying all AF_UNIX socket calls and io_uring setup. Keep the ordinary Bash and worktree filters unchanged. Verify emitted BPF and installed Rust error-to-clean behavior.

## Realized by

(none yet: recorded, not built)

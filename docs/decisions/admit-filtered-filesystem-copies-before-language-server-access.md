# Admit filtered filesystem copies before language-server access

Level: Consequential
Decided by: Developer and Codex
Supersedes: own-bounded-language-servers-inside-the-shared-tool-executor
Cause: the stated condition occurred
Rests on: LSP-001 through LSP-006; approved lsp-004-usable-003 escalation; developer confirmed deterministic exclusions, denied-file cache and debounced filesystem observation
Would be wrong if: A private file enters the server view, a live host data mount bypasses admission, a pending edit produces current-looking old diagnostics, or installed Rust and TypeScript cannot run within the declared view

## Decision

Replace the language-server launch view with admitted project and explicitly selected runtime/dependency copies at their original paths in a fresh filesystem. Keep ordinary Bash and native read policy unchanged. Check known private roots and conservative filename rules before reading bytes; reject unsafe aliases and special files. Keep a bounded session deny cache with reasons and a policy version, and re-evaluate when relevant rules change. Respect Git ignore rules for automatic source inclusion; explicit dependency selection may include ignored dependencies but never protected data. Observe filesystem changes, debounce background reconciliation with an actual timer and a maximum delay, and flush required source changes before explicit queries. Use descriptor-validated reflink copies when supported with bounded byte-copy fallback; never use a live source directory as an overlay lower layer. Stop or invalidate servers before removing admitted files or changing policy so retained diagnostics cannot bypass exclusions. Model calls are not part of file admission. Bound retained files, bytes, cache entries, requests and cleanup; errors remain visible and never fall back to broad host access. Preserve shared tool behavior and all installed-server and adapter falsifiers.

Installed Rust checks exposed a concrete read-only limitation: Cargo could not
create Cargo.lock in a fresh project. Give each server a disposable writable
overlay over the already admitted project copy. Never use the original project
as its lower layer. Host reconciliation reads only its own admitted copies,
never server-written overlay files. Stop the server and discard its overlay
when admitted inputs or policy change. External runtime copies stay read-only.
Include narrowly selected system DNS and CA files for the existing permitted
network access, with the same declared-credential overlap check as other system
runtimes. An unavailable overlay remains a launch failure, not a host fallback.

## Realized by

(none yet: recorded, not built)

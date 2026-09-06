# First coding session: specification review

Date: 2026-09-06
Status: Draft reviewed; developer confirmation pending
Scope: docs/spec/, docs/commitments/first-coding-session.md, and the two proposed mechanism declarations

## What the review attacked

- A nominal TUI that only prints a completed answer: CODE-001 and CODE-003 require the production terminal path and observations while provider and tool operations are still held open.
- Tests of an isolated model loop that never exercise the product: CODE-002 and CODE-006 use the normal session, real temporary-repository effects, and a second prompt with retained context.
- A cancelled request that leaves a tool child running: CODE-005 explicitly checks owned subprocess completion and continued use of the session.
- A hook that changes arguments after permission validation: CODE-007 checks the final request through the actual executor with harmless fixtures.
- A presentation hook that overwrites a failed check: CODE-008 preserves the original result and its call identity.
- A provider menu that advertises unimplemented choices: CONN-001 requires actual transport evidence for all four initial connections.
- Treating Codex and Claude Code as raw model APIs: CONN-004 assigns one loop owner and requires backend-session identity checks.
- Authentication silently changing the billing route: CONN-003 tests distinct synthetic credentials, missing authentication, and failure without fallback.
- A supposedly extensible registry that requires provider branches in the loop: CONN-002 exercises an additional registered adapter through production session creation.
- Stubbed or skipped tests producing success: the commitment requires per-requirement output, live transport evidence, explicit unresolved prerequisites, and failure demonstrations when mechanisms are implemented.
- A scope cut that quietly removes the actual product: later roadmap entries retain subagents, advanced orchestration, and self-improvement; only the first commitment is detailed now.

## Corrections made

The broad narrative previously described all subagents as using the native
loop. It now distinguishes native sessions from external backend sessions
without assigning two owners to either. The cancellation requirement now
explicitly preserves a usable session, matching its falsifier. The first
commitment includes all four requested connections rather than treating one
working provider as sufficient.

## Limits and remaining decisions

No application, check driver, or adapter has been implemented. Source and
documentation inspection does not prove that external backends meet every
session requirement. In particular, steering at the required boundary,
pre-execution admission, subprocess cancellation, and complete usage
reporting need executable evidence against pinned versions.

The exact terminal toolkit, Rust components to reuse, and confinement
implementation remain engineering decisions to record against the agreed
contract. A backend limitation that would change required behavior needs a
developer decision. Public third-party authentication services and fixed
subscription allowances are not promised by this local-client design.

The proposed mechanisms are declarations of future work. Safe violating and
corrected demonstrations cannot run until their drivers and application
paths exist. No implementation review or runtime pass is recorded here.

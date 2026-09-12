# Retain completed tool evidence before synchronous lifecycle effects

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-008,HOOK-011,PCOMP-002,PRUN-001
Would be wrong if: A post-tool hook overwrites an actual result, fabricates a tool event, repeats an uncertain effect, or permits work after a required continuation hold.

## Decision

Extend the existing shared tool boundary with synchronous PostToolUse and PostToolUseFailure dispatch. Retain the original completed result before any lifecycle runner await, and record causal lifecycle admission and outcomes in the existing operation ledger. Keep post-operation receipts separate from pre-tool approval; allowing a completed result to own observers must not reopen pre-tool effect authority. Preserve historical receipt decoding. Generalize the existing five runners and strict source frame/result decoding rather than introducing parallel transport implementations. Dispatch only events supported by actual host execution facts and the frozen source applicability table; a pre-admission rejection is not an executed successful tool.

Apply source-supported context, feedback and model-facing replacements without changing original result identity, success, exit status or evidence. A continuation hold stops future work while keeping the completed mutation visible. Known observation-only failures remain visible without becoming retroactive denials. Uncertain side effects retain ordinary reconciliation requirements and never replay automatically. Follow-up must use the owning task and cumulative correction/allocation policy; absent or exhausted correction authority stops unmet work. Handler output cannot accept work or create developer authority. Native and both external backend loops must respect the same post-operation continuation state before further work.

This is the next lifecycle prerequisite within the full selected commitment. Preserve stable ordering, bounded concurrent source groups, immutable configuration and original ownership. Generalized event payloads must remain host-created and source-validated. Later lifecycle transitions, one-shot and asynchronous job ownership, public registration/activation and the complete all-four-connection matrix remain required; this synchronous tool boundary does not claim their completion.

## Realized by

- 6ab79f4318d464585d5b3fb7b94ab854d2f88cd1 Retain tool evidence through synchronous plugin lifecycle effects

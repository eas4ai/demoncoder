# Run completed product gates with explicit local provider coverage

Level: Judged
Decided by: Codex
Rests on: AUD-006 CODE-007 CODE-008 CONN-003
Would be wrong if: The aggregate omits installed backend tool or result cycles, reports paid availability from local traffic, or ignores any failed constituent check

## Decision

Add one cumulative script that runs every completed product check, the full Rust checks, and installed-release security, workflow and provider regressions. Give only the coding-session and connections runners an explicit --local-only option that omits their final historical paid-provider validation and prints unverified for those cases. Their default behavior remains unchanged. The aggregate uses these local-only options, retains real installed backend execution through the disposable TLS fixture and all negative route checks, and exits on a failing constituent command.

## Realized by

(none yet: recorded, not built)

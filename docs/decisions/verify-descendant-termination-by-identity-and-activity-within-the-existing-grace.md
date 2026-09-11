# Verify descendant termination by identity and activity within the existing grace

Level: Judged
Decided by: agent
Rests on: HOOK-006,CODE-005,REL-002
Would be wrong if: A live owned descendant continues after the cancellation grace, the direct supervisor is not reaped, or the revised oracle accepts the live negative control.
History: The two recorded CODE-domain reversals concern private-source export and show that a convenient proxy can miss the actual invariant. They do not alter process ownership or the cancellation grace. Keep this decision Judged because it changes a test oracle within agreed behavior, requires real-path negative and corrected controls, preserves the original failure, and undergoes independent specification and quality review before completion.

## Decision

Qualify backend cleanup through the real stdin-lease supervisor and BackendProcess path. Retain direct-supervisor reap as a separate invariant. For descendants, check stable process identity, live state and harmless activity within the existing two-second cancellation grace, rather than requiring immediate disappearance of every proc entry. Demonstrate an intentionally surviving descendant fails, normal production group cleanup passes, and an intentionally unreaped zombie cannot be mistaken for continuing work. Keep the original unexplained test failure and its unknown process state in the review. Change the integration oracle only after these controls establish that it measures the agreed termination behavior; do not relax the grace or allow model continuation after cancellation.

## Realized by

- 7fd8d0b2734abf709bc1c6c42d3fdd9da7ac8805 Bind native Submit and Stop hooks to durable lifecycle owners

Real process controls and
topology-specific integration checks passed; specification and quality reviews
approved the bounded change. The runtime review retains the original failure's
unknown cause and the exact verification chronology.

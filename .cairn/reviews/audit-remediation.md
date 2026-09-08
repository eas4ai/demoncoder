# Audit remediation review

commitment: audit-remediation
examined:
  - CODE-007: final tool boundary, installed backend fixture, shared executor and mechanism runner.
findings:
  - open: AUD-006: The installed Codex fixture conflicts with subscription route validation and fails before its tool cycle; repair the mechanism without weakening the guard.
Status: in progress

## CODE-007 mechanism review

Read the current requirement/falsifier, coding-session declaration and runner,
the existing mechanism review, final executor admission, installed backend driver,
and Codex's current effective-route validation. The mechanism still checks real
effects and original results; its installed Codex positive case now has an invalid
configuration. The end-to-end audit ran that exact fixture and its result-cycle
variant on unchanged production inputs: both failed before a model request with
the explicit custom-provider rejection. OpenAI, Anthropic and Claude cases passed.
The deliberate denied direct-file and allowed Git-canary demonstrations in the
audit additionally show the cross-feature export defect addressed by AUD-001/002.

The corrected Codex positive demonstration is not yet possible with this fixture.
Do not treat the expected route rejection as a successful tool-routing proof. The
declared src/tests footprint includes the implementation and fixture dependency;
record the mismatch now and repair it as a separate implementation action. The
root commitment review remains open until corrected cases and cumulative checks
run against committed inputs.

## Protected export mechanism declaration

The audit-private-exports mechanism runs production workspace capture and review,
real Git worktree preparation and integration, the normal developer-access boundary,
and an installed terminal review using synthetic private canaries. Its footprint
includes shared Rust and Python dependencies and the specification. Installation
precedes the terminal case so a stale installed executable cannot pass this check.

The original implementation failed the new capture regression at the assertion that
retained contents exclude PRIVATE_RUNTIME_CANARY. The recorded pre-fix run had zero
passes and one failure; the same regression passed after the export-policy repair.
The Git case computes canary object IDs without writing them, then requires that
the objects do not exist after preparation or integration. This rules out retrieval
through parent Git tools; the developer-access suite independently tests denied
direct private-file reads. Public owned integration and private parent preservation
are checked too. Older snapshots are injected with private text and must not export
it. The installed case inspects the actual reviewer request and retained state,
requires public source and explicit exclusions, and completes verified acceptance.

This is a mechanism review, not the final independent commitment review.

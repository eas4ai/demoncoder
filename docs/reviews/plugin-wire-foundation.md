# Plugin wire validation review

Status: Approved for the bounded foundation

This review covers the reusable wire/profile validators and model-result
interpreter built after `f2e2c91`. It does not establish runner execution,
output effects, admission, activation or completion of the plugin commitment.

## Specification review

The independent reviewer examined the production validators, frozen profile,
model-result rules, typed callback boundary and fixtures. The reviewer found
one blocking coverage gap: the full-schema fixture loop checked 289 valid
payloads but used only a root Boolean as its invalid case. Deleting the required
string constraint for Codex Interrupt input's `cwd` would escape those tests.

The correction adds 1,042 probes across all 29 exact schema identities, including
649 frozen negatives. An independent obligation walker rejects missing or
duplicate probes. Baseline expectations run through the production profile before
controlled schema mutations. Of those mutations, 591 erasures admit unchanged
negative payloads and 443 reject unchanged positive payloads. Redundant constraints
and eight annotation-only formats are distinguished explicitly. The independent
specification reviewer verified the original `cwd` gap was closed, ran the focused
production test and approved the bounded foundation.

Claude graph tests already exercise unchanged payloads through the recorded
event/control roots while removing distinct fields and union branches. Deleting
an identical inherited declaration can be observationally equivalent: another
declaration still enforces the same constraint. Those occurrences use a
conflicting-type mutation instead of claiming that payload validation can
distinguish the duplicate's removal.

## Development checks

Before the full-schema coverage correction, the parent independently ran:

- `rtk cargo test --locked --test plugin_import --test plugin_wire`: 44 passed.
- `rtk cargo test --locked --lib plugins::`: 12 passed.
- `rtk cargo fmt --check`: passed.

The implementer's self-audit found a duplicate JSON visitor and replaced it
with the importer's existing `UniqueJson`. The importer change exposes that
helper within the plugin module; both importer and wire tests passed afterward.

After the correction, the parent independently ran:

- `rtk cargo test --locked`: 363 passed, 16 ignored across 32 suites in 98.94 seconds.
- `rtk cargo clippy --locked --all-targets -- -D warnings`: passed.
- `rtk cargo fmt --check` and `rtk git diff --check`: passed.

The ignored installed/backend qualification tests did not run in that regression
command. The earlier backend qualification remains separately recorded; this
change does not extend its coverage.

## Code-quality review and final correction

The independent quality review found one minor issue: callback ID and matcher
validation copied the borrowed string before checking its size. Both paths now
use allocation-free borrowed-string validation with the same saturating size
accounting. Boundary, oversized-input, parity and saturation tests cover the
change. The reviewer rechecked it and approved with no remaining findings.

After this final correction, the parent independently ran the plugin unit filter
(14 passed) and the unfiltered importer/wire integration suites (45 passed).
The full 363-test regression above preceded this small correction; it was not
rerun afterward. Locked all-target Clippy, formatting and whitespace checks passed
again after the correction.

## Static analysis and limits

Ripwire `--edit-check=CompatibilityProfile` exited 0 with no incompatible callers.
`--quality-delta` exited 2: it reported one gating overlap between `measure` and
its nested `walk` body, plus 80 new-symbol findings. The duplication rows overlap
enclosing and nested functions. Complexity and test-discovery findings were
reviewed as code, not treated as automatic passes. The independent quality review
found no further maintainability defect in this bounded work.

`--test-gate` exited 4 and named both passing integration suites plus 22 inferred
symbols in the adapters, main entry point and plugin modules. Several are inline
tests covered by the plugin unit run. Adapter and application regressions ran in
the full suite. These static results do not prove the new APIs are integrated:
runner execution, output effects, durable admission and public activation remain
required work in the implementation plan.

The final self-audit checked the boundary contracts, bounded validation, secret-safe
diagnostics, reuse, tests and review findings. No known defect remains in this
bounded foundation. These development checks and reviews are not Cairn evidence
and do not complete any whole plugin requirement.

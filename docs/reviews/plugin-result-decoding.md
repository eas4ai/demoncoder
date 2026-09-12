# Plugin result decoding review

## Scope

This stage validates command, HTTP, MCP and callback responses and returns typed,
untrusted proposals for the selected event and dialect. It does not execute those
proposals. Admission, confined runners, durable invocation records, activation,
original tool receipts and presentation integration remain required work under
the complete skills-plugins-hooks commitment.

The implementation is in `src/plugins/results.rs` and its `results/` modules.
It reuses the frozen profile, applicability lookup and bounded wire validators.
Source references and test scope are recorded in
`tests/fixtures/plugin-results/README.md`.

## Specification review

The independent specification review approved this bounded stage after checking
the corrected behavior against the pinned Codex event parsers. It separately ran
the focused Codex decoder tests: 12 passed.

The review attacked source validation and effect ordering, including these
corrected cases:

- Async Codex PreToolUse output can retain additional context while ignoring a
  control that would be rejected for a synchronous required gate. Schema checks
  still apply. Related async post-tool controls cannot escape as feedback or
  rewrites.
- Async Codex Stop and SubagentStop ignore plain output. Their synchronous forms
  still reject it, and malformed JSON remains a failure.
- Codex `continue:false` takes precedence over semantic decision errors for Stop,
  SubagentStop, UserPromptSubmit and PostToolUse. The post-tool branch separately
  decides whether context is valid and retains the source's stop feedback. This
  does not weaken PreToolUse or PermissionRequest rejection.

Additional inspected boundaries include explicit-null Codex optional values,
interactive Claude defer behavior, source worktree path parsing, dynamic watch
replacement, cancellation, shutdown and the distinction between a source with no
decision effect and a result that raises no objection. Paths remain proposals
that need filesystem admission; display and context fields do not gain authority.

## Executed development checks

- `rtk cargo test --test plugin_results --test plugin_import`: 83 passed,
  comprising 49 decoder tests and 34 importer tests.
- `rtk cargo clippy --all-targets -- -D warnings`: passed.
- `rtk cargo fmt --check`: passed.
- `rtk git diff --check`: passed.

Initial decoder cases failed against the API stub. The source corrections above
also had failing regressions before correction. Removing the production dynamic
watch replacement emission made an unchanged watch/empty-clear test fail;
restoring the implementation made it pass. The exact command, mutation and
historical restoration hash are in
`tests/fixtures/plugin-results/deletion-evidence.txt`.

These are development checks, not Cairn receipts. They establish result decoding
and proposed effects, not actual hook execution or complete plugin delivery.

## Quality review

Independent quality review approved the bounded decoder with no corrective
findings. It read all new modules, the shared validation path, fixtures and
deletion record. It did not rerun tests.

Ripwire edit checks reported no incompatible callers. Its test gate named
`tests/plugin_import.rs`, which passed; the decoder suite was run explicitly in
addition to that inferred selection.

The full quality-delta run exited 2 with 114 findings: 11 complexity, 74 dead-code,
5 duplication, 2 nesting, 1 reused-helper clone, 2 parameter-count and 19 verbosity
findings. Seven rows were marked gating. This is not a passing quality-delta
result. The reviewer assessed every category and recorded these dispositions:

- The new Native `validate` function is incorrectly compared with an older
  same-named function. The language-service `watches` method and Python `command`
  helper are unchanged. Small `.any(...)` predicates in unrelated domains do not
  justify a shared abstraction. These account for the seven gating rows.
- Transport and event branch complexity is explicit and cohesive. No beneficial
  decomposition was identified merely from the numeric findings.
- Dead-code rows include the new public API awaiting integration, trait methods,
  internal calls and executed test functions.
- Watch and worktree validators intentionally enforce different protocols.
  Test verbosity keeps actual inputs and expected effects visible.

No metrics were suppressed, no baseline was changed, and the nonzero tool result
was not relabeled as a pass. Both review approvals cover this stage only.

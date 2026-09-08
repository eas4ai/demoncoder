# Provider and agent Settings review

commitment: provider-agent-settings
commit: a864e430b5ead03df6649aff9bdfeef6c220d40d
examined:
  - START-003 startup mechanism against the current requirement and falsifier.
findings:
  - resolved: SET-008: Corrected the comma-separated Host paths declaration; specification lint passes.
Status: incomplete

## START-003 mechanism review

Read the startup declaration, requirement, startup settings_lock/save paths,
production startup driver and its onboarding helper. The declared src/tests
inputs cover the runtime and imported fixture helpers. The script runs the real
binary and derives per-requirement results from unittest assertions.

Ran `python3 tests/startup.py`: six tests passed across START-001 through
START-003. The safe negative cases use disposable directories: a symlinked home
settings directory and an explicitly configured shared parent both refuse startup
without changing their target/parent. The corrected case secures an owned 0775
default directory to 0700, preserves a canary, saves private settings and reaches
the session. Existing connections survive repair and new-workspace trust.
Foreign ownership is checked in source before permission repair; creating a
foreign-owned directory is not available to this unprivileged test process, so
this branch was inspected rather than demonstrated by changing host ownership.

No START-003 behavior mismatch found. The old receipt needs regeneration for
current Cairn execution-order and output-digest metadata. The UI tests currently
expect the old setup prompts and must be updated when SET changes that flow,
while retaining these filesystem and session assertions. The SET specification
has a separate formatting defect recorded above; this review changes no code.

## Connection baseline runner repair

The first baseline stopped on stale CONN-001 live records before executing the
selected authentication and ownership checks. Moved that independent historical
validator to the end of the runner. It still fails on stale evidence, and its
acceptance rules and records are unchanged. Earlier checks can now emit their
own results. No live provider requests are made by the historical validator.

The development run now reaches and passes CONN-002 through CONN-006, including
the selected authentication, ownership and capability checks. It exits one at
the unchanged historical live-record validator, which also requires committed
specification inputs. The subsequent Cairn run records the committed result.

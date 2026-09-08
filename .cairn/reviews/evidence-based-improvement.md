# Evidence-based improvement review

commitment: evidence-based-improvement
commit: 9eae183f2e603cd7dcad5bc03f4fc428dce2e3c7
examined:
  - Original receipt resolution, workspace identity, annotation and proposal attribution.
  - Durable authorization, ordinary task admission, correction outcome and lesson approval.
  - Actual parent and child request preparation, instruction scope and control routing.
  - Bounded storage, Unicode inspection, cancellation, restart and installed delivery.
findings:
  - resolved: LEARN-006: Instruction reads now require a regular file with one hard link, matching confined file-tool admission. Focused and all-connection production tests pass.
  - open: LEARN-008: Request::run saves catalog mutations before rendering their bounded inspection. A later formatting refusal can therefore report an error after changing state. Prepare the bounded response before committing the mutation.
  - open: LEARN-008: The installed release is still the pre-feature binary. Install the reviewed candidate and run the complete production learning workflow against that exact executable.
Status: incomplete

## What the review challenged

All selected LEARN, VERIFY, SUB, ORCH and REM requirements have current passing
Cairn evidence at this tree. The mechanisms also exercised the other requirements
owned by their shared runners. This review examined what those checks would miss;
no code was changed during the review.

Source references resolve checksummed original session records, constrain session
names and validate the workspace path plus captured directory identity. Stable
task history positions survive rechecks; child validation retains original
receipts in existing activity history. Annotations preserve their developer
author and do not change original results. Missing sources visibly block claims.

Correction authorization is saved under an exclusive catalog lock before one
ordinary task is created. The task stores the catalog and candidate linkage.
Reservations cannot replay after a crash or create a second task in another
session. Native task admission, original checks, review, correction allowances,
connection identity and explicit child integration remain the existing controls.
Outcome support checks the original failed command, the authorized task's full
selected check set and clear review at one snapshot. It does not infer benefit
from acceptance alone. Failed and abandoned outcomes remain retained.

Lessons require explicit proposal and approval, retained outcome support and
same-workspace selection. The bounded deterministic selector reaches actual
coding requests on all four connections and records its exact prepared context.
Control commands bypass preparation so quoted guidance cannot become part of a
developer-authored assignment. Inspection labels preparation separately from
proof that provider execution completed. Disabled and superseded lessons stop
future selection; previous opaque conversations cannot be erased.

Two source-level gaps remain. learning/context.rs validates is_file() before
reading instruction text; tools.rs also requires a single hard link in confined
mode. A regular hard-linked instruction can consequently bypass the file tool's
normal read admission. This was identified by comparing the actual metadata
checks; no external or private data was accessed to demonstrate it.

learning/control.rs calls save() before render(). The renderer may reject a
focused report over its 8 MiB limit, independently of the catalog's smaller
retained citation data. This ordering makes a failed response ambiguous after an
otherwise successful mutation. Prepare the bounded report first; test that a
formatting refusal leaves the previous catalog byte-for-byte unchanged.

The remaining delivery finding is concrete: the production LEARN-002 fixture
against /home/shawn/.cargo/bin/demoncoder failed at /improvements on the previous
release. Its SHA-256 was
39060995caa1d9a366341d6ccf9175aa3817d08644bbc8e087ceec26b189bfba.
The corrected debug binary passed, but that does not establish installed behavior.

## Mechanism and production-standard audit

The construction plan records the pre-feature failure, corrected cases and
negative examples. Cairn output and receipts were committed after each run.
The global production standard was checked for boundary validation, explicit
errors, idempotence, finite work and retention, responsive cancellation, preserved
contracts, maintainable placement, documentation and honest evidence claims.
The two implementation findings need revision before this audit can be complete.

Ripwire's unchanged-contract checks passed. Its source quality delta and test
mapping did not pass; their concrete size, trivial-constructor/lookup clone and
unmapped production-driver findings are assessed in the implementation plan.
No metric was hidden by changing a baseline or treating a zero-match result as
proof of coverage. Warning-free Clippy and executed Cargo/PTY mechanisms supply
separate evidence; they do not eliminate the findings above.

## Resolution: instruction read boundary

The instruction reader now checks the opened descriptor's link count before
reading text. A focused regression uses two harmless names inside one disposable
workspace: the multiply linked instruction is refused, and removing the second
name makes the ordinary file readable. The production LEARN-006 driver verifies
that the refusal starts no provider request, then passes the matching/unrelated,
scoped instruction and integration-gate cases on all four connections. The README
now states the same hard-link boundary. No outside or private data was accessed.

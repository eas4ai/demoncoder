# Advanced orchestration review

commitment: advanced-orchestration
commit: c7db3bcd8ebc537e20b97999f6312d10d67ece44
findings:
  - open: Orchestration implementation and production proof are incomplete.
Status: in progress

## Baseline demonstration

The first declared check was unverified because its script did not yet exist;
that receipt proves no runtime behavior. The production dependency test was then
run against the installed-development candidate and failed at actual CLI startup:
`unexpected argument --orchestrate found`. The child workflow cannot start on
the baseline. Other requirement cases explicitly fail until implemented; no
missing test is represented as passing evidence.

## Scheduling component review

Specification review independently passed all twelve tests and approved the
pure graph predicate at dbedbbde217cb4cf4b7c7cbaa53596c99cdb25d3. Quality review
found an omitted falsifier: none of those cases provides more eligible queued
assignments than remaining capacity. Removing the final capacity truncation
would therefore still pass. Add an unsorted overfull eligible queue and a fully
occupied limit, demonstrate the violating implementation fails, then re-review.
This finding is recorded before the test repair.

### Scheduling capacity repair

Removing ready.truncate(capacity) made the new test fail with IDs [10,70,90]
instead of [10]. Restoring it passed all thirteen cases. Both specification
and quality reviewers independently reran thirteen passing tests and approved
5f01da072eaf0d7dfaebb220c328bb2972a8bdad. The reviewed branch was merged with
no fast-forward; there is no remaining component finding.

## Production fixture specification review (in progress)

Before repairs, independent review found these gaps in the draft proof:
role peers accepted only assignment and round, without requiring source, checks,
original advisor findings or worker response; assertions inspected retained
receipts without comparing actual transport input. Recovery counted HTTP requests
but had no subscription peer request log, so silent backend replay could escape.
Recovery also did not challenge changed connection authority or retain an already
integrated prerequisite, and ample limits did not demonstrate spent admission after
restart. These are proof findings, not observed runtime defects. Production
execution remains pending the coupled manager implementation.

# Gate filesystem snapshots

This prerequisite supplies immutable filesystem evidence for PRUN-002. It does
not execute a gate, admit a tool, or complete the plugin commitment. Specification
and quality reviews are approved for this prerequisite.

## Implemented behavior

`GateWorkspace` pins the workspace root. Captures reuse the existing bounded,
descriptor-relative scanner. A gate's default read set includes admitted dirty,
untracked and generated files; verification output exclusions remain separate.
Explicit paths, globs and absent paths retain the membership evidence needed to
detect later changes. Required protected reads and unavailable captures hold.

The revision includes retained bytes, kinds, link targets, membership, ownership,
modes, applicable access/default ACLs, exposed access attributes and statx flags.
Symlink attribute queries use a pinned parent and no-follow leaf operations,
with identity checks around the queries. Query errors never become ACL absence.
Historical snapshots still decode, but missing access metadata requires a fresh
baseline before review evidence can be used.

## Development verification

The implementer ran these checks after the allocation correction:

- Five integration suites: `plugin_gate_snapshots`, `workflow_workspace`,
  `verification_workflow`, `subagent_state`, `subagent_worktrees`: 54 passed.
- `cargo test --lib workflow::workspace`: 9 passed.
- `cargo test --lib subagents::worktree`: 3 passed before the allocation correction.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` and
  `git diff --check`: passed.

Failing development tests exposed missing ownership capture, acceptance of an
old incomplete snapshot, missing metadata format version, unbounded aggregate
metadata, glob traversal through a symlink anchor, and unbounded overlapping-glob
match evidence. Each failed before its correction and passed afterward.

The behavioral fixtures use a real Git index, dirty and untracked files, real
POSIX access/default ACL changes, private-source canaries and deterministic
capture-time mutations. Ownership and statx participation also use independently
varied metadata through the production revision function. They do not establish
privileged ownership, security-label or immutable-flag mutation behavior.

Ripwire edit checks found no incompatible callers. Quality delta still reports
85 heuristic rows, including 15 gating rows; their disposition is below.
Test gate reports 88 broader transitive obligations. The focused tests above do
not satisfy the whole-commitment installed/live test obligations. No Cairn
receipt is claimed for these development runs.

## Specification review

The independent reviewer ran the gate snapshot integration suite: 11 passed.
It also exercised the production capture API with a real POSIX ACL in a separate
temporary harness. A 44-byte ACL retained a 65,536-byte allocation. Attribute
queries truncated buffer length but kept capacity, while the metadata allowance
counted lengths. Many small attributes could therefore exceed the intended
memory bound; the two capture scans also coexist.

The correction compacts shared query results before retention, including opaque
attribute values and canonical attribute-name lists. A production capture with
real ACLs first failed with 396 bytes of material reserving 589,824 bytes; after
the correction it retains exactly 396 bytes. The name-list regression first
failed with 21 bytes reserving 28 bytes and now retains exactly 21 bytes.

The reviewer found no additional concrete violations in selection, freshness,
descriptor confinement, metadata capture or historical decoding. Specification
re-review confirmed the allocation correction and independently ran all 12 gate
snapshot integration tests and nine workspace unit tests successfully. It approved
this prerequisite with no remaining concrete specification findings.

## Quality review

The independent quality reviewer approved the prerequisite with no actionable
findings. It independently passed 12 gate snapshot integration tests, nine
workspace unit tests and 42 affected workspace, verification and subagent
integration tests. It examined descriptor ownership, no-follow metadata queries,
failure behavior, allocations, selection and membership semantics, historical
decoding, diagnostics and existing workflow compatibility.

Its Ripwire disposition is:

- Directory capture is a cohesive scanner method; the remaining scanner paths
  distinguish file kinds and their security checks.
- Selection finalization branches distinguish required paths, absent paths,
  invalid ancestry and retained ancestors. They do not currently justify a split.
- The two private capture parameters express selection and pinned-root reuse
  without creating another scanner.
- Debug implementations preserve deliberate per-type redaction. Similar wrappers
  and unrelated normalized-token matches do not establish shared domain logic.
- The small root-opening primitive also exists in subagent worktree code. Keeping
  the existing ownership is acceptable here; a wider security-module extraction
  is unnecessary for this prerequisite.
- Public API, trait implementations and exercised tests explain the apparent
  unused-code rows. History churn revealed no additional concrete defect.

The broader 88 test obligations remain open for whole-commitment verification.

## Integration limits

The snapshot API is synchronous and cooperatively cancellable; it cannot
interrupt a stalled filesystem syscall. Its future worker owner must propagate
cancellation. Two scans do not provide an atomic transaction with outside
writers. The runtime must compare again before its guarded transition; stronger
external guarantees still need a supported transaction or a hold.

Global, mount, remote and LSM policy are not discovered by object metadata
queries. Relevant external preconditions belong to later admission integration.
Existing worktree materialization copies bytes and modes; preserving arbitrary
ownership, ACLs, labels and flags in a gate's inspection environment remains
unimplemented. Membership-only evidence must not become fabricated file content.

Missing required paths and newly created explicitly absent paths can make a
recapture fail. Both a changed revision and a recapture error mean the old gate
decision is no longer usable. No runner may replace unavailable snapshot bytes
with a read from the live workspace.

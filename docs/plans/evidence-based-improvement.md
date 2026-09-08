# Evidence-based improvement implementation

Implements the confirmed LEARN contract under the recorded private-workspace decision.

1. Persist cited observations, candidates, authorizations, outcomes and lessons in
   the existing checksummed Store, one catalog per canonical workspace identity.
2. Expose explicit commands and bounded saved-evidence inspection. Reserve a
   correction before creating its ordinary task; an interrupted reservation never
   replays. Acceptance remains separate from outcome support.
3. Prepare scoped repository instructions and approved applicable lesson evidence
   before actual parent and child coding requests, and retain that exact context.
4. Demonstrate failure and corrected behavior in focused tests and production PTY
   fixtures, run declared mechanisms on committed input, review, and verify install.

## Commands

`/learning-context [PAGE]` inspects retained prepared coding context.
`/observation ID [PAGE]` opens an observation and its original receipt.
`/improvements` refreshes failed-check observations in this session and shows the
workspace catalog. `/improvement ID [PAGE]` shows a candidate or observation.
`/improvement-note SOURCE TEXT` adds an attributed developer annotation to a cited
observation (sources are selected from retained task or agent checks).
`/improvement-propose OBS JSON` records objective, scope, benefit, behavioral_check
and risks. Discovery also offers a deterministic proposal with an explicitly
unknown cause. No proposal invokes a model.
`/improve ID` authorizes one ordinary task using its currently selected checks.
`/improvement-outcome ID` records what its actual correction checks and review
support, including failed, abandoned and insufficient outcomes.
`/lesson-propose ID JSON` records claim and applicability keywords from a supported
candidate outcome. `/lesson-enable ID`, `/lesson-disable ID` and
`/lesson-supersede OLD NEW` require explicit developer commands and retain history.
`/lesson ID [PAGE]` inspects a lesson and its original evidence.

## Limits and provenance

The private catalog holds at most 128 observations, 64 candidates, 64 lessons,
128 annotations and 128 outcomes per candidate, with an 8 MiB serialized ceiling.
Text fields are at most 4 KiB; behavioral commands retain the existing 8 KiB limit.
A source names a private session directly under the session root and a task or
agent check with its original receipt digest. Reads validate the Store envelope,
workspace identity and exact retained receipt; copied summaries never replace it.
Discovery reads one session, bounded by the existing 64 MiB session limit. Full
capacity refuses additional retention explicitly, without evicting evidence.
Storage uses nonblocking exclusive operation locks; source reads use the atomic
record without acquiring its live writer lock. All file access and formatting run
on blocking workers. Cancellation may leave a completed catalog write but never
starts coding effects; the next explicit command inspects the durable result.

## Instruction scope and matching

Load the selected coding workspace root AGENTS.md. For a child, also load
AGENTS.md in ancestors of its declared owned paths inside that worktree; no
recursive directory scan, external ancestors or import following. Nested rules
apply only to their directory subtree. Refuse symlinks, special files, oversized
instruction content and excessive path depth. At most 32 instruction files and
32 KiB total instruction text enter one request.

Runtime invariants take precedence over developer directions, then applicable
repository instructions, then quoted lesson evidence. These files never grant
tool permissions. Keywords contain 1–8 individual Unicode alphanumeric words;
matching is case-insensitive whole-word matching against the developer objective.
At most four applicable lessons and 32 KiB of lesson context enter one request;
report omitted matches. Record exact supplied content, identities, scope and
selection reason before invoking a provider. Keep at most 128 context receipts;
refuse new context when full. Disabling prevents future selection; it cannot erase
context already present in an opaque backend conversation.

Annotation source selectors also accept task-check:ID:ROUND:INDEX, task-review:ID,
agent-check:ID:GENERATION:INDEX, agent-review:ID and agent-role:ID:INDEX.
These resolve retained receipts in the current private session; indexes start at
zero. Outcomes are retained automatically after verification, review, acceptance
and abandonment, with /improvement-outcome available for an explicit refresh.
Source resolution caches at most eight sessions and 64 MiB per operation;
discovery examines at most 4,096 checks. Four blocking I/O slots and a ten-second
wait limit keep stalled storage from creating unbounded background work.

## Mechanism construction and failure demonstration

The production LEARN-002 driver was run against the installed pre-feature binary
with DEMONCODER_TEST_BINARY=/home/shawn/.cargo/bin/demoncoder. It created a real
failed check, then failed at /improvements: the old runtime treated that command
as an ordinary continuation and refused it with "use /correct to change work
after verification or review". No candidate could be inspected. This is a
failing mechanism observation, not a pass.

The corrected debug binary passes the same production driver. The complete
check-evidence-based-improvement.sh also passes LEARN-001 through LEARN-008,
formatter, warning-free Clippy, eight focused learning tests, sixteen private
Store tests and installed specification lint. These are development runs;
Cairn evidence must still be collected against the committed candidate.

The fixtures execute failing and corrected behavioral commands through the
production tool path. They reject absent evidence, accept an intentionally
ineffective task whose unrelated check passes while marking improvement
insufficient, preserve failed outcomes, and reject unsupported lesson activation.
They inspect actual HTTP requests and both external-backend protocol inputs,
including fresh sessions, matching and unrelated work, nested instruction scope,
disabled guidance and a ready child still awaiting explicit integration.

Recovery tests observe atomic authorization publication using inotify, stop and
kill the actual process, then prove restart does not create another correction.
A second kill during the admitted provider call preserves linkage and consumed
allocation. Limits cases use bracketed terminal paste for large Unicode text,
multiple inspection pages, a narrow terminal, a busy catalog, full candidate
retention, four selected lessons plus an explicit omitted match, and cancellation
while inspecting an active correction. A focused test blocks a storage worker
and proves cancellation returns without waiting for it to finish.

During implementation, receipt hashing initially compared struct field order
with parsed JSON object order. Five focused source tests failed; canonical JSON
normalization corrected the mismatch. A read-through also found that generic
context preparation could append evidence to delegation command arguments. The
workflow now skips context preparation for control commands, and the production
all-connection test asserts the child's exact developer-authored objective.
Prior child validation is retained through the existing activity-history format
before rechecks replace current receipts. Context validation occurs before
launching a new child adapter.

## Maintainability and inspection limits

Ripwire's Store and WorkflowSession edit checks report unchanged public
contracts and no statically identified incompatible callers. The source test
gate exits 4: it cannot map the external Python-to-production-binary coverage;
Cargo and the declared PTY mechanisms establish executed coverage instead.

The source quality delta exits 2 and is not reported as a pass. Its remaining
size/complexity findings concern the explicit task admission branch, bounded
source validation and labeled inspection rendering. Learning controls and outcome
handling were separated into workflow/improvements.rs to keep the main session
flow readable. The clone findings compare small typed record lookups and struct
constructors across unrelated layers; replacing those with a shared abstraction
would couple catalog persistence to model sessions or subagent integration.
Their bodies were inspected and remain intentionally local. Report methods grew
to expose actual context receipts beside original task/agent evidence. New-symbol
'dead code' rows include the eight tests just executed and serialized record
types used by these production paths; the static receiver/test mapping does not
establish that they are unused. These limits do not substitute for reviewing the
actual implementation after the committed mechanisms pass.

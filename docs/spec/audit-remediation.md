# End-to-end audit remediation

Status: Agreed 2026-09-08
Prefix: AUD

The developer selected remediation of the six numbered findings in the
2026-09-08 end-to-end code audit. Preserve the existing tool permissions,
explicit acceptance, bounded allocations, and durable evidence contracts.
The additional roadmap suggestions in that audit are not new feature commitments.

[AUD-001] Workspace capture and task review MUST exclude protected credential
and runtime files from newly retained source contents and outgoing reviewer
requests, including unchanged private files. The same export policy MUST apply
when formatting an older snapshot. Exclusions MUST be visible as scope, never
represented as inspected source. Existing private-path checks MUST remain effective.
Falsifier: A synthetic `.demoncoder` or `.env` credential appears in new captured
contents or an actual reviewer request for unrelated code; a required source
change is silently omitted from the declared review scope.
Mechanism: Exercise production capture and the installed task/review path with
private canaries, public source, and older snapshot input.

[AUD-002] Delegation MUST NOT copy protected workspace contents into children
or write them into the shared Git object database. Runtime-generated snapshots
MUST NOT provide a way around the ordinary private-file tool restriction.
Falsifier: Preparing or integrating a child puts a synthetic ignored credential
in a Git object or child file; ordinary parent tools recover it from a newly
generated snapshot despite a denied direct read.
Mechanism: Prepare and integrate disposable Git worktrees and inspect retained
trees, child contents, and parent tool reads using synthetic private canaries.

[AUD-003] Developers MUST be able to declare generated-output paths before
starting work so normal builds can complete verification without capturing
their outputs. The declaration MUST remain explicit, bounded and retained with
the snapshot. Changing it MUST invalidate affected evidence.
Changing it MUST NOT silently change recovery authority. All other source/input changes MUST still invalidate
verification. Excluded outputs MUST NOT be exported as delegated source changes.
Falsifier: A declared 9 MiB build artifact prevents task verification; a check
that only rewrites declared outputs cannot pass; undeclared source changes pass;
or changing the declared scope reuses prior acceptance/recovery evidence.
Mechanism: Run production checks that write large and changing outputs, change
real inputs, resume sessions, and integrate child results under explicit scopes.

[AUD-004] Review MUST support a small task in a repository whose total source
exceeds one request's evidence limit. Every changed in-scope source MUST remain
complete. Selected supporting context and omitted-source identities MUST be
explicit. Material that cannot fit MUST block before a misleading clear review.
The configured review scope MUST be retained in the actual review evidence.
Falsifier: An unrelated 1.2 MB source baseline blocks a bounded task despite a
valid bounded review scope; a changed source is silently cut out; or omitted
context is described as reviewed in the request.
Mechanism: Inspect actual bounded reviewer requests for a larger repository,
changed files, selected context, and an oversized changed-file refusal.

[AUD-005] Anthropic text, thinking and signature accumulation MUST append
fragments without repeatedly copying the complete prior block. Existing byte
limits, exact UTF-8 text, cancellation and incomplete-response behavior MUST hold.
Falsifier: Each append reconstructs the accumulated prefix; split Unicode differs
from the complete text; or the block limit is bypassed or truncation executes tools.
Mechanism: Check the production accumulator's allocation/reuse and content behavior
with fragmented streams, and run the existing stream/output-limit regressions.

[AUD-006] The installed Codex tool-routing and result-delivery mechanisms MUST
exercise the real supported backend successfully without weakening subscription
validation or hiding conflicting effective configuration. A cumulative regression
check MUST cover the completed product after shared-boundary changes.
Falsifier: Installed routing still stops at the fixture's rejected custom provider;
the test passes without a real installed backend/tool cycle; custom subscription
routes become accepted; or the aggregate gate omits the corrected installed cases.
Mechanism: Run installed tool and correction/result cycles with disposable local
model traffic and synthetic credentials, negative route checks, and the aggregate
completed-product suite. Paid live availability remains a separate check.

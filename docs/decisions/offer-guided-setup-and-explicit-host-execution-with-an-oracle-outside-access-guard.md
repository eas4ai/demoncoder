# Offer guided setup and explicit host execution with an Oracle outside-access guard

Level: Judged
Decided by: Codex
Rests on: The developer requested first-start provider/model/trust onboarding, explicitly defined --yolo as no sandbox or routine permission prompts, requested Oracle checks outside the project, delegated the blocking policy, and identified normal /tmp use
Would be wrong if: Setup hides the active access mode, a flagged host command still runs in a sandbox, outside access bypasses or outlives its Oracle decision, a failed Oracle permits an effect, or normal session scratch work becomes unusable

## Decision

Add guided first-start setup and an explicit setup command for saved connections, models, effort, project trust, and the Oracle assignment. Preserve private home settings and existing backend login directories. --yolo is an explicit per-invocation host-execution choice with no sandbox or routine tool prompts; it retains a blocking Oracle review for outside-project access. A separate no-tools model session judges final requests after hooks and fails closed on denial, malformed output, failure, timeout, or a tool request from the Oracle. Resolved project files and session-owned temporary scratch files do not need that review; unrestricted shell commands are screened because their effects can be indirect. Keep original tool results and show review reasons. Unknown external plugins/hooks remain disabled; no dynamic loader is claimed. Verify with harmless disposable files and verdict-only destructive examples, never by moving or deleting home/system data.

## Realized by

- 7d46564d7555f7eb976aa2943cabe7aff0d0c344 Guide first setup and guard explicit host access with the Oracle

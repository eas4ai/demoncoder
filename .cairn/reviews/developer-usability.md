# Developer usability implementation review

commitment: developer-usability
commit: e8897448fbb114483f0ab035867c0ed7dc26af19
findings:
  - open: USABLE-004: usage regression driver reads the new help row rather than the visible usage row
Reviewer: Codex
Date: 2026-09-06
Status: findings recorded

The four USABLE requirements passed the declared production mechanism. I examined
transcript retention, byte-offset wrapping, visible-row iteration, anchor expiry,
Unicode, empty deltas, private credential aliases, outside-write mounts, cache
fallback, and command cancellation. The bounded display and access canaries pass.

One gap needs correction: DeveloperAccess::command traverses the workspace and
private stores synchronously inside ToolExecutor::bash before its first await.
A large tree can occupy the session task and defer cancellation. The existing
cancellation fixtures use small trees and do not prove cancellation while command
preparation is active. Move inspection off the async runtime and stop abandoned
inspection cooperatively; no task command may start after cancellation.

No production code was changed during this review. Aggregate regressions,
read-only assessment of the selected repository, installation and publication
remain to be completed after this correction.

## Follow-up review

The cancellation finding is resolved by e889744 and passing regression evidence;
the deliberate missing-stop-signal variant fails. The selected real-repository
assessment also passed its read/tool admission checks and retained the service
results and limits in docs/reviews/self-repository-assessment.md.

The existing usage driver now fails because it reads only the last terminal row,
which became scrolling help. Actual usage is on the preceding row. Update this
fixture to inspect the current usage row and retain its exact-value and reset
assertions; do not accept historical text or weaken unknown/zero checks.

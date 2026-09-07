# Acknowledge bounded prompt admission without blocking session controls

Level: Judged
Decided by: Codex
Rests on: REL-002; existing bounded terminal input, correction and two-second cancellation contracts
Would be wrong if: A rejected draft disappears, held queues block input or owned-work cancellation, correction storage grows without a bound, or completed tool receipts are lost

## Decision

Keep the existing Prompt command and Session turn signature. Add an acknowledged submission command for the production terminal. Retain one submitted draft until admission; keep edits responsive and clear only the unchanged draft after acceptance. Reject full correction queues synchronously with a reason, retaining editor text. Bound native corrections at the existing external limit of 32. Correction notices may omit their live copy when the event queue is full, while retaining the event log; model text and original tool receipts retain awaited delivery. Keep one pending cancellation send in the terminal event loop, and include shutdown submission in the existing bounded cleanup wait. Existing direct Prompt callers remain supported; independently implemented exhaustive Command matches must handle the added acknowledged variant. Preserve the registry interface signature and verify the independent registration driver as well as all four built-in paths.

Subscription correction permits remain held after the channel is drained and are released only when the corrections are applied or dropped. Lifecycle event publication also polls controls; a full event queue rejects additional submissions with an admission reason, cancellation can prevent a not-yet-started turn, and shutdown closes the owner without waiting for the terminal to drain. Closing the UI receiver is normal shutdown; retained-log failures remain errors.

## Realized by

(none yet: recorded, not built)

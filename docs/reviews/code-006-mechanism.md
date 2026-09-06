# CODE-006 mechanism review

Reviewed 2026-09-06 against the agreed continuity requirement and falsifier.

The driver opens the actual terminal in a temporary Git repository for each
connection. A controlled model chooses a unique function name that the
developer's prompt does not contain. The first turn creates that function.
One case completes normally with a unique assistant context marker. The
other queues a held Bash operation and another write after the completed
creation, then cancels while Bash is active.

The second prompt asks to extend the preceding function by seven without
restating its name or contents. Native peers inspect the actual submitted
history: the completed creation result and unique function must remain;
normal completion must also retain the assistant's context marker.
Cancelled calls must have explicit unsuccessful, unknown-exit results.
Every call needs exactly one corresponding result, and Anthropic's tool
results must occupy the immediate user message after their tool uses.

External peers own their fixture context and save completed tool results.
After cancellation, they require thread/resume with the original Codex
thread ID or --resume with the original Claude session ID. They refuse a
fresh session. The production adapters retain and validate those identities.
The fields were checked against the installed Codex-generated
ThreadResumeParams schema and the installed Claude CLI help.

The second turn uses production read, edit, and Bash tools to inspect the
existing file, extend its value, and execute a Python assertion. The driver
compares the final file, checks the previously queued write never ran, and
compares the first turn's actual creation result across the retained event
and provider/backend input. Claude's control request ID is mapped to its
runtime call ID without changing the original result used in that comparison.

Failure demonstrations executed:

- The preceding executable failed native cancellation cases because tool
  calls lacked results. Codex cancellation failed because the adapter
  discarded its original thread ID. Normal native and Codex cases passed.
- The corrected implementation passed both cases on all four connections.
- `python3 tests/continuation.py --fault-forget-context` removes preceding
  native messages or clears the backend peer's preceding results before
  the second prompt. The ordinary context checks and second-turn checkpoints
  reject this loss. All eight cases fail rather than reporting completion.

These cases prove local continuation through production adapters and tools.
They do not prove live subscription resumption, crash recovery, arbitrary
context retention under model-window exhaustion, or provider billing after
an interrupted response. Live transport remains required separately.
The cancellation case preserves a result delivered before the next held
tool; presentation and event-publication edge cases remain CODE-008 work.

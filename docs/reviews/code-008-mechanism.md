# CODE-008 mechanism review

Reviewed 2026-09-06 against the actual-result requirement and falsifier.

`tests/installed_backends.py --results` drives the production terminal and
each of the four adapters. Codex and Claude use their installed binaries
with synthetic authentication and local model responses, as in CODE-007.
The model creates a Python module with the wrong value and requests a real
repository assertion through isolated Bash. The assertion must fail with
exit code 1 and a unique error marker. The model peer waits until that error
appears in terminal output before requesting an edit and the same check.
The corrected check must exit 0 and print its unique passing marker.

The driver compares the final file, original retained results, and results
received by the model. All four operations must retain their identity,
tool name, success, output, and exit code. Codex and native call IDs are
compared directly; Claude's control request ID remains attached to its
receipt while its model tool-use ID selects the corresponding MCP response.
The results must have four distinct runtime IDs and match retained events
in order. Failed and successful checks cannot trade places.

The shared Rust executor test adds a presentation hook that claims every
check passed. The actual failing result still contains exit code 1 and the
assertion error. The hook's text is a separate presentation event. The
terminal labels that event as presentation for its original call ID.

Two native-loop tests cover interruption after completion. One fills the
event channel so a completed write waits for UI delivery, then cancels the
turn. The next model context must retain the successful write exactly once,
while the queued write stays unexecuted and uncertain. The event log and
model receipt must agree. The other fails a presentation hook after a real
failed repository check, then continues with a correction and passing check.
The model must receive the original failure, not an unknown-result notice.

Failure demonstrations executed:

- Temporarily omitting the executor's completion receipt made both native
  interruption tests fail. Cancellation reported the completed write as
  unknown; presentation failure lost the actual exit code. Restoring the
  receipt passed both tests.
- `--results --fault-rewrite-result` changes the model fixture's received
  failed result to success with exit code 0. All four ordinary result checks
  reject the lie and report CODE-008 failure. Actual tool execution and its
  original event evidence remain untouched by this controlled fault.
- The unmodified four-connection result cycle passes.

This verifies local result preservation and correspondence. It does not
establish arbitrary task correctness, crash recovery, live authentication,
or live provider behavior. A completed turn remains distinct from task
acceptance. The connection mechanism still requires live two-turn evidence.

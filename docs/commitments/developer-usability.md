# Developer usability

Status: Agreed 2026-09-06
Requirements: USABLE-001, USABLE-002, USABLE-003, USABLE-004

## Deliverable

A scrollable terminal with bounded retained display history and visible-row
rendering, plus a useful default coding environment. Reading documentation,
searching, inspecting Git and Cairn, using installed tools, and reaching network
services must work. Ordinary build artifacts must not disable Bash.

The developer selected this commitment through the scrollable-chat bug report
and the explicit direction to repair permissions that made repository assessment
unusable. The earlier first-session access wording is superseded for normal
read and network access by docs/spec/developer-usability.md. Writes outside the
workspace remain controlled; host access is still explicit.

## Verification

Drive actual terminal controls and production tools with disposable fixtures.
Check memory/entry bounds and rendering work directly, and demonstrate that the
old behavior fails the meaningful cases. Use harmless outside canaries for
read/write and credential boundaries. Run the aggregate Rust and existing
terminal checks, record committed Cairn evidence, and review what the checks
miss. Recheck the selected real repository without making task changes through
the application. Install and publish the verified correction.

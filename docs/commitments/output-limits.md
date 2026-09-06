# Model output limit correction

Status: Agreed 2026-09-06
Requirements: OUTPUT-001, OUTPUT-002, OUTPUT-003

## Deliverable

Remove the obsolete fixed native-model output ceiling identified by the developer.
Use model capabilities or explicit settings, prevent truncated output from being
accepted as completed work, and preserve ordinary provider and session behavior.

## Verification

Demonstrate the old failures and corrected behavior through production adapters
and sessions using temporary repositories and controlled endpoints. Check actual
requests, no tool effects after truncation, reported usage and a usable next
prompt. Run applicable provider, configuration, terminal and Rust regressions.
Record committed Cairn evidence, review, install and publish the correction.

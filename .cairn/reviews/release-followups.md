# Release follow-up review

commitment: release-followups
commit: 6df64b7faac5106263ab13cf49256f09c7683403
findings:
  - open: OUTPUT-001: The Connections and authentication manual paragraph incorrectly describes a fixed 4096-token Anthropic limit and no CLI override.
Status: in progress

## OUTPUT-002 mechanism review

Read the current requirement and falsifier, output-limits declaration and runner,
all Rust output-limit tests, actual terminal output tests and the prior mechanism
review. The declaration includes runtime, tests, scripts and manual inputs. The
shell stops before result lines if a constituent fails.

Executed the 12 existing Rust cases: all pass. Safe truncated Anthropic text and
valid-looking tool responses for both output/context limits fail before tool
effects, retain usage, and allow a subsequent complete response without pending
tool records. The OpenAI incomplete-response case likewise preserves usage and
prevents its write. Complete responses and explicit/discovered limits pass.
Executed all three actual-terminal output tests: all pass, including visible
truncation, usage and a working next prompt. These demonstrate refusal of the
violating input and acceptance of corrected complete input without code changes.
No mismatch was found between OUTPUT-002 and its mechanism. The separate known
manual error remains an implementation action, not a mechanism edit during review.

## Live Oracle availability

The real saved Claude/fable/high Oracle check failed before producing its verdict
pair. A bounded no-tools CLI diagnostic confirmed an exhausted Fable usage limit,
while auth status confirmed the existing logged-in first-party subscription.
The observed failure is retained under .cairn/evidence/live-oracle/. No model,
authentication or billing setting was changed. The CODE-010 mechanism records the
required live verdict evidence as unverified; CODE-001 through CODE-009 pass.
This account-side limitation cannot be repaired by changing the check footprint.
The four live connection sessions have not yet been rerun in this commitment.
The manual correction and queued decision closeout remain pending.

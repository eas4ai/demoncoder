# Live verification and release follow-ups

Status: Agreed 2026-09-08
Requirements: OUTPUT-001, OUTPUT-002, OUTPUT-003, REL-001, CODE-010, CONN-001

The developer confirmed proceeding with the concrete follow-ups after completion
of audit-remediation: refresh live provider and Oracle verification, correct the
older output-limit paragraph, and review the queued host Unix-socket decision.

Use the existing saved connections and authentication through their real default
transports. Run the existing bounded two-turn live coding check for each initial
connection and the existing verdict-only Oracle pair. Preserve original evidence,
account failures and unverified outcomes. Do not substitute accounts, endpoints
or authentication methods to produce a pass. Keep temporary test storage beneath
/home/shawn/workspace2/scratchpads and remove completed artifacts.

Correct the Connections and authentication paragraph to match the existing
OUTPUT requirements and actual CLI/configuration behavior. Review the existing
socket policy against REL-001 and report its practical effect: confined Bash
cannot use local Unix sockets, while TCP/UDP and explicit host mode remain.
Record the developer-confirmed decision review without changing runtime policy.

Done when the named requirements have current passing evidence, the manual
matches the verified output-limit behavior, the queued decision review is
recorded, and the final commitment review has no unresolved findings. New
feature backlog items require their own specified commitment.

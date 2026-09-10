# Bind HTTP hook requests to explicit host endpoint and credential authority

Level: Judged
Decided by: agent
Rests on: PLUG-003,HOOK-004,HOOK-008,PRUN-001,PRUN-002,PCOMP-002
Would be wrong if: A hook request reaches an unadmitted destination, discloses a bound credential through redirects or receipts, escapes its owning deadline, or replays an uncertain remote effect.

## Decision

Implement the HTTP runner through the shared admitted PreToolUse dispatcher. Importing a package grants no network or credential access: the host binds the exact endpoint, allowed headers and credential values before registration, and those facts join immutable invocation identity. Send the bounded source event as a POST body. Disable automatic redirects and ambient proxies; a redirect holds this prerequisite rather than receiving credentials at another destination. Use finite connection and complete exchange bounds under the owning allowance, own cancellation, and retain failed or interrupted transmissions as uncertain rather than retrying them. Decode only bounded successful responses under the source event profile. Retain secret-safe failure categories, and reject credential reflections before ordinary receipts. Revalidation uses a separately admitted read-only endpoint, never an automatic replay of an effecting POST. Retain an opaque host endpoint identity in receipts; bind that identity and both concrete endpoint configurations in the configuration digest so private URLs and query values do not enter ordinary records. Public activation, environment-to-credential configuration, other lifecycle events and MCP service integration follow in this same complete commitment.

## Realized by

- 20d160ac1065d934c758f86c1f30ab7bda7ac4f7 Add admitted HTTP hook runner with bounded secret-safe exchanges

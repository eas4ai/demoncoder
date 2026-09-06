# Separate coding sessions from provider connections

Level: Judged
Decided by: Codex
Rests on: The developer agreed to Cairn and requested a small pi-style loop, four initial provider and authentication connections, and an extensible provider list on 2026-09-06.
Would be wrong if: The split introduces competing owners for one session or permits a connection to claim support without passing the shared session behavior checks.

## Decision

Specify the first commitment in two domains: CODE for the observable coding session and CONN for provider selection, authentication, capabilities, and extensibility. Native model adapters feed the small Rust loop. Codex app-server and Claude headless adapters represent external agent backends with a declared loop owner. The four requested connections remain initial scope. These are draft requirements for review; this record authorizes only their documentation and does not establish runtime compatibility.

## Realized by

- 6eb42369e061e6a2adbf827350f9c51e2c6b591a Draft the first DemonCoder coding session under Cairn

This commit realizes the specification partition. The application and
adapter implementations remain pending.

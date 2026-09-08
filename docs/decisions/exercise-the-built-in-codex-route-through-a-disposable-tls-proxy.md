# Exercise the built-in Codex route through a disposable TLS proxy

Level: Judged
Decided by: Codex
Rests on: AUD-006 CODE-007 CODE-008 CONN-003
Would be wrong if: A custom provider route is accepted, effective configuration is falsified, or the fixture reaches a real provider or uses production credentials

## Decision

Use the actual installed Codex with its built-in provider and synthetic login. Permit only the exact canonical built-in ChatGPT base URL when config/read materializes that default; continue rejecting alternate endpoints and custom provider definitions. Route fixture network traffic through a disposable local TLS proxy with a fixture CA, capture actual tool and result cycles, and keep negative authentication/route tests. Never rewrite effective configuration responses or add a production test bypass.

## Realized by

- 76cc8c3b17e51488ef47309f91db31297efb5f9b Protect private source exports and restore installed Codex routing checks

`src/settings/probe.rs` accepts only the materialized canonical default while retaining origin, provider and alternate-URL rejection. `tests/codex_https_fixture.py` serves a disposable loopback TLS proxy with synthetic certificates. `tests/installed_backends.py` uses the real installed backend through that proxy and checks complete tool/result cycles and the observed CONNECT destination.

# Consider shared ownership helpers for future runner fixtures

Surfaced from: PLUG-002
Captured: 2026-09-10T20:27:46.596Z

Independent MCP quality review found duplicated workspace and loopback-peer ownership scaffolding across command, HTTP and MCP integration tests. A later runner may justify small shared ownership utilities; keep protocol-specific responses and assertions local. Do not introduce a generalized test protocol framework or refactor only to suppress static duplication. This is a nonblocking maintenance recommendation.

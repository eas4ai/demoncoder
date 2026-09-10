# Retain compiled admitted MCP validators if call cost warrants it

Surfaced from: PLUG-003
Captured: 2026-09-10T20:27:46.571Z

Independent bounded MCP quality review found repeated offline schema compilation at admission, preparation and calls. Bounds and tests establish finite behavior; no performance failure was measured. When the service layer next changes, measure the cost and consider retaining compiled validators alongside immutable admitted tool metadata without weakening offline checks or identity. This is a nonblocking maintenance recommendation, not current implementation scope.

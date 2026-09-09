# Managed language services

Status: Agreed 2026-09-09
Requirements: LSP-001, LSP-002, LSP-003, LSP-004, LSP-005, LSP-006

The developer confirmed the first slice in
[the reference feature sequence](../proposals/reference-feature-sequence.md).
Implement the six requirements in docs/spec/managed-language-services.md.

Done when production-path protocol, confinement and cancellation cases pass,
installed Rust and TypeScript smoke cases pass, all four adapters exercise
the shared language tools, applicable regression gates pass, documentation
is current and the commitment review has no unresolved findings. Record actual
transports; controlled provider cases are not live authentication evidence.

Rename and code-action application are outside this commitment. Later
context and history features retain the proposed sequence but are not selected.

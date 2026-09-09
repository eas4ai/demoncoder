# Skills, plugins and lifecycle hooks

Status: Draft 2026-09-09
Requirements: EXT-001, EXT-002, EXT-003, EXT-004, EXT-005, EXT-006, EXT-007, EXT-008, EXT-009, HOOK-001, HOOK-002, HOOK-003, HOOK-004, HOOK-005, HOOK-006, HOOK-007, HOOK-008, HOOK-009, HOOK-010, HOOK-011, PLUG-001, PLUG-002, PLUG-003, PLUG-004, PLUG-005, PLUG-006, PLUG-007, PLUG-008, PLUG-009, PLUG-010, PLUG-011

Proposed complete delivery of [the extension design](../proposals/skills-plugins-hooks.md).
The roadmap does not select this draft for implementation yet.

Deliver Claude Code, Codex and portable package imports; standalone and packaged
skills; all five hook types and the full lifecycle matrix; agents and workflows;
MCP/LSP services; configured connectors; monitors and channels; themes and output
styles; package scopes, author controls, marketplaces, dependencies and updates;
and the optional Best Practices and Cairn workflow packages.

Done when every named requirement has current passing production-path evidence,
all four connections pass the shared behavior cases, representative packages from
both ecosystems pass component and workflow smoke cases, failure/recovery cases
demonstrate their actual effects, documentation is current, and the commitment
review has no unresolved findings. Record exact package revisions and backend
transports. Controlled tests do not constitute live-provider evidence.

Implementation may proceed in dependency order. No intermediate slice, disabled
component, placeholder, parse-only importer or unsupported label satisfies Done.
Actual provider access or private API restrictions require a concrete developer
scope decision if they cannot be resolved. They do not authorize silent deferral.

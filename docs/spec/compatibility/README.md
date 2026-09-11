# Reproducing the plugin compatibility inventory

Status: Agreed 2026-09-09

Profile v1 revision 3 is the normative, frozen compatibility baseline. These
tools verify the documentary inventory; they do not execute plugins or establish
runtime compatibility. All feature families and real package fixtures remain
required by PCOMP-004.

## Inputs

Use TypeScript 5.9.2 and Ajv 8.17.1. They are authoring/check tools, not new
DemonCoder runtime dependencies. The commands accept explicit package paths and
perform no installation or network access.

Obtain Claude SDK 0.3.267 from the `source_revisions.claude_sdk.url` tarball and
verify its recorded SHA256 before extracting `sdk.d.ts`. The generator also
checks the extracted file's SHA256:
`5d243d3837ac5cf211470f11bcb51eb77cb5a0c8554074935d5c2c4c159d0cf3`.
Use the Codex commit in `source_revisions.codex.revision`; each input schema is
individually hash-checked. Download the two recorded portable schema URLs as
`plugin-portable-schema.json` and `mcp-portable-schema.json` in one directory;
their recorded hashes are checked too.

## Commands

From the repository root, substitute the actual source/package locations:

```sh
node docs/spec/compatibility/build-claude-wire.mjs /path/to/typescript/lib/typescript.js /path/to/sdk.d.ts --check
node docs/spec/compatibility/check-claude-wire.mjs /path/to/typescript/lib/typescript.js /path/to/sdk.d.ts
node docs/spec/compatibility/build-source-schemas.mjs /path/to/codex /path/to/portable-schemas --check
node docs/spec/compatibility/check-hook-semantics.mjs /path/to/ajv
```

The two builders accept `--write` only when intentionally revising the agreed
inventory. A source change requires new source identities and review, not simply
regenerating away a failed check. No unknown TypeScript construct or unresolved
reference is silently flattened into an untyped field.

## What is checked

- Regeneration exactly matches the 77-type Claude graph, its 33 event names and
  316 scoped field declarations. Inherited inputs and union alternatives remain
  references/branches. Four named external types have explicit interpretations.
- Ten positive/negative assignability probes compile both against the pinned SDK
  and against independently reconstructed inventory types. Removing a nested
  field or permission branch changes the results and is detected.
- Codex and portable schemas retain full source constraints. Selected Codex
  configuration definitions include their complete local reference closure.
- All 510 dialect/event/handler cells have explicit applicability. Removing a
  cell fails validation. Every model-result condition resolves to a named outcome;
  fourteen concrete outcome cases and four response schemas are checked.

Field pointers identify declaration sites; runtime conformance must exercise
their occurrences through every reachable event and branch. Callback wrappers
are not JSON fields. Foreign general SDK settings APIs outside the root closure
are not imported features; all plugin settings in `field_families` still require
their declared behavior. A flat field index never overrides a full schema.

The application must additionally prove parsing, actual effects, blocking,
recovery and deliberate policy differences through its production paths on all
four connections. These source/type/table checks are not substitutes for those
tests, installed package smoke cases or real backend/connector qualification.

Revision 3 corrects only Codex SessionEnd MCP applicability, as recorded in
[the compatibility contract](../plugin-compatibility.md#profile-v1-revision-3-correction).

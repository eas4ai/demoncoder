# Require exact ordinary callback acknowledgments from the private Codex snapshot

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-004,HOOK-005,HOOK-008,HOOK-009,PRUN-001,PCOMP-002,PCOMP-003
Would be wrong if: An unrelated response releases a model boundary, source failure becomes correction, ambient code executes, or the ordinary patch changes the qualified compaction contract.

## Decision

Use CODEX_DEMONCODER_ORDINARY_RELAY for a separate demoncoder-ordinary-v1 startup requirement containing the absolute declaration source_path, lowercase source_sha256, and exact submit_command and stop_command. Freeze a strict declaration with exactly synchronous command handlers for UserPromptSubmit and ordinary Stop; reject duplicate or unknown fields, wrong types, nonregular files, bad hashes and missing requested declarations before startup. Each actual source command input retains its native fields and adds a separate demonCoderOrdinary acknowledgment envelope: protocol, a fresh transport-only delivery_id, hook_event_name, and the actual source session_id and turn_id already present in that event request. The host response must echo that envelope exactly and explicitly state continue as a boolean; event-specific context and Stop correction fields retain their native meaning. A launch, transport, timeout, nonzero exit, malformed output or absent/mismatched acknowledgment holds continuation and cannot become a correction. The delivery ID never substitutes for source run identity or host operation identity. Preserve synthetic memory/subagent Stop distinctions and the separate compaction challenge contract. Expose --demoncoder-ordinary-capability with protocol, source_version 0.153.4 and patch_version 1. Keep the snapshot immutable across refresh and declaration/config mutation, suppress ambient executable discovery when private hooks are selected, and preserve authentication and non-hook managed policy. Apply the new ordinary patch after the unchanged compaction patch, with separate provenance recording the exact intermediate and final trees; verify both stages and unchanged external dependencies. Actual binary fault, isolation, compaction and model-hook qualification must pass before this boundary is called supported.

## Realized by

(none yet: recorded, not built)

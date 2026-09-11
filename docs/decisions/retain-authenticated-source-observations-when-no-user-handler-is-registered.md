# Retain authenticated source observations when no user handler is registered

Level: Judged
Decided by: agent
Rests on: HOOK-001,HOOK-005,HOOK-008,HOOK-009,PRUN-001
Would be wrong if: An unregistered native event gains a handler or authority, a callback releases continuation before owner validation, or a valid partial source registration cannot perform its admitted correction.

## Decision

The private Codex boundary installs both Submit and Stop callbacks even when the user registers handlers for only one event. Returning no dispatch result for the other authenticated source callback loses its durable delivery and post-correction owner checks. For an actual authenticated ObservedCallback with no event plan, use existing begin_non_tool_owned validation to retain an empty typed source observation, then settle it with no declarations, handlers, proposals or effects. Keep native unregistered events unchanged: without an authenticated source observation they still produce no lifecycle record. Do not fabricate a handler, weaken the nonempty executable-plan constructor, or grant a new task, allocation or correction allowance. Use the normal exact backend/source identity and reserved post-correction owner checks. Validate Pending source delivery and allowed continuation before writing an allow response; retain Sent validation after writing and exact source acknowledgment afterward. Test Stop-only, Submit-only and their post-tool correction interactions, plus wrong source/owner, missing-plan native events, uncertain delivery and replay. This records the real transport observation and preserves existing authority for partial configurations.

## Realized by

(none yet: recorded, not built)

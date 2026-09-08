# Provider and agent settings implementation

The confirmed contract is docs/spec/provider-agent-settings.md. Work in the current
Cairn commitment. No external reference code is copied.

1. Build the provider probe and production PTY mechanism. Demonstrate that the
   existing binary fails the checkbox/authentication sequence. Check API model
   lists with the selected credential, Codex account and model RPCs without a
   thread, and Claude auth status without a prompt. Bound bytes, pages, time and
   cancellation; suppress credential-bearing diagnostics. Exercise valid,
   rejected, missing, malformed and slow fixture responses.
2. Add explicit role assignments and provider enablement to private Config.
   Preserve old connection and Oracle configuration. Resolve inheritance without
   copying Creator values. Reuse the existing settings lock and atomic save;
   reject stale edits. Test migration, invalid assignments and save conflicts.
3. Build one keyboard editor: provider checkboxes, masked API entry, checked
   catalogs, Creator selector, all role rows and Enter/Escape submenus. Invoke it
   from startup and an independent live-terminal panel. Preserve trust, drafts,
   scroll and cancellation. Update the existing onboarding driver to the new UI
   while retaining credential and directory assertions.
4. Bind saved defaults at each eligible work admission. Preserve explicit CLI
   and per-assignment precedence. Capture the selected identity in durable task,
   operation and queued-child records before effects. Keep active tasks pinned;
   preserve allocations and old evidence across later settings changes. Exercise
   native and external requests held across changes, recovery and role isolation.
5. Update documentation and run formatting, lint, targeted PTY/request tests and
   inherited Cairn mechanisms against committed inputs. Commit every receipt and
   captured output. Complete the recorded adversarial review, resolve findings,
   install and demonstrate the full workflow against the installed binary.

The current todo status and verification results are retained in
[the commitment review](../../.cairn/reviews/provider-agent-settings.md). This
plan describes the execution sequence; the review records completed work and
any remaining findings.

A step is complete only when its code and corresponding behavioral verification pass.

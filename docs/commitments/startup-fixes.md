# Startup fixes

Status: Agreed 2026-09-06
Requirements: START-001, START-002, START-003

## Deliverable

The installed CLI accepts the requested information flags, uses the launch
working directory by default, and starts successfully with an existing owned
home settings directory. The directory repair preserves the private-storage
boundary and does not change a custom shared directory's permissions.

The developer selected this corrective work after the first coding session was
published. No new coding-loop, provider, or host-access feature is included.

## Verification

The startup mechanism drives the real executable with disposable homes and
projects. It verifies existing and new behavior, explicit workspace selection,
settings-directory repair, symlink refusal, and custom-directory refusal. The
existing onboarding and configuration checks also run. Source changes are
committed before Cairn checks. Record safe failing and corrected cases and a
final review under .cairn/reviews/startup-fixes.md before Done.

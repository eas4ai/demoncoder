# Startup fixes implementation review

commitment: startup-fixes
commit: 1f66a126550fa2e7d63279f437bbfc0af2e89bec
findings:
  - none: no open findings within this commitment
Reviewer: Codex
Date: 2026-09-06
Status: complete
Open findings: none

## Evidence

The committed startup mechanism produced passing START-001, START-002, and
START-003 receipts at 20260906T165057585Z. The command also passed the existing
three onboarding tests and one configuration test. Its six startup tests include
both default and explicit workspace tool cycles. The failure demonstrations and
fixture limits are recorded in docs/reviews/startup-mechanism.md.

Additional checks passed: cargo test --locked --all-targets (14 passed, two
ignored), cargo fmt --check, cargo clippy --locked --all-targets -- -D warnings,
and git diff --check. The ignored Rust entry points are separate live-provider
and registry-driver tests; no new live-provider check was run for this change.

Installed the release binary with cargo install --path . --locked --force. Its
-h, --help, -v, -V, and --version invocations succeeded and all version forms
agreed. A real PTY launch with no workspace flag displayed the launch directory
and reached project trust. The actual owned home settings directory changed from
0775 to 0700. All 164 pre-existing regular files retained identical contents.
Trust was declined after that observation; no provider session was started.

## What I challenged

- Information flags must exit before startup or settings mutation. Reviewed
  Clap's explicit Version action and the optional field, and checked the real
  executable's output and isolated-home assertions. Ordinary startup also passes,
  covering the earlier required-field mistake.
- Workspace selection must reach actual tools. Reviewed the test launch cwd and
  explicit override, canonical trust, all four host tool results, and the edited
  output file. The default already used the launch directory; this change adds
  regression evidence without introducing another selection rule.
- Directory repair must preserve saved configuration and limit chmod to the
  owned default directory. Reviewed no-follow directory opening, descriptor
  ownership validation before chmod, default/custom selection, saved settings
  reload, locking, and the existing atomic save. Fixtures cover existing canaries,
  saved connections and credentials, a private symlink target, and a writable
  custom parent. Refused paths create no settings lock or session event file.
- Reviewed the README and specification for claims broader than the behavior.
  Repair occurs when setup or a trust update needs to write. Existing trusted
  startup can return without taking a lock. Historical provider receipts are
  identified as evidence for their recorded revision.

No code was changed during this review. No defect within the selected commitment
was found. Foreign ownership is checked in source; the unprivileged fixture does
not create another UID's directory. The permission change uses its opened
file descriptor; the existing full settings transaction is not claimed to be
descriptor-relative or protected from every concurrent path replacement.

## Production self-audit

The change is limited to the requested startup correction, version alias,
regressions, and documentation. Existing connection and coding-loop contracts
remain covered by the tests described above. Error paths retain private-storage
checks and actionable messages. The implementation, declarations, decisions,
receipts, and user instructions agree. No further revision is needed within
this commitment.

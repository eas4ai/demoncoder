# Selected-repository assessment

Observed 2026-09-06 through the production ToolExecutor in its confined default,
using the developer-selected /home/shawn/workspace2/demoncoder repository. The
explicit ignored test is selected_repository_read_only_assessment, invoked with
DEMONCODER_ASSESS_WORKSPACE set to that path. It uses native reads and Bash only;
no model response substitutes for a tool result. Public services are observations,
not unconditional pass criteria of the local mechanism.

- Native reads of RTK.md, TILTH.md, PARTNERSHIP.md, BEST_PRACTICES.md and .git/HEAD
  succeeded. The report records byte counts rather than copying file contents.
- Bash ran despite the repository's build hard links and reference symlinks.
  pwd, branch, status, diff, HEAD and git ls-remote succeeded. It correctly showed
  the pending correction and local HEAD 3ca854a32c6ad54316d7c7a26121257f4de4032a;
  origin/main was c3f546435aceabf0309b2ebec6d90a466b22b167, so this was not yet
  synchronized or a clean final revision.
- Installed Git 2.55.0, Cargo/Rust 1.95.0 and RTK 0.40.0 ran. Cairn also ran and
  correctly reported the in-progress implementation, with exit 1 (Resolvable).
  This is not a missing-tool or permission error and is not a completion claim.
- HTTPS access to the Rust book returned HTTP 200.
- GitHub's public Actions API returned total_count 0 and no workflow runs for
  eas4ai/demoncoder. No CI pass can be inferred from an empty result.
- cargo audit --json ran successfully: 301 dependencies, no known vulnerabilities
  and no warnings, using 1,239 advisories at database commit
  5a0ebedfe8bdd2e295b171f4162f8c977bcad9a5 (updated 2026-09-02).

The normal tool environment does not carry private CI/Git credentials. Public
requests above succeeded; private-service access was not claimed. The local
controlled endpoint tests separately preserve HTTP failure and stale-check
observations. Final publication synchronization is verified outside this snapshot.

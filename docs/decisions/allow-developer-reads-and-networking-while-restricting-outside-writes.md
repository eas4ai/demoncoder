# Allow developer reads and networking while restricting outside writes

Level: Judged
Decided by: Codex
Rests on: USABLE-002 and USABLE-003; the developer explicitly rejected restrictions that prevent searching, documentation access, repository review, and normal coding checks
Would be wrong if: Ordinary source/build links still disable Bash, developer tools or standards remain hidden, provider credentials leak, or the default silently grants arbitrary writes outside its approved workspace and caches

## Decision

Use the host filesystem read-only as the default Bash view, with the selected workspace writable, shared networking, the installed tool path, sanitized environment, private process and temporary namespaces, and protected credential stores. Permit native reads of ordinary source and documentation outside the workspace while keeping native mutations rooted. Preserve read access to machine instruction files. Inspect hard-link ownership instead of rejecting every link: internal build links are usable; outside aliases receive read-only protection or a specific refusal when they refer to protected credentials. Symlinks resolve within the sandbox filesystem. Expose documented build caches as approved writable cache locations. Git operations in the selected repository and read-only Cairn inspection are ordinary coding operations. Keep the explicit host mode and its Oracle policy for unrestricted outside changes. Verify outside and credential canaries, real tool versions, network access, Git metadata, and the developer-selected repository.

## Realized by

- 86012e6465ba55050b443677527c341f3edc5ab9 Make default coding reads, networking, and developer tools usable

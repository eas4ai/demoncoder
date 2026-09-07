# Assignable subagents review

commitment: assignable-subagents
commit: 1dc7b4a8343ce66b6115026ef912238a2da6971c
findings:
  - open: SUB-002 strict child policy follows a bind mount to outside files
Status: in progress

## Strict tool component specification review

Candidate component 94cd704af806fee84f8712d1c5b09aefb39cd048 passed ten strict
boundary tests. Independent review reproduced an omitted path: inside a private
user/mount namespace, a disposable outside directory was bind-mounted beneath
the worktree. Native read returned its canary; native write and Bash changed it.
The outside file changed even though worktree-only policy was selected. Native
resolution lacks NO_XDEV and shell inspection traverses mounts before binding
the root. Reject mount crossings at both tool boundaries; preparation alone is
not a sufficient invariant. The candidate was not changed during review.

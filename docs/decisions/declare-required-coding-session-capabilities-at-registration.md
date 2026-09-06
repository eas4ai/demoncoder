# Declare required coding-session capabilities at registration

Level: Judged
Decided by: Codex
Rests on: Agreed CONN-005 and the versioned Rust adapter registration boundary
Would be wrong if: A declared missing control reaches the factory or starts work, or existing registrations lose their version-1 full-session contract

## Decision

Add explicit read, write, edit, Bash, steering, and cancellation declarations to adapter registration. The registry rejects a selected adapter missing a required coding-session control before calling its factory. Preserve the existing registration method as the full version-1 session contract and offer an additive method for explicit declarations. Keep model and effort validation at the adapter boundary, with provider rejection failing without fallback. Prove rejected factories never run and preserve the four existing connections and independent registration path.

## Realized by

- a6c0ad7a0dac8c779f8b9fa067bb632b617d0a09 Reject missing session capabilities before adapter creation

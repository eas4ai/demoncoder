# Skills and plugin packages

Status: Agreed 2026-09-09
Prefix: EXT

The developer selected this complete commitment on 2026-09-09. The companion
[design](../proposals/skills-plugins-hooks.md) defines the agreed delivery
scope and compatibility boundary. A plugin is a versioned directory containing
instructions, lifecycle handlers, agent templates or service declarations.
An enabled package generation is an immutable snapshot of those files.

[EXT-001] The application MUST import local Claude Code packages using
`.claude-plugin/plugin.json`, Codex packages using `.codex-plugin/plugin.json`,
and portable Agent Plugins 1.0 packages using root `plugin.json`, and report support separately for each
component and field. Invalid declarations and unsupported executable behavior
MUST prevent activation of that component. A required component failure MUST
prevent activation of the package. Partial activation MUST require an explicit
developer choice and list every excluded component.
Falsifier: A package is shown as fully supported while an unknown hook type is
ignored, malformed frontmatter supplies executable defaults, or a required
component is silently dropped.
Mechanism: Validate mixed supported/unsupported fixtures, malformed manifests,
duplicate identities and explicit partial activation through the production loader.
The composition rules in PCOMP-001 include portable roots with Codex overlays.

[EXT-002] The developer MUST be able to install, validate, inspect, enable,
disable, reload and remove local plugins through visible application controls.
Inspection MUST show origin, version, content digest, active components,
requested access, compatibility failures and task references to older versions.
Installing or discovering a package MUST NOT execute its code or enable it.
Falsifier: Opening an untrusted repository runs a discovered hook, installation
runs a package-manager lifecycle script, or disabled code starts on the next turn.
Mechanism: Exercise the complete management flow with a harmless execution
canary and restart; inspect both displayed state and actual process effects.

[EXT-003] The loader MUST admit package files only through bounded, validated
paths inside the imported package snapshot. Enabled generations MUST remain
unchanged when their source directory is edited. The application MUST reject
escaping paths, unsafe links, conflicting component names and oversized input.
Falsifier: A manifest reads an outside canary through `..` or a symlink, or editing
the original script changes a handler already pinned to a running task.
Mechanism: Import safe and escaping fixtures, race source replacement during
import and execution, and exceed each published resource bound.

[EXT-004] The application MUST pin a validated plugin generation and its policy
to each admitted task and child. Reload MUST activate a complete new generation
atomically at an idle boundary. Disable MUST stop new admissions and cancel owned
background activity. An interrupted required gate MUST leave affected work blocked
until the developer explicitly removes or replaces that policy.
Repair uses the linked replacement-task transition in PRUN-003. It MUST NOT
replace the generation inside an existing task, replay settled effects or reset
its cumulative allowance. Ordinary reload cannot initiate that transition.
Falsifier: A task mixes old instructions with a new handler, a failed reload loses
the working generation, or disabling a gate silently authorizes blocked work.
Mechanism: Reload during a running task, fail validation, disable a stalled gate,
and restart with old generations still referenced by durable work.

[EXT-005] The application MUST load standalone and packaged Claude Code and
Codex Agent Skills `SKILL.md` files and legacy Markdown commands with stable
plugin-qualified names. It MUST reserve built-in
control names and expose a bounded name/description catalog before loading a
selected skill's body. Explicit invocation MUST substitute arguments as literal
instruction text without executing substitutions or shell syntax.
Falsifier: A plugin overrides the built-in acceptance command, every skill body enters every prompt,
`$(...)` executes during argument expansion, or two plugins shadow each other.
Mechanism: Invoke namespaced skills and legacy commands, inspect the assembled
prompt, and try name collisions and harmless shell-substitution canaries.

[EXT-006] The application MUST honor skill invocation restrictions, including
Codex `agents/openai.yaml` policy, and report
unsupported skill frontmatter. A skill with `disable-model-invocation` enabled
MUST require developer invocation. Skill instructions and referenced resources
MUST retain their plugin provenance. They MUST NOT grant permissions, approve
lessons, accept work or authorize hidden delegation.
Falsifier: The model invokes a developer-only skill, a skill's text changes tool
authority, or an instruction is recorded as developer approval.
Mechanism: Test explicit and model requests for each restriction and adversarial
instruction bodies through all four connections.

[EXT-007] The application MUST enforce shared skill and plugin policy in
DemonCoder across all four connections. It MUST expose backend capabilities
without enabling a second, inherited backend plugin or hook system. Unsupported
backend events MUST be reported before a dependent plugin can run.
Falsifier: A backend's ambient plugin executes outside DemonCoder's admission
boundary, a supported core hook is bypassed on one connection, or an opaque
backend event is reported as observed without evidence.
Mechanism: Drive each adapter with controlled prompt/tool/completion exchanges
and an ambient-plugin canary; distinguish controlled transport from live smoke evidence.

[EXT-008] Application-bundled workflow plugins MUST use the same activation,
policy, execution and evidence rules as user packages. The bundled Best
Practices plugin MUST enforce the real receipt-based completion policy in PRUN-005
and pair that gate with readable skill instructions. Cairn integration MUST remain
optional. It MUST consume actual referee output when installed.
Falsifier: Bundled code bypasses confinement, a claimed enforcement gate always
passes, or ordinary DemonCoder use requires Cairn to be installed.
Mechanism: Violate each PRUN-005 obligation before and after correction, inspect
its gate receipt, and launch a normal session without either workflow enabled.

[EXT-009] The implementation MUST deliver the complete agreed plugin component
and lifecycle contract before reporting Done. It MUST NOT count deferred work,
placeholder implementations, parse-only support or unsupported labels as delivery
of a required capability. An actual upstream access or API restriction MUST be
resolved or presented for an explicit developer scope decision.
Falsifier: The commitment is reported complete while one of its named components
cannot run, only one ecosystem is supported, or required backend coverage is missing.
Mechanism: Audit a requirement-to-production-test coverage matrix, exercise real
packages from both ecosystems, and challenge every unsupported entry against the
agreed scope during the final commitment review.

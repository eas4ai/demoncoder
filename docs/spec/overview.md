# DemonCoder

Status: Agreed 2026-09-06

DemonCoder is a Rust terminal coding assistant with a small pi-style coding
core, assignable subagents, advanced orchestration, and evidence-based
self-improvement. The developer can watch work, correct its direction,
inspect its results, and continue in the same coding session.

The developer chose an independent application with selective reuse of
reference code. Original pi supplies the main behavioral reference for the
small loop, four initial tools, and hooks. A copied component brings its
dependencies, tests, provenance, and applicable license conditions with it.
The project does not inherit the entire Rust pi fork or its feature list.

Direct model APIs feed DemonCoder's native loop. An adapter for Codex
app-server or Claude's Agent SDK/headless CLI represents an external agent
backend. Each session has one declared loop owner. The application presents
consistent session controls while exposing backend capability limits.

The first commitment is a usable terminal coding session with the four
initial connections requested by the developer. Subagents, advanced review,
and self-improvement remain required product goals for later commitments.
The roadmap stages delivery; it does not remove those goals.

The developer agreed to use Cairn for development on 2026-09-06. Cairn
records the selected scope and check evidence. It is development tooling;
DemonCoder's runtime does not depend on it. The developer and agent remain
responsible for choosing sound requirements and meaningful checks.

## Specification map

| File | Prefix | Owns |
|---|---|---|
| [glossary.md](glossary.md) | — | Terms used in this project. |
| [coding-session.md](coding-session.md) | CODE | The first usable coding session and its execution boundaries. |
| [connections.md](connections.md) | CONN | Provider selection, authentication, adapter boundaries, and extensibility. |
| [startup.md](startup.md) | START | Help, version, launch-directory selection, and safe settings-directory repair. |
| [developer-usability.md](developer-usability.md) | USABLE | Scrollable bounded chat and practical read, network, and coding-tool access. |
| [output-limits.md](output-limits.md) | OUTPUT | Model output settings, discovery and incomplete-response handling. |
| [usage-display.md](usage-display.md) | DISPLAY | Visibility of current-turn usage. |
| [chat-presentation.md](chat-presentation.md) | CHAT | Activity hierarchy, expandable output, syntax colors and spacing. |
| [terminal-usability-sweep.md](terminal-usability-sweep.md) | SWEEP | The selected reliability investigation, mouse interaction, activity and status sweep. |
| [clipboard-shortcuts.md](clipboard-shortcuts.md) | CLIP | Corrected clipboard shortcuts and safe terminal paste. |
| [roadmap.md](roadmap.md) | — | The selected commitment and intended delivery sequence. |

The broader [design narrative](../spec.md) retains product direction,
reference observations, and proposals that have not yet become requirements.
Only confirmed requirements marked Agreed authorize implementation under
Cairn. The developer confirmed this keystone, the glossary, and the first
requirement set with its falsifiers on 2026-09-06.

Implementation choices such as the terminal toolkit and exact component
extraction remain to be recorded against the agreed behavior. They do not
require specifying every later commitment before the first can start.

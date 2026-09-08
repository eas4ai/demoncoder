# Glossary

Status: Agreed 2026-09-06

| Term | Meaning in DemonCoder |
|---|---|
| Coding session | A conversation and its associated workspace that can contain several turns. |
| Turn | Work started by one submitted prompt, ending in completion, cancellation, or failure. |
| Workspace | The directory and files explicitly authorized for a session's tool access. |
| Tool | An operation requested by a model and admitted by the runtime before execution. |
| Hook | A typed extension point with specified ordering, effects, cancellation, and failure behavior. |
| Steering | A developer correction directed at work that is already running. |
| Safe tool boundary | A point after an admitted tool operation finishes and before another tool operation is admitted. |
| Model provider | An adapter that supplies model responses and tool requests to DemonCoder's native loop. |
| Agent backend | An adapter to an external agent runtime, such as Codex app-server or Claude Code, that owns its session's model/tool loop. |
| Connection | A selectable adapter, authentication method, model, and associated configuration. |
| Subagent | An agent assigned bounded work with its own context, tools, allocation, and visible result. |
| Verification | An executed check of the actual workspace or runtime behavior, with its result and limitations retained. |
| Commitment | One developer-selected deliverable with named requirements and a completion condition. |
| Falsifier | An observation that would show a requirement is not met. |
| Mechanism | A declared command that observes whether requirements hold and records evidence through Cairn. |

## Evidence-based improvement terms

Confirmed 2026-09-07 with LEARN-001 through LEARN-008.

| Term | Meaning |
|---|---|
| Observation | A cited record of what happened, including original evidence and any attributed developer annotation. |
| Candidate | A bounded proposed correction with its supporting observations, expected benefit, behavioral check and risks. |
| Outcome | What executed checks and review establish about a candidate's correction, including unresolved or insufficient evidence. |
| Lesson | A scoped reusable statement supported by observation and outcome history, enabled only with developer approval. |

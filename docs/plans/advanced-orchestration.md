# Advanced orchestration implementation plan

> Execute with superpowers:subagent-driven-development, one component at a time
> with specification review followed by quality review. Cairn owns the parent
> loop. Implementation workers use isolated Git worktrees and reviewed branches
> return through `git merge --no-ff`.

**Goal:** Deliver ORCH-001 through ORCH-007 through the installed terminal.

**Architecture:** Extend the existing delegation owner with a pure dependency
predicate and durable supervision state. A queued assignment reserves active
capacity before preparation. The existing strict tools and native/backend owners
execute work; the selected reviewer supplies the advisor role, and a selected
judge resolves disputed findings. No second coding loop is added.

**Tech stack:** Rust/Tokio, existing private store, Git worktrees, existing four
adapter factories and Python production terminal/transport fixtures.

## Task 1: bounded dependency predicate (ORCH-001, ORCH-002)

Files: create src/subagents/schedule.rs and tests/orchestration_schedule.rs;
export schedule from src/subagents/mod.rs. This component has no runtime effects.

- [x] Add the pure projection and API below. It deliberately distinguishes
  completion from integration and does not represent provider/model internals.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Queued, Active, AwaitingIntegration, Integrated, Blocked }
#[derive(Clone, Debug)]
pub struct Node {
    pub id: u64,
    pub dependencies: Vec<u64>,
    pub status: Status,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Gate { Eligible, Waiting(Vec<u64>), Blocked(Vec<u64>) }
pub fn validate_graph(nodes: &[Node]) -> anyhow::Result<()>;
pub fn gate(id: u64, nodes: &[Node]) -> anyhow::Result<Gate>;
pub fn admit_ready(nodes: &[Node], limit: usize) -> anyhow::Result<Vec<u64>>;
```

- [x] Write failing tests first. Reject more than 32 nodes, zero/duplicate IDs,
  more than 31 dependencies, repeated/unknown/self/forward dependencies. Requiring
  earlier IDs makes cycles impossible; validate recovered graphs too. Compute
  waiting and blocked prerequisite IDs deterministically. Only Integrated releases
  a prerequisite. Select eligible Queued nodes in increasing ID order, taking
  `limit - active_count`, where limit is 1 through 8. Reject over-capacity state.

```rust
let nodes = vec![
    Node { id: 1, dependencies: vec![], status: Status::AwaitingIntegration },
    Node { id: 2, dependencies: vec![1], status: Status::Queued },
    Node { id: 3, dependencies: vec![], status: Status::Queued },
];
assert_eq!(gate(2, &nodes).unwrap(), Gate::Waiting(vec![1]));
assert_eq!(admit_ready(&nodes, 2).unwrap(), vec![3]);
```

- [x] Implement validation and scheduling with bounded ordered collections.
  Never consume capacity for waiting work or treat a blocked sibling as a reason
  to stop independent work. Run `cargo test --locked --test orchestration_schedule`
  and `cargo fmt --check`. Record failing demonstrations, review spec then quality,
  and merge the approved component.

## Task 2: durable orchestration and role execution (ORCH-003 through ORCH-007)

Files: extend src/subagents/state.rs, src/subagents/mod.rs,
src/workflow/runtime.rs and src/workflow/runtime/delegation.rs; create
src/subagents/supervision.rs; extend src/workflow/review.rs. Reuse existing
Identity, CheckReceipt, ReviewReceipt, Allocation and Store types.

- [ ] Add optional backward-compatible orchestration state to each AgentRecord:
  dependency IDs, current stage, admitted correction count, retained role receipts
  and a waiting/held explanation. Each receipt contains role, round, connection
  identity, snapshot, supplied evidence and original verdict/findings/explanation.
  Keep shared record-size refusal and bounded history; never silently evict findings.

- [ ] Add optional orchestration identity to DelegationIdentity containing the
  selected judge and the fixed maximum of two corrections. Old sessions keep None.
  Resume must compare original authority and retain all counts. Queued work cannot
  be mistaken for completed or active work. Mark interrupted active supervision
  uncertain through the existing recovery path; never replay it on resume.

- [ ] Generalize the existing tool-free review helper with a trusted role enum:

```rust
pub enum Role { Advisor, WorkerResponse, Judge }
pub async fn run_role(
    role: Role,
    config: &Connection,
    workspace: &Path,
    evidence: String,
    events: &EventSink,
) -> anyhow::Result<Decision>;
```

  Preserve the existing `run` reviewer behavior. Each new role gets a fresh
  tool-free session, its own labeled phase and actual bounded runtime evidence.
  WorkerResponse uses the assignment connection with coding tools disabled. Judge
  receives original advisor findings and worker response alongside source/checks.
  Retain Decision validation and reject malformed, missing or oversized output.

- [ ] Exercise the role helper with a forbidden tool request, invalid verdict,
  unknown usage and cancellation. Assert no filesystem effect and no unaccounted
  native call/backend invocation. Retain output and role identity before advancing.

## Task 3: manager, queue and terminal integration (ORCH-001 through ORCH-007)

Files: extend src/subagents/manager.rs, src/subagents/session.rs, src/config.rs,
src/main.rs, src/events.rs and src/terminal.rs. Add tests/orchestration_state.rs
for durable gates and tests only where the production fixture cannot observe them.

- [ ] Add `--orchestrate --judge CONNECTION`, requiring enabled child connections,
  selected checks and `--reviewer` (the advisor). Normal delegation without the
  opt-in keeps its existing manual validation behavior. Trusted settings select
  judge identity; model arguments cannot introduce a connection or access policy.

- [ ] Extend the parent delegate schema with optional `depends_on: [ID, ...]` only
  when orchestration is enabled. Preserve AssignmentRequest for ordinary callers.
  Add `/delegate-after IDS CONNECTION OWNED,PATHS OBJECTIVE` for developer entry.
  Retain an assignment as Queued before any worktree creation. Reject dependencies
  unless the graph predicate admits them. Store the selected settings and identity.

- [ ] In the shared durable update, select eligible queued IDs and transition
  them to Preparing before launching jobs. The active count includes preparation,
  worker, validation, advisor, response, judge, correction and integration. Capture
  parent content only after admission. Pump waiting work after explicit integration
  and job completion without bypassing parent/integration exclusion or cancellation.

- [ ] After a worker completes, inspect the current child patch and execute checks
  through the strict executor. Run the advisor on current source and results.
  Clear plus passing checks becomes Ready; Blocked holds. Findings retain a worker
  response and judge decision. A judge cannot clear failed checks or missing evidence.
  A clear judge can resolve a disputed advisor finding only for the same snapshot.

- [ ] When correction is required, persist the next admitted round before a
  coding turn. Reuse the confined assignment and its existing adapter loop. Supply
  original findings and current evidence as agent evidence, not human instructions.
  At most two corrective turns run. After each, rerun checks and supervision from
  a fresh snapshot. A third request holds without calling the worker. Existing
  shared deadline/call/tool/backend limits apply to every step.

- [ ] Preserve the explicit integration gate, including current passing checks,
  resolved current review, ownership and conflicts. Individual cancel affects one
  assignment. Parent cancel/shutdown marks queued work cancelled before stopping
  active jobs, so job completion cannot launch queued effects after cancellation.
  Recovery preserves queued prerequisites and holds all uncertain execution.

- [ ] Render assignment IDs, waiting causes, supervision roles and correction
  counts; `/agent ID` retains original evidence. Exercise held publication and
  delayed roles with the existing responsive terminal control path. Run Rust
  tests, formatting and Clippy; spec review then quality review before merging.

## Task 4: production mechanisms and release (ORCH-001 through ORCH-007)

Files: create tests/advanced_orchestration.py,
tests/orchestration_backend_fixture.py and scripts/check-advanced-orchestration.sh;
reuse assignable_subagents.py, verification_workflow.py and terminal_session.py.
Update README.md with exact flags, role behavior, bounds and recovery limits.

- [ ] First run the actual binary with the new opt-in and retain its missing-option
  failure. Build controlled API and subscription fixtures that use actual adapter
  transports, tool effects, verification commands, role requests and durable state.
  Fake service responses may choose verdicts but cannot substitute for host checks.

- [ ] ORCH-001/002: queue independent and dependent assignments with delayed work,
  assert concurrency bounds, hold completed/Ready prerequisites, integrate explicitly,
  inspect dependent baseline, and show failed/cancelled prerequisites holding only
  their dependents. Reject malformed dependency inputs before effects.
- [ ] ORCH-003/004: inspect advisor source/check inputs and original worker/judge
  messages; exercise upheld/dismissed/blocked/invalid findings and attempted role
  tool use. Stale source and failed checks must prevent readiness or integration.
- [ ] ORCH-005/006: persist two corrective file mutations and deny a third; exhaust
  each applicable shared allowance; cancel delayed roles and heartbeat work under
  queue pressure; verify no queued or active effects after the two-second bound.
- [ ] ORCH-007: kill during waiting, advisor, worker response, judge and correction.
  Resume and inspect dependency IDs, original evidence and spent rounds. No uncertain
  work replays, and no prerequisite becomes a fabricated successful integration.

```bash
cargo build --locked
python3 tests/advanced_orchestration.py --requirement ORCH-001
```

- [ ] The shell mechanism runs prerequisite Rust tests and all seven production
  cases, emits each `cairn: ORCH-00N: pass` only after that case passes, and returns
  failure for any failed or absent behavior. Commit code before Cairn checks;
  commit captured outputs and receipts after each check.
- [ ] Run the full Rust suite, strict Clippy, formatting and affected existing
  subagent/verification/terminal tests. Review without changing code, record findings
  before repairs, perform the 14-rule audit, install with `cargo install --path .
  --locked`, compare executable hashes and run all seven production cases using
  `DEMONCODER_TEST_BINARY=/home/shawn/.cargo/bin/demoncoder`. Finish only on Cairn
  Done with completed installed verification.

//! Explicit developer acceptance around the existing coding session.
pub mod allocation;
mod improvements;
pub mod review;
pub mod runtime;
pub mod state;
pub mod store;
pub mod workspace;

pub(crate) const BUSY_CONTROL: &str =
    "Task controls require stopped work; draft retained. Cancel or wait for the current operation.";

pub(crate) fn is_control(text: &str) -> bool {
    crate::learning::is_control(text)
        || matches!(
            text.split_whitespace().next(),
            Some(
                "/task"
                    | "/task-status"
                    | "/verify"
                    | "/review"
                    | "/correct"
                    | "/accept"
                    | "/abandon"
                    | "/workflow-help"
                    | "/reconcile"
                    | "/delegate"
                    | "/delegate-after"
                    | "/agents"
                    | "/agent"
                    | "/agent-cancel"
                    | "/agent-validate"
                    | "/agent-integrate"
                    | "/agent-reconcile"
            )
        )
}

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use async_trait::async_trait;
use serde_json::json;
use tokio::sync::mpsc;

use crate::{
    config::Connection,
    events::{Event, EventSink},
    session::{Command, Session, TurnEnd},
    tools::{ToolCall, ToolExecutor},
};
use state::{CheckReceipt, Task};

#[derive(Clone, Default)]
pub struct Settings {
    pub checks: Vec<String>,
    pub capture_scope: workspace::CaptureScope,
    pub reviewer: Option<Connection>,
    pub reviewer_default: bool,
    pub correction_limit: u32,
    pub limits: allocation::Limits,
}

pub struct WorkflowSession {
    inner: Box<dyn Session>,
    connection: Connection,
    workspace: PathBuf,
    settings: Settings,
    task: Option<Task>,
    next_id: u64,
    runtime: runtime::SharedRuntime,
    resumed: bool,
    resume_inspected: bool,
    live_settings: Option<crate::settings::Handle>,
    connection_label: Option<String>,
    admitted_creator: Option<crate::config::Selection>,
}

impl WorkflowSession {
    pub fn new(
        mut inner: Box<dyn Session>,
        connection: Connection,
        workspace: PathBuf,
        settings: Settings,
        runtime: runtime::SharedRuntime,
        resumed: bool,
    ) -> Result<Self> {
        let mut record = runtime.record()?;
        settings.capture_scope.validate()?;
        ensure!(
            record.capture_scope == settings.capture_scope,
            "workflow source and review scope differs from the retained session scope"
        );
        if let Some(task) = &mut record.task
            && task.creator_identity.is_none()
        {
            task.creator_identity = Some(record.identity.clone());
        }
        if resumed && record.task.is_some() {
            ensure!(
                (record
                    .task
                    .as_ref()
                    .is_some_and(|task| task.reviewer_default)
                    && settings.reviewer_default
                    && settings.reviewer.is_some())
                    || match (&record.reviewer_identity, &settings.reviewer) {
                        (Some(identity), Some(connection)) => identity.matches(connection),
                        (None, None) => true,
                        _ => false,
                    },
                "resume requires the task's original reviewer connection and model"
            );
        }
        if resumed && let Some(checkpoint) = &record.checkpoint {
            let results: Vec<_> = record
                .operations
                .iter()
                .filter(|o| o.id > record.checkpoint_cursor && o.phase == "worker")
                .filter_map(|o| o.result.clone())
                .collect();
            inner.restore(checkpoint, &results)?;
        }
        if resumed && let Some(task) = &mut record.task {
            task.stopped = true;
        }
        Ok(Self {
            connection_label: None,
            inner,
            connection,
            workspace,
            settings,
            task: record.task,
            next_id: record.next_task,
            runtime,
            resumed,
            resume_inspected: !resumed,
            live_settings: None,
            admitted_creator: None,
        })
    }

    pub fn with_live_settings(mut self, settings: crate::settings::Handle) -> Self {
        self.live_settings = Some(settings);
        self
    }

    async fn refresh_creator(&mut self, events: &EventSink) -> Result<()> {
        let Some(selection) = self.admitted_creator.take() else {
            return Ok(());
        };
        let new_identity = runtime::Identity::from(&selection.connection);
        if new_identity == runtime::Identity::from(&self.connection) {
            self.connection_label = Some(selection.name);
            return Ok(());
        }
        ensure!(
            !self.runtime.record()?.recovery_pending,
            "reconcile interrupted work before applying the new Creator assignment"
        );
        let mut replacement =
            crate::adapters::builtins()?.open(&selection.connection, &self.workspace)?;
        // Native checkpoints can continue within the same adapter and account.
        // External backends own opaque contexts; changing them opens a new one.
        let retain_native = self.connection.adapter == selection.connection.adapter
            && self.connection.endpoint == selection.connection.endpoint
            && self.connection.api_key == selection.connection.api_key
            && self.inner.supports_workflow();
        if retain_native && let Some(checkpoint) = self.inner.checkpoint() {
            replacement.restore(&checkpoint, &[])?;
        }
        self.runtime
            .bind_creator(&selection.connection, replacement.checkpoint())?;
        if let Err(error) = self.inner.close().await {
            self.runtime.hold()?;
            return Err(error).context("old provider could not close; inspect before continuing");
        }
        self.inner = replacement;
        self.connection = selection.connection;
        self.connection_label = Some(selection.name);
        events.emit(Event::ModelAssignment {
            connection: format!("{} · {}", self.connection_label.as_deref().unwrap_or(&self.connection.adapter), self.connection.adapter),
            model: self.connection.model.clone(),
            explanation: if retain_native {
                "New work uses the saved assignment. Native conversation retained; earlier identities, results and allocation remain recorded."
            } else {
                "New work uses the saved assignment in a new provider context. Earlier conversation and results remain visible; no opaque backend session was transferred."
            }.into(),
        }).await
    }

    async fn snapshot(&mut self) -> Result<workspace::Snapshot> {
        let session_path = self.runtime.directory()?;
        ensure!(
            !session_path.starts_with(&self.workspace)
                && !crate::export_policy::contains_declared_private(
                    &self.workspace,
                    &self.connection.access.credential_paths,
                )?,
            "task workspace contains private session or connection settings; select the project directory that excludes those private files"
        );
        let root = self.workspace.clone();
        let scope = self.settings.capture_scope.clone();
        tokio::task::spawn_blocking(move || workspace::capture_with_scope(&root, &scope))
            .await
            .context("workspace capture failed")?
    }

    async fn publish(&mut self, events: &EventSink) -> Result<()> {
        if self.task.is_none() {
            return Ok(());
        }
        let snapshot = self.snapshot().await?;
        self.runtime
            .save_task(&self.task, self.next_id, Some(&snapshot.digest))?;
        let task = self.task.as_ref().expect("active task");
        events
            .emit(Event::TaskState {
                task_id: task.id,
                stopped: task.stopped,
                verification: if task.verified(&snapshot.digest) {
                    "passed"
                } else if task.checks.iter().any(|c| !c.success) {
                    "failed"
                } else {
                    "unverified"
                }
                .into(),
                review: if task.reviewed(&snapshot.digest) {
                    "clear"
                } else if task.review.is_some() {
                    "blocked"
                } else {
                    "not reviewed"
                }
                .into(),
                accepted: task.accepted.as_ref() == Some(&snapshot.digest),
            })
            .await?;
        if let Some(a) = self.runtime.record()?.allocation {
            events
                .emit(Event::TaskAllocation {
                    remaining_seconds: a.remaining_ms()? / 1000,
                    model_calls: a.model_calls,
                    model_limit: a.limits.model_calls,
                    tool_calls: a.tool_calls,
                    tool_limit: a.limits.tool_calls,
                    usage: a.usage,
                })
                .await?;
        }
        Ok(())
    }

    async fn dispatch(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        if !self.resume_inspected {
            // Ordinary conversations have no acceptance snapshot. Inspect on
            // each resume rather than capturing private files in a home workspace.
            let changed = if self.task.is_some() {
                let current = self.snapshot().await?;
                self.runtime
                    .record()?
                    .last_snapshot
                    .as_ref()
                    .is_none_or(|old| old != &current.digest)
            } else {
                true
            };
            if changed {
                self.runtime.hold()?;
            }
            self.resume_inspected = true;
        }
        if let Some(explanation) = prompt.strip_prefix("/reconcile ") {
            let digest = if self.task.is_some() {
                Some(self.snapshot().await?.digest)
            } else {
                None
            };
            self.runtime.reconcile(explanation, digest.as_deref())?;
            events.checkpoint(self.inner.checkpoint())?;
            events
                .emit(Event::Text {
                    text: "\nInspection recorded. No interrupted operation was replayed.\n".into(),
                })
                .await?;
            return Ok(TurnEnd::Complete);
        }
        let improvement = if prompt.split_whitespace().next() == Some("/improve") {
            Some(crate::learning::control::improvement_id(&prompt)?)
        } else {
            None
        };
        if crate::learning::is_control(&prompt) && improvement.is_none() {
            return self.learning_control(prompt, commands, events).await;
        }
        if prompt.strip_prefix("/task ").is_some() || improvement.is_some() {
            ensure!(
                !self.runtime.record()?.recovery_pending,
                "inspect and reconcile interrupted work before starting a new task"
            );
            ensure!(
                self.inner.supports_workflow(),
                "this backend cannot enforce individual model-call allocations or restore task context; explicit tasks require a native API connection"
            );
            ensure!(
                self.connection
                    .access
                    .oracle
                    .as_ref()
                    .is_none_or(|o| matches!(o.adapter.as_str(), "openai-api" | "anthropic-api")),
                "task Oracle must use a native API connection with enforceable call admission"
            );
            ensure!(
                self.settings
                    .reviewer
                    .as_ref()
                    .is_none_or(|r| matches!(r.adapter.as_str(), "openai-api" | "anthropic-api")),
                "task reviewer must use a native API connection with enforceable call admission"
            );
            ensure!(
                self.task.as_ref().is_none_or(|t| t.accepted.is_some()),
                "current task is not accepted; continue it or use /abandon before starting another"
            );
            let snapshot = self.snapshot().await?;
            let (objective, linkage) = if let Some(candidate) = improvement {
                match self
                    .reserve_improvement(candidate, commands, events)
                    .await?
                {
                    Ok(value) => value,
                    Err(end) => return Ok(end),
                }
            } else {
                (
                    prompt
                        .strip_prefix("/task ")
                        .expect("task command")
                        .to_owned(),
                    None,
                )
            };
            let mut task = Task::new(
                self.next_id,
                objective.clone(),
                self.settings.checks.clone(),
                snapshot,
                self.settings.correction_limit,
            )?;
            task.improvement = linkage;
            task.reviewer_default = self.settings.reviewer_default;
            task.creator_identity = Some(runtime::Identity::from(&self.connection));
            self.runtime.archive()?;
            self.task = Some(task);
            self.next_id += 1;
            self.runtime.allocate(
                self.settings.limits.clone(),
                self.settings.reviewer.as_ref(),
            )?;
            self.runtime.save_task(&self.task, self.next_id, None)?;
            self.publish(events).await?;
            return self.work(objective, false, commands, events).await;
        }
        match prompt.trim() {
            "/task-status" => self.show_inspection(events).await,
            "/accept" => {
                ensure!(
                    !self.runtime.record()?.recovery_pending,
                    "reconcile interrupted or changed work before acceptance"
                );
                let snapshot = self.snapshot().await?;
                self.task
                    .as_mut()
                    .context("no task; start with /task followed by its objective")?
                    .accept(&snapshot.digest)?;
                self.runtime
                    .save_task(&self.task, self.next_id, Some(&snapshot.digest))?;
                Ok(TurnEnd::Complete)
            }
            "/verify" => self.verify(commands, events).await,
            "/review" => self.review(commands, events).await,
            "/correct" => {
                let task = self.task.as_ref().context("no task to correct")?;
                let prompt = format!(
                    "Correct this task within its original scope: {}\nRetained verification results: {}\nRetained review: {}",
                    task.objective,
                    serde_json::to_string(&task.checks)?,
                    serde_json::to_string(
                        &task
                            .review
                            .as_ref()
                            .map(|r| json!({"findings":r.findings,"explanation":r.explanation}))
                    )?
                );
                let outcome = self.work(prompt, true, commands, events).await?;
                if outcome != TurnEnd::Complete {
                    return Ok(outcome);
                }
                let outcome = self.verify(commands, events).await?;
                if outcome != TurnEnd::Complete {
                    return Ok(outcome);
                }
                self.review(commands, events).await
            }
            "/abandon" => {
                self.runtime.archive()?;
                self.task = None;
                events
                    .emit(Event::Text {
                        text: "\nTask abandoned; workspace files preserved.\n".into(),
                    })
                    .await?;
                Ok(TurnEnd::Complete)
            }
            "/workflow-help" => {
                events.emit(Event::Text { text: "\n/task OBJECTIVE starts a task. /verify runs selected --check commands. /review requests the --reviewer connection. /correct uses a bounded correction round. /accept accepts current verified and reviewed files. /task-status shows evidence status. /abandon ends a task without acceptance.\n".into() }).await?;
                Ok(TurnEnd::Complete)
            }
            _ => self.work(prompt, false, commands, events).await,
        }
    }

    async fn show_inspection(&mut self, events: &EventSink) -> Result<TurnEnd> {
        let target = self
            .task
            .as_ref()
            .map_or(crate::inspection::Target::Overview, |task| {
                crate::inspection::Target::Task(task.id)
            });
        let snapshot = self
            .runtime
            .inspection(Some(crate::inspection::Request {
                target,
                ..Default::default()
            }))?
            .context("session record is busy; retry inspection or use F2")?;
        let page = snapshot.page.context("inspection page unavailable")?;
        events
            .emit(Event::Text {
                text: page.message(),
            })
            .await?;
        Ok(TurnEnd::Complete)
    }

    async fn work(
        &mut self,
        prompt: String,
        correction: bool,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        let runtime = self.runtime.clone();
        let root = self.workspace.clone();
        let objective = self
            .task
            .as_ref()
            .map_or_else(|| prompt.clone(), |task| task.objective.clone());
        let target = self.task.as_ref().map_or_else(
            || "conversation".to_owned(),
            |task| format!("task:{}", task.id),
        );
        let prepare = crate::learning::control::blocking(move || {
            crate::learning::context::prepare(&runtime, &root, &[], &objective, target)
        });
        // The inner session parses these controls. Evidence must not change their
        // arguments or become part of a developer-authored child objective.
        let context = if is_control(&prompt) {
            None
        } else {
            match crate::learning::control::cancellable(prepare, commands, events).await? {
                Ok(context) => Some(context),
                Err(end) => return Ok(end),
            }
        };
        let prompt = match &context {
            Some(context) => crate::learning::context::with_prompt(context, prompt),
            None => prompt,
        };
        if let Some(task) = &mut self.task {
            task.start_work(correction)?;
        }
        self.runtime.save_task(&self.task, self.next_id, None)?;
        self.runtime.begin_phase("worker", Some(&prompt))?;
        if let Some(context) = context {
            self.runtime.retain_learning_context(context)?;
        }
        let result = if self.task.is_some() || self.runtime.record()?.delegation.is_some() {
            tokio::time::timeout(
                self.runtime.remaining()?,
                self.inner.turn(prompt, commands, events),
            )
            .await
            .context("cumulative task deadline exhausted")
            .and_then(|r| r)
        } else {
            self.inner.turn(prompt, commands, events).await
        };
        self.inner.settle_interruption()?;
        events.checkpoint(self.inner.checkpoint())?;
        if let Some(task) = &mut self.task {
            task.stopped = true;
        }
        result
    }

    async fn verify(
        &mut self,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        let before = self.snapshot().await?;
        let task = self.task.as_mut().context("no task to verify")?;
        task.start_verification()?;
        let task_id = task.id;
        let generation = task.verification_generation;
        let check_commands = task.commands.clone();
        let executor = ToolExecutor::with_policy(&self.workspace, &self.connection.access)?;
        executor.set_intent(&task.objective);
        // Tool admission copies this attribution into the durable operation,
        // retaining it even if capture or receipt publication is interrupted.
        self.runtime
            .save_task(&self.task, self.next_id, Some(&before.digest))?;
        self.runtime.begin_phase("verification", None)?;
        let events = events.for_phase("verification");
        for (index, command) in check_commands.into_iter().enumerate() {
            let call = ToolCall {
                id: format!("verify-{task_id}-{generation}-{index}"),
                name: "bash".into(),
                arguments: json!({"command":command}),
            };
            let result = {
                let run = tokio::time::timeout(
                    self.runtime.remaining()?,
                    executor.execute(call, &events),
                );
                tokio::pin!(run);
                loop {
                    tokio::select! {
                        biased;
                        command = commands.recv() => match command {
                            Some(Command::Cancel) => return Ok(TurnEnd::Cancelled),
                            Some(Command::Shutdown) | None => return Ok(TurnEnd::Shutdown),
                            Some(Command::Submit {reply, ..}) => { let _ = reply.send(Err("Verification is running; draft retained. Cancel or wait for it to finish.")); },
                            Some(Command::Prompt(_)) => events.emit_advisory(Event::Error { message: "Verification is running; submit after it stops.".into() })?,
                        },
                        result = &mut run => break result.context("cumulative task deadline exhausted")??,
                    }
                }
            };
            let limitation = match self.snapshot().await {
                Ok(after) if before.digest == after.digest => None,
                Ok(_) => Some(
                    "Workspace changed during verification; rerun checks on stable files."
                        .to_owned(),
                ),
                Err(error) => Some(format!(
                    "Workspace capture failed after verification: {error:#}; result retained, but verification is unusable. Restore a capturable workspace and rerun checks."
                )),
            };
            let stable = limitation.is_none();
            self.task
                .as_mut()
                .expect("active verification task")
                .checks
                .push(CheckReceipt {
                    command,
                    snapshot: before.digest.clone(),
                    success: result.success && stable,
                    output: match limitation {
                        Some(reason) => format!("{}\n{reason}", result.output),
                        None => result.output,
                    },
                    exit_code: result.exit_code,
                });
            self.runtime.save_task(&self.task, self.next_id, None)?;
            if !stable {
                break;
            }
        }
        Ok(TurnEnd::Complete)
    }

    async fn review(
        &mut self,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        if self.settings.reviewer.is_some()
            && let Some(settings) = &self.live_settings
        {
            self.settings.reviewer = Some(settings.role(
                crate::settings::Role::Reviewer,
                settings.args().reviewer.as_deref(),
            )?);
        }
        let config = self
            .settings
            .reviewer
            .as_ref()
            .context("select --reviewer with a configured connection before requesting review")?
            .clone();
        ensure!(
            matches!(config.adapter.as_str(), "openai-api" | "anthropic-api"),
            "task reviewer must use a native API connection with enforceable call admission"
        );
        let after = self.snapshot().await?;
        let task = self.task.as_mut().context("no task to review")?;
        task.start_review()?;
        ensure!(
            !task.commands.is_empty() && task.checks.len() == task.commands.len(),
            "run the selected checks before review; missing evidence blocks review"
        );
        ensure!(
            task.checks.iter().all(|c| c.snapshot == after.digest),
            "workspace changed since verification; rerun checks before review"
        );
        let sources = workspace::review_evidence(&task.baseline, &after)?;
        let payload = serde_json::to_string(
            &json!({ "task_id":task.id, "objective":task.objective, "source_evidence":sources, "current_checks":task.checks, "previous_checks":task.check_history, "previous_reviews":task.review_history.iter().map(|r| json!({"reviewer":r.reviewer,"snapshot":r.snapshot,"clear":r.clear,"findings":r.findings,"explanation":r.explanation})).collect::<Vec<_>>() }),
        )?;
        self.runtime.save_task(&self.task, self.next_id, None)?;
        self.runtime.update(|record| {
            record.reviewer_identity = Some(runtime::Identity::from(&config));
            Ok(())
        })?;
        self.runtime.begin_phase("review", None)?;
        let decision = {
            let run = tokio::time::timeout(
                self.runtime.remaining()?,
                review::run(&config, &self.workspace, payload.clone(), events),
            );
            tokio::pin!(run);
            loop {
                tokio::select! {
                    biased;
                    command = commands.recv() => match command {
                        Some(Command::Cancel) => return Ok(TurnEnd::Cancelled),
                        Some(Command::Shutdown) | None => return Ok(TurnEnd::Shutdown),
                        Some(Command::Submit {reply, ..}) => { let _ = reply.send(Err("Review is running; draft retained. Cancel or wait for it to finish.")); },
                        Some(Command::Prompt(_)) => events.emit_advisory(Event::Error { message: "Review is running; submit after it stops.".into() })?,
                    },
                    result = &mut run => break result.context("cumulative task deadline exhausted")??,
                }
            }
        };
        ensure!(
            self.snapshot().await?.digest == after.digest,
            "workspace changed during review; review is stale"
        );
        let task = self.task.as_mut().expect("active review task");
        let receipt = state::ReviewReceipt {
            evidence: payload,
            snapshot: after.digest,
            verification_generation: task.verification_generation,
            reviewer: format!(
                "{} / {}",
                config.adapter,
                config.model.as_deref().unwrap_or("backend-default")
            ),
            clear: decision.verdict == review::Verdict::Clear,
            findings: decision.findings,
            explanation: decision.explanation,
        };
        task.review = Some(receipt.clone());
        self.runtime.save_task(&self.task, self.next_id, None)?;
        events
            .emit(Event::Text {
                text: format!(
                    "\nReview: {}\n{}\n{}\n",
                    if receipt.clear { "clear" } else { "blocked" },
                    receipt.explanation,
                    receipt.findings.join("\n")
                ),
            })
            .await?;
        Ok(TurnEnd::Complete)
    }
}

#[async_trait]
impl Session for WorkflowSession {
    fn admit(&mut self, prompt: &str) -> Result<()> {
        self.admitted_creator = None;
        let task = self.runtime.record()?.task;
        let starts_task = prompt.starts_with("/task ") || prompt.starts_with("/improve ");
        if ((task.is_none() && !is_control(prompt))
            || (starts_task && task.as_ref().is_none_or(|task| task.accepted.is_some())))
            && let Some(settings) = &self.live_settings
        {
            self.admitted_creator = Some(settings.creator(&self.connection)?);
        }
        Ok(())
    }
    fn owner(&self) -> &'static str {
        self.inner.owner()
    }
    fn initial_events(&self) -> Result<Vec<Event>> {
        let record = self.runtime.record()?;
        let mut events = vec![Event::SessionRecord {
            path: self.runtime.directory()?.display().to_string(),
            resumed: self.resumed,
        }];
        if self.resumed {
            events.extend(record.messages.into_iter().map(|m| Event::RetainedMessage {
                role: m.role,
                text: m.text,
            }));
            events.extend(
                record
                    .operations
                    .iter()
                    .filter(|operation| !operation.phase.starts_with("agent:"))
                    .filter_map(|o| o.result.clone())
                    .map(|result| Event::RetainedTool { result }),
            );
            if record.recovery_pending {
                events.push(Event::Error {message:"Interrupted work has uncertain results. Inspect the workspace and use /reconcile EXPLANATION before continuing. Nothing was replayed.".into()});
            }
        }
        Ok(events)
    }
    async fn turn(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        // Developer integration can invalidate acceptance between parent turns.
        let record = self.runtime.record()?;
        self.task = record.task;
        self.next_id = record.next_task;
        let starts_task = prompt.starts_with("/task ") || prompt.starts_with("/improve ");
        if (self.task.is_none() && !is_control(&prompt))
            || (starts_task
                && self
                    .task
                    .as_ref()
                    .is_none_or(|task| task.accepted.is_some()))
        {
            self.refresh_creator(events).await?;
        }
        let events = match &self.connection_label {
            Some(label) => events.for_connection(label),
            None => events.clone(),
        }
        .with_identity(&self.connection);
        let outcome_candidate = if matches!(
            prompt.trim(),
            "/verify" | "/review" | "/correct" | "/accept" | "/abandon"
        ) {
            self.task
                .as_ref()
                .and_then(|task| task.improvement.as_ref())
                .map(|link| link.candidate)
        } else {
            None
        };
        let result = self.dispatch(prompt, commands, &events).await;
        self.runtime.save_task(&self.task, self.next_id, None)?;
        self.runtime.finish_phase()?;
        if let Some(candidate) = outcome_candidate
            && !matches!(&result, Ok(TurnEnd::Cancelled | TurnEnd::Shutdown))
            && let Some(end) = self
                .retain_improvement_outcome(candidate, commands, &events)
                .await?
        {
            return Ok(end);
        }
        if self.runtime.record()?.recovery_pending {
            events.emit_advisory(Event::Error {message:"An interrupted operation may have partial effects. Inspect the workspace and use /reconcile EXPLANATION before continuing; nothing will be replayed automatically.".into()})?;
        }
        self.publish(&events).await?;
        result
    }
    async fn close(&mut self) -> Result<()> {
        self.inner.close().await
    }
    async fn cancel_background(&mut self) -> Result<()> {
        if self.runtime.record()?.delegation.is_none() {
            return Ok(());
        }
        self.inner.settle_interruption()?;
        if let Some(checkpoint) = self.inner.checkpoint() {
            self.runtime.checkpoint(checkpoint)?;
        }
        if let Some(task) = &mut self.task {
            task.stopped = true;
        }
        self.runtime.save_task(&self.task, self.next_id, None)?;
        self.runtime.finish_phase()?;
        self.inner.close().await
    }
}

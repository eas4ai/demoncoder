//! Read-only terminal projections of the authoritative runtime record.
mod pager;
pub(crate) mod poller;
mod report;
#[cfg(test)]
pub(crate) mod tests;

use crate::{
    subagents::state::{AgentStatus, OrchestrationStage},
    workflow::{runtime::Record, state::Task},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Target {
    #[default]
    Overview,
    Task(u64),
    Agent(u64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Request {
    pub target: Target,
    pub page: usize,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Page {
    pub request: Request,
    pub text: String,
    pub more: bool,
}

impl Page {
    pub fn message(&self) -> String {
        format!(
            "Inspection · {} · Page {}\nFreshness: saved evidence; files not rechecked. Actions recheck before acceptance or integration.\n{}\n{}",
            self.request.target.label(),
            self.request.page.saturating_add(1),
            self.text,
            if self.more {
                "More evidence: F2 inspection · Left/Right pages."
            } else {
                "End of saved evidence. F2 opens inspection."
            }
        )
    }
}

impl Target {
    pub fn label(self) -> String {
        match self {
            Self::Overview => "Overview".into(),
            Self::Task(id) => format!("Task {id}"),
            Self::Agent(id) => format!("Agent {id}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Summary {
    pub active: usize,
    pub waiting: usize,
    pub held: usize,
    pub ready: usize,
    pub task: Option<String>,
    pub recovery: bool,
    pub targets: Vec<Target>,
}

impl Summary {
    pub fn from_record(record: &Record) -> Self {
        let mut summary = Self {
            task: record
                .task
                .as_ref()
                .map(|task| task_line(task, record.last_snapshot.as_deref())),
            recovery: record.recovery_pending,
            targets: vec![Target::Overview],
            ..Self::default()
        };
        if let Some(task) = &record.task {
            summary.targets.push(Target::Task(task.id));
        }
        for agent in &record.agents {
            summary.targets.push(Target::Agent(agent.id));
            if agent.status.active() {
                summary.active += 1;
            } else {
                match agent.status {
                    AgentStatus::Queued => summary.waiting += 1,
                    AgentStatus::Ready => summary.ready += 1,
                    AgentStatus::Integrated => {}
                    _ => summary.held += 1,
                }
            }
        }
        for archived in record.archived.iter().rev() {
            summary.targets.push(Target::Task(archived.task.id));
        }
        summary
    }

    pub fn counts(&self) -> String {
        format!(
            "agents {} active · {} waiting · {} held · {} ready",
            self.active, self.waiting, self.held, self.ready
        )
    }
}

pub(crate) fn task_line(task: &Task, snapshot: Option<&str>) -> String {
    let verified = snapshot.is_some_and(|digest| task.verified(digest));
    let reviewed = snapshot.is_some_and(|digest| task.reviewed(digest));
    let checks = if verified {
        "passed"
    } else if task.checks.iter().any(|check| !check.success) {
        "failed"
    } else if task.checks.is_empty() {
        "unverified"
    } else {
        "stale/incomplete"
    };
    let review = if reviewed {
        "clear"
    } else if task
        .review
        .as_ref()
        .is_some_and(|review| !review.clear || !review.findings.is_empty())
    {
        "blocked"
    } else if task.review.is_none() {
        "not reviewed"
    } else {
        "stale"
    };
    let accepted = if task
        .accepted
        .as_deref()
        .is_some_and(|digest| Some(digest) == snapshot)
    {
        "yes"
    } else {
        "no"
    };
    format!(
        "Task {} · Work {} · Checks {checks} · Review {review} · Accepted {accepted} (recorded)",
        task.id,
        if task.stopped { "stopped" } else { "running" }
    )
}

pub(crate) fn agent_stage(status: AgentStatus, stage: Option<OrchestrationStage>) -> String {
    match stage {
        Some(stage) if status.active() => format!("{status:?} · {stage:?}"),
        _ => format!("{status:?}"),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub summary: Summary,
    pub page: Option<Page>,
}

pub(crate) fn project(record: &Record, request: Option<Request>) -> Snapshot {
    Snapshot {
        summary: Summary::from_record(record),
        page: request.map(|request| report::page(record, request)),
    }
}

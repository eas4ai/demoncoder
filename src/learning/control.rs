//! Explicit developer controls and cancellation-aware, bounded blocking work.
use super::{
    catalog::CatalogStore,
    state::{LessonProposal, Proposal},
};
use crate::{
    events::{Event, EventSink},
    session::{Command, TurnEnd},
    workflow::runtime::SharedRuntime,
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};
use tokio::sync::{Semaphore, mpsc};

static IO_SLOTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(4)));

pub async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    let permit = IO_SLOTS
        .clone()
        .try_acquire_owned()
        .context("learning I/O is busy; retry after pending operations stop")?;
    tokio::time::timeout(Duration::from_secs(10), tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })).await.context("learning I/O deadline reached; a pending save may complete. Inspect before repeating; no coding work was started")?
        .context("learning I/O worker failed")?
}

pub async fn cancellable<T>(
    work: impl std::future::Future<Output = Result<T>>,
    commands: &mut mpsc::Receiver<Command>,
    events: &EventSink,
) -> Result<std::result::Result<T, TurnEnd>> {
    tokio::pin!(work);
    loop {
        tokio::select! {
            biased;
            command = commands.recv() => match command {
                Some(Command::Cancel) => {
                    events.emit(Event::Text { text: "\nLearning operation cancelled. A pending catalog save may finish; inspect before repeating. No correction starts from this cancellation.\n".into() }).await?;
                    return Ok(Err(TurnEnd::Cancelled));
                }
                Some(Command::Shutdown) => return Ok(Err(TurnEnd::Shutdown)),
                None => return Ok(Err(TurnEnd::CommandsClosed)),
                Some(Command::Submit { reply, .. }) => { let _ = reply.send(Err("Learning operation is running; draft retained. Cancel or wait.")); },
                Some(Command::Prompt(_)) => events.emit_advisory(Event::Error { message: "Learning operation is running; submit after it stops.".into() })?,
            },
            result = &mut work => return Ok(Ok(result?)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct View {
    pub title: String,
    pub text: String,
}

pub enum Request {
    Discover,
    Inspect(u64, usize),
    Observation(u64, usize),
    Note(u64, String),
    NoteSource(String, String),
    Propose(u64, Proposal),
    Outcome(u64),
    LessonPropose(u64, LessonProposal),
    LessonInspect(u64, usize),
    Enable(u64, bool),
    Supersede(u64, u64),
}

fn id(raw: &str) -> Result<u64> {
    let id = raw
        .parse::<u64>()
        .context("expected a positive retained identifier")?;
    ensure!(id > 0, "identifier must be positive");
    Ok(id)
}

pub fn improvement_id(prompt: &str) -> Result<u64> {
    let parts: Vec<_> = prompt.split_whitespace().collect();
    ensure!(parts.len() == 2, "use /improve CANDIDATE_ID");
    id(parts[1])
}

impl Request {
    pub fn parse(prompt: &str) -> Result<Self> {
        ensure!(prompt.len() <= 32 * 1024, "learning command exceeds 32 KiB");
        let (command, rest) = prompt
            .trim()
            .split_once(char::is_whitespace)
            .unwrap_or((prompt.trim(), ""));
        let rest = rest.trim();
        if command == "/improvements" {
            ensure!(rest.is_empty(), "use /improvements without arguments");
            return Ok(Self::Discover);
        }
        let (number, body) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let body = body.trim();
        if command == "/improvement-note" && number.contains(':') {
            super::state::text(number, 128, "source selector")?;
            super::state::text(body, 4096, "developer annotation")?;
            return Ok(Self::NoteSource(number.into(), body.into()));
        }
        let number = id(number)?;
        Ok(match command {
            "/improvement" | "/lesson" | "/observation" => {
                let page = if body.is_empty() { 0 } else {
                    let page = body.parse::<usize>().context("page must be a positive number")?;
                    ensure!((1..=16384).contains(&page), "page must be between 1 and 16384");
                    page - 1
                };
                if command == "/observation" { Self::Observation(number, page) }
                else if command == "/lesson" { Self::LessonInspect(number, page) } else { Self::Inspect(number, page) }
            }
            "/improvement-note" => Self::Note(number, body.into()),
            "/improvement-propose" => Self::Propose(number, serde_json::from_str(body).map_err(|_| anyhow::anyhow!("proposal JSON requires objective, scope, benefit, behavioral_check and risks"))?),
            "/lesson-propose" => Self::LessonPropose(number, serde_json::from_str(body).map_err(|_| anyhow::anyhow!("lesson JSON requires claim and keywords"))?),
            "/lesson-supersede" => Self::Supersede(number, id(body)?),
            _ => {
                ensure!(body.is_empty(), "unexpected command arguments");
                match command {
                    "/improvement-outcome" => Self::Outcome(number),
                    "/lesson-enable" => Self::Enable(number, true),
                    "/lesson-disable" => Self::Enable(number, false),
                    _ => bail!("unknown learning command"),
                }
            }
        })
    }

    pub fn needs_snapshot(&self) -> bool {
        matches!(self, Self::Outcome(_))
    }

    pub fn run(self, runtime: &SharedRuntime, snapshot: Option<String>) -> Result<(View, usize)> {
        let write = !matches!(
            self,
            Self::Inspect(..) | Self::LessonInspect(..) | Self::Observation(..)
        );
        let mut store = CatalogStore::open(runtime, write)?;
        let (notice, focus, page) = match self {
            Self::Observation(id, page) => {
                let observation = store.catalog.observation(id)?.clone();
                let original = match store.resolver.resolve(&observation.source) {
                    Ok(original) => serde_json::to_string_pretty(&original)?,
                    Err(error) => {
                        format!("Source unavailable; dependent claims are blocked: {error:#}")
                    }
                };
                let text = format!(
                    "Workspace: {}\nFreshness: saved evidence; files not rechecked.\nObservation and attributed annotations:\n{}\nOriginal cited receipt (quoted evidence):\n{original}\n",
                    store.catalog.workspace.path.display(),
                    serde_json::to_string_pretty(&observation)?
                );
                ensure!(
                    text.len() <= 8 * 1024 * 1024,
                    "observation inspection exceeds 8 MiB; inspect the original source"
                );
                return Ok((
                    View {
                        title: format!("Observation {id}"),
                        text,
                    },
                    page,
                ));
            }
            Self::Discover => {
                let added = store.discover()?;
                (
                    format!(
                        "Detected {added} new failed-check observations. Discovery made no model or tool calls."
                    ),
                    None,
                    0,
                )
            }
            Self::Inspect(id, page) => (String::new(), Some((false, id)), page),
            Self::Note(id, note) => {
                store.annotate(id, note)?;
                (
                    format!(
                        "Developer annotation retained for observation {id}. Original result unchanged."
                    ),
                    None,
                    0,
                )
            }
            Self::NoteSource(selector, note) => {
                let source = super::source::select(
                    store.resolver.record(&store.session)?,
                    &store.session,
                    &selector,
                )?;
                let original = store.resolver.resolve(&source)?;
                let failed =
                    original.get("success").and_then(serde_json::Value::as_bool) == Some(false);
                let snapshot = original
                    .get("snapshot")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let id = store.observe(source, failed, snapshot)?;
                store.annotate(id, note)?;
                (
                    format!(
                        "Developer annotation retained for observation {id}, citing {selector}. Original result unchanged."
                    ),
                    None,
                    0,
                )
            }
            Self::Propose(id, proposal) => {
                let candidate =
                    store.propose(id, proposal, "developer proposal (explanation unproven)")?;
                (
                    format!("Candidate {candidate} proposed. No correction authorized."),
                    Some((false, candidate)),
                    0,
                )
            }
            Self::Outcome(id) => {
                let result = store.outcome(
                    id,
                    snapshot
                        .as_deref()
                        .context("outcome requires a current workspace capture")?,
                )?;
                (
                    format!(
                        "Candidate {id} outcome: {:?}. {}",
                        result.status, result.reason
                    ),
                    Some((false, id)),
                    0,
                )
            }
            Self::LessonPropose(id, proposal) => {
                let lesson = store.propose_lesson(id, proposal)?;
                (
                    format!(
                        "Lesson {lesson} proposed and disabled. Use /lesson-enable {lesson} to approve future use."
                    ),
                    Some((true, lesson)),
                    0,
                )
            }
            Self::LessonInspect(id, page) => (String::new(), Some((true, id)), page),
            Self::Enable(id, enabled) => {
                store.enable(id, enabled)?;
                (
                    format!(
                        "Lesson {id} {} for future selection.",
                        if enabled {
                            "enabled by developer"
                        } else {
                            "disabled"
                        }
                    ),
                    Some((true, id)),
                    0,
                )
            }
            Self::Supersede(old, new) => {
                store.supersede(old, new)?;
                (
                    format!("Lesson {old} superseded by {new}. Source history retained."),
                    Some((true, old)),
                    0,
                )
            }
        };
        let mut view = render(&mut store, focus)?;
        view.text.insert_str(0, &format!("{notice}\n"));
        if write {
            store.save()?;
        }
        Ok((view, page))
    }
}

fn render(store: &mut CatalogStore, focus: Option<(bool, u64)>) -> Result<View> {
    use std::fmt::Write;
    let mut out = String::new();
    writeln!(
        out,
        "Workspace: {}\nFreshness: saved evidence; files not rechecked by inspection.\nCatalog: {}",
        store.catalog.workspace.path.display(),
        store.directory().display()
    )?;
    let title = if let Some((lesson, id)) = focus {
        let candidate = if lesson {
            let lesson = store.catalog.lesson(id)?;
            writeln!(
                out,
                "Lesson {id} (developer claim; not execution authority)\n{}",
                serde_json::to_string_pretty(lesson)?
            )?;
            lesson.candidate
        } else {
            id
        };
        let candidate = store.catalog.candidate(candidate)?.clone();
        let observation = store.catalog.observation(candidate.observation)?.clone();
        writeln!(
            out,
            "Candidate {} (proposed explanation)\n{}\nObservation {}\n{}",
            candidate.id,
            serde_json::to_string_pretty(&candidate)?,
            observation.id,
            serde_json::to_string_pretty(&observation)?
        )?;
        match store.resolver.resolve(&observation.source) {
            Ok(original) => writeln!(
                out,
                "Original cited receipt (quoted evidence)\n{}",
                serde_json::to_string_pretty(&original)?
            )?,
            Err(error) => writeln!(
                out,
                "Source unavailable; dependent claims are blocked: {error:#}"
            )?,
        }
        for outcome in &candidate.outcomes {
            for source in outcome.checks.iter().chain(outcome.review.iter()) {
                match store.resolver.resolve(source) {
                    Ok(original) => writeln!(
                        out,
                        "Original correction receipt {}\n{}",
                        serde_json::to_string(source)?,
                        serde_json::to_string_pretty(&original)?
                    )?,
                    Err(error) => writeln!(
                        out,
                        "Correction source unavailable; dependent claims blocked: {error:#}"
                    )?,
                }
                ensure!(
                    out.len() <= 8 * 1024 * 1024,
                    "inspection exceeds 8 MiB; use the cited original session inspector"
                );
            }
        }
        if lesson {
            format!("Lesson {id}")
        } else {
            format!("Candidate {id}")
        }
    } else {
        writeln!(
            out,
            "Observations: {} · Candidates: {} · Lessons: {}",
            store.catalog.observations.len(),
            store.catalog.candidates.len(),
            store.catalog.lessons.len()
        )?;
        for observation in &store.catalog.observations {
            writeln!(
                out,
                "Observation {} · failed check {} · annotations {} · citation {}",
                observation.id,
                observation.failed_check,
                observation.annotations.len(),
                serde_json::to_string(&observation.source)?
            )?;
        }
        for candidate in &store.catalog.candidates {
            writeln!(
                out,
                "Candidate {} · observation {} · {} · latest outcome {:?}\n  Proposed objective: {:?}",
                candidate.id,
                candidate.observation,
                if candidate.authorization.is_some() {
                    "authorization retained; inspect before continuing"
                } else {
                    "not authorized"
                },
                candidate.outcomes.last().map(|o| &o.status),
                candidate.proposal.objective
            )?;
        }
        for lesson in &store.catalog.lessons {
            writeln!(
                out,
                "Lesson {} · enabled {} · superseded by {:?} · applicability {:?}",
                lesson.id, lesson.enabled, lesson.superseded_by, lesson.proposal.keywords
            )?;
        }
        "Improvements".into()
    };
    writeln!(
        out,
        "\nDeveloper controls: /improvement ID [PAGE], /improvement-note OBS TEXT, /improvement-propose OBS JSON, /improve ID, /improvement-outcome ID, /lesson-propose CANDIDATE JSON, /lesson ID [PAGE], /lesson-enable ID, /lesson-disable ID, /lesson-supersede OLD NEW. These printed examples never execute themselves."
    )?;
    ensure!(
        out.len() <= 8 * 1024 * 1024,
        "inspection exceeds 8 MiB; inspect original sources separately"
    );
    Ok(View { title, text: out })
}

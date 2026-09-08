//! Learning controls reuse ordinary task admission and never create a second effect path.
use super::{WorkflowSession, state};
use crate::{
    events::{Event, EventSink},
    session::{Command, TurnEnd},
};
use anyhow::{Context, Result, ensure};
use tokio::sync::mpsc;

impl WorkflowSession {
    pub(super) async fn learning_control(
        &mut self,
        prompt: String,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<TurnEnd> {
        if prompt.split_whitespace().next() == Some("/learning-context") {
            let parts: Vec<_> = prompt.split_whitespace().collect();
            ensure!(parts.len() <= 2, "use /learning-context [PAGE]");
            let page = parts
                .get(1)
                .map(|p| p.parse::<usize>())
                .transpose()
                .context("page must be a positive number")?
                .unwrap_or(1);
            ensure!(
                (1..=16384).contains(&page),
                "page must be between 1 and 16384"
            );
            self.runtime.clear_learning_view()?;
            let page = self
                .runtime
                .inspection(Some(crate::inspection::Request {
                    target: crate::inspection::Target::Learning,
                    page: page - 1,
                    ..Default::default()
                }))?
                .and_then(|snapshot| snapshot.page)
                .context("context inspection is busy; retry")?;
            events
                .emit(Event::Text {
                    text: page.message(),
                })
                .await?;
            return Ok(TurnEnd::Complete);
        }
        let request = crate::learning::control::Request::parse(&prompt)?;
        let snapshot = if request.needs_snapshot() {
            Some(self.snapshot().await?.digest)
        } else {
            None
        };
        let runtime = self.runtime.clone();
        let work = crate::learning::control::blocking(move || request.run(&runtime, snapshot));
        let (view, page) =
            match crate::learning::control::cancellable(work, commands, events).await? {
                Ok(result) => result,
                Err(end) => return Ok(end),
            };
        let page = crate::inspection::learning_page(
            &view,
            crate::inspection::Request {
                target: crate::inspection::Target::Learning,
                page,
                ..Default::default()
            },
        );
        self.runtime.learning_view(view)?;
        events
            .emit(Event::Text {
                text: page.message(),
            })
            .await?;
        Ok(TurnEnd::Complete)
    }

    pub(super) async fn reserve_improvement(
        &mut self,
        candidate: u64,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<std::result::Result<(String, Option<state::ImprovementLink>), TurnEnd>> {
        let runtime = self.runtime.clone();
        let task_id = self.next_id;
        let checks = self.settings.checks.clone();
        let work = crate::learning::control::blocking(move || {
            let mut store = crate::learning::catalog::CatalogStore::open(&runtime, true)?;
            let objective = store.reserve(candidate, task_id, &checks)?;
            Ok((
                objective,
                Some(state::ImprovementLink {
                    catalog: store.directory().into(),
                    candidate,
                }),
            ))
        });
        crate::learning::control::cancellable(work, commands, events).await
    }

    pub(super) async fn retain_improvement_outcome(
        &mut self,
        candidate: u64,
        commands: &mut mpsc::Receiver<Command>,
        events: &EventSink,
    ) -> Result<Option<TurnEnd>> {
        let snapshot = self.snapshot().await;
        match snapshot {
            Ok(snapshot) => {
                let runtime = self.runtime.clone();
                let operation = crate::learning::control::blocking(move || {
                    let mut store = crate::learning::catalog::CatalogStore::open(&runtime, true)?;
                    let outcome = store.outcome(candidate, &snapshot.digest)?;
                    store.save()?;
                    Ok(outcome)
                });
                match crate::learning::control::cancellable(operation, commands, events).await {
                    Ok(Ok(outcome)) => events.emit(Event::Text { text: format!("\nCandidate {candidate} saved outcome: {:?}. {}\n", outcome.status, outcome.reason) }).await?,
                    Ok(Err(end)) => return Ok(Some(end)),
                    Err(error) => events.emit(Event::Error { message: format!("Correction evidence was retained, but its learning outcome is incomplete: {error:#}") }).await?,
                }
            }
            Err(error) => {
                events
                    .emit(Event::Error {
                        message: format!(
                            "Learning outcome cannot establish current files: {error:#}"
                        ),
                    })
                    .await?
            }
        }
        Ok(None)
    }
}

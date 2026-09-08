use super::{
    Assignment, Assignments, Role,
    probe::{self, Catalog},
    provider_label,
};
use crate::config::Config;
use anyhow::{Context, Result, bail, ensure};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use futures_util::FutureExt;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};
use tokio::task::JoinHandle;

enum Page {
    Providers,
    Models(Role),
    Roles,
    Key(usize),
}

struct Provider {
    name: String,
    selected: bool,
    status: String,
    catalog: Option<Catalog>,
    job: Option<JoinHandle<Result<Catalog>>>,
}

impl Drop for Provider {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.abort();
        }
    }
}

pub(crate) enum Action {
    Stay,
    Cancel,
    Save,
}

pub(crate) struct Editor {
    pub config: Config,
    assignments: Assignments,
    providers: Vec<Provider>,
    page: Page,
    cursor: usize,
    key: String,
    pub notice: String,
    onboarding: bool,
}

impl Editor {
    pub fn paste(&mut self, text: &str) {
        if matches!(self.page, Page::Key(_)) {
            let value: String = text.chars().filter(|c| !c.is_control()).collect();
            if self.key.len() + value.len() <= 4096 {
                self.key.push_str(&value);
            } else {
                self.notice = "API key exceeds 4096 bytes; paste was not applied.".into();
            }
        }
    }
    pub fn new(mut config: Config, onboarding: bool) -> Result<Self> {
        ensure!(
            config.connections.len() <= 64,
            "Settings supports at most 64 configured connections"
        );
        let assignments = Assignments::from_config(&config);
        for adapter in ["codex", "claude", "openai-api", "anthropic-api"] {
            // Optional choices must not make our own saved file impossible to reopen.
            if config.connections.len() == 64 {
                break;
            }
            if !config.connections.values().any(|c| c.adapter == adapter) {
                let mut name = adapter.to_owned();
                while config.connections.contains_key(&name) {
                    name.push('_');
                }
                config.connections.insert(
                    name,
                    serde_json::from_value(serde_json::json!({"adapter": adapter}))?,
                );
            }
        }
        let providers = config
            .connections
            .keys()
            .map(|name| Provider {
                name: name.clone(),
                selected: assignments.providers.contains(name),
                status: "Not checked · select and press r to check".into(),
                catalog: None,
                job: None,
            })
            .collect();
        Ok(Self {
            config,
            assignments,
            providers,
            page: Page::Providers,
            cursor: 0,
            key: String::new(),
            notice: String::new(),
            onboarding,
        })
    }

    pub fn poll(&mut self) {
        for provider in &mut self.providers {
            if provider.job.as_ref().is_some_and(|job| job.is_finished()) {
                let result = provider.job.take().expect("finished job").now_or_never();
                match result {
                    Some(Ok(Ok(catalog))) => {
                        provider.status = format!("Authenticated · {}", catalog.note);
                        provider.catalog = Some(catalog);
                    }
                    Some(Ok(Err(error))) => provider.status = format!("Unavailable · {error}"),
                    _ => provider.status = "Unavailable · check stopped; press r to retry".into(),
                }
            }
        }
    }

    fn start(&mut self, index: usize) {
        let provider = &mut self.providers[index];
        if let Some(job) = provider.job.take() {
            job.abort();
        }
        provider.catalog = None;
        let connection = self.config.connections[&provider.name].clone();
        provider.status = "Checking · Escape cancels".into();
        provider.job = Some(tokio::spawn(async move { probe::check(&connection).await }));
    }

    fn authenticate(&mut self, index: usize) {
        let connection = &self.config.connections[&self.providers[index].name];
        let variable = match connection.adapter.as_str() {
            "openai-api" => Some("OPENAI_API_KEY"),
            "anthropic-api" => Some("ANTHROPIC_API_KEY"),
            _ => None,
        };
        if variable.is_some_and(|v| connection.api_key(v).is_err()) {
            self.key.clear();
            self.page = Page::Key(index);
        } else {
            self.start(index);
        }
    }

    fn choices(&self) -> Vec<Assignment> {
        self.providers
            .iter()
            .filter(|p| p.selected)
            .flat_map(|p| {
                p.catalog
                    .iter()
                    .flat_map(|catalog| catalog.models.iter())
                    .map(|model| Assignment {
                        connection: p.name.clone(),
                        model: Some(model.clone()),
                        effort: self.config.connections[&p.name].effort.clone(),
                    })
            })
            .collect()
    }

    fn open_models(&mut self, role: Role) {
        let current = self.assignments.assignment(role);
        let offset = usize::from(role != Role::Creator);
        self.cursor = self
            .choices()
            .iter()
            .position(|a| Some(a) == current)
            .map_or(0, |i| i + offset);
        if role != Role::Creator && !self.assignments.overrides.contains_key(&role) {
            self.cursor = 0;
        }
        self.page = Page::Models(role);
    }

    fn apply(&mut self) -> Result<()> {
        self.assignments.providers = self
            .providers
            .iter()
            .filter(|p| p.selected)
            .map(|p| p.name.clone())
            .collect();
        self.assignments.validate(&self.config)?;
        for role in Role::ALL {
            if let Some(assignment) = self.assignments.assignment(role)
                && let Some(provider) = self
                    .providers
                    .iter()
                    .find(|p| p.name == assignment.connection && p.selected)
                && let Some(catalog) = &provider.catalog
            {
                ensure!(
                    assignment
                        .model
                        .as_ref()
                        .is_some_and(|model| catalog.models.contains(model)),
                    "{} model is unavailable in the checked catalog; select a listed model",
                    role.name()
                );
            }
        }
        if self.onboarding {
            for role in Role::ALL {
                self.assignments.resolve(&self.config, role)?;
            }
        }
        if let Some(creator) = &self.assignments.creator {
            self.config.default_connection = Some(creator.connection.clone());
            let connection = self
                .config
                .connections
                .get_mut(&creator.connection)
                .context("Creator connection missing")?;
            connection.model = creator.model.clone();
            connection.effort = creator.effort.clone();
        }
        self.config.settings = Some(self.assignments.clone());
        self.config.onboarding_complete = true;
        Ok(())
    }

    pub fn key(&mut self, key: KeyEvent) -> Result<Action> {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'q' | 'd'))
        {
            return Ok(Action::Cancel);
        }
        if let Page::Key(index) = self.page {
            match key.code {
                KeyCode::Esc => {
                    self.key.clear();
                    self.page = Page::Providers;
                    self.providers[index].status = "Cancelled · press r to retry".into();
                }
                KeyCode::Enter => {
                    let name = self.providers[index].name.clone();
                    if !self.key.trim().is_empty() {
                        self.config
                            .connections
                            .get_mut(&name)
                            .expect("provider")
                            .api_key = Some(std::mem::take(&mut self.key));
                    }
                    self.page = Page::Providers;
                    self.start(index);
                }
                KeyCode::Backspace => {
                    self.key.pop();
                }
                KeyCode::Char(c)
                    if !c.is_control()
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && self.key.len() + c.len_utf8() <= 4096 =>
                {
                    self.key.push(c);
                }
                _ => {}
            }
            return Ok(Action::Stay);
        }
        if key.code == KeyCode::Esc {
            if let Some(provider) = self.providers.iter_mut().find(|p| p.job.is_some()) {
                provider.job.take().expect("job").abort();
                provider.status = "Cancelled · press r to retry".into();
                return Ok(Action::Stay);
            }
            return match self.page {
                Page::Models(_) => {
                    self.page = Page::Roles;
                    self.cursor = 0;
                    Ok(Action::Stay)
                }
                Page::Roles => {
                    self.page = Page::Providers;
                    self.cursor = 0;
                    Ok(Action::Stay)
                }
                _ => Ok(Action::Cancel),
            };
        }
        let count = match self.page {
            Page::Providers => self.providers.len() + 1,
            Page::Roles => Role::ALL.len() + 2,
            Page::Models(role) => self.choices().len() + usize::from(role != Role::Creator),
            Page::Key(_) => unreachable!(),
        };
        match key.code {
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => {
                self.cursor = (self.cursor + 1).min(count.saturating_sub(1))
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = count.saturating_sub(1),
            _ => {}
        }
        match self.page {
            Page::Providers => {
                if self.cursor < self.providers.len() {
                    match key.code {
                        KeyCode::Char(' ') => {
                            let p = &mut self.providers[self.cursor];
                            p.selected = !p.selected;
                            if p.selected {
                                self.authenticate(self.cursor);
                            } else {
                                if let Some(job) = p.job.take() {
                                    job.abort();
                                }
                                p.catalog = None;
                                p.status = "Not selected".into();
                            }
                        }
                        KeyCode::Char('r') if self.providers[self.cursor].selected => {
                            self.authenticate(self.cursor)
                        }
                        KeyCode::Char('k')
                            if self.providers[self.cursor].selected
                                && self.config.connections[&self.providers[self.cursor].name]
                                    .adapter
                                    .ends_with("-api") =>
                        {
                            self.key.clear();
                            self.page = Page::Key(self.cursor);
                        }
                        _ => {}
                    }
                } else if key.code == KeyCode::Enter {
                    if !self.providers.iter().any(|p| p.selected) {
                        self.notice = "Select at least one provider.".into();
                    } else if self
                        .providers
                        .iter()
                        .any(|p| p.selected && p.catalog.is_none())
                    {
                        self.notice =
                            "Check each selected provider, or deselect unavailable choices.".into();
                        let unchecked: Vec<_> = self
                            .providers
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| p.selected && p.catalog.is_none() && p.job.is_none())
                            .map(|(i, _)| i)
                            .collect();
                        for i in unchecked {
                            self.authenticate(i);
                            if matches!(self.page, Page::Key(_)) {
                                break;
                            }
                        }
                    } else {
                        self.notice.clear();
                        if self.assignments.creator.is_none() {
                            self.open_models(Role::Creator);
                        } else {
                            self.page = Page::Roles;
                            self.cursor = 0;
                        }
                    }
                }
            }
            Page::Models(role) if key.code == KeyCode::Enter => {
                if role != Role::Creator && self.cursor == 0 {
                    self.assignments.overrides.remove(&role);
                } else if let Some(assignment) = self
                    .choices()
                    .get(
                        self.cursor
                            .saturating_sub(usize::from(role != Role::Creator)),
                    )
                    .cloned()
                {
                    if role == Role::Creator {
                        self.assignments.creator = Some(assignment);
                    } else {
                        self.assignments.overrides.insert(role, assignment);
                    }
                } else {
                    self.notice = "No authenticated model choices available.".into();
                    return Ok(Action::Stay);
                }
                self.page = Page::Roles;
                self.cursor = Role::ALL.iter().position(|r| *r == role).unwrap_or(0);
            }
            Page::Roles if key.code == KeyCode::Enter => {
                if let Some(role) = Role::ALL.get(self.cursor) {
                    self.open_models(*role);
                } else if self.cursor == Role::ALL.len() {
                    self.page = Page::Providers;
                    self.cursor = 0;
                } else {
                    match self.apply() {
                        Ok(()) => return Ok(Action::Save),
                        Err(error) => self.notice = error.to_string(),
                    }
                }
            }
            _ => {}
        }
        Ok(Action::Stay)
    }

    fn assignment_label(&self, assignment: &Assignment) -> String {
        let adapter = self
            .config
            .connections
            .get(&assignment.connection)
            .map(|c| provider_label(&c.adapter))
            .unwrap_or("Missing provider");
        let unavailable = !self
            .providers
            .iter()
            .any(|p| p.name == assignment.connection && p.selected);
        format!(
            "{}{} · {adapter} ({})",
            if unavailable {
                "Unavailable — provider deselected · "
            } else {
                ""
            },
            assignment.model.as_deref().unwrap_or("backend-default"),
            assignment.connection
        )
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let title = match self.page {
            Page::Providers => "Settings · Providers".into(),
            Page::Roles => "Settings · Agent assignments".into(),
            Page::Models(role) => format!("Settings · {} model", role.name()),
            Page::Key(_) => "Settings · API key".into(),
        };
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let (hint, rows): (&str, Vec<String>) = match self.page {
            Page::Providers => (
                "Space toggles · r checks login · k edits key · End then Enter continues · Escape cancels",
                self.providers
                    .iter()
                    .map(|p| {
                        format!(
                            "[{}] {} ({}) — {}",
                            if p.selected { "x" } else { " " },
                            provider_label(&self.config.connections[&p.name].adapter),
                            p.name,
                            p.status
                        )
                    })
                    .chain(std::iter::once("Continue".into()))
                    .collect(),
            ),
            Page::Roles => (
                "Enter selects · Escape returns · unchanged roles use Creator",
                Role::ALL
                    .iter()
                    .map(|role| {
                        let inherited = *role != Role::Creator
                            && !self.assignments.overrides.contains_key(role);
                        let value = self
                            .assignments
                            .assignment(*role)
                            .map(|a| self.assignment_label(a))
                            .unwrap_or("Choose a model".into());
                        format!(
                            "{} — {}{} · {}",
                            role.name(),
                            if inherited {
                                "Use Creator model → "
                            } else {
                                ""
                            },
                            value,
                            role.description()
                        )
                    })
                    .chain([
                        "Providers".into(),
                        if self.onboarding {
                            "Continue with these assignments".into()
                        } else {
                            "Save and return to conversation".into()
                        },
                    ])
                    .collect(),
            ),
            Page::Models(role) => (
                "Enter assigns · Escape keeps the previous assignment",
                std::iter::once("Use Creator model".into())
                    .filter(|_| role != Role::Creator)
                    .chain(self.choices().iter().map(|a| {
                        format!(
                            "{}{}",
                            if self.assignments.assignment(role) == Some(a) {
                                "✓ "
                            } else {
                                ""
                            },
                            self.assignment_label(a)
                        )
                    }))
                    .collect(),
            ),
            Page::Key(_) => (
                "API key: Enter checks · Escape cancels · environment key takes precedence",
                vec!["*".repeat(self.key.chars().count().min(80))],
            ),
        };
        let height = inner.height.saturating_sub(5) as usize;
        let start = self.cursor.saturating_sub(height.saturating_sub(1));
        let mut lines = vec![Line::from(hint), Line::default()];
        for (index, row) in rows.into_iter().enumerate().skip(start).take(height) {
            let safe: String = row
                .chars()
                .map(|c| if c.is_control() { '�' } else { c })
                .collect();
            lines.push(Line::styled(
                format!("{} {safe}", if index == self.cursor { "›" } else { " " }),
                if index == self.cursor {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                },
            ));
        }
        lines.push(Line::default());
        lines.push(Line::from(self.notice.clone()));
        lines.push(Line::from(
            "Assignments grant no permissions. Saved changes apply to subsequent work.",
        ));
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

pub(crate) async fn onboarding(config: &mut Config) -> Result<()> {
    use crossterm::event::{Event, EventStream, KeyEventKind};
    use futures_util::StreamExt;
    let mut editor = Editor::new(config.clone(), true)?;
    let mut terminal = ratatui::try_init()?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
            ratatui::restore();
        }
    }
    let _restore = Restore;
    crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste)?;
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
    loop {
        editor.poll();
        terminal.draw(|f| editor.draw(f, f.area()))?;
        tokio::select! {
            _ = tick.tick() => {}
            event = events.next() => match event.context("setup input closed")?? {
                Event::Key(key) if key.kind == KeyEventKind::Press => match editor.key(key)? {
                    Action::Save => { *config = editor.config.clone(); return Ok(()); }
                    Action::Cancel => bail!("setup cancelled before saving"),
                    Action::Stay => {}
                },
                Event::Paste(text) => editor.paste(&text),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_near_capacity_keeps_existing_connections_and_can_reopen() {
        for count in 61..=64 {
            let connections: std::collections::BTreeMap<_, _> = (0..count)
                .map(|index| {
                    (
                        format!("saved-{index}"),
                        serde_json::json!({"adapter": "codex", "model": "existing-model"}),
                    )
                })
                .collect();
            let config: Config = serde_json::from_value(serde_json::json!({
                "default_connection": "saved-0", "connections": connections
            }))
            .unwrap();
            let mut editor = Editor::new(config.clone(), false).unwrap();
            editor.apply().unwrap();
            let persisted = toml::to_string(&editor.config).unwrap();
            let restored: Config = toml::from_str(&persisted).unwrap();
            let reopened = Editor::new(restored, false)
                .expect("a Settings save must remain within its own reopening limit");
            for (name, connection) in config.connections {
                assert_eq!(
                    serde_json::to_value(&reopened.config.connections[&name]).unwrap(),
                    serde_json::to_value(&connection).unwrap()
                );
            }
        }
    }
}

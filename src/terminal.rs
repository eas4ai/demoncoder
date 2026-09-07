use crate::{
    chat::{Anchor, Chat, Role},
    events::{ContextUsage, Envelope, Event},
    highlight::Source,
    selection::Selection,
    session::{Command, PromptAdmission},
    status::{DisplayOptions, GitPoller, GitStatus, context_text, usage_text},
};
use anyhow::{Context, Result, bail};
use crossterm::{
    clipboard::CopyToClipboard,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event as InputEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers,
        KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
};
use futures_util::StreamExt;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use std::{collections::BTreeMap, io::IsTerminal, path::Path, time::Duration};
use tokio::sync::{mpsc, oneshot};
use unicode_width::UnicodeWidthStr;

const MAX_INPUT_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct View {
    input: String,
    chat: Chat,
    assistant: Option<u64>,
    tools: BTreeMap<String, ToolActivity>,
    anchor: Option<Anchor>,
    chat_area: Rect,
    status: String,
    usage: String,
    options: DisplayOptions,
    context: ContextUsage,
    git: Option<Result<GitStatus, String>>,
    busy: bool,
    activity_tick: u64,
    selection: Option<Selection>,
    displayed: Option<Vec<ratatui::text::Line<'static>>>,
    rail: Option<ScrollRail>,
    drag_offset: Option<u16>,
    copy_notice: Option<&'static str>,
    pending_prompt: Option<PendingPrompt>,
    cancel_pending: bool,
}

struct PendingPrompt {
    text: String,
    reply: oneshot::Receiver<PromptAdmission>,
}

#[derive(Clone, Copy)]
struct ScrollRail {
    area: Rect,
    max_top: usize,
    thumb: u16,
    top: u16,
}

impl ScrollRail {
    fn new(area: Rect, length: usize, position: usize) -> Self {
        let height = usize::from(area.height);
        let max_top = length.saturating_sub(height);
        let thumb = (height * height / length.max(1)).clamp(1, height.max(1)) as u16;
        let travel = area.height.saturating_sub(thumb);
        let top = if max_top == 0 {
            0
        } else {
            ((position as u128 * u128::from(travel)) / max_top as u128) as u16
        };
        Self {
            area,
            max_top,
            thumb,
            top,
        }
    }
    fn position(self, row: u16, grab: u16) -> usize {
        let offset = row
            .saturating_sub(self.area.y)
            .saturating_sub(grab)
            .min(self.area.height.saturating_sub(self.thumb));
        let travel = self.area.height.saturating_sub(self.thumb);
        if travel == 0 {
            0
        } else {
            (u128::from(offset) * self.max_top as u128 / u128::from(travel)) as usize
        }
    }
}

struct ToolActivity {
    block: u64,
    name: String,
    target: String,
}

impl ToolActivity {
    fn title(&self, role: Role, exit: Option<i32>) -> String {
        let verb = match role {
            Role::Success => match self.name.as_str() {
                "read" => "Read",
                "write" => "Wrote",
                "edit" => "Edited",
                "bash" => "Ran",
                _ => "Finished",
            },
            Role::Failed => "Failed",
            Role::Stopped => "Stopped",
            _ => "Running",
        };
        let tool = if role == Role::Success {
            ""
        } else {
            &self.name
        };
        let detail = format!("{verb} {tool} {}", self.target)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        match exit.filter(|code| *code != 0) {
            Some(code) => format!("{detail} · exit {code}"),
            None => detail,
        }
    }
}

impl View {
    fn submit(&mut self, commands: &mpsc::Sender<Command>) {
        if self.pending_prompt.is_some() {
            self.copy_notice = Some("Waiting for prompt admission · draft retained and editable");
            return;
        }
        if self.cancel_pending {
            self.copy_notice = Some("Cancellation pending · draft retained");
            return;
        }
        let (reply, received) = oneshot::channel();
        match commands.try_send(Command::Submit {
            text: self.input.clone(),
            reply,
        }) {
            Ok(()) => {
                self.pending_prompt = Some(PendingPrompt {
                    text: self.input.clone(),
                    reply: received,
                });
                self.copy_notice = Some("Submitting prompt · draft retained until accepted");
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.copy_notice =
                    Some("Command queue is full · draft retained · try again when work advances");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.copy_notice = Some("Session stopped · prompt was not sent · draft retained");
            }
        }
    }

    // Poll once per event/frame, without waiting for either queue. At most one
    // admission and one cancellation are pending, even under repeated keys.
    fn poll_commands(&mut self, commands: &mpsc::Sender<Command>) {
        if let Some(pending) = &mut self.pending_prompt {
            let admitted = match pending.reply.try_recv() {
                Ok(result) => Some(result),
                Err(oneshot::error::TryRecvError::Empty) => None,
                Err(oneshot::error::TryRecvError::Closed) => Some(Err(
                    "Session stopped before accepting prompt · draft retained",
                )),
            };
            if let Some(admitted) = admitted {
                let pending = self.pending_prompt.take().expect("pending admission");
                match admitted {
                    Ok(()) => {
                        if self.input == pending.text {
                            self.input.clear();
                            self.copy_notice = None;
                        } else {
                            self.copy_notice = Some("Prompt accepted · edited draft retained");
                        }
                        self.selection = None;
                        self.note(Role::User, "You:", &pending.text);
                        self.anchor = None;
                        self.status = if self.busy {
                            "Queuing correction"
                        } else {
                            "Starting"
                        }
                        .into();
                        self.busy = true;
                    }
                    Err(reason) => self.copy_notice = Some(reason),
                }
            }
        }
        if self.cancel_pending {
            match commands.try_send(Command::Cancel) {
                Ok(()) => {
                    self.cancel_pending = false;
                    if self.busy {
                        self.status = "Cancelling".into();
                    }
                }
                Err(mpsc::error::TrySendError::Full(_)) => {}
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    self.cancel_pending = false;
                    self.copy_notice = Some("Session stopped · cancellation receiver closed");
                }
            }
        }
    }

    fn append(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let id = match self.assistant.filter(|id| self.chat.contains(*id)) {
            Some(id) => id,
            None => {
                let id = self
                    .chat
                    .begin(Role::Assistant, "Assistant", Source::Markdown);
                self.assistant = Some(id);
                id
            }
        };
        self.chat.append(id, text);
    }

    fn note(&mut self, role: Role, title: &str, text: &str) {
        self.assistant = None;
        let id = self.chat.begin(role, title, Source::Markdown);
        self.chat.append(id, text);
    }

    fn scroll(&mut self, rows: i64) {
        self.selection = None;
        self.copy_notice = None;
        self.chat.layout(self.chat_area.width);
        self.anchor = self.chat.scroll(self.anchor, rows, self.chat_area.height);
    }

    fn mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.copy_notice = None;
                if let Some(rail) = self
                    .rail
                    .filter(|r| r.area.contains((mouse.column, mouse.row).into()))
                {
                    self.selection = None;
                    let row = mouse.row.saturating_sub(rail.area.y);
                    let grab = if (rail.top..rail.top + rail.thumb).contains(&row) {
                        row - rail.top
                    } else {
                        rail.thumb / 2
                    };
                    self.drag_offset = Some(grab);
                    self.drag_to(mouse.row, rail, grab);
                } else if self.chat_area.contains((mouse.column, mouse.row).into()) {
                    let rows = self
                        .selection
                        .take()
                        .map(|s| s.rows())
                        .or_else(|| self.displayed.take());
                    if let Some(rows) = rows {
                        self.selection = Some(Selection::new(
                            rows,
                            mouse.row - self.chat_area.y,
                            mouse.column - self.chat_area.x,
                        ));
                    } else {
                        self.copy_notice = Some(
                            "Selection unavailable: visible text exceeds 256 KiB or 1024 rows",
                        );
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left) => {
                if let (Some(rail), Some(grab)) = (self.rail, self.drag_offset) {
                    self.drag_to(mouse.row, rail, grab);
                } else if let Some(selection) = &mut self.selection
                    && selection.dragging
                {
                    selection.update(
                        mouse.row.saturating_sub(self.chat_area.y),
                        mouse.column.saturating_sub(self.chat_area.x),
                    );
                }
                if matches!(mouse.kind, MouseEventKind::Up(_)) {
                    self.drag_offset = None;
                    if let Some(selection) = &mut self.selection {
                        selection.dragging = false;
                    }
                }
            }
            MouseEventKind::ScrollUp
                if self.chat_area.contains((mouse.column, mouse.row).into()) =>
            {
                self.scroll(-3)
            }
            MouseEventKind::ScrollDown
                if self.chat_area.contains((mouse.column, mouse.row).into()) =>
            {
                self.scroll(3)
            }
            _ => {}
        }
    }

    fn drag_to(&mut self, row: u16, rail: ScrollRail, grab: u16) {
        self.chat.layout(self.chat_area.width);
        let position = rail.position(row, grab);
        self.anchor = self
            .chat
            .scroll(self.chat.oldest(), position as i64, self.chat_area.height);
    }

    fn event(&mut self, envelope: Envelope) {
        match envelope.event {
            Event::SessionRecord { path, resumed } => self.note(
                Role::Notice,
                if resumed {
                    "Session restored"
                } else {
                    "Session saved"
                },
                &format!("Resume with --resume {path}. Task controls: /workflow-help"),
            ),
            Event::RetainedMessage { role, text } => self.note(
                if role == "developer" {
                    Role::User
                } else {
                    Role::Assistant
                },
                &format!("Retained {role}"),
                &text,
            ),
            Event::TaskAllocation {
                remaining_seconds,
                model_calls,
                model_limit,
                tool_calls,
                tool_limit,
                usage,
            } => {
                self.note(Role::Notice, "Task allocation", &format!("{remaining_seconds}s remaining · Model calls {model_calls}/{model_limit} · Tools {tool_calls}/{tool_limit}\nReported input {}{} · output {}{} · cost {}", usage.reported_input, if usage.unknown_input { " + unknown" } else { "" }, usage.reported_output, if usage.unknown_output { " + unknown" } else { "" }, if usage.unknown_cost { format!("${:.4} + unknown", usage.reported_cost_usd) } else { format!("${:.4}", usage.reported_cost_usd) }));
            }
            Event::TaskState {
                task_id,
                stopped,
                verification,
                review,
                accepted,
            } => {
                self.note(
                    Role::Notice,
                    &format!("Task {task_id}"),
                    &format!(
                        "Work: {} · Verification: {verification} · Review: {review} · Accepted: {}",
                        if stopped { "stopped" } else { "running" },
                        if accepted { "yes" } else { "no" }
                    ),
                );
            }
            Event::Ready { .. } => self.status = "Ready".into(),
            Event::TurnStarted => {
                self.busy = true;
                self.status = "Working".into();
                self.activity_tick = 0;
                self.usage.clear();
                self.context = ContextUsage::default();
                self.assistant = None;
            }
            Event::Text { text } => self.append(&text),
            Event::ToolStarted { call } => {
                self.assistant = None;
                let target = call
                    .arguments
                    .get("path")
                    .or_else(|| call.arguments.get("command"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let source = if call.name == "read" {
                    Path::new(target)
                        .extension()
                        .and_then(|s| s.to_str())
                        .filter(|extension| extension.len() <= 64)
                        .map_or(Source::Plain, |extension| Source::Code(extension.into()))
                } else {
                    Source::Markdown
                };
                let mut activity = ToolActivity {
                    block: 0,
                    name: call.name,
                    target: target.chars().take(384).collect(),
                };
                activity.block =
                    self.chat
                        .begin(Role::Running, &activity.title(Role::Running, None), source);
                self.tools.insert(call.id, activity);
            }
            Event::ToolOutput { call_id, text, .. } => {
                if let Some(activity) = self.tools.get(&call_id) {
                    self.chat.append(activity.block, &text);
                }
            }
            Event::ToolPresentation { call_id, text } => {
                self.note(Role::Notice, &format!("Presentation · {call_id}"), &text);
            }
            Event::ToolReview {
                call_id,
                reviewer,
                decision,
                reason,
            } => {
                self.note(
                    Role::Notice,
                    &format!("Oracle {reviewer} · {call_id} · {decision}"),
                    &reason,
                );
            }
            Event::ReviewUsage {
                reviewer,
                input,
                output,
                cached,
                cost_usd,
            } => {
                let text = usage_text(input, output, cached, cost_usd);
                if !text.is_empty() {
                    self.note(Role::Notice, &format!("Review usage {reviewer}"), &text);
                }
            }
            Event::OracleUsage {
                reviewer,
                input,
                output,
                cached,
                cost_usd,
            } => {
                let text = usage_text(input, output, cached, cost_usd);
                if !text.is_empty() {
                    self.note(Role::Notice, &format!("Oracle usage {reviewer}"), &text);
                }
            }

            Event::ToolFinished { result } | Event::RetainedTool { result } => {
                let role = if result.success {
                    Role::Success
                } else {
                    Role::Failed
                };
                let activity = self.tools.remove(&result.call_id).unwrap_or_else(|| {
                    let mut activity = ToolActivity {
                        block: 0,
                        name: result.tool.clone(),
                        target: String::new(),
                    };
                    activity.block = self.chat.begin(
                        role,
                        &activity.title(role, result.exit_code),
                        Source::Markdown,
                    );
                    activity
                });
                self.chat.replace(activity.block, &result.output);
                self.chat.heading(
                    activity.block,
                    role,
                    &activity.title(role, result.exit_code),
                );
            }
            Event::Usage {
                input,
                output,
                cached,
                cost_usd,
            } => {
                self.usage = usage_text(input, output, cached, cost_usd);
            }
            Event::Context { usage } => self.context = usage,
            Event::TurnFinished { status } => {
                self.busy = false;
                self.status = status.into();
                self.assistant = None;
                for (_, activity) in std::mem::take(&mut self.tools) {
                    self.chat.heading(
                        activity.block,
                        Role::Stopped,
                        &activity.title(Role::Stopped, None),
                    );
                }
            }
            Event::Error { message } => self.note(Role::Failed, "Error", &message),
        }
        self.tools
            .retain(|_, activity| self.chat.contains(activity.block));
    }
}

/// Provider and repository text cannot emit terminal control sequences.
fn visible_text(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}

pub async fn run(
    connection: &str,
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Envelope>,
) -> Result<()> {
    run_with_status(connection, commands, events, DisplayOptions::default()).await
}

pub async fn run_with_status(
    connection: &str,
    commands: mpsc::Sender<Command>,
    mut events: mpsc::Receiver<Envelope>,
    options: DisplayOptions,
) -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("DemonCoder requires an interactive terminal");
    }
    let mut terminal = ratatui::try_init().context("initialize terminal")?;
    let result = async {
        let _mouse = MouseCapture::enable()?;
        run_view(connection, &commands, &mut events, &mut terminal, options).await
    }
    .await;
    ratatui::restore();
    result
}

struct MouseCapture;

impl MouseCapture {
    fn enable() -> Result<Self> {
        let guard = Self;
        execute!(
            std::io::stdout(),
            EnableMouseCapture,
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .context("enable terminal input modes")?;
        Ok(guard)
    }
}

impl Drop for MouseCapture {
    fn drop(&mut self) {
        let _ = execute!(
            std::io::stdout(),
            PopKeyboardEnhancementFlags,
            DisableBracketedPaste,
            DisableMouseCapture
        );
    }
}

async fn run_view(
    connection: &str,
    commands: &mpsc::Sender<Command>,
    events: &mut mpsc::Receiver<Envelope>,
    terminal: &mut ratatui::DefaultTerminal,
    options: DisplayOptions,
) -> Result<()> {
    let (git_tx, mut git_rx) = mpsc::channel(1);
    let _git = GitPoller::start(options.workspace.clone(), git_tx);
    let mut view = View {
        options,
        status: "Connecting".into(),
        ..View::default()
    };
    let mut input_events = EventStream::new();
    let mut refresh = tokio::time::interval(Duration::from_millis(33));
    loop {
        view.poll_commands(commands);
        tokio::select! {
            Some(status) = git_rx.recv() => view.git = Some(status),
            event = events.recv() => match event {
                Some(event) => {
                    // Admission precedes turn events in the runtime. Consume it
                    // first even when this select woke on the event channel.
                    view.poll_commands(commands);
                    view.event(event);
                },
                None => bail!("session runtime stopped"),
            },
            _ = refresh.tick() => {
                if view.busy { view.activity_tick = view.activity_tick.wrapping_add(1); }
                terminal.draw(|frame| draw(&mut view, connection, frame)).context("draw terminal")?;
            },
            input = input_events.next() => {
                let Some(input) = input else { return Ok(()); };
                match input.context("read terminal input")? {
                    InputEvent::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                        KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                        KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) && !view.input.is_empty() => {
                                execute!(std::io::stdout(), CopyToClipboard::to_clipboard_from(view.input.as_str())).context("copy prompt text")?;
                                view.copy_notice = Some("Prompt copy requested · terminal must allow OSC 52");
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::SHIFT) => {
                            if view.busy || view.pending_prompt.is_some() { view.cancel_pending = true; }
                            else { view.input.clear(); }
                        }
                        KeyCode::Esc if view.selection.is_some() => { view.selection = None; view.copy_notice = None; },
                        KeyCode::Char('y') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if let Some(selection) = &view.selection {
                                let text = selection.text();
                                if !text.is_empty() {
                                    execute!(std::io::stdout(), CopyToClipboard::to_clipboard_from(text)).context("send clipboard request")?;
                                    view.copy_notice = Some("Copy requested · terminal must allow OSC 52 · Esc clears selection");
                                }
                            }
                        },
                        KeyCode::Esc if view.busy || view.pending_prompt.is_some() => { view.cancel_pending = true; }
                        KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => { view.selection = None; view.copy_notice = None; view.chat.toggle(); },
                        KeyCode::PageUp => view.scroll(-i64::from(view.chat_area.height.max(1))),
                        KeyCode::PageDown => view.scroll(i64::from(view.chat_area.height.max(1))),
                        KeyCode::Up => view.scroll(-1),
                        KeyCode::Down => view.scroll(1),
                        KeyCode::Home => {
                            view.selection = None; view.copy_notice = None;
                            view.chat.layout(view.chat_area.width);
                            view.anchor = view.chat.oldest();
                        }
                        KeyCode::End => { view.selection = None; view.copy_notice = None; view.anchor = None; },
                        KeyCode::Enter if !view.input.trim().is_empty() => {
                            view.submit(commands);
                        }
                        KeyCode::Backspace => { view.input.pop(); }
                        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && !c.is_control()
                            && view.input.len() + c.len_utf8() <= MAX_INPUT_BYTES => { view.input.push(c); }
                        _ => {}
                    },
                    InputEvent::Paste(text) => {
                        for c in text.chars().filter(|c| !c.is_control()) {
                            if view.input.len() + c.len_utf8() > MAX_INPUT_BYTES { break; }
                            view.input.push(c);
                        }
                    }
                    InputEvent::Mouse(mouse) => view.mouse(mouse),

                    _ => {}
                }
            }
        }
    }
}

fn status_line(view: &View) -> String {
    let git = match &view.git {
        Some(Ok(git)) => format!(
            "{} dirty {} · {}",
            git.dirty,
            visible_text(&git.branch),
            git.diff
                .map_or_else(|| "diff ?".into(), |(a, d)| format!("+{a}/-{d}"))
        ),
        Some(Err(_)) => "Git unavailable · diff ?".into(),
        None => "Git ? · diff ?".into(),
    };
    let mut text = format!(
        "Model {} · {} · {} · agents 0",
        visible_text(view.options.model.as_deref().unwrap_or("backend-default")),
        context_text(view.context, view.options.context_window),
        git
    );
    if !view.usage.is_empty() {
        text.push_str(" · ");
        text.push_str(&view.usage);
    }
    text
}

fn draw(view: &mut View, connection: &str, frame: &mut ratatui::Frame<'_>) {
    let mut area = frame.area();
    // One scrollbar column, then two empty character cells at the right edge.
    area.width = area
        .width
        .saturating_sub(if area.width >= 8 { 3 } else { 0 });
    let [header, notice, body, editor, usage, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(u16::from(view.chat.expired())),
        Constraint::Min(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(format!(
            "DemonCoder · {}{}{} · {}",
            if view.busy {
                ["⠋ ", "⠙ ", "⠹ ", "⠸ ", "⠼ ", "⠴ ", "⠦ ", "⠧ ", "⠇ ", "⠏ "]
                    [(view.activity_tick / 3 % 10) as usize]
            } else {
                ""
            },
            view.status,
            view.tools
                .values()
                .next()
                .map(|t| format!(" · {} {}", t.name, visible_text(&t.target)))
                .unwrap_or_default(),
            visible_text(connection)
        ))
        .style(Style::default().fg(Color::Cyan)),
        header,
    );
    if view.chat.expired() {
        frame.render_widget(
            Paragraph::new("Older chat expired · display retention limit")
                .style(Style::default().fg(Color::Yellow)),
            notice,
        );
    }
    view.chat_area = body;
    view.chat.layout(body.width);
    if let Some(selection) = &view.selection {
        frame.render_widget(Paragraph::new(selection.lines(body.height)), body);
    } else if view.chat.is_empty() {
        view.displayed = None;
        frame.render_widget(
            Paragraph::new("Conversation starts with your next prompt.")
                .style(Style::default().fg(Color::DarkGray)),
            body,
        );
    } else {
        let window = view.chat.window(view.anchor, body.height);
        view.displayed = Selection::snapshot(&window);
        frame.render_widget(Paragraph::new(window), body);
    }
    let width = editor.width.saturating_sub(2) as usize;
    let mut start = view.input.len();
    let mut columns = 0;
    for (index, c) in view.input.char_indices().rev() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if columns + w >= width {
            break;
        }
        start = index;
        columns += w;
    }
    let shown = &view.input[start..];
    frame.render_widget(
        Paragraph::new(shown).block(Block::default().borders(Borders::ALL).title(if view.busy {
            "Correction · Enter sends · Ctrl-Shift-C/V copy/paste · Esc cancels"
        } else {
            "Prompt · Enter sends · Ctrl-Shift-C/V copy/paste"
        })),
        editor,
    );
    if width > 0 && editor.height > 1 {
        frame.set_cursor_position((editor.x + 1 + shown.width() as u16, editor.y + 1));
    }
    frame.render_widget(
        Paragraph::new(status_line(view)).style(Style::default().fg(Color::DarkGray)),
        usage,
    );
    frame.render_widget(
        Paragraph::new(if let Some(notice) = view.copy_notice {
            notice
        } else if view.selection.is_some() {
            "Selection frozen · Ctrl-Y copy · Esc clears · Ctrl-Q quit"
        } else {
            match (view.chat.expanded(), view.anchor.is_some()) {
                (true, true) => {
                    "Full output · History · End latest · Ctrl-O collapse · Ctrl-Q quit"
                }
                (true, false) => "Full output · Ctrl-O collapse · PgUp/PgDn scroll · Ctrl-Q quit",
                (false, true) => {
                    "History · PgUp/PgDn scroll · End latest · Ctrl-O full output · Ctrl-Q quit"
                }
                (false, false) => "PgUp/PgDn scroll · Ctrl-O full output · Ctrl-Q quit",
            }
        })
        .style(Style::default().fg(Color::DarkGray)),
        help,
    );

    let (length, position) = view.chat.scroll_metrics(view.anchor, body.height);
    view.rail = None;
    if frame.area().width >= 8 && length > usize::from(body.height) && body.height > 0 {
        let rail = ScrollRail::new(
            Rect {
                x: area.right(),
                y: body.y,
                width: 1,
                height: body.height,
            },
            length,
            position,
        );
        for y in 0..body.height {
            let thumb = (rail.top..rail.top + rail.thumb).contains(&y);
            frame.render_widget(
                Paragraph::new(if thumb { "█" } else { "║" })
                    .style(Style::default().fg(if thumb { Color::Cyan } else { Color::DarkGray })),
                Rect {
                    y: body.y + y,
                    height: 1,
                    ..rail.area
                },
            );
        }
        view.rail = Some(rail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    fn render(view: &mut View, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(view, "fixture", frame)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn deliver(view: &mut View, event: Event) {
        view.event(Envelope {
            connection: "fixture".into(),
            event,
        });
    }

    #[test]
    fn selection_survives_final_receipt_resize_and_reselection() {
        let mut view = View::default();
        deliver(
            &mut view,
            Event::ToolStarted {
                call: crate::tools::ToolCall {
                    id: "read-1".into(),
                    name: "read".into(),
                    arguments: serde_json::json!({"path":"file.rs"}),
                },
            },
        );
        deliver(
            &mut view,
            Event::ToolOutput {
                call_id: "read-1".into(),
                stream: "stdout",
                text: "a界e\u{301}z".into(),
            },
        );
        render(&mut view, 60, 25);
        let rows = view.displayed.as_ref().unwrap();
        let y = rows
            .iter()
            .position(|line| line.to_string().contains("a界"))
            .unwrap() as u16
            + view.chat_area.y;
        let x = rows[(y - view.chat_area.y) as usize]
            .to_string()
            .find('a')
            .unwrap() as u16;
        let mouse = |kind, column| MouseEvent {
            kind,
            column,
            row: y,
            modifiers: KeyModifiers::NONE,
        };
        view.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x + 6));
        view.mouse(mouse(MouseEventKind::Up(MouseButton::Left), x + 1));
        assert_eq!(view.selection.as_ref().unwrap().text(), "界e\u{301}z");
        deliver(
            &mut view,
            Event::ToolFinished {
                result: crate::tools::ToolResult {
                    call_id: "read-1".into(),
                    tool: "read".into(),
                    success: true,
                    output: "REPLACEMENT".into(),
                    exit_code: None,
                },
            },
        );
        render(&mut view, 8, 10);
        assert_eq!(view.selection.as_ref().unwrap().text(), "界e\u{301}z");
        render(&mut view, 60, 25);
        view.mouse(mouse(MouseEventKind::Down(MouseButton::Left), x));
        view.mouse(mouse(MouseEventKind::Up(MouseButton::Left), x + 1));
        assert_eq!(view.selection.as_ref().unwrap().text(), "a");
        view.scroll(0);
        let buffer = render(&mut view, 60, 25);
        assert!(
            buffer
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .contains("REPLACEMENT")
        );
    }

    #[test]
    fn scrollbar_drag_endpoints_follow_compact_full_and_resized_geometry() {
        let mut view = View::default();
        for n in 0..40 {
            view.note(Role::Notice, "Block", &format!("block-{n}\n"));
        }
        for expanded in [false, true] {
            if expanded {
                view.chat.toggle();
            }
            for (width, height) in [(80, 25), (30, 15)] {
                render(&mut view, width, height);
                let rail = view.rail.unwrap();
                view.drag_to(0, rail, 0);
                assert_eq!(
                    view.chat
                        .scroll_metrics(view.anchor, view.chat_area.height)
                        .1,
                    0
                );
                view.drag_to(u16::MAX, rail, 0);
                assert_eq!(
                    view.chat
                        .scroll_metrics(view.anchor, view.chat_area.height)
                        .1,
                    rail.max_top
                );
            }
        }
    }

    #[test]
    fn status_and_oracle_omit_unknown_money_but_keep_zero_and_reset_context() {
        let mut view = View::default();
        deliver(
            &mut view,
            Event::Context {
                usage: ContextUsage {
                    used: Some(123),
                    capacity: Some(1000),
                    estimated: false,
                },
            },
        );
        deliver(
            &mut view,
            Event::Usage {
                input: Some(0),
                output: None,
                cached: None,
                cost_usd: None,
            },
        );
        assert!(status_line(&view).contains("Ctx 123/1000"));
        assert!(status_line(&view).contains("in 0"));
        assert!(!status_line(&view).contains("cost"));
        deliver(
            &mut view,
            Event::OracleUsage {
                reviewer: "reviewer".into(),
                input: Some(0),
                output: None,
                cached: None,
                cost_usd: None,
            },
        );
        let buffer = render(&mut view, 180, 25);
        assert!(
            !buffer
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .contains("cost")
        );
        deliver(
            &mut view,
            Event::OracleUsage {
                reviewer: "reviewer".into(),
                input: None,
                output: None,
                cached: None,
                cost_usd: Some(0.0),
            },
        );
        let buffer = render(&mut view, 180, 25);
        assert!(
            buffer
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .contains("cost $0.0000")
        );
        deliver(&mut view, Event::TurnStarted);
        assert!(status_line(&view).contains("Ctx ?/?"));
        assert!(!status_line(&view).contains(" · in "));
    }

    #[test]
    fn scrollbar_tracks_full_history_and_leaves_two_empty_outer_columns() {
        let mut view = View::default();
        view.append(
            &(0..200)
                .map(|n| format!("row-{n:03} 界 👩‍💻\n"))
                .collect::<String>(),
        );
        let compact = render(&mut view, 60, 25);
        assert!((0..25).all(|y| compact[(57, y)].symbol() == " "));
        view.chat.toggle();
        view.anchor = view.chat.oldest();
        let first = render(&mut view, 60, 25);
        let body = view.chat_area;
        let thumbs = |buffer: &Buffer| {
            (body.y..body.bottom())
                .filter(|y| buffer[(57, *y)].fg == Color::Cyan)
                .collect::<Vec<_>>()
        };
        let top = thumbs(&first);
        assert!(!top.is_empty(), "full history has no scrollbar thumb");
        assert_eq!(top[0], body.y);
        view.anchor = None;
        let last = render(&mut view, 60, 25);
        let bottom = thumbs(&last);
        assert_eq!(bottom.last(), Some(&(body.bottom() - 1)));
        assert!(bottom[0] > top[0]);
        for buffer in [&first, &last] {
            for y in 0..25 {
                for x in [58, 59] {
                    assert_eq!(buffer[(x, y)].symbol(), " ");
                }
            }
        }
        view.chat.toggle();
        let compact = render(&mut view, 60, 25);
        assert!((body.y..body.bottom()).all(|y| compact[(57, y)].symbol() == " "));
        for (width, height) in [(1, 1), (4, 8), (8, 4), (20, 10)] {
            render(&mut view, width, height);
        }
    }

    #[test]
    fn fenced_code_renders_styles_without_interpreting_control_sequences() {
        let mut view = View::default();
        view.append("```rust\nlet greeting = \"界\";\n```\n\x1b[2Jliteral");
        let buffer = render(&mut view, 60, 25);
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("let greeting = \"界 \";"));
        assert!(text.contains("�[2Jliteral"));
        assert!(
            buffer
                .content
                .iter()
                .any(|cell| matches!(cell.fg, Color::Rgb(..)))
        );
        let keyword = buffer
            .content
            .iter()
            .find(|cell| cell.symbol() == "l" && matches!(cell.fg, Color::Rgb(..)))
            .unwrap();
        let string = buffer
            .content
            .iter()
            .find(|cell| cell.symbol() == "界")
            .unwrap();
        assert_ne!(keyword.fg, string.fg);
    }

    #[test]
    fn empty_output_deltas_do_not_accumulate_transcript_entries() {
        let mut view = View::default();
        for _ in 0..100_000 {
            view.append("");
        }
        assert!(
            view.chat.is_empty(),
            "empty output accumulated transcript metadata"
        );
    }
}

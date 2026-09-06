use crate::{
    events::{Envelope, Event},
    session::Command,
};
use anyhow::{Context, Result, bail};
use crossterm::event::{Event as InputEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::{collections::VecDeque, io::IsTerminal, time::Duration};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_TRANSCRIPT_BYTES: usize = 1024 * 1024;

#[derive(Default)]
struct View {
    input: String,
    transcript: VecDeque<String>,
    transcript_bytes: usize,
    status: String,
    usage: String,
    busy: bool,
}

impl View {
    fn append(&mut self, text: &str) {
        let text = visible_text(text);
        self.transcript_bytes += text.len();
        self.transcript.push_back(text);
        while self.transcript_bytes > MAX_TRANSCRIPT_BYTES {
            if let Some(text) = self.transcript.pop_front() {
                self.transcript_bytes -= text.len();
            }
        }
    }

    fn event(&mut self, envelope: Envelope) {
        match envelope.event {
            Event::Ready { .. } => self.status = "Ready".into(),
            Event::TurnStarted => {
                self.busy = true;
                self.status = "Working".into();
            }
            Event::Text { text } => self.append(&text),
            Event::Usage {
                input,
                output,
                cached,
                cost_usd,
            } => {
                let count =
                    |value: Option<u64>| value.map_or_else(|| "unknown".into(), |v| v.to_string());
                self.usage = format!(
                    "in {} · out {} · cached {} · cost {}",
                    count(input),
                    count(output),
                    count(cached),
                    cost_usd.map_or_else(|| "unknown".into(), |v| format!("${v:.4}"))
                );
            }
            Event::TurnFinished { status } => {
                self.busy = false;
                self.status = status.into();
                self.append("\n");
            }
            Event::Error { message } => self.append(&format!("\nError: {message}\n")),
        }
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
    mut events: mpsc::Receiver<Envelope>,
) -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("DemonCoder requires an interactive terminal");
    }
    let mut terminal = ratatui::try_init().context("initialize terminal")?;
    let result = run_view(connection, &commands, &mut events, &mut terminal).await;
    ratatui::restore();
    result
}

async fn run_view(
    connection: &str,
    commands: &mpsc::Sender<Command>,
    events: &mut mpsc::Receiver<Envelope>,
    terminal: &mut ratatui::DefaultTerminal,
) -> Result<()> {
    let mut view = View {
        status: "Connecting".into(),
        usage: "usage unknown".into(),
        ..View::default()
    };
    let mut input_events = EventStream::new();
    let mut refresh = tokio::time::interval(Duration::from_millis(33));
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Some(event) => view.event(event),
                None => bail!("session runtime stopped"),
            },
            _ = refresh.tick() => {
                terminal.draw(|frame| {
                    let [header, body, editor, footer] = Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(3), Constraint::Length(1)]).areas(frame.area());
                    frame.render_widget(Paragraph::new(format!("DemonCoder · {} · {}", visible_text(connection), view.status)).style(Style::default().fg(Color::Cyan)), header);
                    let transcript = view.transcript.iter().cloned().collect::<String>();
                    let paragraph = Paragraph::new(transcript).wrap(Wrap { trim: false });
                    let lines = paragraph.line_count(body.width).saturating_sub(body.height as usize);
                    frame.render_widget(paragraph.scroll((lines.min(u16::MAX as usize) as u16, 0)), body);
                    let width = editor.width.saturating_sub(2) as usize;
                    let mut start = view.input.len();
                    let mut columns = 0;
                    for (index, c) in view.input.char_indices().rev() {
                        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                        if columns + w >= width { break; }
                        start = index; columns += w;
                    }
                    let shown = &view.input[start..];
                    frame.render_widget(Paragraph::new(shown).block(Block::default().borders(Borders::ALL).title(if view.busy { "Draft · Esc cancels work" } else { "Prompt · Enter sends" })), editor);
                    if width > 0 { frame.set_cursor_position((editor.x + 1 + shown.width() as u16, editor.y + 1)); }
                    frame.render_widget(Paragraph::new(format!("{} · Ctrl-Q quit", view.usage)).style(Style::default().fg(Color::DarkGray)), footer);
                }).context("draw terminal")?;
            },
            input = input_events.next() => {
                let Some(input) = input else { return Ok(()); };
                match input.context("read terminal input")? {
                    InputEvent::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                        KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if view.busy { commands.send(Command::Cancel).await.context("cancel session")?; }
                            else { view.input.clear(); }
                        }
                        KeyCode::Esc if view.busy => { commands.send(Command::Cancel).await.context("cancel session")?; }
                        KeyCode::Enter if !view.input.trim().is_empty() && !view.busy => {
                            let prompt = std::mem::take(&mut view.input);
                            view.append(&format!("\nYou: {prompt}\n\n"));
                            view.busy = true;
                            view.status = "Starting".into();
                            commands.send(Command::Prompt(prompt)).await.context("submit prompt")?;
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
                    _ => {}
                }
            }
        }
    }
}

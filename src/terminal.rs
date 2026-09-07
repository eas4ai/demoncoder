use crate::{
    events::{Envelope, Event},
    session::Command,
    transcript::{Position, Transcript},
};
use anyhow::{Context, Result, bail};
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event as InputEvent, EventStream, KeyCode,
        KeyEventKind, KeyModifiers, MouseEventKind,
    },
    execute,
};
use futures_util::StreamExt;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};
use std::{io::IsTerminal, time::Duration};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;

const MAX_INPUT_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct View {
    input: String,
    transcript: Transcript,
    anchor: Option<Position>,
    chat_area: Rect,
    status: String,
    usage: String,
    busy: bool,
}

impl View {
    fn append(&mut self, text: &str) {
        self.transcript.append(text);
    }

    fn scroll(&mut self, rows: i64) {
        self.transcript.layout(self.chat_area.width);
        self.anchor = self
            .transcript
            .scroll(self.anchor, rows, self.chat_area.height);
    }

    fn event(&mut self, envelope: Envelope) {
        match envelope.event {
            Event::Ready { .. } => self.status = "Ready".into(),
            Event::TurnStarted => {
                self.busy = true;
                self.status = "Working".into();
                self.usage.clear();
            }
            Event::Text { text } => self.append(&text),
            Event::ToolStarted { call } => self.append(&format!("\n[{} {}]\n", call.name, call.id)),
            Event::ToolOutput { text, .. } => self.append(&text),
            Event::ToolPresentation { call_id, text } => {
                self.append(&format!("\n[Presentation for {call_id}]\n{text}\n"))
            }
            Event::ToolReview {
                call_id,
                reviewer,
                decision,
                reason,
            } => {
                self.append(&format!(
                    "\n[Oracle {reviewer} · {call_id} · {decision}] {reason}\n"
                ));
            }
            Event::OracleUsage {
                reviewer,
                input,
                output,
                cached,
                cost_usd,
            } => {
                let count =
                    |value: Option<u64>| value.map_or_else(|| "unknown".into(), |v| v.to_string());
                self.append(&format!(
                    "\n[Oracle usage {reviewer}] in {} · out {} · cached {} · cost {}\n",
                    count(input),
                    count(output),
                    count(cached),
                    cost_usd.map_or_else(|| "unknown".into(), |v| format!("${v:.4}"))
                ));
            }
            Event::ToolFinished { result } => self.append(&format!(
                "\n[{}: {}]\n{}\n",
                result.call_id,
                if result.success { "ok" } else { "failed" },
                result.output
            )),
            Event::Usage {
                input,
                output,
                cached,
                cost_usd,
            } => {
                if input.is_none() && output.is_none() && cached.is_none() && cost_usd.is_none() {
                    self.usage.clear();
                    return;
                }
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
    let result = async {
        let _mouse = MouseCapture::enable()?;
        run_view(connection, &commands, &mut events, &mut terminal).await
    }
    .await;
    ratatui::restore();
    result
}

struct MouseCapture;

impl MouseCapture {
    fn enable() -> Result<Self> {
        let guard = Self;
        execute!(std::io::stdout(), EnableMouseCapture).context("enable chat mouse scrolling")?;
        Ok(guard)
    }
}

impl Drop for MouseCapture {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
    }
}

async fn run_view(
    connection: &str,
    commands: &mpsc::Sender<Command>,
    events: &mut mpsc::Receiver<Envelope>,
    terminal: &mut ratatui::DefaultTerminal,
) -> Result<()> {
    let mut view = View {
        status: "Connecting".into(),
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
                    let [header, notice, body, editor, usage, help] = Layout::vertical([Constraint::Length(1), Constraint::Length(u16::from(view.transcript.expired())), Constraint::Min(1), Constraint::Length(3), Constraint::Length(u16::from(!view.usage.is_empty())), Constraint::Length(1)]).areas(frame.area());
                    frame.render_widget(Paragraph::new(format!("DemonCoder · {} · {}", visible_text(connection), view.status)).style(Style::default().fg(Color::Cyan)), header);
                    if view.transcript.expired() {
                        frame.render_widget(Paragraph::new("Older chat expired · display retention limit").style(Style::default().fg(Color::Yellow)), notice);
                    }
                    view.chat_area = body;
                    view.transcript.layout(body.width);
                    if view.transcript.is_empty() {
                        frame.render_widget(Paragraph::new("Conversation starts with your next prompt.").style(Style::default().fg(Color::DarkGray)), body);
                    } else {
                        let window = view.transcript.window(view.anchor, body.height);
                        frame.render_widget(Paragraph::new(window.rows.into_iter().map(Line::raw).collect::<Vec<_>>()), body);
                    }
                    let width = editor.width.saturating_sub(2) as usize;
                    let mut start = view.input.len();
                    let mut columns = 0;
                    for (index, c) in view.input.char_indices().rev() {
                        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                        if columns + w >= width { break; }
                        start = index; columns += w;
                    }
                    let shown = &view.input[start..];
                    frame.render_widget(Paragraph::new(shown).block(Block::default().borders(Borders::ALL).title(if view.busy { "Correction · Enter sends · Esc cancels" } else { "Prompt · Enter sends" })), editor);
                    if width > 0 && editor.height > 1 { frame.set_cursor_position((editor.x + 1 + shown.width() as u16, editor.y + 1)); }
                    frame.render_widget(Paragraph::new(view.usage.as_str()).style(Style::default().fg(Color::DarkGray)), usage);
                    frame.render_widget(Paragraph::new(if view.anchor.is_some() { "History · PgUp/PgDn scroll · End latest · Ctrl-Q quit" } else { "PgUp/PgDn or wheel scroll · Home oldest · Ctrl-Q quit" }).style(Style::default().fg(Color::DarkGray)), help);
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
                        KeyCode::PageUp => view.scroll(-i64::from(view.chat_area.height.max(1))),
                        KeyCode::PageDown => view.scroll(i64::from(view.chat_area.height.max(1))),
                        KeyCode::Up => view.scroll(-1),
                        KeyCode::Down => view.scroll(1),
                        KeyCode::Home => {
                            view.transcript.layout(view.chat_area.width);
                            view.anchor = view.transcript.oldest();
                        }
                        KeyCode::End => view.anchor = None,
                        KeyCode::Enter if !view.input.trim().is_empty() => {
                            let prompt = std::mem::take(&mut view.input);
                            view.append(&format!("\nYou: {prompt}\n\n"));
                            view.anchor = None;
                            view.status = if view.busy { "Queuing correction" } else { "Starting" }.into();
                            view.busy = true;
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
                    InputEvent::Mouse(mouse) if view.chat_area.contains((mouse.column, mouse.row).into()) => {
                        match mouse.kind {
                            MouseEventKind::ScrollUp => view.scroll(-3),
                            MouseEventKind::ScrollDown => view.scroll(3),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::View;

    #[test]
    fn empty_output_deltas_do_not_accumulate_transcript_entries() {
        let mut view = View::default();
        for _ in 0..100_000 {
            view.append("");
        }
        assert!(
            view.transcript.is_empty(),
            "empty output accumulated transcript metadata"
        );
    }
}

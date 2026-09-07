//! The inspector has its own viewport; conversation anchors and drafts stay owned
//! by the terminal. Every key here is read-only.
use crate::inspection::{Page, Request, Snapshot, Summary, Target};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Inspection {
    pub enabled: bool,
    pub open: bool,
    summary: Option<Summary>,
    updated: Option<Instant>,
    error: Option<String>,
    target: Target,
    page: usize,
    generation: u64,
    content: Option<Page>,
    scroll: u16,
    max_scroll: u16,
    height: u16,
}

impl Inspection {
    pub fn poll(&mut self, reader: &mut Option<crate::inspection::poller::Poller>) {
        if let Some(reader) = reader {
            reader.request(self.request());
            if let Some(snapshot) = reader.take() {
                self.update(snapshot);
            }
        }
    }

    pub fn mouse(&mut self, mouse: crossterm::event::MouseEvent) -> bool {
        if !self.open {
            return false;
        }
        match mouse.kind {
            crossterm::event::MouseEventKind::ScrollUp => self.scroll_by(-3),
            crossterm::event::MouseEventKind::ScrollDown => self.scroll_by(3),
            _ => {}
        }
        true
    }

    pub fn update(&mut self, snapshot: Result<Snapshot, String>) {
        match snapshot {
            Ok(snapshot) => {
                self.summary = Some(snapshot.summary);
                self.updated = Some(Instant::now());
                self.error = None;
                if let Some(page) = snapshot.page
                    && self.request() == Some(page.request)
                {
                    self.content = Some(page);
                }
            }
            Err(error) => {
                self.error = Some(error);
                self.updated = None;
            }
        }
    }

    pub fn request(&self) -> Option<Request> {
        self.open.then_some(Request {
            target: self.target,
            page: self.page,
            generation: self.generation,
        })
    }

    fn refresh(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.content = None;
        self.scroll = 0;
        self.max_scroll = 0;
    }

    fn aged(&self) -> bool {
        self.updated
            .is_none_or(|updated| updated.elapsed() > Duration::from_secs(1))
    }

    pub fn counts(&self) -> String {
        match (&self.summary, &self.error) {
            (_, Some(_)) => "agents ? · state unavailable".into(),
            (Some(summary), _) if !self.aged() => summary.counts(),
            (Some(_), _) => "agents ? · state refresh pending".into(),
            _ => "agents ? · awaiting state".into(),
        }
    }

    pub fn task_line(&self) -> Option<String> {
        if let Some(error) = &self.error {
            return Some(format!("State unavailable: {error}"));
        }
        let summary = self.summary.as_ref()?;
        if self.aged() {
            return Some("State refresh pending · previous state is not current".into());
        }
        if summary.recovery {
            return Some(format!(
                "Inspection required · {}",
                summary
                    .task
                    .as_deref()
                    .unwrap_or("interrupted work is uncertain")
            ));
        }
        summary.task.clone()
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::F(2) && self.enabled {
            self.open = !self.open;
            if self.open {
                self.target = Target::Overview;
                self.page = 0;
            }
            self.refresh();
            return true;
        }
        if !self.open {
            return false;
        }
        match key.code {
            KeyCode::Esc => {
                self.open = false;
                self.refresh();
            }
            KeyCode::F(5) => self.refresh(),
            KeyCode::Tab | KeyCode::BackTab => {
                if let Some(summary) = &self.summary {
                    let targets = &summary.targets;
                    if !targets.is_empty() {
                        let index = targets
                            .iter()
                            .position(|target| *target == self.target)
                            .unwrap_or(0);
                        let reverse = key.code == KeyCode::BackTab
                            || key.modifiers.contains(KeyModifiers::SHIFT);
                        self.target = targets
                            [(index + if reverse { targets.len() - 1 } else { 1 }) % targets.len()];
                        self.page = 0;
                        self.refresh();
                    }
                }
            }
            KeyCode::Right if self.content.as_ref().is_some_and(|page| page.more) => {
                self.page += 1;
                self.refresh();
            }
            KeyCode::Left if self.page > 0 => {
                self.page -= 1;
                self.refresh();
            }
            KeyCode::PageUp => self.scroll_by(-i32::from(self.height.max(1))),
            KeyCode::PageDown => self.scroll_by(i32::from(self.height.max(1))),
            KeyCode::Up => self.scroll_by(-1),
            KeyCode::Down => self.scroll_by(1),
            KeyCode::Home => self.scroll = 0,
            KeyCode::End => self.scroll = self.max_scroll,
            // These keys otherwise belong to conversation navigation. Do not
            // change the hidden chat or its text selection during inspection.
            KeyCode::Left | KeyCode::Right => {}
            KeyCode::Char('o' | 'y') if key.modifiers.contains(KeyModifiers::CONTROL) => {}
            _ => return false,
        }
        true
    }

    pub fn scroll_by(&mut self, rows: i32) {
        self.scroll = (i32::from(self.scroll) + rows).clamp(0, i32::from(self.max_scroll)) as u16;
    }

    pub fn draw(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let title = format!(
            "Inspection · {} · Page {}",
            self.target.label(),
            self.page + 1
        );
        let block = Block::default().borders(Borders::ALL).title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let freshness =
            "Freshness: saved evidence; files not rechecked. F5 refreshes saved state.\n";
        let text = match (&self.content, &self.error) {
            (_, Some(error)) => format!(
                "State unavailable: {error}\nPrevious evidence cannot establish current status."
            ),
            (Some(page), _) => format!(
                "{freshness}{}\n{}",
                page.text,
                if page.more {
                    "More evidence on the next page (Right)."
                } else {
                    "End of saved evidence."
                }
            ),
            _ => "Loading saved evidence… input and cancellation remain available.".into(),
        };
        let text = super::visible_text(&text);
        let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
        let rows = paragraph.line_count(inner.width.max(1));
        self.height = inner.height;
        self.max_scroll = rows
            .saturating_sub(usize::from(inner.height))
            .min(u16::MAX as usize) as u16;
        self.scroll = self.scroll.min(self.max_scroll);
        frame.render_widget(
            paragraph
                .scroll((self.scroll, 0))
                .style(Style::default().fg(Color::White)),
            inner,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_stale_and_failed_state_never_looks_measured() {
        let mut panel = Inspection::default();
        assert!(panel.counts().contains("agents ?"));
        panel.update(Ok(Snapshot {
            summary: Summary {
                active: 3,
                ..Summary::default()
            },
            page: None,
        }));
        assert!(panel.counts().contains("agents 3 active"));
        panel.updated = Some(Instant::now() - Duration::from_secs(2));
        assert!(panel.counts().contains("agents ?"));
        panel.update(Err("record unavailable".into()));
        assert!(panel.counts().contains("state unavailable"));
        assert!(panel.task_line().unwrap().contains("record unavailable"));
    }

    #[test]
    fn late_page_cannot_replace_a_new_request() {
        let mut panel = Inspection {
            enabled: true,
            ..Inspection::default()
        };
        panel.key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        let old_request = panel.request().unwrap();
        panel.key(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE));
        panel.update(Ok(Snapshot {
            summary: Summary::default(),
            page: Some(Page {
                request: old_request,
                text: "obsolete".into(),
                more: false,
            }),
        }));
        assert!(panel.content.is_none());
        panel.update(Ok(Snapshot {
            summary: Summary::default(),
            page: Some(Page {
                request: panel.request().unwrap(),
                text: "requested".into(),
                more: false,
            }),
        }));
        assert_eq!(panel.content.as_ref().unwrap().text, "requested");
        assert!(!panel.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    }
}

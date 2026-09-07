//! Activity blocks over the bounded, incremental transcript index.
use crate::{
    highlight::Source,
    transcript::{MAX_TRANSCRIPT_BYTES, MAX_TRANSCRIPT_LINES, Position, Transcript},
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const PREVIEW_HEAD: u64 = 6;
const PREVIEW_TAIL: u64 = 2;
const COLLAPSE_AFTER: u64 = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    User,
    Assistant,
    Running,
    Success,
    Failed,
    Stopped,
    Notice,
}

impl Role {
    fn color(self) -> Color {
        match self {
            Self::User | Self::Notice => Color::Cyan,
            Self::Assistant => Color::White,
            Self::Running | Self::Stopped => Color::Yellow,
            Self::Success => Color::Green,
            Self::Failed => Color::Red,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Header,
    Body(Position),
    Fold,
    Gap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Anchor {
    block: u64,
    part: Part,
}

struct Block {
    role: Role,
    title: String,
    source: Source,
    text: Transcript,
    start: Option<u64>,
    body_rows: u64,
    height: u64,
}

impl Block {
    fn size(&self) -> usize {
        self.title.len() + self.text.size() + 1
    }
    fn lines(&self) -> usize {
        self.text.line_count() + 2
    }
    fn folded(&self, expanded: bool) -> bool {
        !expanded && self.role != Role::User && self.body_rows > COLLAPSE_AFTER
    }
    fn body_height(&self, expanded: bool) -> u64 {
        if self.folded(expanded) {
            PREVIEW_HEAD + PREVIEW_TAIL + 1
        } else {
            self.body_rows
        }
    }
    fn local_row(&self, part: Part, expanded: bool) -> u64 {
        match part {
            Part::Header => 0,
            Part::Gap => self.height.saturating_sub(1),
            Part::Fold => 1 + PREVIEW_HEAD.min(self.body_height(expanded)),
            Part::Body(position) => {
                let offset = self.text.offset_of(position);
                1 + if !self.folded(expanded) || offset < PREVIEW_HEAD {
                    offset
                } else if offset >= self.body_rows - PREVIEW_TAIL {
                    PREVIEW_HEAD + 1 + offset - (self.body_rows - PREVIEW_TAIL)
                } else {
                    PREVIEW_HEAD
                }
            }
        }
    }
    fn part_at(&self, local: u64, expanded: bool) -> Part {
        if local == 0 {
            return Part::Header;
        }
        if local >= self.height - 1 {
            return Part::Gap;
        }
        let mut offset = local - 1;
        if self.folded(expanded) && offset >= PREVIEW_HEAD {
            if offset == PREVIEW_HEAD {
                return Part::Fold;
            }
            offset = self.body_rows - PREVIEW_TAIL + offset - PREVIEW_HEAD - 1;
        }
        self.text.at_offset(offset).map_or(Part::Gap, Part::Body)
    }
    fn row(&self, local: u64, width: u16, expanded: bool) -> Line<'_> {
        match self.part_at(local, expanded) {
            Part::Header => Line::from(vec![
                Span::styled(
                    if self.role == Role::User {
                        "› "
                    } else {
                        "● "
                    },
                    Style::default().fg(self.role.color()),
                ),
                Span::styled(
                    elide(&self.title, usize::from(width.saturating_sub(2))),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Part::Fold => Line::from(Span::styled(
                format!(
                    "  … {} rows hidden · Ctrl-O expand",
                    self.body_rows - PREVIEW_HEAD - PREVIEW_TAIL
                ),
                Style::default().fg(Color::DarkGray),
            )),
            Part::Gap => Line::default(),
            Part::Body(position) => {
                let mut rows = self.text.styled_rows(self.text.offset_of(position), 1);
                let mut row = rows.pop().unwrap_or_default();
                row.spans.insert(0, Span::raw("  "));
                row
            }
        }
    }
}

fn elide(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut columns = 0;
    for grapheme in text.graphemes(true) {
        if columns + grapheme.width() > width - 1 {
            break;
        }
        columns += grapheme.width();
        result.push_str(grapheme);
    }
    result.push('…');
    result
}

#[derive(Default)]
pub(crate) struct Chat {
    blocks: BTreeMap<u64, Block>,
    rows: BTreeMap<u64, u64>,
    next_id: u64,
    next_row: u64,
    dirty: Option<u64>,
    bytes: usize,
    lines: usize,
    width: u16,
    expanded: bool,
    expired: bool,
}

impl Chat {
    pub(crate) fn begin(&mut self, role: Role, title: &str, source: Source) -> u64 {
        // Headers are labels, not a second unbounded copy of tool arguments.
        let title: String = title
            .chars()
            .take(512)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let block = Block {
            role,
            title,
            text: Transcript::with_source(source.clone()),
            source,
            start: None,
            body_rows: 0,
            height: 2,
        };
        let id = self.next_id;
        self.next_id += 1;
        self.bytes += block.size();
        self.lines += block.lines();
        self.blocks.insert(id, block);
        self.mark_dirty(id);
        self.enforce_limits();
        id
    }

    pub(crate) fn contains(&self, id: u64) -> bool {
        self.blocks.contains_key(&id)
    }

    pub(crate) fn append(&mut self, id: u64, text: &str) {
        if text.is_empty() {
            return;
        }
        let Some(block) = self.blocks.get_mut(&id) else {
            return;
        };
        self.bytes -= block.size();
        self.lines -= block.lines();
        block.text.append(text);
        self.bytes += block.size();
        self.lines += block.lines();
        self.expired |= block.text.expired();
        self.mark_dirty(id);
        self.enforce_limits();
    }

    pub(crate) fn replace(&mut self, id: u64, text: &str) {
        let Some(block) = self.blocks.get_mut(&id) else {
            return;
        };
        self.bytes -= block.size();
        self.lines -= block.lines();
        block.text = Transcript::with_source(block.source.clone());
        block.text.append(text);
        self.bytes += block.size();
        self.lines += block.lines();
        self.expired |= block.text.expired();
        self.mark_dirty(id);
        self.enforce_limits();
    }

    pub(crate) fn heading(&mut self, id: u64, role: Role, title: &str) {
        let Some(block) = self.blocks.get_mut(&id) else {
            return;
        };
        self.bytes -= block.title.len();
        block.title = title
            .chars()
            .take(512)
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        block.role = role;
        self.bytes += block.title.len();
        self.mark_dirty(id);
        self.enforce_limits();
    }

    fn mark_dirty(&mut self, id: u64) {
        self.dirty = Some(self.dirty.map_or(id, |old| old.min(id)));
    }

    fn enforce_limits(&mut self) {
        while self.bytes > MAX_TRANSCRIPT_BYTES || self.lines > MAX_TRANSCRIPT_LINES {
            self.expired = true;
            if self.blocks.len() > 1 {
                let (_, block) = self.blocks.pop_first().expect("oldest block");
                self.bytes -= block.size();
                self.lines -= block.lines();
                if let Some(start) = block.start {
                    self.rows.remove(&start);
                }
            } else {
                let (&id, block) = self.blocks.first_key_value().expect("retained block");
                let allowance = MAX_TRANSCRIPT_BYTES.saturating_sub(block.title.len() + 1);
                let block = self.blocks.get_mut(&id).expect("retained block");
                block
                    .text
                    .trim_to_limits(allowance, MAX_TRANSCRIPT_LINES - 2);
                self.bytes = block.size();
                self.lines = block.lines();
                self.mark_dirty(id);
            }
        }
    }

    pub(crate) fn layout(&mut self, width: u16) -> usize {
        let width = width.max(1);
        if self.width != width {
            self.width = width;
            self.rows.clear();
            self.next_row = 0;
            for block in self.blocks.values_mut() {
                block.start = None;
            }
            self.dirty = self.blocks.first_key_value().map(|(id, _)| *id);
        }
        let Some(first) = self.dirty.take() else {
            return 0;
        };
        let Some((&first, block)) = self.blocks.range(first..).next() else {
            return 0;
        };
        let mut row = block.start.unwrap_or(self.next_row);
        drop(self.rows.split_off(&row));
        let mut examined = 0;
        for (&id, block) in self.blocks.range_mut(first..) {
            examined += block.text.layout(width.saturating_sub(2).max(1));
            block.body_rows = block.text.row_count();
            block.height = 2 + block.body_height(self.expanded);
            block.start = Some(row);
            self.rows.insert(row, id);
            row += block.height;
        }
        self.next_row = row;
        examined
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
    pub(crate) fn expired(&self) -> bool {
        self.expired
    }
    pub(crate) fn expanded(&self) -> bool {
        self.expanded
    }

    pub(crate) fn toggle(&mut self) {
        self.expanded = !self.expanded;
        self.dirty = self.blocks.first_key_value().map(|(id, _)| *id);
    }

    fn first_row(&self) -> u64 {
        self.rows.first_key_value().map_or(0, |(row, _)| *row)
    }
    fn tail_top(&self, height: u16) -> u64 {
        self.first_row()
            .max(self.next_row.saturating_sub(u64::from(height)))
    }
    fn row_of(&self, anchor: Anchor) -> u64 {
        self.blocks
            .get(&anchor.block)
            .map(|block| {
                block.start.unwrap_or(self.next_row) + block.local_row(anchor.part, self.expanded)
            })
            .or_else(|| {
                self.blocks
                    .range(anchor.block..)
                    .next()
                    .and_then(|(_, block)| block.start)
            })
            .unwrap_or_else(|| self.next_row.saturating_sub(1))
    }
    fn at_row(&self, row: u64) -> Option<Anchor> {
        let row = row
            .max(self.first_row())
            .min(self.next_row.saturating_sub(1));
        let (&start, &id) = self.rows.range(..=row).next_back()?;
        Some(Anchor {
            block: id,
            part: self.blocks[&id].part_at(row - start, self.expanded),
        })
    }
    pub(crate) fn oldest(&self) -> Option<Anchor> {
        self.at_row(self.first_row())
    }
    pub(crate) fn scroll(&self, anchor: Option<Anchor>, delta: i64, height: u16) -> Option<Anchor> {
        let tail = self.tail_top(height);
        let top = anchor
            .map_or(tail, |a| self.row_of(a))
            .clamp(self.first_row(), tail);
        let target = top
            .saturating_add_signed(delta)
            .clamp(self.first_row(), tail);
        if target == tail {
            None
        } else {
            self.at_row(target)
        }
    }
    pub(crate) fn scroll_metrics(&self, anchor: Option<Anchor>, height: u16) -> (usize, usize) {
        let top = anchor
            .map_or(self.tail_top(height), |a| self.row_of(a))
            .clamp(self.first_row(), self.tail_top(height));
        (
            (self.next_row - self.first_row()) as usize,
            (top - self.first_row()) as usize,
        )
    }
    pub(crate) fn window(&self, anchor: Option<Anchor>, height: u16) -> Vec<Line<'_>> {
        let mut result = Vec::with_capacity(usize::from(height));
        if height == 0 || self.rows.is_empty() {
            return result;
        }
        let top = anchor
            .map_or(self.tail_top(height), |a| self.row_of(a))
            .clamp(self.first_row(), self.tail_top(height));
        let (&first, _) = self.rows.range(..=top).next_back().expect("visible block");
        for (&start, &id) in self.rows.range(first..) {
            let block = &self.blocks[&id];
            for local in top.saturating_sub(start)..block.height {
                result.push(block.row(local, self.width, self.expanded));
                if result.len() == usize::from(height) {
                    return result;
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(chat: &Chat, anchor: Option<Anchor>, height: u16) -> String {
        chat.window(anchor, height)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn compact_wrapped_rows_expand_without_losing_content() {
        let mut chat = Chat::default();
        let id = chat.begin(Role::Assistant, "Assistant", Source::Plain);
        chat.append(id, &("abcdefghij".repeat(80) + "TAIL"));
        chat.layout(12);
        assert_eq!(chat.blocks[&id].height, 11);
        assert!(text(&chat, None, 30).contains("rows hidden"));
        assert!(text(&chat, None, 30).contains("TAIL"));
        chat.toggle();
        assert_eq!(chat.layout(12), 0, "expansion must reuse cached wrapping");
        assert!(!text(&chat, None, 100).contains("rows hidden"));
        assert!(text(&chat, None, 100).contains("TAIL"));
    }

    #[test]
    fn full_view_anchor_survives_new_output_resize_and_toggle() {
        let mut chat = Chat::default();
        chat.toggle();
        let id = chat.begin(Role::Assistant, "Assistant", Source::Plain);
        for row in 0..100 {
            chat.append(id, &format!("row-{row:03} abcdefghijklmnopqrstuvwxyz\n"));
        }
        chat.layout(80);
        let anchor = chat.scroll(None, -35, 10);
        let before = text(&chat, anchor, 1);
        chat.append(id, "new tail\n");
        chat.layout(80);
        assert_eq!(text(&chat, anchor, 1), before);
        chat.layout(20);
        assert!(before.starts_with(&text(&chat, anchor, 1)));
        chat.toggle();
        chat.layout(20);
        assert!(text(&chat, anchor, 10).contains("hidden"));
        chat.toggle();
        chat.layout(20);
        assert!(before.starts_with(&text(&chat, anchor, 1)));
    }

    #[test]
    fn retained_blocks_and_indexes_remain_bounded() {
        let mut chat = Chat::default();
        for _ in 0..20_000 {
            let id = chat.begin(Role::Assistant, "Assistant", Source::Plain);
            chat.append(id, "short\n");
        }
        chat.layout(80);
        assert!(chat.expired());
        assert!(chat.bytes <= MAX_TRANSCRIPT_BYTES);
        assert!(chat.lines <= MAX_TRANSCRIPT_LINES);
        assert_eq!(chat.rows.len(), chat.blocks.len());
        let id = chat.begin(Role::Assistant, "Assistant", Source::Plain);
        chat.append(id, &"界".repeat(MAX_TRANSCRIPT_BYTES));
        chat.layout(1);
        assert!(chat.bytes <= MAX_TRANSCRIPT_BYTES);
        assert_eq!(chat.blocks.len(), 1);
        assert_eq!(chat.layout(1), 0);
        assert!(text(&chat, None, 5).contains('界'));
    }

    #[test]
    fn final_tool_receipt_replaces_streamed_output() {
        let mut chat = Chat::default();
        let id = chat.begin(Role::Running, "Running test", Source::Plain);
        chat.append(id, "original output");
        chat.replace(id, "original output");
        chat.heading(id, Role::Failed, "Failed test · exit 1");
        chat.layout(80);
        let view = text(&chat, None, 20);
        assert_eq!(view.matches("original output").count(), 1);
        assert!(view.contains("Failed test"));
    }
}

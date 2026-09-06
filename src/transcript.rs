//! Bounded retained chat and an ordered index of cached visual rows.
use std::collections::BTreeMap;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) const MAX_TRANSCRIPT_BYTES: usize = 1024 * 1024;
const MAX_TRANSCRIPT_LINES: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Position {
    line: u64,
    byte: u64,
}

#[derive(Default)]
struct LogicalLine {
    text: String,
    origin: u64,
    starts: Vec<u32>,
    first_row: Option<u64>,
    width: u16,
}

impl LogicalLine {
    fn reflow(&mut self, width: u16) -> usize {
        // Revisit the last two rows when a streaming fragment extends a line.
        // A combining mark or ZWJ can extend the previous final grapheme.
        let from = if self.width == width && !self.starts.is_empty() {
            let keep = self.starts.len().saturating_sub(2);
            let from = self.starts[keep] as usize;
            self.starts.truncate(keep + 1);
            from
        } else {
            self.starts.clear();
            self.starts.push(0);
            0
        };
        let mut columns = 0;
        for (offset, grapheme) in self.text[from..].grapheme_indices(true) {
            let cells = grapheme.width();
            if columns > 0 && columns + cells > usize::from(width) {
                self.starts.push((from + offset) as u32);
                columns = 0;
            }
            columns += cells;
        }
        self.width = width;
        self.text.len() - from
    }
}

#[derive(Default)]
pub(crate) struct Transcript {
    lines: BTreeMap<u64, LogicalLine>,
    // Only one entry per logical line. Soft-wrap offsets stay in that line.
    rows: BTreeMap<u64, u64>,
    next_line: u64,
    next_row: u64,
    bytes: usize,
    width: u16,
    dirty_from: Option<u64>,
    expired: bool,
}

pub(crate) struct Window<'a> {
    pub(crate) rows: Vec<&'a str>,
    #[cfg(test)]
    visited_rows: usize,
    #[cfg(test)]
    visited_lines: usize,
}

impl Transcript {
    pub(crate) fn append(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // Never make a second whole-event allocation, even for oversized input.
        let mut fragment = String::with_capacity(4096);
        for character in text.chars() {
            match character {
                '\n' => {
                    self.append_fragment(&fragment);
                    fragment.clear();
                    self.ensure_line();
                    self.new_line();
                }
                '\t' => fragment.push_str("    "),
                value if value.is_control() => fragment.push('\u{fffd}'),
                value => fragment.push(value),
            }
            if fragment.len() >= 4096 {
                self.append_fragment(&fragment);
                fragment.clear();
            }
        }
        self.append_fragment(&fragment);
    }

    fn append_fragment(&mut self, fragment: &str) {
        if fragment.is_empty() {
            return;
        }
        self.ensure_line();
        let id = *self.lines.last_key_value().expect("current line").0;
        self.lines
            .get_mut(&id)
            .expect("current line")
            .text
            .push_str(fragment);
        self.bytes += fragment.len();
        self.dirty(id);
        self.enforce_limits();
    }

    fn ensure_line(&mut self) {
        if self.lines.is_empty() {
            self.new_line();
        }
    }

    fn new_line(&mut self) {
        let id = self.next_line;
        self.next_line += 1;
        self.lines.insert(id, LogicalLine::default());
        // Charge a byte for the line boundary, including the open final line.
        self.bytes += 1;
        self.dirty(id);
        self.enforce_limits();
    }

    fn dirty(&mut self, id: u64) {
        self.dirty_from = Some(self.dirty_from.map_or(id, |first| first.min(id)));
    }

    fn enforce_limits(&mut self) {
        while self.bytes > MAX_TRANSCRIPT_BYTES || self.lines.len() > MAX_TRANSCRIPT_LINES {
            self.expired = true;
            if self.lines.len() > 1 {
                let (_, line) = self.lines.pop_first().expect("oldest line");
                self.bytes -= line.text.len() + 1;
                if let Some(row) = line.first_row {
                    self.rows.remove(&row);
                }
            } else {
                let (id, line) = self.lines.first_key_value().expect("oversized line");
                let id = *id;
                let mut cut = self.bytes - MAX_TRANSCRIPT_BYTES;
                while !line.text.is_char_boundary(cut) {
                    cut += 1;
                }
                let line = self.lines.get_mut(&id).expect("oversized line");
                // Replace the allocation too: draining would keep a large capacity.
                line.text = line.text[cut..].to_owned();
                line.origin += cut as u64;
                line.starts.clear();
                line.width = 0;
                self.bytes -= cut;
                self.dirty(id);
            }
        }
    }

    /// Reflow changed text, or all retained text after a width change. The result
    /// is the number of text bytes examined, used by deterministic work checks.
    pub(crate) fn layout(&mut self, width: u16) -> usize {
        let width = width.max(1);
        if width != self.width {
            self.rows.clear();
            self.next_row = 0;
            for line in self.lines.values_mut() {
                line.first_row = None;
                line.width = 0;
            }
            self.dirty_from = self.lines.first_key_value().map(|(id, _)| *id);
            self.width = width;
        }
        let Some(first) = self.dirty_from.take() else {
            return 0;
        };
        let Some((&first, line)) = self.lines.range(first..).next() else {
            return 0;
        };
        let mut row = line.first_row.unwrap_or(self.next_row);
        drop(self.rows.split_off(&row));
        let mut examined = 0;
        for (&id, line) in self.lines.range_mut(first..) {
            examined += line.reflow(width);
            line.first_row = Some(row);
            self.rows.insert(row, id);
            row += line.starts.len() as u64;
        }
        self.next_row = row;
        examined
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub(crate) fn expired(&self) -> bool {
        self.expired
    }

    fn first_row(&self) -> u64 {
        self.rows.first_key_value().map_or(0, |(row, _)| *row)
    }

    fn tail_top(&self, height: u16) -> u64 {
        self.first_row()
            .max(self.next_row.saturating_sub(u64::from(height)))
    }

    fn row_of(&self, position: Position) -> u64 {
        if let Some(line) = self.lines.get(&position.line) {
            let index = line
                .starts
                .partition_point(|start| line.origin + u64::from(*start) <= position.byte)
                .saturating_sub(1);
            return line.first_row.unwrap_or(self.next_row) + index as u64;
        }
        self.lines
            .range(position.line..)
            .next()
            .and_then(|(_, line)| line.first_row)
            .unwrap_or_else(|| self.next_row.saturating_sub(1))
    }

    fn position_at(&self, row: u64) -> Option<Position> {
        let row = row
            .max(self.first_row())
            .min(self.next_row.saturating_sub(1));
        let (&start, &id) = self.rows.range(..=row).next_back()?;
        let line = &self.lines[&id];
        Some(Position {
            line: id,
            byte: line.origin + u64::from(line.starts[(row - start) as usize]),
        })
    }

    pub(crate) fn oldest(&self) -> Option<Position> {
        self.position_at(self.first_row())
    }

    pub(crate) fn scroll(
        &self,
        current: Option<Position>,
        delta: i64,
        height: u16,
    ) -> Option<Position> {
        let tail = self.tail_top(height);
        let top = current
            .map_or(tail, |position| self.row_of(position))
            .clamp(self.first_row(), tail);
        let target = top
            .saturating_add_signed(delta)
            .clamp(self.first_row(), tail);
        if target == tail {
            None
        } else {
            self.position_at(target)
        }
    }

    pub(crate) fn window(&self, position: Option<Position>, height: u16) -> Window<'_> {
        let mut window = Window {
            rows: Vec::with_capacity(usize::from(height)),
            #[cfg(test)]
            visited_rows: 0,
            #[cfg(test)]
            visited_lines: 0,
        };
        if height == 0 || self.rows.is_empty() {
            return window;
        }
        let tail = self.tail_top(height);
        let top = position
            .map_or(tail, |value| self.row_of(value))
            .clamp(self.first_row(), tail);
        let (&start, _) = self.rows.range(..=top).next_back().expect("visible row");
        for (&start, &id) in self.rows.range(start..) {
            #[cfg(test)]
            {
                window.visited_lines += 1;
            }
            let line = &self.lines[&id];
            let first = top.saturating_sub(start) as usize;
            for index in first..line.starts.len() {
                #[cfg(test)]
                {
                    window.visited_rows += 1;
                }
                let start = line.starts[index] as usize;
                let end = line
                    .starts
                    .get(index + 1)
                    .map_or(line.text.len(), |end| *end as usize);
                window.rows.push(&line.text[start..end]);
                if window.rows.len() == usize::from(height) {
                    return window;
                }
            }
        }
        window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_fragments_and_many_newlines_have_bounded_metadata() {
        let mut transcript = Transcript::default();
        for _ in 0..100_000 {
            transcript.append("");
        }
        assert!(transcript.lines.is_empty());
        assert_eq!(transcript.bytes, 0);
        transcript.append(&"\n".repeat(MAX_TRANSCRIPT_LINES * 3));
        transcript.layout(80);
        assert_eq!(transcript.lines.len(), MAX_TRANSCRIPT_LINES);
        assert!(transcript.rows.len() <= MAX_TRANSCRIPT_LINES);
        assert!(transcript.expired());
    }

    #[test]
    fn oversized_unicode_retains_a_bounded_valid_tail_and_expires_old_anchors() {
        let mut transcript = Transcript::default();
        transcript.append("first\nsecond\n");
        transcript.layout(20);
        let old = transcript.oldest();
        transcript.append(&"界".repeat(MAX_TRANSCRIPT_BYTES));
        transcript.layout(20);
        assert!(transcript.bytes <= MAX_TRANSCRIPT_BYTES);
        assert!(transcript.expired());
        let capacity: usize = transcript
            .lines
            .values()
            .map(|line| line.text.capacity())
            .sum();
        assert!(capacity <= MAX_TRANSCRIPT_BYTES * 2);
        assert!(
            transcript
                .window(old, 10)
                .rows
                .iter()
                .all(|row| !row.contains("first"))
        );
        assert!(
            transcript
                .window(None, 10)
                .rows
                .iter()
                .any(|row| row.contains('界'))
        );
    }

    #[test]
    fn idle_layout_and_visible_rows_do_not_scan_the_history() {
        let mut transcript = Transcript::default();
        transcript.append(&"abcdefghij".repeat(80_000));
        assert_eq!(transcript.layout(80), 800_000);
        assert_eq!(transcript.layout(80), 0);
        let window = transcript.window(None, 24);
        assert_eq!(window.rows.len(), 24);
        assert_eq!(window.visited_rows, 24);
        assert_eq!(window.visited_lines, 1);
        transcript.append("z");
        assert!(
            transcript.layout(80) <= 161,
            "append reflow scanned old rows"
        );
        assert_eq!(transcript.layout(80), 0);
    }

    #[test]
    fn many_lines_and_resizes_keep_the_index_bounded_and_visit_only_visible_lines() {
        let mut transcript = Transcript::default();
        for index in 0..10_000 {
            transcript.append(&format!("line-{index:05} abcdefghijklmnopqrstuvwxyz\n"));
        }
        for width in [80, 1, 200, 2, 50] {
            transcript.layout(width);
            let window = transcript.window(None, 24);
            assert_eq!(window.rows.len(), 24);
            assert_eq!(window.visited_rows, 24);
            assert!(window.visited_lines <= 24);
            let offset_capacity: usize = transcript
                .lines
                .values()
                .map(|line| line.starts.capacity())
                .sum();
            assert!(offset_capacity <= MAX_TRANSCRIPT_BYTES * 2 + MAX_TRANSCRIPT_LINES * 4);
            assert!(transcript.rows.len() <= MAX_TRANSCRIPT_LINES);
            assert_eq!(transcript.layout(width), 0);
        }
    }

    #[test]
    fn scroll_anchor_survives_new_output_and_resize_until_expiry() {
        let mut transcript = Transcript::default();
        for index in 0..100 {
            transcript.append(&format!("row-{index:03} abcdefghijklmnopqrstuvwxyz\n"));
        }
        transcript.layout(80);
        let anchor = transcript.scroll(None, -30, 10);
        let first = transcript.window(anchor, 10).rows[0].to_owned();
        transcript.append("new live output\n");
        transcript.layout(80);
        assert_eq!(transcript.window(anchor, 10).rows[0], first);
        transcript.layout(12);
        assert!(first.starts_with(transcript.window(anchor, 10).rows[0]));
        assert!(
            transcript
                .window(None, 10)
                .rows
                .iter()
                .any(|row| row.contains("new live"))
        );
        assert!(transcript.scroll(anchor, i64::MAX, 10).is_none());
    }

    #[test]
    fn streaming_graphemes_match_a_single_complete_append() {
        let text = "abc e\u{301} 界 👩\u{200d}💻 🇺🇸 xyz";
        for width in [2, 3, 4, 7, 12] {
            let mut complete = Transcript::default();
            complete.append(text);
            complete.layout(width);
            let expected = complete
                .window(complete.oldest(), 100)
                .rows
                .iter()
                .map(|row| (*row).to_owned())
                .collect::<Vec<_>>();
            let mut streaming = Transcript::default();
            for character in text.chars() {
                streaming.append(&character.to_string());
                streaming.layout(width);
            }
            assert_eq!(
                streaming.window(streaming.oldest(), 100).rows,
                expected,
                "width {width}"
            );
        }
    }

    #[test]
    fn rows_beyond_u16_are_reachable_and_controls_stay_literal() {
        let mut transcript = Transcript::default();
        transcript.append(&"x".repeat(100_000));
        transcript.append("Z");
        transcript.layout(1);
        assert_eq!(transcript.window(None, 3).rows, vec!["x", "x", "Z"]);
        let old = transcript.scroll(None, -90_000, 3);
        assert_eq!(transcript.window(old, 3).rows, vec!["x", "x", "x"]);
        let mut transcript = Transcript::default();
        transcript.append("\x1b[2J\tend");
        transcript.layout(80);
        assert_eq!(transcript.window(None, 2).rows, vec!["�[2J    end"]);
    }
}

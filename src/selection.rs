//! A bounded snapshot of the last displayed rows, independent of live output.
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) const MAX_SELECTION_BYTES: usize = 256 * 1024;
const MAX_SELECTION_ROWS: usize = 1024;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Point {
    row: usize,
    column: usize,
}

pub(crate) struct Selection {
    rows: Vec<Line<'static>>,
    start: Point,
    end: Point,
    pub(crate) dragging: bool,
}

impl Selection {
    pub(crate) fn snapshot(rows: &[Line<'_>]) -> Option<Vec<Line<'static>>> {
        let bytes: usize = rows
            .iter()
            .map(|line| line.spans.iter().map(|s| s.content.len()).sum::<usize>() + 1)
            .sum();
        if rows.is_empty() || rows.len() > MAX_SELECTION_ROWS || bytes > MAX_SELECTION_BYTES {
            return None;
        }
        Some(
            rows.iter()
                .map(|line| {
                    let mut owned = Line::from(
                        line.spans
                            .iter()
                            .map(|span| Span::styled(span.content.to_string(), span.style))
                            .collect::<Vec<_>>(),
                    )
                    .style(line.style);
                    owned.alignment = line.alignment;
                    owned
                })
                .collect(),
        )
    }

    pub(crate) fn new(rows: Vec<Line<'static>>, row: u16, column: u16) -> Self {
        let point = Point {
            row: usize::from(row).min(rows.len().saturating_sub(1)),
            column: column.into(),
        };
        Self {
            rows,
            start: point,
            end: point,
            dragging: true,
        }
    }

    pub(crate) fn rows(self) -> Vec<Line<'static>> {
        self.rows
    }

    pub(crate) fn update(&mut self, row: u16, column: u16) {
        self.end = Point {
            row: usize::from(row).min(self.rows.len().saturating_sub(1)),
            column: column.into(),
        };
    }

    fn bounds(&self, row: usize) -> Option<(usize, usize)> {
        let (start, end) = if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        };
        (row >= start.row && row <= end.row).then_some((
            if row == start.row { start.column } else { 0 },
            if row == end.row {
                end.column
            } else {
                usize::MAX
            },
        ))
    }

    pub(crate) fn text(&self) -> String {
        let mut rows = Vec::new();
        for (row, line) in self.rows.iter().enumerate() {
            let Some((start, end)) = self.bounds(row) else {
                continue;
            };
            let text = line.to_string();
            let mut column = 0;
            let mut selected = String::new();
            for grapheme in text.graphemes(true) {
                let next = column + grapheme.width();
                if column < end && next > start {
                    selected.push_str(grapheme);
                }
                column = next;
            }
            rows.push(selected);
        }
        rows.join("\n")
    }

    pub(crate) fn lines(&self, height: u16) -> Vec<Line<'static>> {
        self.rows
            .iter()
            .take(usize::from(height))
            .enumerate()
            .map(|(row, line)| {
                let Some((start, end)) = self.bounds(row) else {
                    return line.clone();
                };
                let mut column = 0;
                let mut spans = Vec::new();
                for span in &line.spans {
                    for grapheme in span.content.graphemes(true) {
                        let next = column + grapheme.width();
                        let style = if column < end && next > start {
                            span.style
                                .patch(Style::default().bg(Color::Cyan).fg(Color::Black))
                        } else {
                            span.style
                        };
                        spans.push(Span::styled(grapheme.to_owned(), style));
                        column = next;
                    }
                }
                Line::from(spans).style(line.style)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reverse_unicode_selection_uses_display_columns_and_keeps_snapshot_bytes() {
        let rows = vec![Line::from("a界e\u{301}z"), Line::from("second")];
        let mut selection = Selection::new(Selection::snapshot(&rows).unwrap(), 1, 3);
        selection.update(0, 1);
        assert_eq!(selection.text(), "界e\u{301}z\nsec");
        assert_eq!(selection.lines(1)[0].to_string(), "a界e\u{301}z");
        assert_eq!(selection.text(), "界e\u{301}z\nsec");
    }
    #[test]
    fn snapshot_rejects_excess_bytes_or_rows() {
        assert!(Selection::snapshot(&[Line::from("x".repeat(MAX_SELECTION_BYTES))]).is_none());
        assert!(Selection::snapshot(&vec![Line::default(); MAX_SELECTION_ROWS + 1]).is_none());
    }
}

//! A streaming page keeps at most one slice; skipped pages are never retained.
use std::fmt;

pub(super) const PAGE_BYTES: usize = 8192;
pub(super) const MAX_PAGE: usize = 16383;

pub(super) struct Pager {
    start: usize,
    end: usize,
    seen: usize,
    pub text: String,
    pub more: bool,
}

impl Pager {
    pub fn new(page: usize) -> Self {
        let start = page.min(MAX_PAGE) * PAGE_BYTES;
        Self {
            start,
            end: start + PAGE_BYTES,
            seen: 0,
            text: String::new(),
            more: false,
        }
    }
}

impl fmt::Write for Pager {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let before = self.seen;
        self.seen = self.seen.saturating_add(text.len());
        let start = self.start.saturating_sub(before).min(text.len());
        let end = self.end.saturating_sub(before).min(text.len());
        // The preceding page owns a character which crosses the byte boundary.
        let start = text.ceil_char_boundary(start);
        let end = text.ceil_char_boundary(end);
        if start < end {
            self.text.push_str(&text[start..end]);
        }
        if self.seen > self.end {
            self.more = true;
            // Formatting stops as soon as the next page is known to exist.
            return Err(fmt::Error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    #[test]
    fn pages_reconstruct_unicode_without_gaps_or_repetition() {
        let source = format!(
            "{}λ🙂{}\nEND",
            "a".repeat(PAGE_BYTES - 1),
            "b".repeat(PAGE_BYTES)
        );
        let mut joined = String::new();
        for page in 0..3 {
            let mut writer = Pager::new(page);
            let _ = write!(writer, "{source}");
            assert!(writer.text.len() <= PAGE_BYTES + 3);
            joined.push_str(&writer.text);
        }
        assert_eq!(joined, source);
    }

    #[test]
    fn chunk_boundaries_and_skipped_pages_preserve_every_byte() {
        let parts = [
            "x".repeat(PAGE_BYTES),
            "λ".repeat(PAGE_BYTES),
            "\nlast".into(),
        ];
        let mut joined = String::new();
        for page in 0..4 {
            let mut writer = Pager::new(page);
            for part in &parts {
                if writer.write_str(part).is_err() {
                    break;
                }
            }
            joined.push_str(&writer.text);
            if !writer.more {
                break;
            }
        }
        assert_eq!(joined, parts.concat());
    }
}

//! Incremental, bounded syntax colors. Text remains owned by the transcript.
use ratatui::style::{Color, Modifier, Style};
use std::sync::LazyLock;
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, HighlightState, Theme, ThemeSet},
    parsing::{ParseState, SyntaxSet},
};

const MAX_CODE_BYTES: usize = 256 * 1024;
const MAX_CODE_LINES: usize = 4096;
const MAX_LINE_BYTES: usize = 8192;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_nonewlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    ThemeSet::load_defaults()
        .themes
        .remove("base16-ocean.dark")
        .expect("bundled theme")
});

#[derive(Clone, Default)]
pub(crate) enum Source {
    #[default]
    Plain,
    Markdown,
    Code(String),
}

#[derive(Clone)]
pub(crate) struct State {
    markdown: bool,
    fence: Option<(char, usize)>,
    syntax: Option<(HighlightState, ParseState)>,
    bytes: usize,
    lines: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct ColorRange {
    pub(crate) end: usize,
    pub(crate) style: Style,
}

impl State {
    pub(crate) fn new(source: &Source) -> Self {
        Self {
            markdown: matches!(source, Source::Markdown),
            fence: None,
            syntax: match source {
                Source::Code(language) => Self::syntax(language),
                _ => None,
            },
            bytes: 0,
            lines: 0,
        }
    }

    fn syntax(language: &str) -> Option<(HighlightState, ParseState)> {
        let syntax = SYNTAXES.find_syntax_by_token(language)?;
        Some(HighlightLines::new(syntax, &THEME).state())
    }

    pub(crate) fn line(&mut self, text: &str) -> Vec<ColorRange> {
        if self.markdown {
            let trimmed = text.trim_start();
            let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
            if let Some(marker) = marker {
                let count = trimmed.chars().take_while(|c| *c == marker).count();
                if count >= 3 {
                    let rest = &trimmed[count..];
                    if let Some((open, length)) = self.fence {
                        if marker == open && count >= length && rest.trim().is_empty() {
                            self.fence = None;
                            self.syntax = None;
                            return Self::fence_style(text);
                        }
                    } else {
                        self.fence = Some((marker, count));
                        self.syntax = rest.split_whitespace().next().and_then(Self::syntax);
                        self.bytes = 0;
                        self.lines = 0;
                        return Self::fence_style(text);
                    }
                }
            }
            if self.fence.is_none() {
                return Vec::new();
            }
        }
        self.bytes = self.bytes.saturating_add(text.len());
        self.lines = self.lines.saturating_add(1);
        if text.len() > MAX_LINE_BYTES || self.bytes > MAX_CODE_BYTES || self.lines > MAX_CODE_LINES
        {
            // Drop expensive parser state for the rest of this code block.
            // Plain text still uses the ordinary bounded transcript and wrapping.
            self.syntax = None;
        }
        let Some((highlight, parse)) = self.syntax.take() else {
            return Vec::new();
        };
        let mut highlighter = HighlightLines::from_state(&THEME, highlight, parse);
        let Ok(ranges) = highlighter.highlight_line(text, &SYNTAXES) else {
            return Vec::new();
        };
        let mut end = 0;
        let mut colors: Vec<ColorRange> = Vec::new();
        for (syntax, text) in ranges {
            end += text.len();
            let color = syntax.foreground;
            let mut style = Style::default().fg(Color::Rgb(color.r, color.g, color.b));
            if syntax.font_style.contains(FontStyle::BOLD) {
                style = style.add_modifier(Modifier::BOLD);
            }
            if syntax.font_style.contains(FontStyle::ITALIC) {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if let Some(last) = colors.last_mut().filter(|last| last.style == style) {
                last.end = end;
            } else {
                colors.push(ColorRange { end, style });
            }
        }
        self.syntax = Some(highlighter.state());
        colors
    }

    fn fence_style(text: &str) -> Vec<ColorRange> {
        vec![ColorRange {
            end: text.len(),
            style: Style::default().fg(Color::DarkGray),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fenced_code_is_colored_and_unknown_languages_remain_plain() {
        let mut state = State::new(&Source::Markdown);
        assert!(state.line("ordinary prose").is_empty());
        state.line("```rust");
        let ranges = state.line("let greeting = \"hello\";");
        assert!(ranges.len() >= 3);
        assert!(ranges.windows(2).any(|pair| pair[0].style != pair[1].style));
        state.line("```");
        assert!(state.line("more prose").is_empty());
        state.line("```unknown-language");
        assert!(state.line("let x = 1;").is_empty());
    }

    #[test]
    fn multiline_code_state_and_oversized_fallback_are_bounded() {
        let mut state = State::new(&Source::Code("rs".into()));
        state.line("/* comment");
        let comment = state.line("still a comment");
        state.line("*/");
        let code = state.line("fn main() {}");
        assert_ne!(comment[0].style, code[0].style);
        assert!(state.line(&"x".repeat(MAX_LINE_BYTES + 1)).is_empty());
        assert!(state.line("fn main() {}").is_empty());
    }
}

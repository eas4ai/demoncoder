//! Cited improvement records. This module never executes corrections or grants tools.
pub(crate) mod catalog;
pub(crate) mod context;
pub(crate) mod control;
pub(crate) mod source;
pub(crate) mod state;
#[cfg(test)]
mod tests;

pub(crate) fn is_control(text: &str) -> bool {
    matches!(
        text.split_whitespace().next(),
        Some(
            "/improvements"
                | "/observation"
                | "/learning-context"
                | "/improvement"
                | "/improvement-note"
                | "/improvement-propose"
                | "/improve"
                | "/improvement-outcome"
                | "/lesson-propose"
                | "/lesson-enable"
                | "/lesson-disable"
                | "/lesson-supersede"
                | "/lesson"
        )
    )
}

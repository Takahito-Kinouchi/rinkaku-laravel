//! The TUI's mark and color vocabulary (ADR 0099 decision 7): one meaning
//! per mark and one mark per meaning, shared by every pane — the tree, the
//! Diff/Detail/Blast-radius panes and the help legend all draw from here,
//! so "removed" or "dependency cycle" can never look different in two
//! places again.
//!
//! Colors carry one meaning each, too: [`danger`] breaks or removes,
//! [`contract`] changes a contract, [`added`] adds, [`selection`] is what
//! the cursor is on and what connects to it, [`deferred`] can wait. A mark
//! whose meaning is none of those (a cycle, a review note) stays uncolored
//! rather than borrowing a color that already means something else.

use ratatui::style::{Color, Modifier, Style};

pub(crate) const ADDED: &str = "+";
pub(crate) const REMOVED: &str = "-";
/// Signature changed, few or no users.
pub(crate) const SIGNATURE_CHANGED: &str = "○";
/// Signature changed and other symbols use it: it may break them.
pub(crate) const MAY_BREAK: &str = "●";
pub(crate) const CYCLE: &str = "↻";
/// "Uses the symbol under the cursor" (ADR 0099 decision 3).
pub(crate) const USED_BY: &str = "↑";
/// "Used by the symbol under the cursor" (ADR 0099 decision 3).
pub(crate) const USES: &str = "↓";
/// A Blast-radius entry: "the pivot reaches this".
pub(crate) const REACHES: &str = "↓";
pub(crate) const NOTE: &str = "✎";
/// Checked off by the reviewer (ADR 0099 decision 4).
pub(crate) const CHECKED: &str = "✓";
pub(crate) const EXPANDED: &str = "▾";
pub(crate) const COLLAPSED: &str = "▸";
pub(crate) const MORE_ABOVE: &str = "▲";
pub(crate) const MORE_BELOW: &str = "▼";
pub(crate) const RANGE_BAR: &str = "┃";

pub(crate) fn danger() -> Style {
    Style::default().fg(Color::Red)
}

pub(crate) fn contract() -> Style {
    Style::default().fg(Color::Yellow)
}

pub(crate) fn added() -> Style {
    Style::default().fg(Color::Green)
}

pub(crate) fn selection() -> Style {
    Style::default().fg(Color::Cyan)
}

pub(crate) fn deferred() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// The style for a mark that has a meaning of its own but no color to go
/// with it (see the module doc comment).
pub(crate) fn uncolored_mark() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

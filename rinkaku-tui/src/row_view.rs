//! Pure formatting of one entry-view row into styled `ratatui` text
//! (stage B, ADR 0015/0016): given a `crate::nav::Row` (already resolved
//! against the tree/collapse state) plus its display label, produces the
//! [`ratatui::text::Line`] that `crate::ui` draws for it.
//!
//! `Line`/`Span`/`Style` are plain, `PartialEq`-comparable data — not
//! `Frame`/`Terminal` — so building one from a `Row` is a pure
//! transformation, unit-tested here the same way `crate::tree`/`crate::nav`
//! test their own plain-data outputs. Layout (which rows are visible at
//! all, where the split between panes falls, and computing each row's
//! `label` from ancestor context — see [`relative_label`]) is `crate::ui`'s
//! job; this module only decides one row's content and styling given that
//! label.

use crate::connections::{Connection, ProductionEdges};
use crate::glyph;
use crate::nav::Row;
use crate::order::DirRank;
use crate::tree::{Badges, NodeKind, SymbolRef};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use rinkaku_core::extract::Classification;
use rinkaku_core::file_size::FileSizeBand;
use std::collections::HashMap;

/// Indent width per tree depth level, in columns.
const INDENT_WIDTH: usize = 2;

/// Builds the styled [`Line`] for one visible row.
///
/// `label` is this row's display name: for a `Dir`/`File` row, the
/// segment(s) of `crate::tree::TreeNode::path` not already implied by an
/// ancestor row on screen (see [`relative_label`], which `crate::ui` uses
/// to compute it while walking the tree — a pure per-row function like
/// this one has no ancestor context of its own, since `crate::nav::Row`
/// only exposes one node). Unused for a `Symbol` row, which always
/// displays `SymbolRef::name` regardless of `label`.
///
/// `ranks` is consulted only for a [`NodeKind::Dir`] row, to show the
/// `(cycle)` warning marker (ADR 0016 decision 4 / ADR 0008's existing
/// symbol-level cycle warning) — a directory's own `crate::order::DirRank`
/// is looked up by its `path`, matching `crate::order`'s own map key.
/// `selected` styles the row for the cursor (reverse video), independent
/// of the node's own kind-based styling.
pub fn entry_row_line(
    row: &Row<'_>,
    label: &str,
    ranks: &HashMap<String, DirRank>,
    annotation_markers: &crate::annotation_markers::AnnotationMarkers,
    state: RowState,
) -> Line<'static> {
    let RowState {
        connection,
        checked,
        selected,
    } = state;
    let indent = " ".repeat(row.depth * INDENT_WIDTH);
    let mut spans = vec![Span::raw(indent)];

    match &row.node.kind {
        NodeKind::Dir => {
            let in_cycle = ranks.get(&row.node.path).is_some_and(|rank| rank.in_cycle);
            spans.push(Span::raw(format!("{} ", expand_marker(row))));
            push_hidden_risk_span(&mut spans, row);
            spans.push(Span::styled(
                label.to_string(),
                Style::default().add_modifier(Modifier::BOLD),
            ));
            if in_cycle {
                spans.push(Span::raw(" "));
                spans.push(cycle_marker_span());
            }
        }
        NodeKind::Section(section_kind) => {
            // `label` is unused: a section's path is a synthetic
            // constant, not a real file-tree path for `crate::ui`'s
            // ancestor-prefix stripping to compute a label from. No
            // `ranks` lookup either — that path never gets a `DirRank`
            // entry (`crate::order::rank_directories` only ranks real
            // directories).
            spans.push(Span::raw(format!("{} ", expand_marker(row))));
            spans.push(Span::styled(
                section_kind.label(),
                Style::default().add_modifier(Modifier::BOLD),
            ));
        }
        NodeKind::TestGroup { count } => {
            spans.push(Span::raw(format!("{} ", expand_marker(row))));
            spans.push(Span::styled(
                format!("{count} {}", if *count == 1 { "test" } else { "tests" }),
                Style::default().fg(Color::DarkGray),
            ));
        }
        NodeKind::File => {
            spans.push(Span::raw(format!("{} ", expand_marker(row))));
            // The `[test] (N symbols)` badge is placed *before* the file
            // label rather than trailing after the badges below: a long
            // label plus badges can overflow the pane width, and a
            // trailing badge is the first thing truncation clips away
            // (see `crate::ui`'s row-truncation), which would hide the
            // one signal ("this is a test file") a reviewer most needs
            // to keep visible at a glance.
            if let Some(count) = row.node.test_symbol_count {
                spans.push(test_badge_span(count));
                spans.push(Span::raw(" "));
            }
            push_hidden_risk_span(&mut spans, row);
            spans.push(Span::styled(label.to_string(), file_label_style(row.node)));
            push_oversized_file_span(&mut spans, &row.node.badges);
            push_annotation_badge_span(
                &mut spans,
                annotation_markers.file_counts.get(&row.node.path).copied(),
            );
            // `skip_reason` is mutually exclusive with `test_symbol_count`
            // (`crate::tree::TreeNode::skip_reason`'s own doc comment: a
            // skipped file has no `FileReport`/`TestFileSummary` entry of
            // its own), so a skip reason and the test badge never both
            // need to render for the same row.
            if let Some(reason) = row.node.skip_reason {
                spans.push(Span::raw(" "));
                spans.push(skip_reason_span(reason));
            }
        }
        NodeKind::Symbol(symbol_ref) => {
            spans.push(connection_span(connection));
            if checked {
                spans.push(Span::styled(glyph::CHECKED, glyph::deferred()));
            } else {
                spans.push(symbol_marker_span(symbol_ref, &row.node.badges));
            }
            spans.push(Span::raw(" "));
            let name_style = if checked {
                glyph::deferred()
            } else {
                symbol_name_style(symbol_ref)
            };
            spans.push(Span::styled(symbol_ref.name.clone(), name_style));
            push_annotation_badge_span(
                &mut spans,
                annotation_markers
                    .symbol_counts
                    .get(&symbol_ref.id)
                    .copied(),
            );
        }
    }

    let mut line = Line::from(spans);
    if connection == Connection::Unrelated && !selected {
        line = line.style(Style::default().add_modifier(Modifier::DIM));
    }
    if selected {
        line = line.style(Style::default().add_modifier(Modifier::REVERSED));
    }
    line
}

/// The expand/collapse indicator for a `Dir`/`File` row: `" "` (blank) for
/// a childless node (nothing to expand), [`glyph::EXPANDED`] when its
/// children are currently shown, [`glyph::COLLAPSED`] when collapsed.
fn expand_marker(row: &Row<'_>) -> &'static str {
    if row.node.children.is_empty() {
        " "
    } else if row.expanded {
        glyph::EXPANDED
    } else {
        glyph::COLLAPSED
    }
}

/// What `crate::ui::entry` knows about a row beyond the tree itself: how
/// it relates to the cursor's symbol (ADR 0099 decision 3), whether the
/// reviewer has checked it off (decision 4), and whether the cursor is on
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowState {
    pub connection: Connection,
    /// A checked symbol row trades its mark for a gray `✓` and dims its
    /// name: read, so it can wait.
    pub checked: bool,
    pub selected: bool,
}

impl Default for RowState {
    fn default() -> Self {
        Self {
            connection: Connection::Neutral,
            checked: false,
            selected: false,
        }
    }
}

/// The two columns in front of a symbol row's mark: `↑` when it uses the
/// symbol under the cursor, `↓` when that symbol uses it (ADR 0099
/// decision 3), blank otherwise.
fn connection_span(connection: Connection) -> Span<'static> {
    match connection {
        Connection::UsesCursor => Span::styled(format!("{} ", glyph::USED_BY), glyph::selection()),
        Connection::UsedByCursor => Span::styled(format!("{} ", glyph::USES), glyph::selection()),
        Connection::Neutral | Connection::Unrelated => Span::raw("  "),
    }
}

/// The dependency-cycle mark (ADR 0099: `↻` everywhere a cycle is shown).
///
/// `pub(crate)`: also reused by `crate::ui::help_overlay`'s Markers legend.
pub(crate) fn cycle_marker_span() -> Span<'static> {
    Span::styled(glyph::CYCLE, glyph::uncolored_mark())
}

/// Computes the display label for every row in `rows` (as returned by
/// `crate::nav::Nav::rows`): a divider row (a top-level directory) shows
/// its full path, and a file row under it shows its path relative to that
/// divider — `render/markdown.rs` under `rinkaku-core/src` — since the
/// directories between the two get no row of their own (ADR 0099 decision
/// 2). A file with no divider above it (a top-level file, or one under the
/// synthetic Tests section, whose path is no prefix of anything) shows its
/// full path.
///
/// `rows` is a pre-order flattening (`Nav::rows`' own doc comment), so the
/// most recent divider seen while scanning forward is the one a file sits
/// under, and a later top-level file is never mistaken for one: a
/// top-level file is a sibling of the divider, not under it, so the
/// divider's path is no prefix of its path. A `Symbol` row's label is never
/// consulted by `entry_row_line` (it always shows `SymbolRef::name`), so
/// this returns an empty string for those rows.
pub fn relative_labels(rows: &[Row<'_>]) -> Vec<String> {
    let mut divider_path: Option<&str> = None;
    rows.iter()
        .map(|row| match &row.node.kind {
            NodeKind::Dir => {
                divider_path = Some(row.node.path.as_str());
                row.node.path.clone()
            }
            NodeKind::Section(_) => {
                divider_path = Some(row.node.path.as_str());
                String::new()
            }
            NodeKind::File => divider_path
                .and_then(|divider| row.node.path.strip_prefix(divider))
                .and_then(|rest| rest.strip_prefix('/'))
                .unwrap_or(&row.node.path)
                .to_string(),
            NodeKind::Symbol(_) | NodeKind::TestGroup { .. } => String::new(),
        })
        .collect()
}

/// How many other production symbols use this row's symbol (ADR 0099
/// decision 1), for `crate::ui::entry` to draw flush right: `Some` only on
/// a symbol row with users. Read from the same edge index as the `↑`
/// arrows and the Diff header's user list, so the three always agree.
pub(crate) fn row_uses_count(row: &Row<'_>, edges: &ProductionEdges) -> Option<usize> {
    match &row.node.kind {
        NodeKind::Symbol(symbol_ref) => {
            Some(edges.users_of(&symbol_ref.id).len()).filter(|&count| count > 0)
        }
        _ => None,
    }
}

/// The style for a row's [`row_uses_count`] number: as red as the row's own
/// `●` when the symbol may break its users, otherwise gray.
pub(crate) fn uses_count_style(row: &Row<'_>) -> Style {
    match &row.node.kind {
        NodeKind::Symbol(symbol_ref) if is_high_risk_symbol(symbol_ref, &row.node.badges) => {
            glyph::danger()
        }
        _ => glyph::deferred(),
    }
}

/// Appends `N lines` to a file row whose size is past the warn threshold
/// (ADR 0028's Warn/Split bands) — the only files whose size calls for
/// action, so the only ones that show it (ADR 0099 decision 1).
fn push_oversized_file_span(spans: &mut Vec<Span<'static>>, badges: &Badges) {
    let (Some(band), Some(line_count)) = (badges.own_file_size_band, badges.own_file_line_count)
    else {
        return;
    };
    if matches!(band, FileSizeBand::Warn | FileSizeBand::Split) {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("{line_count} lines"),
            band_style(band),
        ));
    }
}

/// Appends a `✎N` badge (ADR 0048, ADR 0058, glyph per ADR 0099) — "this
/// row has N review annotations attached" — a no-op when `count` is `None`
/// or `0`. Kept as its
/// own function rather than folded into [`push_badge_spans`]: that
/// function's `Badges` input is baked once at tree-build time
/// (`crate::tree::build_tree`'s own doc comment), while annotation counts
/// change during the session (`crate::annotation_markers`'s own module doc
/// comment) — a genuinely different data source, not just a different
/// badge label. `ann:` rather than `n:` (ADR 0058) — `n` no longer names
/// this feature's key binding, so keeping it as the badge abbreviation
/// would be a stale mnemonic.
fn push_annotation_badge_span(spans: &mut Vec<Span<'static>>, count: Option<usize>) {
    let Some(count) = count.filter(|&count| count > 0) else {
        return;
    };
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        format!("{}{count}", glyph::NOTE),
        glyph::uncolored_mark(),
    ));
}

/// The `N lines` style for an oversized file (ADR 0028's bands): red for
/// Warn, bold red for Split. A Normal/Watch file shows no line count at all
/// (ADR 0099), so those bands have no style of their own.
///
/// `pub(crate)`: also reused by `crate::ui::overlay`'s Markers legend, the
/// single source of truth for that legend's swatch styles.
pub(crate) fn band_style(band: FileSizeBand) -> Style {
    match band {
        FileSizeBand::Normal | FileSizeBand::Watch => Style::default(),
        FileSizeBand::Warn => glyph::danger(),
        FileSizeBand::Split => glyph::danger().add_modifier(Modifier::BOLD),
    }
}

/// A `File` row's label style. A whole-test-file row stays undimmed and
/// carries its own `[test]` badge (see `test_badge_span`) instead, since a
/// test file is not "uninteresting", just excluded from the default
/// symbol-level view (ADR 0009).
fn file_label_style(node: &crate::tree::TreeNode) -> Style {
    match node.skip_reason {
        Some(reason) if has_no_readable_content(reason) => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}

/// Whether a skipped file has nothing left for a reviewer to read — what
/// ADR 0043's "read bright rows, skim dimmed ones" protocol encodes as
/// dimming. `UnsupportedLanguage` is excluded: lacking a grammar says
/// nothing about a file's review importance, and dimming it would steer
/// the reviewer away from the one case rinkaku cannot help with.
///
/// `pub(crate)`: `crate::ui::detail_pane` splits on the same distinction.
pub(crate) fn has_no_readable_content(reason: rinkaku_core::render::SkipReason) -> bool {
    match reason {
        rinkaku_core::render::SkipReason::UnsupportedLanguage => false,
        rinkaku_core::render::SkipReason::Binary
        | rinkaku_core::render::SkipReason::Deleted
        | rinkaku_core::render::SkipReason::Generated
        // ADR 0090: the file was never read, and opening it is exactly
        // what the skip exists to prevent.
        | rinkaku_core::render::SkipReason::OutsideRepository
        // ADR 0094: the read was attempted and failed, so there is no
        // content behind this row either.
        | rinkaku_core::render::SkipReason::Unreadable => true,
    }
}

/// The `(skipped: <reason>)` annotation for a skipped `File` row, reusing
/// [`rinkaku_core::render::skip_reason_label`]'s exact wording so the TUI
/// and Markdown output never describe the same `SkipReason` differently.
fn skip_reason_span(reason: rinkaku_core::render::SkipReason) -> Span<'static> {
    Span::styled(
        format!(
            "(skipped: {})",
            rinkaku_core::render::skip_reason_label(reason)
        ),
        Style::default().fg(Color::DarkGray),
    )
}

/// The `[test] (N symbols)` badge for a whole-test-file `File` row (a file
/// with no `FileReport` in `report.files` at all, see
/// `crate::tree::TreeNode::test_symbol_count`'s doc comment) — `N symbol`
/// (singular) when there is exactly one, matching `render.rs`'s own
/// singular/plural "Tests" section wording.
fn test_badge_span(symbol_count: usize) -> Span<'static> {
    let noun = if symbol_count == 1 {
        "symbol"
    } else {
        "symbols"
    };
    Span::styled(
        format!("[test] ({symbol_count} {noun})"),
        test_badge_style(),
    )
}

/// Style for the `[test] (N symbols)` whole-test-file badge: test code is
/// "can wait" gray everywhere (ADR 0099), the same as a `N tests` group.
///
/// `pub(crate)`: also reused by `crate::ui::overlay`'s Markers legend.
pub(crate) fn test_badge_style() -> Style {
    glyph::deferred()
}

/// A symbol row's one mark (ADR 0099 decision 1): what happened to the
/// symbol and how much it matters, in a single column.
///
/// - `●` red: signature changed and it has users ([`is_high_risk_symbol`])
///   — it may break them
/// - `○` yellow: signature changed, no users to speak of
/// - `+` green: added
/// - `-` red: removed (the same mark the diff uses for a removed line)
/// - blank: body-only or unclassified (the name dims instead)
///
/// `pub(crate)`: also reused by `crate::ui::overlay`'s Markers legend.
pub(crate) fn symbol_marker_span(symbol_ref: &SymbolRef, badges: &Badges) -> Span<'static> {
    if symbol_ref.removed {
        return Span::styled(glyph::REMOVED, glyph::danger());
    }
    match symbol_ref.classification {
        Some(Classification::Added) => Span::styled(glyph::ADDED, glyph::added()),
        Some(Classification::SignatureChanged) if is_high_risk_symbol(symbol_ref, badges) => {
            Span::styled(glyph::MAY_BREAK, risk_marker_style())
        }
        Some(Classification::SignatureChanged) => {
            Span::styled(glyph::SIGNATURE_CHANGED, glyph::contract())
        }
        Some(Classification::BodyOnly) | None => Span::raw(" "),
    }
}

/// The symbol name's own style: a removed symbol renders dimmed +
/// crossed-out (`Modifier::CROSSED_OUT`, widely supported by modern
/// terminals) to read as "gone" at a glance, distinct from the marker span
/// above which only flags *why*. A body-only (or unclassified) symbol —
/// [`symbol_marker_span`]'s blank marker — renders dimmed without the
/// strikethrough: its signature didn't change, so it carries less review
/// weight than an added/signature-changed/removed symbol, but it still
/// exists (unlike a removed one). A test symbol (only reachable inside a
/// mixed file's `TestGroup`, see `crate::tree::NodeKind::TestGroup`) is
/// dimmed the same way — group membership already conveys "this is test
/// code", so the name itself only needs to read as lower review priority,
/// not carry its own badge anymore.
///
/// `pub(crate)`: also reused by `crate::ui::overlay`'s Markers legend.
pub(crate) fn symbol_name_style(symbol_ref: &SymbolRef) -> Style {
    if symbol_ref.removed {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::CROSSED_OUT)
    } else if symbol_ref.is_test
        || matches!(
            symbol_ref.classification,
            Some(Classification::BodyOnly) | None
        )
    {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default()
    }
}

/// Whether a symbol may break its users: its signature changed and its
/// own fan-in (already test-referrer-excluded, see
/// `rinkaku_core::graph::compute_fan_ins`) clears the high-fan-in
/// threshold. A symbol's own badge only ever holds *its own* fan-in
/// (`crate::tree::symbol_badges`), so the comparison needs no aggregation.
///
/// ADR 0059's test-coverage signal deliberately does **not** escalate to
/// this mark: it fires on most changed symbols in practice, and the mark
/// only earns attention by staying rare (ADR 0043).
pub(crate) fn is_high_risk_symbol(symbol_ref: &SymbolRef, badges: &Badges) -> bool {
    symbol_ref.classification == Some(Classification::SignatureChanged)
        && badges.fan_in >= rinkaku_core::graph::HIGH_FAN_IN_THRESHOLD
}

/// Whether any symbol under `node` may break its users.
fn subtree_has_high_risk_symbol(node: &crate::tree::TreeNode) -> bool {
    match &node.kind {
        NodeKind::Symbol(symbol_ref) => is_high_risk_symbol(symbol_ref, &node.badges),
        _ => node.children.iter().any(subtree_has_high_risk_symbol),
    }
}

/// Puts a `●` in front of a collapsed `Dir`/`File` row hiding a symbol
/// that may break its users. An expanded row gets none: the `●` is on the
/// symbol's own row, right below, and repeating it on every ancestor is
/// what made one problem look like four (ADR 0099 decision 1).
fn push_hidden_risk_span(spans: &mut Vec<Span<'static>>, row: &Row<'_>) {
    if !row.expanded && subtree_has_high_risk_symbol(row.node) {
        spans.push(Span::styled(glyph::MAY_BREAK, risk_marker_style()));
        spans.push(Span::raw(" "));
    }
}

/// The `●` mark's style (bold red), factored out so
/// `crate::ui::overlay`'s Markers legend can reuse it too.
pub(crate) fn risk_marker_style() -> Style {
    glyph::danger().add_modifier(Modifier::BOLD)
}

#[cfg(test)]
#[path = "row_view_tests/mod.rs"]
mod tests;

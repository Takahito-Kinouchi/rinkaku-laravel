//! Pure scroll/wrap helpers shared by the panes in this module — extracted so
//! `clamp_scroll`, `scroll_indicator`, `visible_index_window`,
//! `window_overflow_indicators`, `windowed_rows_with_indicators`,
//! `wrap_lines_with_origins`/`wrap_one_line`, and `truncate_to_width` stay
//! unit-testable without a live `ratatui::backend::TestBackend`, and so
//! [`render_scrollable_pane`] itself (the one non-pure helper here) has a
//! single home shared by every pane that scrolls.
//!
//! `App::right_pane_scroll` is a logical-line offset (an index into the
//! unwrapped content), the same unit `crate::diff_shape` computes symbol
//! section positions in — [`logical_line_to_display_row`]/
//! [`display_row_to_logical_line`] are the only place that unit is ever
//! converted to/from the wrapped display-row index `ratatui::widgets::Paragraph::scroll`
//! actually consumes, confining wrap-width knowledge to this module rather
//! than leaking it into `App`/`crate::diff_shape`.

use super::style::pane_border_style;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthChar;

/// Renders `lines` into a bordered pane titled `title`, scrolled to
/// `requested_scroll` *logical* lines down (an index into the unwrapped
/// `body`, the same unit `crate::diff_shape`'s own row walk and its
/// derivatives compute against) and clamped to what actually fits
/// (`clamp_scroll`'s own doc comment on why clamping only happens here,
/// not in `crate::app`). When the content overflows the pane's inner
/// height, the title grows a `(first-last/total)` suffix so the reviewer
/// knows more content exists below/above without needing to scroll blind
/// (this iteration's answer to "全部が見えているように見えて実は続きがある"
/// — the same concern `crate::source`'s highlighted-window view sidesteps
/// by auto-centering instead of paginating).
///
/// Both the Detail and Diff panes share this one function rather than each
/// duplicating the clamp-then-scroll-then-indicator sequence, since the two
/// panes' only difference is which `Vec<Line>` and title they pass in.
///
/// `lines` is wrapped (via [`wrap_lines_with_origins`]) to the pane's inner
/// width *before* handing it to `Paragraph`, and `Paragraph::wrap` is
/// deliberately not used here: that widget's own line-wrapping happens
/// after `Paragraph::scroll` has already consumed `scroll.y`, which would
/// force the scroll unit back onto rendered rows. Confining wrap knowledge
/// to this function (rather than the logical-line unit `App::right_pane_scroll`
/// and `crate::diff_shape` share) is this fix's whole point: converting
/// `requested_scroll` to its first display row via [`logical_line_to_display_row`]
/// happens *after* wrapping, `clamp_scroll`/`scroll_indicator`/
/// `Paragraph::scroll` all operate on the wrapped (display-row) unit as
/// before, and the clamped result is converted back to a logical line via
/// [`display_row_to_logical_line`] before being returned — so every
/// consumer outside this function only ever sees logical lines.
///
/// Returns the actually-applied (clamped) scroll offset, in the same
/// logical-line unit as `requested_scroll` — dogfooding finding:
/// `App::right_pane_scroll` is deliberately an *unclamped* "requested"
/// value (its own doc comment), so repeated `j` past the content's end
/// kept incrementing that request with no visible effect, and winding it
/// back down again took as many `k` presses as it took to overshoot — the
/// scrollbar-less pane gave no feedback that this had happened.
/// `crate::run_app` feeds this return value back into `App` via
/// `App::with_right_pane_scroll` after every draw, so the *next* `k` moves
/// the visible content immediately instead of first re-tracing the
/// overshoot.
///
/// `focused` selects the pane's border style via
/// [`pane_border_style`] — dogfooding finding: every bordered pane looked
/// identical regardless of which one currently received motion keys, so a
/// reviewer had no visual way to tell which pane `j`/`k` would move. The
/// Detail/Diff/Blast-radius panes pass `app.focus() == Focus::Right`; the
/// `?` help overlay and jump popup, which are modal and always the active
/// surface while shown, pass `true` unconditionally.
///
/// `header_lines` renders fixed above the scrollable body, inside the same
/// bordered `Block`, via its own `Layout::vertical` split of the block's
/// inner area — a separate `Paragraph` with no `.scroll(..)` of its own, so
/// it lives entirely outside the coordinate system `requested_scroll`/
/// `clamp_scroll`/`scroll_indicator` operate in (the same way the `Block`'s
/// border and title already sit outside that coordinate system). This
/// matters beyond layout: `crate::diff_shape`'s own row walk hand-mirrors
/// this function's line-counting to place both ADR 0027's forward
/// (selection → scroll target) and ADR 0030's reverse (scroll position →
/// selected symbol) sync, so a header that shifted the body's own scroll
/// coordinates would desync both — splicing the header into the scrolled
/// content itself was considered and rejected for exactly this reason. Pass
/// `&[]` for a pane with no pinned header (every caller except the Diff
/// pane's identification/stats header).
///
/// `body` selects single-column (`Body::Single`, every caller except the
/// Diff pane's split mode) or side-by-side (`Body::Split`, ADR 0044)
/// rendering of the scrollable content — see [`Body`]'s own doc comment.
pub(crate) fn render_scrollable_pane(
    frame: &mut Frame,
    title: &str,
    header_lines: &[Line<'static>],
    body: Body<'_>,
    requested_scroll: usize,
    area: Rect,
    focused: bool,
) -> usize {
    render_marked_scrollable_pane(
        frame,
        title,
        header_lines,
        body,
        requested_scroll,
        area,
        focused,
        ReadThroughRows::Unmeasured,
    )
    .clamped_scroll
}

/// What [`render_marked_scrollable_pane`] reports back about the frame it
/// just drew: the clamped scroll offset every caller already folds back
/// into `App`, plus ADR 0088's two marks-derived values the Diff pane
/// needs — how much of the selected symbol is off-screen, and how far one
/// screen actually advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ScrollablePaneRender {
    /// The actually-applied (clamped) scroll offset, in `requested_scroll`'s
    /// own logical-line unit — see [`render_scrollable_pane`]'s doc comment
    /// on why every caller folds this back into `App`.
    pub(crate) clamped_scroll: usize,
    /// How many `marked_rows` fall above/below the viewport this frame
    /// ([`marked_rows_outside_viewport`]). All-zero for a caller that
    /// passes no marks.
    pub(crate) outside: MarkedRowsOutsideViewport,
    /// How many *logical* rows the viewport currently spans — the read-
    /// through step (ADR 0088) is derived from this rather than from
    /// `viewport_height`, since a wrapped body shows fewer logical rows
    /// than it has display rows and stepping by the display count would
    /// scroll clean past content that was never drawn.
    pub(crate) visible_logical_span: usize,
}

/// [`render_scrollable_pane`] plus ADR 0088's read-through accounting:
/// `rows` says what the pane offers to read through (see
/// [`ReadThroughRows`]), and the returned [`ScrollablePaneRender`] reports
/// how much of it the frame just drawn left off-screen.
///
/// Kept as the shared implementation of both entry points, rather than the
/// Diff pane growing its own copy of the wrap/clamp sequence: the counts
/// are only correct if they are measured against the very wrap origins and
/// clamped display row this same call handed to `Paragraph::scroll`, and
/// two copies of that sequence would be two chances for them to disagree.
///
/// The title carries the counts as a bold-yellow `▲N`/`▼N` suffix after
/// the existing `(first-last/total)` indicator — bold yellow because that
/// is the range bar's own color (`crate::ui::diff_pane::range_bar_span`),
/// which is what ties the numbers to the selected symbol rather than to
/// the file-scoped indicator sitting immediately to their left. Written
/// after that indicator so a title too narrow to hold both loses the
/// symbol-scoped half first — the file-scoped one is the half that is
/// meaningful for every pane.
// One parameter past clippy's threshold, and every one of them is an
// independent piece of already-computed content (see `crate::ui::draw`'s
// own identical allowance) — `marked_rows` in particular is computed by
// the caller for the range bar and passed through, not derived here.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_marked_scrollable_pane(
    frame: &mut Frame,
    title: &str,
    header_lines: &[Line<'static>],
    body: Body<'_>,
    requested_scroll: usize,
    area: Rect,
    focused: bool,
    rows: ReadThroughRows<'_>,
) -> ScrollablePaneRender {
    // `Block::inner` already folds in the border's own row/column, matching
    // `draw_source_screen`'s `saturating_sub(2)` convention for a bordered
    // pane's inner height without a manual subtraction here.
    let inner_area = Block::bordered().inner(area);
    let header_rows = (header_lines.len() as u16).min(inner_area.height);
    let [header_area, body_area] =
        Layout::vertical([Constraint::Length(header_rows), Constraint::Min(0)]).areas(inner_area);

    let viewport_height = body_area.height as usize;
    let (content_len, display_row, logical_scroll, origins) = match body {
        Body::Single(lines) => {
            let viewport_width = body_area.width as usize;
            let (wrapped, origins) = wrap_lines_with_origins(lines, viewport_width);
            let requested_display_row = logical_line_to_display_row(&origins, requested_scroll);
            let display_row = clamp_scroll(wrapped.len(), viewport_height, requested_display_row);
            let paragraph = Paragraph::new(wrapped.clone()).scroll((display_row as u16, 0));
            frame.render_widget(paragraph, body_area);
            let logical_scroll =
                resolve_folded_back_logical_line(&origins, display_row, requested_scroll);
            (wrapped.len(), display_row, logical_scroll, origins)
        }
        Body::Split(left, right) => {
            // A 1-column gutter between the two sides, mirroring a border's
            // own single-column width, so the two columns read as visually
            // distinct panes rather than running edge to edge.
            let [left_area, gutter_area, right_area] = Layout::horizontal([
                Constraint::Percentage(50),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .areas(body_area);
            let (left_rows, right_rows, origins) = pair_wrap_with_origins(
                left,
                right,
                left_area.width as usize,
                right_area.width as usize,
            );
            let requested_display_row = logical_line_to_display_row(&origins, requested_scroll);
            let display_row = clamp_scroll(left_rows.len(), viewport_height, requested_display_row);

            frame.render_widget(
                Paragraph::new(left_rows.clone()).scroll((display_row as u16, 0)),
                left_area,
            );
            frame.render_widget(
                Paragraph::new(vec![Line::raw("│"); left_rows.len()])
                    .scroll((display_row as u16, 0)),
                gutter_area,
            );
            frame.render_widget(
                Paragraph::new(right_rows.clone()).scroll((display_row as u16, 0)),
                right_area,
            );
            let logical_scroll =
                resolve_folded_back_logical_line(&origins, display_row, requested_scroll);
            (left_rows.len(), display_row, logical_scroll, origins)
        }
    };

    // Callers pass a title already padded with a leading/trailing space
    // (e.g. `" Detail "`, matching every other `Block` title in this
    // module) — trim the trailing one before appending the indicator so
    // the two don't produce a double space (`"Detail  (1-17/43)"`).
    // The indicator is measured in display rows (`content_len` is the
    // wrapped row count), not logical lines, so it uses `display_row`
    // rather than the logical-line `logical_scroll` this function returns.
    let title = match scroll_indicator(content_len, viewport_height, display_row) {
        Some(indicator) => format!("{}{indicator} ", title.trim_end()),
        None => title.to_string(),
    };
    let outside = marked_rows_outside_viewport(&origins, rows, display_row, viewport_height);
    let block = Block::bordered()
        .title(marked_title_line(
            title,
            symbol_scoped_counters(rows, outside),
        ))
        .border_style(pane_border_style(focused));

    frame.render_widget(block, area);
    if !header_lines.is_empty() {
        let header = Paragraph::new(header_lines[..header_rows as usize].to_vec());
        frame.render_widget(header, header_area);
    }
    ScrollablePaneRender {
        clamped_scroll: logical_scroll,
        outside,
        visible_logical_span: visible_logical_span(&origins, display_row, viewport_height),
    }
}

/// The counters the title should show: ADR 0088's amendment keeps them
/// scoped to a *symbol* selection. On a [`ReadThroughRows::WholeBody`] pane
/// the title's own `(first-last/total)` indicator already answers the same
/// question for the same content, so a second pair of numbers beside it
/// would say nothing new — and the counters' bold yellow is meaningful
/// precisely because it matches a range bar that a symbol-less selection
/// does not paint.
fn symbol_scoped_counters(
    rows: ReadThroughRows<'_>,
    outside: MarkedRowsOutsideViewport,
) -> MarkedRowsOutsideViewport {
    match rows {
        ReadThroughRows::Symbol(_) => outside,
        ReadThroughRows::WholeBody | ReadThroughRows::Unmeasured => {
            MarkedRowsOutsideViewport::default()
        }
    }
}

/// The pane title with ADR 0088's off-screen counters appended: `▲N` for
/// marked rows above the viewport, `▼N` for those below, each omitted when
/// its count is zero (so a pane with no marks, or a symbol that fits on
/// screen, renders exactly the title it did before this feature existed).
///
/// Bold yellow, matching `crate::ui::diff_pane::range_bar_span` — see
/// [`render_marked_scrollable_pane`]'s doc comment on why the color is
/// load-bearing here rather than decorative.
fn marked_title_line(title: String, outside: MarkedRowsOutsideViewport) -> Line<'static> {
    let mut spans = vec![Span::raw(title)];
    let counter_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    if outside.above > 0 {
        spans.push(Span::styled(format!("▲{} ", outside.above), counter_style));
    }
    if outside.below > 0 {
        spans.push(Span::styled(format!("▼{} ", outside.below), counter_style));
    }
    Line::from(spans)
}

/// How many distinct logical rows the viewport currently shows —
/// [`ScrollablePaneRender::visible_logical_span`]'s value. `0` when there is
/// nothing to show at all; otherwise at least 1, since a visible display row
/// was wrapped from exactly one logical row.
fn visible_logical_span(origins: &[usize], display_row: usize, viewport_height: usize) -> usize {
    match visible_logical_range(origins, display_row, viewport_height) {
        Some((first, last)) => last.saturating_sub(first) + 1,
        None => 0,
    }
}

/// The first and last *logical* rows any part of which is visible in the
/// display rows `[display_row, display_row + viewport_height)`, or `None`
/// when the pane shows nothing at all (empty content, or zero height).
/// The one place ADR 0088's two derived values agree on what "on screen"
/// means.
fn visible_logical_range(
    origins: &[usize],
    display_row: usize,
    viewport_height: usize,
) -> Option<(usize, usize)> {
    if origins.is_empty() || viewport_height == 0 {
        return None;
    }
    let last_display_row = display_row
        .saturating_add(viewport_height)
        .saturating_sub(1)
        .min(origins.len() - 1);
    Some((
        display_row_to_logical_line(origins, display_row),
        display_row_to_logical_line(origins, last_display_row),
    ))
}

/// How many *marked* logical rows sit outside the viewport (ADR 0088) —
/// the Diff pane's answer to "does the selected symbol's change continue
/// past this screen, and by how much", counted in the same logical-line
/// unit `App::right_pane_scroll` and `crate::diff_shape::marked_body_rows`
/// already share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MarkedRowsOutsideViewport {
    pub(crate) above: usize,
    pub(crate) below: usize,
}

/// What a pane offers to read through (ADR 0088, scope widened by its own
/// amendment) — the input [`marked_rows_outside_viewport`] counts against.
///
/// The two non-empty variants exist because a reviewer's reading unit is
/// not always a symbol. A file row (and any changed file rinkaku extracts
/// no symbols from at all — a Blade template, a config file, a migration)
/// carries no [`crate::app::DiffFocus`], so scoping the measurement to the
/// selected symbol left those rows with nothing to read through and
/// degraded `ctrl-f` to a plain cursor move over exactly the diffs that
/// most need paging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadThroughRows<'a> {
    /// The selected symbol's own rows — the same slice the Diff pane's
    /// range bar paints.
    Symbol(&'a [usize]),
    /// No symbol is selected, so the whole pane body is the thing to read.
    WholeBody,
    /// This pane does not participate in read-through at all (every pane
    /// except the Diff pane).
    Unmeasured,
}

/// Counts the rows of `rows` that fall outside the display rows
/// `[display_row, display_row + viewport_height)`, given `origins`
/// (either wrap helper's per-display-row logical-line index).
///
/// Defined against the *visible logical range* — the logical rows the
/// first and last visible display rows were wrapped from — rather than
/// against each marked row's own display position, so one pass over
/// `marked_rows` suffices no matter how large the wrapped body is
/// (`logical_line_to_display_row` is itself a scan, and calling it per
/// marked row would make this O(marked × display rows) on every frame).
///
/// A logical row that is only *partly* visible (a wrapped row whose later
/// fragments fall past the bottom edge) counts as visible, not as below:
/// it is on screen, and its own continuation is visible as a wrap. The
/// counts exist to answer "is there something here I have not seen at
/// all", and a row the reviewer is already looking at is not that.
pub(crate) fn marked_rows_outside_viewport(
    origins: &[usize],
    rows: ReadThroughRows<'_>,
    display_row: usize,
    viewport_height: usize,
) -> MarkedRowsOutsideViewport {
    let Some((first_visible, last_visible)) =
        visible_logical_range(origins, display_row, viewport_height)
    else {
        return MarkedRowsOutsideViewport::default();
    };

    match rows {
        ReadThroughRows::Unmeasured => MarkedRowsOutsideViewport::default(),
        ReadThroughRows::Symbol(marked_rows) => MarkedRowsOutsideViewport {
            above: marked_rows
                .iter()
                .filter(|&&row| row < first_visible)
                .count(),
            below: marked_rows
                .iter()
                .filter(|&&row| row > last_visible)
                .count(),
        },
        // Every logical row is in play, so the counts are the distances to
        // the body's own two ends rather than a scan: rows `0..first_visible`
        // above, and `last_visible+1..=last_logical` below.
        ReadThroughRows::WholeBody => {
            let last_logical = origins.last().copied().unwrap_or(0);
            MarkedRowsOutsideViewport {
                above: first_visible,
                below: last_logical.saturating_sub(last_visible),
            }
        }
    }
}

/// [`render_scrollable_pane`]'s scrollable content, either a single column
/// (every pane except the Diff pane's split mode) or two side-by-side
/// columns (ADR 0044) sharing one scroll offset.
pub(crate) enum Body<'a> {
    Single(&'a [Line<'static>]),
    Split(&'a [Line<'static>], &'a [Line<'static>]),
}

/// Wraps `left`/`right` independently to their own column widths, then pads
/// the shorter wrapped side back up to the longer side's row count with
/// blank filler lines — so both columns scroll in lockstep off one shared
/// offset (ADR 0044 decision 6). `left`/`right` already agree on *logical*
/// row count by construction ([`crate::diff_shape::SplitRow`]'s own
/// invariant, `diff_pane_split_rows`'s doc comment), but a long single
/// logical line can still wrap to a different number of *visual* rows than
/// its counterpart on the other side at a narrow column width — this
/// function pads per logical row so the two columns never drift out of
/// alignment after wrapping.
///
/// Also returns, for each output display row, the logical row index (the
/// shared `left`/`right` index) it was wrapped from — the split-view input
/// to [`logical_line_to_display_row`]/[`display_row_to_logical_line`], so
/// [`render_scrollable_pane`] can convert between `App::right_pane_scroll`'s
/// logical-line unit and the wrapped rows `Paragraph::scroll` actually
/// consumes.
pub(crate) fn pair_wrap_with_origins(
    left: &[Line<'static>],
    right: &[Line<'static>],
    left_width: usize,
    right_width: usize,
) -> (Vec<Line<'static>>, Vec<Line<'static>>, Vec<usize>) {
    let mut left_out = Vec::new();
    let mut right_out = Vec::new();
    let mut origins = Vec::new();
    let row_count = left.len().max(right.len());
    for index in 0..row_count {
        let left_wrapped = left
            .get(index)
            .map(|line| wrap_one_line(line, left_width))
            .unwrap_or_else(|| vec![Line::raw("")]);
        let right_wrapped = right
            .get(index)
            .map(|line| wrap_one_line(line, right_width))
            .unwrap_or_else(|| vec![Line::raw("")]);
        let rows = left_wrapped.len().max(right_wrapped.len());
        for row in 0..rows {
            left_out.push(left_wrapped.get(row).cloned().unwrap_or(Line::raw("")));
            right_out.push(right_wrapped.get(row).cloned().unwrap_or(Line::raw("")));
            origins.push(index);
        }
    }
    (left_out, right_out, origins)
}

/// Wraps each of `lines` to `width` display columns, splitting a logical
/// [`Line`] into as many output lines as needed while keeping each [`Span`]'s
/// style attached to the fragment it contributed (a wrap point can fall
/// mid-span, in which case the span itself is split and both fragments keep
/// the original span's style). Width is measured with
/// [`UnicodeWidthChar::width`] (falling back to 1 column for the zero-width/
/// control-character case `width()` returns `None` for) rather than byte or
/// `char` count, so a wide (e.g. full-width CJK) character that would
/// overflow `width` on its own wraps onto the next line instead of being
/// sliced in half.
///
/// A pure, unit-testable stand-in for `ratatui::widgets::Wrap`'s own
/// char-wrapping (`trim: false` mode) — needed because this crate must know
/// the *wrapped* line count up front, before `Paragraph::scroll` ever runs
/// (see `render_scrollable_pane`'s doc comment on why `Paragraph::wrap`
/// itself cannot be used for a scrollable pane). Deliberately does not
/// attempt `ratatui::widgets::Wrap`'s word-boundary trimming behavior —
/// content here is source/diff text, not prose, so a plain character wrap
/// (breaking wherever the width limit is hit, mid-word if needed) is the
/// right fidelity, not an approximation to chase.
///
/// `width == 0` returns `lines` unchanged (nothing meaningful to wrap
/// into — an actual zero-width pane cannot render any column anyway, and
/// looping without ever advancing would otherwise be a defensive infinite-
/// loop risk).
///
/// Also returns, for each output display row, the index into `lines` (the
/// logical line, `App::right_pane_scroll`'s unit) it was wrapped from — the
/// single-column input to [`logical_line_to_display_row`]/
/// [`display_row_to_logical_line`], mirroring [`pair_wrap_with_origins`] for
/// the split-view body.
pub(crate) fn wrap_lines_with_origins(
    lines: &[Line<'static>],
    width: usize,
) -> (Vec<Line<'static>>, Vec<usize>) {
    if width == 0 {
        return (lines.to_vec(), (0..lines.len()).collect());
    }

    let mut output = Vec::new();
    let mut origins = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let wrapped = wrap_one_line(line, width);
        origins.extend(std::iter::repeat_n(index, wrapped.len()));
        output.extend(wrapped);
    }
    (output, origins)
}

/// Converts a logical-line offset (`App::right_pane_scroll`'s unit, and
/// `crate::diff_shape`'s own row walk) to the display-row index of that
/// logical line's *first* wrapped row, given `origins` (either
/// [`wrap_lines_with_origins`]'s or [`pair_wrap_with_origins`]'s per-row
/// logical-line index). A `logical_line` past every origin (an overscroll
/// about to be clamped by [`clamp_scroll`] anyway, or empty content) maps to
/// `origins.len()` — one past the last display row, which `clamp_scroll`
/// then pulls back into range exactly like any other overscrolled request.
pub(crate) fn logical_line_to_display_row(origins: &[usize], logical_line: usize) -> usize {
    origins
        .iter()
        .position(|&origin| origin >= logical_line)
        .unwrap_or(origins.len())
}

/// The inverse of [`logical_line_to_display_row`]: the logical-line index
/// that display row `display_row` was wrapped from, given the same
/// `origins`. `display_row` past the end of `origins` (defensively — a
/// caller passes in a value [`clamp_scroll`] already bounded to
/// `origins.len() - 1` in practice) falls back to the last origin, or `0`
/// when `origins` itself is empty (nothing to scroll to).
pub(crate) fn display_row_to_logical_line(origins: &[usize], display_row: usize) -> usize {
    origins
        .get(display_row)
        .or_else(|| origins.last())
        .copied()
        .unwrap_or(0)
}

/// The logical-line value [`render_scrollable_pane`] should write back to
/// its caller, given `origins`, the just-clamped `display_row`, and the
/// `requested_logical_line` that produced it before clamping.
///
/// Plain [`display_row_to_logical_line`] is not enough here: when
/// `display_row` lands *inside* a preceding logical line's wrapped span
/// (rather than exactly at its first row) — which happens whenever
/// `clamp_scroll` pulls the display row back into a span that started
/// before the requested logical line — folding back reports that earlier
/// line's own index, silently undoing the request. The next `Down` then
/// asks for `requested_logical_line + 1` again, [`logical_line_to_display_row`]
/// resolves it to the same clamped `display_row`, and the value never
/// advances. Flooring the fold-back at `requested_logical_line` (itself
/// capped to the last available logical line, so an overscrolled request
/// cannot be reported as unclamped) keeps every request's floor intact
/// without touching the display-row clamp `Paragraph::scroll` actually
/// consumes.
pub(crate) fn resolve_folded_back_logical_line(
    origins: &[usize],
    display_row: usize,
    requested_logical_line: usize,
) -> usize {
    let Some(&last_logical_line) = origins.last() else {
        return 0;
    };
    let capped_request = requested_logical_line.min(last_logical_line);
    display_row_to_logical_line(origins, display_row).max(capped_request)
}

/// Wraps a single logical [`Line`] into one or more output lines, per
/// [`wrap_lines_with_origins`]'s doc comment. A line with no spans at all (a blank line)
/// produces exactly one empty output line, matching `ratatui::widgets::Wrap`
/// rendering a blank logical line as one blank row rather than zero rows.
pub(crate) fn wrap_one_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    let mut result_lines = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for span in &line.spans {
        let style = span.style;
        let mut fragment = String::new();
        let mut fragment_width = 0usize;

        for ch in span.content.chars() {
            let char_width = ch.width().unwrap_or(1);

            if current_width + fragment_width + char_width > width {
                // Flush the fragment accumulated so far (if any) onto the
                // current output line, then start a brand-new output line —
                // the char that overflowed becomes the first char of the
                // next fragment.
                if !fragment.is_empty() {
                    current_spans.push(Span::styled(fragment.clone(), style));
                    fragment.clear();
                    fragment_width = 0;
                }
                result_lines.push(Line::from(std::mem::take(&mut current_spans)).style(line.style));
                current_width = 0;
            }

            fragment.push(ch);
            fragment_width += char_width;
        }

        if !fragment.is_empty() {
            current_spans.push(Span::styled(fragment, style));
            current_width += fragment_width;
        }
    }

    result_lines.push(Line::from(current_spans).style(line.style));
    result_lines
}

/// Truncates `text` to fit within `width` display columns, replacing the
/// tail with a single `…` (1 column) when it does not fit — unlike
/// [`wrap_lines_with_origins`], which turns overflow into *more rows*, this turns
/// overflow into a shorter *single* row, for callers whose windowing math
/// (e.g. [`windowed_rows_with_indicators`]) has already committed to
/// "one logical item = one rendered row" and would desync if a row were
/// allowed to wrap (`draw_jump_popup`'s own fix for exactly that: a
/// `Paragraph::wrap`-ed candidate label taller than one row pushed later
/// candidates, including the cursor row, out of the popup's viewport with
/// no visual feedback, defeating `windowed_rows_with_indicators`'s own
/// "cursor always inside the window" contract).
///
/// Width is measured with [`UnicodeWidthChar::width`] (same fallback-to-1
/// convention as [`wrap_one_line`]) so a wide (e.g. CJK) character is never
/// sliced in half — if the last character that would fit is wide and only
/// one column of room remains, it is dropped along with the rest of the
/// tail rather than emitted half-width.
///
/// `text` already fitting within `width` (including exactly, or when
/// `width == 0`) is returned unchanged/empty respectively without adding
/// the `…` marker — nothing was actually cut off.
pub(crate) fn truncate_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let text_width: usize = text.chars().map(|ch| ch.width().unwrap_or(1)).sum();
    if text_width <= width {
        return text.to_string();
    }

    // Reserve 1 column for the trailing `…` marker, then greedily take
    // characters until the next one would overflow the remaining budget.
    let budget = width - 1;
    let mut result = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let char_width = ch.width().unwrap_or(1);
        if used + char_width > budget {
            break;
        }
        result.push(ch);
        used += char_width;
    }
    result.push('…');
    result
}

/// [`Line`]/[`Span`] counterpart of [`truncate_to_width`] — kept separate
/// because a tree-pane row is built from several differently-styled spans,
/// so truncating flattened text would lose which style belonged to which
/// surviving character. Stops at the first overflow and appends one `…`
/// span rather than wrapping, preserving the "one logical row stays one
/// rendered row" invariant [`windowed_rows_with_indicators`] depends on.
pub(crate) fn truncate_line_to_width(line: &Line<'static>, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::default().style(line.style);
    }
    let line_width: usize = line
        .spans
        .iter()
        .flat_map(|span| span.content.chars())
        .map(|ch| ch.width().unwrap_or(1))
        .sum();
    if line_width <= width {
        return line.clone();
    }

    let mut result_spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    let mut last_style = Style::default();
    // 1 column reserved for the trailing `…`.
    let budget = width.saturating_sub(1);

    'spans: for span in &line.spans {
        let style = span.style;
        let mut fragment = String::new();
        let mut fragment_width = 0usize;

        for ch in span.content.chars() {
            let char_width = ch.width().unwrap_or(1);
            if used + fragment_width + char_width > budget {
                if !fragment.is_empty() {
                    result_spans.push(Span::styled(fragment, style));
                }
                last_style = style;
                break 'spans;
            }
            fragment.push(ch);
            fragment_width += char_width;
        }

        if !fragment.is_empty() {
            result_spans.push(Span::styled(fragment, style));
            used += fragment_width;
        }
        last_style = style;
    }

    // Merge into the last span when styles match, else a single-style
    // line would fail `PartialEq` against an equivalent single-span value.
    match result_spans.last_mut() {
        Some(last) if last.style == last_style => {
            let mut content = last.content.to_string();
            content.push('…');
            last.content = content.into();
        }
        _ => result_spans.push(Span::styled("…", last_style)),
    }
    Line::from(result_spans).style(line.style)
}

/// Truncates `text` to fit within `width` display columns from the *tail*,
/// replacing the head with a leading `…` when it does not fit — the mirror
/// image of [`truncate_to_width`], for callers whose most informative part
/// is at the end of the string rather than the start (the Diff pane's
/// header line: a long path's basename/symbol name is the part worth
/// keeping visible, not its leading directories).
///
/// Same width measurement and edge cases as [`truncate_to_width`]
/// (`width == 0` returns empty, text already fitting is returned
/// unchanged), mirrored from the tail instead of the head.
pub(crate) fn truncate_to_width_keeping_tail(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let text_width: usize = text.chars().map(|ch| ch.width().unwrap_or(1)).sum();
    if text_width <= width {
        return text.to_string();
    }

    // Reserve 1 column for the leading `…` marker, then greedily take
    // characters from the end until the next one (going backwards) would
    // overflow the remaining budget.
    let budget = width - 1;
    let mut kept: Vec<char> = Vec::new();
    let mut used = 0usize;
    for ch in text.chars().rev() {
        let char_width = ch.width().unwrap_or(1);
        if used + char_width > budget {
            break;
        }
        kept.push(ch);
        used += char_width;
    }
    kept.reverse();
    let mut result = String::from('…');
    result.extend(kept);
    result
}

/// Clamps a requested scroll offset (lines) to `[0, content_len -
/// viewport_height]` — the largest offset that still leaves the viewport
/// full of content rather than trailing off into blank space below the
/// last line. Returns 0 whenever the content already fits entirely
/// (`content_len <= viewport_height`, including `viewport_height == 0`
/// defensively), so a pane that has nothing to scroll can never report a
/// nonzero offset.
///
/// Deliberately pure and free of any `ratatui`/ `Rect` type (just the three
/// `usize`s a caller already has in hand) so it is unit-testable without a
/// `TestBackend` — `crate::app::App` intentionally does not do this
/// clamping itself (see `right_pane_scroll`'s own doc comment): only
/// `crate::ui`, at draw time, knows the pane's actual rendered height.
pub(crate) fn clamp_scroll(
    content_len: usize,
    viewport_height: usize,
    requested_scroll: usize,
) -> usize {
    let max_scroll = content_len.saturating_sub(viewport_height);
    requested_scroll.min(max_scroll)
}

/// Computes the `[start, end)` window of `total_items` rows to display in a
/// viewport `viewport_height` rows tall so that `cursor_index` is always
/// inside `[start, end)` — the tree pane's own cursor-follow scroll
/// (post-#61 review finding: `draw_tree_pane` used to hand `Nav::rows`'
/// *entire* row list to `Paragraph` unscrolled, so jumping the cursor to a
/// row outside the initial viewport — via `j`/`k` repeated past the bottom,
/// or a `gd`/`gr` jump — left the screen showing exactly the same rows as
/// before, looking like the keypress had no effect) and the jump-target
/// popup's own candidate-list scroll (same underlying gap: `draw_jump_popup`
/// used to hand every candidate to `Paragraph` unscrolled).
///
/// Mirrors `crate::source::visible_window`'s centering approach (keep the
/// point of interest mid-viewport rather than pinned to an edge, so a few
/// rows of context are visible on both sides) but for a single index rather
/// than a highlighted range, and 0-based indices/half-open `[start, end)`
/// throughout — matching this module's own row-index convention
/// (`draw_tree_pane`'s `index == cursor` check) rather than `visible_window`'s
/// 1-based line-number convention, so callers here never need to convert.
///
/// `total_items == 0` or `viewport_height == 0` returns `(0, 0)` — an empty
/// window, nothing to show either way.
pub(crate) fn visible_index_window(
    total_items: usize,
    cursor_index: usize,
    viewport_height: usize,
) -> (usize, usize) {
    if total_items == 0 || viewport_height == 0 {
        return (0, 0);
    }

    let half = viewport_height / 2;
    let ideal_start = cursor_index.saturating_sub(half);

    // Clamp so the window never runs past the end of the list, then clamp
    // again at zero so a short list (fewer items than `viewport_height`)
    // still yields a valid, in-bounds window rather than a negative start —
    // same two-step clamp `visible_window` itself uses.
    let max_start = total_items.saturating_sub(viewport_height);
    let start = ideal_start.min(max_start);
    let end = (start + viewport_height).min(total_items);

    (start, end)
}

/// Builds a `"…N more above"`/`"…N more below"` pair of indicator lines for
/// content windowed by [`visible_index_window`] — `above`/`below` are the
/// counts of items hidden on each side (`start`/`total_items - end`
/// respectively), formatted only when nonzero so a window that already
/// shows everything (or sits at an edge) does not grow a spurious "…0 more"
/// line. Returned as `(above, below)`, each `Option<String>`, for the caller
/// to place immediately before/after the windowed content — kept as plain
/// `String`s rather than `ratatui::text::Line` so this stays unit-testable
/// without a `ratatui` type, matching [`scroll_indicator`]'s own precedent.
pub(crate) fn window_overflow_indicators(
    total_items: usize,
    window_start: usize,
    window_end: usize,
) -> (Option<String>, Option<String>) {
    let above = window_start;
    let below = total_items.saturating_sub(window_end);
    (
        (above > 0).then(|| format!("… {above} more above")),
        (below > 0).then(|| format!("… {below} more below")),
    )
}

/// Ties [`visible_index_window`] and [`window_overflow_indicators`]
/// together correctly for a caller that renders the indicator lines
/// *inside* the same fixed-height viewport as the windowed content itself
/// (`draw_tree_pane`/`draw_jump_popup`'s own layout): naively computing the
/// content window against the *full* `viewport_height` and then
/// unconditionally prepending/appending indicator lines on top overflows
/// the viewport by up to 2 rows, silently clipping the last row or two of
/// real content off the bottom of the pane (including, in the worst case,
/// the cursor row itself — the exact bug this windowing feature exists to
/// fix, reintroduced one layer up). This function reserves a row for each
/// indicator *before* sizing the content window, so the total row count
/// (indicators + content) never exceeds `viewport_height`.
///
/// Reserving is a small fixed-point search rather than a single
/// calculation: whether the "above"/"below" indicator is needed at all
/// depends on where the content window ends up, which itself depends on
/// how many rows are reserved for indicators — so this recomputes the
/// window with 0, then up to 2, reserved rows until the reservation and the
/// window it produces agree. This always converges in at most 3 iterations
/// (each iteration can only add a reservation, never remove one, and there
/// are only two indicators to add), so a small bounded loop is used rather
/// than proving a closed-form formula.
///
/// Returns `(content_start, content_end, above_indicator, below_indicator)`.
pub(crate) fn windowed_rows_with_indicators(
    total_items: usize,
    cursor_index: usize,
    viewport_height: usize,
) -> (usize, usize, Option<String>, Option<String>) {
    let mut reserved = 0;
    loop {
        let content_height = viewport_height.saturating_sub(reserved);
        let (start, end) = visible_index_window(total_items, cursor_index, content_height);
        let (above, below) = window_overflow_indicators(total_items, start, end);
        let needed = above.is_some() as usize + below.is_some() as usize;
        if needed <= reserved {
            return (start, end, above, below);
        }
        reserved = needed;
    }
}

/// Builds the `(first-last/total)` title suffix for a pane whose content
/// overflows its viewport, or `None` when everything already fits (nothing
/// to indicate). `scroll` must already be clamped (`clamp_scroll`) — this
/// function does not re-clamp, it only formats.
pub(crate) fn scroll_indicator(
    content_len: usize,
    viewport_height: usize,
    scroll: usize,
) -> Option<String> {
    if content_len <= viewport_height {
        return None;
    }
    let first_visible = scroll + 1;
    let last_visible = (scroll + viewport_height).min(content_len);
    Some(format!(" ({first_visible}-{last_visible}/{content_len})"))
}

#[cfg(test)]
#[path = "scroll_tests/mod.rs"]
mod tests;

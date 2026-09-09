//! ADR 0088's off-screen counters: the `▼N`/`▲N` title suffix, and the
//! [`crate::app::ReadThrough`] measurement `crate::run_app` folds out of
//! [`crate::ui::DrawOutcome`] to drive `ctrl-f`/`ctrl-b`. Checked
//! end-to-end through `draw` rather than against
//! `marked_rows_outside_viewport` alone (which has its own unit tests in
//! `crate::ui::scroll_tests::marked_rows`): the whole point of the feature
//! is that the numbers describe *the frame the reviewer is looking at*, so
//! the wiring from the range bar's marked rows through the pane's actual
//! clamped scroll is the part worth pinning here.

use super::*;
use crate::app::{InputKey, ReadThrough};
use pretty_assertions::assert_eq;

/// A symbol spanning `lines` rows of a brand-new file, plus the matching
/// single-hunk diff — the shape ADR 0074's Context calls the common one
/// (a new file arrives as exactly one hunk) and the shape that overflows
/// the pane.
fn report_and_diff_for_a_tall_symbol(lines: usize) -> (Report, String) {
    let report = Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                range: LineRange {
                    start: 1,
                    end: lines,
                },
                ..symbol("lib.rs::foo", "foo")
            }],
        }],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![],
            edges: vec![],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };
    let body: String = (0..lines)
        .map(|index| format!("+    let x{index} = {index};\n"))
        .collect();
    let diff_text = format!(
        "\
diff --git a/lib.rs b/lib.rs
index e69de29..4b825dc 100644
--- a/lib.rs
+++ b/lib.rs
@@ -0,0 +1,{lines} @@
{body}"
    );
    (report, diff_text)
}

/// The same diff as [`report_and_diff_for_a_tall_symbol`] against a file
/// rinkaku extracted no symbols from at all (a Blade template, a config
/// file, a migration) — the selection ADR 0088's first amendment widened
/// the measurement for, and the one its second 2026-09-09 amendment hands
/// back to the Diff pane's own focus.
fn report_and_diff_for_a_symbol_less_file(lines: usize) -> (Report, String) {
    let (mut report, diff_text) = report_and_diff_for_a_tall_symbol(lines);
    report.files[0].symbols.clear();
    (report, diff_text)
}

/// [`draw_tall_symbol_frame`] with the cursor left on the *file* row (row
/// 0, `App::new`'s own default position) instead of stepping down onto the
/// symbol — a selection that carries no `DiffFocus`, which the tree walk
/// steps past rather than pages (ADR 0088's 2026-09-09 amendments).
fn draw_tall_file_row_frame(scroll: usize, lines: usize) -> (crate::ui::DrawOutcome, String) {
    let (report, diff_text) = report_and_diff_for_a_tall_symbol(lines);
    draw_frame(&report, &diff_text, scroll, Selection::File)
}

/// [`draw_tall_file_row_frame`]'s file row with its symbol row folded away
/// (`InputKey::Select` on the row itself) — still a file row under the
/// tree's keys, so still nothing to page.
fn draw_collapsed_file_row_frame(scroll: usize, lines: usize) -> (crate::ui::DrawOutcome, String) {
    let (report, diff_text) = report_and_diff_for_a_tall_symbol(lines);
    draw_frame(&report, &diff_text, scroll, Selection::CollapsedFile)
}

/// [`draw_tall_file_row_frame`]'s file row with the *right pane* focused
/// (`InputKey::Open`), where there is no tree walk to delegate the reading
/// to.
fn draw_right_focused_file_row_frame(
    scroll: usize,
    lines: usize,
) -> (crate::ui::DrawOutcome, String) {
    let (report, diff_text) = report_and_diff_for_a_tall_symbol(lines);
    draw_frame(&report, &diff_text, scroll, Selection::RightFocusedFile)
}

/// A file row on a file with no symbol rows to walk on to at all.
fn draw_symbol_less_file_frame(scroll: usize, lines: usize) -> (crate::ui::DrawOutcome, String) {
    let (report, diff_text) = report_and_diff_for_a_symbol_less_file(lines);
    draw_frame(&report, &diff_text, scroll, Selection::File)
}

/// The same symbol-less file with the *right pane* focused — the one focus
/// a whole file diff is still read through in.
fn draw_right_focused_symbol_less_file_frame(
    scroll: usize,
    lines: usize,
) -> (crate::ui::DrawOutcome, String) {
    let (report, diff_text) = report_and_diff_for_a_symbol_less_file(lines);
    draw_frame(&report, &diff_text, scroll, Selection::RightFocusedFile)
}

/// Draws one frame of the entry screen at 80x20 with the cursor on the
/// symbol row and unified view forced (split is the default per ADR 0044's
/// amendment; unified keeps this module's row arithmetic to one column),
/// returning the frame's [`crate::ui::DrawOutcome`] and its rendered text.
fn draw_tall_symbol_frame(scroll: usize, lines: usize) -> (crate::ui::DrawOutcome, String) {
    let (report, diff_text) = report_and_diff_for_a_tall_symbol(lines);
    draw_frame(&report, &diff_text, scroll, Selection::Symbol)
}

/// Which row [`draw_frame`] leaves the cursor on, and in which focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    File,
    CollapsedFile,
    RightFocusedFile,
    Symbol,
}

fn draw_frame(
    report: &Report,
    diff_text: &str,
    scroll: usize,
    selection: Selection,
) -> (crate::ui::DrawOutcome, String) {
    let app = App::new(report);
    let app = match selection {
        Selection::File => app,
        Selection::CollapsedFile => app.handle_key(InputKey::Select),
        Selection::RightFocusedFile => app.handle_key(InputKey::Open),
        Selection::Symbol => app.handle_key(InputKey::Down),
    };
    let app = app
        .handle_key(InputKey::ToggleSplitView)
        .with_right_pane_scroll(scroll);
    let diff_files = crate::diff_view::parse_diff_hunks(diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(report, &diff_files, &app);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    let mut outcome = crate::ui::DrawOutcome::default();
    terminal
        .draw(|frame| {
            outcome = draw(
                frame,
                &app,
                report,
                &diff_content,
                &diff_highlights,
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");

    (outcome, buffer_text(&terminal))
}

#[test]
fn should_report_the_rows_left_below_when_a_symbol_outgrows_the_pane() {
    // At 80x20 the pane's body holds 14 rows (20 rows, less the status
    // line, the two borders and the pane's own 2-line pinned header). The
    // first of those is the `@@` hunk header, which carries no line of its
    // own and is never marked, so 13 of the symbol's 40 changed rows are
    // on screen and 27 are still below. One screen advances by 13 — the
    // 14 visible logical rows less a row of overlap.
    let (outcome, _) = draw_tall_symbol_frame(0, 40);

    assert_eq!(
        Some(ReadThrough {
            rows_above: 0,
            rows_below: 27,
            step: 13,
        }),
        outcome.diff_read_through
    );
}

#[test]
fn should_render_the_below_counter_in_the_pane_title() {
    let (_, text) = draw_tall_symbol_frame(0, 40);

    let title_row = text
        .lines()
        .find(|line| line.contains("Diff"))
        .unwrap_or_else(|| panic!("expected a Diff pane title, got:\n{text}"));
    assert!(title_row.contains("▼27"), "title row was: {title_row}");
    assert!(!title_row.contains('▲'), "title row was: {title_row}");
}

#[test]
fn should_report_both_counters_once_the_pane_is_scrolled_into_the_middle_of_the_symbol() {
    let (outcome, text) = draw_tall_symbol_frame(20, 40);

    assert_eq!(
        Some(ReadThrough {
            rows_above: 19,
            rows_below: 7,
            step: 13,
        }),
        outcome.diff_read_through
    );
    let title_row = text
        .lines()
        .find(|line| line.contains("Diff"))
        .unwrap_or_else(|| panic!("expected a Diff pane title, got:\n{text}"));
    assert!(title_row.contains("▲19"), "title row was: {title_row}");
    assert!(title_row.contains("▼7"), "title row was: {title_row}");
}

#[test]
fn should_report_no_counters_and_show_no_marker_when_the_whole_symbol_fits() {
    let (outcome, text) = draw_tall_symbol_frame(0, 3);

    assert_eq!(
        Some(ReadThrough {
            rows_above: 0,
            rows_below: 0,
            step: 3,
        }),
        outcome.diff_read_through
    );
    assert!(!text.contains('▼'), "rendered frame was:\n{text}");
    assert!(!text.contains('▲'), "rendered frame was:\n{text}");
}

#[test]
fn should_offer_nothing_to_read_through_on_a_file_row_with_symbol_rows_below_it() {
    // ADR 0088's 2026-09-09 amendments: the body is 41 rows (a `@@` header
    // plus 40 changed lines), and the reviewer is one `↓` away from reading
    // it symbol by symbol — paging the whole file here would show it all
    // twice.
    let (outcome, _) = draw_tall_file_row_frame(0, 40);

    assert_eq!(
        Some(ReadThrough {
            rows_above: 0,
            rows_below: 0,
            step: 13,
        }),
        outcome.diff_read_through
    );
}

#[test]
fn should_offer_nothing_to_read_through_on_a_file_row_with_its_symbol_rows_collapsed() {
    let (outcome, _) = draw_collapsed_file_row_frame(0, 40);

    assert_eq!(
        Some(ReadThrough {
            rows_above: 0,
            rows_below: 0,
            step: 13,
        }),
        outcome.diff_read_through
    );
}

#[test]
fn should_offer_nothing_to_read_through_on_a_file_row_without_any_symbol_rows() {
    // The second 2026-09-09 amendment: read-through from the tree is a
    // symbol-row motion, so even the file rinkaku extracted no symbols from
    // pages nothing here — `↓` walks on, and the pane's own `ctrl-f`
    // (below) stays the way to read this diff.
    let (outcome, _) = draw_symbol_less_file_frame(0, 40);

    assert_eq!(
        Some(ReadThrough {
            rows_above: 0,
            rows_below: 0,
            step: 14,
        }),
        outcome.diff_read_through
    );
}

#[test]
fn should_offer_the_whole_file_on_a_file_row_while_the_right_pane_is_focused() {
    let (outcome, _) = draw_right_focused_file_row_frame(0, 40);

    assert_eq!(
        Some(ReadThrough {
            rows_above: 0,
            rows_below: 27,
            step: 13,
        }),
        outcome.diff_read_through
    );
}

#[test]
fn should_offer_the_whole_symbol_less_file_while_the_right_pane_is_focused() {
    // A symbol-less file's pinned header is one line shorter (no per-symbol
    // stats line), so 15 of the 41 rows fit and 26 are still below.
    let (outcome, _) = draw_right_focused_symbol_less_file_frame(0, 40);

    assert_eq!(
        Some(ReadThrough {
            rows_above: 0,
            rows_below: 26,
            step: 14,
        }),
        outcome.diff_read_through
    );
}

#[test]
fn should_leave_the_title_counters_to_symbol_selections() {
    // The file-scoped `(1-15/41)` indicator already answers "is there more"
    // for this same content, and the counters' bold yellow means "this is
    // the range bar's symbol" — a file row paints no bar.
    let (_, text) = draw_symbol_less_file_frame(0, 40);

    let title_row = text
        .lines()
        .find(|line| line.contains("Diff"))
        .unwrap_or_else(|| panic!("expected a Diff pane title, got:\n{text}"));
    assert!(
        title_row.contains("(1-15/41)"),
        "title row was: {title_row}"
    );
    assert!(!title_row.contains('▼'), "title row was: {title_row}");
    assert!(!title_row.contains('▲'), "title row was: {title_row}");
}

#[test]
fn should_report_the_rows_left_above_a_scrolled_right_focused_file_selection() {
    let (outcome, _) = draw_right_focused_symbol_less_file_frame(20, 40);

    assert_eq!(
        Some(ReadThrough {
            rows_above: 20,
            rows_below: 6,
            step: 14,
        }),
        outcome.diff_read_through
    );
}

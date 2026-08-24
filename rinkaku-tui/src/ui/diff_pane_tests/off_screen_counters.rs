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

/// Draws one frame of the entry screen at 80x20 with the cursor on the
/// symbol row and unified view forced (split is the default per ADR 0044's
/// amendment; unified keeps this module's row arithmetic to one column),
/// returning the frame's [`crate::ui::DrawOutcome`] and its rendered text.
fn draw_tall_symbol_frame(scroll: usize, lines: usize) -> (crate::ui::DrawOutcome, String) {
    let (report, diff_text) = report_and_diff_for_a_tall_symbol(lines);
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::ToggleSplitView)
        .with_right_pane_scroll(scroll);
    let diff_files = crate::diff_view::parse_diff_hunks(&diff_text);
    let diff_highlights = crate::highlight::highlight_diff_files(&diff_files);
    let diff_content = diff_content_for(&report, &diff_files, &app);
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).expect("terminal");

    let mut outcome = crate::ui::DrawOutcome::default();
    terminal
        .draw(|frame| {
            outcome = draw(
                frame,
                &app,
                &report,
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

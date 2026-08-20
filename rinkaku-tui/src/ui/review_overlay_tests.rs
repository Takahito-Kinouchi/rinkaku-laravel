use crate::app::{App, BlastRadiusSelection};
use crate::locale::Locale;
use crate::review::{AnnotationTarget, ReviewState, SelectionSnapshot};
use crate::ui::draw;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Position, Rect};
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{FileReport, Report};

fn symbol(id: &str, name: &str) -> ExtractedSymbol {
    ExtractedSymbol {
        id: id.to_string(),
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature: format!("fn {name}()"),
        range: LineRange { start: 1, end: 1 },
        container: None,
        referenced_names: vec![],
        referenced_method_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }
}

fn report_with_one_symbol() -> Report {
    Report {
        origin: rinkaku_core::render::ReportOrigin::Diff,
        files: vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![symbol("lib.rs::foo", "foo")],
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
    }
}

fn snapshot() -> SelectionSnapshot {
    SelectionSnapshot {
        target: AnnotationTarget::Symbol,
        path: "lib.rs".to_string(),
        symbol_id: Some("lib.rs::foo".to_string()),
        symbol_name: Some("foo".to_string()),
        range: Some((1, 5)),
        anchor: Some((1, 5)),
        signature: Some("fn foo()".to_string()),
    }
}

fn dir_snapshot() -> SelectionSnapshot {
    SelectionSnapshot {
        target: AnnotationTarget::Dir,
        path: "src".to_string(),
        symbol_id: None,
        symbol_name: None,
        range: None,
        anchor: None,
        signature: None,
    }
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Draws `app`/`report` onto a fresh 100x30 [`TestBackend`] and returns the
/// [`Terminal`] itself (rather than just its rendered text, `draw_app`'s
/// own return value below) — the compose-cursor tests need the terminal to
/// query the backend's own cursor visibility/position after the draw,
/// which `buffer_text` alone cannot expose.
fn draw_terminal(app: &App, report: &Report) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");
    terminal
        .draw(|frame| {
            draw(
                frame,
                app,
                report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::English,
            );
        })
        .expect("draw");
    terminal
}

fn draw_app(app: &App, report: &Report) -> String {
    buffer_text(&draw_terminal(app, report))
}

#[test]
fn should_not_draw_review_overlay_when_review_is_idle() {
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let text = draw_app(&app, &report);

    assert!(!text.contains("New annotation"));
    assert!(!text.contains("Review annotations"));
}

#[test]
fn should_draw_compose_overlay_with_location_and_buffer_when_composing() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('h')
        .push_char('i');
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("New annotation"));
    assert!(text.contains("lib.rs:1-5 foo"));
    assert!(text.contains("hi"));
    assert!(text.contains("Enter: save"));
}

#[test]
fn should_draw_compose_overlay_with_trailing_slash_when_composing_over_a_dir_snapshot() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(dir_snapshot())
        .push_char('h')
        .push_char('i');
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("New annotation"));
    assert!(text.contains("src/"));
    assert!(text.contains("hi"));
}

#[test]
fn should_draw_annotations_list_overlay_with_annotation_summary() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('f')
        .push_char('i')
        .push_char('x')
        .confirm_compose()
        .open_list();
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("Review annotations"));
    assert!(text.contains("lib.rs:1-5 foo: fix"));
    assert!(text.contains("Enter: export"));
    assert!(text.contains("d: delete"));
}

#[test]
fn should_draw_annotations_list_overlay_with_trailing_slash_for_a_dir_annotation() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(dir_snapshot())
        .push_char('m')
        .push_char('e')
        .push_char('s')
        .push_char('s')
        .confirm_compose()
        .open_list();
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("Review annotations"));
    assert!(text.contains("src/: mess"));
}

#[test]
fn should_draw_empty_annotations_list_placeholder_when_there_are_no_annotations() {
    let report = report_with_one_symbol();
    let review = ReviewState::default().open_list();
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("no annotations yet"));
}

#[test]
fn should_draw_both_export_menu_entries_when_sink_a_is_available() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list()
        .open_export_menu();
    let app = App::new(&report)
        .with_review_sink_a_available(true)
        .with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("Export to"));
    assert!(text.contains("GitHub PR review"));
    assert!(text.contains("Clipboard"));
}

#[test]
fn should_draw_only_clipboard_entry_when_sink_a_is_unavailable() {
    // Regression test: the export menu's *rendering* must match
    // `ReviewState::confirm_export`'s own `sink_a_available`-gated entry
    // list (`export_menu_entries`) — drawing "GitHub PR review"
    // unconditionally, regardless of whether a `PrContext` was ever wired
    // up, misleads the reviewer into thinking cursor position 0 posts a
    // GitHub review when it actually confirms whatever
    // `export_menu_entries(false)` put there instead (`Clipboard`).
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list()
        .open_export_menu();
    let app = App::new(&report).with_review(review);
    assert!(!app.review_sink_a_available());

    let text = draw_app(&app, &report);

    assert!(text.contains("Export to"));
    assert!(!text.contains("GitHub PR review"));
    assert!(text.contains("Clipboard"));
}

#[test]
fn should_export_to_clipboard_when_confirming_cursor_zero_with_sink_a_unavailable() {
    // The other half of the regression above: confirming the menu's
    // cursor-0 entry while sink A is unavailable must produce the same
    // `ExportRequest` the rendered (sink-A-omitted) menu actually shows at
    // that position — `Clipboard`, not a silently-closed menu.
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list()
        .open_export_menu();
    let app = App::new(&report).with_review(review);

    let app = app.handle_key(crate::app::InputKey::PopupConfirm);

    let mut review = app.review().clone();
    assert_eq!(
        Some(crate::review::ExportRequest::Clipboard),
        review.take_pending_export()
    );
}

#[test]
fn should_draw_verdict_menu_overlay_entries() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list()
        .open_export_menu()
        .confirm_export(true);
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("Submit review as"));
    assert!(text.contains("Approve"));
    assert!(text.contains("Request changes"));
    assert!(text.contains("Comment"));
}

#[test]
fn should_show_last_status_message_in_annotations_list_overlay() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .set_status("posted 1 review comment(s) to PR #7")
        .open_list();
    let app = App::new(&report).with_review(review);

    let text = draw_app(&app, &report);

    assert!(text.contains("posted 1 review comment(s) to PR #7"));
}

// IME-anchoring cursor tests (ADR 0085): a terminal IME anchors/engages
// composition at the terminal's own hardware cursor, which ratatui
// otherwise leaves hidden — these pin that the compose overlay places it
// at the buffer's insertion point, and nowhere else.

/// The compose overlay's own `70% x 50%` box (`draw_compose_overlay`'s own
/// `centered_rect` call), computed independently of `compose_cursor_position`
/// itself so these tests exercise real layout math rather than assert
/// against a copy of the same private helper's own arithmetic.
fn compose_overlay_area() -> Rect {
    let full_area = Rect::new(0, 0, 100, 30);
    super::centered_rect(full_area, 70, 50)
}

#[test]
fn should_place_cursor_after_the_last_ascii_char_in_the_compose_buffer() {
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('h')
        .push_char('i');
    let app = App::new(&report).with_review(review);

    let terminal = draw_terminal(&app, &report);

    let overlay_area = compose_overlay_area();
    // "hi" is 2 ASCII characters, 2 display columns wide.
    let expected = Position::new(overlay_area.x + 1 + 2, overlay_area.y + 1);

    assert!(terminal.backend().cursor_visible());
    assert_eq!(expected, terminal.backend().cursor_position());
}

#[test]
fn should_advance_cursor_two_columns_per_fullwidth_char_in_the_compose_buffer() {
    // The bug this whole feature exists to fix: full-width (Japanese IME)
    // characters occupy two terminal columns each — using char *count*
    // instead of display *width* would anchor the cursor two columns short
    // after "あい" (a 4-column buffer, but only 2 chars).
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('あ')
        .push_char('い');
    let app = App::new(&report).with_review(review);

    let terminal = draw_terminal(&app, &report);

    let overlay_area = compose_overlay_area();
    let expected = Position::new(overlay_area.x + 1 + 4, overlay_area.y + 1);

    assert!(terminal.backend().cursor_visible());
    assert_eq!(expected, terminal.backend().cursor_position());
}

#[test]
fn should_not_show_a_visible_cursor_when_review_is_idle() {
    let report = report_with_one_symbol();
    let app = App::new(&report);

    let terminal = draw_terminal(&app, &report);

    assert!(!terminal.backend().cursor_visible());
}

#[test]
fn should_not_show_a_visible_cursor_in_the_annotations_list_overlay() {
    // The cursor must appear only while composing — every other review
    // overlay mode (list/export-menu/verdict-menu) keeps it hidden the
    // same way every non-review screen does.
    let report = report_with_one_symbol();
    let review = ReviewState::default()
        .begin_compose(snapshot())
        .push_char('x')
        .confirm_compose()
        .open_list();
    let app = App::new(&report).with_review(review);

    let terminal = draw_terminal(&app, &report);

    assert!(!terminal.backend().cursor_visible());
}

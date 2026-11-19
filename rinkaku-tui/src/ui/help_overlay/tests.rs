use super::{FileSizeBand, markers_legend_lines};
use crate::app::{App, BlastRadiusSelection};
use crate::locale::Locale;
use crate::ui::draw;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use rinkaku_core::diff::LineRange;
use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
use rinkaku_core::graph::SymbolGraph;
use rinkaku_core::render::{FileReport, Report};

/// The style of the swatch span reading `swatch` in the legend line that
/// carries it.
fn swatch_style(swatch: &str) -> Style {
    let lines = markers_legend_lines(Locale::English);
    let line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == swatch)
        })
        .unwrap_or_else(|| panic!("{swatch:?} line present"));
    line_span(line, swatch).style
}

#[test]
fn should_style_each_change_mark_swatch_like_the_tree_row_draws_it() {
    let actual = [
        swatch_style("+"),
        swatch_style("○"),
        swatch_style("●"),
        swatch_style("-"),
    ];

    assert_eq!(
        [
            Style::default().fg(Color::Green),
            Style::default().fg(Color::Yellow),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            Style::default().fg(Color::Red),
        ],
        actual
    );
}

#[test]
fn should_render_cycle_marker_swatch_as_an_uncolored_bold_mark_when_building_markers_legend() {
    assert_eq!(
        Style::default().add_modifier(Modifier::BOLD),
        swatch_style("↻")
    );
}

#[test]
fn should_render_the_usage_count_swatch_in_deferred_gray() {
    assert_eq!(Style::default().fg(Color::DarkGray), swatch_style("N"));
}

#[test]
fn should_reuse_row_view_band_style_for_the_oversized_file_swatch() {
    assert_eq!(
        crate::row_view::band_style(FileSizeBand::Warn),
        swatch_style("N lines")
    );
}

#[test]
fn should_render_the_used_by_header_swatch_in_the_selection_color() {
    assert_eq!(Style::default().fg(Color::Cyan), swatch_style("↑ N:"));
}

#[test]
fn should_render_dimmed_and_crossed_out_removed_name_swatch_when_building_markers_legend() {
    assert_eq!(
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::CROSSED_OUT),
        swatch_style("(dimmed + struck-through name)")
    );
}

#[test]
fn should_render_test_swatches_in_deferred_gray_when_building_markers_legend() {
    assert_eq!(
        [Style::default().fg(Color::DarkGray); 2],
        [swatch_style("[test] (N symbols)"), swatch_style("N tests")]
    );
}

fn line_span<'a>(line: &'a ratatui::text::Line<'static>, content: &str) -> &'a Span<'static> {
    line.spans
        .iter()
        .find(|span| span.content.as_ref() == content)
        .unwrap_or_else(|| panic!("span {content:?} not found in line"))
}

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

/// Every [`crate::help::HelpGroup`] title string, in [`crate::help::keymap_groups`]'s
/// own display order — the full, unfiltered set this crate's group-filtering
/// tests below check subsets of.
const ALL_GROUP_TITLES: [&str; 5] = [
    "Tree focus",
    "Right focus",
    "Source view",
    "Review",
    "Global",
];

/// Which of [`ALL_GROUP_TITLES`] are present in `text` (the overlay's
/// rendered buffer), preserving [`ALL_GROUP_TITLES`]'s own order — the
/// fully-qualified value each context test below compares against, rather
/// than one `assert!(text.contains(...))` per heading.
fn present_group_titles(text: &str) -> Vec<&'static str> {
    ALL_GROUP_TITLES
        .into_iter()
        .filter(|title| text.contains(title))
        .collect()
}

#[test]
fn should_draw_help_overlay_with_keymap_markers_and_glossary_when_help_is_open() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
    assert_eq!(crate::app::Focus::Tree, app.focus());
    // A 150x80 terminal (up from 150x76 for the `✎N` legend entry, ADR
    // 0099): taller again so the overlay's 80% x 90% area still fits
    // every keymap group, the Markers legend, and the trailing Glossary
    // section without the last section being pushed off the bottom.
    // Grown here rather than narrowing the content itself, same
    // rationale as the 100x40 -> 100x50 -> 150x70 -> 150x74 -> 150x76 growths
    // this test already went through for earlier keymap additions.
    let mut terminal = Terminal::new(TestBackend::new(150, 80)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
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

    let text = buffer_text(&terminal);
    assert!(text.contains("Help"));
    // Tree focus (Focus::Tree, ADR 0020) hides the Right-focus and
    // Source-view groups, which are only ever reachable from a different
    // focus/screen (`crate::help::is_group_applicable`'s own doc comment).
    assert_eq!(
        vec!["Tree focus", "Review", "Global"],
        present_group_titles(&text)
    );
    assert!(text.contains("Markers"));
    assert!(text.contains("N lines"));
    assert!(text.contains("Glossary"));
    assert!(text.contains("blast radius"));
}

#[test]
fn should_hide_tree_focus_and_source_view_groups_when_right_focused_on_diff_pane() {
    let report = report_with_one_symbol();
    // `Open` on the symbol row (cursor starts there, `report_with_one_symbol`'s
    // own single file/symbol tree) reaches Focus::Right + RightPane::Diff
    // (ADR 0020) without leaving Screen::Entry.
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Open)
        .handle_key(crate::app::InputKey::ToggleHelp);
    assert_eq!(crate::app::Focus::Right, app.focus());
    let mut terminal = Terminal::new(TestBackend::new(150, 80)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
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

    let text = buffer_text(&terminal);
    assert_eq!(
        vec!["Right focus", "Review", "Global"],
        present_group_titles(&text)
    );
}

#[test]
fn should_show_only_source_view_and_global_groups_on_source_screen() {
    let report = report_with_one_symbol();
    // `report_with_one_symbol`'s tree starts with the cursor on the file
    // row; `Down` moves it onto the symbol row `Source` (`s`) requires
    // (`App::handle_key`'s own `InputKey::Source` arm only fires on a
    // `NodeKind::Symbol` row) — the same sequence
    // `should_show_source_view_scroll_hints_on_source_screen_regardless_of_focus`
    // in `ui::status`'s own tests already uses.
    let app = App::new(&report)
        .handle_key(crate::app::InputKey::Down)
        .handle_key(crate::app::InputKey::Source)
        .handle_key(crate::app::InputKey::ToggleHelp);
    assert!(matches!(app.screen(), crate::app::Screen::Source { .. }));
    let mut terminal = Terminal::new(TestBackend::new(150, 80)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
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

    let text = buffer_text(&terminal);
    // Review is hidden on the source screen: `n`/`N` (AnnotationCompose/AnnotationsList)
    // only dispatch on Screen::Entry (`review_flow::derive_selection_snapshot`,
    // `App::handle_key`'s own `AnnotationsList` arm).
    assert_eq!(vec!["Source view", "Global"], present_group_titles(&text));
}

#[test]
fn should_draw_help_overlay_in_japanese_when_locale_is_japanese() {
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
    let mut terminal = Terminal::new(TestBackend::new(150, 80)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
                &crate::diff_shape::DiffPaneContent::Empty,
                &[],
                &BlastRadiusSelection::NotApplicable,
                None,
                &[],
                &crate::annotation_markers::AnnotationMarkers::default(),
                Locale::Japanese,
            );
        })
        .expect("draw");

    let text = buffer_text(&terminal);
    // `TestBackend`'s buffer devotes two cells to every double-width CJK
    // glyph (the second cell an empty-symbol placeholder that
    // `Cell::symbol` renders as a literal space), so a run of CJK
    // characters reads back with one extra space after each double-width
    // char — `widened` reproduces exactly that spacing (single-width
    // chars are left as-is) so the expected/actual comparison does not
    // have to special-case it per assertion.
    let widened = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if unicode_width::UnicodeWidthChar::width(c) == Some(2) {
                    format!("{c} ")
                } else {
                    c.to_string()
                }
            })
            .collect()
    };

    // One key-binding description (move-cursor, "Tree focus" group).
    assert!(text.contains(&widened("カーソルを移動")));
    // One marker-legend entry (the usage count's explanation).
    assert!(text.contains(&widened("これを使っている本番コードのシンボル数")));
    // One glossary entry (blast radius).
    assert!(text.contains(&widened("依存ツリー")));
    // The overlay title and section headings switch too, proving this is
    // a locale switch and not just one string happening to match.
    assert!(text.contains(&widened("ヘルプ")));
    assert!(text.contains(&widened("用語集")));
    // English text from the same keys must not leak into the Japanese
    // render.
    assert!(!text.contains("Move the cursor"));
}

#[test]
fn should_not_show_glossary_when_terminal_is_too_short_to_fit_the_whole_keymap_and_scroll_is_zero()
{
    // A small terminal (30 rows) whose overlay box cannot fit every
    // keymap group *and* the trailing Glossary section at once — the
    // gap this feature exists to close (previously: the unscrolled
    // `Paragraph` simply clipped the bottom of the content with no way
    // to reach it, `draw_help_overlay`'s pre-scroll doc comment).
    // Pinning that the Glossary is *not* visible at scroll 0 here, and
    // *is* visible after scrolling in the next test, is what proves
    // scrolling actually moves the rendered content rather than the
    // box merely being tall enough by coincidence.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
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

    let text = buffer_text(&terminal);
    assert!(text.contains("Tree focus"));
    assert!(!text.contains("Glossary"), "Glossary should not fit yet");
}

#[test]
fn should_reveal_glossary_after_scrolling_down_when_terminal_is_too_short_to_fit_the_whole_keymap()
{
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(crate::app::InputKey::ToggleHelp);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

    // Scroll well past every keymap group — `handle_scroll_key`'s own
    // clamp-free "requested" semantics mean this overshoots on
    // purpose; `render_scrollable_pane`'s clamp inside `draw` below is
    // what brings it back in bounds, mirroring how every other
    // scrollable pane in this crate is exercised in its own tests.
    let app = app.handle_scroll_key(crate::app::InputKey::ScrollToBottom, 20);

    let mut actual_outcome = crate::ui::DrawOutcome::default();
    terminal
        .draw(|frame| {
            actual_outcome = draw(
                frame,
                &app,
                &report,
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

    let text = buffer_text(&terminal);
    assert!(
        text.contains("Glossary"),
        "Glossary should be visible after scrolling to the bottom"
    );
    assert!(
        text.contains("jumplist"),
        "the last glossary entry should be visible at the bottom"
    );
    // The scroll actually applied must be reported back (`DrawOutcome`'s
    // own doc comment on why `crate::run_app` needs this to fold the
    // overshot request back down) rather than staying at the
    // unclamped `usize::MAX` sentinel `ScrollToBottom` set.
    assert!(actual_outcome.clamped_help_scroll.is_some());
    assert_ne!(Some(usize::MAX), actual_outcome.clamped_help_scroll);
    assert!(actual_outcome.help_scroll_viewport_height.is_some());
}

#[test]
fn should_report_none_clamped_help_scroll_and_none_viewport_height_when_help_overlay_is_closed() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

    let mut actual_outcome = crate::ui::DrawOutcome::default();
    terminal
        .draw(|frame| {
            actual_outcome = draw(
                frame,
                &app,
                &report,
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

    assert_eq!(None, actual_outcome.clamped_help_scroll);
    assert_eq!(None, actual_outcome.help_scroll_viewport_height);
}

#[test]
fn should_not_draw_help_overlay_when_help_is_closed() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("terminal");

    terminal
        .draw(|frame| {
            draw(
                frame,
                &app,
                &report,
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

    let text = buffer_text(&terminal);
    assert!(!text.contains("Glossary"));
}

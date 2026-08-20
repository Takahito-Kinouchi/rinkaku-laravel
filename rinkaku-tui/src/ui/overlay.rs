//! Small popups composited on top of whatever screen was already rendered
//! underneath, after the pane split has drawn everything else: the
//! jump-target popup (ADR 0022) and the quit-confirmation popup (ADR
//! 0085). [`centered_rect`] is this module's own layout primitive, shared
//! with the larger `?` help overlay in [`super::help_overlay`] (ADR 0028
//! split, once this module's combined help-overlay + popup content grew
//! past the file-size threshold).

use super::scroll::{truncate_to_width, windowed_rows_with_indicators};
use crate::locale::Locale;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};

/// A `Rect` centered within `area`, `percent_width`/`percent_height` of
/// `area`'s own dimensions — the standard `ratatui` centered-popup layout
/// recipe (two nested `Layout::vertical`/`horizontal` splits with a
/// `Percentage` constraint sandwiched between two equal `Percentage`
/// margins), extracted as its own pure function so the overlay's sizing
/// rule is nameable and independent of any one popup's own
/// `Clear`/`Paragraph` concerns. Shared by [`super::help_overlay::draw_help_overlay`]
/// and [`draw_jump_popup`].
pub(crate) fn centered_rect(area: Rect, percent_width: u16, percent_height: u16) -> Rect {
    let vertical_margin = (100 - percent_height) / 2;
    let [_, middle, _] = Layout::vertical([
        Constraint::Percentage(vertical_margin),
        Constraint::Percentage(percent_height),
        Constraint::Percentage(vertical_margin),
    ])
    .areas(area);

    let horizontal_margin = (100 - percent_width) / 2;
    let [_, center, _] = Layout::horizontal([
        Constraint::Percentage(horizontal_margin),
        Constraint::Percentage(percent_width),
        Constraint::Percentage(horizontal_margin),
    ])
    .areas(middle);

    center
}

/// Draws the jump-target popup (ADR 0022) centered over `full_area`,
/// listing every candidate as `name (path)` with the currently highlighted
/// one shown reversed — the same `Clear`-first, centered-bordered-box
/// compositing the `?` help overlay already uses, just a narrower and
/// shorter box (60% x 40%, vs. the help overlay's 80% x 90%) since a
/// candidate list is typically much shorter than the whole keymap.
///
/// Windowed around `popup.cursor` via [`windowed_rows_with_indicators`]
/// (post-#61 review finding: this used to hand every candidate to
/// `Paragraph` unscrolled, so a popup with more candidates than the box's
/// height could select an off-screen candidate with no visual feedback at
/// all) — the same cursor-follow scroll `draw_tree_pane` uses, plus dim
/// "… N more above/below" lines inside the box when the window does not
/// reach an edge of the candidate list.
///
/// Candidate labels are [`truncate_to_width`]-ed to the popup's own inner
/// width rather than wrapped (a second bug found while fixing the first:
/// `windowed_rows_with_indicators`'s window math assumes one candidate is
/// one rendered row, but this used to render with `Paragraph::wrap`
/// enabled — a `"{name} ({path})"` label longer than the box's inner width
/// wrapped onto 2-3 physical rows, so the window's row-count budget
/// silently undercounted and pushed later candidates, including the
/// cursor row, off the bottom of the popup with no visual feedback,
/// exactly the failure mode the windowing fix above exists to prevent).
/// Truncating instead of wrapping restores the "one candidate, one row"
/// invariant the window math relies on.
pub(crate) fn draw_jump_popup(frame: &mut Frame, popup: &crate::app::JumpPopup, full_area: Rect) {
    let overlay_area = centered_rect(full_area, 60, 40);
    frame.render_widget(Clear, overlay_area);

    // 2 rows/columns for the top/bottom and left/right border, matching
    // `render_scrollable_pane`'s own `saturating_sub(2)` convention for a
    // bordered pane's inner height/width.
    let viewport_height = overlay_area.height.saturating_sub(2) as usize;
    let viewport_width = overlay_area.width.saturating_sub(2) as usize;
    let (start, end, above, below) =
        windowed_rows_with_indicators(popup.candidates.len(), popup.cursor, viewport_height);

    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(above) = above {
        lines.push(Line::styled(
            above,
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    lines.extend(
        popup.candidates[start..end]
            .iter()
            .enumerate()
            .map(|(offset, candidate)| {
                let text = truncate_to_width(
                    &format!("{} ({})", candidate.name, candidate.path),
                    viewport_width,
                );
                if start + offset == popup.cursor {
                    Line::styled(
                        text,
                        Style::default()
                            .add_modifier(Modifier::REVERSED)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    Line::raw(text)
                }
            }),
    );
    if let Some(below) = below {
        lines.push(Line::styled(
            below,
            Style::default().add_modifier(Modifier::DIM),
        ));
    }

    let block = Block::bordered().title(" Jump to (enter: go, esc: cancel) ");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, overlay_area);
}

/// Draws the quit-confirmation popup (ADR 0085) centered over `full_area`:
/// a short title plus a `y`/Enter-quit, `n`/Esc-cancel prompt, reusing the
/// same `Clear`-first, centered-bordered-box compositing every other popup
/// in this module already uses — just smaller (40% x 20%) than the
/// jump-target popup above, since its content is a single fixed-length
/// prompt rather than a variable-length candidate list.
///
/// `locale` selects the prompt's language via `rust_i18n::t!`, the same
/// mechanism [`super::help_overlay::draw_help_overlay`] already uses for
/// the `?` overlay's own prose (ADR 0055) — an intentional, narrow
/// amendment to ADR 0055's "every other screen stays English-only" scope
/// decision (see ADR 0085's own Consequences): an accidental-quit guard is
/// exactly the kind of low-frequency, high-stakes prompt worth reading
/// correctly regardless of locale, unlike the rest of this crate's
/// English-only chrome.
pub(crate) fn draw_quit_confirm_popup(frame: &mut Frame, full_area: Rect, locale: Locale) {
    let overlay_area = centered_rect(full_area, 40, 20);
    frame.render_widget(Clear, overlay_area);

    let tag = locale.tag();
    let title = format!(" {} ", rust_i18n::t!("quit_confirm.title", locale = tag));
    let prompt = rust_i18n::t!("quit_confirm.prompt", locale = tag).into_owned();

    let block = Block::bordered().title(title);
    let paragraph = Paragraph::new(prompt)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, overlay_area);
}

#[cfg(test)]
mod tests {
    use crate::app::{App, BlastRadiusSelection};
    use crate::locale::Locale;
    use crate::ui::draw;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
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

    #[test]
    fn should_draw_jump_popup_with_every_candidate_when_jump_popup_is_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report).open_jump_popup(vec![
            crate::app::JumpCandidate {
                id: "lib.rs::alpha".to_string(),
                name: "alpha".to_string(),
                path: "lib.rs".to_string(),
            },
            crate::app::JumpCandidate {
                id: "lib.rs::beta".to_string(),
                name: "beta".to_string(),
                path: "lib.rs".to_string(),
            },
        ]);
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).expect("terminal");

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
        assert!(text.contains("Jump to"));
        assert!(text.contains("alpha"));
        assert!(text.contains("beta"));
    }

    #[test]
    fn should_window_candidates_around_cursor_when_popup_has_more_candidates_than_fit() {
        // #61-review finding: the popup used to hand every candidate to
        // `Paragraph` unscrolled, so a popup with more candidates than the
        // box's own height could highlight an off-screen candidate with no
        // visual feedback at all. 25 candidates, cursor moved to the last
        // one (index 24) via repeated Down, is more than any reasonably
        // sized popup box can show at once.
        let report = report_with_one_symbol();
        let candidates: Vec<crate::app::JumpCandidate> = (0..25)
            .map(|i| crate::app::JumpCandidate {
                id: format!("lib.rs::sym{i}"),
                name: format!("sym{i}"),
                path: "lib.rs".to_string(),
            })
            .collect();
        let mut app = App::new(&report).open_jump_popup(candidates);
        for _ in 0..24 {
            app = app.handle_key(crate::app::InputKey::Down);
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).expect("terminal");

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
        // The highlighted candidate (the last one, cursor at index 24) must
        // always be visible — the whole point of the windowing fix.
        assert!(text.contains("sym24"), "cursor candidate sym24 not visible");
        // The first candidate is far outside the window around index 24, so
        // it must not be rendered, and an overflow indicator must say so.
        assert!(!text.contains("sym0 ("), "sym0 should have scrolled off");
        assert!(text.contains("more above"));
    }

    #[test]
    fn should_keep_highlighted_candidate_visible_when_labels_wrap_across_multiple_rows() {
        // Regression test for the bug this change fixes:
        // `windowed_rows_with_indicators` computes its window assuming one
        // candidate = one rendered row, but `draw_jump_popup` used to hand
        // its lines to `Paragraph` with `Wrap { trim: false }` enabled — a
        // candidate label (`"{name} ({path})"`) longer than the popup's
        // inner width wrapped onto 2-3 physical rows, so the window's
        // row-count math silently undercounted and pushed later
        // candidates, including the cursor row, off the bottom of the
        // popup with no visual feedback at all. An 80x24 terminal (a
        // common real-world size, smaller than every other test in this
        // file's 100-wide terminals) with 40-90 character paths reproduces
        // it: the popup's 60%-width box is nowhere near wide enough to fit
        // "name (path)" on one row unwrapped.
        let report = report_with_one_symbol();
        let candidates: Vec<crate::app::JumpCandidate> = (0..20)
            .map(|i| crate::app::JumpCandidate {
                id: format!("lib.rs::sym{i}"),
                name: format!("very_long_symbol_name_number_{i}"),
                path: format!(
                    "src/very/deeply/nested/module/path/number_{i}/that/is/quite/long/file.rs"
                ),
            })
            .collect();
        let mut app = App::new(&report).open_jump_popup(candidates);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");

        // Move the cursor one step at a time, checking after *every* step
        // (not just at the end) that the highlighted candidate is visible —
        // the bug manifested at intermediate cursor positions too, not only
        // at the very last candidate.
        for step in 0..19 {
            app = app.handle_key(crate::app::InputKey::Down);
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
            let expected_marker = format!("very_long_symbol_name_number_{}", step + 1);
            assert!(
                text.contains(&expected_marker),
                "cursor candidate {expected_marker} not visible after {} Down presses:\n{text}",
                step + 1
            );
        }
    }

    #[test]
    fn should_not_draw_jump_popup_when_no_jump_is_pending() {
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
        assert!(!text.contains("Jump to"));
    }

    // Quit-confirmation popup (ADR 0085).

    #[test]
    fn should_draw_quit_confirm_popup_when_open() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::RequestQuit);
        assert!(app.quit_confirm_open());
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
        assert!(text.contains("Quit?"));
        assert!(text.contains("Quit rinkaku-laravel?"));
        assert!(text.contains("y/Enter: quit"));
        assert!(text.contains("n/Esc: cancel"));
    }

    #[test]
    fn should_draw_quit_confirm_popup_in_japanese_when_locale_is_japanese() {
        let report = report_with_one_symbol();
        let app = App::new(&report).handle_key(crate::app::InputKey::RequestQuit);
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
                    Locale::Japanese,
                );
            })
            .expect("draw");

        let text = buffer_text(&terminal);
        // `TestBackend`'s buffer devotes two cells to every double-width
        // CJK glyph (the second cell an empty-symbol placeholder that
        // `Cell::symbol` renders as a literal space), so a run of CJK
        // characters reads back with one extra space after each
        // double-width char — mirrors `ui::help_overlay::tests`'s own
        // `widened` helper for the identical reason.
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
        assert!(text.contains(&widened("終了確認")));
        assert!(text.contains(&widened("終了しますか？")));
    }

    #[test]
    fn should_not_draw_quit_confirm_popup_when_closed() {
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
        assert!(!text.contains("Quit rinkaku-laravel?"));
    }
}

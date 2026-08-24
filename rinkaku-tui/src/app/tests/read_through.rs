use super::{report_with_one_symbol, report_with_two_directories};
use crate::app::{App, Focus, InputKey, ReadThrough, Screen};
use pretty_assertions::assert_eq;

/// The measurement a Diff pane frame reports when the selected symbol
/// still runs past the bottom edge — the state ADR 0088's read-through
/// keys exist for.
fn continues_below() -> Option<ReadThrough> {
    Some(ReadThrough {
        rows_above: 0,
        rows_below: 12,
        step: 5,
    })
}

/// The mirror image: the symbol started above the viewport's top edge.
fn continues_above() -> Option<ReadThrough> {
    Some(ReadThrough {
        rows_above: 12,
        rows_below: 0,
        step: 5,
    })
}

/// What a frame reports for a symbol whose whole change is already on
/// screen — nothing left to read through in either direction.
fn fully_visible() -> Option<ReadThrough> {
    Some(ReadThrough {
        rows_above: 0,
        rows_below: 0,
        step: 5,
    })
}

#[test]
fn should_scroll_the_diff_pane_when_the_selected_symbol_continues_below_the_viewport() {
    let report = report_with_two_directories();
    let app = App::new(&report).with_right_pane_scroll(3);
    let cursor_before = app.nav().cursor();

    let app = app.handle_read_through_key(InputKey::ReadThroughDown, continues_below());

    assert_eq!(8, app.right_pane_scroll());
    assert_eq!(cursor_before, app.nav().cursor());
}

#[test]
fn should_move_the_tree_cursor_when_nothing_of_the_selected_symbol_is_left_below() {
    let report = report_with_two_directories();
    let app = App::new(&report).with_right_pane_scroll(3);
    let cursor_before = app.nav().cursor();

    let app = app.handle_read_through_key(InputKey::ReadThroughDown, fully_visible());

    assert_eq!(cursor_before + 1, app.nav().cursor());
    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_scroll_the_diff_pane_back_when_the_selected_symbol_continues_above_the_viewport() {
    let report = report_with_two_directories();
    let app = App::new(&report).with_right_pane_scroll(7);

    let app = app.handle_read_through_key(InputKey::ReadThroughUp, continues_above());

    assert_eq!(2, app.right_pane_scroll());
}

#[test]
fn should_move_the_tree_cursor_up_when_nothing_of_the_selected_symbol_is_left_above() {
    let report = report_with_two_directories();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .with_right_pane_scroll(3);
    assert_eq!(1, app.nav().cursor());

    let app = app.handle_read_through_key(InputKey::ReadThroughUp, fully_visible());

    assert_eq!(0, app.nav().cursor());
    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_stop_at_the_top_when_reading_backward_past_the_start_of_the_pane() {
    let report = report_with_two_directories();
    let app = App::new(&report).with_right_pane_scroll(2);

    let app = app.handle_read_through_key(InputKey::ReadThroughUp, continues_above());

    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_move_the_tree_cursor_when_no_frame_has_measured_a_diff_pane_yet() {
    // A right pane other than Diff, the Diff pane's own placeholder path,
    // or the very first key of a session: nothing was measured, so there
    // is nothing to read through and both keys are plain cursor movement.
    let report = report_with_two_directories();
    let app = App::new(&report);

    let app = app.handle_read_through_key(InputKey::ReadThroughDown, None);

    assert_eq!(1, app.nav().cursor());
    assert_eq!(0, app.right_pane_scroll());
}

#[test]
fn should_read_through_while_right_focused_the_same_way_it_does_from_the_tree() {
    let report = report_with_two_directories();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Open)
        .with_right_pane_scroll(3);
    assert_eq!(Focus::Right, app.focus());

    let app = app.handle_read_through_key(InputKey::ReadThroughDown, continues_below());

    assert_eq!(8, app.right_pane_scroll());
}

#[test]
fn should_do_nothing_on_the_source_screen() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source)
        .with_right_pane_scroll(3);
    assert!(matches!(app.screen(), Screen::Source { .. }));
    let before = app.clone();

    let app = app.handle_read_through_key(InputKey::ReadThroughDown, continues_below());

    assert_eq!(before, app);
}

#[test]
fn should_survive_handle_keys_blanket_scroll_reset_when_it_scrolls() {
    // `run_app` calls `handle_key` first for the blanket bookkeeping and
    // `handle_read_through_key` second (ADR 0088's third dispatch step) —
    // without the `preserve_scroll` exemption for these two variants, the
    // first call would zero `right_pane_scroll` a moment before the second
    // scrolled it, exactly as it once did for `Ctrl-d`/`Ctrl-u`.
    let report = report_with_two_directories();
    let app = App::new(&report).with_right_pane_scroll(3);

    let app = app
        .handle_key(InputKey::ReadThroughDown)
        .handle_read_through_key(InputKey::ReadThroughDown, continues_below());

    assert_eq!(8, app.right_pane_scroll());
}

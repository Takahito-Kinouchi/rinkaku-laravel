//! `translate_key` tests for ADR 0088's amendment: the arrow keys read
//! through the change while the entry view's tree has focus, and translate
//! exactly like `j`/`k` everywhere else. The two halves of that contract
//! are equally load-bearing — the amendment only holds if `j`/`k` keep the
//! one-row cursor move ADR 0088's own Alternatives refused to take away
//! from them, and if no overlay or popup that already binds Up/Down loses
//! its binding to the new arm.

use super::{candidate, empty_report, report_with_one_symbol};
use crate::app::{App, Focus, InputKey, Screen};
use crate::input_translate::translate_key;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

#[test]
fn should_translate_arrow_down_to_read_through_when_tree_focused_on_entry_screen() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let actual = translate_key(KeyCode::Down, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ReadThroughDown), actual);
}

#[test]
fn should_translate_arrow_up_to_read_through_when_tree_focused_on_entry_screen() {
    let report = report_with_one_symbol();
    let app = App::new(&report);
    assert_eq!(Focus::Tree, app.focus());

    let actual = translate_key(KeyCode::Up, KeyModifiers::NONE, &app);

    assert_eq!(Some(InputKey::ReadThroughUp), actual);
}

#[test]
fn should_translate_j_and_k_to_plain_motion_when_tree_focused_on_entry_screen() {
    // ADR 0088 rejected making `j` itself read through — one row per press
    // is the tree's own contract, relied on by search jumps and `ctrl-d`
    // sizing. The amendment gives the gesture to the arrow keys precisely
    // so this stays true.
    let report = report_with_one_symbol();
    let app = App::new(&report);

    assert_eq!(
        Some(InputKey::Down),
        translate_key(KeyCode::Char('j'), KeyModifiers::NONE, &app)
    );
    assert_eq!(
        Some(InputKey::Up),
        translate_key(KeyCode::Char('k'), KeyModifiers::NONE, &app)
    );
}

#[test]
fn should_translate_arrow_keys_to_plain_motion_when_right_focused_on_entry_screen() {
    // The right pane already scrolls a line per press there (ADR 0020),
    // which is the fine-grained reading the amendment must not displace.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::Open);
    assert_eq!(Focus::Right, app.focus());

    assert_eq!(
        Some(InputKey::Down),
        translate_key(KeyCode::Down, KeyModifiers::NONE, &app)
    );
    assert_eq!(
        Some(InputKey::Up),
        translate_key(KeyCode::Up, KeyModifiers::NONE, &app)
    );
}

#[test]
fn should_translate_arrow_keys_to_plain_motion_on_source_screen() {
    let report = report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Source);
    assert!(matches!(app.screen(), Screen::Source { .. }));

    assert_eq!(
        Some(InputKey::Down),
        translate_key(KeyCode::Down, KeyModifiers::NONE, &app)
    );
    assert_eq!(
        Some(InputKey::Up),
        translate_key(KeyCode::Up, KeyModifiers::NONE, &app)
    );
}

#[test]
fn should_translate_arrow_keys_to_plain_motion_while_help_overlay_is_open() {
    // The overlay is opened from the entry view's tree, so it is exactly
    // the state the new arm would capture if it were not short-circuited
    // by the overlay's own early return.
    let report = report_with_one_symbol();
    let app = App::new(&report).handle_key(InputKey::ToggleHelp);
    assert!(app.help_open());

    assert_eq!(
        Some(InputKey::Down),
        translate_key(KeyCode::Down, KeyModifiers::NONE, &app)
    );
    assert_eq!(
        Some(InputKey::Up),
        translate_key(KeyCode::Up, KeyModifiers::NONE, &app)
    );
}

#[test]
fn should_translate_arrow_keys_to_plain_motion_while_jump_popup_is_open() {
    let report = empty_report();
    let app = App::new(&report).open_jump_popup(vec![candidate("a", "a", "a.rs")]);

    assert_eq!(
        Some(InputKey::Down),
        translate_key(KeyCode::Down, KeyModifiers::NONE, &app)
    );
    assert_eq!(
        Some(InputKey::Up),
        translate_key(KeyCode::Up, KeyModifiers::NONE, &app)
    );
}

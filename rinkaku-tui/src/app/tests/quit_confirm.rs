use super::empty_report;
use crate::app::{App, InputKey};
use pretty_assertions::assert_eq;

#[test]
fn should_start_with_quit_confirm_closed() {
    let report = empty_report();
    let app = App::new(&report);

    assert_eq!(false, app.quit_confirm_open());
    assert_eq!(false, app.should_quit());
}

#[test]
fn should_open_quit_confirm_instead_of_quitting_when_request_quit_is_pressed() {
    // ADR 0085: `q` (`InputKey::RequestQuit`) no longer quits directly —
    // it opens the confirmation popup, and the app must still be running.
    let report = empty_report();
    let app = App::new(&report);

    let app = app.handle_key(InputKey::RequestQuit);

    assert_eq!(true, app.quit_confirm_open());
    assert_eq!(false, app.should_quit());
}

#[test]
fn should_quit_when_popup_confirm_is_pressed_while_quit_confirm_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::RequestQuit);
    assert_eq!(true, app.quit_confirm_open());

    let app = app.handle_key(InputKey::PopupConfirm);

    assert_eq!(true, app.should_quit());
    assert_eq!(false, app.quit_confirm_open());
}

#[test]
fn should_close_popup_without_quitting_when_popup_cancel_is_pressed_while_quit_confirm_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::RequestQuit);
    assert_eq!(true, app.quit_confirm_open());

    let app = app.handle_key(InputKey::PopupCancel);

    assert_eq!(false, app.quit_confirm_open());
    assert_eq!(false, app.should_quit());
}

#[test]
fn should_quit_immediately_via_ctrl_c_even_while_quit_confirm_is_open() {
    // Ctrl-C (`InputKey::Quit`) stays the unconditional escape hatch —
    // `App::handle_quit_confirm_key` honors it the same way `PopupConfirm`
    // is honored, rather than swallowing it like every other key.
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::RequestQuit);
    assert_eq!(true, app.quit_confirm_open());

    let app = app.handle_key(InputKey::Quit);

    assert_eq!(true, app.should_quit());
}

#[test]
fn should_swallow_unrelated_keys_while_quit_confirm_is_open() {
    let report = empty_report();
    let app = App::new(&report).handle_key(InputKey::RequestQuit);
    let cursor_before = app.nav().cursor();

    let app = app.handle_key(InputKey::Down);

    assert_eq!(true, app.quit_confirm_open());
    assert_eq!(false, app.should_quit());
    assert_eq!(cursor_before, app.nav().cursor());
}

#[test]
fn should_preserve_right_pane_scroll_when_quit_confirm_opens() {
    // ADR 0085: opening the popup by mistake and then cancelling must not
    // have thrown away the reviewer's scroll position along the way — the
    // blanket end-of-`handle_key` scroll reset is skipped for
    // `RequestQuit` the same way it already is for the other popups this
    // function short-circuits.
    let report = super::report_with_one_symbol();
    let app = App::new(&report)
        .handle_key(InputKey::Open)
        .handle_key(InputKey::Down)
        .handle_key(InputKey::Down);
    let scroll_before = app.right_pane_scroll();
    assert!(scroll_before > 0);

    let app = app
        .handle_key(InputKey::RequestQuit)
        .handle_key(InputKey::PopupCancel);

    assert_eq!(scroll_before, app.right_pane_scroll());
}

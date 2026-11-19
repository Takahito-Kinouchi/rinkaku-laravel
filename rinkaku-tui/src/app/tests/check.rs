use super::*;
use pretty_assertions::assert_eq;

// ADR 0099 decision 4: `space` on a symbol row checks it off and follows
// the change to a symbol that uses it; on any other row it still folds.
// `report_with_caller_and_callee`: `foo` (a/one.rs) uses `bar` (b/two.rs).
// Rows: a(0), a/one.rs(1), foo(2), b(3), b/two.rs(4), bar(5).

fn cursor_symbol(app: &App) -> Option<&str> {
    app.selected_symbol_id()
}

#[test]
fn should_check_the_symbol_and_move_to_the_symbol_that_uses_it() {
    let report = super::jump::report_with_caller_and_callee();
    let app = App::new(&report).with_nav_cursor(5);
    assert_eq!(Some("b/two.rs::bar"), cursor_symbol(&app));

    let app = app.handle_key(InputKey::Select);

    assert_eq!(
        (true, Some("a/one.rs::foo"), (1, 2)),
        (
            app.is_checked("b/two.rs::bar"),
            cursor_symbol(&app),
            app.review_progress()
        )
    );
}

#[test]
fn should_stay_and_say_so_once_every_symbol_is_checked() {
    let report = super::jump::report_with_caller_and_callee();
    let app = App::new(&report)
        .with_nav_cursor(5)
        .handle_key(InputKey::Select)
        .handle_key(InputKey::Select);

    assert_eq!(
        (
            Some("a/one.rs::foo"),
            Some("note: every symbol is checked"),
            (2, 2)
        ),
        (cursor_symbol(&app), app.status(), app.review_progress())
    );
}

#[test]
fn should_uncheck_a_checked_symbol_without_moving() {
    let report = super::jump::report_with_caller_and_callee();
    let app = App::new(&report)
        .with_nav_cursor(5)
        .handle_key(InputKey::Select)
        .with_nav_cursor(5)
        .handle_key(InputKey::Select);

    assert_eq!(
        (false, Some("b/two.rs::bar")),
        (app.is_checked("b/two.rs::bar"), cursor_symbol(&app))
    );
}

#[test]
fn should_still_fold_a_directory_row_on_space() {
    let report = super::jump::report_with_caller_and_callee();
    let app = App::new(&report).handle_key(InputKey::Select);

    let rows = app.nav().rows(app.tree());
    assert_eq!(
        vec!["a", "b", "b/two.rs", "b/two.rs"],
        rows.iter()
            .map(|row| row.node.path.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn should_come_back_to_the_checked_symbol_through_the_jumplist() {
    let report = super::jump::report_with_caller_and_callee();
    let app = App::new(&report)
        .with_nav_cursor(5)
        .handle_key(InputKey::Select)
        .handle_key(InputKey::JumpBack);

    assert_eq!(Some("b/two.rs::bar"), cursor_symbol(&app));
}

use super::*;
use pretty_assertions::assert_eq;

fn bold(text: &str) -> Span<'static> {
    Span::styled(
        text.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    )
}

fn users_line(count: usize, names: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("↑ {count}: "), crate::glyph::selection()),
        Span::raw(names.to_string()),
    ])
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| name.to_string()).collect()
}

#[test]
fn should_join_symbol_name_and_path_on_first_header_line_when_selection_name_is_present() {
    let actual = diff_pane_header_lines(None, Some("foo"), "src/lib.rs", &[], 0, 80);

    assert_eq!(vec![Line::from(bold("foo · src/lib.rs"))], actual);
}

#[test]
fn should_show_bare_path_on_first_header_line_when_no_selection_name() {
    let actual = diff_pane_header_lines(None, None, "src/lib.rs", &[], 0, 80);

    assert_eq!(vec![Line::from(bold("src/lib.rs"))], actual);
}

#[test]
fn should_lead_the_first_header_line_with_the_rows_own_tree_mark() {
    let mark = Span::styled("●", crate::glyph::danger());

    let actual = diff_pane_header_lines(Some(mark.clone()), Some("foo"), "src/lib.rs", &[], 0, 80);

    assert_eq!(
        vec![Line::from(vec![
            mark,
            Span::raw(" "),
            bold("foo · src/lib.rs")
        ])],
        actual
    );
}

#[test]
fn should_leave_out_a_blank_mark_when_the_symbol_is_body_only() {
    let actual =
        diff_pane_header_lines(Some(Span::raw(" ")), Some("foo"), "src/lib.rs", &[], 0, 80);

    assert_eq!(vec![Line::from(bold("foo · src/lib.rs"))], actual);
}

#[test]
fn should_name_the_symbols_that_use_this_one_on_the_second_line() {
    let used_by = names(&["build_symbol", "kind_word"]);

    let actual = diff_pane_header_lines(None, Some("foo"), "src/lib.rs", &used_by, 2, 80);

    assert_eq!(
        vec![
            Line::from(bold("foo · src/lib.rs")),
            users_line(2, "build_symbol · kind_word")
        ],
        actual
    );
}

#[test]
fn should_truncate_first_header_line_keeping_the_tail_when_it_overflows_width() {
    let actual = diff_pane_header_lines(
        None,
        Some("very_long_symbol_name_here"),
        "src/very/deeply/nested/module/lib.rs",
        &[],
        0,
        20,
    );

    assert_eq!(vec![Line::from(bold("…ested/module/lib.rs"))], actual);
}

#[test]
fn should_count_each_user_once_and_name_a_repeated_name_once_with_its_multiplicity() {
    let used_by = names(&["build_symbol", "kind_abbrev", "kind_abbrev", "kind_word"]);

    let actual = users_summary(&used_by);

    assert_eq!("build_symbol · kind_abbrev ×2 · kind_word", actual);
}

#[test]
fn should_say_how_many_users_are_left_to_read_once_some_are_checked() {
    let used_by = names(&["build_symbol", "kind_word", "symbol_kind"]);

    let actual = diff_pane_header_lines(None, Some("foo"), "src/lib.rs", &used_by, 1, 80);

    assert_eq!(
        Some(&Line::from(vec![
            Span::styled("↑ 3 (1 unchecked): ", crate::glyph::selection()),
            Span::raw("build_symbol · kind_word · symbol_kind"),
        ])),
        actual.get(1)
    );
}

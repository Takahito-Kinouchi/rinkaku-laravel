use super::*;
use pretty_assertions::assert_eq;

// ADR 0099 decision 1: a symbol row carries one mark and no badges; a
// `Dir`/`File` row carries no counts, and a `●` only while it is collapsed
// over a symbol that may break its users.

fn render(row: &Row<'_>, label: &str) -> Line<'static> {
    entry_row_line(
        row,
        label,
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        crate::connections::Connection::Neutral,
        false,
    )
}

fn signature_changed(name: &str) -> SymbolRef {
    SymbolRef {
        classification: Some(Classification::SignatureChanged),
        ..plain_symbol(name)
    }
}

fn used_by(count: usize) -> Badges {
    Badges {
        fan_in: count,
        ..Badges::default()
    }
}

fn row<'a>(node: &'a TreeNode, expanded: bool) -> Row<'a> {
    Row {
        node,
        depth: 0,
        expanded,
    }
}

#[test]
fn should_mark_a_signature_changed_symbol_that_others_use_as_may_break() {
    let node = symbol_node("lib.rs", signature_changed("foo"), used_by(2));

    let actual = render(&row(&node, false), "");

    let expected = Line::from(vec![
        Span::raw(""),
        Span::raw("  "),
        Span::styled("●", crate::glyph::danger().add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled("foo", Style::default()),
    ]);
    assert_eq!(expected, actual);
}

#[test]
fn should_mark_a_signature_changed_symbol_below_the_fan_in_threshold_as_a_contract_change() {
    let node = symbol_node("lib.rs", signature_changed("foo"), used_by(1));

    let actual = render(&row(&node, false), "");

    let expected = Line::from(vec![
        Span::raw(""),
        Span::raw("  "),
        Span::styled("○", crate::glyph::contract()),
        Span::raw(" "),
        Span::styled("foo", Style::default()),
    ]);
    assert_eq!(expected, actual);
}

#[test]
fn should_leave_test_coverage_off_the_row_when_no_test_references_the_symbol() {
    let node = symbol_node(
        "lib.rs",
        signature_changed("foo"),
        Badges {
            test_count: Some(0),
            ..Badges::default()
        },
    );

    assert_eq!("  ○ foo", line_text(&render(&row(&node, false), "")));
}

#[test]
fn should_show_no_counts_on_a_directory_row() {
    let node = dir_node(
        "src",
        Badges {
            changed_symbols: 3,
            contract_changes: 2,
            fan_in: 9,
            file_size_warn_count: 1,
            ..Badges::default()
        },
        vec![file_node("src/a.rs", Badges::default())],
    );

    assert_eq!("▾ src", line_text(&render(&row(&node, true), "src")));
}

#[test]
fn should_mark_a_collapsed_directory_hiding_a_symbol_that_may_break() {
    let risky = symbol_node("src/a.rs", signature_changed("foo"), used_by(3));
    let file = TreeNode {
        children: vec![risky],
        ..file_node("src/a.rs", Badges::default())
    };
    let node = dir_node("src", Badges::default(), vec![file]);

    assert_eq!("▸ ● src", line_text(&render(&row(&node, false), "src")));
}

#[test]
fn should_not_mark_an_expanded_directory_since_the_symbols_own_row_is_visible() {
    let risky = symbol_node("src/a.rs", signature_changed("foo"), used_by(3));
    let file = TreeNode {
        children: vec![risky],
        ..file_node("src/a.rs", Badges::default())
    };
    let node = dir_node("src", Badges::default(), vec![file]);

    assert_eq!("▾ src", line_text(&render(&row(&node, true), "src")));
}

#[test]
fn should_mark_a_collapsed_file_hiding_a_symbol_that_may_break() {
    let risky = symbol_node("a.rs", signature_changed("foo"), used_by(2));
    let node = TreeNode {
        children: vec![risky],
        ..file_node("a.rs", Badges::default())
    };

    assert_eq!("▸ ● a.rs", line_text(&render(&row(&node, false), "a.rs")));
}

#[test]
fn should_report_how_many_symbols_use_a_symbol_row() {
    let node = symbol_node("lib.rs", signature_changed("foo"), used_by(6));

    assert_eq!(Some(6), row_uses_count(&row(&node, false)));
}

#[test]
fn should_report_no_usage_count_when_nothing_uses_the_symbol() {
    let node = symbol_node("lib.rs", signature_changed("foo"), used_by(0));

    assert_eq!(None, row_uses_count(&row(&node, false)));
}

#[test]
fn should_report_no_usage_count_on_a_file_row_even_with_aggregated_fan_in() {
    let node = file_node("lib.rs", used_by(6));

    assert_eq!(None, row_uses_count(&row(&node, false)));
}

#[test]
fn should_color_the_usage_count_red_only_when_the_symbol_may_break() {
    let risky = symbol_node("lib.rs", signature_changed("foo"), used_by(2));
    let added = symbol_node(
        "lib.rs",
        SymbolRef {
            classification: Some(Classification::Added),
            ..plain_symbol("bar")
        },
        used_by(2),
    );

    assert_eq!(
        (crate::glyph::danger(), crate::glyph::deferred()),
        (
            uses_count_style(&row(&risky, false)),
            uses_count_style(&row(&added, false))
        )
    );
}

fn render_with(row: &Row<'_>, connection: crate::connections::Connection) -> Line<'static> {
    entry_row_line(
        row,
        "",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        connection,
        false,
    )
}

#[test]
fn should_put_an_up_arrow_on_a_symbol_that_uses_the_cursors_symbol() {
    let node = symbol_node("lib.rs", plain_symbol("caller"), Badges::default());

    let actual = render_with(
        &row(&node, false),
        crate::connections::Connection::UsesCursor,
    );

    assert_eq!(
        Some(&Span::styled("↑ ", crate::glyph::selection())),
        actual.spans.get(1)
    );
}

#[test]
fn should_put_a_down_arrow_on_a_symbol_the_cursors_symbol_uses() {
    let node = symbol_node("lib.rs", plain_symbol("callee"), Badges::default());

    let actual = render_with(
        &row(&node, false),
        crate::connections::Connection::UsedByCursor,
    );

    assert_eq!("↓   callee", line_text(&actual));
}

#[test]
fn should_fade_a_symbol_unrelated_to_the_cursors_symbol() {
    let node = symbol_node("lib.rs", plain_symbol("bystander"), Badges::default());

    let actual = render_with(
        &row(&node, false),
        crate::connections::Connection::Unrelated,
    );

    assert_eq!(Style::default().add_modifier(Modifier::DIM), actual.style);
}

use super::*;
use pretty_assertions::assert_eq;
use rinkaku_core::file_size::FileSizeBand;

// ADR 0099 decision 1: a file row shows its line count only past the warn
// threshold (ADR 0028's Warn/Split bands), the only sizes that call for
// action.

fn file_with(band: FileSizeBand, line_count: usize) -> TreeNode {
    file_node(
        "src/f.rs",
        Badges {
            own_file_size_band: Some(band),
            own_file_line_count: Some(line_count),
            ..Badges::default()
        },
    )
}

fn render(node: &TreeNode) -> Line<'static> {
    let row = Row {
        node,
        depth: 0,
        expanded: false,
    };
    entry_row_line(
        &row,
        "f.rs",
        &HashMap::new(),
        &crate::annotation_markers::AnnotationMarkers::default(),
        crate::row_view::RowState::default(),
    )
}

#[test]
fn should_show_no_line_count_for_a_normal_or_watch_band_file() {
    let actual = [
        line_text(&render(&file_with(FileSizeBand::Normal, 120))),
        line_text(&render(&file_with(FileSizeBand::Watch, 950))),
    ];

    assert_eq!(["  f.rs".to_string(), "  f.rs".to_string()], actual);
}

#[test]
fn should_show_the_line_count_in_red_for_a_warn_band_file() {
    let actual = render(&file_with(FileSizeBand::Warn, 1240));

    let expected = Line::from(vec![
        Span::raw(""),
        Span::raw("  "),
        Span::styled("f.rs", Style::default()),
        Span::raw(" "),
        Span::styled("1240 lines", crate::glyph::danger()),
    ]);
    assert_eq!(expected, actual);
}

#[test]
fn should_show_the_line_count_in_bold_red_for_a_split_band_file() {
    let actual = render(&file_with(FileSizeBand::Split, 4837));

    let expected = Line::from(vec![
        Span::raw(""),
        Span::raw("  "),
        Span::styled("f.rs", Style::default()),
        Span::raw(" "),
        Span::styled(
            "4837 lines",
            crate::glyph::danger().add_modifier(Modifier::BOLD),
        ),
    ]);
    assert_eq!(expected, actual);
}

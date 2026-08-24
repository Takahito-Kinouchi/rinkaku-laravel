use super::*;
use pretty_assertions::assert_eq;

// --- marked_rows_outside_viewport ---

#[test]
fn should_count_nothing_outside_when_every_marked_row_is_on_screen() {
    let origins = vec![0, 1, 2, 3];

    let actual = marked_rows_outside_viewport(&origins, &[1, 2], 0, 4);

    assert_eq!(MarkedRowsOutsideViewport { above: 0, below: 0 }, actual);
}

#[test]
fn should_count_the_marked_rows_past_the_bottom_edge_when_the_symbol_outgrows_the_pane() {
    let origins = vec![0, 1, 2, 3, 4, 5];

    let actual = marked_rows_outside_viewport(&origins, &[0, 1, 2, 3, 4, 5], 0, 2);

    assert_eq!(MarkedRowsOutsideViewport { above: 0, below: 4 }, actual);
}

#[test]
fn should_count_both_edges_when_the_viewport_sits_inside_the_marked_range() {
    let origins = vec![0, 1, 2, 3, 4, 5, 6];

    let actual = marked_rows_outside_viewport(&origins, &[0, 1, 2, 3, 4, 5, 6], 3, 2);

    assert_eq!(MarkedRowsOutsideViewport { above: 3, below: 2 }, actual);
}

#[test]
fn should_count_in_logical_rows_not_display_rows_when_the_body_wraps() {
    // Logical row 0 wraps onto three display rows, so a 3-row viewport at
    // the top shows exactly one logical row — rows 1 and 2 are below it,
    // even though only two display rows separate them from the edge.
    let origins = vec![0, 0, 0, 1, 2];

    let actual = marked_rows_outside_viewport(&origins, &[0, 1, 2], 0, 3);

    assert_eq!(MarkedRowsOutsideViewport { above: 0, below: 2 }, actual);
}

#[test]
fn should_treat_a_partly_visible_wrapped_row_as_visible() {
    // The viewport's last display row is the *middle* fragment of logical
    // row 1 — the reviewer can see that row and that it continues, so it
    // is not something they have not seen at all.
    let origins = vec![0, 1, 1, 1];

    let actual = marked_rows_outside_viewport(&origins, &[0, 1], 0, 2);

    assert_eq!(MarkedRowsOutsideViewport { above: 0, below: 0 }, actual);
}

#[test]
fn should_count_nothing_when_there_are_no_marked_rows() {
    let origins = vec![0, 1, 2, 3];

    let actual = marked_rows_outside_viewport(&origins, &[], 0, 1);

    assert_eq!(MarkedRowsOutsideViewport::default(), actual);
}

#[test]
fn should_count_nothing_when_the_pane_has_no_height_to_show_anything_in() {
    let origins = vec![0, 1, 2, 3];

    let actual = marked_rows_outside_viewport(&origins, &[0, 3], 0, 0);

    assert_eq!(MarkedRowsOutsideViewport::default(), actual);
}

#[test]
fn should_count_nothing_when_there_is_no_content_at_all() {
    let actual = marked_rows_outside_viewport(&[], &[0, 1], 0, 10);

    assert_eq!(MarkedRowsOutsideViewport::default(), actual);
}

#[test]
fn should_clamp_the_bottom_edge_to_the_content_when_the_viewport_outgrows_it() {
    let origins = vec![0, 1];

    let actual = marked_rows_outside_viewport(&origins, &[0, 1], 0, 50);

    assert_eq!(MarkedRowsOutsideViewport::default(), actual);
}

// --- visible_logical_span ---

#[test]
fn should_span_every_visible_logical_row_when_nothing_wraps() {
    let origins = vec![0, 1, 2, 3, 4];

    let actual = visible_logical_span(&origins, 1, 3);

    assert_eq!(3, actual);
}

#[test]
fn should_span_fewer_logical_rows_than_display_rows_when_the_body_wraps() {
    let origins = vec![0, 0, 0, 1, 2];

    let actual = visible_logical_span(&origins, 0, 4);

    assert_eq!(2, actual);
}

#[test]
fn should_span_one_row_when_a_single_logical_row_fills_the_viewport() {
    let origins = vec![0, 0, 0, 0];

    let actual = visible_logical_span(&origins, 0, 2);

    assert_eq!(1, actual);
}

#[test]
fn should_span_nothing_when_there_is_no_content_at_all() {
    let actual = visible_logical_span(&[], 0, 10);

    assert_eq!(0, actual);
}

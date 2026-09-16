//! `read_through_claim` tests: which rendered rows each row of the tree is
//! responsible for paging through, and the property the whole partition
//! exists for — the file row's claim plus every symbol's claim tile the
//! body exactly once (ADR 0088's 2026-09-15 amendment).

use super::*;
use crate::app::DiffViewMode;
use crate::diff_shape::ReadThroughSelection;
use crate::diff_view::{DiffLine, DiffLineKind};
use pretty_assertions::assert_eq;

/// Rows 0..=8: header(0) at new-side line 1, then body lines 1..=8 at rows
/// 1..=8 — the one fixture every claim below is expressed against, so the
/// expected row lists can be read directly as "which part of this body".
fn eight_line_file() -> DiffPaneContent {
    DiffPaneContent::File(vec![attributed(
        0,
        hunk(
            "@@ -1,8 +1,8 @@",
            Some((1, 8)),
            vec![
                "use std::fmt;",
                "",
                "fn alpha() {",
                "}",
                "",
                "fn beta() {",
                "}",
                "",
            ],
        ),
    )])
}

fn range(start: usize, end: usize) -> LineRange {
    LineRange { start, end }
}

#[test]
fn should_claim_nothing_when_content_is_empty() {
    let actual = read_through_claim(
        &DiffPaneContent::Empty,
        &[range(1, 2)],
        ReadThroughSelection::FileRow,
        DiffViewMode::Unified,
    );

    assert_eq!(Vec::<usize>::new(), actual);
}

#[test]
fn should_claim_the_whole_body_for_a_file_row_when_the_file_yields_no_symbols() {
    let actual = read_through_claim(
        &eight_line_file(),
        &[],
        ReadThroughSelection::FileRow,
        DiffViewMode::Unified,
    );

    assert_eq!(vec![0, 1, 2, 3, 4, 5, 6, 7, 8], actual);
}

#[test]
fn should_claim_the_rows_above_the_first_symbol_for_a_file_row() {
    // `alpha` starts at new-side line 3 (row 3), so the file row owns the
    // `use` line and the blank after it — the rows no symbol will read.
    let actual = read_through_claim(
        &eight_line_file(),
        &[range(3, 4), range(6, 7)],
        ReadThroughSelection::FileRow,
        DiffViewMode::Unified,
    );

    assert_eq!(vec![0, 1, 2], actual);
}

#[test]
fn should_claim_nothing_for_a_file_row_whose_first_symbol_starts_at_the_first_row() {
    // The hunk header shares its first body line's coordinate
    // (`hunk_header_position`), so a symbol covering line 1 starts at row 0
    // and there is no leading gap left for the file row to read.
    let actual = read_through_claim(
        &eight_line_file(),
        &[range(1, 4)],
        ReadThroughSelection::FileRow,
        DiffViewMode::Unified,
    );

    assert_eq!(Vec::<usize>::new(), actual);
}

#[test]
fn should_claim_from_a_symbols_own_start_to_the_next_symbols_start() {
    // `alpha` (lines 3-4) reads on through the blank line 5 that follows
    // it — the row between the two symbols, which neither of them covers.
    let actual = read_through_claim(
        &eight_line_file(),
        &[range(3, 4), range(6, 7)],
        ReadThroughSelection::Symbol(range(3, 4)),
        DiffViewMode::Unified,
    );

    assert_eq!(vec![3, 4, 5], actual);
}

#[test]
fn should_claim_to_the_end_of_the_body_for_the_last_symbol() {
    let actual = read_through_claim(
        &eight_line_file(),
        &[range(3, 4), range(6, 7)],
        ReadThroughSelection::Symbol(range(6, 7)),
        DiffViewMode::Unified,
    );

    assert_eq!(vec![6, 7, 8], actual);
}

#[test]
fn should_tile_the_whole_body_exactly_once_across_the_file_row_and_its_symbols() {
    // The property the partition exists for: a reviewer who walks the file
    // row and then each of its symbol rows pages every rendered row, and
    // pages none of them twice.
    let symbol_ranges = [range(3, 4), range(6, 7)];
    let content = eight_line_file();

    let mut walked: Vec<usize> = read_through_claim(
        &content,
        &symbol_ranges,
        ReadThroughSelection::FileRow,
        DiffViewMode::Unified,
    );
    for symbol_range in symbol_ranges {
        walked.extend(read_through_claim(
            &content,
            &symbol_ranges,
            ReadThroughSelection::Symbol(symbol_range),
            DiffViewMode::Unified,
        ));
    }

    assert_eq!(vec![0, 1, 2, 3, 4, 5, 6, 7, 8], walked);
}

#[test]
fn should_tile_the_whole_body_when_the_symbol_ranges_arrive_out_of_diff_order() {
    // A `Report`'s symbol order is not the diff's row order, so the claim
    // boundaries have to be sorted rather than taken as given.
    let symbol_ranges = [range(6, 7), range(3, 4)];
    let content = eight_line_file();

    let mut walked: Vec<usize> = read_through_claim(
        &content,
        &symbol_ranges,
        ReadThroughSelection::FileRow,
        DiffViewMode::Unified,
    );
    for symbol_range in symbol_ranges {
        walked.extend(read_through_claim(
            &content,
            &symbol_ranges,
            ReadThroughSelection::Symbol(symbol_range),
            DiffViewMode::Unified,
        ));
    }
    walked.sort_unstable();

    assert_eq!(vec![0, 1, 2, 3, 4, 5, 6, 7, 8], walked);
}

#[test]
fn should_give_two_symbols_resolving_to_one_start_the_same_claim() {
    // Nested ranges (a method inside the class its own range covers)
    // resolve to the same first row. Keying the claim on the start row
    // rather than on the symbol keeps the second one from claiming
    // nothing at all.
    let symbol_ranges = [range(3, 8), range(3, 4)];
    let content = eight_line_file();

    let outer = read_through_claim(
        &content,
        &symbol_ranges,
        ReadThroughSelection::Symbol(range(3, 8)),
        DiffViewMode::Unified,
    );
    let inner = read_through_claim(
        &content,
        &symbol_ranges,
        ReadThroughSelection::Symbol(range(3, 4)),
        DiffViewMode::Unified,
    );

    assert_eq!(
        (vec![3, 4, 5, 6, 7, 8], vec![3, 4, 5, 6, 7, 8]),
        (outer, inner)
    );
}

#[test]
fn should_claim_nothing_when_no_row_covers_the_selected_symbols_range() {
    let actual = read_through_claim(
        &eight_line_file(),
        &[range(3, 4)],
        ReadThroughSelection::Symbol(range(100, 200)),
        DiffViewMode::Unified,
    );

    assert_eq!(Vec::<usize>::new(), actual);
}

#[test]
fn should_claim_across_a_hunk_separator_when_no_symbol_starts_in_between() {
    // Rows: header0(0) line 1, body0(1) line 1, separator(2), header1(3)
    // line 5, body1(4) line 5. The separator belongs to no symbol range,
    // which is exactly why the claim has to be a contiguous span rather
    // than a set of covered rows.
    let content = DiffPaneContent::File(vec![
        attributed(0, hunk("@@ -1,1 +1,1 @@", Some((1, 1)), vec!["a"])),
        attributed(1, hunk("@@ -5,1 +5,1 @@", Some((5, 5)), vec!["e"])),
    ]);

    let actual = read_through_claim(
        &content,
        &[range(1, 1)],
        ReadThroughSelection::Symbol(range(1, 1)),
        DiffViewMode::Unified,
    );

    assert_eq!(vec![0, 1, 2, 3, 4], actual);
}

#[test]
fn should_claim_the_rows_a_symbol_actually_starts_on_in_split_view() {
    // `pair_hunk_lines` reorders a replace run's rows, so the same symbol
    // range starts at a different row in split view — the claim boundaries
    // have to be resolved in the mode the pane actually rendered in.
    let content = DiffPaneContent::File(vec![attributed(
        0,
        Hunk {
            header: "@@ -10,3 +10,3 @@".to_string(),
            new_range: Some((10, 12)),
            lines: vec![
                (DiffLineKind::Removed, "fn alpha(x: u32) {}"),
                (DiffLineKind::Removed, "fn beta(x: u32) {}"),
                (DiffLineKind::Removed, "fn gamma(x: u32) {}"),
                (DiffLineKind::Added, "fn alpha(x: u64) {}"),
                (DiffLineKind::Added, "fn beta(x: u64) {}"),
                (DiffLineKind::Added, "fn gamma(x: u64) {}"),
            ]
            .into_iter()
            .map(|(kind, content)| DiffLine {
                kind,
                content: content.to_string(),
            })
            .collect(),
        },
    )]);

    let unified = read_through_claim(
        &content,
        &[range(12, 12)],
        ReadThroughSelection::Symbol(range(12, 12)),
        DiffViewMode::Unified,
    );
    let split = read_through_claim(
        &content,
        &[range(12, 12)],
        ReadThroughSelection::Symbol(range(12, 12)),
        DiffViewMode::Split,
    );

    assert_eq!((vec![6], vec![3, 4, 5, 6]), (unified, split));
}

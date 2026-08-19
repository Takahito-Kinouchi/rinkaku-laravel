//! `TagsResolver::from_entries` — the cache-backed counterpart to `new`
//! (ADR 0079): builds the same index shape from pre-extracted
//! `IndexEntry` data instead of parsing file content.

use super::*;
use pretty_assertions::assert_eq;

fn entry(name: &str, signature: &str, container: Option<&str>, is_test: bool) -> IndexEntry {
    IndexEntry {
        name: name.to_string(),
        signature: signature.to_string(),
        container: container.map(str::to_string),
        is_test,
    }
}

#[test]
fn should_resolve_entry_when_built_from_a_single_file() {
    let entries = vec![(
        "src/lib.rs".to_string(),
        vec![entry("helper", "fn helper(x: i32) -> i32", None, false)],
    )];

    let resolver = TagsResolver::from_entries(entries, false);

    let expected = vec![ResolvedSymbol {
        signature: "fn helper(x: i32) -> i32".to_string(),
        path: "src/lib.rs".to_string(),
        container: None,
    }];
    let actual = resolver.resolve("helper");

    assert_eq!(expected, actual);
}

#[test]
fn should_return_empty_vec_when_name_has_no_entry() {
    let entries = vec![(
        "src/lib.rs".to_string(),
        vec![entry("helper", "fn helper(x: i32) -> i32", None, false)],
    )];

    let resolver = TagsResolver::from_entries(entries, false);

    let expected: Vec<ResolvedSymbol> = Vec::new();
    let actual = resolver.resolve("missing");

    assert_eq!(expected, actual);
}

#[test]
fn should_exclude_test_entries_by_default() {
    let entries = vec![(
        "src/lib.rs".to_string(),
        vec![entry(
            "should_add_two_numbers",
            "fn should_add_two_numbers()",
            None,
            true,
        )],
    )];

    let resolver = TagsResolver::from_entries(entries, false);

    let expected: Vec<ResolvedSymbol> = Vec::new();
    let actual = resolver.resolve("should_add_two_numbers");

    assert_eq!(expected, actual);
}

#[test]
fn should_include_test_entries_when_include_tests_is_true() {
    let entries = vec![(
        "src/lib.rs".to_string(),
        vec![entry(
            "should_add_two_numbers",
            "fn should_add_two_numbers()",
            None,
            true,
        )],
    )];

    let resolver = TagsResolver::from_entries(entries, true);

    let expected = vec![ResolvedSymbol {
        signature: "fn should_add_two_numbers()".to_string(),
        path: "src/lib.rs".to_string(),
        container: None,
    }];
    let actual = resolver.resolve("should_add_two_numbers");

    assert_eq!(expected, actual);
}

// Regression for `resolve_dependencies`'s stable-sort tie-break: entries
// sharing a name must keep the exact input order `from_entries` was
// called with, the same guarantee `TagsResolver::new`'s
// `parallel_determinism` tests pin for the parse-then-insert path.
#[test]
fn should_preserve_input_order_for_same_named_entries_across_files() {
    let entries = vec![
        (
            "src/a.rs".to_string(),
            vec![entry("helper", "fn helper() -> i32", None, false)],
        ),
        (
            "src/b.rs".to_string(),
            vec![entry("helper", "fn helper() -> i32", None, false)],
        ),
        (
            "src/c.rs".to_string(),
            vec![entry("helper", "fn helper() -> i32", None, false)],
        ),
    ];

    let resolver = TagsResolver::from_entries(entries, false);

    let expected_paths = vec![
        "src/a.rs".to_string(),
        "src/b.rs".to_string(),
        "src/c.rs".to_string(),
    ];
    let actual_paths: Vec<String> = resolver
        .resolve("helper")
        .into_iter()
        .map(|resolved| resolved.path)
        .collect();

    assert_eq!(expected_paths, actual_paths);
}

// A single file's multiple entries must all be indexed, each carrying its
// own `container` — mirrors what `extract_all_symbols` would have produced
// for a file with more than one definition.
#[test]
fn should_index_every_entry_in_a_file_with_multiple_definitions() {
    let entries = vec![(
        "src/point.rs".to_string(),
        vec![
            entry("Point", "struct Point { x: i32 }", None, false),
            entry("new", "fn new(x: i32) -> Point", Some("impl Point"), false),
        ],
    )];

    let resolver = TagsResolver::from_entries(entries, false);

    assert_eq!(
        vec![ResolvedSymbol {
            signature: "struct Point { x: i32 }".to_string(),
            path: "src/point.rs".to_string(),
            container: None,
        }],
        resolver.resolve("Point")
    );
    assert_eq!(
        vec![ResolvedSymbol {
            signature: "fn new(x: i32) -> Point".to_string(),
            path: "src/point.rs".to_string(),
            container: Some("impl Point".to_string()),
        }],
        resolver.resolve("new")
    );
}

#[test]
fn should_build_empty_index_when_no_entries_given() {
    let entries: Vec<(String, Vec<IndexEntry>)> = Vec::new();

    let resolver = TagsResolver::from_entries(entries, true);

    let expected: Vec<ResolvedSymbol> = Vec::new();
    assert_eq!(expected, resolver.resolve("anything"));
}

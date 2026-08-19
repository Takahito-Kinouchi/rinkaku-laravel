//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on Svelte components: the
//! shared `mask_non_script` preprocessing keeps only the `<script>`
//! block(s) at their original file lines (both the instance script and
//! `<script context="module">`), and markup-only edits surface no
//! symbols.

use super::*;
use crate::language::svelte::SvelteSupport;
use pretty_assertions::assert_eq;

fn component_source() -> &'static str {
    "<script context=\"module\">\nexport function preload(id: number): Promise<Data> {\n  return fetchData(id);\n}\n</script>\n\n<script lang=\"ts\">\nexport let count = 0;\n\nfunction bump(): void {\n  count += 1;\n}\n</script>\n\n<button on:click={bump}>{count}</button>\n"
}

#[test]
fn should_extract_symbols_from_both_script_blocks_at_original_lines() {
    let lang = SvelteSupport;

    let symbols = extract_all_symbols(component_source(), &lang);

    let shapes: Vec<(String, String, LineRange)> = symbols
        .iter()
        .map(|s| (s.name.clone(), s.signature.clone(), s.range))
        .collect();
    let expected = vec![
        (
            "preload".to_string(),
            "function preload(id: number): Promise<Data>".to_string(),
            LineRange { start: 2, end: 4 },
        ),
        (
            "bump".to_string(),
            "function bump(): void".to_string(),
            LineRange { start: 10, end: 12 },
        ),
    ];
    assert_eq!(expected, shapes);
}

#[test]
fn should_extract_changed_symbol_when_script_body_line_changed() {
    let lang = SvelteSupport;
    // Line 11 is `count += 1;`, inside `bump`'s body.
    let changed_ranges = vec![LineRange { start: 11, end: 11 }];

    let actual = extract_changed_symbols(component_source(), &lang, &changed_ranges);

    assert_eq!(1, actual.len());
    assert_eq!("bump", actual[0].name);
    assert_eq!("function bump(): void", actual[0].signature);
}

#[test]
fn should_extract_no_symbols_when_only_markup_lines_changed() {
    let lang = SvelteSupport;
    // Line 15 is the `<button ...>` markup line — masked away, so no
    // definition can contain it.
    let changed_ranges = vec![LineRange { start: 15, end: 15 }];

    let actual = extract_changed_symbols(component_source(), &lang, &changed_ranges);

    assert_eq!(Vec::<ExtractedSymbol>::new(), actual);
}

#[test]
fn should_capture_type_references_from_the_module_script() {
    let lang = SvelteSupport;

    let symbols = extract_all_symbols(component_source(), &lang);

    let preload = symbols
        .iter()
        .find(|s| s.name == "preload")
        .expect("preload extracted");
    assert_eq!(
        vec![
            "Data".to_string(),
            "Promise".to_string(),
            "fetchData".to_string(),
        ],
        preload.referenced_names
    );
}

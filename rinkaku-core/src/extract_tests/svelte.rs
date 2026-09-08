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
        // ADR 0097: an `export let` in a Svelte instance script is a prop
        // — part of the surface a parent binds to — so it is its own
        // symbol rather than a line the report can only count.
        (
            "count".to_string(),
            "export let count = 0;".to_string(),
            LineRange { start: 8, end: 8 },
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

// ADR 0097: Svelte declares a component's props two ways, and both
// extract as named symbols rather than reducing to a changed-line count.

#[test]
fn should_extract_props_rune_destructuring_as_a_component_api_symbol() {
    let source = "<script lang=\"ts\">\nlet { start = 0, max = 10 }: { start?: number; max?: number } = $props();\n\nlet count = $state(start);\nlet doubled = $derived(count * 2);\n\nfunction bump(): void {\n  count += 1;\n}\n</script>\n\n<button onclick={bump}>{doubled}</button>\n";
    let lang = SvelteSupport;

    let symbols = extract_all_symbols(source, &lang);

    // `$state`/`$derived` are internal component state, not surface a
    // parent binds to, so only the `$props()` destructuring is captured.
    let shapes: Vec<(String, SymbolKind)> =
        symbols.iter().map(|s| (s.name.clone(), s.kind)).collect();
    let expected = vec![
        ("props".to_string(), SymbolKind::ComponentApi),
        ("bump".to_string(), SymbolKind::Function),
    ];
    assert_eq!(expected, shapes);
}

#[test]
fn should_report_the_prop_symbol_when_only_the_export_let_line_changed() {
    // The regression ADR 0097 exists for, in its Svelte 4 shape: line 8
    // is `export let count = 0;`, which used to report as nothing but a
    // changed-line count.
    let lang = SvelteSupport;
    let changed_ranges = vec![LineRange { start: 8, end: 8 }];

    let actual = extract_changed_symbols(component_source(), &lang, &changed_ranges);

    assert_eq!(1, actual.len());
    assert_eq!("count", actual[0].name);
    assert_eq!(SymbolKind::ComponentApi, actual[0].kind);
    assert_eq!("export let count = 0;", actual[0].signature);
}

#[test]
fn should_prefer_the_function_when_an_exported_prop_is_an_arrow_function() {
    // `export let onPick = () => {}` matches both the prop pattern (on
    // the `export_statement`) and the arrow-function pattern (on the
    // declarator nested inside it). The narrowest-enclosing-definition
    // rule resolves it to the function, which is why the prop pattern
    // needs no negation to avoid double-reporting.
    let source = "<script lang=\"ts\">\nexport let onPick: (id: string) => void = () => {\n  return;\n};\n</script>\n";
    let lang = SvelteSupport;
    // Line 3 is the arrow function's own body.
    let changed_ranges = vec![LineRange { start: 3, end: 3 }];

    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(1, actual.len());
    assert_eq!("onPick", actual[0].name);
    assert_eq!(SymbolKind::Function, actual[0].kind);
}

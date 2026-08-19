//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on Vue single-file components:
//! the `source_for_parse` masking keeps only the `<script>` block(s) for
//! the TypeScript grammar while every extracted range and signature still
//! refers to the original file's lines, and template/style edits alone
//! surface no symbols.

use super::*;
use crate::language::vue::VueSupport;
use pretty_assertions::assert_eq;

fn sfc_source() -> &'static str {
    "<template>\n  <button @click=\"increment\">{{ count }}</button>\n</template>\n\n<script setup lang=\"ts\">\nimport { ref } from 'vue';\n\nconst count = ref(0);\n\nfunction increment(): void {\n  count.value += 1;\n}\n\nfunction useCounter(start: number): Counter {\n  return new Counter(start);\n}\n</script>\n\n<style scoped>\nbutton { color: red; }\n</style>\n"
}

#[test]
fn should_extract_script_symbols_at_their_original_file_lines() {
    let lang = VueSupport;

    let symbols = extract_all_symbols(sfc_source(), &lang);

    let shapes: Vec<(String, String, LineRange)> = symbols
        .iter()
        .map(|s| (s.name.clone(), s.signature.clone(), s.range))
        .collect();
    let expected = vec![
        (
            "increment".to_string(),
            "function increment(): void".to_string(),
            LineRange { start: 10, end: 12 },
        ),
        (
            "useCounter".to_string(),
            "function useCounter(start: number): Counter".to_string(),
            LineRange { start: 14, end: 16 },
        ),
    ];
    assert_eq!(expected, shapes);
}

#[test]
fn should_extract_changed_symbol_when_script_body_line_changed() {
    let lang = VueSupport;
    // Line 11 is `count.value += 1;`, inside `increment`'s body.
    let changed_ranges = vec![LineRange { start: 11, end: 11 }];

    let actual = extract_changed_symbols(sfc_source(), &lang, &changed_ranges);

    assert_eq!(1, actual.len());
    assert_eq!("increment", actual[0].name);
    assert_eq!("function increment(): void", actual[0].signature);
}

#[test]
fn should_extract_no_symbols_when_only_template_lines_changed() {
    let lang = VueSupport;
    // Line 2 is the `<button ...>` template line — masked away, so no
    // definition can contain it.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let actual = extract_changed_symbols(sfc_source(), &lang, &changed_ranges);

    assert_eq!(Vec::<ExtractedSymbol>::new(), actual);
}

#[test]
fn should_capture_type_references_from_the_script_block() {
    let lang = VueSupport;

    let symbols = extract_all_symbols(sfc_source(), &lang);

    let use_counter = symbols
        .iter()
        .find(|s| s.name == "useCounter")
        .expect("useCounter extracted");
    assert_eq!(vec!["Counter".to_string()], use_counter.referenced_names);
}

#[test]
fn should_extract_symbols_from_both_script_blocks_when_sfc_has_two() {
    let source = "<script lang=\"ts\">\nexport function setup(): void {}\n</script>\n<script setup lang=\"ts\">\nconst handler = () => 1;\n</script>\n";
    let lang = VueSupport;

    let symbols = extract_all_symbols(source, &lang);

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(vec!["setup", "handler"], names);
}

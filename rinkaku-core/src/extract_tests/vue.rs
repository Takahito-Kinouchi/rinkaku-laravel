//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on Vue single-file components:
//! the `source_for_parse` masking keeps only the `<script>` block(s) for
//! the TypeScript grammar while every extracted range and signature still
//! refers to the original file's lines, and template/style edits alone
//! surface no symbols.

use super::*;
use crate::language::vue::VueSupport;
use pretty_assertions::assert_eq;

/// Vue and Svelte read the path: the component symbol (ADR 0098) is
/// named after the file. These shims name a real component file, in
/// place of the parent module's own path-less ones, so every test below
/// reads the same as it did before the path existed.
const COMPONENT_PATH: &str = "src/components/Counter.vue";

fn extract_all_symbols(source: &str, lang: &dyn LanguageSupport) -> Vec<ExtractedSymbol> {
    crate::extract::extract_all_symbols(COMPONENT_PATH, source, lang)
}

fn extract_changed_symbols(
    source: &str,
    lang: &dyn LanguageSupport,
    changed_ranges: &[LineRange],
) -> Vec<ExtractedSymbol> {
    crate::extract::extract_changed_symbols(COMPONENT_PATH, source, lang, changed_ranges)
}

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
        // ADR 0098: the SFC itself is a symbol, named after the file and
        // spanning it, so a parent that renders `<Counter />` has
        // something to point at.
        (
            "Counter".to_string(),
            "<Counter />".to_string(),
            LineRange { start: 1, end: 21 },
        ),
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
fn should_report_only_the_component_when_template_lines_changed() {
    // ADR 0098 narrowed ADR 0075's "template edits surface nothing": the
    // template is still not parsed, but the component it belongs to is
    // now a symbol spanning the file, so a template edit names the
    // component instead of reporting a bare line count.
    let lang = VueSupport;
    // Line 2 is the `<button ...>` template line.
    let changed_ranges = vec![LineRange { start: 2, end: 2 }];

    let actual = extract_changed_symbols(sfc_source(), &lang, &changed_ranges);

    assert_eq!(1, actual.len());
    assert_eq!("Counter", actual[0].name);
    assert_eq!(SymbolKind::Component, actual[0].kind);
    assert_eq!(LineRange { start: 1, end: 21 }, actual[0].range);
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
    assert_eq!(vec!["Counter", "setup", "handler"], names);
}

// ADR 0097: a component's declared inputs and outputs are surface a
// parent binds to, so they extract as named symbols rather than reducing
// to a changed-line count. Both authoring styles are covered — `<script
// setup>`'s `define*` macros here, the Options API's `props:`/`emits:`
// options below.

fn script_setup_source() -> &'static str {
    "<template>\n  <button @click=\"go\">{{ label }}</button>\n</template>\n\n<script setup lang=\"ts\">\nconst props = defineProps<{ start: number }>();\nconst emit = defineEmits<{ (e: 'changed', value: number): void }>();\n\nfunction go(): void {\n  emit('changed', props.start);\n}\n</script>\n"
}

#[test]
fn should_extract_define_props_and_define_emits_as_component_api_symbols() {
    let lang = VueSupport;

    let symbols = extract_all_symbols(script_setup_source(), &lang);

    let shapes: Vec<(String, SymbolKind, String)> = symbols
        .iter()
        .map(|s| (s.name.clone(), s.kind, s.signature.clone()))
        .collect();
    let expected = vec![
        (
            "Counter".to_string(),
            SymbolKind::Component,
            "<Counter />".to_string(),
        ),
        (
            "props".to_string(),
            SymbolKind::ComponentApi,
            "defineProps<{ start: number }>()".to_string(),
        ),
        (
            "emits".to_string(),
            SymbolKind::ComponentApi,
            "defineEmits<{ (e: 'changed', value: number): void }>()".to_string(),
        ),
        (
            "go".to_string(),
            SymbolKind::Function,
            "function go(): void".to_string(),
        ),
    ];
    assert_eq!(expected, shapes);
}

#[test]
fn should_report_the_props_symbol_when_only_the_props_declaration_changed() {
    // The regression ADR 0097 exists for: before it, this edit reported
    // nothing but "1 changed line outside any definition", even though a
    // props change is the most contract-breaking edit an SFC can carry.
    let lang = VueSupport;
    // Line 6 is the `defineProps<...>()` line.
    let changed_ranges = vec![LineRange { start: 6, end: 6 }];

    let actual = extract_changed_symbols(script_setup_source(), &lang, &changed_ranges);

    assert_eq!(1, actual.len());
    assert_eq!("props", actual[0].name);
    assert_eq!(SymbolKind::ComponentApi, actual[0].kind);
    assert_eq!("defineProps<{ start: number }>()", actual[0].signature);
}

#[test]
fn should_extract_options_api_props_and_emits_options_as_component_api_symbols() {
    let source = "<template><div /></template>\n\n<script lang=\"ts\">\nexport default {\n  props: { title: { type: String, required: true } },\n  emits: ['close'],\n};\n</script>\n";
    let lang = VueSupport;

    let symbols = extract_all_symbols(source, &lang);

    let shapes: Vec<(String, SymbolKind)> = symbols.iter().map(|s| (s.name.clone(), s.kind)).collect();
    let expected = vec![
        ("Counter".to_string(), SymbolKind::Component),
        ("props".to_string(), SymbolKind::ComponentApi),
        ("emits".to_string(), SymbolKind::ComponentApi),
    ];
    assert_eq!(expected, shapes);
}

#[test]
fn should_extract_define_model_and_define_expose_as_component_api_symbols() {
    // The macro name's `define` prefix is dropped and its first letter
    // lowered, so every macro lands on the noun it declares.
    let source = "<script setup lang=\"ts\">\nconst value = defineModel<string>();\ndefineExpose({ focus });\n</script>\n";
    let lang = VueSupport;

    let symbols = extract_all_symbols(source, &lang);

    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(vec!["Counter", "model", "expose"], names);
}

// ADR 0098: the component is a symbol of its own, named after the file
// and carrying the children its markup renders — the only place those
// couplings can come from, since the markup is masked before any query
// runs and a Nuxt auto-imported child is never named in the script.

#[test]
fn should_carry_markup_children_as_the_components_own_references() {
    let source = "<template>\n  <BaseButton @click=\"go\" />\n  <div />\n</template>\n\n<script setup lang=\"ts\">\nfunction go(): void {}\n</script>\n";
    let lang = VueSupport;

    let symbols = extract_all_symbols(source, &lang);

    let component = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Component)
        .expect("component symbol extracted");
    assert_eq!("Counter", component.name);
    assert_eq!(
        vec!["BaseButton".to_string(), "base-button".to_string()],
        component.referenced_names
    );
}

#[test]
fn should_not_carry_the_scripts_own_references_on_the_component() {
    // The captured node is the whole file, so the reference query would
    // return every call and type in it — which would wire the component
    // to everything it happens to contain instead of to what it renders.
    let source = "<script setup lang=\"ts\">\nimport { helper } from './helper';\nfunction go(): Widget {\n  return helper();\n}\n</script>\n";
    let lang = VueSupport;

    let symbols = extract_all_symbols(source, &lang);

    let component = symbols
        .iter()
        .find(|s| s.kind == SymbolKind::Component)
        .expect("component symbol extracted");
    assert_eq!(Vec::<String>::new(), component.referenced_names);
}

#[test]
fn should_suppress_the_component_when_a_definition_inside_it_changed() {
    // The component spans the file, so it encloses every definition in
    // it; the narrowest-enclosing rule is what keeps a report from
    // carrying both the function that changed and the component around
    // it. It surfaces only when nothing narrower did (the template-only
    // case above).
    let lang = VueSupport;
    // Line 11 is `count.value += 1;`, inside `increment`'s body.
    let changed_ranges = vec![LineRange { start: 11, end: 11 }];

    let actual = extract_changed_symbols(sfc_source(), &lang, &changed_ranges);

    assert_eq!(1, actual.len());
    assert_eq!("increment", actual[0].name);
}

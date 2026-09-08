//! Vue single-file-component (`.vue`) `LanguageSupport` implementation.
//!
//! An SFC is not one language: `<template>` is HTML-flavored markup,
//! `<style>` is CSS, and only the `<script>`/`<script setup>` block(s)
//! carry the symbols rinkaku extracts. Rather than pulling in a dedicated
//! Vue grammar (unmaintained on crates.io, and it would still delegate
//! script parsing to a JS/TS grammar), this impl reuses the TypeScript
//! grammar and masks everything outside the script blocks to whitespace
//! first — see [`mask_non_script`] and
//! [`LanguageSupport::source_for_parse`]. The TypeScript grammar also
//! parses plain-JS script blocks (TS is a syntactic superset for the
//! constructs the definition query captures), so `lang="ts"` and untyped
//! `<script>` blocks route through the same impl.

use super::LanguageSupport;
use super::typescript;

/// TypeScript's definition query plus the SFC's own component-API
/// declarations (ADR 0097). A `.vue` file's public surface is not only its
/// functions: `defineProps`/`defineEmits` — and, in the Options API, the
/// `props:`/`emits:` options — are what a *parent* component is coupled
/// to, so a change to them is a contract change in exactly the sense ADR
/// 0014 classifies, and it has to reach the report as a named symbol
/// rather than as a bare changed-line count.
///
/// The five `define*` macros are matched by name because they are macros,
/// not imports: `<script setup>` makes them available without a binding,
/// so there is no declaration node to anchor on and the callee identifier
/// is the only thing that identifies them.
///
/// The Options-API pattern matches a `props:`/`emits:` pair anywhere in
/// the script rather than only inside the default export. Narrowing it
/// would need two patterns (`export default {...}` and `export default
/// defineComponent({...})` nest the object differently) and would still
/// miss an options object assigned to a local first; a same-named key in
/// an unrelated object literal inside a `.vue` script is rare enough, and
/// wrong in a benign way — it reports one extra named symbol, never a
/// missing one.
const DEFINITION_QUERY: &str = "\
[
  (function_declaration) @definition.function
  (method_definition) @definition.function
  (abstract_method_signature) @definition.function
  (variable_declarator value: (arrow_function)) @definition.function
  (class_declaration) @definition.class
  (abstract_class_declaration) @definition.class
  (interface_declaration) @definition.interface
  (type_alias_declaration) @definition.type_alias
  (enum_declaration) @definition.enum
  (call_expression
    function: (identifier) @_macro
    (#match? @_macro \"^(defineProps|defineEmits|defineModel|defineSlots|defineExpose)$\")) @definition.component_api
  (pair key: (property_identifier) @_option
    (#match? @_option \"^(props|emits)$\")) @definition.component_api
  (program) @definition.component
] @definition";

pub struct VueSupport;

impl LanguageSupport for VueSupport {
    fn name(&self) -> &'static str {
        "vue"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }

    fn definition_query(&self) -> &str {
        DEFINITION_QUERY
    }

    fn reference_query(&self) -> &str {
        typescript::REFERENCE_QUERY
    }

    // `index_prefilter_patterns` (ADR 0080) is not overridden: this
    // query is `TypeScriptSupport`'s plus the component-API patterns
    // above, and that impl's doc comment on this same method explains why
    // the bare-name default is kept (arrow-function `variable_declarator`s
    // and `method_definition`s have no single fixed keyword to anchor a
    // pattern to). The added patterns do not change that answer: one
    // node lacking a provable pattern already forces the default.

    /// Vitest/Jest's conventions, mirroring the TypeScript impl's:
    /// `.test.vue`/`.spec.vue` suffixes or a `__tests__/` directory
    /// anywhere in the path.
    fn is_test_path(&self, path: &str) -> bool {
        let file_name = path.rsplit('/').next().unwrap_or(path);
        [".test.vue", ".spec.vue"]
            .iter()
            .any(|suffix| file_name.ends_with(suffix))
            || path.split('/').any(|segment| segment == "__tests__")
    }

    fn component_markup_references(&self, source: &str) -> Option<Vec<String>> {
        Some(component_tags(source))
    }

    fn source_for_parse<'a>(&self, source: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Owned(mask_non_script(source))
    }
}

/// Replaces every byte outside the SFC's `<script ...>...</script>`
/// block(s) with a space (newlines kept), so the TypeScript grammar sees
/// only the script content while every kept byte stays at its original
/// line and byte offset — the diff's changed-line ranges, extracted
/// signature text, and `ExtractedSymbol::range` all keep referring to
/// positions in the real file.
///
/// Multi-byte UTF-8 outside the script blocks becomes several spaces
/// (byte-wise replacement), which is still valid UTF-8 and preserves
/// offsets exactly. Tags are matched lowercase, per the SFC spec's own
/// block naming; a `<script>` with no closing tag masks nothing after it
/// is opened (the rest of the file is treated as script), matching how
/// browsers and the SFC compiler error-recover. Multiple script blocks
/// (`<script>` + `<script setup>`) are all kept — the SFC spec allows
/// exactly that pairing, and the TypeScript grammar parses their
/// concatenation (statements separated by blank lines) fine.
pub(crate) fn mask_non_script(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked: Vec<u8> = bytes
        .iter()
        .map(|&b| if b == b'\n' { b'\n' } else { b' ' })
        .collect();

    for region in script_regions(source) {
        masked[region.clone()].copy_from_slice(&bytes[region]);
    }

    String::from_utf8(masked).expect(
        "masking replaces whole bytes with ASCII spaces and copies back byte ranges that lie \
         on ASCII tag boundaries, so the result stays valid UTF-8",
    )
}

/// The byte ranges of `source`'s `<script ...>` block *contents* — what
/// [`mask_non_script`] keeps and [`component_tags`] must skip. Extracted
/// so those two can never disagree about where the script is.
fn script_regions(source: &str) -> Vec<std::ops::Range<usize>> {
    let mut regions = Vec::new();
    let mut cursor = 0;
    while let Some(open_offset) = source[cursor..].find("<script") {
        let open = cursor + open_offset;
        let after_tag_name = open + "<script".len();
        // Require a real tag boundary so an unrelated `<scripting>`-like
        // token in masked-away content can't open a block.
        let is_tag_boundary = source[after_tag_name..]
            .chars()
            .next()
            .is_some_and(|c| c == '>' || c.is_whitespace());
        if !is_tag_boundary {
            cursor = after_tag_name;
            continue;
        }
        let Some(tag_end_offset) = source[after_tag_name..].find('>') else {
            break;
        };
        let content_start = after_tag_name + tag_end_offset + 1;
        let content_end = source[content_start..]
            .find("</script")
            .map(|offset| content_start + offset)
            .unwrap_or(source.len());
        regions.push(content_start..content_end);
        cursor = content_end;
    }
    regions
}

/// Every child-component tag rendered by the markup outside `source`'s
/// `<script>` block(s) (ADR 0098), each reported in the spelling the
/// markup used *and* in both naming conventions, so `<BaseButton />` and
/// `<base-button />` alike resolve whether the project names its files
/// `BaseButton.vue` or `base-button.vue`. Which spelling the markup uses
/// says nothing about which the filesystem uses — both frameworks accept
/// either — so neither direction can be assumed.
///
/// Scanned as text rather than parsed: the markup is HTML-flavored but
/// not HTML (Vue's `v-if`/`{{ }}`, Svelte's `{#each}`), and nothing here
/// needs a tree — only which tag names appear. [`script_regions`] is
/// shared with [`mask_non_script`] so the two can never disagree about
/// where the script is.
///
/// A tag counts as a component when it is PascalCase or contains a
/// hyphen, which is exactly the convention both frameworks require to
/// tell a component from a plain element. Framework built-ins
/// (`<Transition>`, `<KeepAlive>`, `<svelte:component>`, ...) are
/// excluded: they resolve to nothing in any repository, so keeping them
/// would only add unmatched names.
pub(crate) fn component_tags(source: &str) -> Vec<String> {
    let script = script_regions(source);
    let in_script = |offset: usize| script.iter().any(|range| range.contains(&offset));

    let mut found: Vec<String> = Vec::new();
    let bytes = source.as_bytes();
    for (offset, _) in source.match_indices('<') {
        if in_script(offset) {
            continue;
        }
        let name_start = offset + 1;
        let name_end = bytes[name_start..]
            .iter()
            .position(|byte| !is_tag_name_byte(*byte))
            .map(|len| name_start + len)
            .unwrap_or(source.len());
        let tag = &source[name_start..name_end];
        if !is_component_tag(tag) {
            continue;
        }
        for name in [tag.to_string(), pascal_case(tag), kebab_case(tag)] {
            if !found.contains(&name) {
                found.push(name);
            }
        }
    }
    found
}

fn is_tag_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.'
}

/// Whether `tag` names a component rather than a plain element: Vue and
/// Svelte both require a component to be PascalCase or hyphenated, which
/// is what keeps `<div>`/`<button>` out without an element allowlist.
fn is_component_tag(tag: &str) -> bool {
    const FRAMEWORK_BUILTINS: &[&str] = &[
        "Transition",
        "TransitionGroup",
        "KeepAlive",
        "Teleport",
        "Suspense",
        "Component",
        "Slot",
        "Template",
    ];

    let mut chars = tag.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() && !tag.contains('-') {
        return false;
    }
    // `<svelte:component>`, `<svelte:head>`, ... are namespaced special
    // elements, not components; the `.`-separated `<Foo.Bar>` form is a
    // component reached through a namespace, so only `:` is excluded.
    if tag.contains(':') || tag.starts_with("svelte") {
        return false;
    }
    !FRAMEWORK_BUILTINS.contains(&pascal_case(tag).as_str())
}

/// `BaseButton` / `base-button` / `base_button` → `base-button`.
fn kebab_case(tag: &str) -> String {
    let mut kebab = String::with_capacity(tag.len() + 4);
    for (index, character) in tag.chars().enumerate() {
        if character == '_' {
            kebab.push('-');
        } else if character.is_ascii_uppercase() {
            if index > 0 && !kebab.ends_with('-') {
                kebab.push('-');
            }
            kebab.extend(character.to_lowercase());
        } else {
            kebab.push(character);
        }
    }
    kebab
}

/// `base-button` / `BaseButton` / `base_button` → `BaseButton`.
fn pascal_case(tag: &str) -> String {
    tag.split(['-', '_'])
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[test]
    fn should_keep_script_content_at_its_original_lines_when_masking() {
        let source = "<template>\n  <div>{{ msg }}</div>\n</template>\n\n<script setup lang=\"ts\">\nfunction greet(name: string): string {\n  return name;\n}\n</script>\n";

        let actual = mask_non_script(source);

        // Byte-length- and line-preserving: the script body's lines (the
        // sixth through eighth) come through verbatim at their original
        // line numbers, every other line is blanked to same-width spaces.
        assert_eq!(source.len(), actual.len());
        let expected_lines: Vec<String> = source
            .lines()
            .enumerate()
            .map(|(index, line)| {
                if (5..=7).contains(&index) {
                    line.to_string()
                } else {
                    " ".repeat(line.len())
                }
            })
            .collect();
        assert_eq!(expected_lines, actual.lines().collect::<Vec<_>>());
    }

    #[test]
    fn should_keep_both_blocks_when_sfc_has_script_and_script_setup() {
        let source = "<script>\nconst a = () => 1;\n</script>\n<script setup>\nconst b = () => 2;\n</script>\n";

        let actual = mask_non_script(source);

        assert!(actual.contains("const a = () => 1;"));
        assert!(actual.contains("const b = () => 2;"));
        assert!(!actual.contains("<script"));
    }

    #[test]
    fn should_mask_everything_when_sfc_has_no_script_block() {
        let source = "<template>\n  <div />\n</template>\n";

        let actual = mask_non_script(source);

        assert_eq!("          \n         \n           \n", actual);
    }

    #[test]
    fn should_find_component_tags_in_both_spellings_when_markup_renders_children() {
        let source = "<template>\n  <BaseButton />\n  <base-button />\n  <div class=\"plain\" />\n  <button>go</button>\n</template>\n\n<script setup lang=\"ts\">\nconst n = 1;\n</script>\n";

        let actual = component_tags(source);

        // Each tag is reported as written and as PascalCase, so the same
        // component resolves whether the project names its files
        // `BaseButton.vue` or `base-button.vue`. Plain elements — no
        // capital, no hyphen — are not components in either framework.
        assert_eq!(
            vec!["BaseButton".to_string(), "base-button".to_string()],
            actual
        );
    }

    #[test]
    fn should_report_a_pascal_case_tag_in_kebab_case_too() {
        // The spelling the markup uses says nothing about the filename's,
        // so a PascalCase tag has to reach a `base-button.vue` component
        // as well — the mirror of the kebab case above.
        let source = "<template>\n  <BaseButton />\n</template>\n";

        let actual = component_tags(source);

        assert_eq!(
            vec!["BaseButton".to_string(), "base-button".to_string()],
            actual
        );
    }

    #[test]
    fn should_skip_framework_builtins_and_namespaced_elements() {
        let source = "<template>\n  <Transition><KeepAlive><Teleport to=\"#x\" /></KeepAlive></Transition>\n  <svelte:component this={C} />\n  <RealChild />\n</template>\n";

        let actual = component_tags(source);

        assert_eq!(
            vec!["RealChild".to_string(), "real-child".to_string()],
            actual
        );
    }

    #[test]
    fn should_ignore_tags_inside_the_script_block() {
        // The script is the one region `mask_non_script` keeps, so a
        // generic like `Array<Item>` there must not read as markup.
        let source = "<script setup lang=\"ts\">\nconst items: Array<Item> = [];\nconst node = h(\"div\");\n</script>\n\n<template>\n  <RealChild />\n</template>\n";

        let actual = component_tags(source);

        assert_eq!(
            vec!["RealChild".to_string(), "real-child".to_string()],
            actual
        );
    }

    #[test]
    fn should_return_no_tags_when_markup_renders_only_plain_elements() {
        let source = "<template>\n  <div><span>hi</span></div>\n</template>\n";

        let actual = component_tags(source);

        assert_eq!(Vec::<String>::new(), actual);
    }

    #[rstest]
    #[case::should_treat_tests_dir_as_test_path("src/__tests__/Button.vue", true)]
    #[case::should_treat_spec_suffix_as_test_path("src/Button.spec.vue", true)]
    #[case::should_treat_test_suffix_as_test_path("src/Button.test.vue", true)]
    #[case::should_not_treat_component_as_test_path("src/components/Button.vue", false)]
    fn is_test_path_cases(#[case] path: &str, #[case] expected: bool) {
        let support = VueSupport;

        let actual = support.is_test_path(path);

        assert_eq!(expected, actual);
    }
}

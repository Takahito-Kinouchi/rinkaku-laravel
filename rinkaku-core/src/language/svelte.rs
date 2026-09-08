//! Svelte component (`.svelte`) `LanguageSupport` implementation.
//!
//! A Svelte component has the same shape ADR 0075 solved for Vue: markup
//! plus `<script>` block(s) (`<script>`, `<script lang="ts">`, and the
//! module-level `<script context="module">`/`<script module>` variants),
//! where only the script carries the symbols rinkaku extracts. It
//! therefore reuses [`super::vue::mask_non_script`] — everything outside
//! the script blocks is masked to whitespace, line- and offset-preserving
//! — and the TypeScript grammar/queries, exactly as `VueSupport` does.
//! Markup-level `{expression}` bindings are masked away with the rest of
//! the template, the same v1 tradeoff Vue's `<template>` gets.

use super::LanguageSupport;
use super::typescript;
use super::vue;

/// TypeScript's definition query plus Svelte's own prop declarations
/// (ADR 0097), the counterpart to `vue::DEFINITION_QUERY`'s `define*`
/// macros: a component's props are what a parent is coupled to, so a
/// change to them has to reach the report as a named symbol rather than
/// as a bare changed-line count.
///
/// Svelte declares them two ways, and both are captured:
///
/// - Svelte 4's `export let items: string[] = []`. The captured node is
///   the whole `export_statement`, not the declarator inside it, so that
///   an `export let f = () => {}` — a prop whose value happens to be a
///   function — still reports as the function it is: the declarator is a
///   descendant of the export statement, and `extract`'s narrowest-
///   enclosing-definition rule therefore prefers it. That is also why no
///   negation is needed to keep the two patterns from double-reporting
///   one declaration.
/// - Svelte 5's `let { start, max } = $props()`. Matched on the `$props`
///   callee for the same reason Vue matches `defineProps`: it is a
///   compiler rune with no declaration to anchor on. `$state`/`$derived`
///   are deliberately not captured — they are internal component state,
///   not surface a parent can bind to.
///
/// `export const`/`export function` are left to the shared TypeScript
/// patterns: those are a component's module exports, already reported as
/// the functions and values they are.
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
  (export_statement declaration: (lexical_declaration)) @definition.component_api
  (variable_declarator
    value: (call_expression function: (identifier) @_rune
      (#eq? @_rune \"$props\"))) @definition.component_api
  (program) @definition.component
] @definition";

pub struct SvelteSupport;

impl LanguageSupport for SvelteSupport {
    fn name(&self) -> &'static str {
        "svelte"
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

    // `index_prefilter_patterns` (ADR 0080) is not overridden, for the
    // same reason as `VueSupport`: this query is `TypeScriptSupport`'s
    // plus the prop patterns above, and that impl's doc comment on this
    // method explains why the bare-name default is kept.

    /// Vitest's conventions, mirroring the Vue impl's: `.test.svelte`/
    /// `.spec.svelte` suffixes or a `__tests__/` directory anywhere in
    /// the path.
    fn is_test_path(&self, path: &str) -> bool {
        let file_name = path.rsplit('/').next().unwrap_or(path);
        [".test.svelte", ".spec.svelte"]
            .iter()
            .any(|suffix| file_name.ends_with(suffix))
            || path.split('/').any(|segment| segment == "__tests__")
    }

    fn component_markup_references(&self, source: &str) -> Option<Vec<String>> {
        Some(vue::component_tags(source))
    }

    fn source_for_parse<'a>(&self, source: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Owned(vue::mask_non_script(source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[test]
    fn should_keep_both_blocks_when_component_has_module_and_instance_scripts() {
        let source = "<script context=\"module\">\nexport function preload(): void {}\n</script>\n\n<script>\nfunction bump(): void {}\n</script>\n\n<button on:click={bump}>go</button>\n";

        let actual = vue::mask_non_script(source);

        assert!(actual.contains("export function preload(): void {}"));
        assert!(actual.contains("function bump(): void {}"));
        assert!(!actual.contains("<button"));
    }

    #[rstest]
    #[case::should_treat_tests_dir_as_test_path("src/__tests__/Button.svelte", true)]
    #[case::should_treat_spec_suffix_as_test_path("src/Button.spec.svelte", true)]
    #[case::should_treat_test_suffix_as_test_path("src/Button.test.svelte", true)]
    #[case::should_not_treat_component_as_test_path("src/lib/Button.svelte", false)]
    fn is_test_path_cases(#[case] path: &str, #[case] expected: bool) {
        let support = SvelteSupport;

        let actual = support.is_test_path(path);

        assert_eq!(expected, actual);
    }
}

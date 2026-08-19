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

pub struct SvelteSupport;

impl LanguageSupport for SvelteSupport {
    fn name(&self) -> &'static str {
        "svelte"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }

    fn definition_query(&self) -> &str {
        typescript::DEFINITION_QUERY
    }

    fn reference_query(&self) -> &str {
        typescript::REFERENCE_QUERY
    }

    // `index_prefilter_patterns` (ADR 0080) is not overridden, for the
    // same reason as `VueSupport`: it shares `DEFINITION_QUERY` with
    // `TypeScriptSupport`, whose doc comment on this method explains why
    // the bare-name default is kept.

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

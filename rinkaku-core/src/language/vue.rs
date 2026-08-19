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

pub struct VueSupport;

impl LanguageSupport for VueSupport {
    fn name(&self) -> &'static str {
        "vue"
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
        masked[content_start..content_end].copy_from_slice(&bytes[content_start..content_end]);
        cursor = content_end;
    }

    String::from_utf8(masked).expect(
        "masking replaces whole bytes with ASCII spaces and copies back byte ranges that lie \
         on ASCII tag boundaries, so the result stays valid UTF-8",
    )
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
        let source =
            "<script>\nconst a = () => 1;\n</script>\n<script setup>\nconst b = () => 2;\n</script>\n";

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

//! Markdown `LanguageSupport` implementation (ADR 0096).

use super::LanguageSupport;

/// Captures every `section` — the block grammar's own heading-rooted
/// subtree — rather than the heading node itself. A section is to a
/// document what a function is to a source file: the heading is its
/// declaration and the prose under it is its body, so capturing sections
/// makes "the definition containing a changed line" mean "the section the
/// edit landed in", and lets the existing narrowest-enclosing-definition
/// rule pick the deepest touched subsection out of its ancestors.
///
/// `setext_heading` is captured on its own because the grammar opens a
/// `section` for ATX headings only: an underlined heading stays a plain
/// block among its siblings, with no subtree to call its body. Capturing
/// it reports the heading itself when the heading lines change, which is
/// less than a section gives but more than the silence of not capturing
/// it at all (ADR 0096).
///
/// A document's leading section (content before its first heading) is
/// captured too but never becomes a symbol: it has no heading child, so
/// `extract`'s `definition_name` finds no name for it and drops it.
const DEFINITION_QUERY: &str = "[(section) (setext_heading)] @definition";

/// Markdown declares nothing another file can call, so there is no
/// reference to capture: a section's dependencies are prose links, not
/// resolvable symbols. Deliberately empty (a valid query with zero
/// patterns) rather than approximated — an inline code span like
/// `` `foo()` `` names a symbol only by coincidence of formatting, and
/// resolving it would invent dependency edges out of typography.
const REFERENCE_QUERY: &str = "";

pub struct MarkdownSupport;

impl LanguageSupport for MarkdownSupport {
    fn name(&self) -> &'static str {
        "markdown"
    }

    fn grammar(&self) -> tree_sitter::Language {
        tree_sitter_md::LANGUAGE.into()
    }

    fn definition_query(&self) -> &str {
        DEFINITION_QUERY
    }

    fn reference_query(&self) -> &str {
        REFERENCE_QUERY
    }

    /// Markdown has no test-file *naming* convention of its own — no
    /// ecosystem runs a `*_test.md` — so only the directory decides. The
    /// segment names checked here are exactly the ones
    /// `rinkaku-tui`'s `tests_section::is_test_dir_path` falls back to
    /// for a path with no registered language, which is what governed
    /// `.md` files before Markdown had one: a document under `tests/`
    /// keeps routing to the Tests section instead of reviving a `tests/`
    /// node in the production tree beside it (ADR 0077's duplication).
    fn is_test_path(&self, path: &str) -> bool {
        path.split('/').any(|segment| {
            matches!(
                segment,
                "tests" | "Tests" | "test" | "__tests__" | "testdata"
            )
        })
    }

    /// Never: heading text is prose, and prose collides with code
    /// (`Context`, `Usage`, `Overview`) far too readily for the
    /// name-only resolution of ADR 0003 to survive it. Indexing
    /// documents would attach a "Depends on:" entry pointing at an ADR
    /// heading to any symbol that happens to share its wording — a
    /// wrong answer, not merely a slow one, so this is a correctness
    /// gate rather than the scan-cost trade-off PHP's Blade override
    /// makes (ADR 0078 addendum). Changed documents still go through
    /// diff analysis and rendering exactly as any other file does.
    fn contributes_to_dependency_index(&self, _path: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[test]
    fn should_report_markdown_as_name() {
        let support = MarkdownSupport;

        assert_eq!("markdown", support.name());
    }

    #[test]
    fn should_produce_a_grammar_that_parses_without_errors() {
        let support = MarkdownSupport;
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&support.grammar())
            .expect("Markdown grammar should load into a tree-sitter parser");

        let tree = parser
            .parse("# Title\n\nBody text.\n\n## Section\n", None)
            .expect("parse should produce a tree");

        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn should_compile_definition_query_against_its_own_grammar() {
        let support = MarkdownSupport;

        tree_sitter::Query::new(&support.grammar(), support.definition_query())
            .expect("DEFINITION_QUERY must be valid against the Markdown grammar");
    }

    #[test]
    fn should_compile_reference_query_against_its_own_grammar() {
        let support = MarkdownSupport;

        tree_sitter::Query::new(&support.grammar(), support.reference_query())
            .expect("REFERENCE_QUERY must be valid against the Markdown grammar");
    }

    #[rstest]
    #[case::should_return_false_when_path_is_a_plain_document("docs/adr/0096-markdown.md", false)]
    #[case::should_return_false_when_a_name_merely_mentions_testing("docs/testing.md", false)]
    #[case::should_return_false_when_a_file_name_looks_like_a_test("docs/parser_test.md", false)]
    #[case::should_return_true_when_path_is_under_tests("tests/README.md", true)]
    #[case::should_return_true_when_path_is_under_capitalized_tests(
        "Tests/Feature/README.md",
        true
    )]
    #[case::should_return_true_when_path_is_under_testdata("internal/testdata/notes.md", true)]
    fn is_test_path_cases(#[case] path: &str, #[case] expected: bool) {
        let support = MarkdownSupport;

        let actual = support.is_test_path(path);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_not_contribute_to_dependency_index() {
        let support = MarkdownSupport;

        assert!(!support.contributes_to_dependency_index("README.md"));
    }
}

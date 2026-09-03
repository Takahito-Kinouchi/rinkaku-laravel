//! Tests pinning [`super::extract_changed_symbols`] and
//! [`super::extract_all_symbols`] behavior on Markdown documents (ADR
//! 0096): a section is the definition, its heading the signature, and its
//! enclosing section the container — plus the two shapes the block
//! grammar handles differently from an ATX section (a document's leading
//! content, and an underlined setext heading).

use super::*;
use crate::language::markdown::MarkdownSupport;
use pretty_assertions::assert_eq;

/// Line numbers referenced by the tests below:
///
/// ```text
///  1  # rinkaku
///  3  Intro prose.
///  5  ## Architecture
///  7  Prose about layers.
///  9  ### Ports
/// 11  Trait talk.
/// 13  ## Toolchain
/// 15  Setext Style
/// 16  ------------
/// 18  Make targets.
/// ```
fn document_source() -> &'static str {
    "# rinkaku\n\nIntro prose.\n\n## Architecture\n\nProse about layers.\n\n### Ports\n\nTrait talk.\n\n## Toolchain\n\nSetext Style\n------------\n\nMake targets.\n"
}

#[test]
fn should_extract_every_heading_as_a_section_nested_under_its_parent_heading() {
    let lang = MarkdownSupport;

    let symbols = extract_all_symbols(document_source(), &lang);

    // Projected rather than compared whole: the four fields below are
    // everything Markdown extraction decides. `referenced_names` and
    // `referenced_method_names` are empty for every Markdown symbol by
    // construction (the language has no reference query), and the
    // remaining fields are populated by later pipeline stages, so a
    // whole-struct comparison would repeat the same constant tail five
    // times without pinning anything this test is about — the
    // single-symbol cases below compare whole structs instead.
    let shapes: Vec<(String, String, LineRange, Option<String>)> = symbols
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                s.signature.clone(),
                s.range,
                s.container.clone(),
            )
        })
        .collect();
    let expected = vec![
        (
            "rinkaku".to_string(),
            "# rinkaku".to_string(),
            LineRange { start: 1, end: 18 },
            None,
        ),
        (
            "Architecture".to_string(),
            "## Architecture".to_string(),
            LineRange { start: 5, end: 12 },
            Some("section rinkaku".to_string()),
        ),
        (
            "Ports".to_string(),
            "### Ports".to_string(),
            LineRange { start: 9, end: 12 },
            Some("section Architecture".to_string()),
        ),
        (
            "Toolchain".to_string(),
            "## Toolchain".to_string(),
            LineRange { start: 13, end: 18 },
            Some("section rinkaku".to_string()),
        ),
        (
            "Setext Style".to_string(),
            "Setext Style\n------------".to_string(),
            LineRange { start: 15, end: 16 },
            Some("section Toolchain".to_string()),
        ),
    ];
    assert_eq!(expected, shapes);
}

#[test]
fn should_report_the_innermost_section_when_prose_inside_a_subsection_changed() {
    let lang = MarkdownSupport;
    // Line 11 is `Trait talk.`, the prose under `### Ports`.
    let changed_ranges = vec![LineRange { start: 11, end: 11 }];

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Ports".to_string(),
        kind: SymbolKind::Section,
        signature: "### Ports".to_string(),
        range: LineRange { start: 9, end: 12 },
        container: Some("section Architecture".to_string()),
        referenced_names: vec![],
        referenced_method_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(document_source(), &lang, &changed_ranges);

    assert_eq!(expected, actual);
}

/// A section's node runs to the line terminator that ends its last block,
/// which tree-sitter reports as column 0 of the *next* section's heading
/// line. Left uncorrected, editing one heading would report its
/// predecessor as changed too.
#[test]
fn should_report_only_the_edited_section_when_a_sibling_heading_line_changed() {
    let lang = MarkdownSupport;
    // Line 13 is `## Toolchain`, the line directly after `## Architecture`'s
    // last line.
    let changed_ranges = vec![LineRange { start: 13, end: 13 }];

    let actual = extract_changed_symbols(document_source(), &lang, &changed_ranges);

    let names: Vec<&str> = actual.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(vec!["Toolchain"], names);
}

#[test]
fn should_extract_no_symbols_when_the_document_has_no_heading() {
    let lang = MarkdownSupport;
    let source = "Just prose, no heading at all.\n\nA second paragraph.\n";

    let actual = extract_all_symbols(source, &lang);

    assert_eq!(Vec::<ExtractedSymbol>::new(), actual);
}

#[test]
fn should_extract_no_symbols_when_the_change_is_above_the_first_heading() {
    let lang = MarkdownSupport;
    let source = "Preamble prose.\n\n# Title\n\nBody.\n";
    // Line 1 is the preamble, inside the grammar's heading-less leading
    // section — a definition node with no name, so no symbol.
    let changed_ranges = vec![LineRange { start: 1, end: 1 }];

    let actual = extract_changed_symbols(source, &lang, &changed_ranges);

    assert_eq!(Vec::<ExtractedSymbol>::new(), actual);
}

#[test]
fn should_extract_markdown_symbol_end_to_end_from_a_unified_diff() {
    use crate::diff::parse_unified_diff;
    use crate::language::language_for_path;

    let diff = "\
diff --git a/docs/guide.md b/docs/guide.md
index e69de29..4b825dc 100644
--- a/docs/guide.md
+++ b/docs/guide.md
@@ -1,3 +1,3 @@
 ## Install

-Run the old command.
+Run the new command.
";
    let source = "## Install\n\nRun the new command.\n";
    let changed_file = parse_unified_diff(diff)
        .expect("diff should parse")
        .into_iter()
        .next()
        .expect("diff should contain one changed file");
    let lang = language_for_path(&changed_file.path).expect("*.md should resolve to Markdown");

    let expected = vec![ExtractedSymbol {
        id: String::new(),
        name: "Install".to_string(),
        kind: SymbolKind::Section,
        signature: "## Install".to_string(),
        range: LineRange { start: 1, end: 3 },
        container: None,
        referenced_names: vec![],
        referenced_method_names: vec![],
        dependencies: vec![],
        omitted_dependency_matches: 0,
        is_test: false,
        classification: None,
        previous_signature: None,
    }];
    let actual = extract_changed_symbols(source, lang, &changed_file.changed_ranges);

    assert_eq!(expected, actual);
}

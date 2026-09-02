//! Markdown-specific reads of a captured `section` or `setext_heading`
//! node (ADR 0096).
//!
//! Markdown's definition is the `section`: its heading is the declaration
//! and everything under it is the body. Both of those are positional
//! rather than field-addressed — the block grammar gives a section no
//! `name` or `body` field — so `extract`'s generic paths cannot reach
//! them, and this module holds the three reads that know the shape:
//! which child is the heading, what the heading is called, and where the
//! body begins. Sibling of [`super::hcl`], which does the same job for
//! the one other language whose definitions are not field-addressed.

/// The heading a captured definition is named by: the node itself when a
/// `setext_heading` was captured directly, otherwise the heading a
/// `section` is rooted at. `None` for a document's leading section — the
/// content before its first heading, which the grammar still wraps in a
/// `section` node.
fn heading_of<'a>(node: tree_sitter::Node<'a>) -> Option<tree_sitter::Node<'a>> {
    if is_heading(node) {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| is_heading(*child))
}

fn is_heading(node: tree_sitter::Node) -> bool {
    matches!(node.kind(), "atx_heading" | "setext_heading")
}

/// A Markdown definition's name: its heading text with the `#` markers,
/// the setext underline, and any surrounding whitespace removed — `##
/// Test strategy` is named `Test strategy`. `None` when the node has no
/// heading (a leading section) or an empty one (`##` alone).
pub(super) fn heading_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let heading = heading_of(node)?;
    let text = heading_inline_text(heading, source)?;
    let name = strip_closing_sequence(text.trim());
    (!name.is_empty()).then(|| name.to_string())
}

/// Where a section's body starts: its first child after the heading, the
/// node [`super::signature_slice`] cuts the signature off at, exactly as a
/// function's `body` field is for every field-addressed language. `None`
/// for a heading-only section, whose whole text is already its signature.
pub(super) fn section_body_start<'a>(
    section: tree_sitter::Node<'a>,
) -> Option<tree_sitter::Node<'a>> {
    let heading = heading_of(section)?;
    let mut cursor = section.walk();
    section
        .children(&mut cursor)
        .find(|child| child.start_byte() >= heading.end_byte())
}

/// The `inline` node carrying a heading's text. An ATX heading holds it
/// directly; a setext heading holds it inside the `paragraph` its
/// underline promotes to a heading, so one level of nesting is searched
/// rather than only the immediate children.
fn heading_inline_text<'a>(heading: tree_sitter::Node, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = heading.walk();
    let inline = heading
        .named_children(&mut cursor)
        .find_map(|child| match child.kind() {
            "inline" => Some(child),
            "paragraph" => {
                let mut inner = child.walk();
                child
                    .named_children(&mut inner)
                    .find(|grandchild| grandchild.kind() == "inline")
            }
            _ => None,
        })?;
    inline.utf8_text(source).ok()
}

/// Drops an ATX heading's optional closing `#` sequence (`## Title ##`),
/// which the grammar leaves inside the heading's inline text. CommonMark
/// requires the closing run to be preceded by a space, so requiring that
/// space here is what keeps a heading legitimately *ending* in a hash —
/// `## Why C#` — from being truncated to `Why C`.
fn strip_closing_sequence(text: &str) -> &str {
    let without_hashes = text.trim_end_matches('#');
    if without_hashes.len() == text.len() {
        return text;
    }
    match without_hashes.ends_with(char::is_whitespace) {
        true => without_hashes.trim_end(),
        false => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    fn parse(source: &str) -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_md::LANGUAGE.into())
            .expect("Markdown grammar should load");
        parser.parse(source, None).expect("parse produces a tree")
    }

    /// Every node the Markdown definition query would capture, in
    /// document order — the same `section` / `setext_heading` set
    /// `language::markdown::DEFINITION_QUERY` selects, gathered here by
    /// a plain walk so these tests stay independent of query mechanics.
    fn definitions(tree: &tree_sitter::Tree) -> Vec<tree_sitter::Node<'_>> {
        let mut found = Vec::new();
        let mut stack = vec![tree.root_node()];
        while let Some(node) = stack.pop() {
            if matches!(node.kind(), "section" | "setext_heading") {
                found.push(node);
            }
            let mut cursor = node.walk();
            let children: Vec<_> = node.children(&mut cursor).collect();
            stack.extend(children.into_iter().rev());
        }
        found.sort_by_key(|node| node.start_byte());
        found
    }

    #[rstest]
    #[case::should_name_an_atx_heading("## Test strategy\n\nbody\n", Some("Test strategy"))]
    #[case::should_name_a_setext_heading(
        "Test strategy\n=============\n\nbody\n",
        Some("Test strategy")
    )]
    #[case::should_strip_a_closing_hash_sequence("##  Spaced  ##\n\nbody\n", Some("Spaced"))]
    #[case::should_keep_a_hash_that_is_part_of_the_text("## Why C#\n\nbody\n", Some("Why C#"))]
    #[case::should_keep_inline_markup_verbatim(
        "## `slice_signature()`\n\nbody\n",
        Some("`slice_signature()`")
    )]
    #[case::should_find_no_name_when_the_section_has_no_heading("Preamble prose.\n", None)]
    #[case::should_find_no_name_when_the_heading_is_empty("##\n\nbody\n", None)]
    fn heading_name_cases(#[case] source: &str, #[case] expected: Option<&str>) {
        let tree = parse(source);

        let actual = heading_name(definitions(&tree)[0], source.as_bytes());

        assert_eq!(expected.map(str::to_string), actual);
    }

    #[test]
    fn should_start_the_body_at_the_first_node_under_the_heading() {
        let source = "# Title\n\nIntro.\n\n## Sub\n";
        let tree = parse(source);

        let actual = section_body_start(definitions(&tree)[0]).expect("section has a body");

        assert_eq!("paragraph", actual.kind());
        assert_eq!(2, actual.start_position().row);
    }

    #[test]
    fn should_start_the_body_at_a_nested_section_when_the_heading_is_followed_by_one() {
        let source = "# Title\n## Sub\n\nprose\n";
        let tree = parse(source);

        let actual = section_body_start(definitions(&tree)[0]).expect("section has a body");

        assert_eq!("section", actual.kind());
        assert_eq!(1, actual.start_position().row);
    }

    #[test]
    fn should_find_no_body_when_the_section_is_a_heading_alone() {
        let source = "# Title\n";
        let tree = parse(source);

        let actual = section_body_start(definitions(&tree)[0]);

        assert!(actual.is_none());
    }
}

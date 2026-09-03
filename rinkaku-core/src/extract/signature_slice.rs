//! Producing a definition's signature text: the declaration without its
//! implementation.
//!
//! This is the core of the signature pipeline `extract` is built around,
//! and the reason rinkaku's output is a narrow surface at all (ADR 0095):
//! a body, a comment and a doc comment are removed *here*, so nothing
//! downstream has to remember that they should be. Its siblings are the
//! rest of that pipeline — [`super::definition_span`] decides where a
//! definition's span starts, [`super::container_slice`] narrows a
//! container down to the members a diff touched, and
//! [`super::signature_bound`] caps what comes out — which is why this
//! belongs beside them rather than in `mod.rs` (ADR 0028: split along
//! responsibility).

use super::TouchedContext;
use super::container_slice::untouched_member_ranges;
use super::definition_span::DefinitionNode;
use super::hcl::hcl_block_type;
use super::signature_bound;

/// Slices a definition's signature: the declaration text with implementation
/// detail and comment nodes removed, keeping its original line structure
/// (dedented — see [`tidy_signature_lines`], ADR 0060) rather than
/// collapsing it to one line. Contract-impact comparison
/// ([`classify_symbols`]) normalizes whitespace separately, at comparison
/// time, so a reflow-only difference between two multi-line signatures
/// still doesn't register as a contract change.
///
/// - `function_item`, `function_declaration` (Go/TS), `method_declaration`
///   (Go), `function_definition` (Python), `method_definition` (TS),
///   `variable_declarator` (TS arrow function): body stripped, only the
///   declaration up to (not including) the body is kept.
/// - `struct_item`, `enum_item`, `trait_item`, `type_spec` (Go),
///   `interface_declaration`, `type_alias_declaration`, `enum_declaration`
///   (TS), `abstract_method_signature` (TS): no separate "body" in the
///   implementation sense — their fields/variants/method signatures *are*
///   the API surface — so the whole node text is kept.
/// - `section` (Markdown, ADR 0096): the heading is the declaration and
///   the prose under it is the body, so only the heading line survives.
/// - `class_definition` (Python), `class_declaration` /
///   `abstract_class_declaration` (TS): the class header plus member
///   signatures are kept (field/method signatures are the API surface,
///   same as struct/interface), but nested method *bodies* — including a
///   class field whose value is an arrow function, e.g. `area = (): number
///   => { ... }` — are stripped so a class reads as a list of member
///   signatures rather than full implementations. When `touched` is
///   `Some` (only `extract_changed_symbols` passes this — see
///   [`TouchedContext`]), a member with no line overlapping
///   `TouchedContext::changed_ranges` is dropped from the signature
///   entirely, header and all (ADR 0071): a container is only ever
///   reported as a symbol because some body-level line of its own was
///   touched, so an unrelated, untouched method or nested class adds
///   nothing but noise to that report.
///
/// Comment nodes (`line_comment`/`block_comment` in Rust, `comment` in
/// Go/Python/TypeScript — see [`is_comment_node`]) inside the kept range are
/// stripped in every case (ADR 0014): otherwise a comment-only edit inside a
/// struct/interface/class body would change the reported signature string,
/// making a `body_only`/`signature_changed` classification based on
/// signature-string equality fire incorrectly. This is a pre-1.0 output
/// change sanctioned by the ADR — some previously-reported signature strings
/// that contained inline comments now omit them.
pub(super) fn slice_signature(
    definition: DefinitionNode,
    source: &[u8],
    touched: Option<TouchedContext>,
) -> String {
    let node = definition.node;
    let first_line_column = definition.span_start_column();

    if matches!(
        node.kind(),
        // `trait_declaration` and `enum_declaration` join the class-like
        // branch for PHP's sake: like a class, their methods carry full
        // bodies that are implementation detail, not API surface. A
        // TypeScript `enum_declaration` also lands here but is unaffected
        // — its members are plain name/value pairs, so there is no method
        // body to strip and no member definition to narrow away.
        "class_definition"
            | "class_declaration"
            | "abstract_class_declaration"
            | "trait_declaration"
            | "enum_declaration"
    ) {
        let mut removed_ranges: Vec<std::ops::Range<usize>> = Vec::new();
        collect_method_body_ranges(node, &mut removed_ranges);
        if let Some(touched) = touched {
            removed_ranges.extend(untouched_member_ranges(
                node,
                touched.all_definition_nodes,
                touched.changed_ranges,
                source,
            ));
        }
        collect_comment_ranges(node, source, &removed_ranges.clone(), &mut removed_ranges);
        return tidy_signature_lines(
            &text_with_ranges_removed(node, definition.span_start_byte(), source, removed_ranges),
            first_line_column,
        );
    }

    // `variable_declarator`'s body (a TS arrow function's `{ ... }`) is
    // nested one level deeper, under its `value` field, rather than being a
    // direct `body` field of the captured node itself.
    let body = if node.kind() == "variable_declarator" {
        node.child_by_field_name("value")
            .and_then(|value| value.child_by_field_name("body"))
    } else if matches!(
        node.kind(),
        "function_item"
            | "function_declaration"
            | "method_declaration"
            | "function_definition"
            | "method_definition"
    ) {
        node.child_by_field_name("body")
    } else if node.kind() == "section" {
        super::markdown::section_body_start(node)
    } else if node.kind() == "block" {
        match hcl_block_type(node, source).as_deref() {
            // variable/output bodies (type, default, value, ...) are
            // the contract itself (ADR 0066) — keep the whole block.
            Some("variable" | "output") => None,
            _ => {
                let mut cursor = node.walk();
                node.children(&mut cursor)
                    .find(|child| child.kind() == "block_start")
            }
        }
    } else {
        None
    };

    let text_end = body
        .map(|body| body.start_byte())
        .unwrap_or(node.end_byte());
    let mut comment_ranges: Vec<std::ops::Range<usize>> = Vec::new();
    collect_comment_ranges(node, source, &[], &mut comment_ranges);
    // Comments at/after `text_end` fall inside the body, which is dropped
    // wholesale below anyway — only ones inside the kept declaration prefix
    // need to be individually removed.
    comment_ranges.retain(|range| range.start < text_end);

    let mut removed_ranges = comment_ranges;
    if let Some(body) = body {
        removed_ranges.push(body.start_byte()..node.end_byte());
    }

    let raw = text_with_ranges_removed(node, definition.span_start_byte(), source, removed_ranges);
    tidy_signature_lines(&raw, first_line_column)
}

/// Removes every byte range in `ranges` from the `span_start_byte..
/// node.end_byte()` text (the definition's full widened span, not just the
/// declaration prefix — callers that only want a prefix pre-truncate
/// `ranges` to stop at that boundary), returning the remainder as a
/// `String`. Ranges are sorted and removed front-to-back, advancing a
/// `cursor` past each removed range in turn, so earlier removals naturally
/// narrow what later ones can still remove; if a range starts before the
/// current `cursor` (overlapping ranges, defensively not expected in
/// practice) *that one range's removal* is skipped — its own iteration does
/// nothing and `cursor` is left wherever the previous iteration advanced it
/// to — rather than the whole function panicking on an invalid slice.
fn text_with_ranges_removed(
    node: tree_sitter::Node,
    span_start_byte: usize,
    source: &[u8],
    mut ranges: Vec<std::ops::Range<usize>>,
) -> String {
    ranges.sort_by_key(|r| r.start);

    let mut result = Vec::with_capacity(source.len());
    let mut cursor = span_start_byte;
    for range in &ranges {
        if range.start < cursor {
            continue; // Defensive: overlapping ranges should not occur.
        }
        result.extend_from_slice(&source[cursor..range.start.min(node.end_byte())]);
        cursor = range.end.max(cursor);
    }
    result.extend_from_slice(&source[cursor.min(node.end_byte())..node.end_byte()]);
    String::from_utf8(result).unwrap_or_default()
}

/// Recursively collects the byte ranges of every nested method body inside
/// a class node (`function_definition`/`method_definition`, or a TS class
/// field whose value is an arrow function, e.g. `area = (): number => {
/// ... }`), without descending into a method's own body (a nested function
/// *inside* a method body is implementation detail, not a member
/// signature).
///
/// `public_field_definition`'s body is nested one level deeper, under its
/// `value` field's own `body`, rather than being a direct `body` field of
/// the field definition itself — same shape as `variable_declarator` in
/// `slice_signature`.
fn collect_method_body_ranges(node: tree_sitter::Node, ranges: &mut Vec<std::ops::Range<usize>>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let is_method = matches!(
            child.kind(),
            // `method_declaration` is PHP's (Go's same-named methods are
            // never nested inside a class-like node, so this walk never
            // sees them).
            "function_definition" | "method_definition" | "method_declaration"
        );
        if is_method && let Some(body) = child.child_by_field_name("body") {
            ranges.push(body.start_byte()..child.end_byte());
            continue; // Don't descend into the stripped body.
        }
        if child.kind() == "public_field_definition"
            && let Some(value) = child.child_by_field_name("value")
            && value.kind() == "arrow_function"
            && let Some(body) = value.child_by_field_name("body")
        {
            ranges.push(body.start_byte()..child.end_byte());
            continue; // Don't descend into the stripped body.
        }
        collect_method_body_ranges(child, ranges);
    }
}

/// Recursively collects the byte ranges of every comment node
/// ([`is_comment_node`]) inside `node`, skipping any range already covered
/// by `already_removed` (e.g. a method body `collect_method_body_ranges`
/// already sliced out) so a comment nested inside an already-removed range
/// isn't redundantly appended a second time — `text_with_ranges_removed`
/// would still handle an overlapping range correctly (it clamps against
/// `cursor`), but skipping it here keeps `ranges` a non-overlapping set,
/// matching that function's stated "not expected in practice" assumption.
///
/// Each comment's own range is widened by [`widen_to_whole_line_comment`]
/// before being pushed, so a whole-line comment's leading indentation is
/// removed along with it.
fn collect_comment_ranges(
    node: tree_sitter::Node,
    source: &[u8],
    already_removed: &[std::ops::Range<usize>],
    ranges: &mut Vec<std::ops::Range<usize>>,
) {
    if is_comment_node(node) {
        ranges.push(widen_to_whole_line_comment(
            node.start_byte()..node.end_byte(),
            source,
        ));
        return; // A comment node has no children worth descending into.
    }
    if already_removed
        .iter()
        .any(|r| r.start <= node.start_byte() && node.end_byte() <= r.end)
    {
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_comment_ranges(child, source, already_removed, ranges);
    }
}

/// Extends a comment node's own byte range to also cover its leading
/// horizontal whitespace back to (not including) the previous newline,
/// when that whitespace is the only thing between the previous newline
/// and the comment — i.e. the comment is the whole line's content.
///
/// Whether a grammar's comment token span includes its own trailing
/// newline is not uniform (tree-sitter-rust's outer/inner line doc
/// comments, `///`/`//!`, include it; every other comment kind this
/// module handles does not), so leaving the leading indent out of the
/// removed range is only safe for the grammars that exclude the
/// newline. Folding the indent into the range here makes the result the
/// same either way, without branching on which grammar produced the
/// node.
fn widen_to_whole_line_comment(
    range: std::ops::Range<usize>,
    source: &[u8],
) -> std::ops::Range<usize> {
    let line_start = source[..range.start]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |pos| pos + 1);
    if source[line_start..range.start]
        .iter()
        .all(|&b| b == b' ' || b == b'\t')
    {
        line_start..range.end
    } else {
        range
    }
}

/// Whether `node` is a tree-sitter comment node under any of the five
/// grammars this module supports: Rust splits line/block comments into two
/// distinct kinds (`line_comment`, `block_comment`); Go, HCL, Python, and
/// TypeScript each use a single `comment` kind (verified against each grammar
/// directly).
fn is_comment_node(node: tree_sitter::Node) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment" | "comment")
}

/// Normalizes a signature for contract-impact comparison only
/// ([`classify_symbols`], ADR 0014/0060) — display uses
/// [`tidy_signature_lines`] instead, which keeps line structure, and other
/// single-line render sites collapse whitespace unconditionally instead
/// (each documented at its own call, e.g. `render::markdown`'s
/// `collapse_to_single_line`; that transform is display-only and must stay
/// unconditional so a struct like `Foo{x: i32}` still reads with a space
/// after `{`, so it is not reused here).
///
/// Every maximal run of whitespace is replaced with a single space when
/// both the character immediately before and immediately after the run are
/// word characters (alphanumeric or `_`), and removed entirely otherwise.
/// A naive `split_whitespace().join(" ")` (this function's predecessor)
/// always inserts a space at a normalized run regardless of what the
/// original text had there, so a reflow that introduces a line break right
/// after a symbol like `(` or `,` — a position that never had a space in
/// the original — would compare unequal to the un-reflowed form purely
/// because of where the normalizer chose to put a space back. Restricting
/// the collapse to word/word boundaries keeps a reflow-only change
/// (whitespace inserted or moved next to punctuation) invisible to the
/// comparison while still preserving whitespace that is itself meaningful
/// content (e.g. the space in `a b` vs. no space in `ab`).
pub(super) fn normalize_for_comparison(text: &str) -> String {
    let is_word_char = |c: char| c.is_alphanumeric() || c == '_';
    let chars: Vec<char> = text.chars().collect();

    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            let run_start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let before = chars[..run_start].last().copied();
            let after = chars.get(i).copied();
            if before.is_some_and(is_word_char) && after.is_some_and(is_word_char) {
                result.push(' ');
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

/// Shapes a raw, range-stripped declaration slice into the multi-line text
/// stored on [`ExtractedSymbol::signature`] (ADR 0060): dedents every
/// continuation line relative to the least-indented non-blank line
/// (`first_line_column` standing in for the first line's own indentation,
/// see below), trims trailing whitespace from each line, collapses runs
/// of blank lines left behind by a stripped comment/body into a single
/// blank line, and trims leading/trailing blank lines. A single-line
/// input is returned trimmed, same as before ADR 0060.
///
/// `first_line_column` is the node's `start_position().column` in the
/// original source — the column the declaration keyword (`fn`/`class`/...)
/// actually starts at, which is *not* recoverable from the text alone: a
/// tree-sitter node's text only ever contains that node's own span, so the
/// first line of the sliced text never carries the leading whitespace
/// before it, whether or not the definition is nested. Folding this column
/// into the dedent-baseline calculation (as a stand-in for "line 0's own
/// indent") is what tells a nested definition (`first_line_column` > 0,
/// e.g. 4 inside an `impl` block) apart from a top-level one
/// (`first_line_column` == 0): a nested method's continuation lines carry
/// their real absolute column and must be dedented back down to
/// `first_line_column`'s depth, while a top-level struct/class's
/// continuation lines are indented *relative to that definition's own
/// body* and must be left alone.
pub(super) fn tidy_signature_lines(text: &str, first_line_column: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();

    let min_indent = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            if index == 0 {
                first_line_column
            } else {
                line.len() - line.trim_start().len()
            }
        })
        .min()
        .unwrap_or(0);

    let dedented: Vec<&str> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if line.trim().is_empty() {
                ""
            } else if index == 0 {
                // Never dedented: see `first_line_column`'s doc comment.
                line.trim_end()
            } else {
                line.get(min_indent..).unwrap_or(line).trim_end()
            }
        })
        .collect();

    let mut collapsed: Vec<&str> = Vec::with_capacity(dedented.len());
    for line in dedented {
        if line.is_empty() && collapsed.last().is_some_and(|last: &&str| last.is_empty()) {
            continue;
        }
        collapsed.push(line);
    }

    while collapsed.first().is_some_and(|line| line.is_empty()) {
        collapsed.remove(0);
    }
    while collapsed.last().is_some_and(|line| line.is_empty()) {
        collapsed.pop();
    }

    signature_bound::bound_signature(collapsed.join("\n"))
}

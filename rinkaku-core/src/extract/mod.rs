//! Tree-sitter based signature extraction.
//!
//! Given a source file's text and the line ranges that changed in a diff
//! (see [`crate::diff::LineRange`]), finds the definitions that contain
//! those changed lines and slices out their signatures — the API surface,
//! without the implementation body.

use crate::diff::LineRange;
use crate::language::LanguageSupport;
use definition_span::DefinitionNode;
use hcl::{build_hcl_locals_symbols, hcl_block_name, hcl_block_type};
use references::collect_referenced_names;
use serde::Serialize;
use signature_slice::{normalize_for_comparison, slice_signature};
use std::collections::HashMap;
use tree_sitter::StreamingIterator;

mod container_slice;
mod definition_span;
mod hcl;
mod references;
pub mod signature_bound;
mod signature_slice;

/// Threaded through `build_symbols`/`build_symbol`/`slice_signature` only
/// by `extract_changed_symbols` (ADR 0071) — `extract_all_symbols` and HCL
/// `locals` expansion pass `None`, keeping their signatures whole
/// regardless of any diff, since neither has a "this diff's touched lines"
/// concept to narrow by (see [`untouched_member_ranges`]'s doc comment for
/// why: `extract_all_symbols` indexes every definition in a file
/// independent of any diff).
#[derive(Clone, Copy)]
struct TouchedContext<'a> {
    all_definition_nodes: &'a [DefinitionNode<'a>],
    changed_ranges: &'a [LineRange],
}

/// The kind of symbol a definition node represents, expressed in
/// language-neutral terms so callers don't need to match on
/// language-specific tree-sitter node kinds.
///
/// No `Impl` variant: impl/class/interface bodies are never reported as
/// symbols in their own right when one of their nested members was itself
/// touched (see the filtering in `extract_changed_symbols`) — they only
/// contribute `container` names to the members nested inside them.
///
/// Methods (Go receiver methods, Python/TypeScript class methods, Rust
/// impl/trait methods, TypeScript arrow functions bound to a
/// `const`/`let`/`var`) are all reported as `Function`, matching the
/// precedent already set by the Rust support: `container` is what
/// distinguishes "a method of X" from a free function, so a separate
/// `Method` variant would duplicate information already carried by
/// `container` without adding any.
/// Variants are named for the language-neutral concept they represent, not
/// for a specific language's keyword (e.g. `Class` covers both Python
/// `class` and TypeScript `class`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Class,
    Interface,
    TypeAlias,
    /// A named HCL block or `locals` attribute (ADR 0066): the name
    /// carries the Terraform-specific role (`aws_instance.web`,
    /// `var.region`), so one language-neutral kind suffices.
    Block,
}

/// A changed symbol's contract impact (ADR 0014), classified by comparing
/// its comment-stripped, normalized signature against the base side's:
///
/// - [`Classification::Added`]: no matching symbol on the base side at all
///   (a brand-new definition).
/// - [`Classification::SignatureChanged`]: a matching base-side symbol
///   exists, but its signature text differs — the API surface itself
///   changed, not just the implementation.
/// - [`Classification::BodyOnly`]: a matching base-side symbol exists with
///   an identical signature — only the body changed.
///
/// `None` (rather than a fourth "unknown" variant) is used when base-side
/// content wasn't available to compare against at all (e.g. plain stdin
/// input with no resolvable base commit) — see
/// [`crate::pipeline::analyze_diff`]'s `read_base_file` parameter. Modeling
/// "unknown" as the field's absence, rather than as a variant of this enum,
/// keeps every variant here meaning "we know, and this is what we found"; a
/// caller checking `symbol.classification.is_none()` reads unambiguously as
/// "classification wasn't attempted" rather than "found nothing interesting".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Added,
    SignatureChanged,
    BodyOnly,
}

/// A definition whose signature was extracted because one of its lines
/// (declaration or body) fell inside a changed range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExtractedSymbol {
    /// Stable identifier matching this symbol's [`crate::graph::Node::id`]
    /// once graph-building has run, so JSON consumers can correlate a
    /// symbol with the graph's `nodes`/`edges`/`roots` without recomputing
    /// the `{path}::{name}` scheme themselves. Empty until
    /// [`crate::graph::build_graph`] populates it (mirrors `dependencies`
    /// and `omitted_dependency_matches`, both populated post-extraction by
    /// a later pipeline stage rather than by `build_symbol`).
    pub id: String,
    pub name: String,
    pub kind: SymbolKind,
    /// Declaration text without its body, whitespace-normalized. Doc
    /// comments and attributes are not included.
    pub signature: String,
    /// Full definition range (new-side, 1-based inclusive) — body included,
    /// since this describes where the change lives, not the signature's
    /// own extent.
    pub range: LineRange,
    /// The enclosing impl/trait/class block's descriptive name, or a Go
    /// method's receiver type name, if the definition belongs to one (e.g.
    /// `Some("impl Foo")`, `Some("class Point")`, `Some("Repo")`).
    pub container: Option<String>,
    /// Names this definition references as *bare* calls/types (no
    /// receiver) — `@reference.call`/`@reference.type` captures, plus the
    /// non-query code walks (macro bodies, module-scoped calls, HCL
    /// traversals) — as captured by [`LanguageSupport::reference_query`].
    /// Deduplicated but otherwise unresolved — an intermediate pipeline
    /// artifact, not part of rinkaku's output shape, so it is excluded
    /// from serialization. [`crate::deps::resolve_dependencies`] resolves
    /// these against a repo-wide definition index to populate
    /// `dependencies`.
    ///
    /// A bare reference cannot syntactically denote a symbol nested inside
    /// a container (impl/trait/class/interface) in Python, Go, or
    /// TypeScript — member access there is a distinct grammar shape
    /// (`obj.method()`), captured separately as
    /// [`referenced_method_names`](Self::referenced_method_names) — so
    /// `crate::graph::collect_edges` restricts a `referenced_names`
    /// match to changed symbols with no container, or the same container
    /// as the referencing symbol itself (ADR 0068).
    ///
    /// A bare reference can, however, name a *container* — most of these
    /// captures are type references, and a diff's changed symbols are
    /// usually that container's members rather than the container itself
    /// — so `collect_edges` also matches an entry here against
    /// [`container`](Self::container), linking the referrer to the
    /// container's changed members (ADR 0086).
    #[serde(skip)]
    pub referenced_names: Vec<String>,
    /// Names this definition references via a receiver or method-spec
    /// position — `@reference.method` (Rust's `x.foo()` receiver calls,
    /// ADR 0064) plus `@reference.methodspec` (Rust trait method names,
    /// Go interface method-spec names, TypeScript interface
    /// method-signature names — the container-referring captures ADR 0012
    /// introduced, split out so only receiver calls pass the ADR 0064
    /// stoplist; ADR 0068). Unlike `referenced_names`, these may legitimately
    /// denote a symbol nested inside any container, so
    /// `crate::graph::collect_edges` matches them without the
    /// same-container restriction. Same "intermediate pipeline artifact"
    /// status as `referenced_names`, so also excluded from serialization.
    #[serde(skip)]
    pub referenced_method_names: Vec<String>,
    /// This symbol's 1-hop dependencies: `referenced_names` that resolved
    /// to a definition elsewhere in the repo (ADR 0003), excluding the
    /// symbol's own definition and any symbol already reported in the same
    /// diff (see `crate::deps::resolve_dependencies`). Empty when
    /// dependency resolution was skipped (`--deps 0`) or found nothing.
    ///
    /// Capped at 3 matches per referenced name, ranked by path proximity to
    /// this symbol's own file (see `deps::resolve_dependencies`'s doc
    /// comment) — matches beyond the cap are counted, not dropped, in
    /// `omitted_dependency_matches`.
    pub dependencies: Vec<crate::deps::ResolvedSymbol>,
    /// Count of same-name candidate definitions that resolved but were cut
    /// by the top-3-per-name cap on `dependencies` (ADR 0003's name-only
    /// resolution can otherwise return many same-named matches for a common
    /// identifier). Zero when every match fit under the cap, dependency
    /// resolution was skipped (`--deps 0`), or nothing resolved.
    ///
    /// Serialized as `omitted_matches` (shorter, output-facing name) even
    /// though the Rust field name spells out "dependency" for clarity at
    /// the call site.
    #[serde(rename = "omitted_matches")]
    pub omitted_dependency_matches: usize,
    /// Whether this definition is test code by its AST context (ADR 0009),
    /// e.g. Rust's `#[cfg(test)]` modules and `#[test]`/`#[rstest]`/
    /// `#[tokio::test]`-attributed functions — see
    /// [`crate::language::LanguageSupport::is_test_definition`]. `false`
    /// for every language whose test convention is fully captured by file
    /// path alone (`is_test_definition`'s default), which is the common
    /// case; path-based detection happens at the file level in
    /// `pipeline.rs`, not here, since it does not depend on any individual
    /// node. An intermediate pipeline artifact, not part of rinkaku's
    /// output shape (test symbols are filtered out of `files` before a
    /// `Report` is built, see `pipeline::analyze_diff`), so excluded from
    /// serialization like `referenced_names`.
    #[serde(skip)]
    pub is_test: bool,
    /// This symbol's contract impact (ADR 0014), or `None` when no base-side
    /// content was available to classify against (see
    /// [`crate::pipeline::analyze_diff`]'s `read_base_file` parameter).
    /// Populated by [`crate::pipeline::classify_symbols`], a pipeline stage
    /// that runs after extraction — `None` here at construction time, same
    /// as `dependencies`/`id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub classification: Option<Classification>,
    /// The base-side symbol's comment-stripped, normalized signature, only
    /// when [`Classification::SignatureChanged`] — lets renderers show a
    /// before/after diff of the signature text itself. `None` for every
    /// other classification (including `None` classification) since there
    /// is nothing meaningful to show otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_signature: Option<String>,
}

/// Extracts the signatures of definitions that contain at least one
/// changed line. A changed line that isn't inside any definition (e.g. a
/// top-level statement) is not surfaced — v1 only reports symbol-level
/// changes.
pub fn extract_changed_symbols(
    source: &str,
    lang: &dyn LanguageSupport,
    changed_ranges: &[LineRange],
) -> Vec<ExtractedSymbol> {
    if changed_ranges.is_empty() {
        return Vec::new();
    }

    with_definition_nodes(source, lang, |all_nodes, source_bytes, reference_query| {
        let touched_nodes: Vec<DefinitionNode> = all_nodes
            .iter()
            .copied()
            .filter(|node| overlaps_any(node.line_range(), changed_ranges))
            .collect();
        let touched_context = TouchedContext {
            all_definition_nodes: all_nodes,
            changed_ranges,
        };

        touched_nodes
            .iter()
            .filter(|node| {
                // Prefer the narrowest enclosing definition: a touched node
                // that itself contains another touched node (e.g. an
                // `impl_item`/`class_definition` containing a touched method,
                // or a Python function containing a touched nested function)
                // is suppressed as a symbol in its own right — otherwise a
                // single changed line would surface both the inner definition
                // and every definition enclosing it. Go's `method_declaration`
                // is exempt implicitly: it is never nested inside its receiver
                // struct's node (see `find_container`'s doc comment), so this
                // situation cannot arise for Go structs.
                !touched_nodes
                    .iter()
                    .any(|other| other.node != node.node && is_descendant_of(other.node, node.node))
            })
            .flat_map(|node| {
                build_symbols(
                    *node,
                    source_bytes,
                    reference_query,
                    lang,
                    Some(touched_context),
                )
            })
            .filter(|symbol| overlaps_any(symbol.range, changed_ranges))
            .collect()
    })
}

/// Extracts every definition in `source`, regardless of whether it
/// changed. Used by [`crate::deps::TagsResolver`] to build a repo-wide
/// name-to-signature index: dependency resolution needs to look up
/// definitions in files that were not part of the diff at all, so it
/// cannot reuse `extract_changed_symbols`, which only ever reports
/// definitions overlapping a given set of changed ranges.
///
/// Unlike `extract_changed_symbols`, nested definitions are not
/// suppressed in favor of their narrowest enclosing one — an index needs
/// every definition, and a nested definition's own `container` (set by
/// `build_symbol`/`find_container`) already records its relationship to
/// its enclosing block, so there is nothing to suppress.
pub fn extract_all_symbols(source: &str, lang: &dyn LanguageSupport) -> Vec<ExtractedSymbol> {
    with_definition_nodes(source, lang, |all_nodes, source_bytes, reference_query| {
        all_nodes
            .iter()
            .flat_map(|node| build_symbols(*node, source_bytes, reference_query, lang, None))
            .collect()
    })
}

/// A symbol present on the base side of a diff but absent (by name and
/// container) from the head side — ADR 0014's `removed` classification,
/// reported separately from `ExtractedSymbol` since a removed symbol has no
/// head-side signature, range, or dependencies to speak of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemovedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub path: String,
    /// The base-side symbol's comment-stripped, normalized signature —
    /// the same text that would have been `previous_signature` on the head
    /// symbol had one still existed to attach it to.
    pub signature: String,
}

/// Classifies every symbol in `head_symbols` by contract impact (ADR 0014),
/// setting its `classification`/`previous_signature` in place, and returns
/// the base-side symbols this file had that no longer exist on the head
/// side at all (`removed`).
///
/// Matches head and base symbols within the same file by `(name,
/// container)` — the same identity `graph::collect_nodes` uses for a
/// symbol's stable id, one file at a time rather than by any cross-file
/// index, since ADR 0014 only classifies a *changed* file's own symbols
/// against that same file's base content.
///
/// - A head symbol with no base-side match at all → [`Classification::Added`].
/// - A head symbol with a base-side match whose comment-stripped,
///   normalized signature differs → [`Classification::SignatureChanged`],
///   with `previous_signature` set to the base signature.
/// - A head symbol with a base-side match whose signature is identical →
///   [`Classification::BodyOnly`].
/// - A base symbol absent from `head_file_identities` (i.e. gone from the
///   head file entirely, not merely absent from `head_symbols`), whose
///   base-side range overlaps `old_changed_ranges` (the diff's old-side
///   hunk ranges for this file) → returned as a [`RemovedSymbol`]. A
///   base-only symbol *outside* every changed range is not reported:
///   nothing in the diff actually touched it, so it is unrelated to this
///   change (e.g. a symbol that merely moved later in the file because of
///   an unrelated edit above it) — restricting to overlapping ranges is
///   what keeps this from flooding output on a diff that only touches a
///   small part of a large file.
///
/// `all_head_symbols` is deliberately a separate parameter from
/// `head_symbols`: `head_symbols` only carries the *narrowest* enclosing
/// definition touched by the diff (`extract_changed_symbols` suppresses a
/// container whose own member was the thing actually touched, so the
/// Change graph doesn't report both), so a still-alive container would
/// never appear in `head_symbols` when only one of its members changed —
/// checking removal against `head_symbols` alone would misreport that
/// container as removed. `all_head_symbols` instead is the head file's
/// *complete* symbol set (typically `extract_all_symbols`'s output), so a
/// container's continued existence is judged against the whole file, not
/// against the subset the diff happens to surface.
///
/// Pure: takes both sides' already-extracted symbol lists and matches them
/// in memory, no IO. `lang` is not needed here — `head_symbols` and
/// `base_symbols` are both already the output of `extract_changed_symbols`/
/// `extract_all_symbols`, whose signatures are already comment-stripped
/// (ADR 0014). Signatures are compared with [`normalize_for_comparison`]
/// applied to both sides (ADR 0060) rather than as plain strings: a
/// signature now keeps its original line structure for display, so a
/// reflow-only difference (indentation, line wrapping) must be normalized
/// away here to avoid a false `SignatureChanged`, same as ADR 0014 already
/// required before display signatures went multi-line.
///
/// A head symbol's own `signature` is not necessarily what gets compared:
/// a reported container's `signature` may be narrowed to only its touched
/// member lines (ADR 0071), which would never textually match a
/// same-container base symbol's always-whole-class signature even when
/// nothing else about the class actually changed. The comparison instead
/// looks up each head symbol's `(name, container)` identity in
/// `all_head_symbols` — the file's complete, un-narrowed symbol set
/// already threaded through for the removal check above — and compares
/// *that* signature, falling back to `symbol.signature` itself when no
/// `all_head_symbols` entry exists (defensive; every `head_symbols` entry
/// is expected to have one, since it is by construction a subset of the
/// same file's complete symbol set).
pub fn classify_symbols(
    head_symbols: &mut [ExtractedSymbol],
    base_symbols: &[ExtractedSymbol],
    all_head_symbols: &[ExtractedSymbol],
    old_changed_ranges: &[LineRange],
    path: &str,
) -> Vec<RemovedSymbol> {
    let base_by_identity: HashMap<(&str, Option<&str>), &ExtractedSymbol> = base_symbols
        .iter()
        .map(|s| ((s.name.as_str(), s.container.as_deref()), s))
        .collect();
    let all_head_by_identity: HashMap<(&str, Option<&str>), &ExtractedSymbol> = all_head_symbols
        .iter()
        .map(|s| ((s.name.as_str(), s.container.as_deref()), s))
        .collect();

    for symbol in head_symbols.iter_mut() {
        let identity = (symbol.name.as_str(), symbol.container.as_deref());
        match base_by_identity.get(&identity) {
            None => {
                symbol.classification = Some(Classification::Added);
            }
            Some(base_symbol) => {
                let comparison_signature = all_head_by_identity
                    .get(&identity)
                    .map_or(symbol.signature.as_str(), |s| s.signature.as_str());
                if normalize_for_comparison(&base_symbol.signature)
                    == normalize_for_comparison(comparison_signature)
                {
                    symbol.classification = Some(Classification::BodyOnly);
                } else {
                    symbol.classification = Some(Classification::SignatureChanged);
                    symbol.previous_signature = Some(base_symbol.signature.clone());
                }
            }
        }
    }

    let head_file_identities: std::collections::HashSet<(&str, Option<&str>)> = all_head_symbols
        .iter()
        .map(|s| (s.name.as_str(), s.container.as_deref()))
        .collect();

    base_symbols
        .iter()
        .filter(|base_symbol| {
            let identity = (base_symbol.name.as_str(), base_symbol.container.as_deref());
            !head_file_identities.contains(&identity)
        })
        .filter(|base_symbol| overlaps_any(base_symbol.range, old_changed_ranges))
        .map(|base_symbol| RemovedSymbol {
            name: base_symbol.name.clone(),
            kind: base_symbol.kind,
            path: path.to_string(),
            signature: base_symbol.signature.clone(),
        })
        .collect()
}

/// Parses `source`, runs `lang`'s `definition_query` to find every
/// `@definition` node, widens each one's span to include any
/// decorator/attribute ([`DefinitionNode`], ADR 0073), and hands the
/// resulting list (plus the source bytes they borrow from, and a compiled
/// `reference_query`) to `f`. Node values borrow from the parsed tree, so
/// this scoped-callback shape — rather than returning `Vec<DefinitionNode>`
/// directly — keeps the tree alive exactly as long as needed without
/// leaking it or threading a `Tree` value out through every caller. Shared
/// by `extract_changed_symbols` and `extract_all_symbols`, which differ
/// only in how they filter/use the node list.
///
/// `reference_query` is compiled once here (file granularity) rather than
/// once per definition node: `Query::new` takes ~1ms, and a repo-wide
/// index (`deps::TagsResolver::new`) calls into this path once per file
/// but `build_symbol` used to be called once per *definition*, so
/// compiling inside `build_symbol` multiplied that cost by the file's
/// definition count — measured as several seconds of pure recompilation
/// overhead on a mid-sized repo (see the `--deps` performance note in
/// `deps.rs`).
fn with_definition_nodes<T>(
    source: &str,
    lang: &dyn LanguageSupport,
    f: impl FnOnce(&[DefinitionNode], &[u8], &tree_sitter::Query) -> T,
) -> T {
    // `source_for_parse` is line- and offset-preserving (see its doc
    // comment), so every downstream consumer — changed-range overlap,
    // signature slicing, reference collection — reads the rewritten text
    // at the same positions the raw file has.
    let source = lang.source_for_parse(source);
    let source = source.as_ref();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.grammar())
        .expect("LanguageSupport grammar must be loadable by tree-sitter");
    let tree = parser
        .parse(source, None)
        .expect("parsing a source string always produces a tree");

    let query = tree_sitter::Query::new(&lang.grammar(), lang.definition_query())
        .expect("LanguageSupport definition query must be valid");
    let definition_capture_index = query
        .capture_index_for_name("definition")
        .expect("definition query must have a @definition capture");
    let reference_query = tree_sitter::Query::new(&lang.grammar(), lang.reference_query())
        .expect("LanguageSupport reference query must be valid");

    let mut cursor = tree_sitter::QueryCursor::new();
    let source_bytes = source.as_bytes();
    let mut matches = cursor.matches(&query, tree.root_node(), source_bytes);

    let mut nodes = Vec::new();
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if capture.index == definition_capture_index {
                nodes.push(DefinitionNode::new(capture.node, lang));
            }
        }
    }
    f(&nodes, source_bytes, &reference_query)
}

/// Whether `node` is strictly nested inside `ancestor` in the syntax tree.
pub(super) fn is_descendant_of(node: tree_sitter::Node, ancestor: tree_sitter::Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent == ancestor {
            return true;
        }
        current = parent.parent();
    }
    false
}

/// Whether `range` shares at least one line with any range in `others`.
pub(super) fn overlaps_any(range: LineRange, others: &[LineRange]) -> bool {
    others
        .iter()
        .any(|other| range.start <= other.end && other.start <= range.end)
}

/// Builds every symbol a captured definition node yields. One node is
/// one symbol for every current kind; kinds that expand a single
/// captured node into several symbols (HCL `locals` blocks, one symbol
/// per attribute — ADR 0066) get their own branch here rather than in
/// `build_symbol`, whose single-`Option` shape they cannot fit.
/// `extract_changed_symbols` re-filters the expansion by changed-range
/// overlap — a no-op for single-node symbols, whose range is the
/// already-touched captured node's own.
fn build_symbols(
    definition: DefinitionNode,
    source: &[u8],
    reference_query: &tree_sitter::Query,
    lang: &dyn LanguageSupport,
    touched: Option<TouchedContext>,
) -> Vec<ExtractedSymbol> {
    let node = definition.node;
    if node.kind() == "block" && hcl_block_type(node, source).as_deref() == Some("locals") {
        return build_hcl_locals_symbols(node, source, reference_query, lang);
    }

    build_symbol(definition, source, reference_query, lang, touched)
        .into_iter()
        .collect()
}

/// Builds an [`ExtractedSymbol`] from a captured definition node, or
/// `None` if the node kind isn't one this module knows how to report
/// (defensive default for query/grammar drift, not expected in practice
/// given `definition_query` only captures known kinds).
fn build_symbol(
    definition: DefinitionNode,
    source: &[u8],
    reference_query: &tree_sitter::Query,
    lang: &dyn LanguageSupport,
    touched: Option<TouchedContext>,
) -> Option<ExtractedSymbol> {
    let node = definition.node;
    let kind = symbol_kind(node, source)?;
    let name = definition_name(node, source)?;
    let signature = slice_signature(definition, source, touched);
    let container = find_container(node, source);
    let references = collect_referenced_names(node, source, reference_query);
    let is_test = lang.is_test_definition(node, source);

    Some(ExtractedSymbol {
        // Populated later by `graph::build_graph`, once node IDs are
        // assigned across the whole diff (see the field's doc comment).
        id: String::new(),
        name,
        kind,
        signature,
        range: definition.line_range(),
        container,
        referenced_names: references.bare,
        referenced_method_names: references.method,
        // Populated later by `deps::resolve_dependencies`, once the full
        // set of a file's extracted symbols is known (needed to exclude
        // diff-internal symbols from the resolved dependency list).
        dependencies: Vec::new(),
        omitted_dependency_matches: 0,
        is_test,
        // Populated later by `pipeline::classify_symbols`, which needs the
        // base-side content this function has no access to.
        classification: None,
        previous_signature: None,
    })
}

/// Maps a captured definition node to a language-neutral [`SymbolKind`].
/// Node kind strings are matched flat across every supported grammar;
/// kinds that collide across grammars (`block` also names ordinary
/// braced blocks in the Rust/Go/Python grammars) are safe because this
/// function only ever receives nodes captured by some language's
/// `definition_query`, and only HCL's query captures `block` (ADR
/// 0066). The source bytes are here for kinds whose classification
/// needs identifier text rather than node shape alone.
///
/// Takes the node rather than just its kind string because Go's
/// `type_spec` needs to inspect its `type` field to tell a struct from an
/// interface — the definition query captures `type_spec` for both (see
/// `language/go.rs`), so the node kind alone is ambiguous for Go.
fn symbol_kind(node: tree_sitter::Node, source: &[u8]) -> Option<SymbolKind> {
    match node.kind() {
        // Rust.
        "function_item" | "function_signature_item" => Some(SymbolKind::Function),
        "struct_item" => Some(SymbolKind::Struct),
        "enum_item" => Some(SymbolKind::Enum),
        "trait_item" => Some(SymbolKind::Trait),
        // Go.
        "type_spec" => match node.child_by_field_name("type")?.kind() {
            "struct_type" => Some(SymbolKind::Struct),
            "interface_type" => Some(SymbolKind::Interface),
            _ => None,
        },
        "function_declaration" => Some(SymbolKind::Function),
        "method_declaration" => Some(SymbolKind::Function),
        // Python.
        "class_definition" => Some(SymbolKind::Class),
        "function_definition" => Some(SymbolKind::Function),
        // TypeScript.
        "interface_declaration" => Some(SymbolKind::Interface),
        "type_alias_declaration" => Some(SymbolKind::TypeAlias),
        "class_declaration" | "abstract_class_declaration" => Some(SymbolKind::Class),
        "method_definition" | "abstract_method_signature" => Some(SymbolKind::Function),
        "enum_declaration" => Some(SymbolKind::Enum),
        // `variable_declarator` is captured only for `const f = () => {}`
        // style arrow-function bindings (see the TypeScript definition
        // query); other declarators are never captured.
        "variable_declarator" => Some(SymbolKind::Function),
        // PHP. Its other captured kinds reuse strings already mapped
        // above: `function_definition` (Python), `method_declaration`
        // (Go), `class_declaration`/`interface_declaration`/
        // `enum_declaration` (TypeScript) — the flat matching here is
        // per-captured-node, and only each language's own
        // `definition_query` decides what gets captured, so the sharing
        // is safe (same reasoning as HCL's `block` below).
        "trait_declaration" => Some(SymbolKind::Trait),
        // HCL (ADR 0066): every definition is a `block`; the block-type
        // keyword decides whether it is reported. `locals` blocks are
        // expanded per attribute in `build_symbols` instead.
        "block" => match hcl_block_type(node, source).as_deref() {
            Some("resource" | "data" | "module" | "variable" | "output" | "provider") => {
                Some(SymbolKind::Block)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Extracts a definition's declared name.
///
/// Most kinds expose their name through a `name` field
/// (`type_identifier`/`identifier`/`field_identifier`/...), which is
/// uniform across all grammars this module supports. `type_spec` (Go) is
/// the only kind that needs special handling: it is technically named via
/// its own `name` field too, so the generic path already covers it — kept
/// as a fallthrough rather than a special case.
fn definition_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    if node.kind() == "block" {
        return hcl_block_name(node, source);
    }

    node.child_by_field_name("name")
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())
}

/// Walks up from `node` to find an enclosing container (Rust
/// `impl_item`/`trait_item`, Go method receiver type, Python/TypeScript
/// `class_definition`/`class_declaration`), returning a descriptive
/// container name (e.g. `"impl Foo"`, `"trait Bar"`, `"Repo"`, `"class
/// Point"`). Returns `None` for top-level definitions.
///
/// Go is handled differently from the rest: a `method_declaration` is never
/// nested inside its receiver type's node (see `is_container_only_node`),
/// so its container is read directly off its own `receiver` field rather
/// than by walking ancestors.
fn find_container(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // A Go method carries its receiver on the node itself; a PHP
    // `method_declaration` (same node kind name, different grammar) has
    // no `receiver` field, so it falls through to the ancestor walk and
    // picks up its enclosing class/interface/trait/enum below.
    if node.kind() == "method_declaration"
        && let Some(receiver) = go_receiver_type_name(node, source)
    {
        return Some(receiver);
    }

    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "impl_item" => {
                let type_name = candidate
                    .child_by_field_name("type")
                    .and_then(|n| n.utf8_text(source).ok())?;
                return Some(format!("impl {type_name}"));
            }
            "trait_item" => {
                let name = definition_name(candidate, source)?;
                return Some(format!("trait {name}"));
            }
            "class_definition" | "class_declaration" | "abstract_class_declaration" => {
                let name = definition_name(candidate, source)?;
                return Some(format!("class {name}"));
            }
            // PHP nests captured `method_declaration` definitions inside
            // all three of these; TypeScript shares the node kind names
            // but never captures a definition nested inside its
            // interface/enum bodies, so the arms are unreachable there.
            "trait_declaration" => {
                let name = definition_name(candidate, source)?;
                return Some(format!("trait {name}"));
            }
            "interface_declaration" => {
                let name = definition_name(candidate, source)?;
                return Some(format!("interface {name}"));
            }
            "enum_declaration" => {
                let name = definition_name(candidate, source)?;
                return Some(format!("enum {name}"));
            }
            _ => current = candidate.parent(),
        }
    }
    None
}

/// Extracts the receiver type name from a Go `method_declaration`'s
/// `receiver` field (a `parameter_list` containing one
/// `parameter_declaration`), stripping the leading `*` for pointer
/// receivers so `func (r *Repo) Save(...)` and `func (r Repo) Save(...)`
/// both report container `"Repo"`.
fn go_receiver_type_name(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let receiver = node.child_by_field_name("receiver")?;
    let mut cursor = receiver.walk();
    let param = receiver
        .named_children(&mut cursor)
        .find(|c| c.kind() == "parameter_declaration")?;
    let type_node = param.child_by_field_name("type")?;
    let type_text = type_node.utf8_text(source).ok()?;
    Some(type_text.trim_start_matches('*').to_string())
}

#[cfg(test)]
#[path = "../extract_tests/mod.rs"]
mod tests;

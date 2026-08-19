//! 1-hop dependency resolution (ADR 0003).
//!
//! [`Resolver`] is the port through which the pipeline resolves a symbol's
//! referenced names (see [`crate::extract::ExtractedSymbol::referenced_names`])
//! into the definitions they point to, if any exist in the repository.
//! [`TagsResolver`] is the v1 implementation: an approximate, syntactic
//! resolver built on the same tree-sitter definition queries used for
//! extraction, with no type information. LSP-backed resolvers (pyright,
//! gopls, rust-analyzer, ...) are a future, opt-in `Resolver` impl that can
//! be plugged in without reshaping the pipeline.
//!
//! Performance: `TagsResolver::new` indexes every file `main.rs` passes
//! it (all of `git ls-files`, not just the diff). Two costs used to
//! dominate `--deps 1`'s wall-clock time:
//! - Query compilation (`Query::new`) ran once per *definition* rather
//!   than once per *file* (fixed; see `extract::with_definition_nodes`'s
//!   doc comment).
//! - Every indexed file was parsed
//!   ([`crate::extract::extract_all_symbols`]) even though most files in
//!   a real repository define nothing any changed symbol actually
//!   references. `TagsResolver::new`'s `reference_names` parameter fixes
//!   this: files are prefiltered by a substring search (`aho-corasick`,
//!   run once per language actually present rather than once per name)
//!   before parsing, skipping the ones that cannot contain a match at
//!   all — see `should_parse_file` and
//!   [`LanguageSupport::index_prefilter_patterns`] for why this cannot
//!   miss a real match (no recall loss).
//!
//! Remaining `--deps 1` overhead in `--base` mode is mostly the
//! `git show`/`git ls-files` subprocess cost of *reading* every indexed
//! file's content (one `git` invocation per file), which the prefilter
//! above does not reduce — it only skips parsing, not reading, since
//! whether a file's content matches can only be known after reading it.
//! Not addressed here, since it is `main.rs`'s file-reading strategy
//! rather than this module's indexing logic.
//!
//! Known limitation, and how ADR 0080 narrowed it: the original
//! prefilter matched `reference_names` as plain substrings anywhere in a
//! file's raw content — safe (a definition's name always appears
//! literally in its own declaration) but coarse. On a same-language
//! repository with all-generic-noise filtered `reference_names` (no
//! `Vec`/`Option`/`String`/...), that plain-substring version still cut
//! ~88% of files from parsing (~8x faster indexing) — but when
//! `reference_names` includes common standard-library-style names (as a
//! typical Rust diff's referenced names often do — `Vec`, `Option`,
//! `Some`, `Ok`, `String`, ...), or, more sharply, a helper *called* from
//! nearly every file in a monorepo (PHP/Laravel's `format_price`-style
//! utilities are the motivating case), a plain-substring match degrades
//! toward matching almost every file — one measured real-world diff saw
//! 93% of files pass. ADR 0080 replaces the plain name with
//! [`LanguageSupport::index_prefilter_patterns`]: for languages where a
//! definition's name is provably introduced by a fixed keyword
//! (PHP's `function`/`class`/..., Python's `def`/`class`, Rust's
//! `fn`/`struct`/..., Go's `func`/`type`/...), the prefilter now matches
//! *declaration-shaped* substrings (`"function helper"`, not just
//! `"helper"`) against a whitespace-normalized copy of the content — a
//! *call* site (`format_price($x)`) no longer makes a file pass the
//! filter, only a plausible *declaration* does. Languages where no
//! node kind can be proven complete this way (the TypeScript family, via
//! its default) keep the original bare-name behavior unchanged, and so
//! does HCL (its zero-recall-loss story runs through dotted-name
//! component expansion instead, unrelated to this mechanism) — for
//! those, this known limitation is unchanged from before this ADR.

use crate::extract::extract_all_symbols;
use crate::language::LanguageSupport;
use crate::progress::{OnProgress, should_report_progress};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The per-symbol projection of [`crate::extract::ExtractedSymbol`] that
/// [`TagsResolver::from_entries`] needs to rebuild an index entry, without
/// the extraction-only fields (`id`, `range`, `referenced_names`, ...)
/// that only matter while a diff's own changed symbols are being
/// classified. Exists so a caller can persist a repository-wide index
/// (rinkaku's `rinkaku/src/deps_cache.rs`, ADR 0079) across runs and
/// rebuild a [`TagsResolver`] from the persisted data alone, without
/// re-parsing every unchanged file — `derive(Serialize, Deserialize)`
/// is what that cache round-trips through disk as JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub signature: String,
    pub container: Option<String>,
    /// Mirrors [`crate::extract::ExtractedSymbol::is_test`] — see its own
    /// doc comment. [`TagsResolver::from_entries`]'s `include_tests`
    /// parameter filters on this per entry, the same way [`TagsResolver::new`]
    /// filters on [`crate::extract::ExtractedSymbol::is_test`] while
    /// building its index from freshly extracted symbols.
    pub is_test: bool,
}

/// A definition found by a [`Resolver`] for a referenced name. Reported
/// verbatim in [`crate::extract::ExtractedSymbol::dependencies`], so it is
/// part of rinkaku's output shape (unlike `referenced_names`) and derives
/// `Serialize`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedSymbol {
    pub signature: String,
    /// Path of the file the definition lives in, as provided to the
    /// resolver's file source (e.g. `TagsResolver::new`'s `files`).
    pub path: String,
    /// The enclosing impl/trait/class block's descriptive name, mirroring
    /// `ExtractedSymbol::container`. Carried through so `resolve_dependencies`
    /// can key its diff-internal/self-reference exclusions on `(name,
    /// container, path)`, matching how `extract::classify_symbols` already
    /// identifies a symbol — a name and path alone collide whenever a diff
    /// adds a same-named member under a different container (e.g. a new
    /// class defining a method that shares its name with an unrelated
    /// top-level definition).
    pub container: Option<String>,
}

/// Resolves a referenced name (a called function, a referenced type, ...)
/// to the definition(s) it points to in the repository, if any.
///
/// Returns every matching definition rather than a single one: v1's
/// [`TagsResolver`] matches by name alone, with no type information to
/// disambiguate overloads or same-named symbols in different
/// modules/packages, so more than one match is a normal, expected outcome
/// rather than an error condition. Callers decide how to present multiple
/// matches (e.g. list them all under "Depends on").
pub trait Resolver {
    fn resolve(&self, name: &str) -> Vec<ResolvedSymbol>;
}

/// v1 [`Resolver`]: builds a name-to-definition index by parsing every
/// supported file handed to it via [`TagsResolver::new`] with the same
/// tree-sitter `definition_query` used for extraction, then resolves by
/// exact name match.
///
/// Approximate by construction (ADR 0003): no type information means a
/// name match cannot distinguish overloads, shadowed names, or same-named
/// symbols in unrelated modules — all definitions sharing a name are
/// returned, not just the "right" one.
pub struct TagsResolver {
    index: HashMap<String, Vec<ResolvedSymbol>>,
}

impl TagsResolver {
    /// Builds the resolver's index eagerly from `files`: `(path, content)`
    /// pairs for every file the resolver should be able to resolve
    /// definitions from. Files are provided rather than discovered here so
    /// this module stays pure (no filesystem/`git` access) — `main.rs`
    /// supplies the real file list via `git ls-files`, tests supply an
    /// in-memory list.
    ///
    /// `reference_names` is the full set of names any changed symbol in
    /// the diff actually references (gathered by `main.rs` before calling
    /// this). A file is only parsed if a whitespace-normalized copy of its
    /// content contains, as a substring, at least one of the patterns its
    /// language's [`LanguageSupport::index_prefilter_patterns`] returns for
    /// one of these names (ADR 0080) — see that method's doc comment and
    /// `should_parse_file`'s for why this prefilter cannot cause a real
    /// definition to be missed. Passing an empty set (no diff, or
    /// `--deps 0`'s caller never reaching this path) indexes nothing,
    /// which is correct: no name is referenced, so no definition needs to
    /// be found.
    ///
    /// `include_tests` mirrors `pipeline::analyze_diff`'s flag of the same
    /// name (ADR 0009's mechanism; ADR 0025 flipped the CLI-facing default
    /// to include tests and renamed the flag to `--exclude-tests`),
    /// extended to this repo-wide index: `false` (the CLI's
    /// `--exclude-tests`) excludes test symbols the same two ways
    /// `analyze_diff` does — a whole file `language.is_test_path`
    /// considers a test file is skipped entirely, and within every other
    /// file, only symbols [`crate::extract::extract_all_symbols`] marked
    /// `ExtractedSymbol::is_test` (AST context, e.g. Rust's `#[cfg(test)]`)
    /// are dropped from indexing. Without this, a changed production
    /// symbol's `referenced_names` could resolve to a same-named test
    /// helper/fixture elsewhere in the repo — a name match a reviewer
    /// would almost always read as coincidental noise in "Depends on:",
    /// not a real dependency, since production code should not actually
    /// depend on test-only definitions (see ADR 0009's Consequences).
    /// `true` (the CLI's new default) indexes every symbol, matching
    /// `analyze_diff`'s own `include_tests: true` behavior.
    ///
    /// `generated_paths` and `include_generated` extend the same exclusion
    /// principle to generated files (ADR 0010/0011's Consequences): a
    /// changed production symbol can just as easily reference a type
    /// defined in a generated file (e.g. an ORM's model struct, dragging
    /// in every column/tag as "Depends on:" noise) as a test helper, and
    /// for the same reason — the reference is a coincidental name match a
    /// reviewer never asked to see, not a meaningful dependency signal.
    /// `generated_paths` is the caller-resolved `.gitattributes` set (ADR
    /// 0010, e.g. `main.rs`'s `git check-attr`, run once over every
    /// indexed path rather than the diff's changed paths — see
    /// `main.rs`'s `check_generated_paths_batch`), checked per-file the
    /// same way `is_test_path` is; on top of that, every file that reaches
    /// parsing is also checked with [`is_generated_content`] (ADR 0011),
    /// same as `analyze_diff`. `include_generated` (`false` = CLI default)
    /// gates both checks together, mirroring `--include-generated`'s
    /// effect on `analyze_diff`.
    ///
    /// Files with no registered [`LanguageSupport`] for their extension
    /// are silently skipped, matching the pipeline's handling of
    /// unsupported files elsewhere (`pipeline::analyze_diff`).
    ///
    /// `on_progress` (ADR 0033), when `Some`, is called with `(files_done,
    /// total)` as the parallel loop below finishes files (in completion
    /// order, same as `pipeline::analyze_repo`) — `files` is materialized
    /// into a `Vec` first (rather than iterated lazily) specifically so
    /// `total` is known up front the same way `pipeline::analyze_repo`'s
    /// `paths.len()` already is; every real call site already passes a
    /// `Vec<(String, String)>` (`main.rs`'s `build_resolver`), so this adds
    /// no new allocation. Reported approximately every
    /// [`crate::progress::PROGRESS_REPORT_STRIDE`] files
    /// (`crate::progress::should_report_progress`), always including a
    /// final `(total, total)` call. `None` (every non-`--tui` caller, every
    /// existing test) skips the counter entirely.
    pub fn new(
        files: impl IntoIterator<Item = (String, String)>,
        language_for_path: impl Fn(&str) -> Option<&'static dyn LanguageSupport> + Sync,
        reference_names: &HashSet<String>,
        include_tests: bool,
        generated_paths: &HashSet<String>,
        include_generated: bool,
        on_progress: Option<OnProgress>,
    ) -> Self {
        let files: Vec<(String, String)> = files.into_iter().collect();
        let total = files.len();

        // One matcher per registered-language *instance actually
        // encountered* in `files`, keyed by `LanguageSupport::name()`
        // (ADR 0080) — built once, single-threaded, before the parallel
        // parse below, since every language a file could route to is
        // already knowable from `files` and `language_for_path` alone.
        // A single shared matcher (the pre-ADR-0080 design) is no longer
        // possible: `index_prefilter_patterns` returns different pattern
        // shapes per language for the same `name` (e.g. PHP's `"function
        // helper"` vs. Rust's `"fn helper"`), so which patterns are valid
        // for a given file's content now depends on that file's language.
        //
        // A dotted reference name (HCL: `var.region`, ADR 0066) never
        // appears literally in its defining file — `variable "region"`
        // contains only the components. Adding each dot-separated
        // component's own `index_prefilter_patterns` preserves the
        // prefilter's zero-recall-loss guarantee (see `should_parse_file`)
        // exactly as the pre-ADR-0080 plain-component expansion did — for
        // HCL (the only language this arises for today, since it keeps
        // the bare-name default) a component's patterns are just the bare
        // component itself, unchanged behavior. Every component is
        // included, including single-character ones: a one-character
        // pattern makes the prefilter pass more files, which costs
        // parsing time, never recall.
        let mut matchers: HashMap<&'static str, aho_corasick::AhoCorasick> = HashMap::new();
        for (path, _content) in &files {
            let Some(lang) = language_for_path(path) else {
                continue;
            };
            matchers.entry(lang.name()).or_insert_with(|| {
                let mut patterns: Vec<String> = Vec::new();
                for name in reference_names {
                    patterns.extend(lang.index_prefilter_patterns(name));
                    if name.contains('.') {
                        for component in name.split('.') {
                            patterns.extend(lang.index_prefilter_patterns(component));
                        }
                    }
                }
                // `AhoCorasick::new` only errors on pathological inputs
                // this call site cannot produce: an empty pattern set is
                // handled gracefully (matches nothing, not an error), and
                // the automaton construction itself only fails on
                // internal overflow at pattern counts/lengths far beyond
                // what a diff's expanded reference-name patterns could
                // realistically reach. `.expect()` here documents "this
                // is not expected to fail in practice" rather than a
                // genuinely handled error path — there is no meaningful
                // fallback if it somehow did (the resolver simply could
                // not be built).
                aho_corasick::AhoCorasick::new(&patterns)
                    .expect("index_prefilter_patterns must build a valid AhoCorasick matcher")
            });
        }

        // ADR 0031's reasoning applies here the same as in
        // `pipeline::analyze_repo`: the per-file body is embarrassingly
        // parallel (`extract_all_symbols` builds a fresh parser per call,
        // every filter reads borrowed state without mutation), and it
        // dominates `--deps 1`'s wall-clock time on a large repository —
        // so rayon fans the parse across CPU cores. Only the per-file
        // *extraction* is parallel; the index insertions below stay
        // sequential over `par_iter().collect()`'s source-order result, so
        // each name's candidate list keeps the exact insertion order the
        // sequential loop produced (`resolve_dependencies`'s stable-sort
        // tie-break depends on it — see its own doc comment).
        //
        // Progress counts files as they *finish*, in completion order, via
        // the same shared-`AtomicUsize` pattern `analyze_repo` uses (ADR
        // 0033): `fetch_add`'s pre-increment return is turned 1-indexed
        // with `+ 1`, and the counter is only touched when a callback is
        // actually present.
        let completed = AtomicUsize::new(0);
        let per_file: Vec<Option<(String, Vec<crate::extract::ExtractedSymbol>)>> = files
            .into_par_iter()
            .map(|(path, content)| {
                let outcome = (|| {
                    let lang = language_for_path(&path)?;
                    if !include_tests && lang.is_test_path(&path) {
                        return None;
                    }
                    if !include_generated && generated_paths.contains(&path) {
                        return None;
                    }
                    if !include_generated && is_generated_content(&content) {
                        return None;
                    }
                    // Every language reachable from `lang`'s own
                    // `language_for_path(&path)` above already has a
                    // matcher, built by the identical lookup in the
                    // single-threaded pass above `files.into_par_iter()`
                    // — `.expect()` documents that invariant rather than
                    // handling a case that cannot occur.
                    let matcher = matchers
                        .get(lang.name())
                        .expect("a matcher was built for every language encountered in `files`");
                    if !should_parse_file(matcher, &normalize_whitespace(&content)) {
                        return None;
                    }
                    Some(extract_all_symbols(&content, lang))
                })();

                if let Some(on_progress) = on_progress {
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if should_report_progress(done, total) {
                        on_progress(done, total);
                    }
                }

                outcome.map(|symbols| (path, symbols))
            })
            .collect();

        let mut index: HashMap<String, Vec<ResolvedSymbol>> = HashMap::new();
        for (path, symbols) in per_file.into_iter().flatten() {
            for symbol in symbols {
                if !include_tests && symbol.is_test {
                    continue;
                }
                index.entry(symbol.name).or_default().push(ResolvedSymbol {
                    signature: symbol.signature,
                    path: path.clone(),
                    container: symbol.container,
                });
            }
        }

        Self { index }
    }

    /// Builds the resolver's index directly from already-extracted
    /// [`IndexEntry`] data, keyed by the path each entry's file was
    /// extracted from — the counterpart to [`TagsResolver::new`] for a
    /// caller that already has entries on hand (a persisted cache, ADR
    /// 0079) and wants to skip re-reading/re-parsing files whose content
    /// hasn't changed since they were extracted. Unlike `new`, this
    /// constructor does no filesystem/`git` work, no parsing, and no
    /// prefiltering — the caller decides which files' entries to include
    /// (e.g. by comparing git blob OIDs) before calling this.
    ///
    /// `include_tests` has the exact same meaning as `new`'s parameter of
    /// the same name: `false` drops every entry whose `is_test` is `true`
    /// from the index. Test-*file* exclusion (`LanguageSupport::is_test_path`)
    /// and generated-file exclusion are the caller's responsibility here —
    /// they are decided by which `(path, entries)` pairs are included in
    /// `entries` at all, since this constructor has no `LanguageSupport`
    /// or `.gitattributes` information to apply them itself.
    ///
    /// Insertion order into the index must match `new`'s exactly: entries
    /// are inserted sequentially in `entries`' iteration order, one
    /// `(name, container, path)` at a time — the same order `new`'s
    /// sequential post-parse loop produces from its rayon-parallel, then
    /// ordered-`collect`ed, per-file results. `resolve_dependencies`'s
    /// stable-sort tie-break (see its own doc comment) depends on this:
    /// candidates sharing a `path_proximity_rank` keep their relative
    /// order from `resolver.resolve(name)`, i.e. from `entries`' order. A
    /// caller that wants the same tie-break behavior as a fresh `new` call
    /// must therefore pass `entries` in the same path order `new` would
    /// have iterated `files` in (e.g. `git ls-files`'s lexicographic
    /// order), not an arbitrary order such as a `HashMap`'s iteration.
    pub fn from_entries(
        entries: impl IntoIterator<Item = (String, Vec<IndexEntry>)>,
        include_tests: bool,
    ) -> Self {
        let mut index: HashMap<String, Vec<ResolvedSymbol>> = HashMap::new();
        for (path, file_entries) in entries {
            for entry in file_entries {
                if !include_tests && entry.is_test {
                    continue;
                }
                index.entry(entry.name).or_default().push(ResolvedSymbol {
                    signature: entry.signature,
                    path: path.clone(),
                    container: entry.container,
                });
            }
        }
        Self { index }
    }
}

/// Whether `content` (already whitespace-normalized by
/// [`normalize_whitespace`]) could plausibly define something a changed
/// symbol references, based on a single `aho-corasick` pass over one
/// language's [`LanguageSupport::index_prefilter_patterns`] output for
/// every reference name at once (rather than one `str::contains` scan per
/// pattern).
///
/// This is a coarse substring test, not a symbol-aware one: it does not
/// verify a match sits at an actual definition site (vs., say, a
/// declaration-shaped substring that happens to appear inside a comment or
/// string literal, or — for languages still on the bare-name default —
/// any unrelated mention of the name at all). That imprecision is
/// deliberately accepted — the goal is only to decide whether parsing
/// `content` is worth attempting, and `extract_all_symbols` (the real,
/// syntax-aware definition finder) still runs afterward and is the only
/// thing that actually populates the index. Skipping a file here can
/// therefore never cause `resolve()` to miss a real definition, as long as
/// `matcher`'s patterns satisfy [`LanguageSupport::index_prefilter_patterns`]'s
/// contract (ADR 0080): every node kind the file's language's
/// `definition_query` could capture a definition named `name` from is
/// provably matched by at least one of that name's patterns, for every
/// name in the file's language's matcher — `TagsResolver::new` builds
/// exactly one matcher per encountered language enforcing this. For a
/// dotted reference name (HCL: `var.region`), the same contract is
/// satisfied per dot-separated component instead, which
/// `TagsResolver::new`'s pattern expansion covers — the prefilter can
/// only save work, not recall.
fn should_parse_file(matcher: &aho_corasick::AhoCorasick, content: &str) -> bool {
    matcher.is_match(content)
}

/// Collapses every maximal run of whitespace in `content` to a single
/// ASCII space, so [`should_parse_file`]'s declaration-anchored patterns
/// (e.g. PHP's `"function helper"`, ADR 0080) match regardless of how
/// many spaces, tabs, or newlines the real file puts between a keyword
/// and the name it introduces (`function\n    helper` still contains
/// `"function helper"` after normalization). A pure, dependency-free scan
/// rather than a regex — the whitespace classes involved
/// (`char::is_whitespace`) are exactly what `str::split_whitespace`/
/// `str::trim` already use elsewhere in the standard library, so this
/// mirrors an established, unsurprising definition of "whitespace" rather
/// than inventing a bespoke one.
///
/// Only used to build the copy of `content` fed into the prefilter match
/// (`TagsResolver::new`) — the real parse (`extract_all_symbols`) always
/// runs against the original, unmodified `content`, so normalization here
/// can never affect extracted signatures, byte ranges, or line numbers.
fn normalize_whitespace(content: &str) -> String {
    let mut normalized = String::with_capacity(content.len());
    let mut in_whitespace = false;
    for ch in content.chars() {
        if ch.is_whitespace() {
            if !in_whitespace {
                normalized.push(' ');
                in_whitespace = true;
            }
        } else {
            normalized.push(ch);
            in_whitespace = false;
        }
    }
    normalized
}

/// Number of leading lines checked by [`is_generated_content`] — mirrors
/// GitHub linguist's own "near the top of the file" scope for its
/// content-based generated-file heuristics (ADR 0011).
const GENERATED_MARKER_SCAN_LINES: usize = 5;

/// Whether `content`'s first [`GENERATED_MARKER_SCAN_LINES`] lines carry a
/// linguist-compatible generated-file marker (ADR 0011): a `@generated`
/// marker (Facebook-style, matched as a plain substring — deliberately not
/// narrowed further per the ADR's "don't overthink context around
/// `@generated`" decision), or a single line containing both `Code
/// generated` and `DO NOT EDIT` (Go tooling/protobuf's
/// `// Code generated by <tool>. DO NOT EDIT.` convention and its `#`-
/// commented equivalents — matched by substring rather than anchoring to a
/// specific comment syntax, since the comment marker itself varies by
/// language). Case-sensitive, matching linguist's own casing for these
/// exact markers.
///
/// A pure text check with no knowledge of `LanguageSupport`/comment syntax
/// by design (ADR 0011's rejected alternative: porting linguist's full
/// rule set) — deliberately a small, easily-audited subset rather than a
/// comprehensive port.
///
/// `pub` (not just `pub(crate)`): shared by `TagsResolver::new` (this
/// module, to exclude generated files from the repo-wide dependency index —
/// ADR 0010/0011's Consequences on dependency resolution),
/// `pipeline::analyze_diff` (to exclude them from the diff's own changed
/// symbols), and, across the `rinkaku_core`/`rinkaku` crate boundary,
/// `rinkaku`'s `deps_cache` module (ADR 0079), which must apply the exact
/// same content-marker check to a cache miss's freshly read content before
/// deciding whether to parse it and what to record as that file's
/// `is_generated` flag in the persisted cache. Lives here rather than in
/// `pipeline.rs` since `pipeline.rs` already imports from this module
/// (`Resolver`/`resolve_dependencies`); the reverse import would be a
/// cycle.
pub fn is_generated_content(content: &str) -> bool {
    content
        .lines()
        .take(GENERATED_MARKER_SCAN_LINES)
        .any(|line| line.contains("@generated") || is_code_generated_do_not_edit_line(line))
}

/// Whether `path` is a Terraform dependency lock file — generated by
/// `terraform init` and carrying no linguist-style content marker
/// [`is_generated_content`] could catch, so it is recognized by path
/// instead, mirroring linguist's own `.terraform.lock.hcl` rule (ADR
/// 0066). Matched as an exact basename so an unrelated file merely
/// ending in the same characters (`x.terraform.lock.hcl`) stays
/// unaffected. Its only production call site is `analyze_diff`'s skip
/// classification: no registered language suffix ever claims the lock
/// file, so indexing/outline sites already skip it as unsupported and
/// need no path check.
pub(crate) fn is_generated_lockfile_path(path: &str) -> bool {
    path == ".terraform.lock.hcl" || path.ends_with("/.terraform.lock.hcl")
}

/// Whether `line` contains both `Code generated` and `DO NOT EDIT` —
/// linguist's `^// Code generated .* DO NOT EDIT\.$` pattern, relaxed to a
/// same-line substring match on both phrases (see
/// [`is_generated_content`]'s doc comment for why the comment prefix and
/// trailing-period anchor are not checked).
fn is_code_generated_do_not_edit_line(line: &str) -> bool {
    line.contains("Code generated") && line.contains("DO NOT EDIT")
}

impl Resolver for TagsResolver {
    fn resolve(&self, name: &str) -> Vec<ResolvedSymbol> {
        self.index.get(name).cloned().unwrap_or_default()
    }
}

/// Populates every symbol's `dependencies` by resolving its
/// `referenced_names` and `referenced_method_names` through `resolver`,
/// across every file in the report — a symbol in one changed file may
/// reference a symbol changed in another, so exclusion is computed over
/// the whole diff, not per-file.
///
/// The two reference sets are container-filtered by different rules
/// (ADR 0068, extended here to dependency candidates per issue #227,
/// mirroring `graph::collect_edges`'s `ContainerRule`): a
/// `referenced_names` entry (a bare call/type reference) only keeps
/// candidates whose `container` is `None` (top-level) or equal to the
/// referencing symbol's own container, since a bare reference cannot
/// syntactically denote an arbitrary container's member in Python, Go,
/// or TypeScript. A `referenced_method_names` entry (a receiver-based
/// call or method-spec name) keeps candidates regardless of container,
/// since it is unambiguous about targeting a contained symbol.
/// Container-restricted-away candidates are dropped before ranking and
/// are not counted in `omitted_dependency_matches`, which is reserved
/// for candidates cut by [`MAX_MATCHES_PER_NAME`].
///
/// Two kinds of matches are deliberately excluded from the resulting
/// `dependencies`, both to avoid redundant noise rather than because they
/// are wrong:
/// - **Self-references**: a symbol's own declared name often appears in
///   its `referenced_names` (e.g. a struct's name is syntactically a type
///   reference inside its own definition — see the doc comment on
///   `LanguageSupport::reference_query`). Resolving it would just point
///   the symbol back at itself.
/// - **Diff-internal symbols**: if a resolved dependency matches another
///   symbol already reported in this same diff, it is already shown in
///   full elsewhere in the report; repeating it under "dependencies" adds
///   noise without adding information.
///
/// Matching for both exclusions is keyed on `(name, container, path)`, not
/// `(name, path)`: a `referenced_names` entry only carries a bare name, but
/// each candidate it resolves to (`ResolvedSymbol`) carries its own
/// `path`/`container`, so exclusion is checked per resolved candidate
/// rather than by filtering `referenced_names` up front. Name-only or
/// `(name, path)` matching would wrongly drop a dependency whenever the
/// diff happens to also touch an unrelated, same-named symbol:
/// - in a *different* file (e.g. a changed `a.rs::helper` coinciding with
///   the actual dependency target `b.rs::helper`) — see ADR 0003 for why
///   resolution itself stays name-based (no type info), but exclusion does
///   not need to inherit that imprecision.
/// - in the *same* file but a different container (e.g. the diff adds
///   `class Baz { fn Foo() }` to `a.rs`, which shares a name and path with
///   an actual dependency target `class Foo` defined elsewhere in
///   `a.rs`). Keying on `container` too — mirroring how
///   `extract::classify_symbols` already identifies a symbol — keeps these
///   apart.
///
/// Also caps same-name candidates at [`MAX_MATCHES_PER_NAME`] per
/// referenced name, ranked by [`path_proximity_rank`] so the kept matches
/// are the ones most likely relevant to the referencing symbol; the excess
/// count is reported via `ExtractedSymbol::omitted_dependency_matches`
/// rather than silently dropped.
///
/// Ranking uses `Vec::sort_by_key`, which is stable: candidates that tie on
/// `path_proximity_rank` (e.g. several same-directory matches) keep their
/// relative order from `resolver.resolve(name)`. For [`TagsResolver`] that
/// order is insertion order into its index, which follows the order of the
/// `files` iterator `TagsResolver::new` was built from — in practice
/// `main.rs`'s `git ls-files` output, i.e. lexicographic path order. This
/// tie-break is therefore an incidental consequence of `git ls-files`'s
/// ordering, not a deliberate ranking signal; a different `Resolver`
/// implementation or file source could change which of several
/// equally-close candidates survives the cap.
pub fn resolve_dependencies(
    files: Vec<crate::render::FileReport>,
    resolver: &dyn Resolver,
) -> Vec<crate::render::FileReport> {
    let diff_symbols: std::collections::HashSet<(String, Option<String>, String)> = files
        .iter()
        .flat_map(|file| {
            file.symbols.iter().map(move |symbol| {
                (
                    symbol.name.clone(),
                    symbol.container.clone(),
                    file.path.clone(),
                )
            })
        })
        .collect();

    files
        .into_iter()
        .map(|file| {
            let file_path = file.path.clone();
            crate::render::FileReport {
                path: file.path,
                symbols: file
                    .symbols
                    .into_iter()
                    .map(|mut symbol| {
                        let own_key = (
                            symbol.name.clone(),
                            symbol.container.clone(),
                            file_path.clone(),
                        );
                        let mut dependencies = Vec::new();
                        let mut omitted = 0usize;

                        for name in &symbol.referenced_names {
                            collect_candidates(
                                resolver,
                                name,
                                &own_key,
                                &diff_symbols,
                                &file_path,
                                ContainerRule::SameOrNone(symbol.container.as_deref()),
                                &mut dependencies,
                                &mut omitted,
                            );
                        }
                        for name in &symbol.referenced_method_names {
                            collect_candidates(
                                resolver,
                                name,
                                &own_key,
                                &diff_symbols,
                                &file_path,
                                ContainerRule::Any,
                                &mut dependencies,
                                &mut omitted,
                            );
                        }

                        symbol.dependencies = dependencies;
                        symbol.omitted_dependency_matches = omitted;
                        symbol
                    })
                    .collect(),
            }
        })
        .collect()
}

/// The container-matching rule [`collect_candidates`] applies to one
/// reference set, mirroring `graph::collect_edges`'s `ContainerRule`
/// (ADR 0068).
enum ContainerRule<'a> {
    /// A candidate only matches if its container is `None`, or equal to
    /// the referencing symbol's own container — the restriction bare
    /// references (`referenced_names`) are limited to.
    SameOrNone(Option<&'a str>),
    /// A candidate matches regardless of its container — the rule
    /// `referenced_method_names` entries use.
    Any,
}

/// Resolves `name` against `resolver`, applies `rule`'s container
/// restriction, then applies the shared self-reference/diff-internal
/// exclusion, proximity ranking, and [`MAX_MATCHES_PER_NAME`] cap —
/// appending survivors to `dependencies` and adding any capped-away
/// count to `omitted`.
///
/// Candidates dropped by `rule`'s container restriction are excluded
/// before this counting, not added to `omitted`: that count is reserved
/// for candidates cut by the cap, a distinct reason from "this
/// candidate's container makes it syntactically unreachable from a bare
/// reference".
#[allow(clippy::too_many_arguments)]
fn collect_candidates(
    resolver: &dyn Resolver,
    name: &str,
    own_key: &(String, Option<String>, String),
    diff_symbols: &std::collections::HashSet<(String, Option<String>, String)>,
    referencing_path: &str,
    rule: ContainerRule<'_>,
    dependencies: &mut Vec<ResolvedSymbol>,
    omitted: &mut usize,
) {
    let mut candidates: Vec<ResolvedSymbol> = resolver
        .resolve(name)
        .into_iter()
        .filter(|resolved| {
            let container_ok = match rule {
                ContainerRule::Any => true,
                ContainerRule::SameOrNone(referencing_container) => {
                    resolved.container.is_none()
                        || resolved.container.as_deref() == referencing_container
                }
            };
            if !container_ok {
                return false;
            }
            let key = (
                name.to_string(),
                resolved.container.clone(),
                resolved.path.clone(),
            );
            &key != own_key && !diff_symbols.contains(&key)
        })
        .collect();

    // Rank before truncating: the cap must keep the closest matches, not
    // an arbitrary prefix of whatever order the resolver happened to
    // return them in (see `path_proximity_rank`'s doc comment).
    candidates.sort_by_key(|resolved| path_proximity_rank(referencing_path, &resolved.path));

    if candidates.len() > MAX_MATCHES_PER_NAME {
        *omitted += candidates.len() - MAX_MATCHES_PER_NAME;
        candidates.truncate(MAX_MATCHES_PER_NAME);
    }
    dependencies.extend(candidates);
}

/// Maximum number of same-name candidate definitions kept per referenced
/// name. Beyond this, name-only resolution (ADR 0003) tends to surface
/// many equally-plausible-looking matches for common identifiers (e.g. a
/// `Config` struct defined in several unrelated packages) that add noise
/// rather than signal; 3 keeps the "Depends on" list skimmable while still
/// showing more than one candidate when genuinely ambiguous.
const MAX_MATCHES_PER_NAME: usize = 3;

/// Ranks how close `candidate_path` is to `referencing_path`, lower being
/// closer. Used to keep the most locally relevant matches when a
/// name-only resolver (ADR 0003) returns several same-named candidates,
/// since v1 has no type information to pick the syntactically "correct"
/// one — proximity in the repository's directory tree is used as a proxy
/// for "more likely to be the intended target", the same heuristic an
/// editor's "go to definition" fallback (or a human skimming candidates)
/// would reach for first.
///
/// Ranks, from closest to farthest:
/// 1. Same file as the referencing symbol.
/// 2. Same directory (immediate parent) as the referencing symbol.
/// 3. Shares a path prefix with the referencing symbol — ranked by *shared
///    prefix depth*, deeper (more path components in common) first, so a
///    common grandparent directory ranks closer than a common
///    great-grandparent.
/// 4. No shared directory prefix at all (other than the repository root).
///
/// Edge case: two files that both live directly at the repository root
/// (e.g. `"a.rs"` and `"b.rs"`, no `/` in the path) both have an empty
/// `path_dir_components` result and therefore rank as "same directory"
/// (rank 2), not "no shared prefix" (rank 4) — there is no directory
/// component to distinguish them by. This is a natural consequence of
/// treating the repository root as a directory like any other, not a
/// special case handled separately.
fn path_proximity_rank(
    referencing_path: &str,
    candidate_path: &str,
) -> (u8, std::cmp::Reverse<usize>) {
    if candidate_path == referencing_path {
        return (0, std::cmp::Reverse(usize::MAX));
    }

    let referencing_dir: Vec<&str> = path_dir_components(referencing_path);
    let candidate_dir: Vec<&str> = path_dir_components(candidate_path);

    if referencing_dir == candidate_dir {
        return (1, std::cmp::Reverse(usize::MAX));
    }

    let shared_depth = referencing_dir
        .iter()
        .zip(candidate_dir.iter())
        .take_while(|(a, b)| a == b)
        .count();

    if shared_depth > 0 {
        (2, std::cmp::Reverse(shared_depth))
    } else {
        (3, std::cmp::Reverse(0))
    }
}

/// Splits a `/`-separated repository-relative path into its directory
/// components, dropping the file name itself — e.g. `"src/pkg/a.rs"` →
/// `["src", "pkg"]`. Paths are always `/`-separated regardless of host OS:
/// they come from `git`, which normalizes separators, not from
/// `std::path` traversal of the local filesystem.
fn path_dir_components(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

#[cfg(test)]
#[path = "deps_tests/mod.rs"]
mod tests;

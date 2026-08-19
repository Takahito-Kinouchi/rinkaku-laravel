# 0080. Declaration-anchored prefilter patterns

Date: 2026-08-19

## Status

Accepted (extends ADR 0003, ADR 0066)

## Context

`TagsResolver::new` (`rinkaku-core/src/deps.rs`) prefilters every indexed
file with an `aho-corasick` substring search over the diff's
`reference_names` before parsing it (ADR 0003's tags-based resolver,
built for zero-setup 1-hop dependency expansion). The prefilter's own
module doc already documented a known limitation: when `reference_names`
includes common, ubiquitous-looking names, the fraction of files that
pass the filter climbs sharply — one measured real-world diff still had
93% of files pass, since those names appear in nearly every file as
plain text.

In a PHP/Laravel monorepo this limitation sharpens into the dominant
cost, not a corner case. A shared helper such as `format_price` is
*called* from nearly every controller/view/service file but *defined* in
exactly one. Because the prefilter matched `reference_names` as bare
substrings anywhere in a file's raw content, every calling file passed
the filter and was parsed, even though `extract_all_symbols` would find
no definition there. The prefilter's own safety argument — "a
definition's name always appears literally in its own declaration" — was
sound but too weak: it only proves the *definer*'s file passes, it does
nothing to keep *caller*-only files out.

ADR 0066 (HCL) already established that the prefilter's pattern set does
not have to be the bare name verbatim — it can be any pattern set proven
to match every file containing a real definition, via dotted-name
component expansion for HCL's `var.region`-style references. This ADR
generalizes that idea: instead of one dotted-name-aware expansion applied
uniformly to every language, each `LanguageSupport` gets to say, from its
own `definition_query`'s grammar shape, which *declaration-shaped*
substrings a definition named `name` must contain — narrowing "does this
file mention `helper` anywhere" to "does this file plausibly *declare*
`helper`".

## Decision

1. **`LanguageSupport` gains a defaulted method**,
   `index_prefilter_patterns(&self, name: &str) -> Vec<String>`,
   defaulting to `vec![name.to_string()]` (today's bare-name behavior,
   unchanged for every language that doesn't override it). Contract:
   patterns are matched, as substrings, against a
   **whitespace-normalized** copy of a candidate file's content (every
   maximal run of whitespace collapsed to one space — `deps::
   normalize_whitespace`, a small dependency-free scan, not a regex). An
   override MUST return a pattern set that matches every file in which
   this language's `definition_query` could capture a definition named
   `name`, checked separately for every captured node kind; when
   completeness cannot be proven for some node kind, the bare `name`
   must remain in the set. A false positive only costs parse time; a
   false negative is a correctness bug — a real dependency silently
   missing from "Depends on:" with no error.

2. **Four languages override it**, each verified against its
   `tree-sitter` grammar's actual rule shapes (not just its
   `DEFINITION_QUERY` string) to confirm the introducing keyword and the
   name are never separated by another token:
   - **PHP**: `["function {name}", "function &{name}", "class {name}",
     "interface {name}", "trait {name}", "enum {name}"]` —
     `function_definition`/`method_declaration` after `function`
     (optionally `&` for by-ref), `class_declaration` after `class`
     (modifiers precede the keyword, never sit between it and the
     name), `interface_declaration`/`trait_declaration`/
     `enum_declaration` after their own keyword directly.
   - **Python**: `["def {name}", "class {name}"]` — `function_definition`
     after `def` (an `async` modifier precedes `def`, not the name, so
     `async def` still matches `"def {name}"`), `class_definition` after
     `class` directly.
   - **Rust**: `["fn {name}", "struct {name}", "enum {name}",
     "trait {name}"]` — `function_item`/`function_signature_item` after
     `fn` (visibility and `async`/`const`/`unsafe`/`default` modifiers
     precede `fn`, never sit between it and the name),
     `struct_item`/`enum_item`/`trait_item` after their own keyword
     directly.
   - **Go**: `["func {name}", "type {name}", ") {name}("]` —
     `function_declaration` after `func`; `type_spec` (restricted to
     struct/interface types per its existing field predicate) after the
     enclosing `type_declaration`'s literal `type` keyword, which always
     precedes it in the file's text even though `type` is not itself
     part of the `type_spec` node; `method_declaration`'s name follows
     its receiver's closing `)` — Go permits exactly one receiver, so
     this is a reliable anchor for gofmt-conformant source (effectively
     all real-world Go). A contrived file inserting whitespace between
     the method name and its own parameter list (`func (r *Repo) Save
     (x int) {}`) is syntactically legal Go that this one pattern would
     not catch — accepted as a narrow, documented gap in the same spirit
     as `should_parse_file`'s own long-standing coarse-substring
     tradeoffs, rather than falling back to the bare name (which would
     make the whole override a no-op, since a superset pattern subsumes
     every narrower one).

3. **TypeScript (and TSX/Vue/Svelte, which reuse its
   `DEFINITION_QUERY`) keep the bare-name default**, explicitly, with a
   comment recording why: two captured node kinds have no fixed
   introducing keyword to anchor on — an arrow function bound via
   `variable_declarator` has its name *before* `=`, not after any
   keyword, and `method_definition` can be preceded by any combination
   of `async`/`static`/`get`/`set`/visibility/`override`/`readonly`
   modifiers in varying order. Per the contract in decision 1, one
   incomplete node kind means the whole language keeps the safe default.

4. **HCL keeps its existing behavior exactly**, also via the default:
   its definitions are undifferentiated top-level `block` nodes with
   block-type dispatch done in `extract.rs`, not distinct
   keyword-introduced node kinds — there is no "keyword + name" shape to
   anchor a pattern to the way PHP's `function`/Rust's `fn` are. HCL's
   zero-recall-loss story continues to come entirely from
   `TagsResolver::new`'s dotted-name component expansion (ADR 0066
   decision 5), which now routes each component through
   `index_prefilter_patterns` too — for HCL that is still just the bare
   component, so behavior is unchanged.

5. **`TagsResolver::new` builds one `aho-corasick` matcher per
   registered-language instance actually encountered in `files`**
   (keyed by `LanguageSupport::name()`, built once in a single-threaded
   pass before the rayon-parallel parse), rather than one matcher shared
   across every file — a single shared matcher is no longer possible
   once different languages return different pattern shapes for the same
   `name`. Each per-file check runs against a whitespace-normalized copy
   of that file's content; `extract_all_symbols` still always parses the
   original, unmodified content, so normalization cannot affect
   extracted signatures, byte ranges, or line numbers. The per-file
   filter order, parallel-parse structure, ordered index insertion, and
   progress reporting are otherwise unchanged (`parallel_determinism`
   pins this).

## Alternatives considered

- **Keep the plain-substring prefilter and accept the known
  limitation**: rejected — it is exactly what makes PHP/Laravel-style
  monorepos slow to index, the motivating case this ADR addresses.
- **A single global "declaration keyword" heuristic (e.g. always try
  `"function {name}"`, `"def {name}"`, `"fn {name}"`, ... regardless of
  the file's actual language)**: rejected — needlessly imprecise (every
  file gets checked against every language's keywords) and fragile to
  extend; per-language `LanguageSupport::index_prefilter_patterns`
  keeps the completeness argument local to the grammar it is about,
  the same locality principle ADR 0002 established for language support
  generally.
- **Require every language to override with a proven-complete pattern
  set, with no bare-name fallback**: rejected — TypeScript's arrow
  functions and modifier-heavy class methods genuinely have no provable
  anchor; forcing an override would either be wrong (recall loss) or a
  disguised bare-name pattern anyway. The defaulted trait method with an
  explicit completeness contract lets each language opt in only where
  it can prove correctness.
- **Scope-aware matching (only count a match inside an actual
  declaration position, via a lightweight scan) instead of substring
  patterns**: rejected as unnecessary complexity for a prefilter whose
  only job is deciding whether the *real* parse (`extract_all_symbols`)
  is worth attempting — a coarse, cheap substring test that still cuts
  the large majority of caller-only files is enough, and a scope-aware
  scanner would start to duplicate the tree-sitter parse it exists to
  avoid.

## Consequences

- The prefilter's effectiveness on PHP/Laravel-style monorepos — the
  motivating case — improves sharply: a helper called from most files
  but defined in one no longer makes every calling file pass the filter,
  narrowing "does `helper` appear anywhere" to "does this file plausibly
  *declare* `helper`". The magnitude is reasoned, not independently
  re-measured against the PR-description numbers cited in `deps.rs`'s
  module doc (no equivalent large PHP corpus was rebenchmarked for this
  change); the precision tests in `deps_tests/prefilter_patterns.rs`
  confirm the mechanism itself (caller-only content is rejected,
  definer content is accepted) rather than a specific percentage.
- Go's `) {name}(` method pattern has one documented, narrow gap
  (whitespace between a method name and its own parameter list) that a
  pathologically formatted — but syntactically legal — file could
  exploit to lose recall for that one file. Accepted rather than adding
  a bare-name fallback that would make the override a no-op.
- TypeScript/TSX/Vue/Svelte and HCL are unaffected in behavior; the
  known-limitation prose in `deps.rs`'s module doc still applies to them
  unchanged.
- `TagsResolver::new` now builds potentially several `aho-corasick`
  automatons (one per encountered language) instead of one; each is
  built once, single-threaded, before the parallel parse, so this adds a
  small, bounded amount of setup work — not per-file cost — in exchange
  for per-language pattern correctness.
- `should_parse_file`'s zero-recall-loss argument now depends on
  `LanguageSupport::index_prefilter_patterns`'s contract holding for
  every override, rather than being self-contained in `deps.rs` alone;
  adding a new language or extending an existing `definition_query` with
  a new captured node kind must re-check that contract (or fall back to
  the bare-name default) the same way ADR 0066 required for HCL.

## Addendum (2026-08-19): Go method gap closed

Because the matcher runs over whitespace-normalized content (every
whitespace run collapses to one space), adding the second anchor
`") {name} ("` makes Go method coverage complete for legal-but-
unformatted spacing between a method name and its parameter list —
the gap the original decision documented as accepted is now closed.

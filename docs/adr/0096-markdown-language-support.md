# 0096. Markdown language support

Date: 2026-09-02

## Status

Accepted

## Context

Documentation is a first-class part of the changes this tool reviews:
this repository requires an ADR for every structural decision, and a
Laravel/Vue PR routinely carries a README, a runbook, or an ADR
alongside the code. Today every `.md` file in a diff is unsupported —
`language_for_path` returns `None`, so the file reaches "Other changed
files" as a line count (ADR 0070) and nothing else. A reviewer reading
rinkaku's output learns that `docs/adr/0091-….md` changed by 40 lines,
but not whether the change was to *Status*, to *Decision*, or to a
single sentence of *Context*.

A document does have the structure rinkaku exists to surface. Its
headings are its outline (輪郭), the prose under a heading is that
heading's body, and heading nesting is containment — the same three
relations that make a function's signature, body, and enclosing class
worth distinguishing. So the question is not whether Markdown has an
API surface, but whether the existing model maps onto it without
distortion.

Two things about the ecosystem shape the answer:

- crates.io `tree-sitter-md` 0.5 (MIT, the tree-sitter-grammars org)
  binds via `tree-sitter-language`, the same mechanism as every existing
  grammar crate, and is the standard Markdown grammar. It ships two
  grammars — a *block* grammar (documents, sections, headings, lists,
  code fences) and an *inline* grammar (emphasis, links, code spans) —
  which parse independently.
- The block grammar's `section` node is heading-rooted: it starts at a
  heading and runs to the next heading of the same or higher level,
  nesting deeper sections inside it. It is, structurally, exactly a
  definition with a declaration and a body.

Two mismatches are real and had to be decided rather than papered over:

1. A block's line terminator belongs to the block, so a `section`'s
   tree-sitter end position is column 0 of the *next* section's heading
   line. Every existing grammar ends a definition on a brace or an
   expression, so `DefinitionNode::line_range` had never met a node
   whose end position names a row it does not occupy.
2. The grammar opens a `section` for ATX headings (`## Title`) only. A
   setext heading (`Title` over `-----`) stays a plain block among its
   siblings, with no subtree that could be called its body.

## Decision

1. **Markdown is a plain `LanguageSupport` impl**
   (`language/markdown.rs`), registered for `.md` and `.markdown`. The
   extraction pipeline is unchanged, per ADR 0002; the language-specific
   reads live in `extract/markdown.rs`, sibling to `extract/hcl.rs`,
   which does the same job for the one other language whose definitions
   are not addressed by a tree-sitter `name`/`body` field.

2. **The definition is the `section`, not the heading.** A section's
   heading is its signature and the prose under it is its body, so "the
   definition containing a changed line" means "the section the edit
   landed in", and `extract_changed_symbols`'s existing
   narrowest-enclosing-definition rule reports the deepest touched
   subsection rather than it and all of its ancestors.

3. **A new `SymbolKind::Section`**, rendered as `section` by every
   kind-label site. Heading level is not part of the kind: it is already
   visible in the signature (`## Test strategy`) and in the container
   label, and a section is the same sort of thing at every depth.

4. **The container is the enclosing section, labelled `section <parent
   heading>`** — the `keyword Name` shape `impl X`/`class X` already
   use. The keyword is deliberately *not* added to
   `graph::container_type_name`'s recognized list: a heading is prose,
   not a type, and a bare identifier reference should never link to the
   members of a document section that happens to share its wording (ADR
   0086).

5. **No reference query, and no contribution to the dependency index.**
   Markdown declares nothing another file can call, so `reference_query`
   is empty (a valid query with zero patterns) rather than an
   approximation over inline code spans, which name symbols only by
   coincidence of formatting. `contributes_to_dependency_index` returns
   `false` for the stronger reason: heading text collides with code
   (`Context`, `Usage`, `Overview`) far too readily for ADR 0003's
   name-only resolution to survive, so indexing documents would attach
   wrong "Depends on:" entries, not merely slow the scan down. This is
   a correctness gate, unlike PHP's Blade exclusion, which is a scan-cost
   trade-off (ADR 0078 addendum).

6. **A definition's line range ends on the last line it occupies.**
   `DefinitionNode::line_range` now treats a node whose end position is
   at column 0 as ending on the previous row. This is a general
   correction — an end position is exclusive, so column 0 means no
   character on that row was consumed — that no existing language
   exercises, and without it every Markdown section would claim the next
   section's heading line and both would be reported for a one-heading
   edit.

7. **Setext headings are captured on their own**, as a second
   alternative in the definition query, and reported as the heading
   alone: there is no section subtree to give them a body. Less than an
   ATX section gives, more than the silence of skipping them.

8. **`is_test_path` follows the conventional test-directory segment
   names** (`tests`, `Tests`, `test`, `__tests__`, `testdata`) rather
   than returning `false`. Markdown has no test-file naming convention
   to read, and those segments are exactly what
   `rinkaku-tui`'s `tests_section::is_test_dir_path` falls back to for a
   path with no registered language — which is what governed `.md` files
   until now. Keeping them means a document under `tests/` carries on
   routing to the Tests section instead of reviving a `tests/` node in
   the production tree beside it, the duplication ADR 0077 exists to
   remove.

9. **Only the block grammar is used.** Heading text is read as raw
   source bytes out of the block tree's `inline` node, so the inline
   grammar is never loaded. An ATX heading's optional closing `#`
   sequence (`## Title ##`) is stripped from the name, requiring the
   preceding space CommonMark itself requires, so that `## Why C#` keeps
   its hash.

## Alternatives considered

- **Capturing headings rather than sections**: rejected — a heading node
  spans only its own line, so an edit to the prose under a heading would
  report nothing at all, which is the majority of documentation edits
  and the whole reason to support Markdown.
- **Making the definition query field-addressed** (adding a `name`
  capture and reusing `extract`'s generic `child_by_field_name("name")`
  path): not possible — the block grammar gives a section neither a
  `name` nor a `body` field, which is why `extract/markdown.rs` exists at
  all.
- **Skipping setext headings entirely**: rejected — a document written
  in setext style would yield no symbols whatsoever, which reads as
  "rinkaku does not support this file" rather than as a grammar
  limitation.
- **Fixing the column-0 end position inside Markdown only** (a new
  `LanguageSupport` hook mirroring `definition_span_start`): rejected —
  the rule is language-neutral and provably safe (an exclusive end
  position at column 0 cannot occupy that row), so a per-language hook
  would add trait surface to encode something true of every grammar.
- **Indexing headings for dependency resolution**: rejected under
  decision 5.

## Consequences

- A documentation-only PR now produces a Change graph, a Definitions
  section, and Added/Removed classification (renaming a heading reads as
  one added and one removed section) instead of a single line count under
  "Other changed files".
- Markdown files enter `## File sizes` like any other analyzed file, so a
  long document can now surface a Watch/Warn/Split band (ADR 0028). The
  bands were calibrated for source readability; on a document they should
  be read as descriptive, not as a split mandate.
- Prose above a document's first heading, and a document with no headings
  at all, produce no symbols: the grammar wraps that content in a
  heading-less `section`, which has no name, so the file falls through to
  the non-symbol-changes path (ADR 0070) exactly as before.
- Two documents can share a heading name (`## Context` in every ADR).
  Cross-file collisions are already handled by dependency resolution
  never reaching Markdown (decision 5); within one file,
  `graph::collect_nodes`'s existing `@{start_line}` disambiguation
  applies unchanged.
- Whole-repository outline mode (`pipeline::analyze_repo`, ADR 0017's
  default when stdin is a terminal) now lists every heading in every
  tracked document — around a thousand more symbols in this repository
  alone, since `extract_all_symbols` keeps nested sections rather than
  collapsing them the way a diff's narrowest-enclosing rule does. They
  are grouped under their own files in the tree, and an outline of a
  repository arguably owes its documentation the same treatment as its
  code, so this is accepted rather than suppressed; a per-language
  opt-out would be a new hook for a case no one has asked for yet.
- The TUI's source-view syntax highlighting has no Markdown entry (its
  table is TUI-only by ADR 0018 and already omits PHP, Vue, Svelte, and
  HCL). A Markdown file's diff renders unhighlighted; adding it is
  independent of this decision.

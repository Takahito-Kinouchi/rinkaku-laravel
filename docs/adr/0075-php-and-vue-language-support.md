# 0075. PHP and Vue (SFC) language support

Date: 2026-08-19

## Status

Accepted

## Context

PHP/Vue codebases (a Laravel- or Symfony-style backend with a Vue
frontend) are a common stack this fork reviews daily, and neither
language routes anywhere in the registry today — their files land in
"Skipped files" as unsupported. ADR 0002 made language support additive:
a `LanguageSupport` impl plus a registry entry, with the extraction
pipeline unchanged. PHP fits that mold directly; Vue single-file
components do not, because an SFC is three languages in one file —
markup (`<template>`), CSS (`<style>`), and the `<script>` block(s) that
carry every symbol rinkaku extracts.

Grammar availability differs sharply between the two:

- crates.io `tree-sitter-php` 0.24 (MIT, tree-sitter org) binds via
  `tree-sitter-language`, the same mechanism as every existing grammar
  crate, and parses full `<?php ...` files with interleaved HTML.
- The Vue grammars on crates.io are unmaintained forks pinned to old
  tree-sitter ABIs, and even a maintained one would only re-delegate the
  script block to a JS/TS grammar for the parts rinkaku cares about.

## Decision

1. **PHP is a plain `LanguageSupport` impl** (`language/php.rs`,
   `LANGUAGE_PHP` — the tags-with-HTML variant, since real `.php` files
   mix markup) capturing `function_definition`, `method_declaration`,
   `class_declaration`, `interface_declaration`, `trait_declaration`,
   and `enum_declaration`. Most node kind strings are shared with
   grammars already mapped in `extract.rs`'s flat `symbol_kind` match
   (Python's `function_definition`, Go's `method_declaration`,
   TypeScript's `class_declaration`/`interface_declaration`/
   `enum_declaration`); only `trait_declaration` is new. Three
   extraction-side adjustments make the shared kinds behave for PHP:
   - `find_container` no longer hard-returns the Go receiver lookup for
     `method_declaration` — when the node has no `receiver` field (PHP),
     it falls through to the ancestor walk, which gains
     `trait_declaration`/`interface_declaration`/`enum_declaration`
     arms (unreachable for TypeScript, which never captures definitions
     nested inside those bodies).
   - `collect_method_body_ranges` treats `method_declaration` as a
     method (Go's same-named methods are never nested inside class-like
     nodes, so the walk never sees them).
   - `trait_declaration` and `enum_declaration` join the class-like
     signature-slicing branch, so PHP trait/enum method bodies are
     stripped the same way class method bodies already are. A
     TypeScript enum also lands in that branch but is unaffected — its
     members are plain name/value pairs.

   PHPUnit's conventions define `is_test_path`: a `tests/`/`Tests/`
   path segment or a `*Test.php` file name.

2. **Vue reuses the TypeScript grammar behind a new, line- and
   offset-preserving `LanguageSupport::source_for_parse` hook** (default:
   borrow the input unchanged; Vue is the only override). `VueSupport`
   masks every byte outside the SFC's `<script ...>...</script>` block(s)
   to a space (newlines kept), then hands the result to the TypeScript
   grammar and queries. Because masking preserves line count and byte
   offsets, the diff's changed-line ranges, `ExtractedSymbol::range`, and
   the sliced signature text all keep referring to positions in the real
   file — no coordinate mapping layer is needed anywhere downstream.
   Plain-JS script blocks parse under the TypeScript grammar too (TS is
   a syntactic superset for the constructs the definition query
   captures), so untyped `<script>` and `lang="ts"` route identically.

## Alternatives considered

- **A dedicated Vue grammar crate**: rejected — unmaintained on
  crates.io, pinned to old ABIs, and it would still delegate the script
  block to an embedded JS/TS parse, adding a dependency without removing
  the need to locate script regions.
- **Extracting the script block into a separate string** (rather than
  masking in place): rejected — extraction changes line numbers and byte
  offsets, which would force a coordinate-translation layer through the
  changed-range overlap check, signature slicing, and the TUI's
  line-keyed panes. Masking gets the same parse with zero translation.
- **`LANGUAGE_PHP_ONLY` for PHP**: rejected — the tags-with-HTML
  variant parses pure-code files to the same tree while also handling
  template-style files, so the stricter variant only removes capability.

## Addendum (2026-08-19): Svelte

Svelte components have the same markup-plus-`<script>` shape, so
`SvelteSupport` (`language/svelte.rs`) reuses `vue::mask_non_script`
and the TypeScript grammar/queries verbatim — the masking already keeps
every `<script ...>` block, which covers Svelte's instance script and
`<script context="module">`/`<script module>` alike. No new decision
was needed; this records that the Vue mechanism is deliberately shared
rather than duplicated.

## Consequences

- `LanguageSupport` grows one defaulted method (`source_for_parse`).
  Issue #219's "promote per-language extraction hooks into
  LanguageSupport" direction gains a precedent: the hook is defined on
  the consumer side and only Vue overrides it.
- PHP method captures inherit the ADR 0064 ubiquitous-name stoplist for
  `->method()` receiver calls; the stoplist's membership is Rust-idiom
  biased, which is conservative (over-filtering common names like
  `find`/`get`) rather than noisy for PHP.
- `<template>`-only or `<style>`-only SFC edits extract no symbols and
  surface the file under "Other changed files" via the existing
  non-symbol-changes path (ADR 0070) — intentional: those edits have no
  API surface to outline.

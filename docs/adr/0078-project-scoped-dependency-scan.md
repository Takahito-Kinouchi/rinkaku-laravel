# 0078. Project-scoped, read-filtered dependency scan

Date: 2026-08-19

## Status

Accepted (extends ADR 0003, ADR 0033, ADR 0076)

## Context

With `--deps 1` (the default), startup builds a repository-wide
definition index: `git ls-files`, a `git check-attr` batch, one blob
read per tracked path, then a parse of every prefilter-passing file.
ADR 0076 parallelized the parse, but on a large monorepo the scan was
still slow twice over:

1. **Everything is read.** Lockfiles, images, markup, and other files
   with no registered `LanguageSupport` dominate `git ls-files` on a
   typical web repository; their blobs were read and only then skipped
   by the indexer.
2. **Every project is read.** A monorepo holding several applications
   (the motivating case: multiple Laravel apps side by side) scans all
   of them for a diff that touches one.

The reading stretch also reported no progress — the spinner sat on
"Building dependency index..." with no count until parsing began.

## Decision

1. **Scope the scan to changed projects** (`--deps-scope
   changed-projects`, the default). A pure helper
   (`rinkaku_core::project_scope`) derives, from the tracked-path list
   alone, each changed file's nearest ancestor directory carrying a
   project manifest (`composer.json`, `package.json`, `Cargo.toml`,
   `go.mod`, `pyproject.toml`, `setup.py`, `Gemfile`); the index
   universe is restricted to those roots' subtrees. Scoping is a
   narrowing, never a correctness gate: whenever it cannot classify a
   changed path (single-project repository, no manifests, a stray
   top-level file), it falls back to the full scan. `--deps-scope repo`
   is the explicit escape hatch for cross-project dependencies.
2. **Filter paths before reading.** Paths with no registered language —
   and test paths under `--exclude-tests` — are dropped from the read
   list itself, not just from indexing, so their blobs are never read
   and the `git check-attr` batch runs over the smaller list too.
3. **Skip the scan entirely when the diff references no names.** An
   empty reference set can never resolve anything; `build_resolver`
   returns `None` before listing a single file.
4. **Make reading visible.** A new `AnalysisPhase::ReadingFiles`
   ("Reading files...") brackets the blob reads, and both the
   `git cat-file --batch` pump and the working-tree read loop report
   `(done, total)` through the existing ADR 0033 progress callback. The
   working-tree loop also moves to rayon (ordered `collect`, shared
   completion counter — the ADR 0076 pattern).

## Alternatives considered

- **Scoping by top-level directory instead of manifests**: rejected —
  monorepos nest projects at arbitrary depths (`apps/x`, `services/y/z`),
  and a manifest is the signal that a directory is a dependency
  boundary, which is exactly the property the index cares about.
- **Making `repo` the default and `changed-projects` opt-in**: rejected
  for this fork — its primary repositories are multi-project monorepos,
  and the fallback-to-full-scan rule already protects single-project
  repositories from any behavior change.

## Consequences

- Cross-project name resolution (a changed app depending on a sibling
  package in the same monorepo) is not found under the default scope
  when the sibling lives outside every changed project root —
  `--deps-scope repo` restores it. Shared code under the *same* project
  root is unaffected.
- An empty diff or reference-free diff now skips the scan; downstream
  behavior is identical (`None` resolver and an empty index resolve
  nothing either way), locked in by `build_resolver`'s tests.

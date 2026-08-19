# 0077. Route every test-path entry into the Tests section

Date: 2026-08-19

## Status

Accepted (amends ADR 0035 Phase B)

## Context

ADR 0035 Phase B routes *whole test files* out of the TUI's production
directory tree into a trailing "Tests" section — but only the entries
arriving via `report.files`. Three other report sections were still
inserted into the production tree unconditionally:

- `report.removed` (a test file that lost a symbol),
- `report.skipped` (a deleted test file — ADR 0065 reports it in both
  `skipped` and `removed` — or a fixture under `tests/` with no
  registered language, e.g. a JSON file), and
- `report.tests` (whole-test-file summaries under `--exclude-tests`).

On a real Laravel repository this made the `tests/` directory appear
**twice**: once in the normal directory list (carrying the removed
symbols, deleted files, and fixtures) and once inside the Tests section
(carrying the changed test files) — with a single file duplicated across
both whenever it both changed and lost a symbol. The duplicate nodes
also shared `TreeNode::path` collapse keys across the two subtrees.

## Decision

`build_tree` records, per `FileReport`, which builder (production or
tests) the file went to, and routes every `removed`/`tests`/`skipped`
entry for the same path to the same builder — so both halves of a file
always merge into one node. A path with no `FileReport` at all routes by
path convention (`tests_section::is_test_dir_path`): the language's own
`is_test_path` when the path has a registered `LanguageSupport`, else a
language-agnostic check for conventional test-directory segments
(`tests`, `Tests`, `test`, `__tests__`, `testdata`) — the fallback is
what catches fixtures and deleted files no grammar claims.

`SkipReason::Generated` entries stay dropped entirely, as before.

## Consequences

- A test-only diff no longer grows a production directory chain; under
  `--exclude-tests` the `[test] (N symbols)` summary rows now render
  inside a Tests section instead of the production tree.
- The Tests section can now contain removed-symbol and skipped-file
  rows; `wrap_section`'s A-Z ordering and badge aggregation already
  handle every `TreeNode` shape, so no renderer changes were needed.
- A mixed file (production + test symbols) keeps its production row and
  its `[test]` badge merged on that row, exactly as before — routing is
  keyed on the file's own destination, not on the entry kind.

# 0076. Parallelize the dependency index build (TagsResolver)

Date: 2026-08-19

## Status

Accepted (extends ADR 0031, ADR 0033)

## Context

ADR 0031 parallelized `pipeline::analyze_repo`'s per-file tree-sitter
parse with rayon after establishing the per-file body is embarrassingly
parallel: `extract_all_symbols` builds a fresh parser per call, and every
filter reads borrowed state without mutation. `deps::TagsResolver::new`
runs the *same* per-file body over the same `git ls-files` universe —
prefilter, then parse, then index — but stayed a sequential loop, and on
large repositories it dominates `--deps 1`'s wall-clock time once file
*reading* is batched (`git cat-file --batch`, one child process for every
path).

One ordering subtlety kept it sequential: `resolve_dependencies` ranks
same-name candidates with a stable sort whose tie-break is the index's
insertion order (documented there as "in practice `git ls-files`'s
lexicographic path order"). A naively concurrent index (per-thread maps
merged in completion order, or a concurrent hash map) would make that
tie-break nondeterministic across runs.

## Decision

Parallelize only the per-file *extraction* with rayon's ordered
`into_par_iter().collect()`, exactly as ADR 0031 did, and keep the index
*insertion* sequential over the collected, source-ordered results. Each
name's candidate list therefore preserves byte-for-byte the order the
sequential loop produced. Progress reporting moves to the shared
`AtomicUsize` completion counter `analyze_repo` already uses (ADR 0033):
files are counted as they finish, in completion order, and the counter is
only touched when a callback is present.

`TagsResolver::new`'s `language_for_path` parameter gains a `+ Sync`
bound — every real caller passes the `Sync` free function
`language::language_for_path`, and tests pass plain function items.

## Consequences

- A determinism regression test
  (`deps_tests/parallel_determinism.rs`) locks the input-file-order
  invariant down, mirroring `pipeline_tests/parallel_determinism.rs`, so
  a future switch to an unordered combinator fails loudly.
- Output is byte-identical to the sequential implementation; only
  wall-clock time and the interleaving of progress callbacks change
  (progress was already completion-ordered in `analyze_repo`, so `--tui`
  consumers already handle it).

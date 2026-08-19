# 0079. Persistent, per-repository dependency-index cache

Date: 2026-08-19

## Status

Accepted (extends ADR 0003, ADR 0010/0011, ADR 0025, ADR 0031, ADR 0033,
ADR 0076, ADR 0078)

## Context

`--deps 1` (the default) rebuilds `TagsResolver`'s repository-wide
definition index from scratch on every `--base`/`--pr` run: ADR 0078
already scopes and read-filters the candidate path list, and ADR 0076
parallelizes the parse, but every candidate file's content is still read
via `git cat-file --batch` and re-parsed via `extract_all_symbols` every
single time. For a repeated local run against the same commit, or a CI
job re-running close together on an unchanged repository, that work is
wasted — the index it produces is byte-identical to the previous run's.

`--base`/`--pr` mode always has a stable git blob to key staleness on
(unlike stdin/whole-repo working-tree mode, ADR 0017, where a file's
on-disk content need not match any committed blob at all). That makes a
blob-OID-keyed cache viable specifically for this mode: `git ls-tree`
already reports every tracked blob's OID in one subprocess, cheaper than
reading a single file's content, so "did this file change since the last
run" can be answered before any content is read at all.

## Decision

1. **Cache API on `TagsResolver`** (`rinkaku-core`):
   `TagsResolver::from_entries(entries: impl IntoIterator<Item = (String,
   Vec<IndexEntry>)>, include_tests: bool) -> Self` builds the same index
   `new` does, but from already-extracted `IndexEntry { name, signature,
   container, is_test }` data instead of parsing file content —
   `derive(Serialize, Deserialize)` is what lets a cache round-trip
   `IndexEntry` through disk as JSON. Insertion order is preserved
   exactly like `new`'s sequential post-parse loop (see
   `resolve_dependencies`'s stable-sort tie-break), so a caller must pass
   `entries` in the same path order `new` would have used.
   `deps::is_generated_content` is promoted from `pub(crate)` to `pub` so
   the cache module (below) can apply the exact same content-marker check
   to a cache miss's freshly read content, across the `rinkaku_core`/
   `rinkaku` crate boundary.
2. **On-disk cache** (`rinkaku`'s new `deps_cache` module): a JSON file at
   `<git-dir>/rinkaku-cache/deps-index-v1.json` (`<git-dir>` from a new
   `git rev-parse --git-dir` helper in `git/commands.rs` — beside git's
   own per-repository state, not inside the worktree, so it needs no
   `.gitignore` entry and can never be accidentally committed). Shape:
   `{ format_version, rinkaku_version, files: { path: { oid,
   is_generated, entries } } }`, keyed by path with each entry keyed on
   git blob OID. The cache is a pure performance optimization with no
   correctness dependency: `load` degrades to an empty cache on *any*
   failure (missing file, malformed JSON, a `format_version` or
   `rinkaku_version` mismatch — an older/newer rinkaku build this version
   cannot assume shape/extraction compatibility with) rather than
   erroring, and `save` (temp file + rename in the same directory, after
   pruning entries whose path isn't among this run's candidates) is
   best-effort — a failure is logged at `debug` and never surfaces to the
   user.
3. **Integration in `pipeline::build_resolver`**, only for the `head:
   Some(_)` branch (`--base`/`--pr`; working-tree mode keeps the
   pre-cache behavior unconditionally — see Context): a single `git
   ls-tree -r <head> -z --format=%(objectname)%x09%(path)` subprocess
   (new `git/commands.rs` helper) reports every candidate path's current
   blob OID. A path whose OID matches the cache is a hit (no read, no
   parse — the cached `IndexEntry`s are reused directly, still subject to
   the current run's `is_generated`/`.gitattributes` gating). Every other
   path is a miss: read via the existing batched `read_git_show_files_batch`,
   then parsed with `extract_all_symbols` in a rayon `par_iter` —
   deliberately bypassing `TagsResolver::new`'s aho-corasick prefilter
   entirely, since a cached entry must stay correct for *any* future
   run's reference names, not just this run's. `TagsResolver::from_entries`
   builds the resolver over hits+misses in the original candidate path
   order (not hits-then-misses, to preserve the tie-break order from
   decision 1). The cache is saved back afterward, best-effort.
4. **`--no-deps-cache`** (`rinkaku/src/cli.rs`, default off — the cache is
   on by default): falls back to the exact pre-cache code path (read
   every candidate, parse every candidate, no cache file ever read or
   written) — an escape hatch for ruling out a stale cache while
   debugging, or a one-shot CI runner where the cache would never be
   reused.

## Alternatives considered

- **Content-hash keying instead of git blob OID**: rejected — would
  require reading every candidate's content before the cache could even
  be consulted, defeating the entire point (skip the read, not just the
  parse). The blob OID is already known from `git ls-tree` for free.
- **Caching across `head: None` (working-tree) runs too**: rejected per
  Context — a working-tree file's content need not match any git blob at
  all, so there is no stable key to check staleness against without
  hashing content on every run (which is exactly the cost being avoided).
- **Keeping the aho-corasick prefilter for cache misses**: rejected — a
  cached entry has to answer *future* runs' reference names, which the
  current run cannot know in advance; skipping the prefilter for misses
  is what makes the cache correct regardless of which run populated it.
  Accepted cost: a cold cache (first run, or a miss-heavy run touching
  many changed files) can therefore be *slower* than the old prefiltered
  path for that one run, since it parses files the prefilter would have
  skipped entirely — subsequent runs against the same commit win back
  that cost and then some.

## Consequences

- Repeat `--base`/`--pr` runs against an unchanged repository (or one
  where only the diff's own files changed) parse only the changed files',
  not the whole repository-wide candidate set — the primary performance
  win this ADR targets.
- First-run cost can regress slightly relative to the old prefiltered
  path (see Alternatives) — acceptable since every subsequent run wins,
  and `--no-deps-cache` recovers the old behavior exactly for anyone who
  needs it.
- Known limitation: a candidate file whose content matches a
  generated-file marker (ADR 0011) has its parse skipped under the
  default `--include-generated=false`, the same way `TagsResolver::new`
  already does — so its cached entry holds `is_generated: true` with an
  empty `entries`. Toggling `--include-generated` on for a later run
  reuses that empty cached entry rather than retroactively parsing the
  file; only a subsequent blob OID change (or `--no-deps-cache`)
  re-parses it with entries populated. Not solved here, matching ADR
  0011's own "accepted as a known limitation" precedent for a related
  edge case.
- `.gitattributes`-based generated detection (ADR 0010) is deliberately
  *not* cached per file — it is a path/pattern rule re-checked fresh
  every run, not a property of the blob content itself, so caching it
  would not save meaningful work and could go stale independently of the
  file's own content.
- The cache directory (`<git-dir>/rinkaku-cache/`) is per-repository and
  grows with the number of distinct paths ever indexed at any commit;
  `save`'s candidate-path pruning keeps it from growing unbounded for a
  single repository over time, but nothing yet caps its total size or
  evicts it — left as a follow-up if it proves large in practice.

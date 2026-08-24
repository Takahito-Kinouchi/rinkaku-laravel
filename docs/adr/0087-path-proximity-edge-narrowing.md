# 0087. Graph edges keep only the closest same-name matches

Date: 2026-08-24

## Status

Accepted

## Context

`graph::collect_edges` resolves a referenced name by matching it against
every changed symbol of that name (ADR 0003, name-only resolution). Its
doc comment stated the reasoning plainly: "the caller has no way to
disambiguate under v1's name-only matching, so all plausible edges are
kept rather than arbitrarily picking one."

That reasoning does not survive a monorepo. Laravel's naming conventions
put an `OrderController`, a `StoreOrderRequest`, and a `UserService` in
*every* application, so in a repository laid out as

```
apps/shop/app/Http/Controllers/OrderController.php
apps/shop/app/Http/Requests/StoreOrderRequest.php
apps/admin/app/Http/Controllers/OrderController.php
apps/admin/app/Http/Requests/StoreOrderRequest.php
```

a diff touching both applications produced this:

```
- fn store (apps/admin/.../OrderController.php)
  - fn rules (apps/admin/.../StoreOrderRequest.php)
  - fn rules (apps/shop/.../StoreOrderRequest.php)      <- wrong
- fn store (apps/shop/.../OrderController.php)
  - fn rules (apps/admin/.../StoreOrderRequest.php) (see above)   <- wrong
  - fn rules (apps/shop/.../StoreOrderRequest.php) (see above)
```

Half the edges claim a dependency between two applications that share
nothing but a naming convention. The damage is not limited to the tree:
`compute_fan_ins` counts referrers from those same edges, so `rules`
appeared under "High fan-in symbols" as "used by 2: store, store" — a
blast-radius warning about an entirely fictional second caller.

The collision is old, not new. Reproduced against a `main` build, two
applications whose `OrderService::create` both changed and whose
controllers both call `$service->create(...)` link across applications
the same way, through `referenced_method_names`. ADR 0086's container
fallback widened the surface rather than creating it, because the names
that collide in Laravel are exactly the class names that appear in type
hints.

Only one layer of rinkaku knows about monorepos today:
`project_scope.rs` (ADR 0078), used at `rinkaku/src/pipeline.rs` to
narrow which files the dependency index scans. It is explicitly a
performance narrowing, "never a correctness gate", and when a diff spans
two applications it returns the union of both roots, so it does not
narrow at all in precisely the case that goes wrong. It also never
reaches the graph.

`deps::resolve_dependencies` already had the missing ingredient. When
name-only resolution hands it several candidates, it ranks them with
`path_proximity_rank` — same file, then same directory, then by shared
path-prefix depth — and keeps the closest, on the stated rationale that
tree proximity is "the same heuristic an editor's go-to-definition
fallback (or a human skimming candidates) would reach for first". The
graph made no use of it.

## Decision

When a referenced name matches several changed symbols, `collect_edges`
builds edges only to the candidates tied at the best
`path_proximity_rank` relative to the referencing symbol's file, and
drops the rest. The rule applies uniformly to all three matching paths:
bare `referenced_names`, `referenced_method_names`, and ADR 0086's
container fallback.

1. **Ties are kept in full.** Candidates at equal distance are equally
   plausible, and choosing among them would be the arbitrary pick the
   original comment rightly refused. Narrowing drops *farther*
   candidates; it does not disambiguate equidistant ones.
2. **The Laravel shape depends on that.** A container's changed members
   live in one file, so they tie and all survive — `store` still links
   to both `authorize` and `rules` of the request class it type-hints.
3. **`path_proximity_rank` moves to a shared module.** It and its
   `path_dir_components` helper move from `deps.rs` to a new
   `path_proximity.rs`, `pub(crate)`. CLAUDE.md permits a shared
   abstraction "after a concrete second use case demands it, and after
   discussing it in an ADR" — the graph is that second consumer, and
   this is that discussion. Nothing about the ranking itself changed, so
   `deps`' behavior is untouched.

Both monorepo reproductions above now link only within their own
application, and the fictional fan-in entry is gone.

## Alternatives considered

**Scope edges by project root instead of proximity.** Reusing
`project_scope`'s manifest-based roots would express the intent directly
— "an edge may not cross a project boundary". Rejected for now:
`changed_project_roots` needs the repository's full tracked-file list,
which is IO and lives at the CLI boundary, while `build_graph` is pure
core that sees only the diff's own `FileReport`s. Threading a root list
into the core would widen the change well past the defect. Proximity
approximates the same boundary from data the graph already has, and
generalizes to repositories with no manifests at all.

**Cap matches per name, as `deps` does with `MAX_MATCHES_PER_NAME`.** A
cap bounds the noise but does not remove it: with exactly two
applications, both candidates fit under a cap of 3 and both wrong edges
survive. Ranking is what actually separates them; the cap is a
list-length concern specific to rendering a "Depends on" list.

**Leave it and document the limitation.** The output is not merely
imprecise here, it is confidently wrong in a way a reviewer cannot
detect from the report — a fan-in warning naming a caller that does not
exist reads exactly like a real one.

## Consequences

- On this repository's own history — Rust, no monorepo, so the
  narrowing is pure removal of cross-tree matches:

  | diff | edges before | edges after | roots | high fan-in entries |
  |---|---|---|---|---|
  | `--base HEAD~5` | 112 | 110 | 38 → 38 | 7 → 5 |
  | `--base HEAD~15` | 623 | 567 | 204 → 206 | 54 → 52 |
  | `--base HEAD~40` | 7752 | 2649 | 631 → 648 | 169 → 124 |

  The large diff loses two thirds of its edges and a quarter of its
  high-fan-in entries. Those were same-named symbols scattered across
  unrelated modules, the non-monorepo form of the same collision.
- Roots rise slightly (+17 of 1010 nodes on the largest diff): a symbol
  whose only incoming edge was a distant same-name match becomes a root
  again. That is the honest outcome — it had no *near* referrer in the
  diff — but it means the narrowing trades a little tree depth for
  correctness. Single-project Laravel diffs are unaffected; their trees
  are byte-identical before and after.
- A genuine dependency on a farther definition is dropped whenever a
  nearer same-named one exists. Under name-only resolution that case is
  undecidable, and the nearer candidate is the better bet — the same bet
  `deps` has been making. A Rust type with `impl` blocks in two files at
  different depths is the shape most likely to lose an edge this way.
- The narrowing is a heuristic over paths, not a boundary rule. Two
  applications nested at different depths (`apps/shop/...` next to a
  top-level `admin/...`) can still tie or rank unexpectedly. Replacing
  it with real project scoping stays open, and becomes straightforward
  if a `Resolver` implementation ever brings repository structure into
  the core.

# 0086. Unmatched references fall back to a container's changed members

Date: 2026-08-24

## Status

Accepted

## Context

rinkaku's change graph is meant to be read top-down: roots are the entry
points a change enters through, and everything a root depends on hangs
underneath it. "Definitions" then follows the same DFS pre-order, so a
reviewer scrolling from the top meets each symbol after the symbol that
depends on it.

On a Laravel-shaped diff that ordering did not happen. A four-file
change — controller, form request, service, model — rendered as:

```
- fn store (app/Http/Controllers/OrderController.php)
  - fn create (app/Services/OrderService.php)
- fn rules (app/Http/Requests/StoreOrderRequest.php)
- fn scopeRecent (app/Models/Order.php)
```

Three of the four symbols are roots. The tree reads as a flat list in
`git diff`'s path order, which for a conventional Laravel layout happens
to be `Controllers/` → `Requests/` → `Models/` → `Services/` —
alphabetical, and close enough to a layered ordering to be mistaken for
one. Nothing in it was derived from dependencies.

The cause is a granularity mismatch between the two halves of edge
building. `graph::collect_edges` matches a symbol's `referenced_names`
against other changed symbols' **names** (ADR 0003, name-only
resolution). But the references that cross file boundaries in this stack
name a **container**, not a member:

- `public function store(StoreOrderRequest $request)` — a type hint
- `return Order::create($attributes)` — a static-call scope
- `new Invoice()` — an instantiation
- `catch (PaymentFailed $e)`, `$x instanceof User`, `extends FormRequest`
  — the shapes ADR 0082 added captures for

while the changed symbols on the other side are almost always the
container's methods. `extract`'s "narrowest enclosing definition" rule
means a class declaration only becomes a changed symbol when its own
declaration line changed; even an entirely new Laravel class yields one
node per method and none for the class. So a reference to
`StoreOrderRequest` had nothing named `StoreOrderRequest` to match, and
`rules` had no incoming edge from anything.

This is not PHP-specific — a Rust `impl` block's methods, a Go receiver
method, and a Python or TypeScript class method are all reached the same
way — but PHP frameworks hit it on essentially every diff, because their
layering is expressed almost entirely through constructor-injected and
type-hinted class names.

`deps::resolve_dependencies` already resolves these references correctly
(the "Depends on:" list under `fn store` did name both
`class StoreOrderRequest` and `class OrderService`), because it resolves
against a repo-wide index of *definitions* including the classes. Only
the graph, restricted to changed symbols, could not make the link.

## Decision

`collect_edges` indexes changed symbols a second way — by the type name
inside their `Node::container` — and, **for a `referenced_names` entry
that matched no changed symbol by name at all**, matches against that
index instead, producing one edge to each of the container's changed
members.

1. **A fallback, not an addition.** When the container is itself among
   the changed symbols, it is the precise target the reference denotes
   and matching stops there; reaching past it to its members would add
   edges saying nothing the direct one does not. Resolution happens at
   the most precise granularity available, and drops to the container
   only when the symbol level has nothing to offer.
2. **Bare references only.** `referenced_method_names` entries are
   method names (`$service->issue()`, a trait method spec); they never
   name a container, so matching them this way would only collide with
   same-named classes. The rule is scoped to the `referenced_names` set
   — which is where type hints, `new`, static-call scopes,
   `extends`/`implements`, and `instanceof` targets land.
3. **The container's type name is recovered from its label.**
   `extract::find_container` emits a fixed set of shapes — `impl X`,
   `trait X`, `class X`, `interface X`, `enum X`, or a Go method's bare
   receiver type name — so `graph::container_type_name` strips a known
   leading keyword and treats the remainder as the type name. No new
   field on `ExtractedSymbol`.
4. **Siblings are excluded.** A reference from inside the container it
   names (`self::`, `static::`, `Foo::class` written within `Foo`) is
   skipped. Expanding it would give every class a complete internal mesh
   that says nothing about which member uses which; a real intra-class
   call is already captured as a method reference.
5. **Edges are deduplicated by `(from, to)`.** A name can appear in both
   reference sets — a method called through a receiver in one place and
   named as a bare reference in another — and each set is matched
   separately. A duplicate edge renders the target a second time as a
   `(see above)` line, so the pair is collapsed where edges are
   appended.

On the diff above the tree becomes:

```
- fn store (app/Http/Controllers/OrderController.php)
  - fn create (app/Services/OrderService.php)
    - fn scopeRecent (app/Models/Order.php)
  - fn rules (app/Http/Requests/StoreOrderRequest.php)
```

## Alternatives considered

**Match containers in addition to names, not as a fallback.** Tried
first, and measured against this repository's own history — a Rust
codebase, where the rule is least likely to pay for itself:

| diff | edges before | always match | fallback only | roots before → after |
|---|---|---|---|---|
| `--base HEAD~5` | 112 | 191 | 111 | 38 → 37 |
| `--base HEAD~15` | 555 | 823 | 621 | 211 → 203 |
| `--base HEAD~40` | 7574 | 11629 | 7722 | 647 → 630 |

Both variants reduce roots identically and produce the same tree on the
Laravel fixtures, but matching unconditionally inflates edges by roughly
50% on Rust diffs — rendered, that is +20% output lines and +4000
`(see above)` entries on the largest — buying nothing. Every one of
those extra edges is a reference whose *precise* target was already in
the graph. Rejected on cost.

**Emit the container itself as a changed symbol.** Making every touched
class a node would give references something to match, and the members
could nest beneath it. Rejected: it changes what "changed symbol"
means — the summary line's count, "Definitions", the removed-symbol
list, and the file-size and test-coverage sections all key off that set
— to fix an edge-building problem. It would also add a node to every
diff that touches a class, whether or not the class declaration itself
was reviewed.

**Resolve member references precisely (LSP).** A resolver that knew
`$request->validated()` binds to `StoreOrderRequest::validated` would
link exactly the members actually used. That is the `Resolver` trait's
eventual job, and it does not conflict with this decision — but it needs
a language server per stack, and the graph needs to be readable before
that exists.

## Consequences

- The change graph's roots become the symbols a diff is actually entered
  through, and reading "Change graph" and "Definitions" from the top now
  follows dependencies rather than path order — for PHP most visibly,
  but for every language whose members live in containers.
- The link is an over-approximation: a referrer gets an edge to every
  changed member of a container it names, not only the members it calls.
  This is the same trade ADR 0063 accepted for transitive test coverage,
  for the same reason — the graph allocates a reviewer's attention, it
  does not prove reachability, and a dependency ranked above its
  dependents is the more misleading error.
- Fan-in counts rise where the fallback fires, so more symbols cross
  `HIGH_FAN_IN_THRESHOLD`. A class referenced from several places now
  attributes that fan-in to the members that changed, which is the
  intent of the section.
- Test coverage improves for the same reason: a PHPUnit test naming a
  controller or service class now reaches its changed methods, so
  "Changes with no referencing tests" stops listing methods that a test
  exercises through their class.
- A generic Rust `impl` (`impl Cache<K, V>`) has a parameterized
  container label that no reference name equals, so its members stay
  unlinked exactly as before this ADR. Not addressed here: it needs the
  container label to carry a base type name separately from its
  parameters, which is a change to extraction, not to edge building.
- ADR 0064's stoplist is unaffected in scope but its end-to-end pin
  needed a fixture change (`pipeline_tests::rust_stoplist_regression`):
  the fixture's `fn build(state: Store)` named the container of the
  changed `impl Store::get` while nothing named `Store` was in the diff,
  so the fallback now links them for a reason that has nothing to do
  with the stoplisted `state.get()` call. `build` now reaches its
  receiver through a helper, leaving the stoplisted call the only
  possible source of that edge.

# 0082. PHP reference-query Laravel gaps

Date: 2026-08-19

## Status

Accepted

## Context

Empirical verification against a standard Laravel-style fixture (a
controller/service class using enum cases, fully-qualified type names,
`instanceof` checks, and `extends`/`implements`) showed no "Depends on"
edges for any of those four shapes, even though the referenced
class/enum/interface definitions were present in the same diff or repo
index. AST analysis of `tree-sitter-php`'s `LANGUAGE_PHP` grammar
confirmed why: `language/php.rs`'s `REFERENCE_QUERY` (ADR 0075) never
had patterns for these shapes at all — not a resolution failure, a
capture gap.

- `OrderStatus::Pending`/`UserController::class` parse as
  `(class_constant_access_expression (name) (name))`, a node kind
  `REFERENCE_QUERY` didn't mention.
- `\App\Models\Order` in a type/`new`/static-call-scope/catch position
  parses as `(qualified_name prefix: (namespace_name (name) (name))
  (name))` — a distinct node kind from the bare `(name)` the existing
  `named_type`/`object_creation_expression`/`scoped_call_expression`
  patterns already capture, so a fully-qualified reference silently
  fell through every one of them.
- `$x instanceof User` parses as `(binary_expression left: ... right:
  (name))`, sharing its node kind with every other PHP binary operator
  (concatenation, comparison, logical) — nothing in `REFERENCE_QUERY`
  touched `binary_expression` at all.
- `class Foo extends Base implements Contract` parses its targets under
  `(base_clause (name))`/`(class_interface_clause (name))`, two node
  kinds `REFERENCE_QUERY` never referenced.

Laravel-style code leans on all four constructs constantly (enum-backed
statuses, fully-qualified imports left unaliased in type hints, `instanceof`
guards, and interface/base-class contracts), so the gap was not a corner
case for that stack.

## Decision

Add eleven patterns (four shapes, most with a bare and a qualified-name
variant) to `REFERENCE_QUERY`, all as `@reference.type` — none of these
are call sites, so none use `@reference.call`/`@reference.method`:

1. `(class_constant_access_expression . (name) @reference.type)` plus a
   `(qualified_name (name) @reference.type)` variant. The leading `.`
   anchor restricts the match to the node's first child (the class/enum
   half); the second child — the constant name, or the literal `class`
   keyword in `Foo::class` (which itself parses as a `(name)` node) — is
   never captured.
2. Qualified-name variants of the three existing class-position
   captures: `(named_type (qualified_name (name) @reference.type))`,
   `(object_creation_expression (qualified_name (name)
   @reference.type))`, `(scoped_call_expression scope: (qualified_name
   (name) @reference.type))`. A `qualified_name`'s namespace prefix
   segments live under a nested `namespace_name` child, not as direct
   children of `qualified_name` itself, so `(qualified_name (name))`
   captures exactly the final segment (the class basename) and never a
   prefix segment — consistent with name-based resolution (ADR 0003),
   which only ever needs the basename. Bare catch-clause types were
   already covered by the plain `named_type (name)` pattern; only the
   qualified form needed a new pattern, so no catch-specific pattern was
   added.
3. `(binary_expression operator: "instanceof" right: (name)
   @reference.type)` plus a qualified-name variant. Anchored on the
   anonymous `"instanceof"` operator token specifically, rather than an
   unfiltered `right: (name)` — `binary_expression` covers every PHP
   binary operator, so an unanchored capture would also pull in
   unrelated constants such as the `SOME_CONST` in `$a . SOME_CONST`
   (string concatenation).
4. `(base_clause (name) @reference.type)` and `(class_interface_clause
   (name) @reference.type)`, each with a qualified-name variant.
   `interface_declaration`'s own `extends` list reuses `base_clause`
   too, so no interface-specific pattern was needed.

No changes to `extract/references.rs`'s capture-collection logic: every
new capture uses the existing `reference.` prefix convention and lands
in the same `bare` set `@reference.call`/`@reference.type` captures
already populate.

No changes to `index_prefilter_patterns` (ADR 0080): PHP's prefilter
already anchors on `enum {name}`/`interface {name}`/`class {name}`/
`trait {name}` declarations, so a referenced enum/interface/class name
newly captured here still matches its defining file's prefilter pattern
— verified by reading the six existing patterns, not by adding new
ones.

## Alternatives considered

- **A single unanchored `binary_expression right: (name) @reference.type`
  pattern for `instanceof`**: rejected — `binary_expression` is PHP's
  shared node kind for every binary operator, so this would also capture
  the right-hand identifier of unrelated operators like string
  concatenation, polluting the graph the same way ADR 0064 already
  documented for unfiltered associated-call captures.
- **A bare-name-only capture for the four shapes, skipping the
  qualified-name variants**: rejected — Laravel code overwhelmingly uses
  fully-qualified names in type hints, `new`, static-call scope, and
  catch clauses (often left unaliased even where a `use` import exists),
  so skipping the qualified form would leave most real occurrences
  uncaptured.

## Consequences

- `Foo::class` and class-constant/enum-case accesses now link the class
  or enum to its definition in "Depends on".
- Fully-qualified names in type/`new`/static-call/catch position now
  resolve by class basename, matching how bare references already
  resolve (ADR 0003) — namespace prefix segments (`App`, `Models`, ...)
  are never captured, by construction of the `(qualified_name (name))`
  pattern rather than by any exclusion list.
- `instanceof` checks and `extends`/`implements` targets now surface as
  dependencies; an `extends`/`implements` reference is attributed to the
  class-level symbol (`base_clause`/`class_interface_clause` sit on the
  `class_declaration` node itself, outside any nested method's own
  subtree), not to whichever method happens to be reported alongside it.
- `REFERENCE_QUERY` grows from 6 to 17 alternatives; no measurable
  parse-time cost expected — tree-sitter query alternation cost scales
  with the number of distinct node kinds touched, not linearly with
  pattern count, and this only adds patterns rooted at kinds the query
  already visits plus four previously-unvisited kinds
  (`class_constant_access_expression`, `binary_expression`,
  `base_clause`, `class_interface_clause`).

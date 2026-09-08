# 0097. Vue and Svelte component API declarations are symbols

Date: 2026-09-08

## Status

Accepted

## Context

ADR 0075 gave Vue SFCs — and, by its addendum, Svelte components — a
`LanguageSupport` impl that masks everything outside `<script>` and
parses the rest with the TypeScript grammar and queries. Its
Consequences record one deliberate gap:

> `<template>`-only or `<style>`-only SFC edits extract no symbols and
> surface the file under "Other changed files" via the existing
> non-symbol-changes path (ADR 0070) — intentional: those edits have no
> API surface to outline.

That reasoning is sound for markup and styles. The trouble is that the
same bucket also swallows the one edit in an SFC that is *most*
obviously API surface. Measured against this fork's own target shape (an
Inertia/Vue frontend), on a diff that changes only prop declarations:

```
## Other changed files

- src/components/Counter.vue (3 changed lines outside any definition)
- src/lib/Runes.svelte (2 changed lines outside any definition)
```

Adding a required prop, widening an emitted event's payload, or
changing a prop's type is a breaking change for every parent that binds
to it. It is exactly what ADR 0014's contract classification exists to
flag, and exactly what a reviewer opens rinkaku to see. Instead it
arrives as a line count, indistinguishable from a CSS tweak.

The cause is structural rather than an oversight in the masking. Vue and
Svelte both reuse `typescript::DEFINITION_QUERY` verbatim, and none of
the shapes a component uses to declare its surface is a TypeScript
definition:

- `defineProps` / `defineEmits` / `defineModel` / `defineSlots` /
  `defineExpose` are compiler macros. In `<script setup>` they are
  available without an import or a binding, so they parse as a plain
  `call_expression` — there is no declaration node to capture.
- The Options API declares the same surface as `props:` / `emits:` keys
  of an object literal: `pair` nodes, which no definition query captures.
- Svelte 4's `export let items: string[]` is an `export_statement`
  wrapping a `lexical_declaration`. The TypeScript query captures a
  `variable_declarator` only when its value is an arrow function, so a
  data prop is skipped by construction.
- Svelte 5's `let { start, max } = $props()` is a declarator whose value
  is a rune call — again not an arrow function, so again skipped.

A component's functions were visible all along; its interface was not.

## Decision

**Vue and Svelte get their own definition queries — TypeScript's plus
the component-API declarations above — and those declarations extract as
a new `SymbolKind::ComponentApi`.**

The queries stay derived from TypeScript's rather than replacing it: a
component's functions, classes, interfaces and type aliases are still
the same TypeScript definitions they were, and the added patterns are
purely additive.

### What each pattern captures, and what it is named

`SymbolKind::ComponentApi` covers inputs and outputs alike. Which one a
declaration is, is already spelled out by its name and its signature —
the same way `SymbolKind::Block` leaves the Terraform-specific role
(`resource`, `variable`, ...) to the name rather than multiplying kinds.

| Shape | Captured node | Name |
| --- | --- | --- |
| `defineProps<{...}>()` and its four siblings | `call_expression` | macro name minus `define`, first letter lowered: `props`, `emits`, `model`, `slots`, `expose` |
| Options API `props:` / `emits:` | `pair` | the key |
| Svelte `export let items` | `export_statement` | the declarator's name (`items`) |
| Svelte `let {...} = $props()` | `variable_declarator` | `props` |

The macros are named after what they declare, not after themselves, for
a reason beyond readability: `defineProps` is a name the *reference*
query already captures from every component that calls it. Naming the
symbol `defineProps` would therefore make every component's macro a
matching edge target for every other component's call — a fully
connected graph of a name that means nothing. `props` is referenced by
nothing, so it produces no edges at all.

### Two shapes deliberately left out

- **`$state` / `$derived` / `$effect`.** Internal component state, not
  surface a parent can bind to. Svelte's query matches `$props` only.
- **`export const` / `export function` in a Svelte script.** Already
  reported as the values and functions they are by the shared
  TypeScript patterns; they are module exports, not props.

### Overlap resolves itself

`export let onPick = () => {}` is both a prop and a function. It matches
the prop pattern (on the `export_statement`) and the arrow-function
pattern (on the `variable_declarator` nested inside it). No negation is
needed to stop it double-reporting: the declarator is a descendant of
the export statement, so `extract_changed_symbols`'s existing
narrowest-enclosing-definition rule prefers the function — which is the
more informative of the two readings.

## Alternatives

- **Name the props symbol after the file, so it doubles as the
  component's identity in the change graph.** Attractive because it
  would also give parent→child component edges a target. Rejected: a
  component declaring both `defineProps` and `defineEmits` would then
  own two symbols with one name, and the Options API would collide the
  same way. Component identity needs its own symbol, not a prop
  declaration wearing the component's name — see Consequences.
- **Extend `typescript::DEFINITION_QUERY` itself** instead of forking it
  per language. Rejected: `export let x = 1` in a plain `.ts` module is
  a mutable export, not a prop, and `props:` in a plain object literal
  is just a key. The patterns are only sound because a `.vue`/`.svelte`
  script is a component.
- **Narrow the Options-API pattern to the default export.** Would need
  two patterns (`export default {...}` and `export default
  defineComponent({...})` nest the object differently) and would still
  miss an options object assigned to a local first. The loose pattern's
  failure mode is one extra named symbol in a `.vue` script that happens
  to have a `props:` key elsewhere — benign, and never a missing one.
- **A dedicated `SymbolKind` per declaration** (`Prop`, `Emit`, ...).
  Rejected as kind inflation: every consumer (`symbol_kind_prefix`, the
  TUI's row view, detail pane and blast radius) would grow five arms to
  say what the name already says.

## Consequences

- A props/emits edit now classifies through ADR 0014 like any other
  signature: the report shows `api props — signature changed` with a
  diff of the declaration, rather than a line count. Verified across all
  four authoring styles (Vue `<script setup>`, Vue Options API, Svelte 4
  `export let`, Svelte 5 `$props()`).
- ADR 0075's Consequences paragraph quoted above is narrowed by this
  ADR: `<template>`-only and `<style>`-only edits still surface as line
  counts, but "an SFC edit outside a function has no API surface" is no
  longer true and was never the reason that paragraph gave.
- SFC-heavy diffs grow by roughly one symbol per component whose
  interface changed. That is the intended cost: those are the symbols a
  reviewer most needs, and a component whose interface did not change
  gains nothing.
- Vue and Svelte no longer share `typescript::DEFINITION_QUERY`, so the
  three copies of its nine TypeScript patterns can drift. Accepted for
  now: a shared base string concatenated at runtime would trade a
  visible duplication for a query that cannot be read in one place, and
  the patterns have been stable since ADR 0002. Revisit if a fourth
  language forks it.
- **Component identity is still missing.** These symbols describe a
  component's surface but none of them *is* the component, so a parent
  that imports and renders a child still produces no edge to it. That
  gap — including the Nuxt/global-registration case, where the child is
  never named in the script at all — is left to its own ADR.

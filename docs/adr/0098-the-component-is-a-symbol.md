# 0098. A Vue SFC or Svelte component is itself a symbol

Date: 2026-09-08

## Status

Accepted

## Context

ADR 0097 gave a component's declared interface — props, emits, slots —
symbols of its own. What it explicitly left open is the other half:

> **Component identity is still missing.** These symbols describe a
> component's surface but none of them *is* the component, so a parent
> that imports and renders a child still produces no edge to it.

Two components changed in one diff read as two unrelated roots:

```
## Change graph

- fn describe (src/components/BaseButton.vue)
- fn increment (src/components/Counter.vue)
```

even though `Counter.vue` renders `<BaseButton>` on its second line. For
a tool whose output is a change *graph*, a frontend's most important
structural relation — which component renders which — was the one
relation it could not draw.

Two separate things block it.

**Nothing to point at.** `collect_edges` matches a referenced name
against symbol names. `BaseButton.vue` contains `describe`, `props`,
and nothing named `BaseButton`: a component's identity lives in its
*filename*, not in any declaration inside the file. There is no node a
definition query could capture to represent it.

**Nothing to point with.** A parent names its child in the markup, and
ADR 0075 masks the markup to whitespace before any query runs. Reading
the child from the script instead is not a workaround but a different,
smaller feature: under Nuxt's auto-import — and under any global
registration — the child is *never* named in the script. In a Nuxt app
that is the normal case, not an edge case.

## Decision

**A component-shaped file extracts one more symbol: the component
itself, named after the file, whose references are the child components
its markup renders.**

### Identity comes from the path

The symbol is named with the file's stem (`BaseButton`), because that is
what a parent writes. This is the first time extraction needs the path,
so `extract_changed_symbols` and `extract_all_symbols` take one.

It is anchored to the `program` node — the whole file — since an SFC *is*
the component and there is no narrower node to hang it on. Two
consequences of that node needed handling:

- **Its range is the file, not the node's own span.** `program` starts at
  the first token the masked source leaves standing, which is below a
  Vue `<template>` written above the script — so the node's own span
  excludes exactly the template edit the component symbol exists to
  name. For the same reason it counts as touched by any change to its
  file rather than by `overlaps_any` on that span.
- **Its references are the markup's children and nothing else.** Running
  the reference query over a whole-file node returns every call and type
  in it, which would make the component a hub wired to everything it
  contains. A component's own couplings are the components it renders.

### The markup is scanned, not parsed

`LanguageSupport::component_markup_references` returns the child
components found outside the `<script>` blocks — sharing `script_regions`
with `mask_non_script` so the two can never disagree about where the
script is.

It is a text scan. The markup is HTML-flavored but is not HTML (Vue's
`v-if` and `{{ }}`, Svelte's `{#each}`), and nothing here needs a tree —
only which tag names appear. A tag counts as a component when it is
PascalCase or hyphenated, which is exactly the convention both
frameworks require to tell a component from a plain element, so `<div>`
stays out without an element allowlist. Framework built-ins
(`<Transition>`, `<KeepAlive>`, `<svelte:component>`, ...) are excluded:
they resolve to nothing in any repository.

Each tag is reported in the spelling the markup used *and* in both
naming conventions, because which spelling the markup uses says nothing
about which the filesystem uses — both frameworks accept either, so
`<BaseButton />` has to reach a `base-button.vue` and vice versa.

Returning `Some` is also what marks a file as a component: the extractor
reports a component symbol for exactly the languages that answer, so an
SFC with no markup still gets its identity (`Some(vec![])`) while a
`.ts` file never does.

### It stays out of the way

The component encloses every definition in its file, so
`extract_changed_symbols`'s existing narrowest-enclosing-definition rule
suppresses it whenever anything narrower was touched. A diff that
changes a function reports the function, exactly as before; the
component surfaces only when nothing narrower did — which is precisely
the template-, style- and import-only case that used to reach the report
as "N changed lines outside any definition".

The index (`extract_all_symbols`) keeps every component unconditionally,
which is what lets a parent's reference resolve even when the child's
own component symbol was suppressed in the diff.

## Alternatives

- **Name the props symbol after the file** so ADR 0097's work doubles as
  identity. Rejected there and here: a component declaring both
  `defineProps` and `defineEmits` would own two symbols with one name.
- **Read the child from `import BaseButton from './BaseButton.vue'`.**
  Smaller — no markup scanning, no new hook. Rejected as the *only*
  mechanism: it misses Nuxt auto-import and global registration entirely,
  which is where much of the real Vue world lives. Worth adding later
  for a child used from a render function rather than markup, which the
  scan does not see.
- **Parse the markup with an HTML/Vue grammar.** The faithful answer,
  and the only one that could also read `:is` bindings or `v-bind`
  targets. Rejected for the same reasons ADR 0075 rejected a Vue
  grammar, plus one of its own: the tag names are all this needs, and a
  scan gets them without a second grammar, a second parse, or a second
  set of coordinates.
- **Emit the component unconditionally**, not subject to the
  narrowest-enclosing rule, so the change graph always shows component
  structure. Rejected: it would roughly double the symbol count of an
  SFC-heavy diff to restate a relation the reader can already see, in a
  tool whose premise is condensation. The index already carries every
  component, so dependency resolution and blast radius do not need it.

## Consequences

- A parent's coupling to its children now resolves, from the markup
  alone. Verified on a Nuxt-style component that imports nothing:
  `<BaseButton>` and `<base-button>` in the template both resolve to
  `src/components/BaseButton.vue`, deduplicated to one dependency, with
  `<Transition>` and `<div>` correctly excluded.
- Template-, style- and import-only edits now name their component
  (`component Counter`) instead of reporting a line count. This narrows
  ADR 0075's Consequences further than ADR 0097 already did: an SFC edit
  outside every function is no longer invisible, it is the component's.
- `extract_changed_symbols` / `extract_all_symbols` grow a `path`
  parameter. The extraction tests, which are path-agnostic for every
  other language, go through shims in `extract_tests` rather than
  naming one ~150 times.
- A tag that is PascalCase or hyphenated but resolves to nothing (a
  design-system element, a web component) contributes an unresolved
  reference, which costs an index lookup and shows up nowhere. That is
  the same failure mode every language's reference query already has by
  design (ADR 0003), not a new one.
- A child used only from a script — a render function, a dynamic
  `<component :is>` — is still not seen. The import-based mechanism
  above is the natural follow-up if it turns out to matter.

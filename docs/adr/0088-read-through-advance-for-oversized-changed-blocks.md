# 0088. Read-through advance and off-screen row counters for the diff pane

- Status: accepted
- Date: 2026-08-24

## Context

Walking the tree pane top to bottom is the entry view's primary reading
motion: `j` moves the cursor one row, ADR 0027's auto-scroll re-aims the
diff pane at the newly selected symbol, and the reviewer reads whatever
the pane now shows.

That motion silently skips content whenever a symbol's changed block is
taller than the diff pane. ADR 0072 made the pane show the whole file's
hunks in original order, so a symbol selection only chooses a *scroll
target* — the first changed row of that symbol. Pressing `j` on a symbol
whose change spans 200 rows jumps straight from that symbol's first
changed row to the next symbol's first changed row; the 190-odd rows in
between never appear on screen. Nothing in the UI says so:

- the range bar (`┃`, painted by `ui::diff_pane::mark_range_bar_lines`)
  marks the selected symbol's extent, but only on rows that are already
  visible — its whole point was feedback when *no* scroll happens, and it
  is silent about the part of the range that is off-screen;
- the pane title's `(12-41/210)` indicator
  (`ui::scroll::scroll_indicator`) is scoped to the **file**, not to the
  selected symbol, so "there is more below" cannot be read as "there is
  more *of this symbol* below";
- the tree row itself carries no size signal at all
  (`tree::Badges`), so a row whose change is 5 lines and one whose change
  is 300 look identical.

The reviewer is therefore expected to notice the shortfall, switch focus
to the right pane, scroll, and switch back — for every symbol, with no
cue about which symbols need it. In practice the cue never arrives and
the tail of every large block goes unread.

This is not the tool's condensation concept failing. rinkaku's map is
supposed to allocate attention rather than replace reading (see
`docs/experiments/0001-map-assisted-llm-review/`). But a map that does
not admit the territory extends past its edge is a defective map, not a
concise one.

## Decision

**The diff pane counts how many of the selected symbol's rows sit outside
the viewport, and both the display and the reading motion are expressed
against that count.**

### 1. `ui::scroll` owns the count

`render_scrollable_pane` already converts between `App::right_pane_scroll`'s
logical-line unit and the wrapped display rows `Paragraph::scroll`
consumes, and is the only place that knows the wrap origins. It is
therefore the only place that can answer "is logical row *r* on screen
right now". The Diff pane passes the marked rows it already computes for
the range bar (`ui::diff_pane::range_bar_lines`) into a marks-aware
variant of that function, which returns a `ScrollablePaneRender` carrying
the clamped scroll (as before) plus the counts.

`marked_rows_outside_viewport` is pure and defined against the *visible
logical range*: the logical rows the first and last visible display rows
were wrapped from. A marked row below the last visible logical row counts
as below; above the first, as above. A logical row that is only partly
visible (its later wrapped rows fall past the bottom edge) counts as
visible — it is on screen, and the reviewer can see that it continues.

### 2. The counts render as `▲N` / `▼N` in the pane title

Appended after the existing `(first-last/total)` scroll indicator, in the
range bar's own bold yellow — the color is what ties the number to the
symbol's `┃` bar rather than to the file-scoped indicator sitting next to
it. Omitted entirely when the count is zero, so a symbol that fits on
screen shows exactly what it shows today.

### 3. `ctrl-f` / `ctrl-b` read through the change, not past it

A new pair of bindings whose one rule is:

> If the selected symbol still has rows outside the viewport in that
> direction, scroll the diff pane by one screen. Otherwise move the tree
> cursor one row.

So holding `ctrl-f` walks the whole change — every row of every symbol —
in the same "keep pressing one key" motion `j` offers today, without the
reviewer having to know in advance which symbols overflow. The step is
one *visible logical span* minus one row, so consecutive screens overlap
by a row and no row can fall between two steps.

The bindings work in both focuses on the entry view. When the right pane
is not the Diff pane there are no marked rows, the count is zero, and
both keys degrade to plain cursor movement — the same thing `j`/`k`
already do.

Advancing the cursor resets `right_pane_scroll` to 0, exactly as a plain
`j` does; ADR 0027's auto-scroll then re-aims the pane if the new row has
a diff focus of its own.

### 4. The count reaches the key handler through `DrawOutcome`

`App` has no notion of pane layout, so the counts follow the seam ADR
0026 already established for viewport height: `ui::draw` reports them in
`DrawOutcome`, `run_app` remembers the last drawn value, and hands it to
`App::handle_read_through_key` as a third dispatch step alongside
`handle_key`/`handle_scroll_key`.

## Alternatives

- **Per-symbol changed-line badges in the tree** (`Δ+12/-3`). Makes the
  shortfall visible one step earlier, but only informs — the reviewer
  still has to act on it manually, and the tree has no access to the diff
  hunks today. Complementary, not a substitute; deferred.
- **Read tracking (which rows were actually displayed), with unread
  markers per tree row.** The only option that *guarantees* coverage
  rather than making it easy. Rejected for now: it adds persistent
  per-row state to `App` and a new correctness surface (what counts as
  read), for a guarantee the `ctrl-f` motion already delivers in
  practice as long as it is the motion being used. Revisit if
  dogfooding shows reviewers mixing motions and losing track.
- **Splitting oversized symbols into hunk-level child rows in the tree.**
  Makes the tree walk itself fine-grained, but hunk boundaries are diff
  artifacts, not structure — ADR 0072 deliberately removed per-symbol
  grouping from the pane, and re-introducing diff shape into the *tree*
  would put the same coupling back one level up.
- **Making `j` itself read through.** Rejected: `j` moving the cursor by
  exactly one row is the tree's own contract, relied on by search jumps,
  `Ctrl-d` sizing, and muscle memory. A gesture that sometimes moves the
  cursor and sometimes does not must be its own key.
- **Reporting the count from the previous frame instead of the current
  one.** Simpler (no new return value from `render_scrollable_pane`), but
  the title would then describe the state *before* the last keypress,
  which is exactly when the reviewer is looking at it.

## Amendment (2026-08-24): read-through covers a symbol-less selection too

Decision 3 as first written scoped the measurement to the **selected
symbol**, and the very first real session found the hole that leaves.

A file row carries no `crate::app::DiffFocus`, so `range_bar_lines`
returns nothing, the count is zero, and `ctrl-f` moves the cursor —
past a pane that was showing one screenful of a diff several screens
tall. The same holds for every changed file rinkaku extracts no symbols
from at all: in this fork's own target codebases that is Blade
templates, config files, migrations, JSON. Those files never acquire a
symbol row, so the file row is the *only* place their diff is ever
shown, and it was exactly the place read-through declined to work.

**When the selection carries no symbol, the whole pane body is what
there is to read through.** `ui::scroll::ReadThroughRows` makes the
three cases explicit — `Symbol(rows)`, `WholeBody`, `Unmeasured` (every
pane but the Diff pane) — so the measurement no longer infers "nothing
to read" from "no symbol here".

The `▲N`/`▼N` counters stay symbol-scoped. On a `WholeBody` selection
the title's own `(first-last/total)` indicator already answers the same
question about the same content, and the counters' bold yellow earns its
meaning by matching a range bar that a symbol-less selection does not
paint; a second pair of numbers beside the first would add noise, not
information.

## Amendment (2026-09-08): the arrow keys read through on the tree

Decision 3 put read-through on `ctrl-f`/`ctrl-b` only, and the
Alternatives above explain why `j` could not have it: one row per press
is the tree's own contract, relied on by search jumps, `Ctrl-d` sizing,
and muscle memory.

That reasoning is about `j`, not about *every* key that moves the tree
cursor. `↓`/`↑` are bound to the same cursor move purely because
`translate_key` folds them into `j`/`k`, and nothing else in this crate
depends on that folding — the tree's own callers (`nav::Action::CursorUp`/
`CursorDown`, the search jumps, `Ctrl-d`'s sizing) go through `InputKey`,
never through a `KeyCode`. So the arrow keys are free where `j`/`k` are
not.

The cost of leaving them folded is the gap this ADR exists to close,
one step further out: a reviewer who has not learned `ctrl-f` walks the
tree with `↓` and reads only the first screenful of every oversized
change. "Learn the shortcut" is the same expectation the Context above
already rejected as the thing that never actually happens.

**On `Screen::Entry` + `Focus::Tree`, `↓`/`↑` translate to
`ReadThroughDown`/`ReadThroughUp`; `j`/`k` keep `Down`/`Up`.** Nothing
downstream changes — the same `handle_read_through_key` runs, with the
same one-screen step and the same spill-over into a cursor move.

This only ever adds motion. Read-through is a superset of the cursor
move: it scrolls only while the selection still has rows off-screen in
that direction, and moves the cursor by one row otherwise, so an arrow
key on a change that fits on screen does exactly what it did before.

Scoped to that one screen/focus pair:

- `Focus::Right` keeps `↓`/`↑` on its one-line scroll (ADR 0020). That
  *is* fine-grained reading of the same pane; replacing it with a
  one-screen step would take away the finer motion rather than add a
  coarser one.
- `Screen::Source` keeps its own one-line scroll, for the same reason.
- Every overlay and popup that binds Up/Down (help, jump, the review
  list) short-circuits ahead of this arm in `translate_key` and is
  untouched.
- The mouse wheel still maps to plain `Up`/`Down`
  (`translate_mouse_event`). A wheel notch is a pointing gesture at the
  pane under the pointer, not a request to advance a reading position in
  the pane next to it.

The help overlay's tree-focus group splits its `j / k / ↓ / ↑` row
accordingly: `j / k` moves the cursor, and the arrows join the
`ctrl-f / ctrl-b` row. It is the one group in the keymap where those two
halves mean different things, which is exactly why the row had to split
rather than gain a footnote.

## Amendment (2026-09-09): an expanded file row leaves the reading to its symbol rows

The 2026-09-08 amendment put read-through on `↓`/`↑`, which moved the
first amendment's `WholeBody` case onto the tree's primary walking
motion — and the first session with it found what that costs on a file
row whose symbols are listed right below it. Pressing `↓` there pages
the file's entire diff, one screen at a time, before the cursor ever
reaches the first symbol; the walk then reads the same diff again,
symbol by symbol. Every changed line is read twice, and the first pass
is the one with no map attached to it: no range bar, no header naming
what is on screen.

The first amendment's case was never that one. It was written for a
file rinkaku extracts **no** symbols from — a Blade template, a config
file, a migration — where the file row is the only row that diff will
ever be shown on. That property, not "the selection carries no
`DiffFocus`", is what makes the whole body the thing to read.

**A selection whose own rows are shown beneath it offers nothing to read
through; read-through moves the cursor onto them instead.**
`ui::scroll::ReadThroughRows` grows a fourth case,
`CoveredByChildRows`, measured as zero like `Unmeasured` but for the
opposite reason — not "this pane does not participate" but "this content
is already covered, one row down".

The condition is the row's own `nav::Row::expanded`, so it follows fold
state rather than tree shape: collapse a file with `space` and its row
reads through the whole diff again, because nothing below it does any
more. A symbol-less file's row is never expanded to begin with, so that
case is untouched.

`Focus::Right` is excluded: with the pane focused there is no tree walk
to hand the reading to, so `ctrl-f` there still pages the whole file
diff and remains the deliberate way to skim a file top to bottom. It is
the one point where the two focuses' read-through now differ, and the
help overlay's two `ctrl-f / ctrl-b` rows carry separate descriptions
accordingly — the same split the 2026-09-08 amendment already made for
the arrow keys, one row further down.

The `▲N`/`▼N` counters are unaffected: they were already symbol-scoped,
and a file row never painted them.

Alternatives:

- **Scoping this to `↓`/`↑` and letting `ctrl-f` still page the whole
  file from the tree.** The measurement is made at draw time, one frame
  before any key is known (decision 4), so the pressed key cannot reach
  it. And a `ctrl-f` that reads exactly what the next `↓` would re-read
  is the same double reading, merely opted into — the right pane already
  offers that on purpose.
- **Deciding on tree shape (does this file have symbol children) instead
  of fold state.** One less state to think about, but a collapsed file
  row would then read through nothing while nothing else showed its
  diff — precisely the hole the 2026-08-24 amendment closed.

## Amendment (2026-09-09, second): read-through from the tree is a symbol-row motion

The amendment above stopped an *expanded* file row from paging its whole
diff, and the next session with it found the rest of the same problem. A
file row folded shut with `space` still paged 41 lines of unheadered diff
before the cursor moved anywhere, and so did the row of a file rinkaku
extracts no symbols from. With the arrows as the tree's walking motion,
`↓` on a file name meant "page this file" on some rows and "step onto the
symbols" on others, and which one it meant depended on state the reviewer
had not looked at yet (fold state, or whether this file yielded symbols at
all).

**With the tree focused, only a selection that carries a `DiffFocus`
offers anything to read through.** A file or directory row is a row to
walk past: `↓`/`↑` and `ctrl-f`/`ctrl-b` move the cursor on to the symbol
rows, which read the same diff a symbol at a time with a range bar and a
header naming what is on screen.

So `ui::scroll::ReadThroughRows::CoveredByChildRows` becomes
`DeferredToRowWalk`, and its condition is the focus alone —
`ui::diff_pane::read_through_rows` takes `Focus` and the range bar's
marked rows, and needs no `nav` lookup at all. `Focus::Right` is still
where a whole file diff is a reading unit, and now the *only* place:
`ctrl-f` with the Diff pane focused remains the deliberate way to skim a
file top to bottom, which is also how a symbol-less file's diff is read
through now.

That last point is this amendment's cost: a changed file with no symbols
(a Blade template, a config file, a migration) can no longer be paged from
the tree, only from the pane one `l`/`enter` away. Accepted — the first
amendment's case is worth a key, but not at the price of the primary
walking motion behaving differently on every second row.

Alternatives:

- **Keeping the whole-body case for symbol-less files only** (test tree
  shape instead of fold state). This is what the first amendment already
  did, and it is the behavior this amendment was asked to remove: a rule
  the reviewer can predict from the row under the cursor beats one that is
  right slightly more often per row but cannot be predicted without
  knowing whether the file yielded symbols.
- **Scoping this to `↓`/`↑` and letting `ctrl-f` still page a whole file
  from the tree.** The measurement is made at draw time, one frame before
  any key is known (decision 4), so the pressed key cannot reach it — the
  same reason the first 2026-09-09 amendment gave.

## Consequences

- `render_scrollable_pane` keeps its current signature and return type
  for the four panes that pass no marks; the Diff pane calls the
  marks-aware variant. The two share one implementation, so the wrap/
  clamp path cannot drift between them.
- `DrawOutcome` grows one field, and `run_app` one remembered value and
  one dispatch branch — the same shape ADR 0026's viewport height already
  has.
- The title can now carry up to two suffixes. On a narrow pane ratatui
  clips the title; the file-scoped `(first-last/total)` is written first
  and so survives clipping, since it is the one that is meaningful for
  every pane state.
- `ctrl-f`/`ctrl-b` are not vim's exact page-forward/back semantics: they
  spill over into a cursor move at a symbol boundary. This is documented
  in the help overlay and README as "read through", not as paging.
- What a row offers to read through depends only on the row kind (does
  the selection carry a `DiffFocus`) and which pane holds the keys — not
  on fold state or tree shape, so the same cursor position measures the
  same before and after a `space`.
- A changed file rinkaku extracts no symbols from is read through with
  the Diff pane focused, not from the tree. The tree's read-through keys
  walk its row like any other row with nothing to read.

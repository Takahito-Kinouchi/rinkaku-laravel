# 0099. A TUI that says what to read next, built on what only rinkaku knows

Date: 2026-11-19

## Status

Accepted. Implemented in stages; each stage's PR names the part it
implements.

## Context

Dogfooding feedback on the TUI: the screen is cluttered, the same file
name shows up over and over, and the reader has to work out what to look
at next. The concept and the dependency ordering (ADR 0016 decision 4)
are fine; the presentation is what gets in the way.

The reader this design targets has a very small working-memory budget
and an attention span of about five seconds. Every row and every header
has to answer "what is this, and should I look at it now" without
reading the whole line.

What the tree pane showed on a real 35-file diff:

```
v ! rinkaku-core/src chg:36 api:5 fan-in:35 (cycle)
  v ! extract chg:12 api:5 fan-in:21 (cycle)
    v ! mod.rs chg:12 api:5 fan-in:21 lines:992
        ~ enum ! SymbolKind
        ~ fn ! extract_changed_symbols tests:0
        + struct ComponentContext tests:0
```

- `tests:0` (ADR 0059) is on nearly every symbol row; a signal on every
  row is no signal.
- `lines:N` is on every file row, though only files over the split
  threshold call for action.
- Three counts per row, each with a `label:` prefix, trailing the name
  at a different column on every row.
- One risky symbol puts a `!` on every ancestor: four `!` for one
  problem.
- The same glyph means different things in different panes, and the
  same thing gets different glyphs (see "One meaning per mark" below).

The Diff pane's header shows the selected file's changed line ranges
(`range: 74-94, 241-247, ...`) — identical for every symbol of the same
file, so stepping through a file's symbols repeats the same line.

Making it "look like GitHub's Files changed view" is not the goal either.
GitHub knows files and lines. rinkaku knows **which symbols use which**,
in which direction a change spreads, and how many places a change
reaches. The redesign puts that in front.

## Decision

The decisions were made against a design mock, one question at a time.

### 1. Tree rows: one mark, one number

Each symbol row carries exactly one mark for what happened and how much
it matters, and at most one number, flush right: how many other
production symbols use it.

| Mark | Meaning |
| --- | --- |
| `●` red | Contract changed and other symbols use it — may break callers |
| `○` yellow | Contract changed, few or no users |
| `+` green | Added |
| `-` red | Removed (name struck through) |
| (none, gray name) | Body-only change |

Body-only symbols are **not** folded away: they stay on their own rows,
dimmed, as today.

`tests:0` leaves the row (the Detail pane keeps it). The line count
shows only on files over the split threshold.

### 2. Two levels: file heading, then symbols

The directory hierarchy becomes a thin divider line naming the
directory once, followed by file headings (path relative to that
divider) and their symbols. Directory and file **order is unchanged**:
the topological order of ADR 0016 decision 4 is what the reader asked to
keep.

### 3. Connections light up in place

With the cursor on a symbol, every row that uses it gets `↑` and every
row it uses gets `↓`; unrelated rows fade. No navigation, search, or
pane switch is needed to see where a change reaches.

### 4. Checked marks per symbol, following the change

`space` marks the symbol under the cursor checked (`✓`) and moves to an
unchecked symbol **that uses it** — down the path the change travels —
or, when there is none, to the next `●`. A progress count of checked
symbols sits in the tree pane's title. Expand/collapse moves to `enter`
alone. Checked state is per session: closing the TUI forgets it.

Checking is per *symbol*, not per file: a file is not the unit rinkaku
reasons in.

### 5. The Diff pane header names who is affected

The diff body stays as it is (the whole file's hunks, scrolled to the
symbol). The header drops the `range:` line, which only repeated what is
the same for every symbol of the file, and instead names the symbols
that use this one, with how many of them are still unchecked —
information the diff itself cannot show.

### 6. `--pr` mode exports to the clipboard first

The export menu lists "Clipboard" before "GitHub PR review", so
confirming twice copies the notes. Posting a GitHub review stays one
`j` away. Nothing is posted to GitHub without choosing it.

### 7. One meaning per mark, one mark per meaning

Across the tree, Diff, Detail and Blast radius panes:

| Meaning | Before | After |
| --- | --- | --- |
| Removed | `x` (tree, Detail), `-` (diff) | `-` |
| Dependency cycle | `(cycle)` (tree), `! ... (cycle)` (Blast radius) | `↻` |
| Reaches (Blast radius bullet) | `-` | `↓` |
| Already shown above | `(see above)` | `(shown above)`, dimmed |
| Has a review note | `ann:N` (tree), `*` (diff gutter) | `✎N` / `✎` |
| More above / below | `… N more above` (tree), `(24-68/224)` (title), `▲N ▼N` (diff) | `▲N ▼N` |
| Test code | `[test]` magenta, `N tests` gray | gray |
| Expand state | `v` / `>` | `▾` / `▸` |
| Selected symbol's range bar | `┃` yellow | `┃` cyan |

Colors get one meaning each: red = breaks or removed, yellow = contract
changed, green = added, cyan = the current selection and its
connections, gray = can wait. Before, yellow carried five meanings and
cyan three.

`▾`/`▸`/`✎`/`↻`/`✓` are East Asian Width *Neutral*. `●`/`○`/`↑`/`↓`/`▲`/`▼`
are *Ambiguous*, like the box-drawing characters every pane border
already uses — a terminal that draws those double-width already breaks
the frame, so these add no new width risk. Emoji stay out (ADR 0028).

## Alternatives

- **Fold body-only symbols into a count.** Rejected by the reader: a
  hidden change is a change they cannot see.
- **Show only the selected symbol's hunks in the Diff pane.** Rejected:
  the reader prefers the whole-file diff and wanted only the header to
  change.
- **Show the signature before/after in the Diff pane header.** Rejected
  in the mock: it repeats the hunk directly below it.
- **Keep three right-aligned count columns.** Comparable vertically,
  but still three numbers to decode per row.
- **Per-file "viewed" checkboxes.** What GitHub already has; the unit
  rinkaku reasons in is the symbol.
- **Persist checked marks across sessions.** More to build (invalidating
  a mark when the symbol changes again); not needed yet.

## Consequences

- Most rows carry no number; the few that do are the ones worth reading.
- `space` changes meaning (expand/collapse → check and advance). Anyone
  with the old habit gets a checked mark instead of a fold; `enter`
  still folds.
- `tests:0` is only visible in the Detail pane, and a file's line count
  only on files past the warn threshold.
- A directory below a top-level divider has no row, so it cannot be
  collapsed on its own, its Detail/Blast-radius view is out of reach
  from the tree, and `--entry <that directory>` opens with the "no row
  matches" note. Its files are still there, labeled with its path.
- ADR 0059's test-coverage signal and ADR 0028's file-size data are
  unchanged in the Markdown/JSON output; only the TUI rows change.

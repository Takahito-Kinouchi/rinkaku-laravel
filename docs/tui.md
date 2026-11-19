# Interactive TUI

The TUI is the intended path for humans reading a PR
([ADR 0015](adr/0015-tui-for-humans-markdown-for-machines.md)), built
with [ratatui](https://ratatui.rs)
([ADR 0016](adr/0016-tui-crate-and-stack.md)) on a neovim-style focus
model ([ADR 0020](adr/0020-tui-interaction-model-v2.md)).

On an interactive terminal, TUI is the **default output**: bare
`rinkaku-laravel` opens the TUI on a whole-repo outline
([ADR 0017](adr/0017-whole-repo-outline-as-default-input-mode.md)), and
so do `rinkaku-laravel --pr 123`, `rinkaku-laravel --base main`, etc. The `--tui` flag
only becomes load-bearing when the output would otherwise not be TUI
(stdout is not a TTY, e.g. a diff piped in). It conflicts with
`--format`.

## Layout

- **Tree pane (left)** — the *changed files* in two levels
  ([ADR 0099](adr/0099-tui-review-flow-redesign.md)): each top-level
  directory is a divider row (`▾ app ─────`), the files under it follow
  as rows labeled by their path below that directory
  (`Http/Controllers/UserController.php`), and their symbols sit one
  level in. Directories between the divider and the file get no row of
  their own, so `--entry` with such a directory finds no row to land on.
  The order is the directory tree's own: sibling directories order topologically over
  the change graph: entry-point directories (nothing else depends on
  them) sort first, heavily-depended-upon foundations sort last. Files
  within a directory are alphabetical; symbols keep source order. When
  a diff has no cross-directory references, ranking has nothing to
  work with and the order silently falls back to alphabetical — don't
  over-read ordering in that case. The underlying dependency edges come
  from syntactic tree-sitter resolution, not a type checker, so a
  reference can occasionally be missed.

  Each symbol row carries one mark and at most one number
  ([ADR 0099](adr/0099-tui-review-flow-redesign.md)): `+` added, `○`
  signature-changed, `●` signature-changed *and* used by other
  production symbols (it may break them), `-` removed, or blank for
  body-only/unclassified — a blank-marker symbol's name also dims to
  DarkGray, since its signature didn't change and it carries less review
  weight. The number flush right is how many other *production* symbols
  use it (tests exercising it don't count). Directory and file rows
  carry no counts; a collapsed one shows `●` when it hides a `●` symbol,
  and a file over 1000 lines shows `N lines` in red. Directories in a
  dependency cycle are marked `↻`. How many tests reference a symbol is
  in its Detail pane.

  With the cursor on a symbol, the rows connected to it light up in
  place: `↑` on every production symbol that uses it, `↓` on every one
  it uses, and the other symbol rows fade — where a change reaches,
  without a jump or a pane switch. Files rinkaku couldn't analyze appear
  dimmed as `(skipped: <reason>)`; whole-test files as
  `[test] (N symbols)`. A *mixed* file's test symbols (real code
  alongside `#[cfg(test)]`-style tests in the same file) fold into a
  trailing `N tests` row (`1 test` singular), collapsed by default —
  expand it with `space`/`enter` to see them individually, dimmed and
  without any per-symbol badge, since group membership already says
  "this is test code".
- **Diff pane (right, default)** — the raw unified-diff hunks touching
  the selected row, syntax-highlighted for four of rinkaku's five
  built-in languages (HCL renders unhighlighted because its tree-sitter
  grammar publishes no highlights query)
  ([ADR 0018](adr/0018-syntax-highlight-diff-pane-via-tree-sitter.md)).
  Opens in a split (side-by-side old/new) view by default — useful for
  edits where seeing the old and new lines aligned next to each other is
  easier to scan than interleaved `-`/`+` lines. Press `v`/`V` to switch
  to unified rendering for the same hunks. Split view needs a reasonably
  wide pane; on a narrow terminal it falls back to unified regardless of
  the toggle, and the pane header notes why. The pinned header names the
  row (with the same mark as its tree row) and, for a symbol, the
  symbols that use it — `↑ 6: build_symbol · kind_abbrev ×2 · ...` —
  the one thing the hunks below cannot show.
- **Detail pane (right)** — what the cursor is on: classification,
  signature (with an old/new diff on contract change), used-by, callees,
  and — when `--deps` finds anything — depends-on (the repository-wide
  1-hop dependency index,
  [ADR 0003](adr/0003-tags-based-dependency-resolution-first-lsp-later.md));
  or, for a directory, its badge breakdown and cycle members. Dependency
  resolution runs on a background thread once the TUI has already opened
  ([ADR 0081](adr/0081-async-dependency-resolution-in-tui.md)) rather
  than blocking startup: the depends-on area shows a dimmed "resolving
  dependencies..." placeholder until it finishes (or "dependency
  resolution failed" on a background error), then fills in normally.
- **Blast radius pane (right)** — an entry-tree rooted at the cursor's
  directory/file: *"if this changes, what does it reach?"* The
  interactive equivalent of `--entry <path>`
  ([ADR 0019](adr/0019-entry-path-pivot-view.md), named per
  [ADR 0023](adr/0023-tui-blast-radius-naming.md)).

## Key bindings

Press `?` in the TUI for the always-up-to-date table.

**Tree focus (default):**

| Key(s) | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Move the cursor |
| `enter` | Expand/collapse a directory, or open a file/symbol row (moves focus right) |
| `space` | Expand or collapse (never moves focus) |
| `e` / `E` | Expand every row |
| `c` / `C` | Collapse every row |

**Right focus (Detail / Diff / Blast-radius pane):**

| Key(s) | Action |
| --- | --- |
| `j` / `k` / `↓` / `↑` | Scroll by one line |
| `ctrl-d` / `ctrl-u` | Half-page scroll |
| `gg` / `G` | Jump to top / bottom |
| `h` / `esc` | Return focus to the tree |
| `]` / `[` | Next / previous hunk (Diff pane only) |

**Global (any focus):**

| Key(s) | Action |
| --- | --- |
| `o` / `O` | Toggle topological / alphabetical ordering |
| `d` / `D` | Toggle right pane between diff and detail |
| `r` / `R` | Toggle right pane to blast radius |
| `v` / `V` | Toggle the Diff pane between unified and split (side-by-side) rendering |
| `s` / `S` | Open the source view for the symbol under the cursor |
| `gd` / `gr` | Jump to a callee / caller ([ADR 0022](adr/0022-jump-navigation-and-jumplist.md)) |
| `ctrl-o` / `ctrl-i` | Jump back / forward through the jumplist |
| `n` / `N` | Compose a review note / open the notes list ([ADR 0048](adr/0048-tui-review-actions.md)) |
| `w` / `W` | Open the current PR's page in a web browser, `--pr` mode only ([ADR 0050](adr/0050-tui-open-pr-in-browser.md)) |
| `?` | Toggle the help overlay |
| `q` / `ctrl-c` | Quit (from the entry view); `esc`/`q` also returns from the source view |

Each mark has one meaning across every pane
([ADR 0099](adr/0099-tui-review-flow-redesign.md)): `+` added, `○`
signature-changed, `●` may break its users, `-` removed, blank for
body-only, `↻` for a dependency cycle, `✎` for a review note, `▾`/`▸` for
expand state, `▲`/`▼` for rows out of view, and `↓` for what a Blast
radius pivot reaches.

## Jump navigation (`gd` / `gr`)

`gd` jumps toward a callee of the symbol under the cursor, `gr`
toward a caller — a two-key sequence, mirroring neovim's own idiom
([ADR 0022](adr/0022-jump-navigation-and-jumplist.md)). Zero candidates
shows a status-line note, one candidate jumps immediately, more than
one opens a popup (`j`/`k` to choose, `enter` to jump, `esc` to
cancel). Every jump is recorded in a jumplist; `ctrl-o` / `ctrl-i`
move back / forward, and a new jump from mid-history discards forward
entries — same behavior as neovim's jumplist.

## Source view

`s` on a symbol row opens the file scrolled to and highlighting its
line range. In `--base` and stdin mode, this reads the working tree
(not the historical commit compared against), so the highlighted
range can drift if you edit the file after opening the TUI. In `--pr`
mode, this reads the PR's head snapshot instead (`git show <head
SHA>:<path>`), so it stays consistent with the diff regardless of
what's checked out locally. `esc`/`q` returns.

When the open file has diff hunks, they're overlaid directly onto the
full file: added lines get a green background and a `+` gutter,
removed lines appear as extra red-background rows with a `-` gutter
inserted at the position they used to occupy — the diff pane's
signal, but in the context of the whole file rather than clipped to
the changed lines alone. The overlay is always on; there's no key to
toggle it off. If the file's content doesn't match the diff (in
`--base`/stdin mode, edited since the diff was produced or diffed
against a different revision), the pane falls back to its plain,
unhighlighted rendering and says so in its title.

## Combining with `--entry`

`--entry <path>` combined with the TUI opens with the cursor already on
the tree row matching `path` and the Blast-radius pane already active
(the `R` binding) — the interactive session starts exactly where
`--entry` would have rooted the Markdown/JSON tree. If no row's path
matches exactly, the TUI opens normally with a status-line note.

## Piped stdin + TUI

Keyboard input is read from the controlling terminal independently of
stdin ([ADR 0016](adr/0016-tui-crate-and-stack.md)'s addendum), so a
piped diff and interactive navigation coexist:

```sh
gh pr diff 123 | rinkaku-laravel --tui
```

Here `--tui` is required because stdout may not be a TTY under the pipe
composition.

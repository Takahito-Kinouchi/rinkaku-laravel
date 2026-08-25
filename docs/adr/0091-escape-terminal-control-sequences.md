# 0091. Escape terminal control sequences in rendered output

- Status: accepted
- Date: 2026-08-25

## Context

The same security audit that produced ADR 0090 found that control
characters travel from the change under review straight to the
reviewer's terminal.

A report is built out of strings the reviewer did not write: file paths
from the diff, symbol names and signatures sliced out of source, hunk
text. Markdown rendering writes those verbatim to stdout. A file whose
*name* contains an ESC byte is enough to reach it — nothing exotic is
required, since a contributor picks the filenames in their own PR:

    $ rinkaku < crafted.diff | cat -v
    - fn evil (src/^[[31mPWNED^[[0m^[]0;hijack^Gx.rs)

Printed to a real terminal rather than through `cat -v`, that sequence is
executed: it recolors, retitles the window, and — with enough of them —
can scroll earlier report lines out of view or overwrite them via `\r`,
which matters for output whose entire job is to be read as a summary of
what a change does. Terminals configured to allow OSC 52 extend the reach
to the reviewer's clipboard.

The TUI has the same exposure by a different route: `ratatui` writes a
cell's content through to the terminal, so an ESC byte in a diff line or
in a file opened by the source screen is emitted as-is.

JSON output is not affected: `serde_json` already escapes control
characters as ``.

## Decision

**`rinkaku_core::render::escape_control_chars` replaces every control
character a terminal acts on with a printable `\u{..}` escape**, leaving
`\n` and `\t` — the two that carry layout rather than commands — alone.
It covers C0, `DEL`, and C1 (a terminal in 8-bit mode acts on C1 as it
would on the ESC-prefixed two-byte form, so escaping only the latter
would leave the same commands reachable by another spelling).

It is applied at **choke points, not at call sites**:

- `render::render` passes Markdown, Mermaid, and Digest output through it
  on the way out — one place, and no way for a new field or section to
  slip past. `Json` is exempt, both because serde already escapes and
  because a consumer needs the real path back to open the file with it.
- `rinkaku-tui`'s `diff_view::parse_diff_hunks` escapes the path, hunk
  header, and every body line as it parses — the diff pane's only ingress
  point.
- `rinkaku-tui`'s `source::load_symbol_source` escapes the file's lines
  as it reads them, before the highlighter runs over the same lines so
  spans and text stay in step.

**Escaping happens at render time, not in the `Report`.** The `Report` is
also the JSON payload and the TUI's model; a path stored escaped would no
longer name a file anything could open.

## Alternatives

- **Strip the control characters instead of escaping them.** Rejected:
  silently deleting bytes from a name makes two different files render
  identically, and hides the fact that the input contained something
  unusual — which is itself worth seeing in a review.
- **Escape inside each renderer at every string it writes.** Rejected:
  the number of call sites is the problem, and a new section added later
  would not know to do it.
- **Escape in `ExtractedSymbol`/`ChangedFile` construction.** Rejected
  for the JSON/TUI reasons above.
- **Rely on the terminal.** Some terminals filter some sequences; none
  can be relied on, and the output is also piped into files, LLM
  prompts, and PR comments.

## Consequences

- Markdown/Mermaid/Digest output for a report containing control
  characters changes shape: `\u{1b}` appears where a raw byte used to.
  For every ordinary report the output is byte-for-byte unchanged, and
  the escape function returns a borrowed string without allocating.
- **The TUI's remaining surface is not covered yet.** Paths and
  signatures that reach the tree and detail panes from the `Report` are
  still rendered raw; unlike the diff and source panes, those are built
  in many small `Span` construction sites with no single ingress point.
  The helper is public and the pattern is set, so that pass is a
  follow-up rather than a redesign.
- `escape_control_chars` is public API of `rinkaku-core`.

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

The TUI turned out **not** to have the same exposure, which was measured
rather than assumed: `ratatui` writes a line's content through
`Buffer::set_stringn`, which skips zero-width graphemes — and every
control character is zero-width. An ESC in a diff line is dropped, and
the rest of the sequence renders as ordinary text. What the reviewer sees
is `[31m` where the file actually contains `\u{1b}[31m`, which is
harmless but misleading: the pane shows content the file does not have,
and hides content it does.

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

The two TUI sites are **fidelity, not containment**: they make the
sequence visible as `\u{1b}[31m` instead of leaving `ratatui` to swallow
the ESC and show `[31m`. They also mean those two panes do not depend on
a third-party rendering detail to be safe. `ui/mod.rs` pins that detail
with a test, so a `ratatui` upgrade that stopped filtering zero-width
graphemes would fail rather than silently open the hole.

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
- **The tree and detail panes are deliberately left unescaped.** They
  render `Report` strings through `ratatui`, which drops control
  characters on the way to the buffer (pinned by
  `ui::control_character_rendering_tests`), so there is nothing to
  contain there — and escaping them would mean touching many small
  `Span` construction sites for a display nicety. If the pinning test
  ever fails, that trade changes and those sites need the helper.
- `escape_control_chars` is public API of `rinkaku-core`.

## Amendment (2026-08-26): escape the error channel too

The escaping above covers what the *renderers* print. A failure never
reaches a renderer: it leaves `main` as an error chain on stderr, and
those chains carry diff-supplied text verbatim —
`AnalyzeError::ReadFile`'s path, `ParseError::MalformedHunkHeader`'s raw
header line.

This was not reachable through the documented workflows when ADR 0091 was
written, and that is probably why it went unnoticed: git escapes control
characters in paths itself, regardless of `core.quotePath`, so raw ones
only arrived in a diff that git did not produce. Verified rather than
assumed — `git diff` emits `"a/src/\033]0;PWNED\007x.rs"`, six printable
characters where the ESC was.

ADR 0093 removes that accident. Decoding git's quoting is what puts real
control bytes into a path for the first time, so the two changes have to
land together.

**Decision:** `main` escapes the formatted error chain before printing
it, rather than each error type escaping its own fields. Applying it at
the boundary covers every variant at once instead of whichever ones were
remembered, and leaves each type's `Display` faithful for a programmatic
consumer. `main` returns `ExitCode` rather than `anyhow::Result` to get
that hook; the printed shape (`Error: {:?}`, the `Caused by` chain, the
backtrace when enabled, exit status 1) is what `Termination` produced
before, unchanged.

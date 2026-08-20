# 0085. TUI quit confirmation and compose-cursor IME anchoring

- Status: accepted
- Date: 2026-08-20

## Context

Two independent, user-reported gaps in `rinkaku-tui`'s interactive session
land together in this ADR because both are small, both touch the same
`crate::input_translate::translate_key` / `App::handle_key` seam, and both
were fixed in the same working session.

**Accidental quit loses session state.** `q` (`crate::input_translate::translate_key`'s
lowest-priority fallback arm) and Ctrl-C both resolved straight to
`InputKey::Quit`, which `App::handle_key` turns into `should_quit = true` —
the event loop exits on the very next iteration. A TUI session can carry a
meaningful amount of state a reviewer does not want to lose to a stray
keypress: the tree cursor's position, which right pane/diff-view mode is
showing, the jumplist, and — the highest-stakes case — annotations composed
via the review-actions feature (ADR 0048) that have not yet been exported.
Nothing in this crate persists that state across a restart, so an
accidental `q` (an easy slip next to `w`/`a`/`s` on a QWERTY layout, and the
single most-reached-for key in the whole keymap) silently discards all of
it with no undo.

**IME composition has nowhere to anchor.** A reviewer opening the
annotation compose overlay (`a`, ADR 0048) and typing through a Japanese
IME left in 全角 (full-width) mode saw nothing appear at all; switching the
IME to 半角 and typing one ASCII character first made 全角 input start
working from then on. Reproducing this in a plain tmux pty confirmed the
app-side half of the bug: raw multibyte input *does* reach
`ReviewMode::Compose`'s buffer correctly (`crate::input_translate::translate_key`'s
own `ComposeChar` arm forwards every `KeyCode::Char` unmodified) — the gap
is that `rinkaku-tui` never calls `Frame::set_cursor_position` anywhere,
so `ratatui`'s hardware cursor stays hidden for the whole session
(`Frame::set_cursor_position`'s own doc comment: "If this method is not
called, the cursor will be hidden"). A terminal-based IME anchors and
engages composition at the terminal's own cursor position; with no visible
cursor to anchor to, composition silently never engaged until an unrelated
ASCII keypress happened to nudge the IME into a state where it did.

## Decision

**Quit gets a confirmation popup, following this crate's own established
popup precedent (ADR 0022's jump-target popup, ADR 0020's `?` help
overlay).** Top-level `q` on the entry view now translates to
`InputKey::RequestQuit`, a new variant `App::handle_key` turns into
`quit_confirm_open = true` rather than quitting directly. While the popup
is open (`App`'s own new `quit_confirm_open: bool` field,
`App::quit_confirm_open()`), `crate::input_translate::translate_key` takes
over the whole key space — `y`/Enter resolve to the existing
`InputKey::PopupConfirm` (reused rather than adding a dedicated variant,
mirroring how the jump popup and every review-overlay mode already share
that same pair), `n`/Esc/`q` to `InputKey::PopupCancel`, and everything
else is swallowed — the same "takes over the whole key space while open"
shape `help_open`/`jump_popup` already establish.
`App::handle_quit_confirm_key` (new, `app/quit_confirm.rs`, split out the
same way `app/jump.rs`/`app/review_key.rs` split their own popup's
dispatch) resolves `PopupConfirm` to `should_quit = true`, `PopupCancel` to
closing the popup, and swallows everything else. **Ctrl-C stays an
unconditional, ungated escape hatch**: it keeps mapping to the pre-existing
`InputKey::Quit` regardless of whether the popup is open, both in
`translate_key`'s quit-confirm-open early return (a dedicated arm, since
that block's own catch-all would otherwise swallow it like every other
unbound key) and in `handle_quit_confirm_key` (which honors `Quit` the same
way it honors `PopupConfirm`) — a reviewer who deliberately reaches for the
terminal-level "just kill it" gesture is not made to negotiate with a
popup first. The confirmation popup's prompt/title come from the locale
files (`rinkaku-tui/locales/{en,ja}.yml`'s new `quit_confirm` key), drawn
by a new `draw_quit_confirm_popup` (`ui/overlay.rs`, alongside
`draw_jump_popup`) — a narrow, deliberate amendment to ADR 0055's "every
screen but the `?` help overlay stays English-only" scope decision: an
accidental-quit guard is exactly the kind of low-frequency, high-stakes
prompt worth reading correctly regardless of locale. Full-width `ｑ`/`ｙ`/`ｎ`
behave identically to their ASCII forms in this popup for free —
`normalize_fullwidth_key` already runs ahead of every check this popup's
own short-circuit sits behind, so no popup-specific normalization logic
was needed. `q` inside every *other* overlay (help, jump popup, review
overlay, search) keeps its own pre-existing meaning unchanged; only the
one, lowest-priority `q` that used to fall through to `Quit` now opens this
popup instead.

**The compose overlay places the terminal's own cursor at the buffer's
insertion point while open.** `ui::review_overlay::draw_compose_overlay`
now calls `frame.set_cursor_position` with a position computed by a new
`compose_cursor_position` helper: the buffer's `unicode_width::UnicodeWidthStr::width`
(display columns, not `chars().count()` — the buffer can hold full-width
characters, which occupy two columns each) divided/modulo against the
popup's own inner width/height, clamped so an arbitrarily long buffer can
never place the cursor outside the popup's borders. This is not a
pixel-exact reimplementation of `ratatui::widgets::Wrap`'s own word-wrap
algorithm (the overlay renders the buffer through `Wrap { trim: false }`,
which can wrap at word boundaries the plain division does not always
match) — the one hard requirement is that the cursor never escapes the
box, which the division/clamp guarantees regardless, and the common case
this matters most for (an annotation's first few characters, exactly when
an IME needs the anchor) never wraps at all, where the two agree exactly.
No other mode/screen calls `Frame::set_cursor_position` anywhere in this
crate, so the cursor is hidden by construction (ratatui's own default)
everywhere except while this one overlay is open — nothing needed to
change to keep it out of every other screen.

## Alternatives

- **Double-`q`-to-quit** (press `q` twice within a short window, vim-style
  `dd`/`yy`) instead of a popup. Rejected: undiscoverable — nothing on
  screen would tell a first-time reviewer that a second `q` is needed, and
  ADR 0020's own discoverability rule (the `?` overlay/status-line hints
  exist so no binding requires prior knowledge) argues directly against
  adding one that does. A popup's prompt is self-describing on the very
  keypress that opens it.
- **A status-line "press q again to quit" hint instead of a popup**
  (mirroring the status line's existing transient `app.status()` message
  slot). Rejected for the identical discoverability gap as double-`q`
  above, plus a narrower one: the status line is a single unwrapped row
  already close to the 80-column budget (`ui::status`'s own doc comment,
  #196) with no room to spare for a second sentence explaining the
  confirmation gesture.
- **Enabling terminal bracketed-paste mode** as a companion IME fix (some
  terminal IME quirks are paste-adjacent). Deferred, not rejected: the
  reproduction that pinned this bug's actual cause (raw multibyte input
  already reaching the compose buffer correctly; the cursor visibility was
  the entire gap) gave no evidence bracketed paste is involved, and this
  crate does not currently enable it (`crossterm`'s raw-mode setup is
  unchanged by this ADR) — worth revisiting only if a future report shows
  a distinct paste-shaped symptom.
- **Reimplementing `ratatui::widgets::Wrap`'s word-wrap algorithm exactly**
  to compute the compose cursor's row/column with pixel-perfect fidelity
  for a wrapped buffer. Rejected as disproportionate: the compose buffer is
  free text with no length limit, but in practice an annotation's first
  few characters — never wrapped — are exactly when the cursor position
  matters for IME anchoring; a long, already-wrapped buffer being mid-edit
  is the case this feature helps least regardless of how precisely the
  cursor lands, since the IME has almost certainly already engaged by
  then.

## Consequences

- `rinkaku-tui/src/app/input_key.rs`: `InputKey` gains `RequestQuit`;
  `Quit`'s own doc comment is rewritten to describe it as the Ctrl-C-only,
  deliberately-ungated escape hatch.
- `rinkaku-tui/src/app/state.rs`: `App` gains `quit_confirm_open: bool`
  (default `false`) and `App::quit_confirm_open()`.
- `rinkaku-tui/src/app/quit_confirm.rs` (new): `App::handle_quit_confirm_key`.
- `rinkaku-tui/src/app/handle_key.rs`: a new top-priority early return
  dispatches to `handle_quit_confirm_key` while `quit_confirm_open`, slotted
  after the jump-popup check; a new `RequestQuit` arm opens the popup and is
  added to the existing `preserve_scroll` exception list so opening (and
  cancelling out of) the popup never disturbs the right pane's scroll
  offset.
- `rinkaku-tui/src/input_translate.rs`: a new `quit_confirm_open` early
  return (`y`/Enter/`n`/Esc/`q`/Ctrl-C), and the bottom-of-function fallback
  `KeyCode::Char('q') => Some(InputKey::Quit)` becomes `RequestQuit`.
- `rinkaku-tui/src/ui/overlay.rs` gains `draw_quit_confirm_popup`;
  `rinkaku-tui/src/ui/mod.rs`'s `draw` calls it while `quit_confirm_open`.
- `rinkaku-tui/locales/{en,ja}.yml` gain a `quit_confirm` key
  (`title`/`prompt`); `help.binding.quit`'s own description is reworded to
  mention the confirmation step.
- `rinkaku-tui/src/ui/review_overlay.rs` gains `compose_cursor_position`
  and a `frame.set_cursor_position` call in `draw_compose_overlay`; no
  other draw path in this crate is touched.
- No new external dependency: `unicode-width` was already a direct
  `rinkaku-tui` dependency (used by `ui::scroll`'s own width helpers), and
  `Frame::set_cursor_position`/`Terminal`'s cursor-position `Backend`
  methods are already part of the `ratatui`/`ratatui-core` version this
  crate depends on.
- Every existing `q`-quits-immediately test (`translate_key`,
  `App::handle_key`) was updated to expect `RequestQuit`/the popup instead
  of an immediate `should_quit`; Ctrl-C's own immediate-quit test is
  unchanged.

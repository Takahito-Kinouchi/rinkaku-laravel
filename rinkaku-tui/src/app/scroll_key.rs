//! ADR 0026's viewport-height-aware scroll dispatch, split out of
//! `app/handle_key.rs` (ADR 0028): [`App::handle_scroll_key`] is the
//! second step of `crate::run_app`'s two-step key dispatch (the method's
//! own doc comment), kept apart from [`App::handle_key`] so the other
//! ~20 [`InputKey`] variants that don't need viewport height don't pay the
//! plumbing cost.

use super::{App, Focus, InputKey, ReadThrough, Screen};
use crate::nav::Action;

impl App {
    /// Applies one of ADR 0026's four scroll [`InputKey`] variants against
    /// whichever pane is scrollable right now, given `viewport_height` — the
    /// last-drawn inner height of that pane, threaded in by `crate::run_app`
    /// (which knows it from [`crate::ui::draw`]'s return value) since `App`
    /// itself has no notion of the pane's layout.
    ///
    /// Split off from [`Self::handle_key`] rather than folded into it so
    /// the other ~20 [`InputKey`] variants — which don't need viewport
    /// height — don't pay the plumbing cost. `crate::run_app`'s two-step
    /// dispatch is: call [`Self::handle_key`] first for the blanket
    /// bookkeeping (`status`/`pending_prefix` reset, and — on the entry
    /// view — `preserve_scroll` bookkeeping), then call this method for
    /// the four scroll variants only. [`Self::handle_key`]'s own arms
    /// for these four variants are deliberate no-ops that document this
    /// split.
    ///
    /// Scoping:
    ///
    /// - On [`Screen::Source`], acts on `Screen::Source::scroll_top`.
    /// - On [`Screen::Entry`] + [`Focus::Right`], acts on
    ///   [`Self::right_pane_scroll`], the same field plain `j`/`k`
    ///   already updates while Right-focused.
    /// - On [`Screen::Entry`] + [`Focus::Tree`] (ADR 0026 amendment): acts
    ///   on `self.nav`'s cursor via [`crate::nav::Action::CursorPageDown`]/
    ///   [`crate::nav::Action::CursorPageUp`]/[`crate::nav::Action::CursorTop`]/
    ///   [`crate::nav::Action::CursorBottom`] — the tree pane has no scroll
    ///   offset of its own to move (it windows around the cursor at draw
    ///   time, `crate::ui::entry::draw_tree_pane`'s own doc comment), so
    ///   "half-page"/"top"/"bottom" here mean moving the cursor itself.
    ///
    /// `usize::MAX` is used as the "scroll to bottom" sentinel
    /// ([`InputKey::ScrollToBottom`]'s doc comment): the clamp-at-draw
    /// step folds it down to `total_lines - viewport_height` cleanly,
    /// so no per-pane bottom sentinel is needed here.
    /// While the `?` help overlay is open, these four variants act on
    /// [`Self::help_scroll`] instead of whatever `self.screen`/`self.focus`
    /// would otherwise imply, checked before the screen/focus match below —
    /// same priority [`Self::handle_key`] already gives the overlay, and
    /// for the same reason: without this, `crate::run_app`'s unconditional
    /// second-step call to this method (ADR 0026's two-step dispatch;
    /// `Self::handle_key`'s own arms for these variants are deliberate
    /// no-ops) would fall through to the ordinary `Screen::Entry` +
    /// `Focus::Right` branch and silently scroll the right pane *behind*
    /// the overlay while it looked closed to the reviewer.
    pub fn handle_scroll_key(mut self, key: InputKey, viewport_height: usize) -> Self {
        // Floored at 1 so a viewport too short to have a half page still
        // moves by a row, matching vim's own `Ctrl-d`/`Ctrl-u`; without it
        // these keys are silently inert on 3-4 row terminals.
        let step = (viewport_height / 2).max(1);
        if self.help_open {
            match key {
                InputKey::ScrollHalfPageDown => {
                    self.help_scroll = self.help_scroll.saturating_add(step);
                }
                InputKey::ScrollHalfPageUp => {
                    self.help_scroll = self.help_scroll.saturating_sub(step);
                }
                InputKey::ScrollToTop => {
                    self.help_scroll = 0;
                }
                InputKey::ScrollToBottom => {
                    self.help_scroll = usize::MAX;
                }
                _ => {}
            }
            return self;
        }
        match (&self.screen, self.focus, key) {
            (
                Screen::Source {
                    symbol_id,
                    scroll_top,
                },
                _,
                InputKey::ScrollHalfPageDown,
            ) => {
                let next = scroll_top.saturating_add(step);
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: next,
                };
            }
            (
                Screen::Source {
                    symbol_id,
                    scroll_top,
                },
                _,
                InputKey::ScrollHalfPageUp,
            ) => {
                let next = scroll_top.saturating_sub(step);
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: next,
                };
            }
            (Screen::Source { symbol_id, .. }, _, InputKey::ScrollToTop) => {
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: 0,
                };
            }
            (Screen::Source { symbol_id, .. }, _, InputKey::ScrollToBottom) => {
                self.screen = Screen::Source {
                    symbol_id: symbol_id.clone(),
                    scroll_top: usize::MAX,
                };
            }
            (Screen::Entry, Focus::Right, InputKey::ScrollHalfPageDown) => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_add(step);
            }
            (Screen::Entry, Focus::Right, InputKey::ScrollHalfPageUp) => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_sub(step);
            }
            (Screen::Entry, Focus::Right, InputKey::ScrollToTop) => {
                self.right_pane_scroll = 0;
            }
            (Screen::Entry, Focus::Right, InputKey::ScrollToBottom) => {
                self.right_pane_scroll = usize::MAX;
            }
            // Tree focus on the entry view (ADR 0026 amendment): the tree
            // pane has no scroll offset of its own — it windows around the
            // cursor at draw time (`crate::ui::entry::draw_tree_pane`'s own
            // doc comment) — so these four variants move `self.nav`'s
            // cursor instead of any pane's scroll state. `step` (half of
            // `viewport_height`, computed once at the top of this method)
            // is the same half-page size the right pane/source pane use.
            (Screen::Entry, Focus::Tree, InputKey::ScrollHalfPageDown) => {
                self.nav = self.nav.handle(Action::CursorPageDown(step), &self.tree);
            }
            (Screen::Entry, Focus::Tree, InputKey::ScrollHalfPageUp) => {
                self.nav = self.nav.handle(Action::CursorPageUp(step), &self.tree);
            }
            (Screen::Entry, Focus::Tree, InputKey::ScrollToTop) => {
                self.nav = self.nav.handle(Action::CursorTop, &self.tree);
            }
            (Screen::Entry, Focus::Tree, InputKey::ScrollToBottom) => {
                self.nav = self.nav.handle(Action::CursorBottom, &self.tree);
            }
            // Any non-scroll key on the source screen — deliberate no-op.
            // `crate::run_app` only calls this for the four scroll
            // variants, so this arm is defensive.
            _ => {}
        }
        self
    }

    /// Applies ADR 0088's read-through keys (`ctrl-f`/`ctrl-b`) given
    /// `read_through` — what the last drawn Diff pane frame measured about
    /// the selected symbol's own extent, threaded in by `crate::run_app`
    /// for the same reason [`Self::handle_scroll_key`]'s viewport height
    /// is: `App` cannot know it.
    ///
    /// One rule, in both directions: **while part of the selected symbol is
    /// still off-screen that way, scroll the pane; otherwise move the tree
    /// cursor.** That is what lets a single held key walk a whole PR without
    /// the reviewer having to notice in advance which symbols overflow the
    /// pane — the gap ADR 0088 exists to close.
    ///
    /// `None` (no Diff pane drawn yet, a different right pane, or the Diff
    /// pane's placeholder path) means nothing was measured, so there is
    /// nothing to read through and both keys are plain cursor movement.
    ///
    /// Moving the cursor resets `right_pane_scroll` to 0, exactly as a plain
    /// `j`/`k` does via [`Self::handle_key`]'s own blanket reset (which this
    /// key is exempt from — it has to be, since the scrolling branch must
    /// survive it). ADR 0027's auto-scroll then re-aims the pane if the row
    /// landed on has a diff focus of its own; the reset is what keeps a row
    /// that has *no* focus (a file or directory row) from inheriting the
    /// previous symbol's reading position.
    ///
    /// Only [`Screen::Entry`] acts: the source screen has its own scroll
    /// keys and no tree cursor to spill over into.
    pub fn handle_read_through_key(
        mut self,
        key: InputKey,
        read_through: Option<ReadThrough>,
    ) -> Self {
        if !matches!(self.screen, Screen::Entry) {
            return self;
        }
        let read_through = read_through.unwrap_or_default();
        match key {
            InputKey::ReadThroughDown if read_through.rows_below > 0 => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_add(read_through.step);
            }
            InputKey::ReadThroughDown => {
                self.nav = self.nav.handle(Action::CursorDown, &self.tree);
                self.right_pane_scroll = 0;
            }
            InputKey::ReadThroughUp if read_through.rows_above > 0 => {
                self.right_pane_scroll = self.right_pane_scroll.saturating_sub(read_through.step);
            }
            InputKey::ReadThroughUp => {
                self.nav = self.nav.handle(Action::CursorUp, &self.tree);
                self.right_pane_scroll = 0;
            }
            // `crate::run_app` only routes the two read-through variants
            // here, mirroring [`Self::handle_scroll_key`]'s own defensive
            // catch-all.
            _ => {}
        }
        self
    }
}

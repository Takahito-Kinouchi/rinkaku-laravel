//! The quit-confirmation popup's own key dispatch (ADR 0085), split out of
//! `app/handle_key.rs` the same way `app/jump.rs`/`app/review_key.rs`
//! already split their own popup's dispatch out: [`App::handle_quit_confirm_key`]
//! is a self-contained arm reached only via [`App::handle_key`]'s own
//! top-of-function priority check while [`super::App::quit_confirm_open`] is
//! set.

use super::{App, InputKey};

impl App {
    /// Handles one [`InputKey`] while the quit-confirmation popup (ADR
    /// 0085) is open — mirrors the jump-target popup's own "takes over the
    /// whole key space" structure (`App::handle_key`'s own doc comment on
    /// `jump_popup`): [`InputKey::PopupConfirm`] (`y`/Enter) and
    /// [`InputKey::Quit`] (Ctrl-C, the unconditional escape hatch — see
    /// that variant's own doc comment on why it is honored here too rather
    /// than swallowed like every other key) both quit; [`InputKey::PopupCancel`]
    /// (`n`/Esc/`q`) closes the popup and keeps the session running; every
    /// other key is a no-op.
    pub(super) fn handle_quit_confirm_key(mut self, key: InputKey) -> Self {
        match key {
            InputKey::PopupConfirm | InputKey::Quit => {
                self.should_quit = true;
                self.quit_confirm_open = false;
            }
            InputKey::PopupCancel => {
                self.quit_confirm_open = false;
            }
            _ => {}
        }
        self
    }
}

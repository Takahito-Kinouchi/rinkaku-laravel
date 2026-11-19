//! `space` on a symbol row (ADR 0099 decision 4): mark it checked and move
//! on down the path the change travels — or unmark it, staying put, when
//! it was already checked.

use super::App;
use crate::tree::NodeKind;

impl App {
    /// The id of the symbol row under the cursor — removed and test symbols
    /// included, unlike `Self::selected_symbol_id`: a removal is a change
    /// to read and check off like any other.
    pub(super) fn symbol_id_under_cursor(&self) -> Option<String> {
        let rows = self.nav.rows(&self.tree);
        match &rows.get(self.nav.cursor())?.node.kind {
            NodeKind::Symbol(symbol_ref) => Some(symbol_ref.id.clone()),
            _ => None,
        }
    }

    /// Toggles `id`'s checked mark; on checking it, jumps (through the
    /// jumplist, so `ctrl-o` comes back) to the next symbol to read
    /// ([`crate::checklist::next_to_review`]), or says so in the status
    /// line when nothing is left.
    pub(super) fn toggle_checked_and_advance(mut self, id: String) -> Self {
        if self.checked.remove(&id) {
            return self;
        }
        self.checked.insert(id.clone());
        match crate::checklist::next_to_review(&self.tree, &self.edges, &self.checked, &id) {
            Some(next) => self.jump_to_symbol(&next),
            None => {
                self.status = Some("note: every symbol is checked".to_string());
                self
            }
        }
    }
}

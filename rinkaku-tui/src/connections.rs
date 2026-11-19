//! Which tree rows connect to the symbol under the cursor (ADR 0099
//! decision 3): the symbols that use it and the symbols it uses, so the
//! tree can mark them in place instead of the reviewer having to jump
//! (`gd`/`gr`) or open the Detail pane to find out.
//!
//! Pure over `Report::graph`; test symbols are left out on both sides,
//! the same rule `rinkaku_core::graph::compute_fan_ins` applies to the
//! usage count the tree row shows, so `↑` rows always number that count.

use crate::nav::Row;
use crate::tree::NodeKind;
use rinkaku_core::render::Report;
use std::collections::HashSet;

/// How one tree row relates to the symbol under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    /// No highlighting is active (the cursor is not on a symbol with any
    /// connection), or the row is not a symbol row, or it is the cursor's
    /// own row.
    Neutral,
    /// This row's symbol uses the cursor's symbol: drawn with `↑`.
    UsesCursor,
    /// The cursor's symbol uses this row's symbol: drawn with `↓`.
    UsedByCursor,
    /// Highlighting is active and this symbol is connected to neither:
    /// drawn faded.
    Unrelated,
}

/// The production symbols on either side of one symbol's edges.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Connections {
    selected: Option<String>,
    users: HashSet<String>,
    used: HashSet<String>,
}

impl Connections {
    /// The connections of the symbol `selected_id`, or no highlighting at
    /// all when `selected_id` is `None` or the symbol has no production
    /// edge in either direction — fading everything but the cursor would
    /// only say "connected to nothing" at the cost of the whole tree.
    pub fn of(report: &Report, selected_id: Option<&str>) -> Self {
        let Some(selected_id) = selected_id else {
            return Self::default();
        };
        let test_ids: HashSet<&str> = report
            .graph
            .nodes
            .iter()
            .filter(|node| node.is_test)
            .map(|node| node.id.as_str())
            .collect();
        let mut users = HashSet::new();
        let mut used = HashSet::new();
        for edge in &report.graph.edges {
            if edge.to == selected_id && !test_ids.contains(edge.from.as_str()) {
                users.insert(edge.from.clone());
            }
            if edge.from == selected_id && !test_ids.contains(edge.to.as_str()) {
                used.insert(edge.to.clone());
            }
        }
        if users.is_empty() && used.is_empty() {
            return Self::default();
        }
        Self {
            selected: Some(selected_id.to_string()),
            users,
            used,
        }
    }

    /// How `row` relates to the selected symbol.
    pub fn of_row(&self, row: &Row<'_>) -> Connection {
        let (Some(selected), NodeKind::Symbol(symbol_ref)) = (&self.selected, &row.node.kind)
        else {
            return Connection::Neutral;
        };
        if self.users.contains(&symbol_ref.id) {
            Connection::UsesCursor
        } else if self.used.contains(&symbol_ref.id) {
            Connection::UsedByCursor
        } else if &symbol_ref.id == selected {
            Connection::Neutral
        } else {
            Connection::Unrelated
        }
    }
}

#[cfg(test)]
#[path = "connections_tests.rs"]
mod tests;

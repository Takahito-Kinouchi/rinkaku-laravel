//! Which tree rows connect to the symbol under the cursor (ADR 0099
//! decision 3): the symbols that use it and the symbols it uses, so the
//! tree can mark them in place instead of the reviewer having to jump
//! (`gd`/`gr`) or open the Detail pane to find out.
//!
//! Pure over `Report::graph`; test symbols are left out on both sides,
//! the same rule `rinkaku_core::graph::compute_fan_ins` applies to fan-in,
//! so the `↑` rows always number the usage count the tree row shows.

use crate::nav::Row;
use crate::tree::NodeKind;
use rinkaku_core::render::Report;
use std::collections::{HashMap, HashSet};

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

/// Every production edge of the change graph, indexed both ways — built
/// once per session (`crate::app::App::new`) so the tree's arrows, its
/// usage counts, the Diff header's user list and `space`'s next target
/// (ADR 0099 decisions 1, 3, 4, 5) all read the same set.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProductionEdges {
    users: HashMap<String, Vec<String>>,
    used: HashMap<String, Vec<String>>,
    names: HashMap<String, String>,
}

impl ProductionEdges {
    pub fn from_report(report: &Report) -> Self {
        let test_ids: HashSet<&str> = report
            .graph
            .nodes
            .iter()
            .filter(|node| node.is_test)
            .map(|node| node.id.as_str())
            .collect();
        let mut users: HashMap<String, Vec<String>> = HashMap::new();
        let mut used: HashMap<String, Vec<String>> = HashMap::new();
        for edge in &report.graph.edges {
            if test_ids.contains(edge.from.as_str()) || test_ids.contains(edge.to.as_str()) {
                continue;
            }
            push_unique(users.entry(edge.to.clone()).or_default(), &edge.from);
            push_unique(used.entry(edge.from.clone()).or_default(), &edge.to);
        }
        let names = report
            .graph
            .nodes
            .iter()
            .map(|node| (node.id.clone(), node.name.clone()))
            .collect();
        Self { users, used, names }
    }

    /// The production symbols that use `id`, in edge order.
    pub fn users_of(&self, id: &str) -> &[String] {
        self.users.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The production symbols `id` uses, in edge order.
    pub fn used_by(&self, id: &str) -> &[String] {
        self.used.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The display name of graph node `id`, or `id` itself for a node the
    /// graph does not know.
    pub fn name_of<'a>(&'a self, id: &'a str) -> &'a str {
        self.names.get(id).map(String::as_str).unwrap_or(id)
    }
}

fn push_unique(ids: &mut Vec<String>, id: &str) {
    if !ids.iter().any(|existing| existing == id) {
        ids.push(id.to_string());
    }
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
    pub fn of(edges: &ProductionEdges, selected_id: Option<&str>) -> Self {
        let Some(selected_id) = selected_id else {
            return Self::default();
        };
        let users: HashSet<String> = edges.users_of(selected_id).iter().cloned().collect();
        let used: HashSet<String> = edges.used_by(selected_id).iter().cloned().collect();
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

//! Per-symbol checked marks (ADR 0099 decision 4): how far the review has
//! got, and where `space` goes after marking a symbol checked — down the
//! path the change travels, to a symbol that uses the one just read.
//!
//! The checked set itself lives on `crate::app::App` for the session only;
//! everything here is a pure function of the tree, the edges and that set.

use crate::connections::ProductionEdges;
use crate::tree::{Badges, NodeKind, SymbolRef, Tree, TreeNode};
use std::collections::HashSet;

/// Checked and total reviewable symbols — every production symbol row in
/// `tree`, removed ones included (a removal is a change to read too).
pub fn progress(tree: &Tree, checked: &HashSet<String>) -> (usize, usize) {
    let symbols = reviewable_symbols(tree);
    let done = symbols
        .iter()
        .filter(|(symbol_ref, _)| checked.contains(&symbol_ref.id))
        .count();
    (done, symbols.len())
}

/// Where to go after checking `just_checked`, in this order:
///
/// 1. the first unchecked symbol (in tree order) that uses it — following
///    the change to the code it can break;
/// 2. otherwise the next unchecked `●` after it (wrapping around);
/// 3. otherwise the next unchecked symbol after it (wrapping around).
///
/// `None` once every reviewable symbol is checked.
pub fn next_to_review(
    tree: &Tree,
    edges: &ProductionEdges,
    checked: &HashSet<String>,
    just_checked: &str,
) -> Option<String> {
    let symbols = reviewable_symbols(tree);
    let unchecked = |symbol_ref: &&SymbolRef| !checked.contains(&symbol_ref.id);

    let users: HashSet<&str> = edges
        .users_of(just_checked)
        .iter()
        .map(String::as_str)
        .collect();
    if let Some((symbol_ref, _)) = symbols
        .iter()
        .find(|(symbol_ref, _)| users.contains(symbol_ref.id.as_str()) && unchecked(symbol_ref))
    {
        return Some(symbol_ref.id.clone());
    }

    let start = symbols
        .iter()
        .position(|(symbol_ref, _)| symbol_ref.id == just_checked)
        .map_or(0, |index| index + 1);
    let after_then_before = symbols[start..].iter().chain(&symbols[..start]);
    let may_break = after_then_before.clone().find(|(symbol_ref, badges)| {
        unchecked(symbol_ref) && crate::row_view::is_high_risk_symbol(symbol_ref, badges)
    });
    may_break
        .or_else(|| {
            after_then_before
                .clone()
                .find(|(symbol_ref, _)| unchecked(symbol_ref))
        })
        .map(|(symbol_ref, _)| symbol_ref.id.clone())
}

/// Every production symbol in `tree`, in tree (pre-)order, with its badges.
fn reviewable_symbols(tree: &Tree) -> Vec<(&SymbolRef, &Badges)> {
    fn walk<'a>(node: &'a TreeNode, out: &mut Vec<(&'a SymbolRef, &'a Badges)>) {
        match &node.kind {
            NodeKind::Symbol(symbol_ref) if !symbol_ref.is_test => {
                out.push((symbol_ref, &node.badges));
            }
            NodeKind::Section(_) | NodeKind::TestGroup { .. } => {}
            _ => node.children.iter().for_each(|child| walk(child, out)),
        }
    }
    let mut out = Vec::new();
    tree.roots.iter().for_each(|root| walk(root, &mut out));
    out
}

#[cfg(test)]
#[path = "checklist_tests.rs"]
mod tests;

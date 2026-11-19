use super::*;
use pretty_assertions::assert_eq;
use rinkaku_core::extract::{Classification, SymbolKind};
use rinkaku_core::graph::{Edge, Node, SymbolGraph};
use rinkaku_core::render::{Report, ReportOrigin};

fn symbol(id: &str, classification: Classification, fan_in: usize) -> TreeNode {
    TreeNode {
        kind: NodeKind::Symbol(SymbolRef {
            id: id.to_string(),
            name: id.to_string(),
            kind: SymbolKind::Function,
            classification: Some(classification),
            removed: false,
            is_test: false,
        }),
        path: "lib.rs".to_string(),
        badges: Badges {
            fan_in,
            ..Badges::default()
        },
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    }
}

/// `lib.rs` holding, in order: `base` (● — `late_user` and `early_user`
/// use it), `early_user`, `risky` (●, unrelated to `base`), `plain`,
/// `late_user`.
fn tree() -> Tree {
    Tree {
        roots: vec![TreeNode {
            kind: NodeKind::File,
            path: "lib.rs".to_string(),
            badges: Badges::default(),
            children: vec![
                symbol("base", Classification::SignatureChanged, 2),
                symbol("early_user", Classification::BodyOnly, 0),
                symbol("risky", Classification::SignatureChanged, 3),
                symbol("plain", Classification::BodyOnly, 0),
                symbol("late_user", Classification::Added, 0),
            ],
            skip_reason: None,
            test_symbol_count: None,
        }],
    }
}

fn edges() -> ProductionEdges {
    let node = |id: &str| Node {
        id: id.to_string(),
        path: "lib.rs".to_string(),
        name: id.to_string(),
        container: None,
        is_test: false,
    };
    let edge = |from: &str, to: &str| Edge {
        from: from.to_string(),
        to: to.to_string(),
        is_cycle: false,
    };
    ProductionEdges::from_report(&Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: ["base", "early_user", "risky", "plain", "late_user"]
                .map(node)
                .to_vec(),
            edges: vec![edge("late_user", "base"), edge("early_user", "base")],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    })
}

fn checked(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|id| id.to_string()).collect()
}

#[test]
fn should_follow_the_change_to_the_first_unchecked_user_in_tree_order() {
    let actual = next_to_review(&tree(), &edges(), &checked(&["base"]), "base");

    assert_eq!(Some("early_user".to_string()), actual);
}

#[test]
fn should_skip_users_already_checked() {
    let actual = next_to_review(&tree(), &edges(), &checked(&["base", "early_user"]), "base");

    assert_eq!(Some("late_user".to_string()), actual);
}

#[test]
fn should_go_to_the_next_may_break_symbol_when_nothing_uses_the_checked_one() {
    let actual = next_to_review(&tree(), &edges(), &checked(&["plain"]), "plain");

    // After `plain` comes `late_user` (not ●), then, wrapping past the
    // end, `base` — the first unchecked ●.
    assert_eq!(Some("base".to_string()), actual);
}

#[test]
fn should_fall_back_to_the_next_unchecked_symbol_once_no_may_break_symbol_is_left() {
    let actual = next_to_review(
        &tree(),
        &edges(),
        &checked(&["base", "risky", "early_user"]),
        "early_user",
    );

    assert_eq!(Some("plain".to_string()), actual);
}

#[test]
fn should_have_nowhere_to_go_once_every_symbol_is_checked() {
    let all = checked(&["base", "early_user", "risky", "plain", "late_user"]);

    assert_eq!(None, next_to_review(&tree(), &edges(), &all, "plain"));
}

#[test]
fn should_count_checked_symbols_against_every_production_symbol() {
    assert_eq!((2, 5), progress(&tree(), &checked(&["base", "plain"])));
}

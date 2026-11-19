use super::*;
use crate::tree::{Badges, SymbolRef, TreeNode};
use pretty_assertions::assert_eq;
use rinkaku_core::graph::{Edge, Node, SymbolGraph};
use rinkaku_core::render::ReportOrigin;

fn node(id: &str, is_test: bool) -> Node {
    Node {
        id: id.to_string(),
        path: "lib.rs".to_string(),
        name: id.to_string(),
        container: None,
        is_test,
    }
}

fn edge(from: &str, to: &str) -> Edge {
    Edge {
        from: from.to_string(),
        to: to.to_string(),
        is_cycle: false,
    }
}

/// `caller` and `test_caller` use `selected`, which uses `callee`;
/// `bystander` touches none of them.
fn report() -> Report {
    Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![],
        graph: SymbolGraph {
            nodes: vec![
                node("selected", false),
                node("caller", false),
                node("callee", false),
                node("bystander", false),
                node("test_caller", true),
            ],
            edges: vec![
                edge("caller", "selected"),
                edge("test_caller", "selected"),
                edge("selected", "callee"),
            ],
            roots: vec![],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    }
}

fn symbol_tree_node(id: &str) -> TreeNode {
    TreeNode {
        kind: NodeKind::Symbol(SymbolRef {
            id: id.to_string(),
            name: id.to_string(),
            kind: rinkaku_core::extract::SymbolKind::Function,
            classification: None,
            removed: false,
            is_test: false,
        }),
        path: "lib.rs".to_string(),
        badges: Badges::default(),
        children: vec![],
        skip_reason: None,
        test_symbol_count: None,
    }
}

fn connection_of(connections: &Connections, id: &str) -> Connection {
    let node = symbol_tree_node(id);
    connections.of_row(&Row {
        node: &node,
        depth: 1,
        expanded: false,
    })
}

#[test]
fn should_relate_every_symbol_row_to_the_selected_symbol() {
    let connections = Connections::of(&report(), Some("selected"));

    let actual = ["caller", "callee", "bystander", "test_caller", "selected"]
        .map(|id| connection_of(&connections, id));

    assert_eq!(
        [
            Connection::UsesCursor,
            Connection::UsedByCursor,
            Connection::Unrelated,
            Connection::Unrelated,
            Connection::Neutral,
        ],
        actual
    );
}

#[test]
fn should_highlight_nothing_when_the_selected_symbol_has_no_production_connection() {
    let connections = Connections::of(&report(), Some("bystander"));

    assert_eq!(Connection::Neutral, connection_of(&connections, "caller"));
}

#[test]
fn should_highlight_nothing_when_the_cursor_is_not_on_a_symbol() {
    let connections = Connections::of(&report(), None);

    assert_eq!(Connections::default(), connections);
}

#[test]
fn should_leave_non_symbol_rows_neutral_while_highlighting_is_active() {
    let connections = Connections::of(&report(), Some("selected"));
    let file = TreeNode {
        kind: NodeKind::File,
        ..symbol_tree_node("lib.rs")
    };

    let actual = connections.of_row(&Row {
        node: &file,
        depth: 0,
        expanded: true,
    });

    assert_eq!(Connection::Neutral, actual);
}

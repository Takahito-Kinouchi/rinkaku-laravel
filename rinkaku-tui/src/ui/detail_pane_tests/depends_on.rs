//! `detail_lines`'s "Depends on:" rendering (ADR 0081): the pending/failed
//! placeholder lines while `--tui`'s background resolver thread has not
//! finished (or errored), and the populated list once
//! `DependencyStatus::Ready` and `DetailView::depends_on`/
//! `omitted_dependency_matches` actually carry something.

use crate::detail::{DependencyStatus, DetailView, SignatureView};
use crate::ui::detail_pane::detail_lines;
use pretty_assertions::assert_eq;
use rinkaku_core::deps::ResolvedSymbol;
use rinkaku_core::extract::SymbolKind;

fn detail_view() -> DetailView {
    DetailView {
        id: "lib.rs::foo".to_string(),
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        path: "lib.rs".to_string(),
        container: None,
        signature: SignatureView::Current("fn foo()".to_string()),
        classification: None,
        used_by: vec![],
        callees: vec![],
        callers: vec![],
        depends_on: vec![],
        omitted_dependency_matches: 0,
    }
}

fn rendered_text(detail: &DetailView, status: DependencyStatus) -> Vec<String> {
    detail_lines(detail, status)
        .iter()
        .map(|line| line.to_string())
        .collect()
}

#[test]
fn should_show_no_depends_on_section_when_ready_and_empty() {
    let detail = detail_view();

    let actual = rendered_text(&detail, DependencyStatus::Ready);

    assert!(!actual.iter().any(|line| line.contains("Depends on")));
}

#[test]
fn should_show_resolving_placeholder_when_pending_even_with_empty_depends_on() {
    let detail = detail_view();

    let actual = rendered_text(&detail, DependencyStatus::Pending);

    let depends_on_index = actual
        .iter()
        .position(|line| line == "Depends on:")
        .expect("Depends on: heading must be present while pending");
    assert_eq!("  resolving dependencies...", actual[depends_on_index + 1]);
}

#[test]
fn should_show_failure_line_when_failed() {
    let detail = detail_view();

    let actual = rendered_text(&detail, DependencyStatus::Failed);

    let depends_on_index = actual
        .iter()
        .position(|line| line == "Depends on:")
        .expect("Depends on: heading must be present after a failure");
    assert_eq!(
        "  dependency resolution failed",
        actual[depends_on_index + 1]
    );
}

#[test]
fn should_render_dependencies_and_omitted_count_when_ready_and_populated() {
    let mut detail = detail_view();
    detail.depends_on = vec![ResolvedSymbol {
        signature: "fn helper() -> i32".to_string(),
        path: "helper.rs".to_string(),
        container: None,
    }];
    detail.omitted_dependency_matches = 2;

    let actual = rendered_text(&detail, DependencyStatus::Ready);

    let depends_on_index = actual
        .iter()
        .position(|line| line == "Depends on:")
        .expect("Depends on: heading must be present when populated");
    assert_eq!(
        "  fn helper() -> i32 (helper.rs)",
        actual[depends_on_index + 1]
    );
    assert_eq!(
        "  (+2 more definitions matched by name)",
        actual[depends_on_index + 2]
    );
}

// A pending status must win over a nonsensical hand-built `depends_on`
// content — regression guard for the exact ordering `push_depends_on_lines`
// matches on (`dependency_status` first, `detail.depends_on` only consulted
// under `Ready`): the async report merge (ADR 0081's `merge_resolved_files`)
// never actually produces this combination in practice (a report opened
// with `resolver: None` always starts with empty `dependencies`), but
// `detail_lines` itself should not silently render stale content were that
// invariant ever violated upstream.
#[test]
fn should_prefer_pending_placeholder_over_populated_depends_on() {
    let mut detail = detail_view();
    detail.depends_on = vec![ResolvedSymbol {
        signature: "fn helper() -> i32".to_string(),
        path: "helper.rs".to_string(),
        container: None,
    }];

    let actual = rendered_text(&detail, DependencyStatus::Pending);

    assert!(!actual.iter().any(|line| line.contains("helper.rs")));
    assert!(
        actual
            .iter()
            .any(|line| line == "  resolving dependencies...")
    );
}

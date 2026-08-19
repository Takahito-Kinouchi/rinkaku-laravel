//! ADR 0081: async dependency resolution for `--tui` mode.
//!
//! `main.rs`'s composition root opens the TUI with `report`'s symbols
//! carrying empty `dependencies` (`analyze_diff` was called with
//! `resolver: None`) and spawns a background `std::thread` that builds the
//! same `TagsResolver` the synchronous path would have and applies it via
//! `rinkaku_core::deps::resolve_dependencies`. This module holds the pure,
//! terminal-free half of what happens once that thread's result reaches
//! `crate::event_loop::run_app`'s event loop: the message shape the
//! `mpsc` channel carries ([`DependencyResolutionUpdate`]), and the
//! report/status transition it drives ([`apply_update`], built on
//! [`merge_resolved_files`]).

use crate::detail::DependencyStatus;
use rinkaku_core::render::{FileReport, Report};

/// What `main.rs`'s background dependency-resolution thread (ADR 0081)
/// sends back over its `mpsc` channel once it finishes — `crate::event_loop::run_app`
/// polls for this alongside ADR 0054's `update_check` receiver, using the
/// same non-blocking `try_recv` shape.
#[derive(Debug)]
pub enum DependencyResolutionUpdate {
    /// The resolved file list, in exactly the shape/order/count
    /// `Report::files` already had — every symbol's `dependencies`/
    /// `omitted_dependency_matches` populated (or left empty, when nothing
    /// matched by name). See [`merge_resolved_files`]'s own doc comment for
    /// why only `files` travels over the channel rather than a whole
    /// `Report`.
    Resolved(Vec<FileReport>),
    /// The background thread errored before it could resolve anything
    /// (e.g. a `git` failure enumerating the repository). Carries no error
    /// text: the detail pane only ever shows a fixed "dependency
    /// resolution failed" line (ADR 0081's failure-semantics decision) —
    /// the actual error is logged at the thread's own call site via
    /// `log::error!`, which reaches stderr once `--tui` mode's
    /// `DeferredLogSink` flushes after the session ends.
    Failed,
}

/// Merges a background-resolved file list into `report`, replacing only
/// `files` and leaving every other field untouched.
///
/// A whole updated [`Report`] is not sent over the channel instead (the
/// alternative ADR 0081 considered) because every field *other* than
/// `files` is provably unaffected by dependency resolution: `resolve_dependencies`
/// (`rinkaku-core::deps`) mutates only each symbol's `dependencies`/
/// `omitted_dependency_matches` in place, touching neither a symbol's
/// identity (`id`/`name`/`container`/`range`) nor `files`' own shape
/// (order, count, which file each symbol belongs to) — and `graph`/
/// `fan_ins`/`test_coverage`/`skipped`/`tests`/`file_size_*`/`removed`/
/// `non_symbol_changes` are all built from `files` *before* resolution runs
/// in `rinkaku_core::pipeline::analyze_diff` (they read `referenced_names`/
/// `referenced_method_names`/raw file content, never `dependencies` itself
/// — confirmed by reading `analyze_diff`/`build_graph`, ADR 0081's own
/// safety argument for deferring resolution at all). Sending only `files`
/// is therefore both the smaller payload (no need to re-serialize a graph
/// the receiving side already has an identical copy of) and the cleaner
/// contract: the type signature itself says "only this part can change".
pub fn merge_resolved_files(report: Report, resolved_files: Vec<FileReport>) -> Report {
    Report {
        files: resolved_files,
        ..report
    }
}

/// Applies one [`DependencyResolutionUpdate`] to `report`, returning the
/// updated report alongside the [`DependencyStatus`] `crate::app::App`
/// should move to — the pure transition `crate::event_loop::run_app`'s
/// loop calls once its `mpsc::Receiver<DependencyResolutionUpdate>` yields
/// something, extracted into its own function (rather than inlined in the
/// loop) so it is unit-testable without a live `ratatui::DefaultTerminal`,
/// mirroring every other "pure transition pulled out of `run_app`" in this
/// crate (e.g. `crate::event_loop::dispatch_search_confirm`).
pub fn apply_update(
    report: Report,
    update: DependencyResolutionUpdate,
) -> (Report, DependencyStatus) {
    match update {
        DependencyResolutionUpdate::Resolved(files) => {
            (merge_resolved_files(report, files), DependencyStatus::Ready)
        }
        DependencyResolutionUpdate::Failed => (report, DependencyStatus::Failed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::build_tree;
    use pretty_assertions::assert_eq;
    use rinkaku_core::deps::ResolvedSymbol;
    use rinkaku_core::diff::LineRange;
    use rinkaku_core::extract::{ExtractedSymbol, SymbolKind};
    use rinkaku_core::graph::SymbolGraph;

    fn symbol(id: &str, name: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            id: id.to_string(),
            name: name.to_string(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            range: LineRange { start: 1, end: 1 },
            container: None,
            referenced_names: vec![],
            referenced_method_names: vec![],
            dependencies: vec![],
            omitted_dependency_matches: 0,
            is_test: false,
            classification: None,
            previous_signature: None,
        }
    }

    fn report_with_one_unresolved_symbol() -> Report {
        Report {
            origin: rinkaku_core::render::ReportOrigin::Diff,
            files: vec![FileReport {
                path: "lib.rs".to_string(),
                symbols: vec![symbol("lib.rs::foo", "foo")],
            }],
            skipped: vec![],
            graph: SymbolGraph {
                nodes: vec![],
                edges: vec![],
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

    // The report-merge function's core contract (ADR 0081's task list):
    // `files` is replaced wholesale, while every other field — `graph` in
    // particular, since it is what `build_tree`/the tree/nav pane actually
    // reads — stays byte-for-byte the value it already had.
    #[test]
    fn should_replace_only_files_and_leave_every_other_field_untouched() {
        let original = Report {
            skipped: vec![rinkaku_core::render::SkippedFile {
                path: "assets/logo.png".to_string(),
                reason: rinkaku_core::render::SkipReason::Binary,
            }],
            ..report_with_one_unresolved_symbol()
        };
        let resolved_files = vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                dependencies: vec![ResolvedSymbol {
                    signature: "fn helper() -> i32".to_string(),
                    path: "helper.rs".to_string(),
                    container: None,
                }],
                ..symbol("lib.rs::foo", "foo")
            }],
        }];

        let actual = merge_resolved_files(original.clone(), resolved_files.clone());

        assert_eq!(resolved_files, actual.files);
        assert_eq!(original.skipped, actual.skipped);
        assert_eq!(original.graph, actual.graph);
        assert_eq!(original.fan_ins, actual.fan_ins);
        assert_eq!(original.tests, actual.tests);
    }

    // ADR 0081's tree-preservation requirement: the tree/nav/cursor/fold
    // state must survive a dependency-resolution update untouched, which
    // holds exactly when `build_tree`'s output is identical before and
    // after the merge — `build_tree` reads `files`' symbol identity
    // (`id`/`name`/`kind`/`container`/`classification`/`is_test`/removal),
    // never `dependencies`/`omitted_dependency_matches`, so a merge that
    // only changes those two fields must leave it unchanged.
    #[test]
    fn should_produce_identical_tree_before_and_after_merge() {
        let original = report_with_one_unresolved_symbol();
        let resolved_files = vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                dependencies: vec![ResolvedSymbol {
                    signature: "fn helper() -> i32".to_string(),
                    path: "helper.rs".to_string(),
                    container: None,
                }],
                omitted_dependency_matches: 3,
                ..symbol("lib.rs::foo", "foo")
            }],
        }];

        let tree_before = build_tree(&original);
        let merged = merge_resolved_files(original, resolved_files);
        let tree_after = build_tree(&merged);

        assert_eq!(tree_before, tree_after);
    }

    #[test]
    fn should_move_status_to_ready_and_merge_files_when_update_is_resolved() {
        let report = report_with_one_unresolved_symbol();
        let resolved_files = vec![FileReport {
            path: "lib.rs".to_string(),
            symbols: vec![ExtractedSymbol {
                dependencies: vec![ResolvedSymbol {
                    signature: "fn helper() -> i32".to_string(),
                    path: "helper.rs".to_string(),
                    container: None,
                }],
                ..symbol("lib.rs::foo", "foo")
            }],
        }];

        let (updated_report, status) = apply_update(
            report,
            DependencyResolutionUpdate::Resolved(resolved_files.clone()),
        );

        assert_eq!(DependencyStatus::Ready, status);
        assert_eq!(resolved_files, updated_report.files);
    }

    #[test]
    fn should_move_status_to_failed_and_leave_files_unchanged_when_update_is_failed() {
        let report = report_with_one_unresolved_symbol();
        let original_files = report.files.clone();

        let (updated_report, status) = apply_update(report, DependencyResolutionUpdate::Failed);

        assert_eq!(DependencyStatus::Failed, status);
        assert_eq!(original_files, updated_report.files);
    }
}

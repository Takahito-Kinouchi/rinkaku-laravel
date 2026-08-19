//! `--base`/`--pr` pipeline extracted from `main.rs`.
//!
//! Hosts the end-to-end functions that turn a `(base, head, cwd)` triple
//! into a rendered `Report`: `run_base_pipeline` drives the diff → analyze
//! → resolve chain, with `changed_paths`, `resolve_generated_paths`,
//! `build_resolver`, and `read_stdin_diff` as supporting steps.

use crate::cli::Cli;
use crate::generated_paths::{check_generated_paths, check_generated_paths_batch};
use crate::git::cat_file_batch::read_git_show_files_batch;
use crate::git::commands::{list_git_files, run_git_diff};
use crate::git::file_read::{
    read_git_show_file, read_prefetched_or_fallback, read_working_tree_file,
};
use crate::notes::garbage_input_note;
use crate::progress::{AnalysisProgress, SilentProgress};
use crate::spinner::AnalysisPhase;
use rayon::prelude::*;
use rinkaku_core::deps::TagsResolver;
use rinkaku_core::language::language_for_path;
use rinkaku_core::pipeline::analyze_diff;
use rinkaku_core::render::FileReport;
use std::io::{IsTerminal, Read};
use std::sync::Arc;

/// A boxed, `Send` `read_file`-shaped port (mirrors `rinkaku_core::pipeline::ReadBaseFile`'s
/// own reason for existing as a named type: clippy's `type_complexity` lint
/// on the inline form, at [`DeferredResolver`]'s own field). Named `Send`
/// rather than `Send + Sync` since [`DeferredResolver`] moves it wholesale
/// into one background `std::thread` and never shares it by reference
/// across threads — see [`DeferredResolver`]'s own doc comment.
type BoxedReadFile = Box<dyn Fn(&str) -> std::io::Result<String> + Send>;

/// Everything a `--tui`-mode background `std::thread` (ADR 0081) needs to
/// build the same [`TagsResolver`] [`build_resolver`] would otherwise have
/// built synchronously, and apply it to a report's `files` — captured once,
/// on the main thread, from exactly the arguments
/// [`run_base_pipeline`]/`main`'s stdin branch already pass to
/// `build_resolver` today. Constructed only when `--tui` mode's caller asks
/// to defer resolution (`defer_resolver: true`) *and* `cli.deps != 0` — the
/// same top-level gate `build_resolver` itself opens with — so a `--deps 0`
/// run never spawns a thread with nothing to do.
///
/// `read_file` is boxed as a trait object (rather than staying generic, the
/// way `build_resolver`'s own `diff_read_file` parameter is) specifically so
/// this struct has a concrete, nameable type `main.rs` can hold in a local
/// variable and move into `std::thread::spawn`'s closure — a bare `impl Fn`
/// return type cannot appear in a struct field. `Send` (not `Sync`): the
/// closure is moved wholesale into one background thread and never shared
/// by reference across threads, so `Send` alone is both necessary and
/// sufficient (`std::thread::spawn`'s own bound).
pub(crate) struct DeferredResolver {
    cli: Cli,
    diff_text: String,
    read_file: BoxedReadFile,
    head: Option<String>,
    cwd: Option<std::path::PathBuf>,
}

impl DeferredResolver {
    /// `pub(crate)`: both `run_base_pipeline` (this module) and `main.rs`'s
    /// stdin branch construct one directly — `--tui` mode's three input
    /// paths each capture their own `(diff_text, read_file, head, cwd)`
    /// differently (a `git show`-backed batch reader with a head SHA for
    /// `--base`/`--pr`, a plain working-tree reader with no head for
    /// stdin), so there is no single call site to hide this behind.
    pub(crate) fn new(
        cli: &Cli,
        diff_text: &str,
        read_file: impl Fn(&str) -> std::io::Result<String> + Send + 'static,
        head: Option<&str>,
        cwd: Option<&std::path::Path>,
    ) -> Self {
        Self {
            cli: cli.clone(),
            diff_text: diff_text.to_string(),
            read_file: Box::new(read_file),
            head: head.map(str::to_string),
            cwd: cwd.map(std::path::Path::to_path_buf),
        }
    }

    /// Runs on the background thread `main.rs` spawns for it (ADR 0081),
    /// after the TUI has already opened on the main thread with `files`'
    /// symbols carrying empty `dependencies` (`resolver: None` was passed to
    /// `analyze_diff` up front): builds the resolver exactly
    /// [`build_resolver`] would have, synchronously, then applies it via
    /// [`rinkaku_core::deps::resolve_dependencies`] — `files` unchanged (not
    /// even re-ordered; see that function's own doc comment) when no
    /// resolver was built at all (`build_resolver`'s own `Ok(None)` cases:
    /// `cli.deps == 0`, or an empty reference-name set).
    ///
    /// `progress` is [`SilentProgress`], not the splash/spinner ADR 0033
    /// otherwise wires `build_resolver` up with: this call runs on a thread
    /// that does not own the terminal (`--tui`'s event loop already does,
    /// on the main thread), so drawing anything here — even a buffered note
    /// meant for a later flush — has no correct place to land. See ADR
    /// 0081's "None-progress tradeoff" for the full rationale.
    pub(crate) fn resolve(&self, files: Vec<FileReport>) -> anyhow::Result<Vec<FileReport>> {
        let progress = SilentProgress;
        let resolver = build_resolver(
            &self.cli,
            &self.diff_text,
            &self.read_file,
            self.head.as_deref(),
            self.cwd.as_deref(),
            &progress,
        )?;
        Ok(match resolver {
            Some(resolver) => rinkaku_core::deps::resolve_dependencies(files, &resolver),
            None => files,
        })
    }
}

/// Builds the `'static`, `Send` head-side file reader `run_base_pipeline`
/// needs twice: once for `analyze_diff`'s own synchronous call (unchanged
/// behavior), and — only in `--tui` mode (`defer_resolver: true`) — a second
/// time for the [`DeferredResolver`] its background thread owns. Sharing
/// `head_contents` behind an [`Arc`] rather than cloning the whole prefetch
/// map into each closure keeps the second build cheap (a refcount bump, not
/// a `HashMap` copy) — the map can hold one entry per changed file, which a
/// large diff could make non-trivial to duplicate for no reason.
fn make_head_reader(
    head_contents: Arc<std::collections::HashMap<String, String>>,
    head: String,
    cwd: Option<std::path::PathBuf>,
) -> impl Fn(&str) -> std::io::Result<String> + Send + 'static {
    move |path: &str| {
        read_prefetched_or_fallback(&head_contents, path, |path| {
            read_git_show_file(cwd.as_deref(), &head, path)
        })
    }
}

/// `defer_resolver` (ADR 0081): `false` (every non-`--tui` caller) keeps
/// this function's synchronous `build_resolver` → `analyze_diff` order
/// byte-for-byte unchanged — the returned `Option<DeferredResolver>` is
/// always `None` on that path. `true` (`--tui` mode only) skips the
/// synchronous `build_resolver` call, passes `resolver: None` to
/// `analyze_diff` instead (so the TUI can open immediately with every
/// symbol's `dependencies` empty), and returns a [`DeferredResolver`]
/// instead — `Some` whenever `cli.deps != 0` (mirroring `build_resolver`'s
/// own top-level gate), `None` when `cli.deps == 0` (nothing to resolve, so
/// `main.rs` spawns no thread at all) or when the diff turned out to be
/// empty (the early return below, which never reaches dependency resolution
/// either way).
pub(crate) fn run_base_pipeline(
    cli: &Cli,
    base: &str,
    head: &str,
    cwd: Option<&std::path::Path>,
    progress: &dyn AnalysisProgress,
    defer_resolver: bool,
) -> anyhow::Result<(
    rinkaku_core::render::Report,
    String,
    Option<DeferredResolver>,
)> {
    log::debug!("diffing {base}...{head}");
    progress.set_phase(AnalysisPhase::Diffing);
    let diff_text = run_git_diff(base, head, cwd)?;
    if diff_text.trim().is_empty() {
        // ADR 0033: routed through `progress.note` rather than a bare
        // `eprintln!` — see `AnalysisProgress::note`'s own doc comment for
        // why (a raw stderr write here would interleave into the TUI's
        // alternate-screen frame stream mid-redraw during `--tui` mode).
        progress.note("note: diff is empty, nothing to analyze".to_string());
        return Ok((
            rinkaku_core::render::Report {
                origin: rinkaku_core::render::ReportOrigin::Diff,
                files: Vec::new(),
                skipped: Vec::new(),
                graph: rinkaku_core::graph::SymbolGraph {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    roots: Vec::new(),
                },
                tests: Vec::new(),
                fan_ins: Vec::new(),
                test_coverage: Vec::new(),
                file_size_warnings: Vec::new(),
                file_size_bands: vec![],
                removed: Vec::new(),
                non_symbol_changes: vec![],
            },
            diff_text,
            None,
        ));
    }

    let changed_paths = changed_paths(&diff_text)?;
    progress.set_phase(AnalysisPhase::ReadingFiles);
    // Prefetch every changed path's content on both sides via a single
    // `git cat-file --batch` child each, instead of the one `git show`
    // spawn per file the closures below used to do directly — same
    // batching strategy `build_resolver`'s repository-wide scan already
    // uses (`read_git_show_files_batch`'s own doc comment). A path missing
    // from a batch result (added/deleted on that side, or a rename whose
    // old/new path differs from what the diff parser reports) simply isn't
    // in the map; the closures below fall back to the per-file
    // `read_git_show_file`, so behavior for those paths is unchanged.
    let head_contents: Arc<std::collections::HashMap<String, String>> = Arc::new(
        read_git_show_files_batch(cwd, head, changed_paths.clone(), None)?
            .into_iter()
            .collect(),
    );
    let base_contents: std::collections::HashMap<String, String> =
        read_git_show_files_batch(cwd, base, changed_paths.clone(), None)?
            .into_iter()
            .collect();

    // Owned rather than borrowed (`cwd` itself is `Option<&Path>`, tied to
    // the caller's lifetime): `make_head_reader`'s closure must be `'static`
    // to be usable from `DeferredResolver`'s background thread, so every
    // capture — this included — has to own its data.
    let owned_cwd: Option<std::path::PathBuf> = cwd.map(std::path::Path::to_path_buf);
    let read_file = make_head_reader(
        Arc::clone(&head_contents),
        head.to_string(),
        owned_cwd.clone(),
    );
    // ADR 0014: `--base`/`--pr` mode always knows a base commit, so unlike
    // stdin mode (see `main`'s own `analyze_diff` call), a `read_base_file`
    // port is always supplied here rather than `None` — reusing the same
    // `git show <rev>:<path>` strategy `read_file` already uses for the
    // head side, just pointed at `base` instead. A path that doesn't exist
    // on the base side (e.g. a brand-new file) fails this read, which
    // `analyze_diff` treats as "no base content for this file" rather than
    // an error (see its own doc comment).
    let read_base_file = {
        let base = base.to_string();
        move |path: &str| {
            read_prefetched_or_fallback(&base_contents, path, |path| {
                read_git_show_file(cwd, &base, path)
            })
        }
    };
    let (resolver, deferred) = if defer_resolver {
        let deferred = (cli.deps != 0).then(|| {
            DeferredResolver::new(
                cli,
                &diff_text,
                make_head_reader(Arc::clone(&head_contents), head.to_string(), owned_cwd),
                Some(head),
                cwd,
            )
        });
        (None, deferred)
    } else {
        (
            build_resolver(cli, &diff_text, &read_file, Some(head), cwd, progress)?,
            None,
        )
    };
    let generated_paths = resolve_generated_paths(cli, &changed_paths, cwd);
    log::debug!("analyzing diff");
    progress.set_phase(AnalysisPhase::AnalyzingDiff);
    // ADR 0033 (amended): reports `(files_done, total)` back through
    // `progress` as `analyze_diff`'s sequential per-file loop works through
    // the diff's changed files — same closure shape as `build_resolver`'s
    // own `on_file_progress` above, since `rinkaku_core::progress::OnProgress`
    // is exactly the `Fn(usize, usize) + Sync` shape `analyze_diff` expects.
    let on_file_progress = |done: usize, total: usize| progress.report_file_progress(done, total);
    let report = analyze_diff(
        &diff_text,
        read_file,
        Some(&read_base_file),
        resolver
            .as_ref()
            .map(|r| r as &dyn rinkaku_core::deps::Resolver),
        // See sibling `analyze_diff` call in `main` for why this negates
        // `exclude_tests` rather than passing it straight through.
        !cli.exclude_tests,
        &generated_paths,
        cli.include_generated,
        Some(&on_file_progress),
    )?;
    if let Some(note) = garbage_input_note(&diff_text, &report) {
        progress.note(note.to_string());
    }
    Ok((report, diff_text, deferred))
}

pub(crate) fn changed_paths(diff_text: &str) -> anyhow::Result<Vec<String>> {
    Ok(rinkaku_core::diff::parse_unified_diff(diff_text)?
        .into_iter()
        .map(|changed_file| changed_file.path)
        .collect())
}

pub(crate) fn resolve_generated_paths(
    cli: &Cli,
    changed_paths: &[String],
    cwd: Option<&std::path::Path>,
) -> std::collections::HashSet<String> {
    if cli.include_generated {
        return std::collections::HashSet::new();
    }
    check_generated_paths(cwd, changed_paths)
}

pub(crate) fn build_resolver(
    cli: &Cli,
    diff_text: &str,
    diff_read_file: impl Fn(&str) -> std::io::Result<String>,
    head: Option<&str>,
    cwd: Option<&std::path::Path>,
    progress: &dyn AnalysisProgress,
) -> anyhow::Result<Option<TagsResolver>> {
    if cli.deps == 0 {
        return Ok(None);
    }
    progress.set_phase(AnalysisPhase::BuildingDependencyIndex);

    let reference_names =
        rinkaku_core::pipeline::collect_referenced_names(diff_text, diff_read_file)?;
    // ADR 0078: no referenced names means no lookup the index could ever
    // answer — `TagsResolver::new`'s prefilter would skip every file after
    // reading it, so skip the repository scan (listing, attribute checks,
    // blob reads) entirely instead. `None` and an empty index behave
    // identically downstream (`analyze_diff` just resolves nothing).
    if reference_names.is_empty() {
        return Ok(None);
    }

    let all_paths = list_git_files(cwd)?;
    // ADR 0078: in a monorepo, restrict the scan to the project(s) the
    // diff actually touches. `None` (single-project repository, a changed
    // file outside every project, or `--deps-scope repo`) keeps the full
    // list — scoping is a narrowing, never a correctness gate.
    let scope_roots = match cli.deps_scope {
        crate::cli::DepsScope::Repo => None,
        crate::cli::DepsScope::ChangedProjects => {
            let changed = changed_paths(diff_text)?;
            rinkaku_core::project_scope::changed_project_roots(&changed, &all_paths)
        }
    };
    if let Some(roots) = &scope_roots {
        log::debug!("dependency scan scoped to changed project roots: {roots:?}");
    }
    // ADR 0078: drop, *before any content is read*, every path the index
    // could never use — outside the scoped project roots, no registered
    // language, or a test file under `--exclude-tests`. On a typical web
    // monorepo this skips the lockfiles/images/markup that dominate `git
    // ls-files` without ever contributing a definition.
    let paths: Vec<String> = all_paths
        .into_iter()
        .filter(|path| {
            scope_roots.as_ref().is_none_or(|roots| {
                rinkaku_core::project_scope::is_within_project_roots(path, roots)
            })
        })
        .filter(|path| match language_for_path(path) {
            Some(lang) => {
                lang.contributes_to_dependency_index(path)
                    && (!cli.exclude_tests || !lang.is_test_path(path))
            }
            None => false,
        })
        .collect();
    log::debug!("building dependency index over {} files", paths.len());
    let generated_paths = if cli.include_generated {
        std::collections::HashSet::new()
    } else {
        check_generated_paths_batch(cwd, &paths)
    };
    // ADR 0079: `--base`/`--pr` mode (`head: Some(_)`) has a stable git
    // blob to key a persistent cache on, so it routes through
    // `deps_cache::resolve_indexed_files`, which skips reading/parsing
    // any candidate path whose blob OID matches a previous run's —
    // unless `--no-deps-cache` asks for the pre-cache behavior verbatim.
    // Working-tree mode (`head: None`) never takes this path: a working
    // tree file's content need not match any committed blob, so there is
    // nothing stable to key a cache on.
    if let Some(head) = head
        && !cli.no_deps_cache
    {
        let entries = crate::deps_cache::resolve_indexed_files(
            cwd,
            head,
            paths,
            language_for_path,
            &generated_paths,
            cli.include_generated,
            progress,
        )?;
        // Same CLI→core polarity flip as the `analyze_diff` /
        // `analyze_repo` calls above (ADR 0025).
        return Ok(Some(TagsResolver::from_entries(
            entries,
            !cli.exclude_tests,
        )));
    }

    // ADR 0033/0078: `(files_done, total)` for both the read loop below
    // and `TagsResolver::new`'s indexing loop — a plain closure over
    // `progress` (a `&dyn AnalysisProgress`, already object-safe), since
    // `rinkaku_core::progress::OnProgress` is exactly the `Fn(usize,
    // usize) + Sync` shape both expect.
    let on_file_progress = |done: usize, total: usize| progress.report_file_progress(done, total);
    progress.set_phase(AnalysisPhase::ReadingFiles);
    let files: Vec<(String, String)> = match head {
        // One `git cat-file --batch` child process serves every path
        // (see `read_git_show_files_batch`'s doc comment for why this
        // replaces a `git show` subprocess per file). A single
        // unresolvable path is isolated inside that call (same
        // best-effort skip as the working-tree branch below); the `?`
        // here only ever fires for a genuinely unrecoverable failure
        // (the child process itself failing to start, or the batch
        // stream desyncing), which cannot be isolated to one path. Only
        // reached here when `--no-deps-cache` was passed — the
        // cache-backed equivalent already returned above otherwise.
        Some(head) => read_git_show_files_batch(cwd, head, paths, Some(&on_file_progress))?,
        // ADR 0078: working-tree reads are independent blocking syscalls,
        // so rayon fans them out; ordered `collect` keeps the result in
        // `paths` order (the same determinism contract as
        // `TagsResolver::new`'s own parallel loop). The completion
        // counter mirrors `pipeline::analyze_repo`'s ADR 0033 pattern.
        None => {
            let completed = std::sync::atomic::AtomicUsize::new(0);
            let total = paths.len();
            paths
                .into_par_iter()
                .filter_map(|path| {
                    // A file listed by `git ls-files` can still fail to read
                    // (e.g. deleted in the working tree but not yet staged, a
                    // submodule gitlink entry) — skipped rather than failing
                    // the whole run, since the resolver's index is a
                    // best-effort aid, not a correctness-critical input.
                    let file = read_working_tree_file(&path)
                        .ok()
                        .map(|content| (path, content));
                    let done = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                    if rinkaku_core::progress::should_report_progress(done, total) {
                        on_file_progress(done, total);
                    }
                    file
                })
                .collect()
        }
    };
    progress.set_phase(AnalysisPhase::BuildingDependencyIndex);
    Ok(Some(TagsResolver::new(
        files,
        language_for_path,
        &reference_names,
        // Same CLI→core polarity flip as the `analyze_diff` /
        // `analyze_repo` calls above (ADR 0025).
        !cli.exclude_tests,
        &generated_paths,
        cli.include_generated,
        Some(&on_file_progress),
    )))
}

pub(crate) fn read_stdin_diff() -> anyhow::Result<String> {
    if std::io::stdin().is_terminal() {
        anyhow::bail!(
            "no diff input: pipe a diff via stdin (e.g. `gh pr diff 123 | rinkaku`) or pass --base <ref>"
        );
    }
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;

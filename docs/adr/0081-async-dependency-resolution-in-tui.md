# 0081. `--tui` mode: resolve dependencies asynchronously

- Status: accepted
- Date: 2026-08-19

## Context

`--tui` mode's startup pipeline (`main.rs`'s `run_analysis`, via
`pipeline::build_resolver` then `rinkaku_core::pipeline::analyze_diff`)
builds the repository-wide `TagsResolver` index (ADR 0003) *before*
`analyze_diff` runs and the terminal ever opens. On anything but a small
diff/repository, this is the visible bulk of a `--tui` run's startup
latency (ADR 0033's splash screen exists specifically to give this phase
a progress bar) — the reviewer waits on a full repository scan before
seeing anything at all, even though that scan feeds exactly one piece of
the screen.

Reading `rinkaku_core::pipeline::analyze_diff` (`rinkaku-core/src/pipeline.rs`)
confirms the resolver parameter's blast radius precisely:

- `resolver: Option<&dyn Resolver>` is used exactly once in the whole
  function, at the `resolve_dependencies(files, resolver)` call
  (`analyze_diff`, around the `let mut files = match resolver { ... }`
  line) — `None` leaves `files` untouched.
- `rinkaku_core::deps::resolve_dependencies` (`deps.rs`) mutates only two
  fields per symbol: `dependencies: Vec<ResolvedSymbol>` and
  `omitted_dependency_matches: usize`. It touches nothing else — not a
  symbol's `id`/`name`/`container`/`range`/`referenced_names`, not
  `files`' shape, order, or count.
- `build_graph`/`collect_edges` (`graph.rs`), which run *after*
  `resolve_dependencies` in `analyze_diff`, build every edge from each
  symbol's `referenced_names`/`referenced_method_names` — fields set
  during extraction, long before a resolver ever runs — and never read
  `dependencies` at all. `stamp_ids` assigns ids from `build_graph`'s own
  node list, likewise independent of `dependencies`.
- Every other `Report` field (`skipped`, `tests`, `fan_ins`,
  `test_coverage`, `file_size_warnings`/`file_size_bands`, `removed`,
  `non_symbol_changes`) is computed from `files`/raw file content before
  or independently of the `resolve_dependencies` call.

So `resolver`'s entire effect on a `Report` is: populate
`ExtractedSymbol::dependencies`/`omitted_dependency_matches` on the
symbols already in `files`. Nothing else in the pipeline — and, before
this ADR, nothing in `rinkaku-tui` at all — consumed those two fields:
`rinkaku-tui`'s detail pane showed "Used by"/"Callees"/"Callers", derived
entirely from `report.graph.edges` (intra-diff only), never from
`dependencies` (which can point anywhere in the repository the resolver
indexed). This ADR adds the "Depends on:" section the detail pane never
had, as a necessary prerequisite for having anything for the async
update to visibly fill in.

## Decision

**`--tui` mode splits the pipeline into a synchronous half and a
backgrounded half; every other display mode is untouched.** A new
`defer_resolver: bool` parameter threads through `main.rs`'s
`run_analysis` and `pipeline::run_base_pipeline`. `false` (every
non-`--tui` caller) preserves the exact synchronous
`build_resolver` → `analyze_diff` order this codebase has always had —
`pipeline::build_resolver`'s own body is untouched by this ADR, only
*where and when* it is called changes. `true` (`--tui` only) skips the
synchronous `build_resolver` call, passes `resolver: None` to
`analyze_diff` (so the report that opens the TUI has every symbol's
`dependencies` empty), and instead captures a `pipeline::DeferredResolver`
— everything `build_resolver` would need (an owned `Cli` clone, the diff
text, a boxed `'static + Send` file-reader closure, the head ref, the
cwd) — built only when `cli.deps != 0` (mirroring `build_resolver`'s own
top-level gate), so `--deps 0 --tui` spawns no thread at all.

**`main.rs` opens the TUI immediately, then spawns the deferred work on
a `std::thread`.** Once the (unresolved) `Report` is built and
`TuiSession::run` is about to be called, `main.rs` clones `report.files`,
spawns a thread that calls `DeferredResolver::resolve(files)` (which runs
`build_resolver` then `rinkaku_core::deps::resolve_dependencies`), and
sends a `rinkaku_tui::dependency_update::DependencyResolutionUpdate`
(`Resolved(Vec<FileReport>)` or `Failed`) back over an `std::sync::mpsc`
channel. This mirrors ADR 0054's existing background-version-check
pattern exactly (spawn a detached thread before/around `TuiSession::run`,
hand the receiving half of a channel into `TuiSession::run`/`run_app`),
reusing a shape this codebase already trusts rather than inventing a new
one.

**The payload is `Vec<FileReport>`, not a whole `Report`.** The Context
section's reading of `analyze_diff`/`build_graph` is the safety argument:
every `Report` field besides `files` is provably unaffected by
resolution, so sending a whole `Report` back would either duplicate data
the receiving side already holds an identical copy of, or (worse) invite
a future edit to silently start trusting the wrong copy of `graph`/
`fan_ins`/etc. `rinkaku_tui::dependency_update::merge_resolved_files(report,
resolved_files) -> Report` is the one place that combines them back
(`Report { files: resolved_files, ..report }`), with its own tests
pinning both "only `files` changes" and "`build_tree` output is identical
before and after" (the tree-preservation requirement below).

**`rinkaku-tui`'s event loop polls the channel exactly like
`update_check`.** `run_app` gains an `Option<Receiver<DependencyResolutionUpdate>>`
parameter, threaded through `TuiSession::run` the same way
`update_check: Option<Receiver<String>>` already is. The existing 100ms
crossterm poll timeout (already in place for terminal responsiveness, ADR
0054) is reused unchanged — no new timeout was introduced, and the
100ms cadence is an acceptable bound on how stale the "resolving..."
placeholder can look after the background thread actually finishes,
since a single stale frame at that granularity is imperceptible.

**Tree/nav/cursor/fold state survives untouched by construction, not by
special-casing.** `App` never stores a `Report` — `App::new` builds
`tree`/`nav` once from whichever report it was constructed with, and
every other function that needs report data (`build_detail`,
`diff_shape::build_diff_pane_content`, blast-radius/goto resolution)
takes `report: &Report` fresh as a parameter on each call, never reading
it back off `App`. So swapping which `Report` `run_app`'s loop holds
(`owned_report: Report`, reborrowed into `report: &Report` on each
iteration; replaced via `apply_update` once the channel yields a message)
cannot perturb `App`'s already-built tree/nav/cursor/fold state — nothing
downstream re-derives it. `dependency_update.rs`'s
`should_produce_identical_tree_before_and_after_merge` test pins the
underlying reason this holds: `build_tree` reads a symbol's identity
fields, never `dependencies`/`omitted_dependency_matches`.

**The detail pane's "Depends on:" area is the previously-nonexistent
piece this ADR both adds and makes async-aware.** `DetailView` gains
`depends_on: Vec<ResolvedSymbol>`/`omitted_dependency_matches: usize`
(straight from the symbol, mirroring the Markdown renderer's own
`render_dependencies`). A new `crate::detail::DependencyStatus`
(`Ready`/`Pending`/`Failed`, default `Ready`) lives on `App` — set to
`Pending` right after `App::new` only when `main.rs` actually spawned a
thread, and moved to `Ready`/`Failed` by the same `apply_update` call
that merges the resolved report. `ui::detail_pane::detail_lines` takes
`DependencyStatus` as a second parameter and renders three ways:
`Pending` → a single dimmed "resolving dependencies..." line regardless
of the symbol's (still-empty) `depends_on`; `Failed` → a single dimmed
"dependency resolution failed" line; `Ready` → the normal populated list,
or no section at all when both `depends_on` and
`omitted_dependency_matches` are empty/zero (matching Markdown's own
"nothing to show" gate, so the two surfaces agree on when a symbol
genuinely has no dependencies versus not-yet-resolved ones). The status
bar is unchanged — no new persistent hint was added there, unlike ADR
0054's update hint.

**The "None-progress" tradeoff.** The background thread calls
`build_resolver` with `progress: &SilentProgress` (`rinkaku/src/progress.rs`),
a new no-op `AnalysisProgress` implementer, rather than the splash/spinner
this codebase otherwise always wires in. `build_resolver`'s signature
takes `&dyn AnalysisProgress`, not a literal `Option<OnProgress>`, so a
true `None` is not directly expressible at that call boundary —
`SilentProgress` is the practical equivalent: `set_phase`/
`report_file_progress` are no-ops, and `note` is overridden to a no-op
too (defensively — `build_resolver`'s body never calls `.note()` today,
but this type's whole contract is "never draws", so it does not lean on
that staying true). The tradeoff this accepts: once the splash hands off
to the TUI's event loop, the reviewer gets **no visual feedback at all**
that dependency resolution is still running beyond the detail pane's own
"resolving dependencies..." placeholder — no progress bar, no percentage,
no phase label, unlike the splash's `(files_done, total)` bar for this
same work today. This is a deliberate scope cut: the thread runs
concurrently with a live, redrawing terminal the splash mechanism was
never designed to share (ADR 0033 decision 2 explicitly kept terminal
access single-threaded), and building a second, thread-safe progress
surface for one background job was judged not worth it against the
placeholder already being clear about what is happening and roughly how
long index construction tends to take.

**Failure semantics.** If the background thread's `DeferredResolver::resolve`
call errors (e.g. a transient `git` failure enumerating the repository),
the thread logs the error via `log::error!` (which still reaches stderr
once the TUI exits and `--tui` mode's `DeferredLogSink` flushes — the
same plumbing every other background/async `log::` call in this codebase
already goes through) and sends `DependencyResolutionUpdate::Failed`
instead of propagating the error anywhere that could crash the TUI. The
detail pane moves to the fixed "dependency resolution failed" line and
stays there — nothing retries automatically. The TUI itself never
observes a `Result`, let alone unwraps one, from this thread: `main.rs`'s
spawn closure is infallible by construction (it always sends *something*).

## Alternatives

- **Send a whole `Report` back over the channel instead of just
  `Vec<FileReport>`.** Rejected: the Context/Decision sections' reading
  of `analyze_diff` is precisely the argument that every other field is
  unaffected, so this would be a larger payload carrying redundant data
  for no behavioral gain, and a type signature that invites a future
  reader to wonder whether `graph`/`fan_ins`/etc. might also have
  changed, when they provably cannot.
- **Keep the splash bar running for dependency-index construction, moved
  onto the background thread's own progress callback.** Rejected per the
  None-progress tradeoff above: the splash/terminal is single-owner by
  design (ADR 0033), and the TUI's event loop already owns it by the time
  this thread runs — sharing draw access across threads was exactly what
  ADR 0033 decision 2 ruled out for the *synchronous* phases this ADR
  does not change either.
- **Poll on a separate, faster timer just for this channel.** Rejected:
  the existing 100ms crossterm poll timeout is already fine-grained
  enough that reusing it (matching `update_check`'s own precedent)
  avoids introducing a second timing knob for a negligible latency win.
- **Block the TUI's first frame on `try_recv` with a short timeout
  instead of a placeholder line.** Rejected: this reintroduces exactly
  the blocking-startup problem this ADR exists to remove, just moved a
  few hundred milliseconds later and hidden behind a timeout that would
  eventually need tuning per repository size.

## Consequences

- `rinkaku/src/cli.rs`: `Cli`/`Command` gain `#[derive(Clone)]` — every
  field is a plain owned/`Copy` value, so this costs nothing; needed so
  the background thread can hold its own owned copy.
- `rinkaku/src/progress.rs` gains `SilentProgress` (crate-private).
- `rinkaku/src/pipeline.rs` gains `DeferredResolver` (struct + `new`/
  `resolve`) and a `make_head_reader` helper (the `--base`/`--pr` head-side
  reader is now built through an `Arc`-shared prefetch map so it can be
  constructed twice — once for `analyze_diff`'s synchronous read, once
  `'static`/`Send` for the deferred thread — without doubling the `git
  cat-file --batch` prefetch cost). `run_base_pipeline` gains a
  `defer_resolver: bool` parameter and now returns a 3-tuple
  (`Report`, `String`, `Option<DeferredResolver>`); every existing test
  call site updated to pass `false` and destructure the extra `None`.
  `build_resolver`'s own body is unmodified. This pushed `pipeline.rs`
  from the file-size discipline's watch band into warn (per CLAUDE.md's
  own thresholds), so its `mod tests` moved to a sibling
  `pipeline_tests.rs` via `#[path = "pipeline_tests.rs"] mod tests;`
  (the same split mechanism `rinkaku-core`/`rinkaku-tui` already use
  elsewhere), bringing `pipeline.rs` itself back under 600 lines.
- `rinkaku/src/main.rs`: `run_analysis` gains `defer_resolver: bool` and
  `AnalyzedReport` gains `deferred_resolver`; the `DisplayMode::Tui` arm
  passes `true` and, once the (unresolved) report/session are ready,
  spawns the background thread and passes its receiver into
  `session.run`; `DisplayMode::Output` passes `false`, unchanged.
- `rinkaku-tui` gains a new `dependency_update` module
  (`DependencyResolutionUpdate`, `merge_resolved_files`, `apply_update`,
  each independently unit-tested). `crate::detail` gains
  `DependencyStatus` and `DetailView::depends_on`/
  `omitted_dependency_matches`. `App` gains a `dependency_status` field,
  `with_dependency_resolution_pending`/`dependency_status`/
  `set_dependency_status`. `ui::detail_pane::detail_lines` gains a
  `DependencyStatus` parameter and a new "Depends on:" rendering block.
  `TuiSession::run`/`run_app` both gain an
  `Option<Receiver<DependencyResolutionUpdate>>` parameter; the crate's
  free `run()` convenience wrapper passes `None`, mirroring
  `update_check`'s own precedent exactly.
- No new external dependency: the channel is `std::sync::mpsc`, the
  thread is `std::thread`, both already used for ADR 0054's version
  check.
- A future second background job wanting terminal feedback while the
  event loop already owns the screen will hit the same None-progress
  tradeoff this ADR accepts — worth revisiting together rather than
  solving per-feature if a third such job ever appears.

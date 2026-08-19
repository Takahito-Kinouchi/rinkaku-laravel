//! The pre-render analysis progress port `main.rs` threads through
//! `pipeline::run_base_pipeline`/`build_resolver` (ADR 0033), replacing the
//! bare `&Spinner` parameter those functions used to take.
//!
//! Three concerns used to be conflated in `&Spinner`/bare `eprintln!` calls
//! scattered across `main.rs`/`pipeline.rs`: "which phase is running"
//! (every display mode wants this), "how far along a file-scanning phase
//! is" (only `--tui` mode's splash screen can show a real bar for — ADR
//! 0032's stderr spinner is deliberately indeterminate), and "a one-shot
//! advisory note unrelated to phase/progress" (empty diff, garbage input,
//! an `--entry` path matching nothing, a PR base-commit fallback —
//! previously raw `eprintln!`/`log::warn!` calls at each site).
//! [`AnalysisProgress`] separates all three into their own methods so each
//! caller only overrides what it actually needs to handle differently: the
//! stderr [`crate::spinner::Spinner`] no-ops `report_file_progress` and
//! prints `note` immediately (unchanged from before this port existed),
//! while `--tui` mode's `crate::splash_progress::SplashProgress` renders a
//! real bar for the former and *buffers* the latter (see `note`'s own doc
//! comment for why).
//!
//! Kept small and defined here, on the consumer side (`pipeline.rs` is
//! what actually calls through this port) per CLAUDE.md's port convention.

use crate::spinner::AnalysisPhase;

/// What `pipeline::run_base_pipeline`/`build_resolver` report through as
/// the analysis pipeline progresses, decoupling them from which concrete
/// display mode is running (ADR 0032's stderr spinner vs. ADR 0033's TUI
/// splash screen — exactly one of the two is active per run, `main.rs`'s
/// own doc comment on why).
///
/// `Sync`: `report_file_progress` is called through
/// `rinkaku_core::pipeline::analyze_repo`'s `on_progress` port from rayon
/// worker threads during the parallel whole-repo parse (ADR 0031), so a
/// `&dyn AnalysisProgress` reference must be safe to share across those
/// threads — the same reasoning `rinkaku_core::progress::OnProgress`'s own
/// doc comment gives for its `Sync` bound.
pub(crate) trait AnalysisProgress: Sync {
    /// Called when the pipeline moves into a new named phase (resolving a
    /// PR, diffing, building the dependency index, analyzing the diff) —
    /// mirrors the granularity `Spinner::set_message`/`phase_message`
    /// already established under ADR 0032.
    fn set_phase(&self, phase: AnalysisPhase);

    /// Called with `(files_done, total)` while a file-scanning phase (the
    /// dependency index build) is in progress. A no-op default: only a
    /// caller that can actually render a determinate bar (the TUI splash)
    /// needs to override this — the stderr spinner stays indeterminate
    /// (ADR 0032), so its impl leaves this at the default no-op.
    fn report_file_progress(&self, _done: usize, _total: usize) {}

    /// A one-shot advisory note unrelated to phase/progress (empty diff,
    /// garbage input, an `--entry` path matching nothing, a PR base-commit
    /// fallback) — what every call site in `main.rs`/`pipeline.rs` used to
    /// hand straight to a bare `eprintln!` before this method existed.
    ///
    /// Defaults to printing immediately (`eprintln!`), matching that
    /// pre-existing behavior exactly — every non-TUI caller (the stderr
    /// [`crate::spinner::Spinner`]) leaves this at the default, since
    /// stderr is not being drawn over by anything in that mode. `--tui`
    /// mode's `crate::splash_progress::SplashProgress` is the one override:
    /// printing immediately there would interleave raw bytes into the
    /// terminal's alternate-screen frame stream mid-redraw (this method's
    /// whole reason for existing — a bug found by dynamic verification,
    /// `--tui --base <ref> --entry <path-matching-nothing>`), so it buffers
    /// the message instead and `main.rs` flushes the buffer to stderr only
    /// after `TuiSession` has torn down the alternate screen.
    fn note(&self, message: String) {
        eprintln!("{message}");
    }
}

/// A no-op [`AnalysisProgress`] implementation used only for the
/// background dependency-resolution thread `--tui` mode spawns (ADR 0081):
/// that thread calls `pipeline::build_resolver` — which requires an
/// `&dyn AnalysisProgress` — after the terminal has already been handed off
/// to `TuiSession::run`'s own event loop on the main thread, so unlike
/// every other implementer here (the stderr [`crate::spinner::Spinner`],
/// `--tui`'s startup-phase [`crate::splash_progress::SplashProgress`]) it
/// must never draw anything, not even buffer a note for a later flush —
/// there is no later flush point left once the event loop already owns the
/// screen. `set_phase`/`report_file_progress` are exercised routinely by
/// `build_resolver`'s own file-scanning loop; `note` is overridden purely
/// for documentation/defense (`build_resolver`'s current body never calls
/// it, but this type's whole contract is "never draws", so it does not
/// lean on that staying true) rather than any observed call site.
pub(crate) struct SilentProgress;

impl AnalysisProgress for SilentProgress {
    fn set_phase(&self, _phase: AnalysisPhase) {}

    fn note(&self, _message: String) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::Mutex;

    // A minimal fake exercising the default `report_file_progress` no-op —
    // this project's "no mocking of external processes" rule does not
    // apply here (no process/terminal is touched), but there is also
    // nothing to fake beyond recording calls, so a plain hand-rolled fake
    // is used rather than a mocking framework. `Mutex` rather than
    // `RefCell`: `AnalysisProgress: Sync` (this module's own doc comment
    // on why), so any implementer — including this test fake — must be
    // `Sync` too, which `RefCell` is not.
    struct RecordingProgress {
        phases: Mutex<Vec<AnalysisPhase>>,
    }

    impl AnalysisProgress for RecordingProgress {
        fn set_phase(&self, phase: AnalysisPhase) {
            self.phases
                .lock()
                .expect("lock must not be poisoned")
                .push(phase);
        }
    }

    #[test]
    fn should_record_phase_and_no_op_file_progress_when_using_default_impl() {
        let progress = RecordingProgress {
            phases: Mutex::new(Vec::new()),
        };

        progress.set_phase(AnalysisPhase::Diffing);
        // Must not panic and must not require an override — this is the
        // exact contract the default no-op body exists to provide.
        progress.report_file_progress(3, 10);

        let expected = vec![AnalysisPhase::Diffing];
        let actual = progress
            .phases
            .into_inner()
            .expect("lock must not be poisoned");
        assert_eq!(expected, actual);
    }

    // A second fake, this one overriding `note` — proves an implementer can
    // replace the default's immediate `eprintln!` with buffering (the exact
    // shape `crate::splash_progress::SplashProgress` needs) without
    // touching `set_phase`/`report_file_progress`. The default `note`
    // body's own `eprintln!` is deliberately left untested here: it is a
    // one-line, branchless IO call with nothing to assert against besides
    // "did it write to stderr", which this project's "no mocking of
    // external processes" convention does not ask for (mirrors ADR 0032's
    // own stance on not unit-testing `Spinner`'s IO).
    struct BufferingProgress {
        notes: Mutex<Vec<String>>,
    }

    impl AnalysisProgress for BufferingProgress {
        fn set_phase(&self, _phase: AnalysisPhase) {}

        fn note(&self, message: String) {
            self.notes
                .lock()
                .expect("lock must not be poisoned")
                .push(message);
        }
    }

    #[test]
    fn should_buffer_note_instead_of_printing_when_note_is_overridden() {
        let progress = BufferingProgress {
            notes: Mutex::new(Vec::new()),
        };

        progress.note("note: diff is empty, nothing to analyze".to_string());
        progress.note("note: no symbols under nonexistent/path.rs".to_string());

        let expected = vec![
            "note: diff is empty, nothing to analyze".to_string(),
            "note: no symbols under nonexistent/path.rs".to_string(),
        ];
        let actual = progress
            .notes
            .into_inner()
            .expect("lock must not be poisoned");
        assert_eq!(expected, actual);
    }
}

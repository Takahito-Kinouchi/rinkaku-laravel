//! CLI argument definitions extracted from `main.rs`.

use clap::Parser;
use rinkaku_core::render::OutputFormat;

/// rinkaku (輪郭) — condense PR diffs into signatures and their dependencies.
//
// `Clone` (ADR 0081): `--tui` mode's background dependency-resolution
// thread needs its own owned copy of the parsed CLI to call `build_resolver`
// with, since it runs after `main`'s own `cli` binding is long past the
// point a borrow could reach across the `std::thread::spawn` boundary.
// Every field is a plain owned value (`String`/`Option<String>`/`bool`/`u8`/
// `Copy` enums), so deriving `Clone` costs nothing beyond what `main` already
// pays once per run.
#[derive(Parser, Debug, Clone, PartialEq, Eq)]
#[command(
    name = "rinkaku-laravel",
    // ADR 0083: identifies this as a fork build in its own `--version`
    // output, rather than the bare semver upstream `rinkaku --version`
    // prints — the whole point of the rename is that the two binaries
    // must never be mistaken for one another, and the version string is
    // the one place a reviewer is likely to actually check that.
    version = concat!(env!("CARGO_PKG_VERSION"), " (fork of hiro-o918/rinkaku)"),
    about,
    long_about = None
)]
pub(crate) struct Cli {
    /// Base ref to diff against (runs `git diff <base>...<head>` instead
    /// of reading from stdin).
    #[arg(long, conflicts_with = "pr")]
    pub(crate) base: Option<String>,

    /// Head ref to diff against `base`. Only meaningful together with
    /// `--base`; defaults to `HEAD`.
    //
    // `conflicts_with = "pr"` only fires when `--head` is explicitly
    // passed (clap does not treat a default value as "provided"), which
    // is exactly what's wanted: `--pr` resolves its own head commit via
    // `gh`, so an explicit `--head` alongside `--pr` would be silently
    // ignored otherwise.
    #[arg(long, default_value = "HEAD", conflicts_with = "pr")]
    pub(crate) head: String,

    /// GitHub PR to review, as a URL
    /// (`https://github.com/<owner>/<repo>/pull/<number>`) or a bare PR
    /// number (`76`). A bare number must be run inside a local clone of
    /// the target repository; a URL also works from any other directory
    /// by auto-cloning into a cache. Requires `gh` installed and
    /// authenticated. Mutually exclusive with `--base`/`--head`, which
    /// diff local refs instead.
    // See ADR 0004 for the resolve-then-fetch design and ADR 0005 for the
    // auto-clone-into-cache behavior this drives in `main`.
    #[arg(long)]
    pub(crate) pr: Option<String>,

    /// Output format. Defaults to Markdown, or the interactive TUI when
    /// stdout is a terminal and neither `--format` nor `--tui` was given.
    //
    // See `resolve_display_mode` (ADR 0017) for how the default is picked.
    //
    // `Option` rather than a `default_value_t` is what makes "the user
    // didn't pass --format" observable at all; a defaulted `Format` field
    // would look identical to an explicit `--format md`, which
    // `resolve_display_mode` needs to tell apart (see its own doc comment).
    #[arg(long, value_enum, conflicts_with = "tui")]
    pub(crate) format: Option<Format>,

    /// Open the interactive terminal UI instead of printing Markdown/JSON.
    /// The input flow (stdin / `--base` / `--pr`) is unchanged — `--tui`
    /// only changes the output stage, once a `Report` is built. Conflicts
    /// with `--format`, since the two are mutually exclusive output stages
    /// rather than combinable options.
    // See ADR 0015/0016 for the design behind the TUI itself.
    #[arg(long, default_value_t = false)]
    pub(crate) tui: bool,

    /// Whether to resolve each changed symbol's 1-hop dependencies. `1`
    /// (default) runs the tags-based `Resolver` over every file tracked by
    /// `git ls-files`; `0` skips resolution entirely (no
    /// `Resolver::resolve` calls), which is faster and avoids the
    /// repo-wide indexing pass.
    // See ADR 0003 for the 1-hop dependency resolution design.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(0..=1))]
    pub(crate) deps: u8,

    /// Which slice of the repository the `--deps` index covers.
    /// `changed-projects` (default) restricts the scan to the project
    /// root(s) — nearest directory carrying a manifest such as
    /// `composer.json`/`package.json`/`Cargo.toml` — containing the
    /// diff's changed files, so a monorepo holding several applications
    /// only reads and parses the one(s) actually touched. Falls back to
    /// the whole repository whenever scoping cannot narrow anything (a
    /// single-project repository, or a changed file outside every
    /// project). `repo` always scans every tracked file — the escape
    /// hatch for cross-project dependencies scoping would hide.
    // See ADR 0078 for the scoping design.
    #[arg(long, value_enum, default_value_t = DepsScope::ChangedProjects)]
    pub(crate) deps_scope: DepsScope,

    /// Disable the persistent dependency-index cache (ADR 0079). By
    /// default, `--base`/`--pr` mode caches each indexed file's
    /// definitions on disk keyed by its git blob OID, so a later run
    /// against the same repository only re-reads and re-parses files that
    /// actually changed since the last run. Pass this flag to always
    /// rebuild the index from scratch, e.g. to rule out a stale cache
    /// while debugging, or on a one-shot CI runner where the cache would
    /// never be reused anyway.
    #[arg(long, default_value_t = false)]
    pub(crate) no_deps_cache: bool,

    /// Exclude test symbols from the "Change graph"/"Definitions" output
    /// and summarize their per-file counts under a "Tests" section
    /// instead. Without this flag, test symbols appear in the graph and
    /// definitions like any other symbol — the default the Markdown/JSON
    /// output is designed around now that its primary audience is LLM
    /// reviewers (humans read the TUI, which badges test files rather than
    /// omitting them).
    // See ADR 0025 (superseding the ADR 0009 default) for the rationale
    // behind this default.
    #[arg(long, default_value_t = false)]
    pub(crate) exclude_tests: bool,

    /// Include files `.gitattributes` marks `-diff` or `linguist-generated`
    /// instead of skipping them by default.
    // See ADR 0010 for why generated files are skipped by default.
    #[arg(long, default_value_t = false)]
    pub(crate) include_generated: bool,

    /// Re-root the change graph at this path before rendering: entry
    /// points become the symbols under `path` that nothing else under
    /// that same path depends on, and dependency trees still expand
    /// outward through the full graph as usual. This is a viewpoint
    /// change, not a filter — symbols outside `path` are neither hidden
    /// nor excluded from analysis, only no longer eligible to be roots
    /// themselves. Compatible with every input mode (stdin/`--base`/`--pr`/
    /// whole-repo) and with `--tui`: combined, the TUI opens with the
    /// cursor already on the tree row matching `path` and the right pane
    /// already showing its Blast radius, rather than requiring the
    /// reviewer to find the row and press `R` themselves.
    // See ADR 0019 for the re-rooting design and ADR 0023 for the
    // `rinkaku_tui::run` `entry_path` parameter this drives.
    #[arg(long)]
    pub(crate) entry: Option<String>,
}
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Md,
    Json,
    /// A human-oriented call/dependency graph as a mermaid `flowchart`
    /// document — opt-in, aimed at GitHub's native mermaid rendering in PR
    /// comments/descriptions, not the default Markdown output.
    // See ADR 0021 for the design behind this output format.
    Mermaid,
    /// A slim "API changes" list — one line per added/signature-changed/
    /// removed symbol, nothing else — meant for a PR comment's collapsed
    /// details section rather than full-report reading.
    // See ADR 0036 for the design behind this output format.
    Digest,
}
impl From<Format> for OutputFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::Md => OutputFormat::Markdown,
            Format::Json => OutputFormat::Json,
            Format::Mermaid => OutputFormat::Mermaid,
            Format::Digest => OutputFormat::Digest,
        }
    }
}

/// `--deps-scope` values — see the field's doc comment.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepsScope {
    /// Scan only the project(s) containing changed files (default).
    ChangedProjects,
    /// Scan every tracked file, regardless of which project changed.
    Repo,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn should_default_to_markdown_head_and_no_base_when_no_args_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_tui_when_tui_flag_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: true,
        };
        let actual = Cli::parse_from(["rinkaku", "--tui"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_tui_and_format_given_together() {
        let actual = Cli::try_parse_from(["rinkaku", "--tui", "--format", "json"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_reject_format_and_tui_given_together_regardless_of_argument_order() {
        // clap's conflicts_with is declared on `format` (see Cli's own
        // `#[arg(...)]` attribute), but conflicts are symmetric regardless
        // of which flag declares the attribute or which one is passed
        // first on the command line — this pins that symmetry rather than
        // only ever exercising the --tui-first ordering above.
        let actual = Cli::try_parse_from(["rinkaku", "--format", "json", "--tui"]);

        assert!(actual.is_err());
    }
    #[test]
    fn should_set_base_when_base_flag_given() {
        let expected = Cli {
            base: Some("main".to_string()),
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--base", "main"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_base_and_head_when_both_flags_given() {
        let expected = Cli {
            base: Some("main".to_string()),
            head: "feature-branch".to_string(),
            pr: None,
            format: None,
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--base", "main", "--head", "feature-branch"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_format_json_when_format_flag_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: Some(Format::Json),
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--format", "json"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_unknown_format_value() {
        let actual = Cli::try_parse_from(["rinkaku", "--format", "yaml"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_set_deps_zero_when_deps_flag_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 0,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--deps", "0"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_deps_value_outside_zero_or_one() {
        let actual = Cli::try_parse_from(["rinkaku", "--deps", "2"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_set_exclude_tests_when_exclude_tests_flag_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: true,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--exclude-tests"]);

        assert_eq!(expected, actual);
    }

    // ADR 0025's flipped default: with no test-related flag given, the
    // parsed `Cli` must land on `exclude_tests: false` — i.e. tests are
    // included in Change graph/Definitions by default. The
    // `should_default_to_markdown_head_and_no_base_when_no_args_given`
    // above already exercises the whole default `Cli` shape, but this
    // one pins the ADR 0025 decision specifically so a future default
    // flip has to update this test on purpose rather than by
    // consequence.
    #[test]
    fn should_default_to_including_tests_when_no_flag_given() {
        let actual = Cli::parse_from(["rinkaku"]);

        assert!(!actual.exclude_tests);
    }

    // Companion to the above: passing the old `--include-tests` flag
    // must now fail parsing, so a stale script surfaces as an error
    // instead of silently doing nothing. Pins the CLI break called out
    // in ADR 0025's Consequences.
    #[test]
    fn should_reject_the_removed_include_tests_flag() {
        let actual = Cli::try_parse_from(["rinkaku", "--include-tests"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_set_include_generated_when_include_generated_flag_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: false,
            include_generated: true,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--include-generated"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_set_entry_when_entry_flag_given() {
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: false,
            include_generated: false,
            entry: Some("src/api".to_string()),
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--entry", "src/api"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_verify_cli_definition() {
        // clap's own consistency check (duplicate args, invalid
        // configuration, etc.) — mirrors skem's `Cli::command().debug_assert()`
        // convention for catching CLI wiring mistakes at test time.
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    // ADR 0083: pins the fork-identifying `--version` string — a bare
    // upstream-style semver here would make this fork's binary
    // indistinguishable from an `hiro-o918/rinkaku` install of the same
    // name in `--version` output, the same collision the package rename
    // itself exists to remove.
    #[test]
    fn should_print_fork_identifying_version_string() {
        use clap::CommandFactory;
        let version = Cli::command().get_version().expect("version").to_string();

        assert!(
            version.contains("fork of hiro-o918/rinkaku"),
            "version string {version:?} does not identify this as a fork"
        );
    }

    #[test]
    fn should_set_pr_when_pr_flag_given() {
        // Also covers that `--pr` alone (no explicit `--head`) parses
        // successfully: `--head` has a default value, so clap's
        // `conflicts_with` must not fire unless `--head` was actually
        // passed on the command line — this is the behavior the ADR relies
        // on to let `--pr` reuse the `Cli` struct's `head` field internally
        // without users needing to omit an unrelated flag.
        let expected = Cli {
            base: None,
            head: "HEAD".to_string(),
            pr: Some("76".to_string()),
            format: None,
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache: false,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        };
        let actual = Cli::parse_from(["rinkaku", "--pr", "76"]);

        assert_eq!(expected, actual);
    }

    #[test]
    fn should_reject_pr_and_base_together() {
        let actual = Cli::try_parse_from(["rinkaku", "--pr", "76", "--base", "main"]);

        assert!(actual.is_err());
    }

    #[test]
    fn should_reject_pr_and_explicit_head_together() {
        let actual = Cli::try_parse_from(["rinkaku", "--pr", "76", "--head", "feature-branch"]);

        assert!(actual.is_err());
    }
}

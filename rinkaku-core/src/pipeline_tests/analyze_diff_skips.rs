//! Per-file skip cases in [`analyze_diff`]: the reasons a changed file is
//! reported rather than analyzed — deleted, binary, a path that leaves the
//! repository (ADR 0090), an unsupported language, and a pure rename with
//! no changed ranges.
//!
//! Split out of `analyze_diff.rs` (ADR 0028) when the containment tests
//! ADR 0090 added pushed that file past the warn threshold. These share a
//! shape the rest of that file does not: each asserts a `SkippedFile`
//! entry *and* that the reader was never called.

use super::{empty_graph, fake_reader};
use crate::diff::LineRange;
use crate::extract::{ExtractedSymbol, SymbolKind};
use crate::file_size::{FileSizeBand, FileSizeEntry};
use crate::graph::{Node, SymbolGraph};
use crate::pipeline::analyze_diff;
use crate::render::{FileReport, Report, ReportOrigin, SkipReason, SkippedFile};
use pretty_assertions::assert_eq;
use rstest::rstest;
use std::collections::{HashMap, HashSet};

#[test]
fn should_skip_deleted_file_without_reading_it() {
    let diff = "\
diff --git a/src/old.rs b/src/old.rs
deleted file mode 100644
index 4b825dc..0000000
--- a/src/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn a() {}
-fn b() {}
";
    // No entry in the map: if the pipeline tried to read a deleted
    // file, this would return an Err and fail the test.
    let read_file = fake_reader(HashMap::new());

    let expected = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![SkippedFile {
            path: "src/old.rs".to_string(),
            reason: SkipReason::Deleted,
        }],
        graph: empty_graph(),
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };
    let actual = analyze_diff(
        diff,
        read_file,
        None,
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    assert_eq!(expected, actual);
}

#[test]
fn should_skip_binary_file_without_reading_it() {
    let diff = "\
diff --git a/assets/logo.png b/assets/logo.png
index e69de29..4b825dc 100644
Binary files a/assets/logo.png and b/assets/logo.png differ
";
    let read_file = fake_reader(HashMap::new());

    let expected = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![SkippedFile {
            path: "assets/logo.png".to_string(),
            reason: SkipReason::Binary,
        }],
        graph: empty_graph(),
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };
    let actual = analyze_diff(
        diff,
        read_file,
        None,
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    assert_eq!(expected, actual);
}

// ADR 0090. A diff arriving on stdin is attacker-controlled input, so a
// path that walks out of the repository — through `..` or by being
// absolute in the first place — must be reported and never read. The
// reader here panics on any call, which is the actual claim: nothing
// touches the filesystem for such an entry.
#[rstest]
#[case::parent_escape(
    "\
diff --git a/../secret/creds.py b/../secret/creds.py
@@ -1,0 +1,1 @@
+API_TOKEN = \"leaked\"
",
    "../secret/creds.py"
)]
#[case::absolute_path(
    // `diff --git a//etc/passwd b//etc/passwd` — the doubled slash is
    // what survives stripping the `b/` prefix, and is how a crafted diff
    // names an absolute path at all.
    "\
diff --git a//etc/shadow b//etc/shadow
@@ -1,0 +1,1 @@
+root:x:0:0
",
    "/etc/shadow"
)]
fn should_skip_file_without_reading_it_when_diff_path_escapes_the_repository(
    #[case] diff: &str,
    #[case] expected_path: &str,
) {
    let read_file = |path: &str| -> std::io::Result<String> {
        panic!("a path outside the repository must never be read, got: {path}")
    };

    let expected = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![SkippedFile {
            path: expected_path.to_string(),
            reason: SkipReason::OutsideRepository,
        }],
        graph: empty_graph(),
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };
    let actual = analyze_diff(
        diff,
        read_file,
        None,
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    assert_eq!(expected, actual);
}

// ADR 0090's amendment. A lexically-clean path can still be a symlink
// pointing out of the tree, which only the `read_file` port — the side
// that touches the filesystem — can detect. It reports that refusal as
// `InvalidInput`, and this is what `analyze_diff` must do with it: report
// the entry as outside the repository, exactly like the lexical cases
// above, and carry on with the rest of the diff. The second file is in
// the same diff to pin that "carry on" half: a refusal must not fail the
// run.
#[test]
fn should_skip_file_and_still_analyze_the_rest_when_the_reader_refuses_it_as_uncontained() {
    let diff = "\
diff --git a/stolen.py b/stolen.py
@@ -1,0 +1,1 @@
+def leaked(): pass
diff --git a/src/app.py b/src/app.py
@@ -1,1 +1,1 @@
-def real_change(): pass
+def real_change(x): pass
";
    let read_file = |path: &str| -> std::io::Result<String> {
        match path {
            "stolen.py" => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "refusing to read stolen.py: it resolves outside the repository",
            )),
            _ => Ok("def real_change(x): pass\n".to_string()),
        }
    };

    let expected = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/app.py".to_string(),
            symbols: vec![ExtractedSymbol {
                id: "src/app.py::real_change".to_string(),
                name: "real_change".to_string(),
                kind: SymbolKind::Function,
                signature: "def real_change(x):".to_string(),
                range: LineRange { start: 1, end: 1 },
                container: None,
                dependencies: vec![],
                referenced_names: vec![],
                referenced_method_names: vec![],
                omitted_dependency_matches: 0,
                classification: None,
                is_test: false,
                previous_signature: None,
            }],
        }],
        skipped: vec![SkippedFile {
            path: "stolen.py".to_string(),
            reason: SkipReason::OutsideRepository,
        }],
        graph: SymbolGraph {
            nodes: vec![Node {
                id: "src/app.py::real_change".to_string(),
                path: "src/app.py".to_string(),
                name: "real_change".to_string(),
                container: None,
                is_test: false,
            }],
            edges: vec![],
            roots: vec!["src/app.py::real_change".to_string()],
        },
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![FileSizeEntry {
            path: "src/app.py".to_string(),
            line_count: 1,
            band: FileSizeBand::Normal,
        }],
        removed: vec![],
        non_symbol_changes: vec![],
    };
    let actual = analyze_diff(
        diff,
        read_file,
        None,
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("a contained-path refusal must not fail the run");

    assert_eq!(expected, actual);
}

// A rename carries a second, independently chosen path: `rename from`
// names the base side, which `analyze_diff` reads separately. Both sides
// have to be contained, not just the one the entry is reported under.
#[test]
fn should_skip_rename_without_reading_it_when_only_its_base_side_path_escapes() {
    let diff = "\
diff --git a/../secret/creds.py b/src/lib.rs
similarity index 90%
rename from ../secret/creds.py
rename to src/lib.rs
@@ -1,0 +1,1 @@
+fn foo() {}
";
    let read_file = |path: &str| -> std::io::Result<String> {
        panic!("a rename with an escaping base-side path must never be read, got: {path}")
    };

    let expected = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![SkippedFile {
            path: "src/lib.rs".to_string(),
            reason: SkipReason::OutsideRepository,
        }],
        graph: empty_graph(),
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };
    let actual = analyze_diff(
        diff,
        read_file,
        None,
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    assert_eq!(expected, actual);
}

#[test]
fn should_skip_file_with_unsupported_language_without_reading_it() {
    // `.rb` has no registered `LanguageSupport` (only rs/go/py/ts/tsx
    // are registered — see `language.rs`), so this exercises the
    // unsupported-extension path without relying on an extension that
    // might gain support later.
    let diff = "\
diff --git a/src/main.rb b/src/main.rb
index e69de29..4b825dc 100644
--- a/src/main.rb
+++ b/src/main.rb
@@ -1,1 +1,2 @@
 def foo
+  1
";
    let read_file = fake_reader(HashMap::new());

    let expected = Report {
        origin: ReportOrigin::Diff,
        files: vec![],
        skipped: vec![SkippedFile {
            path: "src/main.rb".to_string(),
            reason: SkipReason::UnsupportedLanguage,
        }],
        graph: empty_graph(),
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };
    let actual = analyze_diff(
        diff,
        read_file,
        None,
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    assert_eq!(expected, actual);
}

// Regression test: a pure rename (or a mode-change-only diff) has no
// hunks, so `changed_ranges` is empty and there is no content change to
// extract symbols from. The pipeline must not call `read_file` for such
// an entry — doing so is wasted IO for content that, by construction,
// yields no symbols (`extract_changed_symbols` already returns `[]` for
// an empty `changed_ranges`). Reported as a `FileReport` with empty
// `symbols` rather than a `SkippedFile`: the file *is* supported and
// was looked at, it just has nothing to report, which is a different
// situation from `SkipReason`'s "could not be analyzed" cases.
#[test]
fn should_skip_reading_pure_rename_with_no_changed_ranges() {
    let diff = "\
diff --git a/src/old_name.rs b/src/new_name.rs
similarity index 100%
rename from src/old_name.rs
rename to src/new_name.rs
";
    // No entry in the map: if the pipeline tried to read the renamed
    // file, this would return an Err and fail the test.
    let read_file = fake_reader(HashMap::new());

    let expected = Report {
        origin: ReportOrigin::Diff,
        files: vec![FileReport {
            path: "src/new_name.rs".to_string(),
            symbols: vec![],
        }],
        skipped: vec![],
        graph: empty_graph(),
        tests: vec![],
        fan_ins: vec![],
        test_coverage: vec![],
        file_size_warnings: vec![],
        file_size_bands: vec![],
        removed: vec![],
        non_symbol_changes: vec![],
    };
    let actual = analyze_diff(
        diff,
        read_file,
        None,
        None,
        true,
        &HashSet::new(),
        true,
        None,
    )
    .expect("analyze should succeed");

    assert_eq!(expected, actual);
}

//! Tests for `crate::deps_cache`, split from the source file (ADR 0028):
//! `DepsCache`-level unit tests (load/save/prune, corruption and version
//! mismatches) plus `build_resolver_integration`, which exercises the
//! whole cache-backed path end to end through `pipeline::build_resolver`
//! against real git fixture repositories (ADR 0079).

use super::*;
use pretty_assertions::assert_eq;

fn entry(name: &str) -> IndexEntry {
    IndexEntry {
        name: name.to_string(),
        signature: format!("fn {name}()"),
        container: None,
        is_test: false,
    }
}

#[test]
fn should_load_empty_cache_when_file_does_not_exist() {
    let dir = tempfile::TempDir::new().expect("create tempdir");

    let actual = DepsCache::load(dir.path());

    assert_eq!(DepsCache::empty(), actual);
}

#[test]
fn should_load_empty_cache_when_file_is_corrupted() {
    let dir = tempfile::TempDir::new().expect("create tempdir");
    std::fs::create_dir_all(cache_dir(dir.path())).expect("create cache dir");
    std::fs::write(cache_file_path(dir.path()), "not valid json{{{")
        .expect("write corrupted cache file");

    let actual = DepsCache::load(dir.path());

    assert_eq!(DepsCache::empty(), actual);
}

#[test]
fn should_load_empty_cache_when_format_version_does_not_match() {
    let dir = tempfile::TempDir::new().expect("create tempdir");
    let mut stale = DepsCache::empty();
    stale.format_version = FORMAT_VERSION + 1;
    stale.insert(
        "src/lib.rs".to_string(),
        "abc123".to_string(),
        false,
        vec![entry("helper")],
    );
    std::fs::create_dir_all(cache_dir(dir.path())).expect("create cache dir");
    std::fs::write(
        cache_file_path(dir.path()),
        serde_json::to_string(&stale).expect("serialize"),
    )
    .expect("write stale cache file");

    let actual = DepsCache::load(dir.path());

    assert_eq!(DepsCache::empty(), actual);
}

#[test]
fn should_load_empty_cache_when_rinkaku_version_does_not_match() {
    let dir = tempfile::TempDir::new().expect("create tempdir");
    let mut stale = DepsCache::empty();
    stale.rinkaku_version = "0.0.0-does-not-exist".to_string();
    stale.insert(
        "src/lib.rs".to_string(),
        "abc123".to_string(),
        false,
        vec![entry("helper")],
    );
    std::fs::create_dir_all(cache_dir(dir.path())).expect("create cache dir");
    std::fs::write(
        cache_file_path(dir.path()),
        serde_json::to_string(&stale).expect("serialize"),
    )
    .expect("write stale cache file");

    let actual = DepsCache::load(dir.path());

    assert_eq!(DepsCache::empty(), actual);
}

#[test]
fn should_round_trip_entries_through_save_and_load() {
    let dir = tempfile::TempDir::new().expect("create tempdir");
    let mut cache = DepsCache::empty();
    cache.insert(
        "src/lib.rs".to_string(),
        "abc123".to_string(),
        false,
        vec![entry("helper")],
    );
    let candidates: HashSet<String> = ["src/lib.rs".to_string()].into_iter().collect();
    cache
        .save(dir.path(), &candidates)
        .expect("save must succeed");

    let reloaded = DepsCache::load(dir.path());

    assert_eq!(
        Some(&entry("helper")),
        reloaded
            .get("src/lib.rs", "abc123")
            .map(|cached| &cached.entries[0])
    );
}

#[test]
fn should_return_none_when_cached_oid_does_not_match() {
    let mut cache = DepsCache::empty();
    cache.insert(
        "src/lib.rs".to_string(),
        "abc123".to_string(),
        false,
        vec![entry("helper")],
    );

    let actual = cache.get("src/lib.rs", "different-oid");

    assert_eq!(None, actual);
}

#[test]
fn should_prune_paths_not_in_candidate_set_when_saving() {
    let dir = tempfile::TempDir::new().expect("create tempdir");
    let mut cache = DepsCache::empty();
    cache.insert(
        "src/lib.rs".to_string(),
        "abc123".to_string(),
        false,
        vec![entry("helper")],
    );
    cache.insert(
        "src/removed.rs".to_string(),
        "def456".to_string(),
        false,
        vec![entry("gone")],
    );
    let candidates: HashSet<String> = ["src/lib.rs".to_string()].into_iter().collect();

    cache
        .save(dir.path(), &candidates)
        .expect("save must succeed");
    let reloaded = DepsCache::load(dir.path());

    assert!(reloaded.get("src/lib.rs", "abc123").is_some());
    assert!(reloaded.get("src/removed.rs", "def456").is_none());
}

mod should_exclude_as_generated_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn should_exclude_when_content_marker_is_set_and_generated_not_included() {
        let actual = should_exclude_as_generated("a.rs", true, &HashSet::new(), false);
        assert!(actual);
    }

    #[test]
    fn should_exclude_when_path_is_attribute_generated_and_generated_not_included() {
        let generated_paths: HashSet<String> = ["a.rs".to_string()].into_iter().collect();
        let actual = should_exclude_as_generated("a.rs", false, &generated_paths, false);
        assert!(actual);
    }

    #[test]
    fn should_not_exclude_when_include_generated_is_true() {
        let generated_paths: HashSet<String> = ["a.rs".to_string()].into_iter().collect();
        let actual = should_exclude_as_generated("a.rs", true, &generated_paths, true);
        assert!(!actual);
    }

    #[test]
    fn should_not_exclude_when_neither_signal_is_set() {
        let actual = should_exclude_as_generated("a.rs", false, &HashSet::new(), false);
        assert_eq!(false, actual);
    }
}

/// End-to-end tests exercising `pipeline::build_resolver`'s cache-backed
/// path (ADR 0079) against real git fixture repositories — the level a
/// user's `--base`/`--pr` run actually calls, rather than
/// `resolve_indexed_files` in isolation.
mod build_resolver_integration {
    use crate::cli::Cli;
    use crate::pipeline::build_resolver;
    use crate::spinner::Spinner;
    use crate::test_util::{init_repo_with_committed_file, run_git};
    use pretty_assertions::assert_eq;
    use rinkaku_core::deps::Resolver;

    /// A diff whose sole changed symbol references `helper` — enough for
    /// `collect_referenced_names` to produce a non-empty set, which is
    /// what `build_resolver` requires before it does any indexing at all.
    const DIFF_REFERENCING_HELPER: &str = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,3 @@
 fn run() {
-    old();
+    helper();
 }
";

    fn read_changed_main_rs(_: &str) -> std::io::Result<String> {
        Ok("fn run() {\n    helper();\n}\n".to_string())
    }

    fn cli_with_deps_cache(no_deps_cache: bool) -> Cli {
        Cli {
            command: None,
            base: None,
            head: "HEAD".to_string(),
            pr: None,
            format: None,
            deps: 1,
            deps_scope: crate::cli::DepsScope::ChangedProjects,
            no_deps_cache,
            exclude_tests: false,
            include_generated: false,
            entry: None,
            tui: false,
        }
    }

    fn cache_file(dir: &std::path::Path) -> std::path::PathBuf {
        dir.join(".git/rinkaku-cache/deps-index-v1.json")
    }

    // `head: Some("HEAD")` throughout, matching the comment on the
    // sibling `build_resolver` tests above: it routes reads through the
    // cwd-aware `git cat-file --batch` path, unlike `None`'s
    // process-cwd-relative working-tree reads.
    #[test]
    fn should_produce_identical_resolution_and_write_a_cache_file_on_a_second_run() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn helper() -> i32 {\n    1\n}\n");
        let cli = cli_with_deps_cache(false);
        let spinner = Spinner::start("test");

        let first = build_resolver(
            &cli,
            DIFF_REFERENCING_HELPER,
            read_changed_main_rs,
            Some("HEAD"),
            Some(dir.path()),
            &spinner,
        )
        .expect("first run must succeed")
        .expect("a non-empty reference set must build a resolver");
        assert!(
            cache_file(dir.path()).exists(),
            "expected the cache file to exist after the first run"
        );

        let second = build_resolver(
            &cli,
            DIFF_REFERENCING_HELPER,
            read_changed_main_rs,
            Some("HEAD"),
            Some(dir.path()),
            &spinner,
        )
        .expect("second run must succeed")
        .expect("a non-empty reference set must build a resolver");

        assert_eq!(first.resolve("helper"), second.resolve("helper"));
        assert_eq!(
            vec![rinkaku_core::deps::ResolvedSymbol {
                signature: "fn helper() -> i32".to_string(),
                path: "src/lib.rs".to_string(),
                container: None,
            }],
            second.resolve("helper")
        );
    }

    // Regression for the cache's whole reason to exist: a cache hit must
    // not still be serving a *stale* signature after the underlying blob
    // changed — staleness is keyed on git blob OID (`DepsCache::get`), so
    // a new commit changing `helper`'s signature must produce a new OID
    // and therefore a miss that re-parses it.
    #[test]
    fn should_reflect_a_changed_signature_after_the_definition_is_edited_and_recommitted() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn helper() -> i32 {\n    1\n}\n");
        let cli = cli_with_deps_cache(false);
        let spinner = Spinner::start("test");

        build_resolver(
            &cli,
            DIFF_REFERENCING_HELPER,
            read_changed_main_rs,
            Some("HEAD"),
            Some(dir.path()),
            &spinner,
        )
        .expect("first run must succeed")
        .expect("a non-empty reference set must build a resolver");

        std::fs::write(
            dir.path().join("src/lib.rs"),
            "fn helper(x: i32) -> i32 {\n    x\n}\n",
        )
        .expect("edit src/lib.rs");
        run_git(dir.path(), &["add", "src/lib.rs"]);
        run_git(dir.path(), &["commit", "-m", "widen helper's signature"]);

        let after_edit = build_resolver(
            &cli,
            DIFF_REFERENCING_HELPER,
            read_changed_main_rs,
            Some("HEAD"),
            Some(dir.path()),
            &spinner,
        )
        .expect("second run must succeed")
        .expect("a non-empty reference set must build a resolver");

        assert_eq!(
            vec![rinkaku_core::deps::ResolvedSymbol {
                signature: "fn helper(x: i32) -> i32".to_string(),
                path: "src/lib.rs".to_string(),
                container: None,
            }],
            after_edit.resolve("helper")
        );
    }

    // A corrupted cache file (matching `DepsCache::load`'s own
    // "degrade to empty, never fail the run" contract at this
    // higher integration level) must not fail `build_resolver` — it
    // must rebuild the index from scratch as if the cache were empty.
    #[test]
    fn should_succeed_and_rebuild_the_index_when_the_cache_file_is_corrupted() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn helper() -> i32 {\n    1\n}\n");
        let cache_path = cache_file(dir.path());
        std::fs::create_dir_all(cache_path.parent().expect("cache file has a parent"))
            .expect("create cache dir");
        std::fs::write(&cache_path, "not valid json{{{").expect("write corrupted cache file");
        let cli = cli_with_deps_cache(false);
        let spinner = Spinner::start("test");

        let resolver = build_resolver(
            &cli,
            DIFF_REFERENCING_HELPER,
            read_changed_main_rs,
            Some("HEAD"),
            Some(dir.path()),
            &spinner,
        )
        .expect("a corrupted cache file must not fail the run")
        .expect("a non-empty reference set must build a resolver");

        assert_eq!(
            vec![rinkaku_core::deps::ResolvedSymbol {
                signature: "fn helper() -> i32".to_string(),
                path: "src/lib.rs".to_string(),
                container: None,
            }],
            resolver.resolve("helper")
        );
    }

    #[test]
    fn should_not_create_a_cache_file_when_no_deps_cache_is_set() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn helper() -> i32 {\n    1\n}\n");
        let cli = cli_with_deps_cache(true);
        let spinner = Spinner::start("test");

        build_resolver(
            &cli,
            DIFF_REFERENCING_HELPER,
            read_changed_main_rs,
            Some("HEAD"),
            Some(dir.path()),
            &spinner,
        )
        .expect("run must succeed")
        .expect("a non-empty reference set must build a resolver");

        assert!(
            !cache_file(dir.path()).exists(),
            "expected no cache file to be written when --no-deps-cache is set"
        );
    }
}

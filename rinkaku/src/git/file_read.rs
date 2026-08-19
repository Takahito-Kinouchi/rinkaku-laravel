//! Per-file readers used by `pipeline`: read from the working tree
//! (stdin mode) or from `git show <head>:<path>` (`--base`/`--pr` modes,
//! for parity with the commit the diff was generated against).

/// Reads `path`'s content from `prefetched` if present, falling back to
/// `fallback` otherwise.
///
/// `pipeline::run_base_pipeline` batch-prefetches every changed path's
/// content via a single `git cat-file --batch` child per side
/// (`read_git_show_files_batch`) and serves the `read_file`/`read_base_file`
/// ports it hands to `analyze_diff`/`build_resolver` from that map, calling
/// this with `fallback` set to the equivalent per-file `read_git_show_file`
/// call. A path missing from the map — added/deleted on that side, or a
/// rename/copy whose base-side path differs from the new-side path the
/// prefetch was keyed on (`classify_against_base`'s `read_path`/
/// `report_path` split) — is served by `fallback`, so its result is
/// identical to what an unbatched per-file `git show` would have produced.
pub(crate) fn read_prefetched_or_fallback(
    prefetched: &std::collections::HashMap<String, String>,
    path: &str,
    fallback: impl FnOnce(&str) -> std::io::Result<String>,
) -> std::io::Result<String> {
    match prefetched.get(path) {
        Some(content) => Ok(content.clone()),
        None => fallback(path),
    }
}

/// Reads a changed file's new-side content off the working tree.
pub(crate) fn read_working_tree_file(path: &str) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

/// [`rinkaku_tui::source::SourceReader`] backed by `git show <head>:<path>`
/// (ADR 0047) — `main.rs` wires this into the TUI's source view for `--pr`
/// mode, in place of [`rinkaku_tui::source::WorkingTreeSourceReader`]'s
/// working-tree read, since `--pr` mode never checks the PR's head ref out
/// (this module's own doc comment, and `main.rs`'s module doc comment on
/// its `--pr` read strategy) — the working tree can be anything.
///
/// `head` is the resolved PR head SHA and `cwd` is the resolved `--pr`
/// workdir, exactly the values `main.rs` already has in hand from
/// `fetch_pr_head`/`resolve_pr_workdir` by the time the TUI starts.
pub(crate) struct PrHeadSourceReader {
    pub(crate) head: String,
    pub(crate) cwd: Option<std::path::PathBuf>,
}

impl rinkaku_tui::source::SourceReader for PrHeadSourceReader {
    fn read(&self, _repo_root: &std::path::Path, relative_path: &str) -> Result<String, String> {
        read_git_show_file(self.cwd.as_deref(), &self.head, relative_path)
            .map_err(|source| format!("failed to read {relative_path} at {}: {source}", self.head))
    }
}

/// Reads a changed file's content as committed at `head`, via
/// `git show <head>:<path>`. Used in `--base` mode so the content read
/// always matches the commit the diff was generated against, independent
/// of the working tree's current state.
///
/// `cwd` selects the repository to run `git` in; `None` uses the process's
/// current directory (production callers), `Some(dir)` pins it to a
/// specific directory (tests, so they don't depend on or mutate the
/// process-wide current directory).
pub(crate) fn read_git_show_file(
    cwd: Option<&std::path::Path>,
    head: &str,
    path: &str,
) -> std::io::Result<String> {
    let object = format!("{head}:{path}");
    let mut command = std::process::Command::new("git");
    command.args(["show", &object]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "git show {object} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    String::from_utf8(output.stdout)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::init_repo_with_committed_file;
    use pretty_assertions::assert_eq;

    mod read_prefetched_or_fallback_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use std::collections::HashMap;

        #[test]
        fn should_return_prefetched_content_without_calling_fallback_when_path_is_present() {
            let prefetched: HashMap<String, String> =
                [("a.rs".to_string(), "fn a() {}\n".to_string())].into();

            let actual = read_prefetched_or_fallback(&prefetched, "a.rs", |_| {
                panic!("fallback must not be called when the path is already prefetched")
            })
            .expect("a prefetched path must not error");

            assert_eq!("fn a() {}\n".to_string(), actual);
        }

        #[test]
        fn should_call_fallback_when_path_is_absent_from_prefetch() {
            let prefetched: HashMap<String, String> =
                [("a.rs".to_string(), "fn a() {}\n".to_string())].into();

            let actual = read_prefetched_or_fallback(&prefetched, "b.rs", |path| {
                Ok(format!("fallback content for {path}"))
            })
            .expect("the fallback's Ok result must be returned unchanged");

            assert_eq!("fallback content for b.rs".to_string(), actual);
        }

        #[test]
        fn should_propagate_fallback_error_when_path_is_absent_from_prefetch() {
            let prefetched: HashMap<String, String> = HashMap::new();

            let actual = read_prefetched_or_fallback(&prefetched, "missing.rs", |_| {
                Err(std::io::Error::other("git show missing.rs failed"))
            });

            let error = actual.expect_err("a fallback failure must propagate as an error");
            assert_eq!("git show missing.rs failed", error.to_string());
        }
    }

    // Integration test for the must-fix design: `--base` mode must read
    // file content via `git show <head>:<path>`, not off the working tree.
    // A dirty working tree (uncommitted edit) must not affect what gets
    // read — only the committed content at `head` should come back.
    #[test]
    fn should_read_committed_content_when_working_tree_is_dirty() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let committed = "fn foo(a: i32) -> i32 {\n    a\n}\n";
        init_repo_with_committed_file(dir.path(), committed);

        // Dirty the working tree after the commit: if `read_git_show_file`
        // fell back to the working tree, it would read this instead.
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "fn foo(a: i32) -> i32 {\n    a + 999\n}\n",
        )
        .expect("dirty the working tree");

        let actual = read_git_show_file(Some(dir.path()), "HEAD", "src/lib.rs")
            .expect("git show should succeed for a committed file");

        assert_eq!(committed, actual);
    }

    mod pr_head_source_reader_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rinkaku_tui::source::SourceReader;

        // Integration test for ADR 0047: the source view's `--pr` reader
        // must read the resolved head commit, not the working tree — a
        // working-tree edit made after the head was fetched (analogous to
        // `--pr` never checking the fetched ref out) must not affect what
        // this reader returns.
        #[test]
        fn should_read_head_commit_content_when_working_tree_is_dirty() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            let committed = "fn foo(a: i32) -> i32 {\n    a\n}\n";
            init_repo_with_committed_file(dir.path(), committed);
            let head = String::from_utf8(
                std::process::Command::new("git")
                    .args(["rev-parse", "HEAD"])
                    .current_dir(dir.path())
                    .output()
                    .expect("run git rev-parse")
                    .stdout,
            )
            .expect("git rev-parse output is UTF-8")
            .trim()
            .to_string();

            std::fs::write(
                dir.path().join("src/lib.rs"),
                "fn foo(a: i32) -> i32 {\n    a + 999\n}\n",
            )
            .expect("dirty the working tree");

            let reader = PrHeadSourceReader {
                head,
                cwd: Some(dir.path().to_path_buf()),
            };

            let actual = reader.read(std::path::Path::new("/unused"), "src/lib.rs");

            assert_eq!(Ok(committed.to_string()), actual);
        }

        #[test]
        fn should_return_error_message_when_head_commit_has_no_such_path() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            init_repo_with_committed_file(dir.path(), "fn foo() {}\n");

            let reader = PrHeadSourceReader {
                head: "HEAD".to_string(),
                cwd: Some(dir.path().to_path_buf()),
            };

            let actual = reader.read(std::path::Path::new("/unused"), "src/missing.rs");

            let error = actual.expect_err("a missing path must fail rather than silently succeed");
            assert!(
                error.contains("src/missing.rs"),
                "error message should name the missing path, got: {error}"
            );
        }
    }
}

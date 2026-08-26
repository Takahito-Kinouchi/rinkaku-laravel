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

/// Reads a changed file's new-side content off the working tree, rooted
/// at the process's current directory — which is what a bare relative
/// `std::fs` read resolves against anyway, so this reads exactly the file
/// it always did.
///
/// See [`read_contained_file`] for the containment the read is subject to.
pub(crate) fn read_working_tree_file(path: &str) -> std::io::Result<String> {
    read_contained_file(&std::env::current_dir()?, path)
}

/// Reads `path` (repository-root-relative) from under `root`, refusing
/// any path that does not stay inside it (ADR 0090).
///
/// Two checks, because a path can leave the repository two ways:
///
/// 1. **Lexically** — `../…`, `/etc/…`, `C:\…`. `is_repo_relative`
///    settles this without touching the filesystem.
///    `rinkaku_core::pipeline::analyze_diff` already skips such an entry
///    before it ever calls this port, so this is the second of the two
///    checks rather than the only one: this function is the process's
///    actual `std::fs` boundary for diff-supplied paths, and a boundary
///    that only holds because of a check somewhere else is a boundary one
///    refactor away from not holding at all.
/// 2. **Through a symlink** — `stolen.py -> /home/victim/.ssh/id_rsa` is
///    lexically spotless, and a diff naming it is enough to make an
///    unguarded read follow it out of the tree (ADR 0090's amendment).
///    Only a *resolved* path can rule this out, so the read target is
///    canonicalized and compared against the canonicalized root.
///
/// Both refusals report `ErrorKind::InvalidInput`, which is what
/// `analyze_diff` reads as "this entry is outside the repository" and
/// turns into a skip rather than a failed run.
///
/// The root is canonicalized too, not assumed canonical: `git rev-parse
/// --show-toplevel` and `std::env::current_dir` can both hand back a path
/// that reaches the repository through a symlink (macOS's `/tmp` ->
/// `/private/tmp` is the everyday case), and comparing a resolved path
/// against an unresolved root would reject every legitimate read there.
fn read_contained_file(root: &std::path::Path, path: &str) -> std::io::Result<String> {
    if !rinkaku_core::repo_path::is_repo_relative(path) {
        return Err(outside_repository(path, "path is outside the repository"));
    }
    // Canonicalizing the target before the root keeps a missing file
    // reported as the filesystem's own `NotFound`, exactly as the plain
    // read this replaced did, rather than as a containment refusal.
    let resolved = root.join(path).canonicalize()?;
    let root = root.canonicalize()?;
    if !rinkaku_core::repo_path::is_inside_root(&root, &resolved) {
        return Err(outside_repository(
            path,
            "it resolves outside the repository",
        ));
    }
    std::fs::read_to_string(resolved)
}

fn outside_repository(path: &str, reason: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("refusing to read {path}: {reason}"),
    )
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
    // ADR 0092: `head` is a revision from `--base`/`--pr` and `path` comes
    // from the diff, so the joined object name is never trusted to not
    // start with `-`.
    command.args(["show", "--end-of-options", &object]);
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

    // ADR 0090: this port is the process's `std::fs` boundary for
    // diff-supplied paths. `analyze_diff` already refuses such an entry
    // upstream, so these pin the second check independently of the first.
    mod read_working_tree_file_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        #[rstest]
        #[case::parent_escape("../../etc/passwd")]
        #[case::absolute("/etc/passwd")]
        fn should_refuse_to_read_when_path_escapes_the_repository(#[case] path: &str) {
            let actual = read_working_tree_file(path);

            let error = actual.expect_err("an escaping path must not be read");
            assert_eq!(std::io::ErrorKind::InvalidInput, error.kind());
            assert_eq!(
                format!("refusing to read {path}: path is outside the repository"),
                error.to_string()
            );
        }

        // The complement of the cases above: a repository-relative path
        // reaches the filesystem, so a missing file comes back as the
        // filesystem's own `NotFound` rather than the guard's refusal.
        // Asserted this way round because a positive read would have to
        // put a file at a path relative to the test process's current
        // directory, which no test may mutate.
        #[test]
        fn should_reach_the_filesystem_when_path_is_repository_relative() {
            let actual = read_working_tree_file("src/definitely-not-a-real-file.rs");

            let error = actual.expect_err("the file does not exist");
            assert_eq!(std::io::ErrorKind::NotFound, error.kind());
        }
    }

    // ADR 0090's amendment: the lexical check above cannot see a symlink,
    // so these exercise the resolved-path half against a real tree. They
    // take the root as an argument rather than going through
    // `read_working_tree_file`, whose root is the test process's own
    // current directory — which no test may mutate.
    mod read_contained_file_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        #[rstest]
        #[case::absolute_target("/etc/passwd")]
        #[case::relative_target("../outside/creds.py")]
        #[cfg(unix)]
        fn should_refuse_to_read_when_an_in_tree_symlink_points_out_of_the_repository(
            #[case] link_target: &str,
        ) {
            let dir = tempfile::tempdir().expect("create temp dir");
            let root = dir.path().join("repo");
            std::fs::create_dir_all(&root).expect("create repo dir");
            std::fs::create_dir_all(dir.path().join("outside")).expect("create outside dir");
            std::fs::write(dir.path().join("outside/creds.py"), "SECRET = 1\n")
                .expect("write the out-of-tree file");
            std::os::unix::fs::symlink(link_target, root.join("stolen.py"))
                .expect("create the escaping symlink");

            let actual = read_contained_file(&root, "stolen.py");

            let error = actual.expect_err("an escaping symlink must not be followed");
            assert_eq!(std::io::ErrorKind::InvalidInput, error.kind());
            assert_eq!(
                "refusing to read stolen.py: it resolves outside the repository",
                error.to_string()
            );
        }

        // The complement: a symlink is not refused for being a symlink,
        // only for leaving the repository. Repositories legitimately
        // contain links to their own files.
        #[test]
        #[cfg(unix)]
        fn should_read_through_a_symlink_when_it_stays_inside_the_repository() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let root = dir.path();
            std::fs::create_dir_all(root.join("src")).expect("create src dir");
            std::fs::write(root.join("src/lib.rs"), "fn foo() {}\n").expect("write file");
            std::os::unix::fs::symlink("src/lib.rs", root.join("alias.rs"))
                .expect("create the in-tree symlink");

            let actual = read_contained_file(root, "alias.rs").expect("an in-tree link is read");

            assert_eq!("fn foo() {}\n".to_string(), actual);
        }

        #[test]
        fn should_read_a_plain_file_when_it_is_inside_the_repository() {
            let dir = tempfile::tempdir().expect("create temp dir");
            std::fs::create_dir_all(dir.path().join("src")).expect("create src dir");
            std::fs::write(dir.path().join("src/lib.rs"), "fn foo() {}\n").expect("write file");

            let actual =
                read_contained_file(dir.path(), "src/lib.rs").expect("an in-tree file is read");

            assert_eq!("fn foo() {}\n".to_string(), actual);
        }
    }

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

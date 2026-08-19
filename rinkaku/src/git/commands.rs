//! Local git subprocess wrappers used by the composition root and other
//! modules: `git diff`, `git ls-files`, `git rev-parse --show-toplevel`,
//! `git rev-parse --git-dir`, and `git ls-tree`.

pub(crate) fn run_git_diff(
    base: &str,
    head: &str,
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<String> {
    let range = format!("{base}...{head}");
    let mut command = std::process::Command::new("git");
    command.args(["diff", &range]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git diff {range} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

pub(crate) fn list_git_files(cwd: Option<&std::path::Path>) -> anyhow::Result<Vec<String>> {
    let mut command = std::process::Command::new("git");
    command.args(["ls-files"]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git ls-files failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(str::to_string)
        .collect())
}

/// Lists tracked files for ADR 0017's whole-repo outline, same as
/// `list_git_files(cwd)`, but with guidance attached to a failure via
/// `anyhow::Context`: bare `rinkaku` (this mode's default, the first thing
/// a new user is likely to try) run outside a git repository would
/// otherwise surface only `list_git_files`'s raw `git ls-files` stderr
/// (e.g. "fatal: not a git repository ..."), which does not tell the reader
/// what rinkaku itself expects instead. Kept as its own function (rather
/// than adding this message inside `list_git_files` itself) since that
/// function's error is reused as-is by every other caller (`--base`/`--pr`'s
/// own indexing pass in `build_resolver`) where this specific guidance
/// would not apply.
pub(crate) fn list_repo_files_for_outline(
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<Vec<String>> {
    use anyhow::Context;
    list_git_files(cwd).context(
        "run rinkaku inside a git repository, or pipe a diff (e.g. `gh pr diff 123 | rinkaku`) \
         or pass --base <ref>",
    )
}

pub(crate) fn resolve_repo_root(cwd: Option<&std::path::Path>) -> std::path::PathBuf {
    let mut command = std::process::Command::new("git");
    command.args(["rev-parse", "--show-toplevel"]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let toplevel = command
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| std::path::PathBuf::from(stdout.trim()));

    toplevel.unwrap_or_else(|| match cwd {
        Some(cwd) => cwd.to_path_buf(),
        None => std::env::current_dir().unwrap_or_default(),
    })
}

/// Resolves the repository's git directory (`.git`, or the real directory
/// a linked worktree's `.git` file points at) via `git rev-parse
/// --git-dir`. Used by `deps_cache` (ADR 0079) to place the persistent
/// dependency-index cache alongside git's own per-repository state rather
/// than inside the worktree, where it would need its own `.gitignore`
/// entry and could be accidentally committed.
///
/// `git rev-parse --git-dir` prints a path relative to `cwd` in the
/// common case (`.git`), so the result is joined onto `cwd` to make it
/// usable regardless of the calling process's actual working directory —
/// mirroring why `resolve_repo_root` above does the same for
/// `--show-toplevel`'s output.
pub(crate) fn resolve_git_dir(cwd: Option<&std::path::Path>) -> anyhow::Result<std::path::PathBuf> {
    let mut command = std::process::Command::new("git");
    command.args(["rev-parse", "--git-dir"]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git rev-parse --git-dir failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let printed = std::path::PathBuf::from(String::from_utf8(output.stdout)?.trim());
    Ok(if printed.is_absolute() {
        printed
    } else {
        match cwd {
            Some(cwd) => cwd.join(printed),
            None => printed,
        }
    })
}

/// Lists every blob's git object ID at `rev` via a single `git ls-tree -r
/// <rev> -z --format=%(objectname)%x09%(path)` subprocess — one process
/// for the whole tree, not one per candidate path, matching the batching
/// pattern `git cat-file --batch` uses in `cat_file_batch.rs`. Used by
/// `deps_cache` (ADR 0079) to detect, before reading or parsing a single
/// byte of content, which candidate paths' blobs actually changed since
/// the last cached run.
///
/// `-z` NUL-terminates each entry instead of newline-terminating it, so a
/// path containing a literal newline round-trips correctly (rare, but not
/// something a plain line-oriented parse could rule out).
pub(crate) fn list_blob_oids(
    cwd: Option<&std::path::Path>,
    rev: &str,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    let mut command = std::process::Command::new("git");
    command.args([
        "ls-tree",
        "-r",
        rev,
        "-z",
        "--format=%(objectname)%x09%(path)",
    ]);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!(
            "git ls-tree -r {rev} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8(output.stdout)?;
    Ok(stdout
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| entry.split_once('\t'))
        .map(|(oid, path)| (path.to_string(), oid.to_string()))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::init_repo_with_committed_file;
    use pretty_assertions::assert_eq;
    // Regression test for the TUI source view failing whenever `rinkaku`
    // is launched from a subdirectory of the repository (the bug this
    // function exists to fix): `git rev-parse --show-toplevel` run from
    // `src/` must still resolve to the repository root, not `src/` itself
    // — `resolve_repo_root`'s own doc comment explains why `Report` paths
    // need the *root*, not the process's actual current directory, to
    // join against.
    #[test]
    fn should_resolve_repository_root_when_cwd_is_a_subdirectory() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        init_repo_with_committed_file(dir.path(), "fn foo() {}\n");
        let subdir = dir.path().join("src");

        let actual = resolve_repo_root(Some(&subdir));

        // Compare canonicalized paths on both sides: `git rev-parse
        // --show-toplevel`'s output and `tempfile::TempDir::path()` can
        // differ by a symlink resolution (e.g. macOS's `/tmp` ->
        // `/private/tmp`), which is not the thing this test is checking.
        let expected = dir.path().canonicalize().expect("canonicalize expected");
        let actual = actual.canonicalize().expect("canonicalize actual");
        assert_eq!(expected, actual);
    }

    #[test]
    fn should_fall_back_to_cwd_when_directory_is_not_a_git_repository() {
        let dir = tempfile::TempDir::new().expect("create tempdir");

        let actual = resolve_repo_root(Some(dir.path()));

        assert_eq!(dir.path(), actual);
    }

    mod list_repo_files_for_outline_tests {
        use super::*;
        use pretty_assertions::assert_eq;

        // Regression test for the unfriendly-error fix: running whole-repo
        // mode outside a git repository must not surface only
        // `list_git_files`'s raw `git ls-files` stderr — the wrapped
        // message must guide the reader toward what rinkaku actually
        // expects (a git repo, a piped diff, or `--base`).
        #[test]
        fn should_include_guidance_in_error_when_cwd_is_not_a_git_repository() {
            let dir = tempfile::TempDir::new().expect("create tempdir");

            let actual = list_repo_files_for_outline(Some(dir.path()));

            let error = actual.expect_err("a non-git directory must fail");
            let message = format!("{error:#}");
            assert!(
                message.contains("run rinkaku inside a git repository"),
                "error message did not contain the expected guidance: {message}"
            );
        }

        #[test]
        fn should_return_tracked_paths_when_cwd_is_a_git_repository() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            init_repo_with_committed_file(dir.path(), "fn foo() {}\n");

            let actual = list_repo_files_for_outline(Some(dir.path()))
                .expect("a git repository must succeed");

            assert_eq!(vec!["src/lib.rs".to_string()], actual);
        }
    }

    mod resolve_git_dir_tests {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_resolve_git_dir_to_dot_git_when_cwd_is_repository_root() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            init_repo_with_committed_file(dir.path(), "fn foo() {}\n");

            let actual = resolve_git_dir(Some(dir.path())).expect("a git repository must resolve");

            let expected = dir
                .path()
                .join(".git")
                .canonicalize()
                .expect("canonicalize expected");
            let actual = actual.canonicalize().expect("canonicalize actual");
            assert_eq!(expected, actual);
        }

        // Same regression shape as `resolve_repo_root`'s own subdirectory
        // test above: `git rev-parse --git-dir` prints a path relative to
        // `cwd` (`../.git` from a subdirectory), so a caller invoking it
        // from anywhere but the repository root must still get back a
        // usable path pointing at the same `.git` directory.
        #[test]
        fn should_resolve_git_dir_when_cwd_is_a_subdirectory() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            init_repo_with_committed_file(dir.path(), "fn foo() {}\n");
            let subdir = dir.path().join("src");

            let actual = resolve_git_dir(Some(&subdir)).expect("a git repository must resolve");

            let expected = dir
                .path()
                .join(".git")
                .canonicalize()
                .expect("canonicalize expected");
            let actual = actual.canonicalize().expect("canonicalize actual");
            assert_eq!(expected, actual);
        }

        #[test]
        fn should_fail_when_cwd_is_not_a_git_repository() {
            let dir = tempfile::TempDir::new().expect("create tempdir");

            let actual = resolve_git_dir(Some(dir.path()));

            assert!(actual.is_err());
        }
    }

    mod list_blob_oids_tests {
        use super::*;
        use pretty_assertions::assert_eq;

        #[test]
        fn should_list_oid_for_every_tracked_blob_at_head() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            init_repo_with_committed_file(dir.path(), "fn foo() {}\n");

            let actual =
                list_blob_oids(Some(dir.path()), "HEAD").expect("a git repository must resolve");

            assert_eq!(1, actual.len());
            let oid = actual
                .get("src/lib.rs")
                .expect("src/lib.rs must be present in the tree");
            assert_eq!(40, oid.len(), "expected a full 40-character SHA-1 OID");
        }

        // Regression guard for the staleness detection this function
        // exists for (ADR 0079): a file's OID must change when its
        // committed content changes, so a cache keyed on it can tell the
        // two revisions apart.
        #[test]
        fn should_return_different_oid_when_file_content_changes_across_commits() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            init_repo_with_committed_file(dir.path(), "fn foo() {}\n");
            let before = list_blob_oids(Some(dir.path()), "HEAD")
                .expect("first commit must resolve")
                .get("src/lib.rs")
                .cloned()
                .expect("src/lib.rs must be present");

            std::fs::write(dir.path().join("src/lib.rs"), "fn bar() {}\n")
                .expect("edit src/lib.rs");
            crate::test_util::run_git(dir.path(), &["add", "src/lib.rs"]);
            crate::test_util::run_git(dir.path(), &["commit", "-m", "change foo to bar"]);

            let after = list_blob_oids(Some(dir.path()), "HEAD")
                .expect("second commit must resolve")
                .get("src/lib.rs")
                .cloned()
                .expect("src/lib.rs must be present");

            assert_ne!(before, after);
        }

        #[test]
        fn should_fail_when_rev_does_not_exist() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            init_repo_with_committed_file(dir.path(), "fn foo() {}\n");

            let actual = list_blob_oids(Some(dir.path()), "does-not-exist");

            assert!(actual.is_err());
        }
    }
}

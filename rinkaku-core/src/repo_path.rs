//! Containment check for the paths a unified diff carries (ADR 0090).
//!
//! Every path in a `Report` is meant to be repository-root-relative, and
//! the rest of the pipeline reads and displays files by joining those
//! paths onto a root. Nothing upstream of here enforces that, though:
//! `diff::parse_unified_diff` takes the path out of a `diff --git a/… b/…`
//! header verbatim, and in stdin mode that header is attacker-controlled
//! input (`gh pr diff <someone else's PR> | rinkaku`). This module is the
//! predicate that turns "meant to be" into a checked property.
//!
//! [`is_repo_relative`] answers that question about the *string*.
//! [`is_inside_root`] answers the complementary question about the
//! *filesystem*: a lexically-clean path can still be a symlink pointing
//! out of the tree, and only a resolved path can rule that out. The
//! resolution itself needs IO, so it stays on the adapter side (ADR 0090's
//! amendment); what lives here is the comparison the adapters make once
//! they have both paths in hand.

/// Whether `path` is a plain repository-root-relative path — no absolute
/// root, no parent-directory escape, no empty string.
///
/// Rejects, in the order a reader is likely to think of them:
///
/// - the empty path, and a path of only whitespace (`diff --git` headers
///   with nothing where a path belongs; also what
///   [`crate::diff::parse_unified_diff`] produces for a header it could
///   not read at all);
/// - a POSIX absolute path (`/etc/passwd`) — including the `//etc/passwd`
///   form a `diff --git a//etc/passwd b//etc/passwd` header yields once
///   its `b/` prefix is stripped;
/// - a Windows absolute path (`C:\…`, `\\server\share`), which
///   [`std::path::Path::is_absolute`] does *not* catch when rinkaku runs
///   on Unix — a diff can be authored anywhere, so the check cannot be
///   left to the host platform;
/// - any `..` component, under either separator.
///
/// A leading `./` and interior `.` components are accepted: they are
/// noise rather than an escape, and normalizing them away is not this
/// predicate's job.
pub fn is_repo_relative(path: &str) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    if has_windows_drive_prefix(path) {
        return false;
    }
    !path.split(['/', '\\']).any(|component| component == "..")
}

/// Whether `resolved` stays inside `root`.
///
/// Both arguments must already be canonicalized by the caller — this
/// function does no IO, so it cannot resolve `..`, `.`, or a symlink
/// itself, and comparing un-resolved paths would answer a different
/// question than the one the caller is asking. Canonicalizing is the
/// adapter's job (ADR 0090's amendment: `rinkaku-core`'s domain logic
/// reads nothing off disk); this is the pure half of that check, kept
/// next to [`is_repo_relative`] so both statements of "inside the
/// repository" live in one module.
///
/// Comparison is by path component ([`std::path::Path::starts_with`]),
/// not by string prefix: a sibling directory whose name merely begins
/// with the root's (`/repo-evil` against `/repo`) is outside, and a
/// string prefix test would call it inside.
pub fn is_inside_root(root: &std::path::Path, resolved: &std::path::Path) -> bool {
    resolved.starts_with(root)
}

/// Whether `path` starts with a Windows drive letter (`C:`, `c:/…`).
/// Checked by hand rather than via [`std::path::Path`], whose parsing of
/// this form only happens on Windows targets.
fn has_windows_drive_prefix(path: &str) -> bool {
    let mut chars = path.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && chars.next() == Some(':')
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    mod is_inside_root_tests {
        use super::*;
        use pretty_assertions::assert_eq;
        use rstest::rstest;
        use std::path::Path;

        #[rstest]
        #[case::should_accept_a_file_directly_under_the_root("/repo", "/repo/lib.rs")]
        #[case::should_accept_a_nested_file("/repo", "/repo/src/deep/lib.rs")]
        #[case::should_accept_the_root_itself("/repo", "/repo")]
        fn should_report_inside_when_resolved_stays_under_the_root(
            #[case] root: &str,
            #[case] resolved: &str,
        ) {
            let actual = is_inside_root(Path::new(root), Path::new(resolved));

            assert_eq!(true, actual);
        }

        #[rstest]
        #[case::should_reject_an_unrelated_absolute_path("/repo", "/etc/passwd")]
        #[case::should_reject_a_parent_of_the_root("/repo/src", "/repo")]
        // A string prefix test would call this one inside; component-wise
        // comparison is what makes it outside.
        #[case::should_reject_a_sibling_whose_name_starts_with_the_root("/repo", "/repo-evil/x.rs")]
        fn should_report_outside_when_resolved_leaves_the_root(
            #[case] root: &str,
            #[case] resolved: &str,
        ) {
            let actual = is_inside_root(Path::new(root), Path::new(resolved));

            assert_eq!(false, actual);
        }
    }

    #[rstest]
    #[case::should_accept_a_plain_relative_path("src/lib.rs")]
    #[case::should_accept_a_bare_file_name("README.md")]
    #[case::should_accept_a_dot_slash_prefix("./src/lib.rs")]
    #[case::should_accept_an_interior_dot_component("src/./lib.rs")]
    #[case::should_accept_a_name_merely_starting_with_two_dots("src/..hidden.rs")]
    #[case::should_accept_a_name_merely_ending_with_two_dots("src/hidden...rs")]
    fn should_accept_path_when_it_stays_inside_the_repository(#[case] path: &str) {
        let actual = is_repo_relative(path);

        assert_eq!(true, actual);
    }

    #[rstest]
    #[case::should_reject_empty("")]
    #[case::should_reject_whitespace_only("   ")]
    #[case::should_reject_posix_absolute("/etc/passwd")]
    #[case::should_reject_double_slash_absolute("//etc/passwd")]
    #[case::should_reject_parent_escape("../secret/creds.py")]
    #[case::should_reject_interior_parent_escape("src/../../secret/creds.py")]
    #[case::should_reject_trailing_parent_component("src/..")]
    #[case::should_reject_backslash_parent_escape("..\\secret\\creds.py")]
    #[case::should_reject_windows_drive_absolute("C:\\Users\\victim\\.ssh\\id_rsa")]
    #[case::should_reject_windows_drive_with_forward_slashes("c:/Users/victim/.ssh/id_rsa")]
    #[case::should_reject_unc_path("\\\\server\\share\\secret")]
    fn should_reject_path_when_it_escapes_the_repository(#[case] path: &str) {
        let actual = is_repo_relative(path);

        assert_eq!(false, actual);
    }
}

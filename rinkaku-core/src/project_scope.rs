//! Project-scoped dependency-scan filtering (ADR 0078).
//!
//! In a monorepo holding several projects (e.g. multiple Laravel
//! applications side by side), a diff almost always touches one project,
//! but the dependency index (`crate::deps::TagsResolver`) is built over
//! *every* tracked file — reading and parsing sibling projects whose
//! definitions the changed symbols cannot meaningfully depend on. This
//! module computes, purely from path lists (no filesystem access, keeping
//! `rinkaku-core` IO-free), the set of project roots the changed files
//! belong to, so the caller can restrict the scan to those subtrees.
//!
//! A "project root" is any directory containing one of
//! [`PROJECT_MANIFEST_FILES`]; a changed file's project is its *nearest*
//! such ancestor. The repository root counts as a project root like any
//! other — a changed file whose nearest manifest sits at the root (or that
//! has no manifest ancestor at all) makes scoping a no-op
//! ([`changed_project_roots`] returns `None`), which reduces to the
//! previous scan-everything behavior for single-project repositories.

use std::collections::BTreeSet;
use std::collections::HashSet;

/// Manifest file names that mark a directory as a project root. Matched
/// by exact basename against the tracked-file list. The set covers the
/// package managers of every language rinkaku supports (plus `Gemfile`,
/// which costs nothing and keeps a Rails-in-a-monorepo neighbor from
/// widening a scan): one manifest kind per ecosystem is enough, since a
/// project defining any of them has it at its root by convention.
pub const PROJECT_MANIFEST_FILES: &[&str] = &[
    "composer.json",
    "package.json",
    "Cargo.toml",
    "go.mod",
    "pyproject.toml",
    "setup.py",
    "Gemfile",
];

/// The project roots (directory prefixes, `/`-separated, no trailing
/// slash) whose subtrees cover every path in `changed_paths`, or `None`
/// when scoping cannot narrow the scan:
///
/// - a changed path's nearest manifest directory is the repository root
///   itself (single-project repository), or
/// - a changed path has no manifest ancestor at all (repository without
///   manifests, or a top-level stray file), or
/// - `changed_paths` is empty (nothing to scope by).
///
/// `None` deliberately means "scan everything" rather than "scan
/// nothing": scoping is a performance narrowing, and any case it cannot
/// classify must fall back to the always-correct full scan.
pub fn changed_project_roots(
    changed_paths: &[String],
    tracked_paths: &[String],
) -> Option<Vec<String>> {
    if changed_paths.is_empty() {
        return None;
    }

    let manifest_dirs: HashSet<&str> = tracked_paths
        .iter()
        .filter(|path| {
            let file_name = path.rsplit('/').next().unwrap_or(path);
            PROJECT_MANIFEST_FILES.contains(&file_name)
        })
        .map(|path| parent_dir(path))
        .collect();

    // BTreeSet for a deterministic (sorted) root list — callers log and
    // test against it, and prefix filtering doesn't care about order.
    let mut roots: BTreeSet<String> = BTreeSet::new();
    for changed in changed_paths {
        match nearest_manifest_dir(changed, &manifest_dirs) {
            Some(dir) if !dir.is_empty() => {
                roots.insert(dir.to_string());
            }
            // Nearest project is the repository root, or no project at
            // all — either way the scan cannot be narrowed.
            _ => return None,
        }
    }
    Some(roots.into_iter().collect())
}

/// Whether `path` lies under any root in `roots` (component-boundary
/// prefix match, so root `apps/a` does not claim `apps/ab/x.php`).
pub fn is_within_project_roots(path: &str, roots: &[String]) -> bool {
    roots.iter().any(|root| {
        path.strip_prefix(root.as_str())
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some()
    })
}

/// The deepest directory in `manifest_dirs` that is an ancestor of
/// `path`'s own directory (walking from the immediate parent upward), or
/// `None` when no ancestor — the repository root included — holds a
/// manifest. The root is represented as `""`, matching [`parent_dir`]'s
/// encoding for top-level files.
fn nearest_manifest_dir<'a>(path: &str, manifest_dirs: &HashSet<&'a str>) -> Option<&'a str> {
    let mut dir = parent_dir(path);
    loop {
        if let Some(found) = manifest_dirs.get(dir) {
            return Some(found);
        }
        if dir.is_empty() {
            return None;
        }
        dir = parent_dir(dir);
    }
}

/// The `/`-separated parent directory of `path`, `""` for a top-level
/// entry. Paths come from `git`, which always uses `/` regardless of host
/// OS (same rationale as `deps::path_dir_components`).
fn parent_dir(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn should_scope_to_the_one_project_containing_every_changed_path() {
        let tracked = paths(&[
            "apps/shop/composer.json",
            "apps/shop/app/Service.php",
            "apps/admin/composer.json",
            "apps/admin/app/Panel.php",
        ]);
        let changed = paths(&["apps/shop/app/Service.php"]);

        let actual = changed_project_roots(&changed, &tracked);

        assert_eq!(Some(vec!["apps/shop".to_string()]), actual);
    }

    #[test]
    fn should_union_roots_when_changes_span_two_projects() {
        let tracked = paths(&[
            "apps/shop/composer.json",
            "apps/admin/composer.json",
            "apps/shop/a.php",
            "apps/admin/b.php",
        ]);
        let changed = paths(&["apps/shop/a.php", "apps/admin/b.php"]);

        let actual = changed_project_roots(&changed, &tracked);

        assert_eq!(
            Some(vec!["apps/admin".to_string(), "apps/shop".to_string()]),
            actual
        );
    }

    #[test]
    fn should_pick_the_nearest_manifest_when_projects_nest() {
        // A Laravel app whose frontend workspace carries its own
        // package.json: a change inside the workspace scopes to the
        // workspace, not the whole app.
        let tracked = paths(&[
            "apps/shop/composer.json",
            "apps/shop/frontend/package.json",
            "apps/shop/frontend/src/App.vue",
        ]);
        let changed = paths(&["apps/shop/frontend/src/App.vue"]);

        let actual = changed_project_roots(&changed, &tracked);

        assert_eq!(Some(vec!["apps/shop/frontend".to_string()]), actual);
    }

    #[rstest]
    #[case::should_not_scope_when_nearest_manifest_is_the_repo_root(
        &["composer.json", "app/Service.php"],
        &["app/Service.php"]
    )]
    #[case::should_not_scope_when_no_manifest_exists_anywhere(
        &["src/lib.rs"],
        &["src/lib.rs"]
    )]
    #[case::should_not_scope_when_a_changed_path_sits_outside_every_project(
        &["apps/shop/composer.json", "tools/script.py"],
        &["tools/script.py"]
    )]
    fn unscopable_cases(#[case] tracked: &[&str], #[case] changed: &[&str]) {
        let actual = changed_project_roots(&paths(changed), &paths(tracked));

        assert_eq!(None, actual);
    }

    #[test]
    fn should_return_none_when_changed_paths_is_empty() {
        let tracked = paths(&["apps/shop/composer.json"]);

        let actual = changed_project_roots(&[], &tracked);

        assert_eq!(None, actual);
    }

    #[rstest]
    #[case::should_match_path_under_root("apps/shop", "apps/shop/app/Service.php", true)]
    #[case::should_not_match_sibling_sharing_a_prefix("apps/shop", "apps/shopfront/x.php", false)]
    #[case::should_not_match_the_root_manifest_dir_itself_as_file("apps/shop", "apps/shop", false)]
    #[case::should_not_match_unrelated_path("apps/shop", "packages/lib/x.php", false)]
    fn is_within_project_roots_cases(
        #[case] root: &str,
        #[case] path: &str,
        #[case] expected: bool,
    ) {
        let roots = vec![root.to_string()];

        let actual = is_within_project_roots(path, &roots);

        assert_eq!(expected, actual);
    }
}

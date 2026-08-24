//! Path-proximity ranking for name-only resolution (ADR 0087).
//!
//! rinkaku resolves references by name alone (ADR 0003), so a name that
//! several definitions share — `OrderController`, `StoreOrderRequest`, and
//! `UserService` are conventional enough to appear once per application in
//! a Laravel monorepo — yields several equally-valid-looking candidates
//! with no type information to choose between them. Both consumers of that
//! ambiguity rank candidates by how close they sit to the referencing file
//! in the repository tree: [`crate::deps`] sorts a "Depends on" list by it
//! before capping, and [`crate::graph`] keeps only the closest candidates
//! when building edges.
//!
//! Extracted from `deps.rs` once `graph.rs` became that second consumer —
//! the concrete-second-use-case bar CLAUDE.md sets for a shared
//! abstraction, rather than a speculative one.

/// Ranks how close `candidate_path` is to `referencing_path`, lower being
/// closer. Used to keep the most locally relevant matches when a
/// name-only resolver (ADR 0003) returns several same-named candidates,
/// since v1 has no type information to pick the syntactically "correct"
/// one — proximity in the repository's directory tree is used as a proxy
/// for "more likely to be the intended target", the same heuristic an
/// editor's "go to definition" fallback (or a human skimming candidates)
/// would reach for first.
///
/// Ranks, from closest to farthest:
/// 1. Same file as the referencing symbol.
/// 2. Same directory (immediate parent) as the referencing symbol.
/// 3. Shares a path prefix with the referencing symbol — ranked by *shared
///    prefix depth*, deeper (more path components in common) first, so a
///    common grandparent directory ranks closer than a common
///    great-grandparent.
/// 4. No shared directory prefix at all (other than the repository root).
///
/// Edge case: two files that both live directly at the repository root
/// (e.g. `"a.rs"` and `"b.rs"`, no `/` in the path) both have an empty
/// `path_dir_components` result and therefore rank as "same directory"
/// (rank 2), not "no shared prefix" (rank 4) — there is no directory
/// component to distinguish them by. This is a natural consequence of
/// treating the repository root as a directory like any other, not a
/// special case handled separately.
pub(crate) fn path_proximity_rank(
    referencing_path: &str,
    candidate_path: &str,
) -> (u8, std::cmp::Reverse<usize>) {
    if candidate_path == referencing_path {
        return (0, std::cmp::Reverse(usize::MAX));
    }

    let referencing_dir: Vec<&str> = path_dir_components(referencing_path);
    let candidate_dir: Vec<&str> = path_dir_components(candidate_path);

    if referencing_dir == candidate_dir {
        return (1, std::cmp::Reverse(usize::MAX));
    }

    let shared_depth = referencing_dir
        .iter()
        .zip(candidate_dir.iter())
        .take_while(|(a, b)| a == b)
        .count();

    if shared_depth > 0 {
        (2, std::cmp::Reverse(shared_depth))
    } else {
        (3, std::cmp::Reverse(0))
    }
}

/// Splits a `/`-separated repository-relative path into its directory
/// components, dropping the file name itself — e.g. `"src/pkg/a.rs"` →
/// `["src", "pkg"]`. Paths are always `/`-separated regardless of host OS:
/// they come from `git`, which normalizes separators, not from
/// `std::path` traversal of the local filesystem.
fn path_dir_components(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts
}

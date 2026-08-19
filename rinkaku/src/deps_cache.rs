//! Persistent, per-repository on-disk cache for `TagsResolver`'s
//! repository-wide dependency index (ADR 0079).
//!
//! `pipeline::build_resolver` used to rebuild the index from scratch on
//! every `--base`/`--pr` run: read every candidate file's content at
//! `head` (`git cat-file --batch`), then parse every one of them
//! (`extract_all_symbols`). On an unchanged repository — the common case
//! for repeated local runs, or a CI job re-running close together on the
//! same commit — that work is entirely wasted. This module lets
//! `build_resolver` skip both the read and the parse for a candidate
//! path whose git blob OID matches what was cached from a previous run:
//! `resolve_indexed_files` is the single entry point it calls.
//!
//! On-disk shape: a single JSON file at
//! `<git-dir>/rinkaku-cache/deps-index-v1.json` (`<git-dir>` from
//! `crate::git::commands::resolve_git_dir`, so the cache lives beside
//! git's own per-repository state rather than inside the worktree — no
//! `.gitignore` entry needed, and it can never be accidentally
//! committed). The cache is a pure performance optimization with no
//! correctness dependency: `DepsCache::load` degrades to an empty cache
//! on any failure (missing file, malformed JSON, a `format_version` or
//! `rinkaku_version` mismatch) rather than erroring, and
//! `resolve_indexed_files` saves it back best-effort — a save failure is
//! logged at `debug` and swallowed, never surfacing to the user.
//!
//! Known limitation: a file whose content matched a generated-file
//! marker (`rinkaku_core::deps::is_generated_content`) has its parse
//! skipped under the default `--include-generated=false`, the same way
//! `TagsResolver::new` skips it — so its cached entry holds
//! `is_generated: true` with an empty `entries`. Flipping
//! `--include-generated` on afterward reuses that empty cache entry
//! rather than retroactively parsing the file; only a subsequent blob OID
//! change (or `--no-deps-cache`) re-parses it with entries populated.

use rinkaku_core::deps::IndexEntry;
use rinkaku_core::language::LanguageSupport;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Cache file format version — bumped whenever `CachedFile`/`DepsCache`'s
/// shape changes incompatibly. Checked independently of
/// `CARGO_PKG_VERSION` (see `DepsCache::load`) since a shape change could
/// in principle ship without a version bump being the only signal.
const FORMAT_VERSION: u32 = 1;

const CACHE_DIR_NAME: &str = "rinkaku-cache";
const CACHE_FILE_NAME: &str = "deps-index-v1.json";

/// One cached file's dependency-index entries, keyed by path in
/// `DepsCache::files`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CachedFile {
    /// The git blob OID `entries` were extracted from. A cache hit
    /// requires an exact match against the candidate path's *current*
    /// OID (`crate::git::commands::list_blob_oids`'s output) — any
    /// content change gets a new OID, so this alone is enough to detect
    /// staleness without re-reading or hashing content.
    oid: String,
    /// Whether this file's content carried a linguist-compatible
    /// generated-file marker (`rinkaku_core::deps::is_generated_content`)
    /// the last time it was read. `.gitattributes`-based generated
    /// detection is deliberately *not* cached here — it is a
    /// path/pattern rule, re-checked fresh every run via the caller's
    /// `generated_paths` argument, rather than a property of the blob
    /// itself.
    is_generated: bool,
    entries: Vec<IndexEntry>,
}

/// On-disk shape of the persistent dependency-index cache. See this
/// module's doc comment for the file location and failure-handling
/// contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DepsCache {
    format_version: u32,
    rinkaku_version: String,
    files: HashMap<String, CachedFile>,
}

impl DepsCache {
    fn empty() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            rinkaku_version: env!("CARGO_PKG_VERSION").to_string(),
            files: HashMap::new(),
        }
    }

    /// Loads the cache from `<git_dir>/rinkaku-cache/deps-index-v1.json`.
    /// Never fails: any IO error, JSON parse error, or a
    /// `format_version`/`rinkaku_version` mismatch (an older/newer
    /// rinkaku build this version cannot assume `IndexEntry`/extraction
    /// compatibility with) degrades to `Self::empty()`, which behaves
    /// exactly like a first run against this repository.
    fn load(git_dir: &std::path::Path) -> Self {
        std::fs::read_to_string(cache_file_path(git_dir))
            .ok()
            .and_then(|content| serde_json::from_str::<Self>(&content).ok())
            .filter(|cache| {
                cache.format_version == FORMAT_VERSION
                    && cache.rinkaku_version == env!("CARGO_PKG_VERSION")
            })
            .unwrap_or_else(Self::empty)
    }

    /// Looks up `path`'s cached entry, only if its cached OID matches
    /// `oid` — a mismatch (or no entry at all) means the caller must
    /// treat `path` as a cache miss.
    fn get(&self, path: &str, oid: &str) -> Option<&CachedFile> {
        self.files.get(path).filter(|cached| cached.oid == oid)
    }

    /// Records `path`'s freshly (re-)computed entry, overwriting whatever
    /// was cached for it before.
    fn insert(&mut self, path: String, oid: String, is_generated: bool, entries: Vec<IndexEntry>) {
        self.files.insert(
            path,
            CachedFile {
                oid,
                is_generated,
                entries,
            },
        );
    }

    /// Persists the cache to `<git_dir>/rinkaku-cache/deps-index-v1.json`
    /// via a write to a temporary file in the same directory followed by
    /// a rename, so a concurrent reader never observes a partially
    /// written file. `candidate_paths` prunes every cached path no
    /// longer among this run's candidates first, so a file that moved
    /// out of scope (deleted, renamed, or scoped out by `--deps-scope`)
    /// does not linger in the cache forever.
    fn save(
        &mut self,
        git_dir: &std::path::Path,
        candidate_paths: &HashSet<String>,
    ) -> anyhow::Result<()> {
        self.files.retain(|path, _| candidate_paths.contains(path));
        let dir = cache_dir(git_dir);
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_vec(self)?;
        // A unique per-process suffix, not a fixed name: two concurrent
        // rinkaku invocations against the same repository (e.g. two CI
        // jobs) writing to the same fixed temp path could otherwise clobber
        // each other's write before either gets to rename.
        let tmp_path = dir.join(format!("{CACHE_FILE_NAME}.tmp.{}", std::process::id()));
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, cache_file_path(git_dir))?;
        Ok(())
    }
}

fn cache_dir(git_dir: &std::path::Path) -> std::path::PathBuf {
    git_dir.join(CACHE_DIR_NAME)
}

fn cache_file_path(git_dir: &std::path::Path) -> std::path::PathBuf {
    cache_dir(git_dir).join(CACHE_FILE_NAME)
}

/// Whether `is_generated` (either signal — content marker or
/// `.gitattributes`) means `path` should contribute no entries to this
/// run's index, mirroring `TagsResolver::new`'s own
/// `!include_generated && (generated_paths.contains(path) ||
/// is_generated_content(content))` gate.
fn should_exclude_as_generated(
    path: &str,
    content_marker: bool,
    generated_paths: &HashSet<String>,
    include_generated: bool,
) -> bool {
    !include_generated && (content_marker || generated_paths.contains(path))
}

/// Resolves `paths`' dependency-index entries at `head`, in `paths`'
/// original order, using the persistent cache (this module) to skip
/// reading and parsing any path whose git blob OID hasn't changed since
/// the last cached run against this repository.
///
/// Caller contract, mirroring the pre-cache code this replaces in
/// `pipeline::build_resolver`'s `head: Some(_)` branch:
/// - `paths` must already be filtered to candidates the index could ever
///   use (registered language, `--deps-scope`, `--exclude-tests`'s
///   path-based exclusion) — this function does no such filtering itself.
/// - `generated_paths` is the `.gitattributes`-derived set for `paths`
///   (`generated_paths::check_generated_paths_batch`), applied the same
///   way for both cache hits and misses.
/// - `progress` gets the same two phase transitions
///   (`AnalysisPhase::ReadingFiles` then `AnalysisPhase::BuildingDependencyIndex`)
///   `build_resolver` itself used to drive directly, now driven from
///   inside this function instead since both the read and the parse step
///   live here. `report_file_progress` calls during each phase report
///   `(done, total)` against the number of cache *misses* (the only files
///   actually read/parsed this run), not `paths.len()` — a
///   cache-hit-heavy run therefore reports little to no progress, which
///   reflects the real work done.
///
/// The cache is loaded and saved internally; both are best-effort (see
/// this module's doc comment) and never turn into an `Err` from this
/// function. The only propagated errors are genuine failures to read the
/// repository itself: `crate::git::commands::list_blob_oids` and
/// `crate::git::cat_file_batch::read_git_show_files_batch`.
pub(crate) fn resolve_indexed_files(
    cwd: Option<&std::path::Path>,
    head: &str,
    paths: Vec<String>,
    language_for_path: impl Fn(&str) -> Option<&'static dyn LanguageSupport> + Sync,
    generated_paths: &HashSet<String>,
    include_generated: bool,
    progress: &dyn crate::progress::AnalysisProgress,
) -> anyhow::Result<Vec<(String, Vec<IndexEntry>)>> {
    let git_dir = match crate::git::commands::resolve_git_dir(cwd) {
        Ok(dir) => Some(dir),
        Err(err) => {
            log::debug!(
                "deps cache: failed to resolve git directory, caching disabled for this run: {err}"
            );
            None
        }
    };
    let mut cache = git_dir
        .as_deref()
        .map(DepsCache::load)
        .unwrap_or_else(DepsCache::empty);

    let blob_oids = crate::git::commands::list_blob_oids(cwd, head)?;
    let candidate_paths: HashSet<String> = paths.iter().cloned().collect();

    let mut file_entries: HashMap<String, Vec<IndexEntry>> = HashMap::new();
    let mut miss_paths: Vec<String> = Vec::new();
    for path in &paths {
        let hit = blob_oids.get(path).and_then(|oid| cache.get(path, oid));
        match hit {
            Some(cached) => {
                let entries = if should_exclude_as_generated(
                    path,
                    cached.is_generated,
                    generated_paths,
                    include_generated,
                ) {
                    Vec::new()
                } else {
                    cached.entries.clone()
                };
                file_entries.insert(path.clone(), entries);
            }
            None => miss_paths.push(path.clone()),
        }
    }

    progress.set_phase(crate::spinner::AnalysisPhase::ReadingFiles);
    let on_progress = |done: usize, total: usize| progress.report_file_progress(done, total);
    let read_files = crate::git::cat_file_batch::read_git_show_files_batch(
        cwd,
        head,
        miss_paths,
        Some(&on_progress),
    )?;

    progress.set_phase(crate::spinner::AnalysisPhase::BuildingDependencyIndex);
    let completed = AtomicUsize::new(0);
    let total = read_files.len();
    let parsed: Vec<(String, bool, Vec<IndexEntry>)> = {
        use rayon::prelude::*;
        read_files
            .into_par_iter()
            .map(|(path, content)| {
                let content_marker = rinkaku_core::deps::is_generated_content(&content);
                let entries = if should_exclude_as_generated(
                    &path,
                    content_marker,
                    generated_paths,
                    include_generated,
                ) {
                    Vec::new()
                } else {
                    extract_entries(&path, &content, &language_for_path)
                };

                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if rinkaku_core::progress::should_report_progress(done, total) {
                    on_progress(done, total);
                }

                (path, content_marker, entries)
            })
            .collect()
    };

    for (path, is_generated, entries) in parsed {
        if let Some(oid) = blob_oids.get(&path) {
            cache.insert(path.clone(), oid.clone(), is_generated, entries.clone());
        }
        file_entries.insert(path, entries);
    }

    if let Some(git_dir) = &git_dir
        && let Err(err) = cache.save(git_dir, &candidate_paths)
    {
        log::debug!("deps cache: failed to save cache: {err}");
    }

    Ok(paths
        .into_iter()
        .map(|path| {
            let entries = file_entries.remove(&path).unwrap_or_default();
            (path, entries)
        })
        .collect())
}

/// Parses `content` (already known not to be skipped as generated) into
/// `IndexEntry`s, bypassing `TagsResolver::new`'s aho-corasick prefilter
/// entirely: a cached entry must be complete regardless of which names
/// *this run's* diff happens to reference, since a future run's diff may
/// reference a name this run's prefilter would have skipped past. See
/// this module's doc comment for the accepted first-run cost tradeoff.
/// `None` from `language_for_path` (a path with no registered
/// `LanguageSupport`) yields no entries — `pipeline::build_resolver`
/// already filters `paths` to registered languages before calling
/// `resolve_indexed_files`, so this is defensive rather than an expected
/// path.
fn extract_entries(
    path: &str,
    content: &str,
    language_for_path: &impl Fn(&str) -> Option<&'static dyn LanguageSupport>,
) -> Vec<IndexEntry> {
    let Some(lang) = language_for_path(path) else {
        return Vec::new();
    };
    rinkaku_core::extract::extract_all_symbols(content, lang)
        .into_iter()
        .map(|symbol| IndexEntry {
            name: symbol.name,
            signature: symbol.signature,
            container: symbol.container,
            is_test: symbol.is_test,
        })
        .collect()
}

// Split from this file per CLAUDE.md's file-size discipline (ADR 0028):
// production logic stays under the 600-line "normal" band on its own, but
// production + this module's test coverage together would push past it.
// Same convention as `search.rs`/`annotation_markers.rs`'s
// `#[path = "..._tests.rs"]` siblings.
#[cfg(test)]
#[path = "deps_cache_tests.rs"]
mod tests;

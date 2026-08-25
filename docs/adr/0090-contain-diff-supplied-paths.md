# 0090. Refuse diff-supplied paths that leave the repository

- Status: accepted
- Date: 2026-08-25

## Context

A security audit of this fork found that a crafted diff can make rinkaku
read arbitrary files off the machine it runs on, and print their content
into the report.

The path in `diff --git a/<x> b/<y>` is taken verbatim
(`diff::extract_git_header_paths`), stored as `ChangedFile::path`, and in
stdin mode handed straight to `std::fs::read_to_string` by the
`read_file` port `main.rs` wires in (`git::file_read::read_working_tree_file`).
Nothing between those two points asks whether the path is inside the
repository. Two shapes get out:

- `diff --git a/../secret/creds.py b/../secret/creds.py` — an ordinary
  parent-directory escape.
- `diff --git a//etc/shadow b//etc/shadow` — stripping the `b/` prefix
  leaves `/etc/shadow`, an absolute path. `Path::join` then *discards*
  the base it is joined onto, so anything that resolves such a path
  against a root gets the foreign path back unchanged.

The reproduction that motivated this ADR read an out-of-tree Terraform
file and rendered `default = "PROD-DB-PASSWORD-XYZ"` into the
`## Definitions` section — from a diff that never contained that value.

This matters for stdin mode specifically, and stdin mode is the
README's primary usage: `gh pr diff 123 | rinkaku`, where the diff is
whatever an outside contributor wrote. `--base` and `--pr` read through
`git show <rev>:<path>`, and git itself refuses a path outside the
repository, so they were never exposed — but that is git's containment,
not rinkaku's, and it does not cover the mode that needs it.

`rinkaku-tui`'s source view had the same hole one layer up:
`source::resolve_source_path` documented "Report paths are always
repository-root-relative" and enforced it with a `debug_assert!` — which
is compiled out of the release build a user actually installs.

## Decision

**A path that does not stay inside the repository is refused, at three
places, and the file is never read.**

1. **`rinkaku_core::repo_path::is_repo_relative`** — one pure predicate:
   rejects the empty/whitespace path, a POSIX absolute path, a Windows
   drive or UNC path (checked by hand, since `Path::is_absolute` does not
   recognize those on a Unix host), and any `..` component under either
   separator. A leading `./` and interior `.` components are accepted;
   they are noise, not an escape.

2. **`pipeline::analyze_diff`** applies it as the first check in the
   per-file loop, before any read and before every other skip case, to
   both `path` and a rename/copy's `old_path` (the base side is read
   separately, so containing only the reported path would leave half the
   hole open). A failing entry becomes
   `SkippedFile { reason: SkipReason::OutsideRepository }`.

3. **`git::file_read::read_working_tree_file`** and
   **`rinkaku_tui::source::resolve_source_path`** refuse the same paths
   independently. These are the two `std::fs` boundaries for
   diff-supplied paths; a boundary that holds only because of a check in
   another crate is one refactor away from not holding.

**The entry is reported, not dropped.** A skip with a stated reason is
what tells a reviewer their input contained something rinkaku declined to
open. Silently omitting the file would hide exactly the case worth
seeing.

## Alternatives

- **Canonicalize and compare against the repository root.** Stronger in
  one respect — it also catches a symlink inside the repository pointing
  out of it — but it needs the filesystem, which puts it out of
  `rinkaku-core` (CLAUDE.md: no IO in core's domain logic) and makes the
  check depend on what happens to exist on disk at that moment. The
  string predicate is where the untrusted data enters; a symlink in the
  tree is a property of the repository being reviewed, which is trusted
  under this threat model (`docs/UPSTREAM.md` aside, the same assumption
  every `git diff` invocation already makes).
- **Refuse the whole run on the first escaping path.** Rejected: one
  malformed entry in a large diff should not throw away the rest of the
  review. The skip already makes the entry visible.
- **Fix it only at the `read_file` port.** Rejected: the path would still
  reach the report and the TUI, which resolve and display it.
- **Leave `--base`/`--pr` to git and fix only stdin mode.** Rejected as
  the *only* measure for the same reason as above — the containment
  should be rinkaku's own, stated once, rather than an emergent property
  of what git happens to reject.

## Consequences

- `SkipReason` gains a variant, so its Markdown label
  (`"outside the repository"`), its JSON serialization, and the TUI's
  exhaustive matches over it all gain a case. This is an output-format
  change: a JSON consumer matching on `reason` sees a value it has not
  seen before.
- A diff that legitimately names a path outside the repository — there is
  no such diff for a repository-scoped review — would now be skipped
  rather than analyzed.
- `resolve_source_path` returns `Result` instead of `PathBuf`; its one
  caller already returns `Result<String, String>` and propagates it.

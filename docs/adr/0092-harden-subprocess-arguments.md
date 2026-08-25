# 0092. Harden the arguments handed to `git` and `gh`

- Status: accepted
- Date: 2026-08-25

## Context

ADR 0090 and ADR 0091 closed the two reachable findings from the security
audit. This ADR covers the two the audit rated as hardening: neither is
exploitable today, and both are one line's worth of defense that removes
a class rather than an instance.

**Revisions and refs reach `git` as bare positional arguments.**
`run_git_diff` builds `<base>...<head>`, `run_git_fetch` takes a refspec
`gh pr view` reported, `list_blob_oids` takes a rev, `read_git_show_file`
builds `<head>:<path>`. A value starting with `-` is read by `git` as an
option, and `git`'s options include ones that run commands
(`--upload-pack`, `--output`). What makes this unreachable today is
`git`'s own ref-name rules, which forbid a leading `-`, and `--base`
being the user's own argument. Both are true, and neither is a property
of this code.

**`owner`/`repo` from a `--pr` URL are unvalidated.** `parse_pr_arg`
accepts any non-empty path segments, which then build a cache directory
(`…/repos/github.com/<owner>/<repo>`), a `gh repo clone <owner>/<repo>`
argument, and a `gh api repos/<owner>/<repo>/…` route. `..` walks the
first, a leading `-` turns the second into an option, and either one
walks the third. A `--pr` URL is pasted from chat or a browser, so it is
not reliably the user's own text.

## Decision

**`--end-of-options` immediately before every revision, ref, or object
argument** in `git diff`, `git fetch`, `git ls-tree`, `git cat-file -e`,
and `git show`. Its position is load-bearing and easy to get wrong:
everything *after* it is a revision or path, so a command's own options
must come *before* it. `git ls-tree -r --end-of-options <rev> -z
--format=…` is not an error — it silently reads `-z` and `--format` as
pathspecs and prints nothing. The invocations that pass no untrusted
argument at all (`git rev-parse --show-toplevel`, `git remote get-url
origin`) are left alone.

**`github::slug::is_valid_repo_segment` gates both segments** at the
parse boundary — `parse_pr_arg` for a `--pr` URL, `parse_github_remote`
for the `origin` a bare `--pr <number>` resolves against — rather than at
each of the three uses. A segment must be non-empty, at most 100
characters, ASCII alphanumerics plus `-`/`_`/`.`, not `.` or `..`, and
not start with `-`.

## Alternatives

- **Validate at each use site.** Rejected: three sites today, and the
  next one would not know to.
- **Mirror GitHub's exact naming rules** (39 characters for an owner,
  different character sets per segment). Rejected: the goal is to reject
  what is dangerous or absurd, not to re-implement GitHub's validation —
  which would drift, and whose failure mode is rejecting a legitimate
  repository.
- **Quote or escape the arguments instead.** Nothing here goes through a
  shell (every invocation is `Command::args`), so there is nothing to
  quote; the option/argument ambiguity is the only issue, and
  `--end-of-options` is exactly its fix.

## Consequences

- A repository whose name is not a plausible GitHub name (non-ASCII, a
  space) is now rejected from a `--pr` URL rather than passed to `gh`.
  GitHub does not permit such names, so this rejects nothing that
  previously worked.
- `--end-of-options` requires git 2.24 (November 2019).
- The ordering rule above is a trap for anyone adding an option to one of
  these invocations later: appending it after `--end-of-options` turns it
  into a pathspec, silently. Each call site carries a comment saying so.

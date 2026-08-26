# 0093. Decode git's path quoting before judging a path

- Status: accepted
- Date: 2026-08-26

## Context

A second security review pass, prompted by ADR 0090's amendment, found
that `diff::extract_git_header_paths` reads a `diff --git` header's paths
as literal text. git does not always write them that way.

Under `core.quotePath` — **on by default** — git wraps a path in double
quotes and escapes it C-style as soon as it contains a quote, a
backslash, a control character, or any non-ASCII byte:

```
diff --git "a/src/\346\227\245\346\234\254\350\252\236.rs" "b/src/\346\227\245\346\234\254\350\252\236.rs"
```

The old parser looked for a bare ` b/` to split the two paths. The quoted
form has ` "b/` instead, so the split failed, `extract_git_header_paths`
returned its `(String::new(), String::new())` fallback, and the entry
reached `analyze_diff` carrying an empty path. `is_repo_relative` refuses
the empty path, so the file was reported as:

```
## Skipped files

-  (outside the repository)
```

A file sitting squarely inside the repository, reported under a
containment refusal, with no path to say which file it was. Reproduced on
stdin, `--base`, and `--pr` (the latter two run `git diff` themselves, so
they see the same quoting), and confirmed to disappear entirely under
`core.quotePath=false`.

Two consequences, and the second is why this is an ADR rather than a
bugfix commit:

- **Every non-ASCII filename is invisible to rinkaku.** Not an edge case
  for this project — it ships a Japanese README.
- **It is a review-evasion vector.** rinkaku exists to show a reviewer
  the shape of a change. A contributor who names a file with one
  non-ASCII character keeps it out of that picture, and the reason shown
  reads like the containment machinery working rather than a parse
  failure.

## Decision

**Decode git's C-style quoting at the parse boundary, before any path is
judged or used.**

1. `rinkaku_core::git_quote::unquote_git_path` decodes the quoted form —
   named escapes (`\a \b \f \n \r \t \v \\ \"`) and three-digit octal
   byte escapes — returning the input borrowed and unchanged when it is
   not quoted. `None` when the quoted form is malformed or the decoded
   bytes are not UTF-8.
2. `git_quote::split_quoted_token` splits a leading quoted token off a
   header line by tracking its own escapes, since a quoted path may
   contain an escaped space or quote and cannot be split on whitespace.
3. `diff::parse_unified_diff` decodes both `diff --git` paths and the
   four `rename from`/`rename to`/`copy from`/`copy to` lines, which git
   quotes on the same rule.
4. An undecodable path becomes the empty string rather than the raw
   header text, so `is_repo_relative` refuses it and the entry is
   reported rather than read under a name that is not the file's.

**Decoding runs before containment, and that ordering is the security
property.** The escapes are octal *byte* values, so `"a/\056\056/etc/
passwd"` spells nothing suspicious until it decodes to `a/../etc/passwd`.
Judging the raw header text would pass it; judging the decoded path
refuses it. `ChangedFile::path` therefore carries the decoded path, and
`is_repo_relative` — unchanged — sees only that.

## Alternatives

- **Set `core.quotePath=false` on rinkaku's own `git diff` calls.**
  Rejected: it fixes nothing for stdin mode, which is the README's
  primary usage and the only mode where the diff is untrusted. It also
  would not cover control characters, which git quotes regardless of that
  setting.
- **Strip the surrounding quotes without decoding the escapes.** Rejected
  outright: the path would still not name the file, and it would put
  `\056\056` past the containment check as literal text while leaving the
  real `..` for the filesystem to resolve — strictly worse than failing.
- **Treat an undecodable path as a hard error.** Rejected for the reason
  ADR 0090 already recorded: one malformed entry in a large diff should
  not throw away the rest of the review. The empty path routes it through
  the existing skip.
- **Decode further downstream, at the `read_file` port.** Rejected: the
  path would reach the report, the graph, and the TUI undecoded, and the
  containment check in `analyze_diff` would run on text that is not the
  path.

## Consequences

- **Output changes for non-ASCII filenames, and that is the point.**
  Files previously reported as `(outside the repository)` with an empty
  path are now analyzed and appear in `## Definitions`. A JSON consumer
  that had learned to expect the empty-path skip entry sees real entries
  instead.
- A genuinely escaping quoted path is now reported under its decoded name
  (`../secret/creds.py`) rather than as an empty string, so a reviewer can
  see what was refused.
- The side prefix (`a/`, `b/`) is stripped once rather than repeatedly, so
  a file under a directory named `a` keeps its leading component. The old
  repeated strip only affected the header's a-side, which
  `parse_file_entry` discards, so nothing observable changes.
- Decoding puts real control bytes into a path for the first time — git's
  own quoting had been neutralizing them before they reached rinkaku.
  ADR 0091's escaping is extended to the error channel in the same change
  for exactly that reason; see its amendment.

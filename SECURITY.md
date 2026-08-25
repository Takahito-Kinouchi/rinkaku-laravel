# Security

## Reporting a vulnerability

Open a private security advisory on this repository —
**Security → Advisories → Report a vulnerability** — rather than a public
issue. If the finding also applies to the upstream project this fork is
based on ([`hiro-o918/rinkaku`](https://github.com/hiro-o918/rinkaku)),
please report it there too; the two share most of their code.

## What rinkaku trusts, and what it does not

rinkaku's whole job is to read a change written by someone else, so the
line matters.

**Not trusted — the diff.** `gh pr diff 123 | rinkaku` feeds in a diff
written by whoever opened that pull request. Every path, hunk header, and
line of content in it is treated as hostile input:

- Paths that leave the repository (`../…`, `/etc/…`, `C:\…`) are refused
  and reported as `skipped (outside the repository)`; the file is never
  read (ADR 0090).
- Control characters in anything rendered to a terminal are escaped as
  `\u{..}`, so a filename cannot carry an escape sequence into the
  reviewer's terminal (ADR 0091). JSON output is left to `serde_json`'s
  own escaping, so a consumer still gets the real path back.
- Malformed input — impossible hunk counts, non-numeric headers, empty
  input — fails with an error rather than a panic.

**Trusted — the repository you point it at.** rinkaku shells out to
`git`, so the repository's own configuration applies: `diff.external`,
textconv filters, and `core.fsmonitor` all run commands git decides to
run. Analyzing a clone you do not trust means trusting that clone's
`.git/config`. This is inherent to invoking `git` at all; rinkaku does
not sandbox it.

**Trusted — `gh`.** Authentication is delegated entirely to the GitHub
CLI. rinkaku never reads, stores, or forwards a token.

## Known limits

- **Deeply nested source aborts the process.** A source file nested
  around 5,000 levels deep exhausts the default 8 MB stack during
  parsing; the process aborts cleanly (Rust's stack probes, no memory
  corruption). Measured: 2,000 levels is fine, 5,000 aborts, and a 64 MB
  stack handles 10,000. Fixing this properly means moving analysis onto a
  thread with a larger stack, which conflicts with ADR 0033's rule that
  only the main thread touches the terminal, so it is recorded here
  rather than half-fixed. The effect is a crash, not a compromise, and
  no real source file is nested that deep.
- **rinkaku does not sanitize meaning.** Its output is designed to be fed
  to an LLM, and that output contains text from the change under review.
  A pull request can therefore contain instructions aimed at whatever
  model reads the report. Treat rinkaku's output as data written by the
  PR author, the same way you would treat the diff itself.

## Dependency advisories

`cargo audit` runs in CI on every change to a manifest or the lockfile,
and weekly on a schedule (`.github/workflows/security-audit.yaml`); run
it locally with `make audit`. Dependabot keeps versions moving
(`.github/dependabot.yml`).

Both exist because they cover different things: Dependabot's alerts come
from the GitHub Advisory Database, which does not carry every RustSec
advisory — unmaintained and unsound ones in particular — and one of those
is what the audit that produced this file actually found.

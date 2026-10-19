# Threat model: rinkaku-laravel

## What this project does and where untrusted input enters

rinkaku-laravel is a command-line tool (binary `rinkaku-laravel`) and terminal UI that condenses a pull request's
diff into the signatures of the changed symbols and their 1-hop dependencies. It parses changed files with
tree-sitter (Rust, Go, Python, TypeScript, PHP, Vue SFC, Svelte, HCL, Markdown) and renders Markdown, JSON, Mermaid,
a short digest, or an interactive TUI. Its output is meant to be read by a reviewer in a terminal and fed to an LLM.

It is a fork of `hiro-o918/rinkaku`; most of the code is shared. `SECURITY.md` at the repository root states the
trust boundary in full and is the authoritative statement; this file summarises it for the scanner.

Treat as attacker-controlled:

- **The diff**: every path, `diff --git` / `---` / `+++` header, rename/copy line, hunk header, and content line,
  whether piped on stdin (`gh pr diff 123 | rinkaku-laravel`) or produced by `git diff` under `--base` / `--pr`.
  This includes git's C-style quoted paths (octal escapes), which are decoded before any check (ADR 0093).
- **The working tree's contents** after checking out the change: file contents, symlinks, and file names. A path
  must never resolve outside the repository root, spelled out or via a symlink (ADR 0090 and its amendment).
- **Source text handed to tree-sitter and to the extractors**: identifiers, signatures, default arguments,
  Markdown headings, Vue/Svelte markup and `<script>` blocks.
- **Data `gh` returns** for `--pr` (PR metadata, refs, head/base SHAs) — the refs and SHAs reach `git` as
  arguments (ADR 0092).

Trusted, by design (see `SECURITY.md`): the user's own command-line arguments, the clone's `.git/config` (git runs
`diff.external`, textconv and `core.fsmonitor` itself), and `gh`'s authentication. rinkaku never reads or stores a
token.

## Components that matter most / least

Most important, in this order:

1. `rinkaku-core/src/diff.rs`, `rinkaku-core/src/git_quote.rs`, `rinkaku-core/src/repo_path.rs`: diff parsing, path
   decoding, and path containment.
2. `rinkaku/src/git/` and `rinkaku/src/github/`: every `git` / `gh` / `ghq` invocation and the arguments built from
   diff- or `gh`-supplied data (option injection, `<rev>:<path>` construction, blob reads via `cat-file --batch`).
3. `rinkaku-core/src/render/` (especially `escape.rs`) and `rinkaku-tui/src/`: anything that writes diff-derived text
   to a terminal. Control characters must be escaped as `\u{..}` (ADR 0091), error messages included.
4. `rinkaku-core/src/extract/`, `rinkaku-core/src/language/`, `rinkaku-core/src/deps.rs`, `rinkaku-core/src/graph.rs`:
   extraction and dependency expansion over hostile source — panics, unbounded memory or time.
5. `rinkaku/src/browser.rs`, `rinkaku/src/clipboard.rs`: launching external programs with diff-derived text.

Out of scope: `docs/` (including `docs/experiments/`), CI configuration under `.github/`, test fixtures and
`*_tests` modules, and flaws wholly inside tree-sitter grammars, `git`, `gh`, ratatui or crossterm themselves.

## How to exercise it

The image has every crate cached, `CARGO_NET_OFFLINE` set, the debug workspace and tests built, and the release
binary at `/src/target/release/rinkaku-laravel`.

- **Tests**: `cargo test --workspace --all-features` (the same as `make test`).
- **Stdin mode, non-interactive**: `printf '%s' "$DIFF" | /src/target/release/rinkaku-laravel --format md` (also
  `--format json`, `mermaid`, `digest`). Stdout that is not a TTY selects Markdown, so no terminal is needed.
- **`--base` mode**: create a throwaway repository with `git init`, commit a base, commit a hostile change on top,
  and run `rinkaku-laravel --base <base-commit> --format md` inside it. Symlink and path-escape cases are tested
  this way.
- **Whole-repo mode**: run `rinkaku-laravel --format md` with no diff inside a repository.
- `--pr` needs `gh` and the network and cannot run here; exercise `rinkaku/src/github/` through its unit tests and
  by calling its pure functions with crafted `gh` output.

## How you rate severity

A finding counts when content in a diff or a checked-out working tree makes rinkaku do something the user did not
ask for.

- **Critical**: arbitrary command execution, or writing files, driven by diff or working-tree content (for example
  an option injected into `git` / `gh`, or a crafted path reaching a shell).
- **High**: reading a file outside the repository root and placing its contents in the output; injecting a raw
  terminal escape sequence into the reviewer's terminal from diff-supplied text.
- **Medium**: a panic, hang, or unbounded memory/time on a crafted diff or source file (other than the known stack
  limit below); output that misattributes a change to the wrong file or symbol in a way that hides it from review.
- **Low**: hardening gaps with no demonstrated reachable path.

## How to report

- Give a reproducer that runs in this image: the exact diff bytes (or the commands that build the test repository)
  and the `rinkaku-laravel` command line, plus what was observed.
- Propose a minimal patch against `main` with a regression test following the repository's conventions (rstest,
  `should_<behavior>_when_<condition>` names, whole-value `pretty_assertions::assert_eq!`).
- Report one root cause once.

## Anything to leave alone

- **Deeply nested source aborts the process** (around 5,000 nesting levels exhausts the 8 MB main-thread stack).
  This is documented in `SECURITY.md` as a known limit; do not report it.
- **Prompt injection through the report.** Signatures, paths and identifiers from the change reach the output by
  design, so text aimed at an LLM can ride along (ADR 0095). This is documented and not a vulnerability.
- Behaviour that follows from the trusted inputs above: the user's own arguments and the clone's git configuration.
- JSON output keeps real paths (control characters are left to `serde_json`'s escaping) so consumers get the true
  path back; this is intentional.

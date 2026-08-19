# 0083. Rename the installed command and remove upstream self-update

Date: 2026-08-19

## Status

Accepted

## Context

This fork installs from source (`cargo install --git
https://github.com/Takahito-Kinouchi/rinkaku-laravel`) rather than from
a GitHub Release — the release workflow was deliberately removed
(no release pipeline exists for this fork). Two problems followed from
keeping the inherited `rinkaku` package/binary name and the inherited
`self-update` machinery unchanged:

1. **Name collision.** `cargo install` names the installed binary after
   the bin crate's package name. Since that name was still `rinkaku`,
   an install of this fork was byte-for-byte indistinguishable on
   `PATH` from an install of upstream `hiro-o918/rinkaku` — same
   command, no marker anywhere a user would see it.
2. **`self-update` silently replaces the fork's binary with upstream's.**
   `rinkaku self-update` (and the `--tui` startup background version
   check that offers the same update) is hard-coded to
   `hiro-o918/rinkaku`'s own GitHub Releases (`rinkaku/src/self_update.rs`'s
   `REPO_OWNER`/`REPO_NAME` constants). Accepting that prompt — or simply
   running `self-update` out of habit — downloads and installs the
   *upstream* release binary in place of this fork's, because the two
   share the same binary name and `self_update`'s target resolution has
   no notion of "this install came from a different source." The
   upstream binary lacks this fork's PHP/Vue/Svelte support entirely.
   This was observed in the field as "almost every PHP file skipped as
   unsupported" on a machine that had — unknowingly — self-updated back
   to upstream. There is no way to point `self-update` at this fork's
   own releases instead, because this fork does not publish any: the
   release workflow was removed on purpose, and `cargo install --git`
   is the only supported distribution channel.

Both problems compound: the collision is *why* the self-update bug is
so easy to trigger unnoticed (the binary that comes back has the exact
same name and lives at the exact same path), and the self-update bug is
*why* the collision is more than a cosmetic annoyance.

## Decision

1. **Rename the bin crate's package (and therefore the installed
   command) from `rinkaku` to `rinkaku-laravel`.** The `rinkaku/`
   directory keeps its name — only `[package] name` in
   `rinkaku/Cargo.toml` changes; no `[[bin]]` table pins an alternate
   binary name, and no `rinkaku` alias is kept alongside it. `cargo
   install` now installs `rinkaku-laravel` on `PATH`, which cannot
   collide with an `hiro-o918/rinkaku` install.
2. **Remove `self-update` and the `--tui` startup update check
   entirely** — `rinkaku/src/self_update.rs`, `rinkaku/src/update_prompt.rs`,
   the `self-update` subcommand and its `--yes`/`-y` flag, the
   `RINKAKU_UPDATE_CHECK` background version-check thread, and every
   piece of TUI state/UI it drove (`App::update_available`/
   `update_prompt_open`/`update_requested`, the `u` key, the update
   popup, and the status-line hint) — rather than repointing any of it
   at this fork. The `self_update` dependency is dropped from both the
   bin crate and the workspace `[workspace.dependencies]` table.
3. **Fork-identifying `--version`.** `rinkaku-laravel --version` now
   prints `rinkaku-laravel <semver> (fork of hiro-o918/rinkaku)`, so the
   one piece of output most likely to end up in a bug report or a "which
   version am I running" check names the fork explicitly, rather than a
   bare semver that reads identically to upstream's own `--version`.
4. **LICENSE gains a second copyright line** directly under the
   existing one, crediting this fork's modifications, without altering
   the original line.

## Alternatives considered

- **Point `self-update` at this fork's own GitHub Releases instead of
  upstream's.** Rejected: this fork deliberately has no release
  pipeline (Context) — there is nothing to point it at without also
  standing up and maintaining a release workflow, which is out of scope
  for a rebrand fix and reintroduces exactly the maintenance burden the
  removed release workflow was avoiding.
- **Keep a `rinkaku` bin alias alongside `rinkaku-laravel`.** Rejected:
  the name collision is the bug, not a compatibility nicety — keeping
  `rinkaku` installable would still let a `self-update`-shaped mistake
  (or muscle memory from an upstream install) shadow this fork's binary
  under the same familiar name.

## Consequences

- Updating this fork is now `cargo install --git
  https://github.com/Takahito-Kinouchi/rinkaku-laravel rinkaku-laravel`
  again — there is no in-place update path, matching the fork's actual
  distribution channel instead of pretending to support one it cannot
  serve.
- Existing installs of the old `rinkaku` binary (whether upstream or a
  pre-rename build of this fork) are unaffected by the rename and keep
  running; users should `cargo uninstall rinkaku` to avoid keeping a
  stale or ambiguous binary on `PATH` alongside the new
  `rinkaku-laravel`.
- Every script/workflow/doc invoking the bin crate by package name
  (`cargo build -p rinkaku`, `cargo run -p rinkaku`) or by the installed
  command name needed a matching update; `rinkaku-core`/`rinkaku-tui`
  (library crates, never installed as commands) and the `rinkaku/`
  directory name are unaffected, as is every historical ADR's and the
  GitHub Action's own reference to upstream `hiro-o918/rinkaku`.

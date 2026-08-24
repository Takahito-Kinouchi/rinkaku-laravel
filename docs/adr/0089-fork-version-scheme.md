# 0089. Version this fork as `<upstream base>+laravel.<n>`

- Status: accepted
- Date: 2026-08-24

## Context

This fork inherited its version numbers from upstream. At the fork point
(`7da6421`, upstream's release-please `chore: release main (#234)`,
2026-08-19) the binary crate was at 0.6.22 and the two library crates at
0.6.21, and nothing has changed them since — upstream's release workflows
were removed here (`e3383e6`), so no automation bumps them either.

Two problems came out of that, both found the same day:

**The numbers sit in upstream's namespace.** ADR 0088's feature shipped
without a bump, and the follow-up bumped the binary to 0.6.23 — a version
`hiro-o918/rinkaku` will itself mint sooner or later, at which point the
same number names two different binaries. A user reporting "0.6.23 does X"
would be ambiguous, and so would every answer.

**`--version` could not identify the build.** Because the feature shipped
without a bump at all, an installed binary reported the same 0.6.22 before
and after the update, which is what made "the keys don't work" take a
reproduction to diagnose rather than one command. The versioning was not
incidental to that bug report; it was half of it.

Separately, a bare fork version — however it is numbered — cannot say
*which upstream state* the fork is built on. Version granularity is too
coarse for that (a merge rarely lands exactly on an upstream release), and
this fork's README explicitly sends readers upstream for the mechanism and
docs, so the base is something a reader legitimately needs.

## Decision

**The binary crate's version is `<upstream base>+laravel.<n>`.**

- The part before `+` is the `rinkaku` version of the upstream commit this
  fork is built on — 0.6.22 today.
- `<n>` starts at 1 and increments once per fork change that ships.
- Taking upstream in raises the base to upstream's own new version and
  resets `<n>` to 1.

So `0.6.22+laravel.2` reads as "upstream 0.6.22, second fork build", and
`--version` (ADR 0083's format) prints
`rinkaku-laravel 0.6.22+laravel.2 (fork of hiro-o918/rinkaku)` without
needing a second field.

`+…` is SemVer build metadata, which Cargo accepts and passes through to
`CARGO_PKG_VERSION` (verified, not assumed). Cargo *ignores* it when
comparing versions, which costs this fork nothing: the binary is installed
with `cargo install --git`, whose update decision is made on the git
revision, not the version — and `--force` settles it either way.

**`docs/UPSTREAM.md` records the exact base commit**, updated in the same
commit that takes upstream in. The version says which upstream *release*;
that file says which upstream *commit*, which is the question a merge
between releases actually raises.

**The two library crates keep upstream's 0.6.21 and are not bumped by fork
changes.** They are not installed or reported anywhere on their own — the
binary is the only artifact a user ever sees a version of — so bumping
them would be ceremony with no reader.

**Upstream's three `CHANGELOG.md` files stay frozen and untouched.** They
are upstream's generated history, their links point at upstream's repo, and
nothing here regenerates them. Fork history lives in `docs/adr/` and in
pull requests.

## Alternatives

- **Independent SemVer restarted at 1.0.0.** Defensible — the binary is
  renamed and the fork has its own features — but it drops the upstream
  base out of the version entirely, and merely postpones the collision
  until upstream reaches 1.x.
- **CalVer (`2026.8.1`).** Collision-free forever and says when a build is
  from, but says nothing about the base and reads as a bigger break from
  upstream's line than this fork actually is.
- **Bumping the patch number in upstream's line (`0.6.23`, `0.6.24`, …),
  with the base recorded only in a doc.** What was briefly done. Rejected:
  it is precisely the collision above, and the doc is invisible at the one
  moment the question is asked (`--version` on someone else's machine).
- **A separate `--fork-base` flag or an extra `--version` line.** More
  surface for something the version string can carry on its own.

## Consequences

- Fork changes that ship touch one line in `rinkaku/Cargo.toml`; upstream
  merges touch that line plus `docs/UPSTREAM.md`.
- Nothing enforces either bump. A missed one degrades to today's situation
  (two builds reporting one version), so it belongs in review attention,
  not in tooling that does not exist here yet.
- `0.6.22+laravel.1` sorts *below* the short-lived 0.6.23 under SemVer
  comparison. Harmless in practice — `cargo install --git` resolves by
  revision — but worth knowing if a comparison is ever automated.

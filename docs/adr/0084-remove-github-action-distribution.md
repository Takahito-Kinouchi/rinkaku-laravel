# 0084. Remove the GitHub Action and release-download distribution scripts

Date: 2026-08-19

## Status

Accepted (retires the consumption paths built on ADR 0021's action; ADR
0083 already removed the sibling self-update path)

## Context

Three inherited artifacts all distribute rinkaku by downloading a
prebuilt binary from the **upstream** `hiro-o918/rinkaku` GitHub
Releases: `action.yaml` (the composite GitHub Action), `install.sh`
(the curl-install script), and, transitively, the
`rinkaku-report.yaml` workflow that dogfoods the action on this
repository's own PRs. This fork deliberately has no releases of its
own (the release workflow was removed early on), so every one of these
paths yields the upstream binary — which lacks the PHP/Vue/Svelte
support that is this fork's entire point. ADR 0083 removed the same
trap's `self-update` variant; these are the remaining doors to it.
The PR-report workflow also costs a full `cargo build --release` of
Actions time per PR, against this owner's standing preference to keep
Actions consumption minimal.

## Decision

Delete `action.yaml`, `compose_and_post_comment.sh`, `install.sh`,
`docs/action.md`, and `.github/workflows/rinkaku-report.yaml`. The
sole supported installation and update path is
`cargo install --git https://github.com/Takahito-Kinouchi/rinkaku-laravel rinkaku-laravel`
(README). A PR report, when wanted, is produced locally with
`rinkaku-laravel --pr <n>` / `--base main` instead of by CI.

Historical ADRs (0021, 0036, 0039–0041) and the experiment logs that
mention the action remain untouched — they are immutable records of
decisions made when the action existed.

## Alternatives considered

- **Repoint the action/install script at this fork's releases**:
  rejected — it would require resurrecting the release pipeline this
  fork deliberately dropped, to serve a consumption path nobody uses.
- **Keep the PR-report workflow by inlining the action's steps**:
  rejected by the owner — the per-PR build cost outweighs the report's
  value here; local runs cover the need.

## Consequences

- No automatic PR report comments on this repository's PRs.
- No `uses: Takahito-Kinouchi/rinkaku-laravel@...` consumption from
  other repositories; anyone wanting CI reports builds from source in
  their own workflow.
- The last code path that could put the upstream binary on a user's
  machine is gone.

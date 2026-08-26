# 0094. An unreadable file is a skip, not a failed run

- Status: accepted
- Date: 2026-08-26

## Context

The README leads with:

```sh
gh pr diff 123 | rinkaku
```

That did not work on any pull request that adds a file, unless the
reviewer had checked the branch out first. rinkaku reads new-side content
off the working tree rather than out of the diff, so a file the PR adds is
simply not there — `read_file` returned `NotFound`, `analyze_diff`
propagated it as `AnalyzeError::ReadFile`, and the run ended:

```
$ gh pr diff 123 | rinkaku
Error: failed to read src/brand_new.rs: No such file or directory (os error 2)
```

Not one entry of the report survived, including every file that *was*
readable. Confirmed pre-existing rather than a regression by running the
same input against a binary built from the commit before this ADR's
branch: byte-identical output.

This is the same judgement ADR 0090 already made and recorded — "one
malformed entry in a large diff should not throw away the rest of the
review", and "the entry is reported, not dropped" — applied to the one
failure mode that had been left as fatal. A containment refusal was a
skip; a missing file was not, and the missing file is far more common.

## Decision

**A `read_file` failure becomes `SkipReason::Unreadable` for that entry,
and the rest of the diff is analyzed.**

- `analyze_diff` reports the entry and moves on, exactly as it does for
  binary, deleted, generated, unsupported-language, and out-of-repository
  entries.
- `collect_referenced_names` skips the entry. It only gathers reference
  names to size the dependency index, so a name it misses costs a
  candidate, never correctness.
- `AnalyzeError::ReadFile` is removed. Nothing constructs it any more, and
  a public variant that cannot occur is worse than no variant.
- The reason covers every read failure, not just a missing file: a
  permission error and a non-UTF-8 file land in the same place. All three
  mean the same thing to a reviewer — rinkaku could not see the content —
  and the port's `io::Error` is not part of `Report`, so splitting the
  variant would carry no information that a reader could act on.

**The skip must not be quieter than the failure it replaces.** A hard
error at least said something. A report where every added file reads
`could not be read`, with no hint that checking the branch out is what
fixes it, would be worse than the crash for a first-time user. So
`notes::unreadable_files_note` reports the count on stderr and names the
remedy.

## Alternatives

- **Read new-side content out of the diff instead of off disk.** The real
  fix for the underlying limitation, and out of scope here: a hunk carries
  only changed lines plus context, not the whole file, so signature
  slicing has nothing complete to parse. Worth its own ADR if it is ever
  attempted; this decision is about not throwing the run away meanwhile.
- **Keep the hard error, and tell the user to check the branch out.**
  Rejected: the partial report is genuinely useful — in a typical PR most
  changed files already exist locally — and refusing to produce it buys
  nothing.
- **Distinguish "not in the working tree" from other IO errors with two
  variants.** Rejected as a distinction without a difference for the
  reader, and it would widen the output format twice over.
- **Drop unreadable entries silently.** Rejected for ADR 0090's reason: a
  skip with a stated reason is what tells a reviewer their input contained
  something rinkaku declined to open, and silence hides exactly the case
  worth seeing.

## Consequences

- **Output-format change.** `SkipReason` gains `Unreadable`, so its
  Markdown label (`"could not be read"`), its JSON serialization, and the
  TUI's matches over it all gain a case. A JSON consumer matching on
  `reason` sees a value it has not seen before.
- **`AnalyzeError` loses its `ReadFile` variant.** A caller matching on it
  no longer compiles; a caller that only propagates is unaffected.
- **A run that used to exit non-zero now exits zero with a report.** A
  script that treated the failure as "this PR could not be analyzed" now
  gets a report with skip entries, and must look at `skipped` to notice.
- The TUI dims an `Unreadable` row, alongside the other reasons with no
  content behind them (`row_view::has_no_readable_content`).

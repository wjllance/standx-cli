# Maker strategy Stage 2 v0 implementation evidence — 2026-07-16

## Decision

- Status: `implemented_pending_evidence`
- Live canary: not executed; the required standalone authorization text has
  not been provided as an authorization action.
- Two-hour A/B: not started; it remains gated on the renewed canary.
- Acceptance: not eligible until valid live A/B quote-hours cover both calm
  and trend markets and satisfy every roadmap threshold.

## Frozen Stage 2 arms

- Baseline config SHA-256:
  `3df955e967fa97c92557b545c6eae52b5ff27dc5fd323d5e813eb89aaa04d146`
- Candidate config SHA-256:
  `30fdd415efcc2b57f7a246f4344e9929790b95d5fd9a7a7c49f7e21b7bef891d`
- Normalizing `adaptive_spread.enabled = false` to `true` makes the files
  byte-identical. The validation-only A/B preflight passed.
- Both arms are frozen to XAG-USD, `size=0.01`, one level,
  `max_position=0.2`, no active inventory exit, and the documented account
  and loss floors.

## Deterministic and offline evidence

- The legacy schema-v1 replay output was produced three times with identical
  bytes. SHA-256 for every output:
  `05b8f11b0801c2788c2e6e19ef514e075d21aa2740901a20fc85dfb772351cd1`.
- Workspace tests passed, including 181 `standx-cli`, 154 `standx-maker`, and
  75 `standx-sdk` unit tests, integration tests, credential-free e2e tests and
  doc tests. The two credential-dependent e2e tests remained intentionally
  ignored.
- Strict workspace Clippy with warnings denied, `cargo fmt --check`, Python
  compilation, eight manifest tests, shell syntax validation and
  `git diff --check` passed.

## Candidate paper smoke

- Run ID: `stage2-candidate-paper-20260716T1419Z`
- Mode: paper; no `--live`, no webhook delivery and no production order I/O.
- Event window: `2026-07-16T14:18:51Z` to `2026-07-16T14:49:15Z`
  (`1824` seconds).
- Summary cycles: `609`, plus one explicitly recorded safety skip at cycle
  141 (`mark_mid_divergence=15.22bps > 15bps`); sequence `0..609` is complete.
- Adaptive tier coverage: tier 0 = `322` cycles, tier 1 = `267`, tier 2 =
  `20`; maximum rolling volatility was `25.04bps`.
- Anti-flicker was observed in adaptive tiers with existing quotes held and
  no cancellation (`holds=2`, `cancels=0`), including the first tier-1 hold
  sequence from cycle 46.
- Three simulated paper fills occurred. The simulated terminal inventory was
  `-0.03`; paper smoke does not assert venue flatness and never executes an
  inventory exit. Live A/B arms have separate ledger-flat and venue-flat
  switching gates.
- Lifecycle stopped normally with exit status 0. No panic, fail-safe, or
  accounting-invariant error was found.
- NDJSON SHA-256:
  `1c56c212d934d66bc50a6e778fb5df9af528b555ee48c496ce4771f11afbf782`.
- The manifest is deliberately not baseline-eligible because this smoke ran
  from an uncommitted strategy worktree. It is implementation smoke evidence,
  not a clean release or live A/B comparison window.

## Remaining production evidence

Follow `docs/19-maker-stage2-live-ab-runbook.md`: install one clean release
commit on the target Linux host, confirm all four marked webhook probes, run
the venue-minimum command canary, run the controlled-disconnect maker canary,
prove terminal `orders=[]` and `positions=[]`, and only then start the guarded
two-hour baseline/candidate service. Preserve `run_id`, config hashes, ledger
flatness, venue flatness and manifest evidence at every arm boundary.

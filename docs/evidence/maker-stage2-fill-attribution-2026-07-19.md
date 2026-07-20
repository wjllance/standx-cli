# Cross-experiment fill attribution (534 HYPE fills) — 2026-07-19

Offline analyses over ALL valid HYPE stage2 arms to date (baseline
`bbd769c5`, adaptive candidate `fb91cfd5`, wide12 candidate `658eb167`;
549 fills with book replay, 534 with full attribution). XAG arms, canary
runs, the aborted `…20260719T053439Z` arm and the unplanned arm-7 stub are
excluded. These analyses decided two strategy questions without new code
or live risk: (1) is quoting CLOSER remedy or poison, (2) is predictive
cancel worth building.

## Post-fill markout curve, extended to 900s (n=534)

| horizon | mean | median | neg% |
|---|---|---|---|
| 30s | -8.80 | -7.73 | 92% |
| 60s | -9.53 | -8.49 | 85% |
| 120s | -9.08 | -8.57 | 76% |
| 300s | -10.98 | -11.09 | 71% |
| 600s | -13.58 | -12.68 | 67% |
| 900s | -13.18 | -11.54 | 66% |

- The bleed does NOT saturate at 30-120s; it keeps growing to ~600s before
  flattening. Mean reversion shows up only in the neg% (92→66: a third of
  fills revert) — it never lifts the mean.
- Tail structure: 51% of the mo300 loss mass sits in the worst decile
  (mean -56bps, median resting age 8s — fresh quotes run over by trend
  bursts). The mean is decided by a handful of trend episodes.
- Consequence 1: "quote closer + higher turnover" is POISON — every fill's
  markout keeps decaying through the entire realistic round-trip window,
  so more fills = more bleeding, and capture (+4.9) never covers mo300
  (-11.0). Ruled out.
- Consequence 2 (revises the 2026-07-18 subset read "bleed saturates ~60s,
  no tail to cut; stage 3 stays deferred"): with the full sample there IS
  a growing tail at 300-600s, so post-fill inventory response (faster
  partial exit / stronger skew, roadmap stage 3) re-qualifies as a
  candidate lever. The stage-3 deferral argument is retracted.

## Pre-fill creep replay: predictive-cancel go/no-go (n=549)

For each passive fill, replay the opposite-side best (best_ask for our
buys, best_bid for our sells) distance to our quote over the 30s before
the fill, from cycle_summary book snapshots (~3s cadence). "Warnable" =
distance continuously ≤2bps for ≥3s before the hit.

| class | share | mo30 mean | mo300 mean |
|---|---|---|---|
| no-warning (<3s) | 74% | -9.67 | -12.53 |
| brief (3-8s) | 15% | -7.07 | -7.47 |
| sustained creep (≥8s, med 12s) | 11% | -5.40 | -5.60 |

- 74% of fills teleport: opposite best still a median +4.3bps away at the
  last cycle ≤3s before the hit. No observable approach.
- The toxic tail is concentrated in the no-warning class: 89% of the
  mo300 worst decile; the warnable classes carry only ~10% of the
  worst-decile loss mass (-294 of -2956).
- VERDICT: predictive cancel is DEAD. The fills it could see coming are
  the least toxic ones; the fills that decide the mean give no warning at
  the strategy's own 3s decision cadence (a signal below that cadence is
  unactionable by definition). Not built; no dev time spent.
- Corollary: jump-type toxicity can only be avoided by NOT BEING THERE
  (drift-gated quoting — pull the side / the book when drift is detected)
  rather than by being faster. This is the only quote-side lever left,
  and it is exactly what the stage-4 gate evaluation depends on.

## Provenance

One-off scripts (not committed): `/tmp/mo_curve_tail.py` (markout curve ×
drift buckets × worst-decile split, reusing `maker_markout_ab.py`
loaders), `/tmp/creep_analysis.py` (book-distance replay + markout by
warning class). Inputs: `/opt/standx/var/standx/stage2-*.ndjson` arms
listed by config hash above; outputs reproduced in this document.

# Stage 4 step-1: zero-code widening A/B (8 vs 12bps constant) — 2026-07-19 (docker)

Decision gate for the stage-4 v0 drift controller. The offline signal-pricing
step (see `maker-strategy-stage-2-canary-ab-2026-07-17.md`, attribution
appendix) showed the counterfactual accounting mechanically over-credits any
widening: an unconditional +4bps offset scored as well as every
drift-conditioned combo. This experiment measures that bias live with pure
config (no code change, no gate re-lock) before any controller investment:

- If live widening ≈ 0 (what the stage-2 tier data hinted), the offline
  front-run credit collapses entirely and the drift controller (impl + replay
  equivalence + canary re-lock + three-arm A/B) is cancelled; stage 4 returns
  to design reserve.
- If live widening is significantly positive, widening itself is free edge
  and is adopted into the baseline as a pure config change; only then does
  the three-arm A/B (baseline / wide / drift) become worth building. The
  drift arm must then beat BOTH the baseline and the wide arm in the same
  comparison window — beating only baseline does not count (roadmap stage-4
  dual-beat gate).

## Authorization

Recorded verbatim, provided by the release owner on 2026-07-18 (Asia/Shanghai)
in the session thread, before any live action for this exercise:

> 先跑零代码的加宽 A/B（8 vs 12bps 恒宽），再决定是否建控制器

Scope (same envelope as the completed stage-2 HYPE A/B, narrower in every
other dimension): HYPE-USD only, `size=0.1`, one level, `max_position=1.0`,
two arms differing ONLY in `spread_bps` (8.0 vs 12.0 constant,
`adaptive_spread` disabled in both, `refresh_bps=4.0` in both), alternating
via the existing docker stage2 harness (profile `ab-hype`) with wind-down arm
switching, 4h arms × 3 pairs (24h). This does not authorize active inventory
exit, automatic flatten beyond the established wind-down reduce-only
semantics, larger exposure, or another symbol. Execution environment: docker
(`deploy/docker/docker-compose.yml`, profile `ab-hype`).

## Frozen artifacts

- git SHA: `6729802737a05b64d5a0920283245288f1924874` (exp branch; analysis
  scripts/docs only, strategy source identical to `a37bf4f`).
- Baseline config `examples/maker-stage2-hype-baseline.toml` SHA-256:
  `bbd769c50318953cffff2d62213864c8cd97d3265aa210fd162c9017df460568`
  (unchanged from the stage-2 HYPE A/B).
- Wide config `examples/maker-stage2-hype-wide12.toml` SHA-256:
  `658eb167dbe95d4c1a9745d6376fffa8d598d6e275eeb20c39078f62f2bc2ac7`
  (v2, see incident below). Diff vs baseline: exactly two lines — top-level
  `spread_bps 8.0 -> 12.0` and base-tier `spread_bps 8.0 -> 12.0` (the maker
  requires base tier == top-level spread/refresh even with adaptive
  disabled, `crates/standx-maker/src/volatility.rs:288`; v1 `eb83c064…`
  changed only the top level and was rejected at candidate arm start).
- Harness guardrail relaxation (ops script, not strategy code):
  `scripts/run_maker_stage2_ab.sh`'s frozen-config diff check now accepts a
  diff in ONLY the top-level plus base-tier `spread_bps` when
  `adaptive_spread` is disabled in both arms, and additionally REJECTS any
  arm config whose base-tier spread/refresh disagrees with its top-level
  values (coherence preflight — the v1 failure mode now fails at
  `STANDX_STAGE2_VALIDATE_ONLY=1` instead of at arm start). All other bytes
  must still be identical. Local tests: baseline/wide12 pass,
  baseline/adaptive-candidate pass, incoherent tier0 rejected, spread+size
  tamper rejected, wide12/adaptive-candidate rejected.
- Resume knob (ops script): `STANDX_STAGE2_FIRST_ARM=candidate` makes the
  arm loop open on the candidate arm, used to resume after the v1 incident
  without re-running the completed baseline arm 1. Default `baseline`,
  behavior unchanged.
- Env: `/etc/standx/maker-stage2-hype-ab.env` with
  `STANDX_STAGE2_CANDIDATE_CONFIG=/opt/standx/examples/maker-stage2-hype-wide12.toml`
  (only change vs the stage-2 HYPE A/B env; backups
  `.bak-20260719`, `.bak-20260719b`) and
  `STANDX_STAGE2_FIRST_ARM=candidate`; `STANDX_STAGE2_ARM_SECONDS=14400`.

## Execution log

- 2026-07-19T01:34:10Z: container `standx-maker-stage2-ab-hype` started via
  `docker compose --profile ab-hype up -d`, image `standx-stage2-ab:latest`
  (image id `8dbaf1b5dfbb`, rebuilt from SHA above). Plan: 3 pairs × 4h arms
  = 24h, ETA end 2026-07-20T01:34Z. Venue re-checked clean before start
  (orders=[], positions=[]).
- Preflight passed: `validation ok: symbol=HYPE-USD
  baseline=bbd769c50318953cffff2d62213864c8cd97d3265aa210fd162c9017df460568
  candidate=eb83c064c1fe8b023fb520cbff65c7bd136209b47296e3c1104606d3adff3f8e`.
- First arm = baseline, run_id
  `stage2-baseline-20260719T013410Z-bbd769c50318`. Startup log verified:
  `mode: live`, `effective_spread_bps: 8.0`, two-sided quotes live, uptime
  99.5%, OpenObserve ingest ok.
- Monitoring cron `492aa559` (`12,42 * * * *`, observe-only) active for the
  full run: checks arm alternation, per-arm effective_spread, uptime, errors.
- 2026-07-19T05:34:32Z: baseline arm 1 complete — 5087 cycles, 37 fills,
  uptime 94%, PnL -0.00, cleanup ok, venue orders/positions empty, exit=0.
  run_id `stage2-baseline-20260719T013410Z-bbd769c50318` VALID.
- **Incident 2026-07-19T05:34:40Z**: candidate arm exited immediately
  (status=1): `invalid adaptive spread config: adaptive base tier must match
  base spread_bps/refresh_bps`. Root cause: wide12 v1 changed only the
  top-level spread; the maker validates base tier == top level even with
  adaptive disabled. The harness preflight did not run this check, so the
  mismatch slipped through to arm start. Harness fail-closed: CRITICAL stop,
  container exit 75, no orders ever placed by the candidate arm, venue flat
  (verified). The aborted file
  `stage2-candidate-20260719T053439Z-eb83c064c1fe.ndjson` (0 fills) is
  INVALID and excluded from analysis.
- **Fix + resume 2026-07-19T06:51:13Z** (operator: "修配置，之后直接启动
  candidate"): wide12 v2 sets base-tier spread to 12.0 (new sha
  `658eb167dbe9…`); harness diff check now normalizes the base-tier spread
  and rejects incoherent configs at preflight; `STANDX_STAGE2_FIRST_ARM=
  candidate` resumes the loop on the candidate arm. Image rebuilt
  (`6ee2148c0827`), in-image preflight ok, container recreated. Candidate
  arm 2 run_id `stage2-candidate-20260719T065113Z-658eb167dbe9`; startup log
  verified `mode: live`, `effective_spread_bps: 12.0`, two-sided quotes.
  Remaining schedule: candidate → baseline → candidate → baseline →
  candidate (5 × 4h = 20h, ETA end ~2026-07-20T02:51Z + wind-down slack),
  giving 3 baseline + 3 candidate arms in total.
- Wind-down / final venue check / NDJSON sha256: TBD.

## Results

(to be filled after completion: per-arm fill/capture/markout table, pooled
comparison vs baseline, and the resulting go/no-go decision for the drift
controller)

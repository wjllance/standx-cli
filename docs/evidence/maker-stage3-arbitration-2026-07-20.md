# Stage-3 arbitration analyses and v0 project record — 2026-07-20

## Decision

- Stage 4 (drift-aware quoting): **terminated** per the pre-registered rule.
  The zero-code widening A/B (8 vs 12bps, 3 pairs / 6 arms, all complete)
  showed live widening is significantly negative, which collapses the offline
  counterfactual credit that motivated the drift controller. No controller is
  built; the stage returns to design reserve.
- Stage 3 v0: **approved (`planned`)** by the release owner on 2026-07-20,
  scoped by the three arbitration analyses below. Implementation deliberately
  deferred ("先不要直接实现") — this record is the project definition only.
- Excluded with prejudice (judged dead this round): drawdown-triggered active
  exits (arbitration C), cancel-speed engineering (~3.6% ceiling, earlier
  attribution), spread widening and drift-conditioned quoting (widening A/B).

## Inputs

- Fill attribution dataset: **610 passive fills / 81 inventory episodes /
  16 measured taker-exit costs** across all valid HYPE-USD arms
  (2026-07-17 → 07-20), including the widening A/B arms.
- Analysis script: `scripts/maker_tail_arbitration.py` (committed with this
  record; three sections map 1:1 to the arbitrations below).
- Train/val discipline: exit-pricing sweep tuned on earlier files, validated
  on the last arms; episode-level split (45 independent market episodes) to
  avoid per-fill leakage.

## Widening A/B final verdict (decision input, 2026-07-20)

6 arms, 3 pairs, all manifests complete; the three metrics' ranges do not
overlap between treatments:

| | baseline (8bps) n=117 | constant-wide (12bps) n=52 |
|---|---|---|
| fill rate | 9.0–11.0 /h | 2.7–5.7 /h |
| mo30 | -6.0 ~ -6.7 bps | -10.4 ~ -15.0 bps |
| gross per 4h (total) | +0.403 | +0.327 |

Net per fill -1.86 vs -5.04 bps: the extra +3.3bps capture is more than
consumed by doubled toxicity. Widening is not merged into the baseline; the
production baseline remains the static 8bps config.

## Arbitration A — position at tail fills → size skew is on target

| | pos_pre (at fill) | pos_peak (+300s) | first-fill tail | ≥50% max |
|---|---|---|---|---|
| worst 10% (mo300 mean -57.7) | median 0.3 | median 0.6 | 13% | 21% |
| rest (mo300 mean -6.6) | median 0.2 | median 0.3 | 11% | 14% |

The tail's fixed script is **"first cut + adverse accumulation"**: after a
toxic fill the position doubles (0.3 → 0.6) — the strategy keeps adding in the
bleeding direction. Size skew targets exactly this amplification segment, so
the roadmap's original Stage-3 mechanism stands. Two scoping consequences:

- 13% of the tail is first-fill damage at near-zero position — outside the
  reach of any inventory mechanism. Expectation: cut the tail, lower p95
  |position|; not flip per-fill economics positive.
- 79% of tail loss mass sits below half of `max_position` — the activation
  threshold must be low. v0 freezes **threshold 0.3 × max_position, recovery
  hysteresis 0.2** (fast-up/slow-down, VolBreaker pattern), tuned only on this
  training window.

## Arbitration B — halt overlap → no stage 5-b prerequisite

0% of tail fills occurred while halted; median halted share of the +300s
post-fill window is 0%. `vol_pause` plays no role in the tail events, so the
open 5-b question (exit semantics during volatility halts) is **not** pulled
forward; it stays a scale-up prerequisite as planned.

## Arbitration C — drawdown-triggered exit pricing → dead

- Measured taker exit cost: median **3.72 bps** (16 real exits).
- Grid (drawdown D × window W × fraction f): train-window best
  D=10bps / W=300s / f=1.0 → **+52.1 bps·qty** net saving.
- Out-of-sample (episode-level split, 45 independent episodes): **all
  negative, -15.6**, 25 triggers.

Post-trigger paths have no out-of-sample predictability — the rule pays
3.72bps per exit for a coin flip. Textbook overfit exposed by the split.
Drawdown exits are excluded from v0 and this route is closed absent new
independent evidence.

## v0 mechanism note — degenerate form at current size

HYPE config `size = 0.1` equals the venue `min_order_qty`, so any size
reduction trips the roadmap's "below venue minimum → drop the level" rule:
at current scale v0 degenerates to a **binary add-side suppression**
(`|position| ≥ threshold → no quotes on the accumulating side`). This aligns
with the loss-minimization objective (the suppressed quotes are
negative-expectancy adverse adds); the cost is two-sided-uptime loss while
active, adjudicated by the pre-registered -3pp uptime gate rather than waived.
Implementation-wise this moves the existing near-`max_position` add-side
suppression boundary (`SideSuppressed` path in core) down to the threshold —
a small change on existing plumbing. The alternative (raise size to 0.2 for a
real reduction step) was rejected: it doubles per-fill bleed to buy mechanism
elegance.

The existing linear price skew (`skew_bps = 8`) was active throughout every
dataset above; its 2–5bps center shift demonstrably does not stop trend
run-throughs. v0 is its limiting form; a steeper non-linear skew is the
designated v1 fallback if A/B shows binary suppression is too blunt.

## Stage status record (roadmap template)

```text
阶段：3 v0（size skew；size=venue minimum 时退化为加仓侧压制）
状态：planned
baseline_git_sha：待实现合并后冻结
candidate_git_sha：同上（与 baseline 同 sha，仅差配置）
baseline_config_hash：HYPE 静态 baseline（8bps，size 0.1，adaptive_spread 关闭），
  实现合并时按文件哈希冻结
candidate_config_hash：baseline + size-skew 启用（threshold 0.3×max_position，
  hysteresis 0.2），实现合并时冻结
训练数据窗口：2026-07-17 → 07-20 HYPE 全部有效臂（610 passive fills / 81 episodes）；
  阈值参数仅据此选定
样本外验收窗口：实现后的 live 时间片 A/B（4h 臂，wind-down 换臂，含平静+趋势时段）
专项指标结果：待 A/B（预注册门槛：样本外 p95 |position| ≥-15% 或 ≥70%max 时间
  ≥-25%；主动退出次数与 taker 成本不高于基线；net PnL ≥ 基线 95%；uptime -3pp 内）
统一安全门槛结果：待实现后离线验证（含"skew 关闭 ≡ 原始静态策略"逐 action 等价）
live 授权范围：none（实现 + replay + canary 后按 runbook 另行记录精确授权文本）
release owner 决定：立项批准（2026-07-20）；实现暂缓，另行启动
```

## Honest limits

- The tail rests on a handful of independent trend episodes; every tail
  statistic above is a small-n estimate. The A/B gates, not these
  descriptives, decide acceptance.
- Arbitration C's per-fill simulation double-counts overlapping inventory
  inside an episode; the episode-level aggregates were used for the verdict.
- Exit costs are priced at cycle marks (~2.5s grid) with the touch half-spread
  at the trigger cycle; real fast-market slippage is worse, which only
  strengthens the "dead" verdict.
- All datasets are HYPE-USD at canary size; nothing here transfers to another
  symbol without re-measurement.

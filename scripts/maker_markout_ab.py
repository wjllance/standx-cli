#!/usr/bin/env python3
"""Pooled markout / toxicity comparison for stage2 A/B maker arms. Read-only.

Usage:
    python3 scripts/maker_markout_ab.py ARM.ndjson [ARM.ndjson ...]

Each arm file is a maker NDJSON log whose name contains "baseline" or
"candidate" (treatment is inferred from the filename). Prints per-arm
fill/capture/markout stats, pooled per-treatment aggregates, a
matched-condition cut (candidate tier-0 fills vs baseline), and candidate
fills broken down by adaptive-spread tier.

Markout convention: (mark(t+h) - mark(t_fill)) * side_sign / mark * 1e4 bps,
using cycle_summary marks (~2.45s cadence) as the mark series; the actual
horizon is the first cycle at or after t_fill + h. The runner's own
performance.markout_* fields instead measure from the fill price (i.e. they
include capture), so the two conventions differ by ~capture_bps.
"""
import bisect
import json
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from statistics import mean, median

HORIZONS = [1.0, 5.0, 30.0]


def parse_ts(s):
    return datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()


def load_arm(path):
    cycles, fills, last_perf, pnl_last = [], [], None, None
    with open(path) as f:
        for line in f:
            try:
                d = json.loads(line)
            except Exception:
                continue
            a = d.get("action")
            if a == "cycle_summary":
                try:
                    cycles.append((parse_ts(d["ts"]), float(d["mark"]),
                                   d.get("adaptive_spread_tier", 0)))
                except Exception:
                    pass
                last_perf = d.get("performance")
                pnl_last = d.get("pnl")
            elif a == "fill":
                fills.append(d)
    cycles.sort()
    return cycles, fills, last_perf, pnl_last


def arm_rows(cycles, fills):
    """Per passive fill: (cap_bps, mo1, mo5, mo30, tier, side, qty)."""
    ts_list = [c[0] for c in cycles]
    rows = []
    for f in fills:
        if f.get("role") != "passive_maker":
            continue
        t = parse_ts(f["ts"])
        price = float(f["price"])
        sign = 1.0 if f["side"] == "buy" else -1.0
        i = bisect.bisect_left(ts_list, t)
        if i >= len(cycles):
            continue
        mark0 = cycles[i][1]
        tier = cycles[max(0, i - 1)][2]
        cap = (mark0 - price) * sign / price * 1e4
        mo = {}
        for h in HORIZONS:
            j = bisect.bisect_left(ts_list, t + h)
            mo[h] = (cycles[j][1] - mark0) * sign / mark0 * 1e4 if j < len(cycles) else None
        rows.append((cap, mo[1.0], mo[5.0], mo[30.0], tier, f["side"], float(f["qty"])))
    return rows


def stat(xs):
    xs = [x for x in xs if x is not None]
    if not xs:
        return "n/a"
    neg = sum(1 for x in xs if x < 0) / len(xs) * 100
    return f"mean{mean(xs):+6.2f} med{median(xs):+6.2f} neg%{neg:3.0f}"


def main(paths):
    arm_summary = []
    pooled = defaultdict(list)
    tier0_only = defaultdict(list)
    for path in paths:
        base = path.rsplit("/", 1)[-1]
        treatment = "baseline" if "baseline" in base else "candidate" if "candidate" in base else None
        if treatment is None:
            print(f"skip (no treatment in name): {base}")
            continue
        name = base.split(".")[0]
        cycles, fills, last_perf, pnl_last = load_arm(path)
        if not cycles:
            print(f"skip (no cycles): {base}")
            continue
        ts_list = [c[0] for c in cycles]
        dur_h = (ts_list[-1] - ts_list[0]) / 3600
        marks = [c[1] for c in cycles]
        tiers = Counter(c[2] for c in cycles)
        rows = arm_rows(cycles, fills)
        pooled[treatment].extend(rows)
        tier0_only[treatment].extend(r for r in rows if r[4] == 0)
        mo5 = [r[2] for r in rows if r[2] is not None]
        mo30 = [r[3] for r in rows if r[3] is not None]
        caps = [r[0] for r in rows]
        arm_summary.append(dict(
            name=name, treatment=treatment, dur=dur_h, n=len(rows),
            rate=len(rows) / dur_h, cap=mean(caps) if caps else None,
            mo5=mean(mo5) if mo5 else None, mo30=mean(mo30) if mo30 else None,
            pnl=pnl_last if isinstance(pnl_last, (int, float)) else None,
            mark0=marks[0], mark1=marks[-1], mlo=min(marks), mhi=max(marks),
            tiers=dict(tiers), gross=(last_perf or {}).get("gross_spread_quote"),
            inv=(last_perf or {}).get("inventory_avg_abs_qty")))

    print("=== per-arm ===")
    for a in arm_summary:
        tier_s = f"  tiers {a['tiers']}" if a["treatment"] == "candidate" else ""
        pnl_s = f"{a['pnl']:+7.3f}" if a["pnl"] is not None else "    n/a"
        print(f"{a['name']}: {a['dur']:.1f}h fills {a['n']:3d} ({a['rate']:4.1f}/h) "
              f"cap{a['cap']:+5.2f} mo5{a['mo5']:+6.2f} mo30{a['mo30']:+6.2f} "
              f"pnl{pnl_s} gross{a['gross']:+5.3f} inv{a['inv']:.2f} "
              f"mark {a['mark0']:.2f}->{a['mark1']:.2f} [{a['mlo']:.2f},{a['mhi']:.2f}]{tier_s}")

    print("\n=== pooled by treatment (all passive fills) ===")
    for t in ("baseline", "candidate"):
        rows = pooled.get(t, [])
        if not rows:
            continue
        print(f"{t:9s}: n{len(rows):3d}  cap {stat([r[0] for r in rows])}")
        for i, h in enumerate(HORIZONS):
            print(f"{'':9s}  mo{h:>4.0f}s {stat([r[1 + i] for r in rows])}")
        arms = [a for a in arm_summary if a["treatment"] == t and a["pnl"] is not None]
        if arms:
            print(f"{'':9s}  sum pnl {sum(a['pnl'] for a in arms):+.3f}  "
                  f"sum gross_spread {sum(a['gross'] for a in arms if a['gross']):+.3f}")

    if pooled.get("baseline") and tier0_only.get("candidate"):
        print("\n=== matched-condition: candidate tier0 fills vs all baseline fills ===")
        for label, rows in (("baseline(all)", pooled["baseline"]),
                            ("candidate(t0)", tier0_only["candidate"])):
            mo5 = [r[2] for r in rows if r[2] is not None]
            mo30 = [r[3] for r in rows if r[3] is not None]
            print(f"{label:14s}: n{len(rows):3d} cap{mean([r[0] for r in rows]):+5.2f} "
                  f"mo5{mean(mo5):+6.2f} mo30{mean(mo30):+6.2f}")

    if pooled.get("candidate"):
        print("\n=== candidate fills by tier ===")
        by_t = defaultdict(list)
        for r in pooled["candidate"]:
            by_t[r[4]].append(r)
        for t in sorted(by_t):
            rows = by_t[t]
            mo5 = [r[2] for r in rows if r[2] is not None]
            mo30 = [r[3] for r in rows if r[3] is not None]
            mo5_s = f"{mean(mo5):+6.2f}" if mo5 else "   n/a"
            mo30_s = f"{mean(mo30):+6.2f}" if mo30 else "   n/a"
            print(f"tier{t}: n{len(rows):3d} cap{mean([r[0] for r in rows]):+5.2f} "
                  f"mo5{mo5_s} mo30{mo30_s}")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    main(sys.argv[1:])
